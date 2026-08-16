use super::TextHistoryItem;

/// Case-insensitive substring query over already-loaded history items.
///
/// Empty search shows every supported payload. A non-empty textual query only
/// matches text items; images deliberately have no synthetic searchable text
/// because Phase 4 does not add OCR, filenames, hashes or metadata search.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HistoryQuery {
    needle: String,
}

impl HistoryQuery {
    pub fn new(raw: &str) -> Self {
        Self {
            needle: raw.trim().to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    pub fn matches(&self, item: &TextHistoryItem) -> bool {
        if self.is_empty() {
            return true;
        }
        item.as_text()
            .is_some_and(|text| contains_ignoring_case(text, &self.needle))
    }

    pub fn filter<'items>(&self, items: &'items [TextHistoryItem]) -> Vec<&'items TextHistoryItem> {
        items.iter().filter(|item| self.matches(item)).collect()
    }
}

fn contains_ignoring_case(haystack: &str, lowercase_needle: &str) -> bool {
    if lowercase_needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && lowercase_needle.is_ascii() {
        let needle = lowercase_needle.as_bytes();
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }
    haystack.to_lowercase().contains(lowercase_needle)
}

#[cfg(test)]
mod tests {
    use super::{
        super::{HistoryItemId, ImageData, ImageMime},
        *,
    };

    fn text_items(texts: &[&str]) -> Vec<TextHistoryItem> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| {
                let sequence = i64::try_from(index).expect("test index fits in i64");
                TextHistoryItem::new(
                    HistoryItemId::new(sequence),
                    (*text).into(),
                    sequence,
                    sequence,
                    false,
                )
            })
            .collect()
    }

    fn matching<'a>(items: &'a [TextHistoryItem], raw: &str) -> Vec<&'a str> {
        HistoryQuery::new(raw)
            .filter(items)
            .into_iter()
            .filter_map(TextHistoryItem::as_text)
            .collect()
    }

    #[test]
    fn empty_query_matches_every_item_in_order() {
        let mut items = text_items(&["first", "second"]);
        items.push(TextHistoryItem::new_image(
            HistoryItemId::new(9),
            ImageData::new("a".repeat(64), ImageMime::Png, 100, 10, 10),
            9,
            9,
            false,
        ));
        assert_eq!(HistoryQuery::new("").filter(&items).len(), 3);
    }

    #[test]
    fn matching_is_case_insensitive_and_uses_substrings() {
        let items = text_items(&["Hello World", "https://example.com/path"]);
        assert_eq!(matching(&items, "hello"), ["Hello World"]);
        assert_eq!(
            matching(&items, "EXAMPLE.COM"),
            ["https://example.com/path"]
        );
    }

    #[test]
    fn non_empty_query_does_not_match_image_metadata() {
        let image = TextHistoryItem::new_image(
            HistoryItemId::new(1),
            ImageData::new("a".repeat(64), ImageMime::Png, 100, 1920, 1080),
            1,
            1,
            false,
        );
        assert!(!HistoryQuery::new("png").matches(&image));
        assert!(!HistoryQuery::new("1920").matches(&image));
    }

    #[test]
    fn multiline_unicode_and_source_integrity_are_preserved() {
        let items = text_items(&["fn main() {\n    println!(\"olá\");\n}", "Ünicode Ítem"]);
        let before = items.clone();
        assert_eq!(matching(&items, "OLÁ").len(), 1);
        assert_eq!(matching(&items, "ünicode ítem"), ["Ünicode Ítem"]);
        assert_eq!(items, before);
    }
}
