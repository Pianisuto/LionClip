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

---

## Phase 4 — Images and screenshots

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

---

## Phase 5 — Zorin/GNOME integration and packaging

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
