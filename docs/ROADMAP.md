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

---

## Post-V1 ideas — not committed

Evaluate only after V1 is stable:

- additional MIME types;
- app-specific exclusion if source-app identification can be reliable and privacy-preserving;
- alternate desktop/compositor positioning backends;
- Flatpak feasibility;
- optional direct-paste behavior only if there is a safe, reliable desktop API.

Cloud sync, accounts, AI, OCR, general scripting, and a plugin marketplace are not part of the current product direction.
