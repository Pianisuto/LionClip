use std::path::PathBuf;

use super::{
    HistoryItemId, HistoryQuery, TextHistoryItem,
    repository::{PersistenceError, PersistenceMutation, PersistenceWorker},
};

const DEFAULT_UNPINNED_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryUpdate {
    Inserted,
    MovedToFront,
    Unchanged,
}

impl HistoryUpdate {
    pub fn changed(self) -> bool {
        !matches!(self, Self::Unchanged)
    }
}

/// Result of an explicit history operation requested by the user.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryChange {
    /// The history changed and the persisted mutation was submitted.
    Applied,
    /// Nothing changed: the identifier is unknown or already in that state.
    Rejected,
}

impl HistoryChange {
    pub fn changed(self) -> bool {
        matches!(self, Self::Applied)
    }
}

/// In-memory source of truth for text clipboard history.
///
/// Ordering is deterministic: pinned items come first, then unpinned items,
/// and both groups are ordered by last-used sequence descending. Pinning
/// therefore moves an item into the pinned group without changing its own
/// recency, and re-copying an item refreshes its recency inside its group.
pub struct TextHistory {
    items: Vec<TextHistoryItem>,
    next_id: Option<i64>,
    next_sequence: Option<i64>,
    unpinned_limit: usize,
    persistence: Option<PersistenceWorker>,
}

impl Default for TextHistory {
    fn default() -> Self {
        Self::from_items(Vec::new(), DEFAULT_UNPINNED_LIMIT, None)
    }
}

impl TextHistory {
    pub(crate) fn persistent(path: PathBuf) -> Result<Self, PersistenceError> {
        Self::persistent_with_limit(path, DEFAULT_UNPINNED_LIMIT)
    }

    fn persistent_with_limit(
        path: PathBuf,
        unpinned_limit: usize,
    ) -> Result<Self, PersistenceError> {
        let (persistence, items) = PersistenceWorker::open(path)?;
        Ok(Self::from_items(items, unpinned_limit, Some(persistence)))
    }

    fn from_items(
        mut items: Vec<TextHistoryItem>,
        unpinned_limit: usize,
        persistence: Option<PersistenceWorker>,
    ) -> Self {
        sort_items(&mut items);
        let next_id = next_value(items.iter().map(|item| item.id().value()));
        let next_sequence = next_value(items.iter().map(TextHistoryItem::last_used_sequence));

        let mut history = Self {
            items,
            next_id,
            next_sequence,
            unpinned_limit,
            persistence,
        };
        let removed_ids = history.enforce_retention();
        if !removed_ids.is_empty()
            && let Some(persistence) = &history.persistence
        {
            persistence.submit(PersistenceMutation::Delete { removed_ids });
        }
        history
    }

    pub fn record(&mut self, text: String) -> HistoryUpdate {
        if let Some(index) = self.items.iter().position(|item| item.text() == text) {
            if self.items[index].last_used_sequence() == self.newest_sequence() {
                return HistoryUpdate::Unchanged;
            }

            let Some(sequence) = take_next(&mut self.next_sequence) else {
                eprintln!("lionclip: history update rejected reason=sequence-exhausted");
                return HistoryUpdate::Unchanged;
            };
            self.items[index].set_last_used_sequence(sequence);
            let item = self.items[index].clone();
            sort_items(&mut self.items);
            self.persist(PersistenceMutation::Upsert {
                item,
                removed_ids: Vec::new(),
            });
            return HistoryUpdate::MovedToFront;
        }

        let (Some(id), Some(sequence)) = (
            take_next(&mut self.next_id),
            take_next(&mut self.next_sequence),
        ) else {
            eprintln!("lionclip: history update rejected reason=identifier-exhausted");
            return HistoryUpdate::Unchanged;
        };
        let item = TextHistoryItem::new(HistoryItemId::new(id), text, sequence, sequence, false);
        self.items.push(item.clone());
        sort_items(&mut self.items);
        let removed_ids = self.enforce_retention();
        self.persist(PersistenceMutation::Upsert { item, removed_ids });
        HistoryUpdate::Inserted
    }

    pub fn items(&self) -> &[TextHistoryItem] {
        &self.items
    }

    pub fn item(&self, id: HistoryItemId) -> Option<&TextHistoryItem> {
        self.items.iter().find(|item| item.id() == id)
    }

