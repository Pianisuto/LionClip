use adw::prelude::*;
use gtk::pango;

use crate::history::TextHistoryItem;

/// Visible lines before the preview is cut off.
const PREVIEW_LINES: i32 = 3;
/// Character budget for a preview, so a huge clipboard item never turns into a
/// huge Pango layout.
const PREVIEW_CHARS: usize = 180;
/// Keeps a single long word or URL from widening the popup.
const PREVIEW_WIDTH_CHARS: i32 = 42;
const WHITESPACE_PREVIEW: &str = "(whitespace)";

/// A result row plus the action buttons keyboard navigation moves through.
pub(super) struct RowWidgets {
    pub(super) row: gtk::ListBoxRow,
    pub(super) actions: [gtk::Button; 2],
}

pub(super) fn build(
    item: &TextHistoryItem,
    on_toggle_pin: impl Fn() + 'static,
    on_delete: impl Fn() + 'static,
) -> RowWidgets {
    let preview = gtk::Label::builder()
        .label(preview_text(item.text()))
        .ellipsize(pango::EllipsizeMode::End)
        .lines(PREVIEW_LINES)
        .max_width_chars(PREVIEW_WIDTH_CHARS)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();

    // A toggle carries the pinned state itself: Adwaita draws it checked, so a
    // pinned item is recognisable without reading the tooltip.
    let pin_label = if item.is_pinned() {
        "Unpin item"
    } else {
        "Pin item"
    };
    let pin = gtk::ToggleButton::builder()
        .icon_name("view-pin-symbolic")
        .tooltip_text(pin_label)
        .active(item.is_pinned())
        .valign(gtk::Align::Center)
        .build();
    pin.add_css_class("flat");
    pin.add_css_class("circular");
    pin.update_property(&[gtk::accessible::Property::Label(pin_label)]);
    // Connected after the initial state, so restoring it never re-enters here.
    pin.connect_toggled(move |_| on_toggle_pin());

    let delete = action_button("user-trash-symbolic", "Delete item");
    delete.connect_clicked(move |_| on_delete());

    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(2)
        .valign(gtk::Align::Center)
        .build();
    actions.add_css_class("lionclip-actions");
    if item.is_pinned() {
        actions.add_css_class("pinned");
    }
    actions.append(&pin);
    actions.append(&delete);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(8)
        .build();
    content.append(&preview);
    content.append(&actions);

    let row = gtk::ListBoxRow::builder()
        .activatable(true)
        .selectable(true)
        .child(&content)
        .build();

    RowWidgets {
        row,
        actions: [pin.upcast(), delete],
    }
}

fn action_button(icon_name: &str, label: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon_name)
        .tooltip_text(label)
        .valign(gtk::Align::Center)
        .build();
    button.add_css_class("flat");
    button.add_css_class("circular");
    button.update_property(&[gtk::accessible::Property::Label(label)]);
    button
}

/// Builds a compact, layout-safe preview of clipboard text.
///
/// The stored item keeps the exact original content; only this display copy is
/// trimmed, so restoring an item is always lossless.
fn preview_text(text: &str) -> String {
    let mut preview = String::new();
    let mut rendered_lines = 0;
    let mut budget = PREVIEW_CHARS;
    let mut truncated = false;

    for line in text.lines() {
        if rendered_lines == 0 && line.trim().is_empty() {
            continue;
        }
        if rendered_lines == PREVIEW_LINES {
            truncated = true;
            break;
        }
        if rendered_lines > 0 {
            preview.push('\n');
        }

        for character in line.trim_end().chars() {
            if budget == 0 {
                truncated = true;
                break;
            }
            preview.push(if character.is_control() {
                ' '
            } else {
                character
            });
            budget -= 1;
        }

        rendered_lines += 1;
        if truncated {
            break;
        }
    }

    if truncated {
        preview.push('…');
    }
    if preview.trim().is_empty() {
        return WHITESPACE_PREVIEW.into();
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_text_is_previewed_unchanged() {
        assert_eq!(preview_text("copied value"), "copied value");
    }

    #[test]
    fn trailing_whitespace_and_windows_line_endings_are_normalized() {
        assert_eq!(preview_text("first  \r\nsecond\r\n"), "first\nsecond");
    }

    #[test]
    fn leading_blank_lines_are_skipped() {
        assert_eq!(preview_text("\n\n  \nvisible"), "visible");
    }

    #[test]
    fn tabs_and_control_characters_become_spaces() {
        assert_eq!(preview_text("a\tb\u{7}c"), "a b c");
    }

    #[test]
    fn preview_is_capped_at_three_lines() {
        assert_eq!(preview_text("one\ntwo\nthree"), "one\ntwo\nthree");
        assert_eq!(preview_text("one\ntwo\nthree\nfour"), "one\ntwo\nthree…");
    }

    #[test]
    fn very_long_unbroken_text_is_truncated() {
        let preview = preview_text(&"x".repeat(PREVIEW_CHARS * 4));

        assert_eq!(preview.chars().count(), PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn unicode_content_is_preserved_in_previews() {
        assert_eq!(preview_text("Olá 🦁 mundo"), "Olá 🦁 mundo");
    }

    #[test]
    fn whitespace_only_content_shows_a_marker() {
        assert_eq!(preview_text("   \n\t\n"), WHITESPACE_PREVIEW);
        assert_eq!(preview_text(""), WHITESPACE_PREVIEW);
    }
}
