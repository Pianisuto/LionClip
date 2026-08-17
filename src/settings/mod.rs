mod autostart;
mod schema;
mod service;
mod window;

pub use service::{HISTORY_LIMIT_CHOICES, SettingsService};
pub use window::{PreferencesWindow, build as build_preferences_window};
