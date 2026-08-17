//! Resolves the compiled GSettings schema and opens a [`gio::Settings`] from
//! it, trying an installed schema first and only falling back to a locally
//! compiled copy so `cargo run`/`cargo test` work before the package is
//! installed. All fallbacks resolve the same schema `path`, so they all read
//! and write the same dconf keys as a real install.

use std::{fs, path::Path, process::Command};

use gtk::{gio, glib};

use crate::cli::APPLICATION_ID;

const SCHEMA_XML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/packaging/schemas/io.github.Pianisuto.LionClip.gschema.xml"
));

/// The directory `build.rs` compiled `packaging/schemas/` into, or empty if
/// `glib-compile-schemas` was unavailable at build time.
const DEV_SCHEMA_DIR: &str = env!("LIONCLIP_DEV_SCHEMA_DIR");

/// Opens the real schema, trying, in order: the schema installed on this
/// system, the copy `build.rs` compiled for development, and a copy compiled
/// on the fly into a private temporary directory. Returns `None` only when
/// none of those produced a usable schema, which `SettingsService` treats the
/// same way `storage::paths()` failing is treated: run with defaults and no
/// persistence rather than crash.
pub(super) fn open() -> Option<gio::Settings> {
    installed_schema()
        .or_else(dev_compiled_schema)
        .or_else(runtime_compiled_schema)
}

fn installed_schema() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    let schema = source.lookup(APPLICATION_ID, true)?;
    Some(settings_from_schema(&schema))
}

fn dev_compiled_schema() -> Option<gio::Settings> {
    (!DEV_SCHEMA_DIR.is_empty()).then_some(())?;
    schema_from_directory(Path::new(DEV_SCHEMA_DIR))
}

fn runtime_compiled_schema() -> Option<gio::Settings> {
    let directory = std::env::temp_dir().join(format!(
        "lionclip-schema-{}-{}",
        std::process::id(),
        glib::random_int()
    ));
    fs::create_dir_all(&directory).ok()?;
    fs::write(
        directory.join(format!("{APPLICATION_ID}.gschema.xml")),
        SCHEMA_XML,
    )
    .ok()?;
    let compiled = Command::new("glib-compile-schemas")
        .arg(&directory)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !compiled {
        return None;
    }
    schema_from_directory(&directory)
}

fn schema_from_directory(directory: &Path) -> Option<gio::Settings> {
    let parent = gio::SettingsSchemaSource::default();
    let source =
        gio::SettingsSchemaSource::from_directory(directory, parent.as_ref(), true).ok()?;
    let schema = source.lookup(APPLICATION_ID, false)?;
    Some(settings_from_schema(&schema))
}

fn settings_from_schema(schema: &gio::SettingsSchema) -> gio::Settings {
    gio::Settings::new_full(schema, Option::<&gio::SettingsBackend>::None, None)
}

#[cfg(test)]
pub(super) mod test_support {
    use std::cell::RefCell;

    use super::*;

    thread_local! {
        // `gio::SettingsSchema` wraps a non-atomically-refcounted GObject
        // boxed type, so it cannot be `Sync`/`Send` and therefore cannot sit
        // behind a `static OnceLock`. `cargo test` runs each test on its own
        // thread, so caching it per-thread still avoids recompiling the
        // schema for every test.
        static SCHEMA: RefCell<Option<gio::SettingsSchema>> = const { RefCell::new(None) };
    }

    fn compile_test_schema() -> gio::SettingsSchema {
        let directory = std::env::temp_dir().join(format!(
            "lionclip-schema-test-{}-{}",
            std::process::id(),
            glib::random_int()
        ));
        fs::create_dir_all(&directory).expect("create temp schema dir");
        fs::write(
            directory.join(format!("{APPLICATION_ID}.gschema.xml")),
            SCHEMA_XML,
        )
        .expect("write schema xml");
        let status = Command::new("glib-compile-schemas")
            .arg(&directory)
            .status()
            .expect("run glib-compile-schemas");
        assert!(status.success(), "schema must compile for tests");

        let source = gio::SettingsSchemaSource::from_directory(&directory, None, true)
            .expect("build schema source");
        source
            .lookup(APPLICATION_ID, false)
            .expect("schema present")
    }

    /// Opens a settings instance backed by a fresh in-memory
    /// [`gio::functions::memory_settings_backend_new`] instance, so every
    /// test gets its own throwaway store: tests never read or write the
    /// developer's real settings, and parallel tests never see each other's
    /// values. The schema declares a fixed path, so per-test isolation has
    /// to come from the backend rather than the path (`Settings::new_full`
    /// rejects an explicit path for a schema that already declares one).
    pub(in crate::settings) fn open_isolated() -> gio::Settings {
        let schema = SCHEMA.with(|cell| {
            cell.borrow_mut()
                .get_or_insert_with(compile_test_schema)
                .clone()
        });
        let backend = gio::functions::memory_settings_backend_new();
        gio::Settings::new_full(&schema, Some(&backend), None)
    }
}
