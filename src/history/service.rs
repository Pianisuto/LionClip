use super::{HistoryItemId, TextHistoryItem};

const MAX_IN_MEMORY_ITEMS: usize = 500;

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

#[derive(Debug, Default)]
pub struct TextHistory {
    items: Vec<TextHistoryItem>,
    next_id: u64,
}

impl TextHistory {
    pub fn record(&mut self, text: String) -> HistoryUpdate {
        if let Some(index) = self.items.iter().position(|item| item.text() == text) {
            if index == 0 {
                return HistoryUpdate::Unchanged;
            }

            let item = self.items.remove(index);
            self.items.insert(0, item);
            return HistoryUpdate::MovedToFront;
        }

        let id = HistoryItemId::new(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        self.items.insert(0, TextHistoryItem::new(id, text));
        self.items.truncate(MAX_IN_MEMORY_ITEMS);
        HistoryUpdate::Inserted
    }

    pub fn items(&self) -> &[TextHistoryItem] {
        &self.items
    }

    pub fn item(&self, id: HistoryItemId) -> Option<&TextHistoryItem> {
        self.items.iter().find(|item| item.id() == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(history: &TextHistory) -> Vec<&str> {
        history.items().iter().map(TextHistoryItem::text).collect()
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

        history.record("line one\n  line two\n".into());
        history.record("line one\n line two\n".into());

        assert_eq!(
            texts(&history),
            ["line one\n line two\n", "line one\n  line two\n"]
        );
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

        for index in 0..=MAX_IN_MEMORY_ITEMS {
            history.record(format!("item {index}"));
        }

        assert_eq!(history.items().len(), MAX_IN_MEMORY_ITEMS);
        assert_eq!(history.items()[0].text(), "item 500");
        assert_eq!(history.items()[MAX_IN_MEMORY_ITEMS - 1].text(), "item 1");
    }
}
