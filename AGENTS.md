# AGENTS.md

This file defines the working contract for coding agents operating in this repository.

## 1. Product mission

LionClip is a small, native clipboard-history utility for Linux, initially optimized for Zorin OS / GNOME.

The core interaction is intentionally narrow:

1. the user copies content normally;
2. LionClip records supported clipboard content locally;
3. `Super+V` opens a clean history popup;
4. the popup should appear near the pointer when the platform permits it;
5. the user chooses an item by mouse or keyboard;
6. LionClip places that item back on the clipboard and closes.

Do not turn LionClip into a launcher, automation framework, AI tool, cloud service, or scripting platform unless a future roadmap explicitly says so.

## 2. Primary target

The primary validation environment is:

- Zorin OS based on Ubuntu 24.04 (`noble`);
- GNOME desktop;
- Wayland session;
- XWayland may be present and may be used only behind an isolated platform backend if technically justified.

Do not claim Wayland behavior is solved without validating it on the real target environment.

## 3. Technology direction

Unless a roadmap phase explicitly changes this decision, use:

- Rust stable;
- GTK4 through `gtk4-rs`;
- Libadwaita through `libadwaita-rs`;
- GLib/GIO for application lifecycle and single-instance behavior;
- GDK clipboard APIs for clipboard interaction where viable;
- SQLite for persistence once persistence is introduced;
- `x11rb` only for explicitly isolated X11/XWayland positioning experiments or backend code.

Avoid Electron, Tauri, webviews, Node runtimes, Python daemons, or a second UI toolkit.

## 4. Architectural rules

Keep the application small and explicit. Prefer one resident application process.

Expected responsibilities:

- `clipboard`: observe, read, normalize, and write clipboard content;
- `history`: model, deduplicate, persist, query, and expire history;
- `popup`: presentation and interaction only;
- `positioning`: platform/session-specific popup placement;
- `settings`: preferences and policy;
- `app`: lifecycle, commands, single-instance behavior, orchestration.

Important boundaries:

- UI code must not directly own persistence rules.
- Persistence code must not know GTK widgets.
- Platform-specific positioning must not leak throughout the UI.
- Clipboard callbacks should do minimal work on the UI thread.
- Do not introduce abstractions merely to satisfy patterns. Add an interface/trait when there are genuinely multiple behaviors, platform backends, or a test seam that materially helps.
- Do not build infrastructure for hypothetical future features.

## 5. Wayland/X11 rule

Popup positioning is the main technical risk.

Treat all pointer-relative positioning approaches as experimental until Phase 0 is validated.

The desired order is:

1. use a reliable native mechanism if one exists for the active backend;
2. test an isolated X11/XWayland positioning backend if appropriate;
3. provide a safe fallback, such as showing on the active monitor in a predictable position;
4. only consider a GNOME-specific helper/extension if the first three approaches cannot satisfy the product requirement.

Never scatter `GDK_BACKEND`, X11 calls, compositor assumptions, or shell-specific code across application modules.

## 6. UX contract

LionClip should feel like a small system surface, not a traditional desktop application.

Default popup direction:

- approximately 420–440 px wide;
- compact height based on content, capped around 500 px;
- rounded, restrained Libadwaita presentation;
- search field at the top;
- no permanent toolbar;
- no sidebar;
- no status bar;
- no unnecessary color coding;
- system light/dark appearance;
- system accent where appropriate;
- native typography and spacing.

Expected keyboard behavior:

- `Super+V`: request popup toggle through desktop shortcut integration;
- `Up` / `Down`: move selection;
- `Enter`: restore selected item to clipboard and close;
- `Escape`: close;
- typing while search is focused: filter immediately;
- `Delete`: remove selected history item when that feature exists.

The UI must remain usable without a mouse.

## 7. Performance contract

Treat performance as a feature.

Targets:

- idle CPU should be effectively zero;
- no clipboard polling loop when event-driven APIs can be used;
- popup opening should feel instantaneous;
- do not load full-size image data for every row when only thumbnails are visible;
- database work must not block rendering;
- avoid unbounded history or caches.

Do not add timers, polling, background loops, or broad subscriptions without explaining why an event-driven alternative is insufficient.

## 8. Privacy and security

Clipboard data is sensitive.

Rules:

- store history locally only;
- no telemetry by default;
- no network dependency for core functionality;
- never log clipboard contents in normal logs;
- diagnostic logs should describe types/sizes/events, not sensitive payloads;
- do not implement auto-paste by synthesizing keyboard events in V1;
- keep persistence paths inside standard XDG user directories;
- pinned and retained content must be deletable by the user.

If a new feature creates a privacy risk, document it before implementation.

## 9. Scope discipline

Before implementing anything, read:

1. this file;
2. `docs/ARCHITECTURE.md`;
3. `docs/ROADMAP.md`;
4. the current task/issue/PR context.

Implement only the current vertical slice.

Do not opportunistically add:

- cloud sync;
- accounts;
- OCR;
- AI;
- plugin systems;
- shell scripting engines;
- clipboard sync between machines;
- content transformation actions;
- automatic paste injection;
- unrelated settings.

If you notice a useful future improvement, mention it in the PR summary instead of silently expanding scope.

## 10. Coding standards

For Rust code:

- keep `cargo fmt --check` clean;
- keep `cargo clippy --all-targets --all-features -- -D warnings` clean unless a documented platform limitation requires a narrowly-scoped exception;
- keep tests deterministic;
- prefer typed domain models over stringly-typed state;
- handle errors explicitly; avoid `unwrap()`/`expect()` in runtime paths unless the invariant is truly impossible to violate and explained;
- use structured errors at boundaries that benefit from context;
- keep unsafe code out unless unavoidable for a platform API, then isolate and document it;
- prefer small modules with clear ownership over giant controller files.

Do not add dependencies casually. For every new dependency, verify that it is maintained, suitable for Linux desktop use, and materially reduces complexity.

## 11. Testing strategy

Each vertical slice needs the smallest meaningful set of tests.

Expected layers as the project grows:

- unit tests for normalization, deduplication, history ordering, retention, and search rules;
- repository/database tests against a temporary SQLite database;
- integration tests for application commands where practical;
- manual UI validation checklist for GNOME/Zorin behavior that cannot be reliably automated;
- focused platform tests for positioning backends.

Do not pretend a headless CI test proves compositor behavior.

When a feature depends on the actual desktop session, provide exact manual validation steps in the PR.

## 12. Agent workflow

For each requested implementation:

1. inspect the repository and current roadmap phase;
2. state the narrow implementation plan in the work log/response;
3. implement the smallest complete vertical slice;
4. run formatting, linting, and relevant tests;
5. run/build the application when the environment permits it;
6. document anything that could only be validated manually;
7. review your own diff for scope creep and dead code;
8. update docs only when behavior or architecture actually changed;
9. produce a concise final summary with:
   - what changed;
   - key design choices;
   - commands/tests run;
   - manual validation steps;
   - known limitations;
   - suggested next roadmap phase.

## 13. Pull request policy

Prefer one roadmap phase or one coherent vertical slice per PR.

PRs should:

- have a clear user-visible or technically verifiable outcome;
- avoid unrelated refactors;
- include tests for non-UI logic;
- include manual test steps for UI/platform behavior;
- call out Wayland/X11 assumptions explicitly;
- not mark experimental behavior as production-ready.

## 14. Definition of done

A task is not done merely because it compiles.

It is done when:

- the requested behavior is implemented;
- relevant automated checks pass;
- the implementation respects architecture and privacy boundaries;
- there is no obvious scope creep;
- the user can validate the feature with clear steps;
- known platform limitations are stated accurately.
