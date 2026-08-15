# LionClip

A small, fast, native clipboard history utility for Linux desktops, initially focused on **GNOME/Zorin OS**.

LionClip is being built around one simple interaction:

> Press `Super+V`, get a clean clipboard history popup near the pointer, choose an item, and continue working.

The project intentionally avoids becoming a large automation or scripting platform. The goal is to provide the clipboard-history experience that should feel native to the desktop: fast, predictable, keyboard-friendly, and visually consistent with GNOME.

## Status

**Early development / Phase 0 complete.**

Phase 0 validated pointer-relative popup placement on the real target machine:
Zorin OS with GNOME/X11. Native GNOME Wayland uses a safe compositor-managed
fallback because exact top-level placement is unavailable through the current
approach. XWayland inside a Wayland session remains experimental and has not
yet been validated.

Do not expect a usable release yet.

See [`docs/PHASE0_VALIDATION.md`](docs/PHASE0_VALIDATION.md) for native build
dependencies, the recorded Phase 0 result, positioning diagnostics, and the
optional Wayland/XWayland test matrix.

## Product principles

- **Fast by default** — near-zero idle CPU and an effectively instant popup.
- **Native UI** — GTK4 + Libadwaita, following the system light/dark appearance.
- **Keyboard first** — `Super+V`, arrows, Enter, Escape, search as you type.
- **Mouse friendly** — the popup should appear near the pointer whenever the platform allows it.
- **Private and local** — clipboard history stays on the device.
- **Small scope** — clipboard history first; no cloud, accounts, AI, scripting, or plugin system in V1.
- **GNOME-aware, not GNOME-bound** — isolate compositor/session-specific behavior behind small platform backends.

## Planned stack

- **Rust** — application and domain logic
- **GTK4 / gtk4-rs** — UI and clipboard integration
- **Libadwaita / libadwaita-rs** — GNOME-native visuals and preferences
- **GLib / GIO** — application lifecycle and single-instance command handling
- **SQLite** — local clipboard history persistence
- **x11rb** — validated X11 positioning and isolated XWayland experiments

The V1 positioning strategy is settled for the primary GNOME/X11 target. The
Wayland fallback and experimental XWayland path remain isolated so they can
improve without destabilizing validated X11 behavior.

## Intended V1

- text clipboard history;
- local persistence;
- deduplication;
- `Super+V` launcher integration;
- popup history UI;
- instant search;
- keyboard navigation;
- pin/delete/clear actions;
- image and screenshot history;
- autostart;
- basic preferences;
- graceful fallback when pointer-relative positioning is unavailable.

## Non-goals for V1

- cloud sync;
- accounts;
- AI features;
- OCR;
- scripting;
- plugin systems;
- clipboard sharing between machines;
- automatic keystroke injection / auto-paste;
- becoming a general-purpose launcher.

## Architecture

The application is planned as a single resident process with a small number of explicit responsibilities:

```text
LionClip
├── Clipboard service
│   └── observe / read / write clipboard content
├── History service
│   └── normalize, deduplicate, persist and query items
├── Popup UI
│   └── GTK4 + Libadwaita presentation and interaction
├── Positioning backend
│   ├── validated X11 placement
│   ├── experimental XWayland placement
│   └── compositor-managed Wayland fallback
└── Settings / application lifecycle
    └── GApplication, autostart and preferences
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md) once the repository bootstrap is complete.

## Development workflow

Development is intentionally incremental. Each phase must leave behind something visible and testable on the target desktop before the next phase begins.

Agents and contributors should read [`AGENTS.md`](AGENTS.md) before changing the project. Claude-based agents should also read [`CLAUDE.md`](CLAUDE.md).

The implementation roadmap and copy-paste prompts for coding agents live in [`docs/CODEX_PROMPTS.md`](docs/CODEX_PROMPTS.md).

For the completed Phase 0 spike, install the native packages and review the
recorded platform result as documented in
[`docs/PHASE0_VALIDATION.md`](docs/PHASE0_VALIDATION.md).

## Target environment

Primary validation environment:

- Zorin OS based on Ubuntu 24.04 (`noble`)
- GNOME desktop
- X11 session

Native Wayland remains a supported fallback environment. XWayland inside a
Wayland session remains experimental and is not a V1 validation requirement.

Support for other Linux desktops is welcome later, but must not compromise the small and reliable V1 for the primary environment.

## Contributing

The project is very early. Before opening a large implementation PR, please read [`CONTRIBUTING.md`](CONTRIBUTING.md) and keep changes aligned with the current roadmap phase.

## License

A license has not been selected yet. Until a license file is added, the repository being public does **not** grant permission to copy, modify, or redistribute the code beyond what GitHub's Terms of Service require for viewing and forking on the platform.
