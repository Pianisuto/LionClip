mod row;

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::{
    clipboard::ClipboardWriter,
    history::{HistoryItemId, HistoryQuery, TextHistory, TextHistoryItem},
    settings::SettingsService,
};

const POPUP_WIDTH: i32 = 430;
const PLACEHOLDER_HEIGHT: i32 = 128;
const LIST_MAX_HEIGHT: i32 = 360;

/// How many result rows are built at a time.
///
/// `GtkListBox` is not virtualized: it holds a widget per row and measures
/// every one of them. Building the whole history therefore costs O(items) on
/// exactly the stretch between the shortcut and the popup appearing — measured
/// at about 1 ms per item on the GNOME/X11 target, so 500 ms for a full
/// 500-item history, which is the entire perceived open latency.
///
/// Only rows the viewport can reach are built up front; the rest follow when
/// scrolling or keyboard navigation actually asks for them. The chunk is
/// several times what [`LIST_MAX_HEIGHT`] can show, so a scroll never runs out
/// of built rows before the next chunk lands.
const RENDER_CHUNK: usize = 32;

/// How much unscrolled height may remain below the viewport before the next
/// chunk is built. Roughly two thirds of a viewport, so growth happens while
/// there is still content to scroll through rather than at the very bottom.
const RENDER_AHEAD_PX: f64 = 240.0;

/// Keeps the popup a single rounded surface and reveals row actions on hover,
/// selection or keyboard focus. Both the toplevel and its content explicitly
/// paint Adwaita's window color: relying on a transparent toplevel left some
/// GTK/X11 theme combinations with an opaque black backing surface and no
/// visible child content.
const POPUP_CSS: &str = "\
window.lionclip-popup {
  background-color: @window_bg_color;
  box-shadow: none;
  border: none;
  border-radius: 12px;
}

.lionclip-surface {
  background-color: @window_bg_color;
  border-radius: 12px;
}

.lionclip-actions {
  opacity: 0;
  transition: opacity 100ms ease-out;
}

row:hover .lionclip-actions,
row:selected .lionclip-actions,
row:focus-within .lionclip-actions,
.lionclip-actions.pinned {
  opacity: 1;
}
";

/// Platform probe telling whether the popup still owns the keyboard focus.
/// `None` when the platform cannot answer.
type KeyboardFocusProbe = Box<dyn Fn(&adw::ApplicationWindow) -> Option<bool>>;

/// The buttons the keyboard can reach inside one built result row. The item a
/// row stands for is not repeated here: `matches` already holds it at the same
/// index.
type RowActions = [gtk::Button; 2];

pub struct HistoryPopup {
    pub window: adw::ApplicationWindow,
    state: Rc<PopupState>,
}

