# LionClip Roadmap

The roadmap is organized as **vertical, testable phases**. Do not implement later phases early unless they are required to make the current phase work.

## Phase 0 — Technical spike: popup placement

**Status: complete.** Validated on the primary Zorin GNOME/X11 target.

### Goal

Prove the riskiest interaction before investing in the clipboard/history architecture.

### Build

- minimal Rust + GTK4 + Libadwaita app;
- a single-instance-ready application skeleton without overengineering it;
- a small test popup;
- session/backend diagnostics that do not expose sensitive data;
- positioning abstraction kept minimal;
- isolated X11 backend, also retained for XWayland experiments;
- pointer-relative placement on X11;
- monitor-edge clamping;
- predictable fallback placement.

### Visual result

Invoking the app shows a small Libadwaita test popup. On the primary Zorin
GNOME/X11 machine, it opens near the pointer and remains within the active
monitor. On native Wayland it identifies compositor-managed fallback placement.

### Acceptance

- [x] builds on the target Zorin OS machine;
- [x] popup opens reliably;
- [x] active backend/session is reported for diagnostics;
- [x] no crashes when exact positioning is unavailable;
- [x] GNOME/X11 pointer-relative placement is classified as `working` based on real testing;
- [x] native GNOME Wayland exact placement is classified as `not available` through the current approach and uses fallback;
- [x] XWayland is explicitly classified as `experimental` and unvalidated;
- [x] chosen approach and limitations are documented before Phase 1.

### Conclusion

- **GNOME/X11:** working and validated on the real target machine. This is the
  primary V1 validation environment.
- **Native GNOME Wayland:** exact pointer-relative top-level placement is not
  available through the current approach; compositor fallback remains.
- **XWayland inside a Wayland session:** experimental and not yet validated.

Wayland/XWayland validation is not a prerequisite for starting Phase 1. The
fallback and experimental backend remain in place.

---

## Phase 1 — Real text clipboard history in memory

**Status: complete.**

### Goal

Make LionClip useful for text without persistence.

### Build

- resident clipboard monitor;
- event-driven text capture;
- in-memory history;
- simple deduplication;
- actual history rows in the popup;
- restore selected item to clipboard;
- close after selection;
- basic keyboard navigation;
- no SQLite yet.

### Visual result

Copy text in multiple apps, open LionClip, see recent items, choose one, then paste it normally with `Ctrl+V`.

### Acceptance

- works across representative apps on the target session;
- repeated identical copies do not create obvious duplicate noise;
- no clipboard payload appears in logs;
- idle behavior does not poll aggressively;
- selecting an item restores exact text content to the clipboard.

---

## Phase 2 — Persistence and history rules

**Status: complete.**

### Goal

Keep history across restarts with predictable retention.

### Build

- SQLite repository;
- schema migrations;
- XDG paths;
- persisted text items;
- deterministic deduplication/order rules;
- configurable-in-code initial maximum, default around 500 items;
- cleanup of oldest unpinned entries;
- unit and repository tests.

### Visual result

Copy items, restart LionClip/session, reopen the popup, and the history remains.

### Acceptance

- restart-safe history;
- migration path exists from schema version 1 onward;
- database operations do not freeze the popup;
- retention is bounded;
- tests cover ordering, deduplication, retention, and migration basics.

---

## Phase 3 — Polished text UX

**Status: implemented, pending manual validation on the target machine.**

### Goal

Make text history faster and nicer than using a traditional clipboard manager window.

### Build

- instant search;
- strong keyboard focus behavior;
- hover/focus row actions;
- pin/unpin;
- delete item;
- clear history action with safe confirmation/undo strategy as appropriate;
- polished empty state;
- timestamps/metadata kept subtle;
- robust focus-loss behavior;
- visual cleanup against Libadwaita conventions.

### Visual result

`Super+V` should feel like a system popup: type to filter, arrows to navigate, Enter to choose, Escape to dismiss.

### Acceptance

- fully usable without mouse;
- fully usable with mouse;
- search stays responsive with the configured history limit;
- pinned items behave consistently with retention;
- visual hierarchy remains restrained and native.

### Result

Search, pin/unpin, delete and clear-unpinned are implemented as `TextHistory`
operations persisted through the existing worker; schema v1 remained sufficient.
The popup keeps the validated X11 placement and only rebuilds rows while it is
visible. Ordering is pinned-first, then recency, in both groups. Timestamps were
deliberately not added: schema v1 stores logical sequences, and inventing
human-readable times from them would be fake metadata.

