use std::{env, path::PathBuf, process::Command};

/// Compiles `packaging/schemas/` into `$OUT_DIR` so `cargo run`/`cargo test`
/// can find the GSettings schema before the package is installed to
/// `/usr/share/glib-2.0/schemas`. `src/settings/schema.rs` prefers the
/// installed schema and only falls back to this compiled copy, so a real
/// install and a development build resolve to the same dconf path.
///
/// A missing/failing `glib-compile-schemas` is not a build error: it only
/// costs local persistence before the package is installed, and
/// `SettingsService` falls back to an unpersisted in-memory default in that
/// case rather than crashing.
fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let schema_source = manifest_dir.join("packaging/schemas");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));

    println!("cargo:rerun-if-changed={}", schema_source.display());

    let compiled = Command::new("glib-compile-schemas")
        .arg(format!("--targetdir={}", out_dir.display()))
        .arg(&schema_source)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if compiled {
        println!(
            "cargo:rustc-env=LIONCLIP_DEV_SCHEMA_DIR={}",
            out_dir.display()
        );
    } else {
        println!("cargo:rustc-env=LIONCLIP_DEV_SCHEMA_DIR=");
        println!(
            "cargo:warning=glib-compile-schemas unavailable or failed; \
             preferences will not persist until LionClip is installed \
             or run from an environment with glib-compile-schemas"
        );
    }
}
