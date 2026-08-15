# Coding Agent Prompts

These prompts are intentionally phase-scoped. Use **one prompt at a time**, review/test the result on the target machine, then proceed.

Every prompt assumes the agent has repository access and must read `AGENTS.md`, `docs/ARCHITECTURE.md`, and `docs/ROADMAP.md` first.

## Prompt 0 — Technical spike: popup placement

```text
Implement LionClip Roadmap Phase 0 only.

Read AGENTS.md, docs/ARCHITECTURE.md and docs/ROADMAP.md before changing anything.

Goal: prove the riskiest technical requirement on the primary Zorin OS / GNOME X11 target before implementing clipboard history: a small GTK4 + Libadwaita popup that opens near the current pointer, while retaining a reliable fallback when exact placement is unavailable on secondary backends.

Requirements:
- initialize a clean Rust stable project using GTK4/gtk4-rs and Libadwaita/libadwaita-rs;
- keep the architecture minimal: no SQLite, clipboard history, image support, settings, extension, daemon, or unrelated features yet;
- create a small visually polished test popup using native Libadwaita styling;
- detect/report relevant session/backend diagnostics without sensitive data;
- isolate positioning behavior behind a small boundary so X11/XWayland-specific code does not leak into UI code;
- implement the smallest viable isolated X11 pointer/placement backend and retain the same boundary for XWayland experiments if supported;
- clamp placement so the popup does not intentionally open off-screen;
- if pointer-relative placement cannot be guaranteed, implement a predictable fallback rather than failing;
- do not add a GNOME Shell extension unless you can demonstrate why the simpler approaches are insufficient; if it cannot be validated in your environment, leave it out;
- add only useful dependencies;
- add basic tests for pure positioning/clamping logic where possible;
- add CI for cargo fmt, clippy and tests if the project can run those checks headlessly;
- document exact commands to install native build dependencies on Ubuntu/Zorin noble.

Before finishing run cargo fmt --check, cargo clippy --all-targets --all-features -- -D warnings, cargo test, and cargo build (or explain any environment-specific blocker).

Deliver:
1. working Phase 0 code;
2. concise documentation of the positioning strategy actually implemented;
3. exact manual test steps for the primary Zorin GNOME/X11 machine and optional secondary-backend checks;
4. what output/log line tells me which positioning backend/fallback was used;
5. known limitations and what I should report back after testing.

Do not implement Phase 1.
```

## Prompt 1 — Text clipboard history in memory

```text
Implement LionClip Roadmap Phase 1 only, using the validated Phase 0 positioning behavior already in the repository.

Read AGENTS.md, docs/ARCHITECTURE.md, docs/ROADMAP.md, and the existing Phase 0 implementation first. Do not replace a validated positioning approach without a concrete reason.

Goal: make LionClip usable for text clipboard history during the current process lifetime.

Requirements:
- monitor clipboard changes using event-driven GTK/GDK APIs where viable; no aggressive polling loop;
- capture UTF-8 text from normal copy operations across representative desktop apps;
- keep history in memory only; do not add SQLite yet;
- normalize history items through a typed domain model;
- prevent obvious consecutive/repeated duplicate noise and move/reuse an existing logical text item according to a simple documented rule;
- display recent text items in the existing popup with compact previews;
- Up/Down changes selection, Enter restores the selected text to the clipboard and closes, Escape closes;
- clicking an item performs the same restore-and-close behavior;
- do not synthesize Ctrl+V or implement auto-paste;
- do not log clipboard contents;
- keep clipboard reading asynchronous/non-blocking where the APIs require it;
- unit test normalization/dedup/order logic;
- preserve Phase 0 fallback behavior.

Before finishing run fmt, clippy, tests and build.

Deliver a short manual test matrix covering at least terminal, browser and code editor on Zorin, including what to report if Wayland/XWayland clipboard behavior differs.

Do not implement persistence, images, pinning, settings or Phase 2+ features.
```

## Prompt 2 — SQLite persistence

```text
Implement LionClip Roadmap Phase 2 only.

Read AGENTS.md, docs/ARCHITECTURE.md, docs/ROADMAP.md and inspect the current text-history implementation before editing.

Goal: persist text history across application/session restarts without making the popup slower or changing the established UX.

Requirements:
- add SQLite persistence with explicit versioned migrations starting at schema v1;
- use XDG data directories and respect XDG_DATA_HOME overrides;
- persist the typed text history model and restore it on startup;
- define and document deterministic deduplication/order semantics;
- default to a bounded history around 500 non-pinned items, implemented as a clear policy even though pinning UI arrives later;
- clean oldest eligible items when limits are exceeded;
- do database work so the GTK UI is not blocked by avoidable I/O;
- never store diagnostic logs containing clipboard payloads;
- add repository tests using temporary databases, including migration creation, restart persistence, dedup/order and retention;
- do not add image/blob storage yet.

Keep schema and abstractions as small as the current requirements permit.

Run fmt, clippy, tests and build. Provide exact manual steps to verify that copied text survives a full LionClip restart.

Do not implement Phase 3 UI polish beyond what is necessary to expose persisted history.
```

## Prompt 3 — Polished text UX