struct PopupState {
    history: Rc<RefCell<TextHistory>>,
    settings: Rc<SettingsService>,
    writer: ClipboardWriter,
    window: adw::ApplicationWindow,
    search: gtk::SearchEntry,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    placeholder: gtk::Box,
    placeholder_title: gtk::Label,
    placeholder_body: gtk::Label,
    paused_indicator: gtk::Box,
    clear_action: gio::SimpleAction,
    /// Every item the current query matches, in display order. This is the
    /// list the popup navigates and reports on; `rendered` is the prefix of it
    /// that currently has widgets.
    matches: RefCell<Vec<HistoryItemId>>,
    /// The rows actually built, always a prefix of `matches`, so a list index
    /// means the same thing in both.
    rendered: RefCell<Vec<RowActions>>,
    /// Guards `render_through` against re-entering itself through a signal
    /// emitted while it is appending rows.
    rendering: Cell<bool>,
    restore: Box<dyn Fn(HistoryItemId)>,
    open_settings: Box<dyn Fn()>,
    /// Answering `None` falls back to trusting the toplevel's own state.
    keeps_keyboard_focus: KeyboardFocusProbe,
    /// Number of the popup's own surfaces (overflow menu, confirmation dialog)
    /// that currently own the focus, so the popup does not hide itself before
    /// the interaction it was opened for can complete. A count rather than a
    /// flag because one surface hands over to the next: activating a menu item
    /// opens the dialog while the menu is still closing.
    suppression_depth: Cell<u32>,
    /// Set when the toplevel went inactive while suppressed. Clearing the
    /// suppression cannot rely on a further `is-active` notification, so the
    /// hide condition is re-checked once when the internal surface closes.
    deferred_hide_check: Cell<bool>,
    /// Guards programmatic search-field updates from re-entering the filter.
    updating_search: Cell<bool>,
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    application: &adw::Application,
    history: Rc<RefCell<TextHistory>>,
    settings: Rc<SettingsService>,
    writer: ClipboardWriter,
    on_restore: impl Fn(HistoryItemId) + 'static,
    keeps_keyboard_focus: impl Fn(&adw::ApplicationWindow) -> Option<bool> + 'static,
    on_open_settings: impl Fn() + 'static,
) -> HistoryPopup {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("LionClip")
        .default_width(POPUP_WIDTH)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("lionclip-popup");

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search clipboard…")
        .hexpand(true)
        .search_delay(0)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search clipboard history")]);

    let menu = gio::Menu::new();
    menu.append(
        Some("Clear Unpinned History…"),
        Some("popup.clear-unpinned"),
    );
    let settings_section = gio::Menu::new();
    settings_section.append(Some("Preferences"), Some("popup.open-settings"));
    menu.append_section(None, &settings_section);
    let menu_button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("More options")
        .menu_model(&menu)
        .valign(gtk::Align::Center)
        .build();
    menu_button.add_css_class("flat");
    menu_button.update_property(&[gtk::accessible::Property::Label("More options")]);

    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(8)
        .margin_end(8)
        .build();
    header.append(&search);
    header.append(&menu_button);

    let list = gtk::ListBox::builder()
        .activate_on_single_click(true)
        .selection_mode(gtk::SelectionMode::Single)
        .show_separators(true)
        .build();

    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .propagate_natural_height(true)
        .max_content_height(LIST_MAX_HEIGHT)
        .vexpand(true)
        .child(&list)
        .build();

    let placeholder_title = gtk::Label::builder().wrap(true).build();
    placeholder_title.add_css_class("heading");
    let placeholder_body = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .build();
    placeholder_body.add_css_class("dim-label");
    let placeholder = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .height_request(PLACEHOLDER_HEIGHT)
        .margin_start(24)
        .margin_end(24)
        .build();
    placeholder.append(&placeholder_title);
    placeholder.append(&placeholder_body);

    // Discreet, hidden unless recording is paused: tells the user why new
    // copies stop appearing without a full banner taking over the popup.
    let paused_label = gtk::Label::builder()
        .label("History paused")
        .xalign(0.0)
        .hexpand(true)
        .build();
    paused_label.add_css_class("dim-label");
    paused_label.add_css_class("caption");
    let paused_resume = gtk::Button::builder().label("Resume").build();
    paused_resume.add_css_class("flat");
    let paused_indicator = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .margin_start(12)
        .margin_end(6)
        .margin_bottom(4)
        .visible(false)
        .build();
    paused_indicator.append(&paused_label);
    paused_indicator.append(&paused_resume);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .build();
    content.add_css_class("background");
    content.add_css_class("lionclip-surface");
    // Clips row highlights and the list to the rounded corners.
    content.set_overflow(gtk::Overflow::Hidden);
    content.append(&header);
    content.append(&paused_indicator);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&scrolled);
    content.append(&placeholder);
    window.set_content(Some(&content));

    if let Some(display) = gdk::Display::default() {
        install_style(&display);
    }

    let clear_action = gio::SimpleAction::new("clear-unpinned", None);
    let open_settings_action = gio::SimpleAction::new("open-settings", None);
    let actions = gio::SimpleActionGroup::new();
    actions.add_action(&clear_action);
    actions.add_action(&open_settings_action);
    window.insert_action_group("popup", Some(&actions));

    let state = Rc::new(PopupState {
        history,
        settings,
        writer,
        window: window.clone(),
        search: search.clone(),
        list: list.clone(),
        scrolled: scrolled.clone(),
        placeholder,
        placeholder_title,
        placeholder_body,
        paused_indicator,
        clear_action: clear_action.clone(),
        matches: RefCell::new(Vec::new()),
        rendered: RefCell::new(Vec::new()),
        rendering: Cell::new(false),
        restore: Box::new(on_restore),
        open_settings: Box::new(on_open_settings),
        keeps_keyboard_focus: Box::new(keeps_keyboard_focus),
        suppression_depth: Cell::new(0),
        deferred_hide_check: Cell::new(false),
        updating_search: Cell::new(false),
    });

    paused_resume.connect_clicked({
        let state = Rc::downgrade(&state);

        move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            state.settings.set_recording_paused(false);
            state.writer.cancel_pending_self_write();
            state.update_paused_indicator();
        }
    });

    // Building rows on demand needs a signal for "the user reached the end of
    // what is built". The adjustment reports both scrolling and the resize a
    // freshly appended chunk causes, which is exactly when to check.
    scrolled.vadjustment().connect_value_changed({
        let state = Rc::downgrade(&state);

        move |_| {
            if let Some(state) = state.upgrade() {
                state.render_more_if_near_end();
            }
        }
    });

    search.connect_search_changed({
        let state = Rc::downgrade(&state);

        move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            if state.updating_search.get() {
                return;
            }
            let selected = state.selected_id();
            state.rebuild(selected, 0);
        }
    });

    list.connect_row_activated({
        let state = Rc::downgrade(&state);

        move |_, row| {
            let Some(state) = state.upgrade() else {
                return;
            };
            if let Some(id) = state.id_at(row.index()) {
                state.restore_item(id);
            }
        }
    });

    menu_button.connect_active_notify({
        let state = Rc::downgrade(&state);

        move |menu_button| {
            let Some(state) = state.upgrade() else {
                return;
            };
            if menu_button.is_active() {
                state.suppress_auto_hide();
            } else {
                state.release_auto_hide_suppression();
            }
        }
    });

    clear_action.connect_activate({
        let state = Rc::downgrade(&state);

        move |_, _| {
            if let Some(state) = state.upgrade() {
                state.confirm_clear_unpinned();
            }
        }
    });

    open_settings_action.connect_activate({
        let state = Rc::downgrade(&state);

        move |_, _| {
            if let Some(state) = state.upgrade() {
                state.hide();
                (state.open_settings)();
            }
        }
    });

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed({
        let state = Rc::downgrade(&state);

        move |_, key, _, modifiers| {
            let Some(state) = state.upgrade() else {
                return glib::Propagation::Proceed;
            };
            state.handle_key(key, modifiers)
        }
    });
    window.add_controller(keys);

    window.connect_is_active_notify({
        let state = Rc::downgrade(&state);

        move |_| {
            if let Some(state) = state.upgrade() {
                state.hide_if_inactive();
            }
        }
    });

    window.connect_close_request(|window| {
        window.set_visible(false);
        glib::Propagation::Stop
    });

    HistoryPopup { window, state }
}

