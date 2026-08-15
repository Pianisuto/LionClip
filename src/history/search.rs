use super::TextHistoryItem;

/// Case-insensitive substring query over already-loaded history items.
///
/// The query layer never touches SQLite and never mutates the items it reads,
/// so the popup can re-run it on every keystroke against the in-memory model.
/// Query text is treated like clipboard content: it is never logged.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HistoryQuery {
    needle: String,
}

impl HistoryQuery {
    /// Builds a query from raw search-field text.
    ///
    /// Surrounding whitespace is ignored so a trailing space typed between
    /// words does not empty the result list.
    pub fn new(raw: &str) -> Self {
        Self {
            needle: raw.trim().to_lowercase(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.needle.is_empty()
    }

    pub fn matches(&self, item: &TextHistoryItem) -> bool {
        self.is_empty() || contains_ignoring_case(item.text(), &self.needle)
    }

    /// Returns the matching items in the order they were given, so history
    /// ordering (pinned first, then recency) is preserved while filtering.
    pub fn filter<'items>(&self, items: &'items [TextHistoryItem]) -> Vec<&'items TextHistoryItem> {
        items.iter().filter(|item| self.matches(item)).collect()
    }
}

fn contains_ignoring_case(haystack: &str, lowercase_needle: &str) -> bool {
    if lowercase_needle.is_empty() {
        return true;
    }

    // The common case avoids allocating a lowercase copy of clipboard text,
    // which matters because large items are filtered on every keystroke.
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
    use super::{super::HistoryItemId, *};

    fn items(texts: &[&str]) -> Vec<TextHistoryItem> {
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
            .map(TextHistoryItem::text)
            .collect()
    }

    #[test]
    fn empty_query_matches_every_item_in_order() {
        let items = items(&["first", "second"]);

        assert!(HistoryQuery::new("   ").is_empty());
        assert_eq!(matching(&items, ""), ["first", "second"]);
        assert_eq!(matching(&items, "   "), ["first", "second"]);
    }

    #[test]
    fn matching_is_case_insensitive_in_both_directions() {
        let items = items(&["Hello World", "lowercase"]);

        assert_eq!(matching(&items, "hello"), ["Hello World"]);
        assert_eq!(matching(&items, "LOWERCASE"), ["lowercase"]);
    }

    #[test]
    fn matching_uses_substrings_anywhere_in_the_item() {
        let items = items(&["https://example.com/path?query=1", "unrelated"]);

        assert_eq!(
            matching(&items, "example.com"),
            ["https://example.com/path?query=1"]
        );
        assert_eq!(
            matching(&items, "?QUERY="),
            ["https://example.com/path?query=1"]
        );
    }

    #[test]
    fn matching_spans_multiline_and_unicode_content() {
        let items = items(&["fn main() {\n    println!(\"olá\");\n}", "Ünicode Ítem"]);

        assert_eq!(matching(&items, "PRINTLN").len(), 1);
        assert_eq!(matching(&items, "OLÁ").len(), 1);
        assert_eq!(matching(&items, "ünicode ítem"), ["Ünicode Ítem"]);
        assert_eq!(matching(&items, "\n    println").len(), 1);
    }

    #[test]
    fn no_matches_returns_an_empty_result() {
        let items = items(&["first", "second"]);

        assert!(matching(&items, "third").is_empty());
    }

    #[test]
    fn filtering_never_alters_the_source_items() {
        let items = items(&["Keep  ME\n exact\t", "other"]);
        let before = items.clone();

        let _ = HistoryQuery::new("keep").filter(&items);

        assert_eq!(items, before);
    }

    #[test]
    fn queries_longer_than_the_item_do_not_match() {
        let items = items(&["ab"]);

        assert!(matching(&items, "abcdef").is_empty());
    }
}
