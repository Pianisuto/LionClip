use std::{
    env,
    ffi::OsStr,
    fmt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataPathError;

impl fmt::Display for DataPathError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("neither XDG_DATA_HOME nor HOME provides a usable data directory")
    }
}

pub fn database_path() -> Result<PathBuf, DataPathError> {
    database_path_from(
        env::var_os("XDG_DATA_HOME").as_deref(),
        env::var_os("HOME").as_deref(),
    )
}

fn database_path_from(
    xdg_data_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, DataPathError> {
    let data_home = xdg_data_home
        .filter(|path| !path.is_empty() && Path::new(path).is_absolute())
        .map(PathBuf::from)
        .or_else(|| {
            home.filter(|path| !path.is_empty() && Path::new(path).is_absolute())
                .map(|path| PathBuf::from(path).join(".local/share"))
        })
        .ok_or(DataPathError)?;

    Ok(data_home.join("lionclip/lionclip.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_data_home_override_is_respected_exactly() {
        assert_eq!(
            database_path_from(
                Some(OsStr::new("/tmp/custom-data")),
                Some(OsStr::new("/home/user"))
            ),
            Ok(PathBuf::from("/tmp/custom-data/lionclip/lionclip.db"))
        );
    }

    #[test]
    fn home_fallback_uses_standard_local_share_path() {
        assert_eq!(
            database_path_from(None, Some(OsStr::new("/home/user"))),
            Ok(PathBuf::from(
                "/home/user/.local/share/lionclip/lionclip.db"
            ))
        );
    }

    #[test]
    fn empty_xdg_override_uses_home_fallback() {
        assert_eq!(
            database_path_from(Some(OsStr::new("")), Some(OsStr::new("/home/user"))),
            Ok(PathBuf::from(
                "/home/user/.local/share/lionclip/lionclip.db"
            ))
        );
    }

    #[test]
    fn relative_paths_cannot_resolve_against_the_working_directory() {
        assert_eq!(
            database_path_from(
                Some(OsStr::new("relative")),
                Some(OsStr::new("also-relative"))
            ),
            Err(DataPathError)
        );
    }
}