```text
Implement LionClip Roadmap Phase 3 only.

Read AGENTS.md and the current implementation first. Preserve the validated architecture and platform behavior.

Goal: make the text-history popup feel like a native, fast GNOME system surface rather than a traditional application window.

Requirements:
- add instant search/filtering with focus in the search field on open;
- make keyboard navigation robust: Up/Down, Enter, Escape, Delete where appropriate;
- support mouse selection equivalently;
- add pin/unpin and delete actions with restrained hover/focus affordances;
- add a safe clear-history action;
- ensure pinned items are consistent with retention rules;
- provide polished empty and no-results states;
- keep metadata/timestamps subtle;
- close reliably after restoring an item and define sensible focus-loss behavior;
- stay close to GTK4/Libadwaita conventions; no custom rainbow palette, sidebar, permanent toolbar or heavy chrome;
- keep approximately the compact popup dimensions documented in AGENTS.md unless usability testing gives a reason to adjust;
- keep search responsive at the configured history limit;
- add tests for search, pin/delete/clear policies where those can be separated from GTK widgets.

Do not add images, packaging, cloud features or unrelated settings.

Run fmt, clippy, tests and build. Provide a focused visual/manual QA checklist for me to execute on Zorin.
```

## Prompt 4 — Images and screenshots

```text
Implement LionClip Roadmap Phase 4 only.

Read AGENTS.md, docs/ARCHITECTURE.md and the existing storage/history code first.

Goal: add image/screenshot clipboard history while keeping popup startup and scrolling lightweight.

Requirements:
- detect a conservative set of image clipboard formats supported reliably by GTK/GDK;
- add a typed image history item rather than overloading text fields;
- store original image/blob data under the LionClip XDG data directory using content-addressed or otherwise collision-safe names;
- persist only appropriate metadata/references in SQLite;
- generate/cache thumbnails for list display and avoid decoding every original image when the popup opens;
- restore the original supported image content to the clipboard when selected;
- set explicit per-image and aggregate/retention limits; document the chosen defaults;
- delete orphaned blob/thumbnail files when items are removed or aged out;
- never log image bytes;
- add tests for blob lifecycle, retention and metadata where feasible;
- keep text behavior unchanged.

Do not add OCR, AI, image editing, sync or arbitrary MIME-type support.

Run fmt, clippy, tests and build. Provide manual QA steps for screenshots copied from the target Zorin desktop and at least one browser image copy flow.
```

## Prompt 5 — Desktop integration, packaging and CI

```text
Implement LionClip Roadmap Phase 5 only.

Read AGENTS.md and inspect the current working application first.

Goal: make LionClip installable and launchable as a normal Zorin/GNOME desktop utility.

Requirements:
- finalize a reverse-DNS application ID suitable for the project and use it consistently;
- add desktop metadata, icon placeholders/assets strategy, .desktop integration and any required metainfo;
- implement/start using single-instance command handling so a shortcut can invoke `lionclip toggle` without spawning duplicate resident monitors;
- add autostart integration appropriate for Ubuntu/Zorin and document how it works;
- provide a reproducible `Super+V` setup path for GNOME/Zorin; automate only what is safe and reversible;
- provide a reproducible .deb build/install/uninstall workflow for Ubuntu/Zorin noble;
- add/update GitHub Actions to run fmt, clippy, tests and a clean Linux build with pinned/reasonable system dependencies;
- validate desktop metadata in CI when practical;
- update README installation/development sections;
- keep packaging scripts simple and reviewable.

Do not add Flatpak as the primary package in this phase unless the roadmap is explicitly changed after investigating clipboard/global-shortcut sandbox constraints.

Run all local checks available. Deliver clean-install, upgrade/reinstall and uninstall manual test instructions for my Zorin machine.
```

## Prompt 6 — Preferences and privacy controls

```text
Implement LionClip Roadmap Phase 6 only.

Read AGENTS.md and inspect real settings/policies already present in code before adding UI.

Goal: expose a small native Libadwaita preferences window containing only proven useful controls.

Requirements:
- use Libadwaita preference components;
- expose history limit;
- expose retention period only if the current history policy supports it cleanly;
- expose save-images toggle;
- expose start-at-login toggle if integration can be changed reliably;
- add pause/resume history recording;
- add clear-history/data controls with clear destructive semantics;
- persist settings in an appropriate local mechanism and migrate safely if needed;
- changing limits must trigger safe retention cleanup;
- preserve privacy: no telemetry and no network requirement;
- do not add placeholder settings for hypothetical features;
- add tests for settings/policy interactions where practical.

Run fmt, clippy, tests and build. Provide manual QA steps for each setting and verify that the preferences window remains visually small and native.
```

## Review prompt after each phase

After a phase is implemented and manually tested, this prompt can be used in a separate agent/thread:

```text
Review the current LionClip PR as a strict maintainer. Read AGENTS.md, docs/ARCHITECTURE.md and the roadmap phase targeted by this PR.

Look for correctness bugs, GNOME/Wayland/X11 assumptions, clipboard privacy leaks, GTK main-thread blocking, unnecessary dependencies, Rust error-handling problems, lifecycle/single-instance bugs, scope creep, weak tests, UI inconsistencies and performance regressions.

Do not propose unrelated future features. Separate findings into blocking, important and optional. For every blocking/important finding, cite the relevant file/function and propose the smallest concrete fix. End with whether the PR is ready for manual validation/merge and the exact tests you would run.
```
