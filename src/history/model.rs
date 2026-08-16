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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageMime {
    Png,
    Jpeg,
}

impl ImageMime {
    pub const SUPPORTED: [&'static str; 2] = ["image/png", "image/jpeg"];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
        }
    }

    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "image/png" => Some(Self::Png),
            "image/jpeg" => Some(Self::Jpeg),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImageData {
    content_hash: String,
    mime_type: ImageMime,
    byte_length: u64,
    width: u32,
    height: u32,
}

impl ImageData {
    pub fn new(
        content_hash: String,
        mime_type: ImageMime,
        byte_length: u64,
        width: u32,
        height: u32,
    ) -> Self {
        Self {
            content_hash,
            mime_type,
            byte_length,
            width,
            height,
        }
    }

    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    pub fn mime_type(&self) -> ImageMime {
        self.mime_type
    }

    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HistoryPayload {
    Text(String),
    Image(ImageData),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryItem {
    id: HistoryItemId,
    payload: HistoryPayload,
    created_sequence: i64,
    last_used_sequence: i64,
    pinned: bool,
}

/// Compatibility alias kept so the Phase 1-3 test helpers and callers do not
/// need to churn merely because the item model gained another typed payload.
pub type TextHistoryItem = HistoryItem;

impl HistoryItem {
    pub(super) fn new(
        id: HistoryItemId,
        text: String,
        created_sequence: i64,
        last_used_sequence: i64,
        pinned: bool,
    ) -> Self {
        Self::new_text(id, text, created_sequence, last_used_sequence, pinned)
    }

    pub(super) fn new_text(
        id: HistoryItemId,
        text: String,
        created_sequence: i64,
        last_used_sequence: i64,
        pinned: bool,
    ) -> Self {
        Self {
            id,
            payload: HistoryPayload::Text(text),
            created_sequence,
            last_used_sequence,
            pinned,
        }
    }

    pub(super) fn new_image(
        id: HistoryItemId,
        image: ImageData,
        created_sequence: i64,
        last_used_sequence: i64,
        pinned: bool,
    ) -> Self {
        Self {
            id,
            payload: HistoryPayload::Image(image),
            created_sequence,
            last_used_sequence,
            pinned,
        }
    }

    pub fn id(&self) -> HistoryItemId {
        self.id
    }

    pub fn payload(&self) -> &HistoryPayload {
        &self.payload
    }

    pub fn as_text(&self) -> Option<&str> {
        match &self.payload {
            HistoryPayload::Text(text) => Some(text),
            HistoryPayload::Image(_) => None,
        }
    }

    pub fn image(&self) -> Option<&ImageData> {
        match &self.payload {
            HistoryPayload::Text(_) => None,
            HistoryPayload::Image(image) => Some(image),
        }
    }

    /// Phase 1-3 compatibility accessor. New code that can receive image items
    /// should prefer [`Self::as_text`].
    pub fn text(&self) -> &str {
        self.as_text().unwrap_or("")
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
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

    pub(super) fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }
}
