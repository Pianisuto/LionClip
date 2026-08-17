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

/// Builds the preferences window. The caller owns it and is responsible for
/// reusing the single instance (see `AppState::show_settings` in
/// `src/app.rs`): this function only ever needs to run once per process.
pub fn build(
    application: &adw::Application,
    settings: Rc<SettingsService>,
    history: Rc<RefCell<TextHistory>>,
    writer: ClipboardWriter,
    auto_paste_available: bool,
    on_history_changed: impl Fn() + 'static,
) -> adw::PreferencesWindow {
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

    let page = adw::PreferencesPage::builder().title("Preferences").build();
    page.add(&behavior_group(&settings, &writer, auto_paste_available));
    page.add(&history_group(
        &settings,
        &history,
        on_history_changed.clone(),
    ));
    page.add(&system_group(&settings));
    page.add(&data_group(&window, &history, on_history_changed));
    window.add(&page);

    window
}

fn behavior_group(
    settings: &Rc<SettingsService>,
    writer: &ClipboardWriter,
    auto_paste_available: bool,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Behavior").build();

    let auto_paste = adw::SwitchRow::builder()
        .title("Automatically paste selected items")
        .subtitle(if auto_paste_available {
            "Paste the selected item into the app you were using before LionClip."
        } else {
            "Automatic paste is currently available on X11 only."
        })
        .active(settings.auto_paste())
        .sensitive(auto_paste_available)
        .build();
    auto_paste.connect_active_notify({
        let settings = settings.clone();
        move |row| settings.set_auto_paste(row.is_active())
    });
    group.add(&auto_paste);

    let recording_paused = adw::SwitchRow::builder()
        .title("Pause clipboard recording")
        .subtitle("Stop capturing new items. Existing history stays available.")
        .active(settings.recording_paused())
        .build();
    recording_paused.connect_active_notify({
        let settings = settings.clone();
        let writer = writer.clone();
        move |row| {
            let paused = row.is_active();
            settings.set_recording_paused(paused);
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
    group.add(&recording_paused);

    group
}

fn history_group(
    settings: &Rc<SettingsService>,
    history: &Rc<RefCell<TextHistory>>,
    on_history_changed: Rc<dyn Fn()>,
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("History").build();

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

    let history_limit = adw::ComboRow::builder()
        .title("History limit")
        .subtitle("Oldest unpinned items are removed past this many.")
        .model(&model)
        .selected(u32::try_from(selected).unwrap_or(0))
        .build();
    history_limit.connect_selected_notify({
        let settings = settings.clone();
        let history = history.clone();
        let on_history_changed = on_history_changed.clone();
        move |row| {
            let Some(&limit) = usize::try_from(row.selected())
                .ok()
                .and_then(|index| HISTORY_LIMIT_CHOICES.get(index))
            else {
                return;
            };
            settings.set_history_limit(limit);
            history.borrow_mut().set_unpinned_limit(limit as usize);
            on_history_changed();
        }
    });
    group.add(&history_limit);

    let save_images = adw::SwitchRow::builder()
        .title("Save copied images")
        .subtitle("Turn off to stop capturing new images; existing ones are kept.")
        .active(settings.save_images())
        .build();
    save_images.connect_active_notify({
        let settings = settings.clone();
        move |row| settings.set_save_images(row.is_active())
    });
    group.add(&save_images);

    group
}

fn system_group(settings: &Rc<SettingsService>) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("System").build();

    let start_at_login = adw::SwitchRow::builder()
        .title("Start LionClip at login")
        .active(settings.start_at_login())
        .build();
    let reverting = Rc::new(Cell::new(false));
    start_at_login.connect_active_notify({
        let settings = settings.clone();
        let reverting = reverting.clone();
        move |row| {
            if reverting.get() {
                return;
            }
            let desired = row.is_active();
            if let Err(error) = settings.set_start_at_login(desired) {
                eprintln!(
                    "lionclip: autostart change failed stage={}",
                    error.diagnostic()
                );
                reverting.set(true);
                row.set_active(!desired);
                reverting.set(false);
            }
        }
    });
    group.add(&start_at_login);

    group
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
