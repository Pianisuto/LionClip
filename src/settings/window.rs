//! The Libadwaita preferences window: a normal, small GNOME settings window,
//! not a second popup surface. It never uses the history popup's
//! pointer-relative positioning, keyboard grab handling or auto-hide — see
//! `src/popup/mod.rs` for that behavior, which is deliberately not shared
//! here.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gtk::glib;

use crate::{
    clipboard::ClipboardWriter,
    history::TextHistory,
    settings::{HISTORY_LIMIT_CHOICES, SettingsService},
};

const WINDOW_WIDTH: i32 = 480;
const WINDOW_HEIGHT: i32 = 480;

/// The preferences window plus the rows whose displayed state has to be
/// re-read from the settings before it is shown again.
///
/// The window is built once and reused, so its widgets would otherwise keep
/// whatever value they were constructed with. Settings can change while it
/// is closed — the popup's own *Resume* button clears the pause, and
/// `gsettings`/dconf can write from outside the process entirely — and a
/// switch still showing the old value would be lying about the state it is
/// supposed to control.
pub struct PreferencesWindow {
    window: adw::PreferencesWindow,
    rows: Rc<Rows>,
}

impl PreferencesWindow {
    /// Shows the window, re-reading every control from the settings first.
    pub fn present(&self) {
        self.rows.refresh();
        self.window.present();
    }
}

struct Rows {
    settings: Rc<SettingsService>,
    auto_paste: adw::SwitchRow,
    recording_paused: adw::SwitchRow,
    history_limit: adw::ComboRow,
    save_images: adw::SwitchRow,
    start_at_login: adw::SwitchRow,
    /// Set while [`Rows::refresh`] writes the stored values into the widgets,
    /// so the change handlers can tell a programmatic update from a real user
    /// action and not write the value straight back — which for the autostart
    /// row would mean touching the filesystem on every open.
    updating: Cell<bool>,
}

impl Rows {
    fn refresh(&self) {
        self.updating.set(true);
        self.auto_paste.set_active(self.settings.auto_paste());
        self.recording_paused
            .set_active(self.settings.recording_paused());
        self.save_images.set_active(self.settings.save_images());
        self.start_at_login
            .set_active(self.settings.start_at_login());
        if let Some(index) = HISTORY_LIMIT_CHOICES
            .iter()
            .position(|choice| *choice == self.settings.history_limit())
            .and_then(|index| u32::try_from(index).ok())
        {
            self.history_limit.set_selected(index);
        }
        self.updating.set(false);
    }
}

/// Builds the preferences window. The caller owns it and is responsible for
/// reusing the single instance (see `AppState` in `src/app.rs`): this
/// function only ever needs to run once per process.
pub fn build(
    application: &adw::Application,
    settings: Rc<SettingsService>,
    history: Rc<RefCell<TextHistory>>,
    writer: ClipboardWriter,
    auto_paste_available: bool,
    on_history_changed: impl Fn() + 'static,
) -> PreferencesWindow {
    let on_history_changed: Rc<dyn Fn()> = Rc::new(on_history_changed);

    let window = adw::PreferencesWindow::builder()
        .application(application)
        .title("Preferences")
        .default_width(WINDOW_WIDTH)
        .default_height(WINDOW_HEIGHT)
        .search_enabled(false)
        .build();

    // A normal window hides rather than destroys itself on close, exactly
    // like the popup, so the single resident instance can keep reusing the
    // same window object instead of rebuilding preferences UI on every
    // `lionclip settings` invocation.
    window.connect_close_request(|window| {
        window.set_visible(false);
        glib::Propagation::Stop
    });

    let rows = Rc::new(Rows {
        settings: settings.clone(),
        auto_paste: adw::SwitchRow::builder()
            .title("Automatically paste selected items")
            .subtitle(if auto_paste_available {
                "Paste the selected item into the app you were using before LionClip."
            } else {
                "Automatic paste is currently available on X11 only."
            })
            .active(settings.auto_paste())
            .sensitive(auto_paste_available)
            .build(),
        recording_paused: adw::SwitchRow::builder()
            .title("Pause clipboard recording")
            .subtitle("Stop capturing new items. Existing history stays available.")
            .active(settings.recording_paused())
            .build(),
        history_limit: history_limit_row(&settings),
        save_images: adw::SwitchRow::builder()
            .title("Save copied images")
            .subtitle("Turn off to stop capturing new images; existing ones are kept.")
            .active(settings.save_images())
            .build(),
        start_at_login: adw::SwitchRow::builder()
            .title("Start LionClip at login")
            .active(settings.start_at_login())
            .build(),
        updating: Cell::new(false),
    });

    connect_handlers(&rows, &history, &writer, on_history_changed.clone());

    let behavior = adw::PreferencesGroup::builder().title("Behavior").build();
    behavior.add(&rows.auto_paste);
    behavior.add(&rows.recording_paused);

    let history_group = adw::PreferencesGroup::builder().title("History").build();
    history_group.add(&rows.history_limit);
    history_group.add(&rows.save_images);

    let system = adw::PreferencesGroup::builder().title("System").build();
    system.add(&rows.start_at_login);

    let page = adw::PreferencesPage::builder().title("Preferences").build();
    page.add(&behavior);
    page.add(&history_group);
    page.add(&system);
    page.add(&data_group(&window, &history, on_history_changed));
    window.add(&page);

    PreferencesWindow { window, rows }
}

