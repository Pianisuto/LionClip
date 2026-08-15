#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HistoryItemId(i64);

impl HistoryItemId {
    pub(super) fn new(value: i64) -> Self {
        Self(value)
    }

    pub(super) fn value(self) -> i64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextHistoryItem {
    id: HistoryItemId,
    text: String,
    created_sequence: i64,
    last_used_sequence: i64,
    pinned: bool,
}

impl TextHistoryItem {
    pub(super) fn new(
        id: HistoryItemId,
        text: String,
        created_sequence: i64,
        last_used_sequence: i64,
        pinned: bool,
    ) -> Self {
        Self {
            id,
            text,
            created_sequence,
            last_used_sequence,
            pinned,
        }
    }

    pub fn id(&self) -> HistoryItemId {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn created_sequence(&self) -> i64 {
        self.created_sequence
    }

    pub(super) fn last_used_sequence(&self) -> i64 {
        self.last_used_sequence
    }

    pub(super) fn set_last_used_sequence(&mut self, sequence: i64) {
        self.last_used_sequence = sequence;
    }

    pub(super) fn is_pinned(&self) -> bool {
        self.pinned
    }
}