    /// Filters the loaded history without touching the database.
    pub fn search(&self, query: &HistoryQuery) -> Vec<&TextHistoryItem> {
        query.filter(&self.items)
    }

    /// Exempts an item from the unpinned retention limit and moves it into the
    /// pinned group. Its own recency is left untouched.
    pub fn pin(&mut self, id: HistoryItemId) -> HistoryChange {
        self.set_pinned(id, true)
    }

    /// Returns an item to the unpinned group, where it is subject to retention
    /// again and may be dropped immediately if the limit is already reached.
    pub fn unpin(&mut self, id: HistoryItemId) -> HistoryChange {
        self.set_pinned(id, false)
    }

    pub fn delete(&mut self, id: HistoryItemId) -> HistoryChange {
        let Some(index) = self.items.iter().position(|item| item.id() == id) else {
            return HistoryChange::Rejected;
        };

        self.items.remove(index);
        self.persist(PersistenceMutation::Delete {
            removed_ids: vec![id],
        });
        HistoryChange::Applied
    }

    /// Removes every unpinned item. Pinned items are always kept.
    pub fn clear_unpinned(&mut self) -> HistoryChange {
        if !self.has_unpinned() {
            return HistoryChange::Rejected;
        }

        self.items.retain(TextHistoryItem::is_pinned);
        self.persist(PersistenceMutation::ClearUnpinned);
        HistoryChange::Applied
    }

    pub fn has_unpinned(&self) -> bool {
        self.items.iter().any(|item| !item.is_pinned())
    }

    pub(crate) fn shutdown_persistence(&mut self) {
        self.persistence.take();
    }

    fn set_pinned(&mut self, id: HistoryItemId, pinned: bool) -> HistoryChange {
        let Some(index) = self.items.iter().position(|item| item.id() == id) else {
            return HistoryChange::Rejected;
        };
        if self.items[index].is_pinned() == pinned {
            return HistoryChange::Rejected;
        }

        self.items[index].set_pinned(pinned);
        let item = self.items[index].clone();
        sort_items(&mut self.items);
        // Unpinning can push the history back over the retention limit, and the
        // item that was just unpinned may itself be the oldest one.
        let removed_ids = self.enforce_retention();
        self.persist(PersistenceMutation::Upsert { item, removed_ids });
        HistoryChange::Applied
    }

    fn newest_sequence(&self) -> i64 {
        self.items
            .iter()
            .map(TextHistoryItem::last_used_sequence)
            .max()
            .unwrap_or(i64::MIN)
    }

    fn enforce_retention(&mut self) -> Vec<HistoryItemId> {
        let mut unpinned_seen = 0;
        let mut remove_indices = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.is_pinned() {
                continue;
            }
            unpinned_seen += 1;
            if unpinned_seen > self.unpinned_limit {
                remove_indices.push(index);
            }
        }

        remove_indices
            .into_iter()
            .rev()
            .map(|index| self.items.remove(index).id())
            .collect()
    }

    fn persist(&self, mutation: PersistenceMutation) {
        if let Some(persistence) = &self.persistence {
            persistence.submit(mutation);
        }
    }
}

fn sort_items(items: &mut [TextHistoryItem]) {
    items.sort_by_key(|item| {
        (
            std::cmp::Reverse(item.is_pinned()),
            std::cmp::Reverse(item.last_used_sequence()),
            std::cmp::Reverse(item.id().value()),
        )
    });
}

fn next_value(values: impl Iterator<Item = i64>) -> Option<i64> {
    match values.max() {
        Some(value) => value.checked_add(1),
        None => Some(0),
    }
}

