use std::cell::Cell;

use gtk::gio::{self, prelude::SettingsExt};

use super::{autostart, schema};

/// The discrete history-limit choices the preferences window offers. Any
/// other in-range value (e.g. set through `gsettings` directly, or left over
/// from a future format) is snapped to the nearest of these, so the rest of
/// the application only ever sees one of four known numbers.
pub const HISTORY_LIMIT_CHOICES: [u32; 4] = [100, 250, 500, 1000];

const KEY_HISTORY_LIMIT: &str = "history-limit";
const KEY_SAVE_IMAGES: &str = "save-images";
const KEY_RECORDING_PAUSED: &str = "recording-paused";
const KEY_AUTO_PASTE: &str = "auto-paste";

pub fn nearest_history_limit(value: i32) -> u32 {
    HISTORY_LIMIT_CHOICES
        .into_iter()
        .min_by_key(|choice| (i64::from(*choice) - i64::from(value)).abs())
        .unwrap_or(HISTORY_LIMIT_CHOICES[HISTORY_LIMIT_CHOICES.len() / 2])
}

/// Default values used when no GSettings backend could be opened at all
/// (see [`schema::open`]). This keeps LionClip usable, without persistence,
/// rather than crashing on a broken or absent GLib schema installation.
struct Defaults {
    history_limit: Cell<u32>,
    save_images: Cell<bool>,
    recording_paused: Cell<bool>,
    auto_paste: Cell<bool>,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            history_limit: Cell::new(500),
            save_images: Cell::new(true),
            recording_paused: Cell::new(false),
            auto_paste: Cell::new(false),
        }
    }
}

enum Backing {
    GSettings(gio::Settings),
    Unavailable(Defaults),
}

/// The single authority for LionClip preferences.
///
/// The popup, the clipboard/history services and the preferences window all
/// read and write settings exclusively through this type rather than each
/// touching `gio::Settings` (or the autostart override file) on their own.
pub struct SettingsService {
    backing: Backing,
}

impl SettingsService {
    /// Opens the real, persisted settings, falling back to unpersisted
    /// in-memory defaults if no GSettings schema could be found or compiled
    /// anywhere (see `src/settings/schema.rs`). Never fails and never
    /// panics: preferences are not on the critical path the way clipboard
    /// history is.
    pub fn open() -> Self {
        let backing = match schema::open() {
            Some(settings) => Backing::GSettings(settings),
            None => {
                eprintln!("lionclip: settings unavailable stage=schema-unavailable");
                Backing::Unavailable(Defaults::default())
            }
        };
        Self { backing }
    }

    #[cfg(test)]
    pub(crate) fn open_for_test() -> Self {
        Self {
            backing: Backing::GSettings(schema::test_support::open_isolated()),
        }
    }

    pub fn history_limit(&self) -> u32 {
        match &self.backing {
            Backing::GSettings(settings) => nearest_history_limit(settings.int(KEY_HISTORY_LIMIT)),
            Backing::Unavailable(defaults) => defaults.history_limit.get(),
        }
    }

    /// Snaps `value` to the nearest of [`HISTORY_LIMIT_CHOICES`] before
    /// storing it, so an out-of-band value can never make its way into the
    /// running application.
    pub fn set_history_limit(&self, value: u32) {
        let snapped = nearest_history_limit(i32::try_from(value).unwrap_or(i32::MAX));
        match &self.backing {
            Backing::GSettings(settings) => {
                let _ = settings.set_int(KEY_HISTORY_LIMIT, i32::try_from(snapped).unwrap_or(500));
            }
            Backing::Unavailable(defaults) => defaults.history_limit.set(snapped),
        }
    }

    /// Calls `handler` with the new, snapped limit every time the stored
    /// history limit changes — including changes this process did not make,
    /// such as `gsettings set io.github.Pianisuto.LionClip history-limit
    /// 100` or dconf-editor. GSettings already delivers those, so reacting
    /// to them needs no polling; without this the resident `TextHistory`
    /// would keep enforcing the old limit until the next restart, while the
    /// persisted value and the preferences window showed the new one.
    ///
    /// The subscription lives as long as the `gio::Settings` this service
    /// owns, which is the whole process, so there is no handler id to keep.
    /// Does nothing when settings fell back to in-memory defaults: there is
    /// no backend to change them from the outside (see [`Backing`]).
    pub fn connect_history_limit_changed(&self, handler: impl Fn(u32) + 'static) {
        let Backing::GSettings(settings) = &self.backing else {
            return;
        };
        settings.connect_changed(Some(KEY_HISTORY_LIMIT), move |settings, _| {
            handler(nearest_history_limit(settings.int(KEY_HISTORY_LIMIT)));
        });
    }

    pub fn save_images(&self) -> bool {
        match &self.backing {
            Backing::GSettings(settings) => settings.boolean(KEY_SAVE_IMAGES),
            Backing::Unavailable(defaults) => defaults.save_images.get(),
        }
    }

    pub fn set_save_images(&self, value: bool) {
        match &self.backing {
            Backing::GSettings(settings) => {
                let _ = settings.set_boolean(KEY_SAVE_IMAGES, value);
            }
            Backing::Unavailable(defaults) => defaults.save_images.set(value),
        }
    }