impl HistoryPopup {
    /// Renders the current history for a popup that is already open.
    pub fn refresh(&self) {
        let selected = self.state.selected_id();
        let index = self.state.selected_index().unwrap_or(0);
        self.state.rebuild(selected, index);
    }

    /// Resets the transient search state and renders the current history with
    /// the newest item selected, before the window is shown. Persistent history
    /// state is never reset here.
    ///
    /// Separate from [`Self::present`] so the caller can place the window while
    /// it still holds its final content but is not on screen yet.
    pub fn prepare(&self) {
        // Opening always starts from a known state, so suppression can never
        // stay stuck from an earlier menu or dialog.
        self.state.suppression_depth.set(0);
        self.state.deferred_hide_check.set(false);
        self.state.set_search_text_silently("");
        self.state.rebuild(None, 0);
    }

    pub fn present(&self) {
        self.window.present();
        self.state.focus_search();
    }

    /// Puts the keyboard focus back on the search field of an already open
    /// popup, without presenting the window again.
    pub fn focus_search(&self) {
        self.state.focus_search();
    }

    /// Takes the popup off screen and keeps the process resident, exactly like
    /// `Escape` and the auto-hide do.
    pub fn hide(&self) {
        self.state.hide();
    }
}

impl PopupState {
    fn rebuild(self: &Rc<Self>, prefer: Option<HistoryItemId>, fallback_index: usize) {
        self.update_paused_indicator();
        let query = HistoryQuery::new(&self.search.text());
        let keep_list_focus = self.focus_within(&self.list);

        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        self.rendered.borrow_mut().clear();

        let matches: Vec<HistoryItemId> = {
            let history = self.history.borrow();
            self.clear_action.set_enabled(history.has_unpinned());
            let matches = history
                .search(&query)
                .into_iter()
                .map(TextHistoryItem::id)
                .collect::<Vec<_>>();
            self.update_placeholder(history.items().is_empty(), matches.is_empty());
            matches
        };
        *self.matches.borrow_mut() = matches;

        self.render_through(RENDER_CHUNK);
        self.restore_selection(prefer, fallback_index, keep_list_focus);
    }

