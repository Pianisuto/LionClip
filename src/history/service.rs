use std::path::PathBuf;

use super::{
    HistoryItemId, TextHistoryItem,
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
        items.sort_by_key(|item| {
            (
                std::cmp::Reverse(item.last_used_sequence()),
                std::cmp::Reverse(item.id().value()),
            )
        });
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
            if index == 0 {
                return HistoryUpdate::Unchanged;
            }

            let Some(sequence) = take_next(&mut self.next_sequence) else {
                eprintln!("lionclip: history update rejected reason=sequence-exhausted");
                return HistoryUpdate::Unchanged;
            };
            let mut item = self.items.remove(index);
            item.set_last_used_sequence(sequence);
            self.items.insert(0, item.clone());
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
        self.items.insert(0, item.clone());
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

    pub(crate) fn shutdown_persistence(&mut self) {
        self.persistence.take();
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
            ["new unpinned", "new pinned", "oldest pinned"]
        );
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
        assert_eq!(texts(&history), ["item 4", "item 3", "item 0"]);
        drop(history);

        let reopened = TextHistory::persistent_with_limit(database.path.clone(), 2).unwrap();
        assert_eq!(texts(&reopened), ["item 4", "item 3", "item 0"]);
    }
}
