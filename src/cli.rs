//! The command surface of LionClip.
//!
//! Parsing is pure and runs before anything touches GTK, so `--help`,
//! `--version` and invalid arguments never open a display, never register the
//! application on the bus and never start a clipboard monitor.

use std::ffi::OsStr;

/// The final application ID. It is also the base name of the desktop entry,
/// the autostart entry, the metainfo file and the installed icon.
pub const APPLICATION_ID: &str = "io.github.Pianisuto.LionClip";

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const HELP: &str = "\
LionClip — clipboard history for GNOME/Zorin

Usage:
  lionclip          start the resident instance without showing the popup
  lionclip show     show the clipboard history popup
  lionclip hide     hide the popup and keep the instance resident
  lionclip toggle   show the popup when it is hidden, hide it when it is visible
  lionclip settings open the preferences window

Options:
  -h, --help        print this help
  -V, --version     print the version

Every invocation is delivered to the single resident instance, which owns the
only clipboard monitor. `toggle` is the command to bind to Super+V.
";

pub const USAGE: &str = "usage: lionclip [show | hide | toggle | settings] [--help] [--version]";

/// What the caller asked the resident instance to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Command {
    /// Start the resident instance and leave the popup alone. This is what
    /// autostart runs at login.
    Run,
    Show,
    Hide,
    Toggle,
    /// Opens (or focuses, if already open) the preferences window. Does not
    /// affect the popup, so it has no `PopupIntent`.
    Settings,
}

/// A command line that the invoked process answers by itself, without a
/// display, an application instance or a clipboard monitor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Answer {
    Help,
    Version,
    Invalid(String),
}

impl Answer {
    pub fn text(&self) -> String {
        match self {
            Self::Help => HELP.to_owned(),
            Self::Version => format!("lionclip {VERSION}\n"),
            Self::Invalid(message) => format!("lionclip: {message}\n{USAGE}\n"),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }

    pub fn exit_code(&self) -> u8 {
        if self.is_error() {
            INVALID_ARGUMENTS
        } else {
            0
        }
    }
}

const INVALID_ARGUMENTS: u8 = 2;

/// What the popup should do about a command, given whether it is on screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PopupIntent {
    Show,
    Hide,
    /// Leave the popup exactly as it is, visible or not.
    Leave,
}

impl Command {
    pub fn intent(self, popup_visible: bool) -> PopupIntent {
        match self {
            Self::Run => PopupIntent::Leave,
            Self::Show => PopupIntent::Show,
            Self::Hide => PopupIntent::Hide,
            // A visible popup is hidden, never hidden and shown again: showing
            // it again would re-place the window the compositor already mapped.
            Self::Toggle if popup_visible => PopupIntent::Hide,
            Self::Toggle => PopupIntent::Show,
            // Settings is handled before `intent` is ever consulted; see
            // `AppState::apply`. It never touches the popup either way.
            Self::Settings => PopupIntent::Leave,
        }
    }
}