    /// Builds result rows until `count` of them exist, starting from the first
    /// match that has no widget yet. See [`RENDER_CHUNK`] for why the whole
    /// match list is not built at once.
    ///
    /// Stops early if a match has disappeared from the history rather than
    /// skipping it, so the built rows stay an exact prefix of `matches` and a
    /// list index keeps meaning the same thing in both.
    fn render_through(self: &Rc<Self>, count: usize) {
        if self.rendering.replace(true) {
            return;
        }

        {
            let matches = self.matches.borrow();
            let mut rendered = self.rendered.borrow_mut();
            let target = count.min(matches.len());

            if rendered.len() < target {
                let history = self.history.borrow();
                for id in &matches[rendered.len()..target] {
                    let Some(item) = history.item(*id) else {
                        break;
                    };
                    let widgets = self.build_row(item);
                    self.list.append(&widgets.row);
                    rendered.push(widgets.actions);
                }
            }
        }

        self.rendering.set(false);
    }

    /// Builds the next chunk when scrolling has come within
    /// [`RENDER_AHEAD_PX`] of the last built row.
    fn render_more_if_near_end(self: &Rc<Self>) {
        let rendered = self.rendered.borrow().len();
        let adjustment = self.scrolled.vadjustment();
        if !should_render_more(
            rendered,
            self.matches.borrow().len(),
            adjustment.value(),
            adjustment.page_size(),
            adjustment.upper(),
        ) {
            return;
        }
        self.render_through(rendered + RENDER_CHUNK);
    }

    fn build_row(self: &Rc<Self>, item: &TextHistoryItem) -> row::RowWidgets {
        let id = item.id();

        row::build(
            item,
            {
                let state = Rc::downgrade(self);

                move || {
                    if let Some(state) = state.upgrade() {
                        state.toggle_pin(id);
                    }
                }
            },
            {
                let state = Rc::downgrade(self);

                move || {
                    if let Some(state) = state.upgrade() {
                        state.delete_item(id);
                    }
                }
            },
        )
    }

    fn update_paused_indicator(&self) {
        self.paused_indicator
            .set_visible(self.settings.recording_paused());
    }

    fn update_placeholder(&self, history_is_empty: bool, results_are_empty: bool) {
        self.scrolled.set_visible(!results_are_empty);
        self.placeholder.set_visible(results_are_empty);
        if !results_are_empty {
            return;
        }

        if history_is_empty {
            self.placeholder_title
                .set_label("Clipboard history is empty");
            self.placeholder_body
                .set_label("Copy something and it will appear here.");
        } else {
            self.placeholder_title.set_label("No matches");
            self.placeholder_body
                .set_label("Try a different search term.");
        }
    }

