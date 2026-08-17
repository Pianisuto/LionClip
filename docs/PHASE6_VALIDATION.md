# Phase 6 validation — preferences and privacy controls

Phase 6 adds a native Libadwaita preferences window and safe, opt-in
auto-paste. This file records what was verified automatically in this
environment (no GNOME session, no window manager) and the manual script for
everything that needs a real desktop. Read `AGENTS.md` and
`docs/ARCHITECTURE.md` first for the constraints this phase had to respect.

## Verified automatically

`cargo test --all-features` (106 tests) covers:

- **Settings persistence** — defaults match the schema; each setting
  persists across reads; the history-limit setter snaps an out-of-range value
  to the nearest of 100/250/500/1000; two isolated `SettingsService`
  instances never see each other's values (see `src/settings/service.rs`).
- **Autostart override** — no override file means enabled; disabling writes
  a `Hidden=true` override and enabling removes it; a non-`Hidden` override
  is still treated as enabled; toggling twice is idempotent
  (`src/settings/autostart.rs`).
- **History domain rules** — `clear_unpinned` keeps pinned items and is
  rejected when nothing would change; `clear_all` removes pinned and
  unpinned alike and is rejected when already empty; both bump the
  `TextHistory` generation counter; `set_unpinned_limit` applies retention
  immediately and never touches pinned items; the same-value case is a no-op
  (`src/history/service.rs`). `clear_all` persists and survives a restart
  (`src/history/regression_tests.rs`).
- **CLI** — `settings` parses to `Command::Settings`, which never changes
  `PopupIntent` regardless of popup visibility (`src/cli.rs`).
- **Auto-paste decision logic** — pure, GTK/X11-free: auto-paste off never
  pastes; on with a captured target and a successful restore pastes; on
  without a captured target only restores; a failed restore never pastes
  even with auto-paste on and a valid target (`src/paste/mod.rs`).
- **X11 paste backend, against a real disposable Xvfb server** (spawned and
  torn down per test, own display number, no shared state): capturing the
  window that holds input focus; rejecting `PointerRoot`/`None` focus as "no
  target"; a destroyed target is rejected and nothing is synthesized; the
  full sequence (SetInputFocus reinforcement → focus confirmation → XTEST
  Ctrl+V) hands focus back to the target and delivers both key events only
  to that window, never to a decoy that had focus in between; and a target
  that *already* holds focus is confirmed immediately rather than waiting
  out the timeout for a `FocusIn` event the server will never send
  (`src/paste/x11.rs::xvfb_tests`). There is no window manager under Xvfb, so
  the `_NET_ACTIVE_WINDOW` half of activation is exercised only on the real
  target machine (see below); the direct `SetInputFocus` path is what these
  tests confirm.
- Everything from Phases 0–5 (positioning, clipboard capture, history
  ordering/retention/search, image storage, packaging metadata) is
  unchanged and still passes.