/// Parses the arguments after the program name.
pub fn parse<I, S>(arguments: I) -> Result<Command, Answer>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut arguments = arguments.into_iter();
    let Some(first) = arguments.next() else {
        return Ok(Command::Run);
    };
    let Some(first) = first.as_ref().to_str() else {
        return Err(Answer::Invalid("arguments must be valid UTF-8".to_owned()));
    };

    let command = match first {
        "-h" | "--help" => return Err(Answer::Help),
        "-V" | "--version" => return Err(Answer::Version),
        "show" => Command::Show,
        "hide" => Command::Hide,
        "toggle" => Command::Toggle,
        "settings" => Command::Settings,
        other => return Err(Answer::Invalid(format!("unknown command '{other}'"))),
    };

    match arguments.next() {
        None => Ok(command),
        Some(extra) => Err(Answer::Invalid(format!(
            "unexpected argument '{}'",
            extra.as_ref().to_string_lossy()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::*;

    fn parse_args(arguments: &[&str]) -> Result<Command, Answer> {
        parse(arguments.iter())
    }

    #[test]
    fn no_arguments_start_the_resident_instance() {
        assert_eq!(parse_args(&[]), Ok(Command::Run));
    }

    #[test]
    fn the_three_popup_commands_are_recognized() {
        assert_eq!(parse_args(&["show"]), Ok(Command::Show));
        assert_eq!(parse_args(&["hide"]), Ok(Command::Hide));
        assert_eq!(parse_args(&["toggle"]), Ok(Command::Toggle));
    }

    #[test]
    fn settings_command_is_recognized_and_never_touches_the_popup() {
        assert_eq!(parse_args(&["settings"]), Ok(Command::Settings));
        assert_eq!(Command::Settings.intent(false), PopupIntent::Leave);
        assert_eq!(Command::Settings.intent(true), PopupIntent::Leave);
    }

    #[test]
    fn help_and_version_are_answered_without_a_command() {
        assert_eq!(parse_args(&["--help"]), Err(Answer::Help));
        assert_eq!(parse_args(&["-h"]), Err(Answer::Help));
        assert_eq!(parse_args(&["--version"]), Err(Answer::Version));
        assert_eq!(parse_args(&["-V"]), Err(Answer::Version));
        assert_eq!(Answer::Help.exit_code(), 0);
        assert_eq!(Answer::Version.text(), format!("lionclip {VERSION}\n"));
    }

    #[test]
    fn unknown_and_surplus_arguments_are_rejected() {
        assert_eq!(
            parse_args(&["start"]),
            Err(Answer::Invalid("unknown command 'start'".to_owned()))
        );
        assert_eq!(
            parse_args(&["--quiet"]),
            Err(Answer::Invalid("unknown command '--quiet'".to_owned()))
        );
        assert_eq!(
            parse_args(&["toggle", "now"]),
            Err(Answer::Invalid("unexpected argument 'now'".to_owned()))
        );
    }

    #[test]
    fn invalid_arguments_report_a_short_message_and_a_non_zero_exit_code() {
        let answer = parse_args(&["nope"]).expect_err("an unknown command is not a command");
        assert!(answer.is_error());
        assert_eq!(answer.exit_code(), 2);
        assert_eq!(
            answer.text(),
            format!("lionclip: unknown command 'nope'\n{USAGE}\n")
        );
    }

    #[test]
    fn toggle_hides_a_visible_popup_and_shows_a_hidden_one() {
        assert_eq!(Command::Toggle.intent(true), PopupIntent::Hide);
        assert_eq!(Command::Toggle.intent(false), PopupIntent::Show);
    }

    #[test]
    fn show_and_hide_do_not_depend_on_each_other() {
        assert_eq!(Command::Show.intent(false), PopupIntent::Show);
        assert_eq!(Command::Show.intent(true), PopupIntent::Show);
        assert_eq!(Command::Hide.intent(true), PopupIntent::Hide);
        assert_eq!(Command::Hide.intent(false), PopupIntent::Hide);
    }

    #[test]
    fn starting_the_resident_instance_never_opens_the_popup() {
        assert_eq!(Command::Run.intent(false), PopupIntent::Leave);
        assert_eq!(Command::Run.intent(true), PopupIntent::Leave);
    }

    fn packaging_file(relative: &str) -> String {
        let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "packaging", relative]
            .iter()
            .collect();
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    }

    /// The application ID is a contract between the binary and every installed
    /// desktop file, so a divergence has to fail here rather than on a user's
    /// session.
    #[test]
    fn desktop_integration_uses_the_application_id() {
        let desktop = packaging_file(&format!("desktop/{APPLICATION_ID}.desktop"));
        assert!(desktop.contains(&format!("Icon={APPLICATION_ID}\n")));
        assert!(desktop.contains("Name=LionClip\n"));
        assert!(desktop.contains("Exec=lionclip show\n"));
        assert!(desktop.contains("Terminal=false\n"));

        let autostart = packaging_file(&format!("autostart/{APPLICATION_ID}.desktop"));
        assert!(autostart.contains(&format!("Icon={APPLICATION_ID}\n")));
        // Autostart starts the monitor and must never open the popup at login.
        assert!(autostart.contains("Exec=lionclip\n"));
        assert!(autostart.contains("NoDisplay=true\n"));
        assert!(autostart.contains("X-GNOME-Autostart-enabled=true\n"));

        let metainfo = packaging_file(&format!("metainfo/{APPLICATION_ID}.metainfo.xml"));
        assert!(metainfo.contains(&format!("<id>{APPLICATION_ID}</id>")));
        assert!(metainfo.contains(&format!(
            "<launchable type=\"desktop-id\">{APPLICATION_ID}.desktop</launchable>"
        )));

        packaging_file(&format!("icons/{APPLICATION_ID}.svg"));
    }

    #[test]
    fn the_packaged_version_follows_the_crate_version() {
        let metainfo = packaging_file(&format!("metainfo/{APPLICATION_ID}.metainfo.xml"));
        assert!(metainfo.contains(&format!("version=\"{VERSION}\"")));
    }
}