    fn restore_selection(
        self: &Rc<Self>,
        prefer: Option<HistoryItemId>,
        fallback_index: usize,
        keep_list_focus: bool,
    ) {
        let count = self.matches.borrow().len();
        if count == 0 {
            self.focus_search();
            return;
        }

        let preferred_index = prefer.and_then(|id| self.index_of(id));
        let index = preferred_index.unwrap_or(fallback_index).min(count - 1);
        // A pin or delete can put the item to reselect past the built rows.
        self.render_through(index + 1);
        let Some(row) = self.row_at(index) else {
            return;
        };

        self.list.select_row(Some(&row));
        if keep_list_focus {
            row.grab_focus();
        } else if index == 0 {
            self.scrolled.vadjustment().set_value(0.0);
        }
    }

    fn handle_key(
        self: &Rc<Self>,
        key: gdk::Key,
        modifiers: gdk::ModifierType,
    ) -> glib::Propagation {
        let control = modifiers.contains(gdk::ModifierType::CONTROL_MASK);
        let alt = modifiers.contains(gdk::ModifierType::ALT_MASK);

        match key {
            gdk::Key::Escape => {
                if self.search.text().is_empty() {
                    self.hide();
                } else {
                    self.set_search_text("");
                    self.focus_search();
                }
                glib::Propagation::Stop
            }
            gdk::Key::Up | gdk::Key::KP_Up => {
                self.move_selection(-1);
                glib::Propagation::Stop
            }
            gdk::Key::Down | gdk::Key::KP_Down => {
                self.move_selection(1);
                glib::Propagation::Stop
            }
            gdk::Key::Right | gdk::Key::KP_Right if self.horizontal_arrows_navigate() => {
                self.move_action_focus(true);
                glib::Propagation::Stop
            }
            gdk::Key::Left | gdk::Key::KP_Left if self.horizontal_arrows_navigate() => {
                self.move_action_focus(false);
                glib::Propagation::Stop
            }
            // Enter and Space belong to the focused control when that control
            // activates itself, so a focused row action or the overflow menu
            // button is never shadowed by the window shortcuts.
            gdk::Key::Return
            | gdk::Key::KP_Enter
            | gdk::Key::ISO_Enter
            | gdk::Key::space
            | gdk::Key::KP_Space
                if self.focus_owns_activation() =>
            {
                glib::Propagation::Proceed
            }
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter => {
                if let Some(id) = self.selected_id() {
                    self.restore_item(id);
                }
                glib::Propagation::Stop
            }
            gdk::Key::Delete | gdk::Key::KP_Delete if self.focus_within(&self.list) => {
                if let Some(id) = self.selected_id() {
                    self.delete_item(id);
                }
                glib::Propagation::Stop
            }
            gdk::Key::f | gdk::Key::F if control => {
                self.focus_search();
                self.search.select_region(0, -1);
                glib::Propagation::Stop
            }
            gdk::Key::p | gdk::Key::P if control => {
                if let Some(id) = self.selected_id() {
                    self.toggle_pin(id);
                }
                glib::Propagation::Stop
            }
            _ if control || alt => glib::Propagation::Proceed,
            _ => self.forward_to_search(key),
        }
    }

    /// Types straight into the search field even when a result row holds the
    /// keyboard focus, so search is always one keystroke away.
    fn forward_to_search(&self, key: gdk::Key) -> glib::Propagation {
        if self.focus_within(&self.search) {
            return glib::Propagation::Proceed;
        }
        let Some(character) = key.to_unicode().filter(|character| !character.is_control()) else {
            return glib::Propagation::Proceed;
        };

        let mut text = self.search.text().to_string();
        text.push(character);
        self.focus_search();
        self.set_search_text(&text);
        self.search.set_position(-1);
        glib::Propagation::Stop
    }