---

## Phase 4 — Images and screenshots

**Status: implemented, pending manual validation on the target Zorin GNOME/X11 machine.**

### Goal

Support visual clipboard history without making the popup heavy.

### Build

- detect supported image clipboard formats;
- store image blobs safely;
- thumbnail generation/cache;
- image history rows;
- restore original image content to clipboard;
- blob cleanup tied to history deletion/retention;
- explicit size limits.

### Visual result

Screenshots appear as compact thumbnails mixed into history and can be restored to the clipboard.

### Acceptance

- opening popup does not decode every full-resolution image;
- orphaned blob files are cleaned;
- limits prevent unbounded disk use;
- image contents are never logged.

### Result

Text and images now share one typed history/order/retention model. Phase 4 accepts
PNG and JPEG clipboard payloads, stores exact encoded originals under the private
LionClip XDG data root using SHA-256 content-addressed names, and stores only
metadata/references in SQLite schema v2. The v1 → v2 migration preserves existing
text IDs, exact contents, recency sequences and pin state.

Image processing generates bounded 240×135 thumbnails before list display, so
opening the popup reads only LionClip-generated thumbnail files rather than
decoding every original. Capture keeps the existing event sequence guard and does
validation/thumbnail/blob work off the GTK main path; image restore publishes the
stored compressed MIME bytes before the popup closes and uses typed self-write
suppression to avoid recapture.

The default policies are 500 total unpinned history items, 25 MiB maximum encoded
image size, 16,384 maximum dimension, 50 million maximum pixels and 512 MiB
aggregate image storage. Oldest eligible unpinned images may be evicted to stay
under the byte cap; pinned images are never deleted solely to make room for a new
capture. Delete, clear, retention and startup reconciliation clean unreferenced
originals/thumbnails. No OCR, perceptual matching, image editing or arbitrary MIME
history was added.

---

## Phase 5 — Zorin/GNOME integration and packaging

**Status: implemented and validated from the installed package on the target
Zorin GNOME/X11 machine.** Command routing, single instance, toggle behavior,
desktop integration, clipboard capture and the whole
install/reinstall/upgrade/remove/purge sequence were exercised there; the
remaining checks are the ones that need eyes on the screen or a real keypress,
listed in `docs/PHASE5_VALIDATION.md`.

### Goal

Install and use LionClip like a normal desktop utility.

### Build

- application ID finalized;
- icon and desktop metadata;
- `.desktop` launcher;
- autostart integration;
- documented/setup-friendly `Super+V` shortcut;
- command handling such as `lionclip toggle` if not already complete;
- reproducible build/package process;
- `.deb` for Ubuntu/Zorin;
- GitHub Actions CI for format/lint/test/build if not already present;
- release notes template.

### Visual result

Install package, log in, press `Super+V`, use LionClip without manually starting a terminal process.

### Acceptance

- clean install/uninstall path;
- startup does not spawn duplicate resident instances;
- desktop files validate;
- CI passes from a clean checkout;
- installation instructions are reproducible.

### Result

The final application ID is `io.github.Pianisuto.LionClip`, used by the binary,
the desktop entry, the autostart entry, the metainfo and the icon, with a test
asserting they agree.

The command surface is `lionclip`, `lionclip show`, `lionclip hide`,
`lionclip toggle`, plus `--help` and `--version`, which are answered before GTK
is touched. Commands travel to the single resident instance through GIO's
`HANDLES_COMMAND_LINE`, so no second clipboard monitor can exist and no
hand-written IPC was added. `toggle` on a visible popup hides it instead of
re-showing it, so it never re-places a window that is already on screen.

Desktop integration is an XDG autostart entry running the bare `lionclip`
(resident, no popup), a launcher entry running `lionclip show`, an own lion +
clipboard icon in the hicolor theme from an SVG source, and `lionclip-shortcut`
for a conflict-aware, idempotent `Super+V` binding. No GNOME Shell extension.

Packaging is a small `dpkg-deb` script with runtime dependencies computed by
`dpkg-shlibdeps`. Package removal never deletes `$XDG_DATA_HOME/lionclip`, and
no maintainer script touches a running process. CI additionally builds the
release binary, validates the desktop and AppStream files, smoke-tests the CLI
without a display, builds the `.deb`, asserts its contents and dependencies and
uploads it as an artifact.

