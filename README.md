# LionClip

A small, fast, native clipboard history utility for Linux desktops, initially focused on **GNOME/Zorin OS**.

LionClip is being built around one simple interaction:

> Press `Super+V`, get a clean clipboard history popup near the pointer, choose an item, and continue working.

The project intentionally avoids becoming a large automation or scripting platform. The goal is to provide the clipboard-history experience that should feel native to the desktop: fast, predictable, keyboard-friendly, and visually consistent with GNOME.

## Status

**Early development / technical validation.**

Phase 0 now contains a technical spike for the desired popup experience. Native
GNOME Wayland uses a safe compositor-managed fallback, while an isolated
X11/XWayland experiment attempts pointer-relative placement with monitor-edge
clamping. The experiment still requires classification on the primary Zorin
machine before the positioning approach is considered validated.

Do not expect a usable release yet.

See [`docs/PHASE0_VALIDATION.md`](docs/PHASE0_VALIDATION.md) for native build
dependencies, positioning diagnostics, and the exact Zorin Wayland/XWayland
manual test matrix.

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
- **x11rb** — X11/XWayland pointer/window experiments where needed

The exact positioning strategy is deliberately not considered settled until the technical spike is validated on a real Zorin OS Wayland session.

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
│   ├── X11/XWayland experiment
│   └── safe Wayland fallback
└── Settings / application lifecycle
    └── GApplication, autostart and preferences
```

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) and [`docs/ROADMAP.md`](docs/ROADMAP.md) once the repository bootstrap is complete.

## Development workflow

Development is intentionally incremental. Each phase must leave behind something visible and testable on the target desktop before the next phase begins.

Agents and contributors should read [`AGENTS.md`](AGENTS.md) before changing the project. Claude-based agents should also read [`CLAUDE.md`](CLAUDE.md).

The implementation roadmap and copy-paste prompts for coding agents live in [`docs/CODEX_PROMPTS.md`](docs/CODEX_PROMPTS.md).

For the current Phase 0 spike, install the native packages and run the required
Rust checks as documented in
[`docs/PHASE0_VALIDATION.md`](docs/PHASE0_VALIDATION.md).

## Target environment

Primary validation environment:

- Zorin OS based on Ubuntu 24.04 (`noble`)
- GNOME desktop
- Wayland session, with XWayland available where the system provides it

Support for other Linux desktops is welcome later, but must not compromise the small and reliable V1 for the primary environment.

## Contributing

The project is very early. Before opening a large implementation PR, please read [`CONTRIBUTING.md`](CONTRIBUTING.md) and keep changes aligned with the current roadmap phase.

## License

A license has not been selected yet. Until a license file is added, the repository being public does **not** grant permission to copy, modify, or redistribute the code beyond what GitHub's Terms of Service require for viewing and forking on the platform.
