use adw::prelude::*;
use gtk::pango;

use crate::{
    history::{ImageData, TextHistoryItem},
    image_store, storage,
};

const PREVIEW_LINES: i32 = 3;
const PREVIEW_CHARS: usize = 180;
const PREVIEW_WIDTH_CHARS: i32 = 42;
const WHITESPACE_PREVIEW: &str = "(whitespace)";
const IMAGE_PREVIEW_WIDTH: i32 = 132;
const IMAGE_PREVIEW_HEIGHT: i32 = 76;

pub(super) struct RowWidgets {
    pub(super) row: gtk::ListBoxRow,
    pub(super) actions: [gtk::Button; 2],
}

pub(super) fn build(
    item: &TextHistoryItem,
    on_toggle_pin: impl Fn() + 'static,
    on_delete: impl Fn() + 'static,
) -> RowWidgets {
    let body: gtk::Widget = if let Some(text) = item.as_text() {
        text_preview(text).upcast()
    } else if let Some(image) = item.image() {
        image_preview(image).upcast()
    } else {
        gtk::Label::new(Some("Unsupported clipboard item")).upcast()
    };

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
        .spacing(8)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(8)
        .build();
    content.append(&body);
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

fn text_preview(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(preview_text(text))
        .ellipsize(pango::EllipsizeMode::End)
        .lines(PREVIEW_LINES)
        .max_width_chars(PREVIEW_WIDTH_CHARS)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build()
}

fn image_preview(image: &ImageData) -> gtk::Box {
    let visual = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .width_request(IMAGE_PREVIEW_WIDTH)
        .height_request(IMAGE_PREVIEW_HEIGHT)
        .valign(gtk::Align::Center)
        .halign(gtk::Align::Start)
        .build();
    visual.add_css_class("card");
    visual.set_overflow(gtk::Overflow::Hidden);

    let thumbnail = storage::paths()
        .ok()
        .and_then(|paths| image_store::thumbnail_path(&paths, image));
    if let Some(path) = thumbnail.filter(|path| path.is_file()) {
        // This file is a LionClip-generated, bounded PNG thumbnail rather than
        // an original clipboard image, so the row never decodes full-size data.
        let picture = gtk::Picture::new();
        picture.set_filename(Some(path));
        picture.set_content_fit(gtk::ContentFit::Contain);
        picture.set_can_shrink(true);
        picture.set_size_request(IMAGE_PREVIEW_WIDTH, IMAGE_PREVIEW_HEIGHT);
        picture.set_alternative_text(Some("Clipboard image thumbnail"));
        visual.append(&picture);
    } else {
        let placeholder = gtk::Image::from_icon_name("image-x-generic-symbolic");
        placeholder.set_pixel_size(32);
        placeholder.set_valign(gtk::Align::Center);
        placeholder.set_vexpand(true);
        visual.append(&placeholder);
    }

    let metadata = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let dimensions = gtk::Label::builder()
        .label(format!("{}×{}", image.width(), image.height()))
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    let format = gtk::Label::builder()
        .label(image.mime_type().label())
        .xalign(0.0)
        .build();
    format.add_css_class("dim-label");
    metadata.append(&dimensions);
    metadata.append(&format);

    let container = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(10)
        .hexpand(true)
        .build();
    container.append(&visual);
    container.append(&metadata);
    container
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