    pub fn recording_paused(&self) -> bool {
        match &self.backing {
            Backing::GSettings(settings) => settings.boolean(KEY_RECORDING_PAUSED),
            Backing::Unavailable(defaults) => defaults.recording_paused.get(),
        }
    }

    pub fn set_recording_paused(&self, value: bool) {
        match &self.backing {
            Backing::GSettings(settings) => {
                let _ = settings.set_boolean(KEY_RECORDING_PAUSED, value);
            }
            Backing::Unavailable(defaults) => defaults.recording_paused.set(value),
        }
    }

    pub fn auto_paste(&self) -> bool {
        match &self.backing {
            Backing::GSettings(settings) => settings.boolean(KEY_AUTO_PASTE),
            Backing::Unavailable(defaults) => defaults.auto_paste.get(),
        }
    }

    pub fn set_auto_paste(&self, value: bool) {
        match &self.backing {
            Backing::GSettings(settings) => {
                let _ = settings.set_boolean(KEY_AUTO_PASTE, value);
            }
            Backing::Unavailable(defaults) => defaults.auto_paste.set(value),
        }
    }

    /// Whether LionClip effectively starts at login. This is filesystem
    /// state, not a GSettings key: see `src/settings/autostart.rs`.
    pub fn start_at_login(&self) -> bool {
        autostart::config_home().is_none_or(|home| autostart::is_enabled(&home))
    }

    pub fn set_start_at_login(&self, enabled: bool) -> Result<(), autostart::AutostartError> {
        let home = autostart::config_home().ok_or(autostart::AutostartError::ConfigHome)?;
        autostart::set_enabled(&home, enabled)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use gtk::glib;

    use super::*;

    #[test]
    fn nearest_history_limit_snaps_to_the_closest_choice() {
        assert_eq!(nearest_history_limit(100), 100);
        assert_eq!(nearest_history_limit(500), 500);
        assert_eq!(nearest_history_limit(1000), 1000);
        assert_eq!(nearest_history_limit(0), 100);
        assert_eq!(nearest_history_limit(10_000), 1000);
        assert_eq!(nearest_history_limit(733), 500);
        assert_eq!(nearest_history_limit(760), 1000);
    }

    #[test]
    fn defaults_match_the_schema() {
        let settings = SettingsService::open_for_test();
        assert_eq!(settings.history_limit(), 500);
        assert!(settings.save_images());
        assert!(!settings.recording_paused());
        assert!(!settings.auto_paste());
    }

    #[test]
    fn each_setting_persists_across_reads() {
        let settings = SettingsService::open_for_test();

        settings.set_history_limit(250);
        assert_eq!(settings.history_limit(), 250);

        settings.set_save_images(false);
        assert!(!settings.save_images());

        settings.set_recording_paused(true);
        assert!(settings.recording_paused());

        settings.set_auto_paste(true);
        assert!(settings.auto_paste());
    }

    #[test]
    fn history_limit_setter_snaps_out_of_band_values() {
        let settings = SettingsService::open_for_test();
        settings.set_history_limit(733);
        assert_eq!(settings.history_limit(), 500);
    }

    /// Subscribers are notified through the settings backend, which
    /// dispatches on the main context that was thread-default when the
    /// `gio::Settings` was constructed. Owning a private context for the
    /// whole exchange makes that dispatch run inline, so the test observes
    /// the notification without a main loop and without touching the
    /// process-wide default context other tests may be iterating.
    fn with_owned_main_context<R>(body: impl FnOnce() -> R) -> R {
        glib::MainContext::new()
            .with_thread_default(body)
            .expect("acquire a private main context")
    }

    #[test]
    fn history_limit_changes_notify_subscribers() {
        with_owned_main_context(|| {
            let settings = SettingsService::open_for_test();
            let observed: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
            settings.connect_history_limit_changed({
                let observed = observed.clone();
                move |limit| observed.borrow_mut().push(limit)
            });

            // Stands in for the external `gsettings set ... history-limit`
            // case: the subscriber has to hear about the write regardless of
            // who made it, since it is the only thing that tells the resident
            // history to shrink.
            settings.set_history_limit(100);

            assert_eq!(*observed.borrow(), [100]);
        });
    }

    #[test]
    fn subscribers_only_ever_see_snapped_history_limits() {
        with_owned_main_context(|| {
            let settings = SettingsService::open_for_test();
            let observed: Rc<RefCell<Vec<u32>>> = Rc::new(RefCell::new(Vec::new()));
            settings.connect_history_limit_changed({
                let observed = observed.clone();
                move |limit| observed.borrow_mut().push(limit)
            });

            settings.set_history_limit(733);

            assert_eq!(*observed.borrow(), [500]);
        });
    }

    #[test]
    fn two_isolated_instances_never_see_each_others_values() {
        let a = SettingsService::open_for_test();
        let b = SettingsService::open_for_test();

        a.set_recording_paused(true);
        assert!(a.recording_paused());
        assert!(!b.recording_paused());
    }
}