CI additionally compiles `packaging/schemas/*.gschema.xml` with
`glib-compile-schemas --strict`, asserts the built `.deb` contains the schema
source and does **not** contain a compiled `gschemas.compiled` (that file is
the merge of every schema on the system; shipping our own would clobber every
other application's), and runs the Xvfb-backed tests as part of the normal
test job.

## Not verifiable in this environment

This session has no display, no GNOME session, and no window manager
(Xvfb has neither). The following need a real Zorin/GNOME/X11 machine and
have **not** been exercised beyond code review:

- The Preferences window's actual appearance, light/dark theming, and
  keyboard navigation.
- `_NET_ACTIVE_WINDOW`-based activation specifically (Mutter's EWMH path);
  Xvfb has no window manager to receive it.
- Real application behavior on the receiving end of auto-paste (a terminal,
  a browser, a text editor, a chat client, an image editor).
- GNOME's own "Startup Applications" tool reading back the autostart state
  LionClip's toggle writes.
- Any Wayland-session behavior at all.

## Manual QA checklist

Run on the target Zorin GNOME/X11 machine, from the installed package
(`sudo apt install ./lionclip_<version>_amd64.deb`).

### Preferences window

1. Open via the popup's overflow menu → *Preferences*, and via
   `lionclip settings` from a terminal: both must reuse the same window
   (check `pgrep -xc lionclip` stays `1`; no second window appears).
2. Close the window (X button), then reopen with `lionclip settings` — it
   must reappear with the same state, not rebuild from scratch.
3. Confirm the window looks like a normal small GNOME settings window: no
   pointer-relative placement, no auto-hide on focus loss, decorated,
   resizable/movable like any other window, correct in both light and dark
   system themes.
4. Tab through every row with the keyboard only; confirm every switch,
   the combo row and the destructive button are reachable and operable
   without a mouse.

### Each setting

5. **History limit** — set to 100 with more than 100 unpinned items present;
   confirm the oldest unpinned items (and, for images, their thumbnail/blob
   files under `~/.local/share/lionclip`) disappear immediately, pinned
   items are untouched, and no restart was needed.
6. **Save copied images** — turn off, copy a new screenshot: it must not
   appear in history, but copying new text still works. Copy an image from
   an app that also offers a text representation (e.g. a file manager
   "copy path" alongside a thumbnail): the text should still be captured.
   Turn back on and confirm new images resume appearing, without touching
   images already in history.
7. **Pause clipboard recording** — turn on: copying new text/images does
   nothing; existing items still restore, search, pin, delete and clear
   normally; the popup shows the "History paused" indicator with a *Resume*
   button. Click *Resume*: the indicator disappears and new copies start
   appearing again, and the switch in Preferences reflects the change.
8. **Start LionClip at login** — toggle off, confirm
   `~/.config/autostart/io.github.Pianisuto.LionClip.desktop` appears with
   `Hidden=true`; toggle on, confirm the file is removed; log out and back in
   after each state to confirm the resident instance does/does not start.
   Cross-check with GNOME Settings → *Startup Applications* if available.
9. **Clear history…** — with a mix of pinned and unpinned text and image
   items, click *Clear…*, confirm the dialog names pinned items and images
   explicitly, cancel once (nothing removed), then confirm: all items gone
   from the popup, the SQLite row count is zero, and the image blob/thumbnail
   directories no longer contain files for the removed items.

### Auto-paste (X11)

10. Enable *Automatically paste selected items*. For each of a terminal, a
    browser address bar, a code editor, a chat input and an image-capable
    app (e.g. GIMP): focus it, press `Super+V`, pick a text item with
    `Enter` — the text must land in that exact application, not wherever
    focus happened to end up. Repeat with mouse click selection.
11. Repeat step 10 for an image item pasted into an image-capable app.
12. **Redundant activation.** Select an item with auto-paste enabled and
    watch how long the popup takes to disappear, then disable the setting
    and repeat: the two should feel the same. LionClip skips the activation
    request whenever the target already holds focus, which it normally does
    by then because hiding the popup already returned it — asking the
    compositor to activate an already-active window costs a visible
    activation cycle. This is deliberately not covered by the Xvfb tests: X
    emits no focus event when focus does not actually change, so the
    protocol side is a no-op and the whole cost lives in the compositor.
13. Toggle the setting off and on again without restarting LionClip; confirm
    the very next selection immediately reflects the new state.
14. Open the popup, then close the target application before selecting an
    item: the selection must restore the clipboard and close the popup, and
    must **not** paste into whatever window happens to be focused instead.
15. Open the popup over app A, deliberately click into a different app B
    while the popup is still open (if focus-follows-click permits), then
    select an item: it must not paste into B.
16. Confirm `Up`/`Down` navigation, clicking *Pin*, clicking *Delete*,
    typing in search, and opening the overflow menu or Preferences from the
    popup never trigger a paste — only `Enter` or clicking a row activates
    it.
17. Confirm nothing under `journalctl --user -b | grep lionclip` contains
    clipboard content, search text, or window titles; technical IDs
    (`sent=true`/`sent=false`, stage names) are expected.

### Wayland session

18. Log into a Wayland session (if available) and open Preferences: the
    auto-paste switch must be disabled with the "X11 only" subtitle, every
    other setting must work normally, and selecting a history item must
    restore the clipboard without attempting to synthesize a paste or
    crashing.

## Known limitations

- Auto-paste is X11-only by design; see `AGENTS.md` §5 and
  `docs/ARCHITECTURE.md`'s positioning notes for why the same platform split
  already applies to popup placement. XWayland-in-Wayland sessions are
  attempted (same `is_x11()` gate positioning already uses) but are exactly
  as unvalidated for auto-paste as they are for placement.
- Retention by wall-clock days was deliberately not implemented: schema v2
  stores logical sequences, not real timestamps, and inventing one just for
  a retention slider would be fake metadata for a feature the roadmap marks
  optional. See `docs/ROADMAP.md`.
- The 512 MiB aggregate image storage cap remains a fixed safety limit, not
  a user-facing setting, for the same reason Phase 4 did not expose it: it
  is a technical backstop, not a preference someone tunes day to day.
- Focus confirmation is a bounded ~400 ms wait for real server state — the
  target already owning the focus, or a `FocusIn` event saying it just
  gained it — not a blocking wait forever; a window manager that takes
  longer than that to switch focus will fail safe (restore only, no paste)
  rather than hang.