    /// Whether Left/Right should reach the row actions instead of the search
    /// caret.
    ///
    /// The search field keeps them while it holds text to move through; with an
    /// empty field there is nothing to move, so the arrows are free to reach the
    /// selected result's actions. Inside the result list they always navigate.
    fn horizontal_arrows_navigate(&self) -> bool {
        if self.focus_within(&self.search) {
            return self.search.text().is_empty();
        }
        self.focus_within(&self.list)
    }

    /// Moves the keyboard focus across the selected row: its pin and delete
    /// buttons going forward, and back to the row itself going backward.
    fn move_action_focus(&self, forward: bool) {
        let Some(index) = self.selected_index() else {
            return;
        };
        let actions = {
            let rendered = self.rendered.borrow();
            let Some(actions) = rendered.get(index) else {
                return;
            };
            actions.clone()
        };

        let focused = actions.iter().position(|action| self.focus_within(action));
        match (focused, forward) {
            (None, true) => self.grab_focus_visibly(&actions[0]),
            (Some(current), true) => {
                self.grab_focus_visibly(&actions[(current + 1).min(actions.len() - 1)]);
            }
            (Some(0), false) => {
                if let Some(row) = self.row_at(index) {
                    self.grab_focus_visibly(&row);
                }
            }
            (Some(current), false) => self.grab_focus_visibly(&actions[current - 1]),
            (None, false) => {}
        }
    }

    fn move_selection(self: &Rc<Self>, delta: i32) {
        let count = self.matches.borrow().len();
        if count == 0 {
            self.focus_search();
            return;
        }

        let Some(current) = self.selected_index() else {
            // Without a selection the first result is the target either way.
            self.focus_row(0);
            return;
        };

        // Down always advances from the current selection, including the very
        // first press while the search field still has the focus: the first
        // result is already selected on open, so stopping there would cost an
        // extra keystroke.
        if delta < 0 && current == 0 {
            self.focus_search();
            return;
        }

        let target = if delta > 0 {
            (current + 1).min(count - 1)
        } else {
            current.saturating_sub(1)
        };
        self.focus_row(target);
    }

    fn restore_item(&self, id: HistoryItemId) {
        (self.restore)(id);
        self.hide();
    }

    fn toggle_pin(self: &Rc<Self>, id: HistoryItemId) {
        let change = {
            let mut history = self.history.borrow_mut();
            if history.item(id).is_some_and(TextHistoryItem::is_pinned) {
                history.unpin(id)
            } else {
                history.pin(id)
            }
        };

        if change.changed() {
            self.rebuild(Some(id), 0);
        }
    }

    fn delete_item(self: &Rc<Self>, id: HistoryItemId) {
        let index = self.index_of(id).unwrap_or(0);
        let change = self.history.borrow_mut().delete(id);

        if change.changed() {
            self.rebuild(None, index);
        }
    }

    fn confirm_clear_unpinned(self: &Rc<Self>) {
        let dialog = adw::MessageDialog::new(
            Some(&self.window),
            Some("Clear unpinned history?"),
            Some("Unpinned clipboard items will be removed. Pinned items are kept."),
        );
        dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
        dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");

        self.suppress_auto_hide();
        dialog.connect_response(None, {
            let state = Rc::downgrade(self);

            move |_, response| {
                let Some(state) = state.upgrade() else {
                    return;
                };

                if response == "clear" && state.history.borrow_mut().clear_unpinned().changed() {
                    state.set_search_text_silently("");
                    state.rebuild(None, 0);
                }
                state.focus_search();
                state.release_auto_hide_suppression();
            }
        });
        dialog.present();
    }

    fn hide(&self) {
        self.window.set_visible(false);
    }

    /// Single decision point for the auto-hide: the popup hides when the
    /// toplevel is inactive and none of the popup's own surfaces is open.
    ///
    /// A keyboard grab also deactivates the toplevel, without the focus ever
    /// leaving it — pressing the desktop shortcut while the popup is open does
    /// exactly that, and hiding on it made the popup close and reopen at the
    /// new pointer position. So a deactivation only counts when the popup has
    /// really lost the keyboard focus.
    fn hide_if_inactive(&self) {
        if self.window.is_active() {
            self.deferred_hide_check.set(false);
            return;
        }
        if self.auto_hide_suppressed() {
            self.deferred_hide_check.set(true);
            return;
        }
        if (self.keeps_keyboard_focus)(&self.window) == Some(true) {
            return;
        }
        self.hide();
    }

