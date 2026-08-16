use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use crate::storage::StoragePaths;

use super::{HistoryItemId, HistoryQuery, HistoryUpdate, TextHistory, TextHistoryItem};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestStorage {
    directory: PathBuf,
    paths: StoragePaths,
}

impl TestStorage {
    fn new(test_name: &str) -> Self {
        let suffix = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "lionclip-history-regression-{test_name}-{}-{suffix}",
            std::process::id()
        ));
        let paths = StoragePaths::for_root(directory.clone());
        Self { directory, paths }
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn texts(history: &TextHistory) -> Vec<&str> {
    history
        .items()
        .iter()
        .filter_map(TextHistoryItem::as_text)
        .collect()
}

#[test]
fn inserts_first_text_item() {
    let mut history = TextHistory::default();
    assert_eq!(history.record("first".into()), HistoryUpdate::Inserted);
    assert_eq!(texts(&history), ["first"]);
}

#[test]
fn newest_text_items_are_first() {
    let mut history = TextHistory::default();
    history.record("first".into());
    history.record("second".into());
    history.record("third".into());
    assert_eq!(texts(&history), ["third", "second", "first"]);
}

#[test]
fn duplicate_at_front_is_unchanged_and_keeps_id() {
    let mut history = TextHistory::default();
    history.record("same".into());
    let id = history.items()[0].id();
    assert_eq!(history.record("same".into()), HistoryUpdate::Unchanged);
    assert_eq!(history.items()[0].id(), id);
    assert_eq!(history.items().len(), 1);
}

#[test]
fn recopy_moves_existing_text_and_keeps_logical_id() {
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
fn exact_text_content_still_defines_equality() {
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
    assert_eq!(history.items()[0].as_text(), Some(exact));
}

#[test]
fn default_unpinned_history_remains_bounded_to_500_total_items() {
    let mut history = TextHistory::default();
    for index in 0..=500 {
        history.record(format!("item {index}"));
    }
    assert_eq!(history.items().len(), 500);
    assert_eq!(history.items()[0].as_text(), Some("item 500"));
    assert_eq!(history.items()[499].as_text(), Some("item 1"));
}

#[test]
fn pinned_items_are_first_without_losing_recency_inside_group() {
    let mut history = TextHistory::default();
    history.record("first".into());
    let first_id = history.items()[0].id();
    history.record("second".into());
    let second_id = history.items()[0].id();
    history.record("third".into());

    assert!(history.pin(first_id).changed());
    assert_eq!(texts(&history), ["first", "third", "second"]);
    assert!(history.pin(second_id).changed());
    assert_eq!(texts(&history), ["second", "first", "third"]);
}

#[test]
fn unknown_or_redundant_history_actions_are_rejected_without_mutation() {
    let mut history = TextHistory::default();
    history.record("only".into());
    let id = history.items()[0].id();
    assert!(history.pin(id).changed());
    let before = history.items().to_vec();

    assert!(!history.pin(id).changed());
    assert!(!history.unpin(HistoryItemId::new(99_999)).changed());
    assert!(!history.delete(HistoryItemId::new(99_999)).changed());
    assert_eq!(history.items(), before);
}

#[test]
fn unpinning_returns_text_to_recency_order() {
    let mut history = TextHistory::default();
    history.record("first".into());
    let first_id = history.items()[0].id();
    history.record("second".into());
    history.pin(first_id);
    assert_eq!(texts(&history), ["first", "second"]);
    assert!(history.unpin(first_id).changed());
    assert_eq!(texts(&history), ["second", "first"]);
}

#[test]
fn recopying_pinned_text_refreshes_only_its_group_recency() {
    let mut history = TextHistory::default();
    history.record("pinned".into());
    let pinned_id = history.items()[0].id();
    history.pin(pinned_id);
    history.record("older".into());
    history.record("newer".into());

    assert_eq!(history.record("older".into()), HistoryUpdate::MovedToFront);
    assert_eq!(texts(&history), ["pinned", "older", "newer"]);
    assert_eq!(history.record("pinned".into()), HistoryUpdate::MovedToFront);
    assert_eq!(history.items()[0].id(), pinned_id);
    assert_eq!(history.record("pinned".into()), HistoryUpdate::Unchanged);
}

#[test]
fn deleting_removes_only_requested_text_item() {
    let mut history = TextHistory::default();
    history.record("first".into());
    history.record("second".into());
    let second_id = history.items()[0].id();
    assert!(history.delete(second_id).changed());
    assert_eq!(texts(&history), ["first"]);
    assert!(history.item(second_id).is_none());
    assert!(!history.delete(second_id).changed());
}

#[test]
fn clear_unpinned_keeps_pinned_text() {
    let mut history = TextHistory::default();
    history.record("keep".into());
    let keep_id = history.items()[0].id();
    history.pin(keep_id);
    history.record("drop one".into());
    history.record("drop two".into());
    assert!(history.clear_unpinned().changed());
    assert_eq!(texts(&history), ["keep"]);
    assert!(!history.has_unpinned());
    assert!(!history.clear_unpinned().changed());
}

#[test]
fn search_does_not_mutate_source_history() {
    let mut history = TextHistory::default();
    history.record("Alpha".into());
    history.record("beta".into());
    let before = history.items().to_vec();
    let matches = history.search(&HistoryQuery::new("ALPHA"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].as_text(), Some("Alpha"));
    assert_eq!(history.items(), before);
}

#[test]
fn persistence_survives_restart_with_order_dedup_and_stable_ids() {
    let storage = TestStorage::new("restart");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    history.record("A".into());
    let a_id = history.items()[0].id();
    history.record("B".into());
    history.record("C".into());
    history.record("A".into());
    drop(history);

    let mut reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened), ["A", "C", "B"]);
    assert_eq!(reopened.items()[0].id(), a_id);
    reopened.record("D".into());
    assert_ne!(reopened.items()[0].id(), a_id);
    drop(reopened);

    let reopened_again = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened_again), ["D", "A", "C", "B"]);
}

