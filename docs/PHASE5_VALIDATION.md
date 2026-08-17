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

## Verified on the target session

Zorin OS, GNOME, X11, two 2560×1440 monitors, `cargo build --release` binary:

| Check | Result |
| --- | --- |
| `lionclip` starts resident, no window | 1 process, popup unmapped |
| `lionclip show` | popup `IsViewable`, placed at the pointer |
| `lionclip toggle` while visible | popup `IsUnMapped`, no re-placement |
| `lionclip toggle` while hidden | popup `IsViewable` at the pointer |
| `lionclip show` while already visible | stays at the same coordinates |
| `lionclip hide`, twice | `IsUnMapped`, no error |
| 20 rapid `toggle` invocations | ends hidden, still 1 process |
| bare `lionclip` while already running | exits 0, no second process |
| `SIGTERM` | process exits, history drained |
| `--help`, `--version`, bad argument, with `DISPLAY` unset | work, exit 0/0/2 |
| `WM_CLASS` of the popup | `("lionclip", "lionclip")`, matches `StartupWMClass` |
| Placement across both monitors | correct monitor chosen, coordinates clamped |

`lionclip-shortcut` was run against the live session for detection only
(`status`, and `install` refusing a conflict) and against a stubbed `gsettings`
for the writing paths: install, reinstall, adopting a shortcut left over from a
development build, refusing a foreign `Super+V` shortcut, remove, remove again,
and invalid arguments.

## Manual QA on Zorin GNOME/X11

### Clean install

1. Stop any development build: `pkill -x lionclip`.
2. Remove a stale development shortcut if you have one:
   `lionclip-shortcut status` — if it points at a `target/release/lionclip`
   path, `lionclip-shortcut install` will update it in step 7.
3. `sudo apt install ./lionclip_0.1.0_amd64.deb`
4. Confirm the launcher entry: open the app grid and search for "LionClip".
5. Confirm the icon is the lion tile in the app grid, in the dock and in the
   *Settings → Apps* list.
6. Click it. The popup opens near the pointer.
7. `lionclip-shortcut install` (add `--take-over` if it reports GNOME's
   notification-list conflict). Confirm with `lionclip-shortcut status`.

### Behavior

8. Copy text in two different applications, press `Super+V`, confirm both
   entries, pick one with `Enter`, paste with `Ctrl+V`.
9. Take a screenshot or copy an image, press `Super+V`, confirm the thumbnail,
   restore it and paste it into an image-capable application.
10. Press `Super+V` while the popup is open: it closes, and does not close and
    reopen at a new position.
11. Toggle 20–30 times quickly. No flicker, no second window, no stuck popup.
12. Click away from the popup: it hides.
13. Search, `Up`/`Down`, `Delete`, `Ctrl+P` to pin, overflow menu to clear
    unpinned items.
14. Open the popup with the pointer near each screen edge and on the second
    monitor, including a monitor at a negative X coordinate if you have one.
    The popup stays fully on the monitor under the pointer.
15. Switch the system appearance between light and dark; the popup follows.

### Autostart and single instance

16. Log out and back in. Do not open a terminal.
17. Copy something, press `Super+V`, confirm the history is there and that
    entries from before the logout survived.
18. `pgrep -xc lionclip` must print `1`.
19. Run `lionclip` in a terminal: it must not create a second process and must
    not open the popup.
20. Click the launcher entry while it is running: the popup shows; still one
    process.

### Packaging

21. Reinstall the same package: `sudo apt install ./lionclip_0.1.0_amd64.deb`.
    Confirm it succeeds and the running instance is untouched.
22. Rebuild the package (`packaging/deb/build.sh`) and install the rebuilt file
    as an upgrade. Confirm the files on disk change; the running process keeps
    the old code until you log out or run
    `pkill -x lionclip && setsid lionclip >/dev/null 2>&1 &`.
23. `sudo apt remove lionclip`, then confirm `~/.local/share/lionclip` still
    exists.
24. Reinstall, confirm the history is still there.
25. `sudo apt purge lionclip`, then confirm `~/.local/share/lionclip` still
    exists and `/etc/xdg/autostart/io.github.Pianisuto.LionClip.desktop` is gone.
26. Delete the data yourself if you want it gone:
    `rm -rf ~/.local/share/lionclip`.

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

- Installing, removing and purging the package need a password, so they were
  scripted and inspected here but executed only as an unpacked tree
  (`dpkg-deb -x`), not through `apt`.
- The package is `amd64` and targets `noble`. No other architecture, release or
  distribution is built.
- A running LionClip keeps its old executable across an upgrade by design.
- `Super+V` on GNOME goes through a custom keybinding, not a Shell extension, so
  the shortcut is registered per user and does not survive a different user
  account without running `lionclip-shortcut install` there.
- Native Wayland still uses compositor-managed placement, and XWayland is still
  experimental and unvalidated; Phase 5 changed nothing about positioning.