    /// Releases the suppression held while one of the popup's own surfaces was
    /// open, one main-context turn later, and re-checks the hide condition if
    /// the toplevel went inactive meanwhile.
    ///
    /// Both steps are needed and neither uses a timing delay:
    ///
    /// 1. a display round trip queues the focus events the closing surface
    ///    generates — an overflow menu deactivates the toplevel simply by
    ///    grabbing the keyboard, and dropping that grab deactivates and
    ///    reactivates it again in quick succession;
    /// 2. releasing on the next main-context turn keeps that transient
    ///    deactivation attributed to the popup's own surface, because queued
    ///    focus events are dispatched before idle sources.
    ///
    /// By the time the release runs the activation state has settled: the
    /// popup stays open when the focus came back to it, and hides when another
    /// window kept the focus — which is also the case where clearing the flag
    /// would otherwise produce no further `is-active` notification at all.
    fn release_auto_hide_suppression(self: &Rc<Self>) {
        if let Some(display) = gdk::Display::default() {
            display.sync();
        }

        let state = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            let Some(state) = state.upgrade() else {
                return;
            };
            let depth = state.suppression_depth.get().saturating_sub(1);
            state.suppression_depth.set(depth);
            if depth == 0 && state.deferred_hide_check.replace(false) {
                state.hide_if_inactive();
            }
        });
    }

    fn suppress_auto_hide(&self) {
        self.suppression_depth
            .set(self.suppression_depth.get().saturating_add(1));
    }

    fn auto_hide_suppressed(&self) -> bool {
        self.suppression_depth.get() > 0
    }

    fn focus_search(&self) {
        self.grab_focus_visibly(&self.search);
    }

    /// Moves the focus and tells the window the move came from the keyboard.
    ///
    /// The theme only draws a focus ring while the window's focus-visible flag
    /// is set, and GTK sets it for the key navigation it handles itself. The
    /// popup handles its own arrows, so it has to say so, otherwise focus lands
    /// on a row action with nothing to show for it.
    fn grab_focus_visibly(&self, widget: &impl IsA<gtk::Widget>) {
        widget.grab_focus();
        self.window.set_focus_visible(true);
    }

    fn focus_row(self: &Rc<Self>, index: usize) {
        // Keyboard navigation can walk past the built rows; build ahead so the
        // next presses do not each pay for a chunk.
        self.render_through(index + RENDER_CHUNK);
        if let Some(row) = self.row_at(index) {
            self.list.select_row(Some(&row));
            self.grab_focus_visibly(&row);
        }
    }

    fn focus_widget(&self) -> Option<gtk::Widget> {
        gtk::prelude::GtkWindowExt::focus(&self.window)
    }

    fn focus_within(&self, widget: &impl IsA<gtk::Widget>) -> bool {
        let widget = widget.as_ref();
        self.focus_widget()
            .is_some_and(|focus| &focus == widget || focus.is_ancestor(widget))
    }

    /// True when the focused widget is an interactive control that handles its
    /// own activation keys, such as a row action button or the overflow menu
    /// button. The search field, a result row and the result list itself all
    /// leave Enter to the popup.
    fn focus_owns_activation(&self) -> bool {
        let Some(focus) = self.focus_widget() else {
            return false;
        };
        if focus.is::<gtk::ListBoxRow>() || &focus == self.list.upcast_ref::<gtk::Widget>() {
            return false;
        }
        !self.focus_within(&self.search)
    }

    fn row_at(&self, index: usize) -> Option<gtk::ListBoxRow> {
        i32::try_from(index)
            .ok()
            .and_then(|index| self.list.row_at_index(index))
    }

    fn index_of(&self, id: HistoryItemId) -> Option<usize> {
        self.matches
            .borrow()
            .iter()
            .position(|candidate| *candidate == id)
    }

    fn id_at(&self, index: i32) -> Option<HistoryItemId> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.matches.borrow().get(index).copied())
    }

    fn selected_index(&self) -> Option<usize> {
        self.list
            .selected_row()
            .and_then(|row| usize::try_from(row.index()).ok())
    }

    fn selected_id(&self) -> Option<HistoryItemId> {
        let index = self.selected_index()?;
        self.matches.borrow().get(index).copied()
    }

    fn set_search_text(&self, text: &str) {
        self.search.set_text(text);
    }

    fn set_search_text_silently(&self, text: &str) {
        self.updating_search.set(true);
        self.search.set_text(text);
        self.updating_search.set(false);
    }
}