Preferences, retention settings, a pause control and a tray icon were left to
Phase 6, and no Flatpak/Snap/AppImage path was added.

---

## Phase 6 — Preferences and privacy controls

**Status: implemented, pending manual validation on the target Zorin
GNOME/X11 machine.** See `docs/PHASE6_VALIDATION.md` for what automated tests
cover (including Xvfb-backed X11 auto-paste integration tests) and the manual
QA checklist for everything that needs a real desktop session.

### Goal

Expose only settings that real usage has proven useful.

### Build

- Libadwaita preferences window;
- history limit;
- retention period if justified;
- save-images toggle;
- start-at-login toggle where practical;
- clear history/data controls;
- pause history control;
- privacy documentation.

### Visual result

A compact native settings window, not a second complex application surface.

### Acceptance

- settings persist correctly;
- changing limits triggers safe cleanup behavior;
- destructive controls are clear;
- no setting exists only for hypothetical future functionality.

### Result

Preferences persist through GSettings (schema
`io.github.Pianisuto.LionClip`, installed to
`/usr/share/glib-2.0/schemas`), not a hand-rolled config file: it needed no
new dependency (`gio` was already linked for application lifecycle), gives
native change validation and atomic dconf-backed writes, and keeps the same
`gsettings`/dconf tooling GNOME itself uses available for support and
debugging. A `build.rs` step compiles the schema into a development copy so
`cargo run`/`cargo test` work before the package is installed, both resolving
to the same dconf path as the real install. If no schema can be found or
compiled anywhere, `SettingsService` falls back to unpersisted in-memory
defaults rather than crashing, the same way a broken database path already
degrades to an in-memory `TextHistory`.

`SettingsService` (`src/settings/`) is the single authority: the popup, the
clipboard/history services and the preferences window all read and write
through it, never touching `gio::Settings` directly. "Start at login" is
deliberately not a GSettings key — its effective state is the presence of a
per-user `~/.config/autostart` override with `Hidden=true` over the
package's system-wide entry, so the filesystem stays the one source of truth
instead of risking a GSettings value that disagrees with it.

History limit changes apply retention immediately through the existing
`TextHistory::enforce_retention` path. The reaction hangs off the GSettings
key rather than off the preferences window, so a `gsettings set
io.github.Pianisuto.LionClip history-limit 100` from a terminal shrinks the
resident history exactly like moving the combo row does — no polling, no
restart, and pinned items untouched either way. A new `clear_all` operation
backs the Preferences "Clear history" action, distinct from the popup's
narrower "Clear Unpinned History…", and both bulk-clear operations bump a
generation counter that in-flight asynchronous captures (image processing in
particular, which can span multiple await points) check before inserting, so
a capture that was already running when a clear ran can no longer make a
just-cleared item reappear. The counter is bumped on every clear *attempt*,
before the check for whether anything was there to remove, so a clear over
an empty (or entirely pinned) history still invalidates captures already in
flight; the return value still reports that nothing was removed. `save_images` and `recording_paused` are read
directly by the clipboard capture handler on every clipboard change, with no
caching and no signal-wiring layer to keep in sync; a paused handler does no
work at all, not even inspecting offered MIME types.

