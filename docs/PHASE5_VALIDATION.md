# Phase 5 validation — desktop integration and packaging

Phase 5 makes LionClip installable and launchable like a normal Zorin/GNOME
utility. This file records what was verified automatically, what was verified on
the real target session, and the manual script for the parts that need a
password or a fresh login.

## Verified automatically

`cargo test --all-features` covers the pure parts:

- argument parsing for `lionclip`, `show`, `hide`, `toggle`, `--help`,
  `--version`, unknown commands and surplus arguments;
- the exit code and message shape for invalid arguments;
- `toggle` mapping to hide when the popup is visible and to show when it is not;
- `show`/`hide` being independent of the current visibility;
- the bare command never opening the popup;
- the application ID matching the desktop entry, the autostart entry, the
  metainfo and the icon file name, and the metainfo version matching the crate.

CI additionally runs `desktop-file-validate` on both desktop files,
`appstreamcli validate` on the metainfo, a display-free CLI smoke test, the
`.deb` build, and assertions over the package contents and dependencies.

## Verified on the target session, from the installed package

Zorin OS, GNOME, X11, two 2560×1440 monitors. The package was installed with
`apt install ./lionclip_0.1.0_amd64.deb` and every check below ran against
`/usr/bin/lionclip`.

### Command surface and popup

| Check | Result |
| --- | --- |
| Autostart's `Exec=lionclip` | 1 process, popup toplevel not even realized |
| Launcher entry (`gtk-launch`) while already running | popup `IsViewable`, still 1 process |
| `lionclip show` | `IsViewable`, placed at the pointer |
| `lionclip toggle` while visible | `IsUnMapped`, no re-placement |
| `lionclip toggle` while hidden | `IsViewable` at the pointer |
| `lionclip show` while already visible | identical coordinates |
| `lionclip hide`, twice | `IsUnMapped`, no error |
| 10 toggles at 400 ms, and 10 back to back | correct parity, popup responsive |
| 30 toggles back to back, 7 runs | always exactly 15 shows, 1 process; see the note below |
| bare `lionclip` while already running | exits 0, no second process, popup untouched |
| `SIGTERM` | exits, history drained |
| `--help`, `--version`, bad argument, `DISPLAY` unset | 0, 0, 2 |
| popup `WM_CLASS` | `("lionclip","lionclip")`, matches `StartupWMClass` |
| Placement across both monitors | correct monitor, coordinates clamped |

### Desktop integration

| Check | Result |
| --- | --- |
| `dpkg -L` | binary, helper, both desktop files, 25 icon paths, metainfo, docs |
| Permissions on disk | `root:root`, 755 binaries, 644 data, autostart is a conffile |
| `Gtk.IconTheme` lookup by icon name | resolves at 16/24/32/48/64/128/256 px to the installed PNGs |
| `Gio.DesktopAppInfo` for the entry | name `LionClip`, icon `io.github.Pianisuto.LionClip`, exec `lionclip show`, `Terminal=false`, not `NoDisplay` |
| `lionclip-shortcut install` | adopted the existing development-build shortcut in place, left an unrelated custom shortcut alone |

### Clipboard capture

Run against an isolated `XDG_DATA_HOME` so the real history was not touched:

| Check | Result |
| --- | --- |
| Copy known text | stored as a `text` row, exact content |
| Copy a known PNG | stored as an `image` row; the blob is byte-identical to the source (SHA-256 match, content-addressed name), with a separate generated thumbnail |
| Restart the process | schema v2, all rows and their kinds survive |

### Package lifecycle

One `apt` sequence, checking the binary, the autostart conffile and
`~/.local/share/lionclip` after every step:

| Step | Package | Binary | Autostart | User data |
| --- | --- | --- | --- | --- |
| reinstall same version | 0.1.0 | present | present | 5 files |
| upgrade to a rebuilt 0.1.1~qa | 0.1.1~qa | present | present | 5 files |
| `apt remove` | config-files | absent | **present** | 5 files |
| reinstall after remove | 0.1.0 | present | present | 5 files |
| `apt purge` | not installed | absent | absent | **5 files** |
| final install | 0.1.0 | present | present | 5 files |

The history inventory (names and sizes) was identical before and after the whole
sequence: neither `remove` nor `purge` touched a byte of it.

## Still to check by hand

These need eyes on the screen or a real keypress, and no automation here would
prove anything:

1. The icon in the app grid, the dock, Alt+Tab and *Settings → Apps*, on both
   light and dark backgrounds.
2. `Super+V` itself: press it, confirm the popup opens near the pointer; press
   it again while open and confirm it closes without reopening elsewhere.
3. Restore: `Enter` on a row, then `Ctrl+V` in another application, for text and
   for an image.
4. Search as you type, `Up`/`Down`, `Delete`, `Ctrl+P` to pin, and the overflow
   menu's clear action.
5. Click away from the popup and confirm it hides.
6. The pointer near each screen edge, on the second monitor, and on a monitor at
   a negative X coordinate if you have one.
7. System light/dark switch while the popup is open.
8. Log out and back in, then — without opening a terminal — copy something,
   press `Super+V`, confirm the history from before the logout is there and that
   `pgrep -xc lionclip` prints `1`.

### Diagnosing autostart

- Is the entry installed?
  `ls /etc/xdg/autostart/io.github.Pianisuto.LionClip.desktop`
- Is it disabled for your user?
  `grep -r Autostart-enabled ~/.config/autostart/io.github.Pianisuto.LionClip.desktop`
- Did it start? `pgrep -xa lionclip`
- What did it say? `journalctl --user -b | grep lionclip` — the log lines are
  structural only: session type, placement backend, coordinates, monitor
  geometry, error stages. No clipboard content, no search queries.

## Known limitations

- **Toggle bursts at machine speed.** Driving `lionclip toggle` as fast as
  separate processes can start it — roughly 15 map/unmap cycles per second —
  ends in the opposite state about one run in three. The command handling is not
  at fault: every burst produced exactly 15 shows for 30 toggles, so no command
  was lost and no toggle misread the visibility. The extra hide comes from the
  focus-loss auto-hide firing during the churn. It is self-correcting, never
  leaves the popup stuck, and did not occur at 400 ms between presses, which is
  already far faster than a keyboard shortcut. Not worth a timing heuristic in
  the auto-hide path.
- The package is `amd64` and targets `noble`. No other architecture, release or
  distribution is built.
- A running LionClip keeps its old executable across an upgrade by design.
- `Super+V` on GNOME goes through a custom keybinding, not a Shell extension, so
  the shortcut is registered per user and does not survive a different user
  account without running `lionclip-shortcut install` there.
- Native Wayland still uses compositor-managed placement, and XWayland is still
  experimental and unvalidated; Phase 5 changed nothing about positioning.