/// Whether another chunk of rows should be built for a viewport showing
/// `value..value + page_size` of `upper`, with `rendered` of `total` matches
/// already built.
///
/// Pure so the growth rule is testable without a display: everything it needs
/// is the scroll geometry GTK reports and the two counts.
///
/// A zero `page_size` means the viewport has not been allocated yet, which is
/// exactly the state a fresh rebuild leaves behind when it resets the scroll
/// position. Reading that as "scrolled to the end" would build a second chunk
/// on every open, for a viewport nobody has scrolled.
fn should_render_more(
    rendered: usize,
    total: usize,
    value: f64,
    page_size: f64,
    upper: f64,
) -> bool {
    if rendered >= total || page_size <= 0.0 {
        return false;
    }
    value + page_size >= upper - RENDER_AHEAD_PX
}

fn install_style(display: &gdk::Display) {
    let provider = gtk::CssProvider::new();
    provider.load_from_data(POPUP_CSS);
    gtk::style_context_add_provider_for_display(
        display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

#[cfg(test)]
mod tests {
    use super::{POPUP_CSS, RENDER_AHEAD_PX, should_render_more};

    #[test]
    fn a_viewport_far_from_the_end_builds_no_further_rows() {
        assert!(!should_render_more(32, 500, 0.0, 360.0, 2000.0));
    }

    #[test]
    fn approaching_the_last_built_row_builds_the_next_chunk() {
        assert!(should_render_more(
            32,
            500,
            2000.0 - 360.0 - RENDER_AHEAD_PX,
            360.0,
            2000.0
        ));
    }

    #[test]
    fn everything_already_built_never_grows_again() {
        assert!(!should_render_more(500, 500, 1640.0, 360.0, 2000.0));
        // Not even at the very bottom of the scroll.
        assert!(!should_render_more(500, 500, 1640.0, 360.0, 1640.0));
    }

    #[test]
    fn an_unallocated_viewport_never_grows() {
        // What a rebuild leaves behind before the list is allocated: taking it
        // for "scrolled to the end" would build a second chunk on every open.
        assert!(!should_render_more(32, 500, 0.0, 0.0, 0.0));
    }

    #[test]
    fn a_reset_scroll_over_stale_geometry_never_grows() {
        // The adjustment still describes the previous open's rows when the
        // rebuild resets the position; the top of any scrollable list is far
        // from its end.
        assert!(!should_render_more(32, 500, 0.0, 360.0, 1500.0));
    }

    #[test]
    fn popup_surface_has_a_theme_background_without_a_transparent_toplevel() {
        // A transparent toplevel produced an opaque black X11 surface on the
        // Zorin/GNOME target, hiding every child widget while the window was
        // still mapped. Keep the paint source explicit at both levels.
        assert!(
            POPUP_CSS.contains("window.lionclip-popup {\n  background-color: @window_bg_color;")
        );
        assert!(POPUP_CSS.contains(".lionclip-surface {\n  background-color: @window_bg_color;"));
        assert!(!POPUP_CSS.contains("window.lionclip-popup {\n  background-color: transparent;"));
    }
}