fn history_limit_row(settings: &Rc<SettingsService>) -> adw::ComboRow {
    let labels: Vec<String> = HISTORY_LIMIT_CHOICES
        .iter()
        .map(ToString::to_string)
        .collect();
    let label_refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&label_refs);
    let selected = HISTORY_LIMIT_CHOICES
        .iter()
        .position(|choice| *choice == settings.history_limit())
        .unwrap_or(HISTORY_LIMIT_CHOICES.len() / 2);

    adw::ComboRow::builder()
        .title("History limit")
        .subtitle("Oldest unpinned items are removed past this many.")
        .model(&model)
        .selected(u32::try_from(selected).unwrap_or(0))
        .build()
}

fn connect_handlers(
    rows: &Rc<Rows>,
    history: &Rc<RefCell<TextHistory>>,
    writer: &ClipboardWriter,
    on_history_changed: Rc<dyn Fn()>,
) {
    rows.auto_paste.connect_active_notify({
        let rows = rows.clone();
        move |row| {
            if rows.updating.get() {
                return;
            }
            rows.settings.set_auto_paste(row.is_active());
        }
    });

    rows.recording_paused.connect_active_notify({
        let rows = rows.clone();
        let writer = writer.clone();
        move |row| {
            if rows.updating.get() {
                return;
            }
            let paused = row.is_active();
            rows.settings.set_recording_paused(paused);
            if !paused {
                // A restore performed while paused arms self-write
                // suppression that a paused clipboard handler never
                // consumes (it returns before looking at it). Left armed,
                // it could wrongly suppress the next real external copy
                // that happens to match the same text after resuming.
                writer.cancel_pending_self_write();
            }
        }
    });

    rows.history_limit.connect_selected_notify({
        let rows = rows.clone();
        let history = history.clone();
        move |row| {
            if rows.updating.get() {
                return;
            }
            let Some(&limit) = usize::try_from(row.selected())
                .ok()
                .and_then(|index| HISTORY_LIMIT_CHOICES.get(index))
            else {
                return;
            };
            rows.settings.set_history_limit(limit);
            // `AppState` also applies this through the settings' own change
            // notification, which is what makes an external `gsettings`
            // write reach the running history. Applying it here as well is
            // idempotent, and it is the only path left when settings fell
            // back to in-memory defaults, where there is no backend to
            // notify from.
            history.borrow_mut().set_unpinned_limit(limit as usize);
            on_history_changed();
        }
    });

    rows.save_images.connect_active_notify({
        let rows = rows.clone();
        move |row| {
            if rows.updating.get() {
                return;
            }
            rows.settings.set_save_images(row.is_active());
        }
    });

    rows.start_at_login.connect_active_notify({
        let rows = rows.clone();
        move |row| {
            if rows.updating.get() {
                return;
            }
            let desired = row.is_active();
            if let Err(error) = rows.settings.set_start_at_login(desired) {
                eprintln!(
                    "lionclip: autostart change failed stage={}",
                    error.diagnostic()
                );
                rows.updating.set(true);
                row.set_active(!desired);
                rows.updating.set(false);
            }
        }
    });
}

fn data_group(
    window: &adw::PreferencesWindow,
    history: &Rc<RefCell<TextHistory>>,
    on_history_changed: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Data").build();

    let row = adw::ActionRow::builder()
        .title("Clear history")
        .subtitle("Permanently removes all clipboard history, including pinned items.")
        .build();
    let clear_button = gtk::Button::builder()
        .label("Clear…")
        .valign(gtk::Align::Center)
        .build();
    clear_button.add_css_class("destructive-action");
    clear_button.connect_clicked({
        let window = window.clone();
        let history = history.clone();
        move |_| {
            confirm_clear_history(&window, &history, on_history_changed.clone());
        }
    });
    row.add_suffix(&clear_button);
    row.set_activatable_widget(Some(&clear_button));
    group.add(&row);

    group
}

fn confirm_clear_history(
    window: &adw::PreferencesWindow,
    history: &Rc<RefCell<TextHistory>>,
    on_history_changed: Rc<dyn Fn()>,
) {
    let dialog = adw::MessageDialog::new(
        Some(window),
        Some("Clear history?"),
        Some(
            "This permanently removes all clipboard history, including pinned \
             items and saved images.",
        ),
    );
    dialog.add_responses(&[("cancel", "Cancel"), ("clear", "Clear")]);
    dialog.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");

    dialog.connect_response(None, {
        let history = history.clone();
        move |_, response| {
            if response == "clear" && history.borrow_mut().clear_all().changed() {
                on_history_changed();
            }
        }
    });
    dialog.present();
}
