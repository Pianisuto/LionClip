#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HistoryItemId(u64);

impl HistoryItemId {
    pub(super) fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextHistoryItem {
    id: HistoryItemId,
    text: String,
}

impl TextHistoryItem {
    pub(super) fn new(id: HistoryItemId, text: String) -> Self {
        Self { id, text }
    }

    pub fn id(&self) -> HistoryItemId {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}