fn take_next(next: &mut Option<i64>) -> Option<i64> {
    let current = next.take()?;
    *next = current.checked_add(1);
    Some(current)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::*;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    struct TestDatabase {
        directory: PathBuf,
        path: PathBuf,
    }

    impl TestDatabase {
        fn new(test_name: &str) -> Self {
            let suffix = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let directory = std::env::temp_dir().join(format!(
                "lionclip-history-{test_name}-{}-{suffix}",
                std::process::id()
            ));
            let path = directory.join("lionclip.db");
            Self { directory, path }
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn texts(history: &TextHistory) -> Vec<&str> {
        history.items().iter().map(TextHistoryItem::text).collect()
    }

    fn item(id: i64, text: &str, sequence: i64, pinned: bool) -> TextHistoryItem {
        TextHistoryItem::new(
            HistoryItemId::new(id),
            text.into(),
            sequence,
            sequence,
            pinned,
        )
    }

    #[test]
    fn inserts_first_item() {
        let mut history = TextHistory::default();

        assert_eq!(history.record("first".into()), HistoryUpdate::Inserted);
        assert_eq!(texts(&history), ["first"]);
    }

    #[test]
    fn newest_items_are_first() {
        let mut history = TextHistory::default();

        history.record("first".into());
        history.record("second".into());
        history.record("third".into());

        assert_eq!(texts(&history), ["third", "second", "first"]);
    }

    #[test]
    fn duplicate_at_front_does_not_change_history() {
        let mut history = TextHistory::default();
        history.record("same".into());
        let original_id = history.items()[0].id();

        assert_eq!(history.record("same".into()), HistoryUpdate::Unchanged);
        assert_eq!(texts(&history), ["same"]);
        assert_eq!(history.items()[0].id(), original_id);
    }

    #[test]
    fn recopy_moves_existing_logical_item_to_front() {
        let mut history = TextHistory::default();
        history.record("first".into());
        let first_id = history.items()[0].id();
        history.record("second".into());
        history.record("third".into());

        assert_eq!(history.record("first".into()), HistoryUpdate::MovedToFront);
        assert_eq!(texts(&history), ["first", "third", "second"]);
        assert_eq!(history.items()[0].id(), first_id);
    }

    #[test]
    fn exact_content_defines_equality() {
        let mut history = TextHistory::default();

        for text in ["abc", " abc", "abc ", "abc\n", "abc\r\n"] {
            history.record(text.into());
        }

        assert_eq!(texts(&history), ["abc\r\n", "abc\n", "abc ", " abc", "abc"]);
    }

    #[test]
    fn multiline_and_surrounding_whitespace_are_preserved() {
        let mut history = TextHistory::default();
        let exact = "  leading\n\tmiddle\ntrailing  \n";

        history.record(exact.into());

        assert_eq!(history.items()[0].text(), exact);
    }

    #[test]
    fn in_memory_history_is_bounded() {
        let mut history = TextHistory::default();

        for index in 0..=DEFAULT_UNPINNED_LIMIT {
            history.record(format!("item {index}"));
        }

        assert_eq!(history.items().len(), DEFAULT_UNPINNED_LIMIT);
        assert_eq!(history.items()[0].text(), "item 500");
        assert_eq!(history.items()[DEFAULT_UNPINNED_LIMIT - 1].text(), "item 1");
    }

    #[test]
    fn retention_removes_multiple_oldest_unpinned_items() {
        let history = TextHistory::from_items(
            vec![
                item(0, "oldest", 0, false),
                item(1, "older", 1, false),
                item(2, "newer", 2, false),
                item(3, "newest", 3, false),
            ],
            2,
            None,
        );

        assert_eq!(texts(&history), ["newest", "newer"]);
    }

    #[test]
    fn pinned_items_survive_retention_without_counting_toward_limit() {
        let history = TextHistory::from_items(
            vec![
                item(0, "oldest pinned", 0, true),
                item(1, "old unpinned", 1, false),
                item(2, "new pinned", 2, true),
                item(3, "new unpinned", 3, false),
            ],
            1,
            None,
        );

        assert_eq!(
            texts(&history),
            ["new pinned", "oldest pinned", "new unpinned"]
        );
    }

    #[test]
    fn pinned_items_are_ordered_before_unpinned_items_by_recency() {
        let mut history = TextHistory::default();
        history.record("first".into());
        let first_id = history.items()[0].id();
        history.record("second".into());
        let second_id = history.items()[0].id();
        history.record("third".into());

        assert_eq!(history.pin(first_id), HistoryChange::Applied);
        assert_eq!(texts(&history), ["first", "third", "second"]);

        assert_eq!(history.pin(second_id), HistoryChange::Applied);
        assert_eq!(texts(&history), ["second", "first", "third"]);
    }

    #[test]
    fn pinning_an_already_pinned_or_unknown_item_changes_nothing() {
        let mut history = TextHistory::default();
        history.record("only".into());
        let id = history.items()[0].id();
        history.pin(id);
        let before = history.items().to_vec();

        assert_eq!(history.pin(id), HistoryChange::Rejected);
        assert_eq!(
            history.unpin(HistoryItemId::new(9_999)),
            HistoryChange::Rejected
        );
        assert_eq!(
            history.delete(HistoryItemId::new(9_999)),
            HistoryChange::Rejected
        );
        assert_eq!(history.items(), before);
    }

    #[test]
    fn unpinning_returns_the_item_to_recency_order() {
        let mut history = TextHistory::default();
        history.record("first".into());
        let first_id = history.items()[0].id();
        history.record("second".into());
        history.pin(first_id);
        assert_eq!(texts(&history), ["first", "second"]);

        assert_eq!(history.unpin(first_id), HistoryChange::Applied);
        assert_eq!(texts(&history), ["second", "first"]);
    }

    #[test]
    fn recopying_keeps_an_item_in_its_group_and_refreshes_recency() {
        let mut history = TextHistory::default();
        history.record("pinned".into());
        let pinned_id = history.items()[0].id();
        history.pin(pinned_id);
        history.record("older".into());
        history.record("newer".into());
        assert_eq!(texts(&history), ["pinned", "newer", "older"]);

        assert_eq!(history.record("older".into()), HistoryUpdate::MovedToFront);
        assert_eq!(texts(&history), ["pinned", "older", "newer"]);

        assert_eq!(history.record("pinned".into()), HistoryUpdate::MovedToFront);
        assert_eq!(texts(&history), ["pinned", "older", "newer"]);
        assert_eq!(history.items()[0].id(), pinned_id);
        assert_eq!(history.record("pinned".into()), HistoryUpdate::Unchanged);
    }

    #[test]
    fn unpinning_can_drop_the_item_when_retention_is_already_full() {
        let mut history = TextHistory::from_items(
            vec![item(0, "pinned", 0, true), item(1, "unpinned", 1, false)],
            1,
            None,
        );

        assert_eq!(history.unpin(HistoryItemId::new(0)), HistoryChange::Applied);
        assert_eq!(texts(&history), ["unpinned"]);
    }

    #[test]
    fn deleting_removes_only_the_requested_item() {
        let mut history = TextHistory::default();
        history.record("first".into());
        history.record("second".into());
        let second_id = history.items()[0].id();

        assert_eq!(history.delete(second_id), HistoryChange::Applied);
        assert_eq!(texts(&history), ["first"]);
        assert_eq!(history.item(second_id), None);
        assert_eq!(history.delete(second_id), HistoryChange::Rejected);
    }

    #[test]
    fn clearing_removes_unpinned_items_and_keeps_pinned_items() {
        let mut history = TextHistory::default();
        history.record("keep".into());
        let keep_id = history.items()[0].id();
        history.pin(keep_id);
        history.record("drop one".into());
        history.record("drop two".into());

        assert_eq!(history.clear_unpinned(), HistoryChange::Applied);
        assert_eq!(texts(&history), ["keep"]);
        assert!(!history.has_unpinned());
        assert_eq!(history.clear_unpinned(), HistoryChange::Rejected);
    }

    #[test]
    fn search_filters_the_loaded_history_without_changing_it() {
        let mut history = TextHistory::default();
        history.record("Alpha".into());
        history.record("beta".into());
        let before = history.items().to_vec();

        let matches = history.search(&HistoryQuery::new("ALPHA"));

        assert_eq!(
            matches
                .into_iter()
                .map(TextHistoryItem::text)
                .collect::<Vec<_>>(),
            ["Alpha"]
        );
        assert!(history.search(&HistoryQuery::new("")).len() == 2);
        assert_eq!(history.items(), before);
    }

    #[test]
    fn persistence_survives_restart_with_order_dedup_and_stable_ids() {
        let database = TestDatabase::new("restart");
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        history.record("A".into());
        let a_id = history.items()[0].id();
        history.record("B".into());
        history.record("C".into());
        history.record("A".into());
        assert_eq!(texts(&history), ["A", "C", "B"]);
        drop(history);

        let mut reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened), ["A", "C", "B"]);
        assert_eq!(reopened.items()[0].id(), a_id);
        reopened.record("D".into());
        assert_ne!(reopened.items()[0].id(), a_id);
        drop(reopened);

        let reopened_again = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened_again), ["D", "A", "C", "B"]);
    }

    #[test]
    fn exact_text_survives_worker_shutdown_and_restart() {
        let database = TestDatabase::new("exact-restart");
        let fixtures = [
            " leading",
            "trailing ",
            "blank\n\nlines",
            "tabs\tinside",
            "Unicode: Olá 🦁",
            "trailing newline\n",
        ];
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        for fixture in fixtures {
            history.record(fixture.into());
        }
        drop(history);

        let reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(
            texts(&reopened),
            fixtures.into_iter().rev().collect::<Vec<_>>()
        );
    }

    #[test]
    fn repository_retention_enforces_500_and_removes_oldest_at_501() {
        let database = TestDatabase::new("retention-500");
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        for index in 0..DEFAULT_UNPINNED_LIMIT {
            history.record(format!("item {index}"));
        }
        drop(history);

        let mut reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(reopened.items().len(), DEFAULT_UNPINNED_LIMIT);
        assert_eq!(
            reopened.items().last().map(TextHistoryItem::text),
            Some("item 0")
        );
        reopened.record("item 500".into());
        drop(reopened);

        let reopened_again = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(reopened_again.items().len(), DEFAULT_UNPINNED_LIMIT);
        assert_eq!(reopened_again.items()[0].text(), "item 500");
        assert!(
            reopened_again
                .items()
                .iter()
                .all(|item| item.text() != "item 0")
        );
    }

    #[test]
    fn startup_retention_removes_overflow_but_preserves_pinned_item_in_database() {
        let database = TestDatabase::new("startup-retention");
        let (worker, persisted_items) = PersistenceWorker::open(database.path.clone()).unwrap();
        assert!(persisted_items.is_empty());
        for index in 0..5 {
            worker.submit(PersistenceMutation::Upsert {
                item: item(index, &format!("item {index}"), index, index == 0),
                removed_ids: Vec::new(),
            });
        }
        drop(worker);

        let history = TextHistory::persistent_with_limit(database.path.clone(), 2).unwrap();
        assert_eq!(texts(&history), ["item 0", "item 4", "item 3"]);
        drop(history);

        let reopened = TextHistory::persistent_with_limit(database.path.clone(), 2).unwrap();
        assert_eq!(texts(&reopened), ["item 0", "item 4", "item 3"]);
    }

    #[test]
    fn pin_and_unpin_survive_restart() {
        let database = TestDatabase::new("pin-restart");
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        history.record("pin me".into());
        let pinned_id = history.items()[0].id();
        history.record("other".into());
        history.pin(pinned_id);
        drop(history);

        let mut reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened), ["pin me", "other"]);
        assert!(reopened.items()[0].is_pinned());
        assert_eq!(reopened.items()[0].id(), pinned_id);
        reopened.unpin(pinned_id);
        drop(reopened);

        let reopened_again = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened_again), ["other", "pin me"]);
        assert!(reopened_again.items().iter().all(|item| !item.is_pinned()));
    }

    #[test]
    fn pinned_items_are_exempt_from_retention_across_restart() {
        let database = TestDatabase::new("pin-retention");
        let mut history = TextHistory::persistent_with_limit(database.path.clone(), 2).unwrap();
        history.record("pinned".into());
        let pinned_id = history.items()[0].id();
        history.pin(pinned_id);
        for index in 0..5 {
            history.record(format!("item {index}"));
        }
        assert_eq!(texts(&history), ["pinned", "item 4", "item 3"]);
        drop(history);

        let reopened = TextHistory::persistent_with_limit(database.path.clone(), 2).unwrap();
        assert_eq!(texts(&reopened), ["pinned", "item 4", "item 3"]);
    }

    #[test]
    fn deletion_is_persisted() {
        let database = TestDatabase::new("delete-restart");
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        history.record("keep".into());
        history.record("remove".into());
        let removed_id = history.items()[0].id();
        assert_eq!(history.delete(removed_id), HistoryChange::Applied);
        drop(history);

        let reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened), ["keep"]);
    }

    #[test]
    fn clearing_unpinned_history_is_persisted() {
        let database = TestDatabase::new("clear-restart");
        let mut history = TextHistory::persistent(database.path.clone()).unwrap();
        history.record("pinned".into());
        let pinned_id = history.items()[0].id();
        history.pin(pinned_id);
        for index in 0..3 {
            history.record(format!("item {index}"));
        }
        assert_eq!(history.clear_unpinned(), HistoryChange::Applied);
        assert_eq!(texts(&history), ["pinned"]);
        drop(history);

        let mut reopened = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened), ["pinned"]);
        reopened.record("after clear".into());
        drop(reopened);

        let reopened_again = TextHistory::persistent(database.path.clone()).unwrap();
        assert_eq!(texts(&reopened_again), ["pinned", "after clear"]);
    }
}
