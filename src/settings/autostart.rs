//! Effective "start at login" state and its per-user override.
//!
//! The packaged system-wide entry lives at
//! `/etc/xdg/autostart/<app-id>.desktop` (installed by `packaging/deb`) and
//! is never touched here: it is package-owned, requires root, and is not a
//! dpkg conffile (see `docs/ARCHITECTURE.md`). The supported way to disable
//! autostart without root is a per-user override at
//! `$XDG_CONFIG_HOME/autostart/<app-id>.desktop` containing `Hidden=true`,
//! which is exactly what GNOME's own Startup Applications / Tweaks write and
//! read. So the filesystem, not a GSettings key, is the single source of
//! truth for this setting: storing a second "enabled" boolean in GSettings
//! could silently disagree with what the override file actually says.

use std::{
    env, fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use crate::cli::APPLICATION_ID;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutostartError {
    ConfigHome,
    Directory,
    Write,
}

impl AutostartError {
    pub fn diagnostic(self) -> &'static str {
        match self {
            Self::ConfigHome => "autostart-config-home",
            Self::Directory => "autostart-directory",
            Self::Write => "autostart-write",
        }
    }
}

pub(super) fn config_home() -> Option<PathBuf> {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty() && Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("HOME")
                .filter(|path| !path.is_empty() && Path::new(path).is_absolute())
                .map(|path| PathBuf::from(path).join(".config"))
        })
}

fn override_path(config_home: &Path) -> PathBuf {
    config_home
        .join("autostart")
        .join(format!("{APPLICATION_ID}.desktop"))
}

/// Whether LionClip effectively starts at login: the system-wide entry
/// applies unless the user's own override marks it `Hidden=true`. Any other
/// content in that file is treated as "not disabled by LionClip", the same
/// rule GNOME's own autostart tooling applies, and a missing/unreadable
/// override is treated the same as no override at all.
pub(super) fn is_enabled(config_home: &Path) -> bool {
    let Ok(content) = fs::read_to_string(override_path(config_home)) else {
        return true;
    };
    !content.lines().any(|line| line.trim() == "Hidden=true")
}

/// Applies the effective state: enabling removes any override so the
/// system-wide entry applies unmodified, disabling writes a minimal
/// `Hidden=true` override. The write is publish-by-rename, so a reader (the
/// session's autostart scan at the next login) never observes a partially
/// written file.
pub(super) fn set_enabled(config_home: &Path, enabled: bool) -> Result<(), AutostartError> {
    let path = override_path(config_home);
    if enabled {
        return match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AutostartError::Write),
        };
    }

    let directory = path.parent().ok_or(AutostartError::Directory)?;
    fs::create_dir_all(directory).map_err(|_| AutostartError::Directory)?;

    let temp = directory.join(format!(".lionclip-autostart-{}.tmp", std::process::id()));
    let write_result = (|| -> Result<(), AutostartError> {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| AutostartError::Write)?;
        file.write_all(b"[Desktop Entry]\nType=Application\nHidden=true\n")
            .map_err(|_| AutostartError::Write)?;
        file.sync_all().map_err(|_| AutostartError::Write)?;
        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temp);
        return write_result;
    }
    fs::rename(&temp, &path).map_err(|_| AutostartError::Write)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

    struct TestHome(PathBuf);

    impl TestHome {
        fn new() -> Self {
            let suffix = NEXT_TEST.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "lionclip-autostart-{}-{suffix}",
                std::process::id()
            ));
            Self(root)
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn no_override_file_means_enabled() {
        let home = TestHome::new();
        assert!(is_enabled(&home.0));
    }

    #[test]
    fn disabling_writes_a_hidden_override_and_enabling_removes_it() {
        let home = TestHome::new();

        set_enabled(&home.0, false).unwrap();
        assert!(!is_enabled(&home.0));
        assert!(override_path(&home.0).is_file());

        set_enabled(&home.0, true).unwrap();
        assert!(is_enabled(&home.0));
        assert!(!override_path(&home.0).exists());
    }

    #[test]
    fn enabling_with_no_override_present_is_a_no_op() {
        let home = TestHome::new();
        assert_eq!(set_enabled(&home.0, true), Ok(()));
        assert!(is_enabled(&home.0));
    }

    #[test]
    fn a_non_hidden_override_file_is_still_treated_as_enabled() {
        let home = TestHome::new();
        let path = override_path(&home.0);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "[Desktop Entry]\nName=Something else\n").unwrap();
        assert!(is_enabled(&home.0));
    }

    #[test]
    fn toggling_twice_is_idempotent() {
        let home = TestHome::new();
        set_enabled(&home.0, false).unwrap();
        set_enabled(&home.0, false).unwrap();
        assert!(!is_enabled(&home.0));
        set_enabled(&home.0, true).unwrap();
        set_enabled(&home.0, true).unwrap();
        assert!(is_enabled(&home.0));
    }
}