#[test]
fn exact_text_survives_worker_shutdown_and_restart() {
    let storage = TestStorage::new("exact-restart");
    let fixtures = [
        " leading",
        "trailing ",
        "blank\n\nlines",
        "tabs\tinside",
        "Unicode: Olá 🦁",
        "trailing newline\n",
    ];
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    for fixture in fixtures {
        history.record(fixture.into());
    }
    drop(history);

    let reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(
        texts(&reopened),
        fixtures.into_iter().rev().collect::<Vec<_>>()
    );
}

#[test]
fn repository_retention_remains_500_after_restart() {
    let storage = TestStorage::new("retention-500");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    for index in 0..=500 {
        history.record(format!("item {index}"));
    }
    drop(history);

    let reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(reopened.items().len(), 500);
    assert_eq!(reopened.items()[0].as_text(), Some("item 500"));
    assert!(
        reopened
            .items()
            .iter()
            .all(|item| item.as_text() != Some("item 0"))
    );
}

#[test]
fn pin_and_unpin_survive_restart() {
    let storage = TestStorage::new("pin-restart");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    history.record("pin me".into());
    let pinned_id = history.items()[0].id();
    history.record("other".into());
    history.pin(pinned_id);
    drop(history);

    let mut reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened), ["pin me", "other"]);
    assert!(reopened.items()[0].is_pinned());
    assert_eq!(reopened.items()[0].id(), pinned_id);
    reopened.unpin(pinned_id);
    drop(reopened);

    let reopened_again = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened_again), ["other", "pin me"]);
    assert!(reopened_again.items().iter().all(|item| !item.is_pinned()));
}

#[test]
fn pinned_text_survives_retention_across_restart() {
    let storage = TestStorage::new("pin-retention");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    history.record("pinned".into());
    let pinned_id = history.items()[0].id();
    history.pin(pinned_id);
    for index in 0..505 {
        history.record(format!("item {index}"));
    }
    assert_eq!(history.items().len(), 501);
    drop(history);

    let reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(reopened.items().len(), 501);
    assert!(reopened.items()[0].is_pinned());
    assert_eq!(reopened.items()[0].id(), pinned_id);
}

#[test]
fn deletion_is_persisted() {
    let storage = TestStorage::new("delete-restart");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    history.record("keep".into());
    history.record("remove".into());
    let removed_id = history.items()[0].id();
    history.delete(removed_id);
    drop(history);

    let reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened), ["keep"]);
}

#[test]
fn clearing_unpinned_history_is_persisted() {
    let storage = TestStorage::new("clear-restart");
    let mut history = TextHistory::persistent(storage.paths.clone()).unwrap();
    history.record("pinned".into());
    let pinned_id = history.items()[0].id();
    history.pin(pinned_id);
    history.record("drop one".into());
    history.record("drop two".into());
    history.clear_unpinned();
    drop(history);

    let reopened = TextHistory::persistent(storage.paths.clone()).unwrap();
    assert_eq!(texts(&reopened), ["pinned"]);
    assert!(reopened.items()[0].is_pinned());
}
