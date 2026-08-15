use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{gdk, glib, pango};

use crate::history::{HistoryItemId, TextHistoryItem};

pub struct HistoryPopup {
    pub window: adw::ApplicationWindow,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    empty_state: gtk::Label,
    row_ids: Rc<RefCell<Vec<HistoryItemId>>>,
}

pub fn build(
    application: &adw::Application,
    on_item_activated: impl Fn(HistoryItemId) + 'static,
) -> HistoryPopup {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("LionClip")
        .default_width(430)
        .default_height(420)
        .decorated(false)
        .resizable(false)
        .build();

    let title = gtk::Label::builder()
        .label("Clipboard history")
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("title-2");

    let hint = gtk::Label::builder()
        .label("↑/↓ Navigate  •  Enter Restore  •  Esc Close")
        .halign(gtk::Align::Start)
        .build();
    hint.add_css_class("dim-label");

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .margin_top(20)
        .margin_start(20)
        .margin_end(20)
        .build();
    header.append(&title);
    header.append(&hint);

    let list = gtk::ListBox::builder()
        .activate_on_single_click(true)
        .selection_mode(gtk::SelectionMode::Single)
        .build();
    list.add_css_class("boxed-list");

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .vexpand(true)
        .child(&list)
        .build();

    let empty_state = gtk::Label::builder()
        .label("Copy some text to start your history")
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .wrap(true)
        .build();
    empty_state.add_css_class("dim-label");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(14)
        .margin_bottom(16)
        .margin_start(16)
        .margin_end(16)
        .vexpand(true)
        .build();
    body.append(&scrolled);
    body.append(&empty_state);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(0)
        .build();
    content.append(&header);
    content.append(&body);
    window.set_content(Some(&content));

    let row_ids = Rc::new(RefCell::new(Vec::new()));
    let activate_item: Rc<dyn Fn(HistoryItemId)> = Rc::new(on_item_activated);

    list.connect_row_activated({
        let row_ids = row_ids.clone();
        let activate_item = activate_item.clone();
        let window = window.clone();

        move |_, row| {
            if let Some(id) = row_id_at(&row_ids, row.index()) {
                activate_item(id);
                window.set_visible(false);
            }
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let list = list.clone();
        let row_ids = row_ids.clone();
        let activate_item = activate_item.clone();
        let window = window.clone();

        move |_, key, _, _| match key {
            gdk::Key::Escape => {
                window.set_visible(false);
                glib::Propagation::Stop
            }
            gdk::Key::Up => {
                move_selection(&list, -1);
                glib::Propagation::Stop
            }
            gdk::Key::Down => {
                move_selection(&list, 1);
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                if let Some(row) = list.selected_row()
                    && let Some(id) = row_id_at(&row_ids, row.index())
                {
                    activate_item(id);
                    window.set_visible(false);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    });
    window.add_controller(keys);

    window.connect_close_request(|window| {
        window.set_visible(false);
        glib::Propagation::Stop
    });

    HistoryPopup {
        window,
        list,
        scrolled,
        empty_state,
        row_ids,
    }
}

impl HistoryPopup {
    pub fn render(&self, items: &[TextHistoryItem]) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }

        let mut row_ids = self.row_ids.borrow_mut();
        row_ids.clear();

        for item in items {
            row_ids.push(item.id());
            self.list.append(&build_row(item));
        }

        let is_empty = items.is_empty();
        self.scrolled.set_visible(!is_empty);
        self.empty_state.set_visible(is_empty);

        if let Some(first_row) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first_row));
        }
    }

    pub fn present(&self) {
        self.window.present();
        if let Some(selected_row) = self.list.selected_row() {
            selected_row.grab_focus();
        }
    }
}

fn build_row(item: &TextHistoryItem) -> gtk::ListBoxRow {
    let preview = gtk::Label::builder()
        .label(item.text())
        .ellipsize(pango::EllipsizeMode::End)
        .lines(3)
        .max_width_chars(46)
        .wrap(true)
        .wrap_mode(pango::WrapMode::WordChar)
        .xalign(0.0)
        .build();

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&preview);

    gtk::ListBoxRow::builder()
        .activatable(true)
        .selectable(true)
        .child(&content)
        .build()
}

fn row_id_at(row_ids: &RefCell<Vec<HistoryItemId>>, index: i32) -> Option<HistoryItemId> {
    usize::try_from(index)
        .ok()
        .and_then(|index| row_ids.borrow().get(index).copied())
}

fn move_selection(list: &gtk::ListBox, delta: i32) {
    let current = list.selected_row().map_or(0, |row| row.index());
    let target = current.saturating_add(delta).max(0);

    if let Some(row) = list.row_at_index(target) {
        list.select_row(Some(&row));
        row.grab_focus();
    }
}