Auto-paste ("automatically paste selected items into the app you were using
before LionClip", default off) is implemented as `PasteCoordinator`
(`src/paste/`), shaped like `Positioner` on purpose: a concrete backend
selected once from session diagnostics rather than a trait object, since
there is exactly one real backend (X11) and one degenerate case. It extends
the existing isolated `x11rb` usage instead of a second X11 stack, and uses
`x11rb`'s `xtest` Cargo feature for key synthesis — no `xdotool`/`ydotool`
runtime dependency. The sequence is fail-safe end to end: the paste target
is captured once, when the popup is shown and confirmed not visible yet
(never derived from whatever is focused after the popup closes); only
`Enter`/click activation on a history row can trigger a paste attempt, never
navigation, pin, delete, search, menu or Preferences; the clipboard restore
must complete and report success before any paste is attempted; the target's
existence is re-checked at paste time; activation — through both
`_NET_ACTIVE_WINDOW` and a direct `SetInputFocus` reinforcement — is
requested only when the target does not already hold the focus, and is
abandoned outright when some third window has taken it, because pulling
focus away from whatever the user moved on to would be worse than not
pasting; key synthesis runs only after focus is confirmed from real X server
state (a `GetInputFocus` reply already naming the target, or a `FocusIn`
event saying it just gained it), waited for with a bounded (~400 ms)
non-blocking poll rather than a blind sleep, and that confirmation is
re-checked once more immediately before the keys go out; and every failure
path — invalid/destroyed target, foreign focus, disabled setting, failed
restore, unconfirmed focus — falls back to restore-only. The final re-check
narrows the inherent check-then-act window to a single round trip; X offers
no atomic "send this key only if window W still has focus", so it cannot
close it. On Wayland the setting persists but the switch is disabled with an
explanatory subtitle, and selecting an item only restores the clipboard,
exactly like the setting being off.

Deliberately left out: retention by wall-clock days (schema v2 has no real
timestamps, only logical sequences, and a migration solely to support a
slider was not justified); a second "delete all data" action distinct from
"clear history" (clearing history already removes the database contents,
blobs and thumbnails, and deleting the user's own preferences while the
process that reads them is still running would be confusing for no real
benefit); and exposing the 512 MiB aggregate image storage cap as a setting
(it remains a fixed technical safety backstop, as it was in Phase 4).

---

## Phase 7 — Performance and optimization

**Status: implemented; measured on the target Zorin GNOME/X11 machine.** No
feature, UX, persistence or security behavior changed.

### Goal

Make the existing application faster by removing redundant work, without
adding anything or altering behavior.

### Result

Profiling the real popup path (release build, 500 seeded text items, on the
target session) found one dominant cost that had nothing to do with the list
being long: `gtk_widget_set_tooltip_text` measured **8.3 ms per call**, because
it always ends in `gtk_widget_trigger_tooltip_query`, which asks the display
for the pointer position and the surface under it. Each history row set two
tooltips, so every row cost ~16.6 ms to build regardless of anything LionClip
does. Rows now declare `has-tooltip` and answer `query-tooltip`, which shows
the same tooltip on hover and sets the same accessible description, without the
immediate query — a query that had nothing to re-evaluate anyway, since the
widget is not in a window yet.

Placement stopped opening a fresh X connection per call. It still does not
share GTK's display connection — that separation is what keeps a pending unmap
from being processed after the move — but the connection is now kept between
calls instead of being rebuilt twice per popup open and again on every
activation change. Its four independent queries (pointer, geometry, monitors,
size hints) are issued before any reply is read, and the two writes are queued
before either is checked, so a placement costs one round trip rather than
seven.

The remaining work was redundancy: clearing history compared every id against
every item (quadratic, one million comparisons at the 1000-item limit); a
retention pass recompiled the same `DELETE` once per removed row; every image
capture allocated and zeroed a 25 MiB buffer whatever the screenshot's actual
size, because `read_all` needs a buffer as large as the biggest payload it will
accept; and every history mutation walked the whole history to build an image
reference set that was almost always thrown away unused.

### Measured, 500 items, same machine and build profile

| Popup open phase | Before | After |
| --- | --- | --- |
| `prepare` (build rows) | 8042 ms | 41 ms |
| `place` (X11 placement) | 3.21 ms | 0.14 ms |
| `present` (GTK layout) | 456 ms | 482 ms |
| **total** | **8502 ms** | **522 ms** |

`present` is unchanged within run-to-run noise, and is now the whole cost.

### Known remaining bottleneck

`present` is `GtkListBox` measuring every row: it is not a virtualized list, so
laying out 500 non-uniform wrapped rows is O(n) by construction, about 0.9 ms
per row. Removing it means replacing `GtkListBox` with `GtkListView` and a
list model, which rewrites selection, keyboard navigation, row actions and
index handling — all behavior Phase 3 deliberately pinned down. That is a
scoped change of its own, not a Phase 7 optimization.

Pinning `width_chars` on the preview label makes layout 3× faster (462 ms →
155 ms) but widens the popup from 430 px to 480 px, so it was rejected: Phase 7
does not change UX.

---

## Post-V1 ideas — not committed

Evaluate only after V1 is stable:

- additional MIME types;
- app-specific exclusion if source-app identification can be reliable and privacy-preserving;
- alternate desktop/compositor positioning backends;
- Flatpak feasibility;
- optional direct-paste behavior only if there is a safe, reliable desktop API.

Cloud sync, accounts, AI, OCR, general scripting, and a plugin marketplace are not part of the current product direction.
