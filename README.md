# LionClip

A small, fast, native clipboard history utility for Linux desktops, initially focused on **GNOME/Zorin OS**.

LionClip is being built around one simple interaction:

> Press `Super+V`, get a clean clipboard history popup near the pointer, choose an item, and continue working.

The project intentionally avoids becoming a large automation or scripting platform. The goal is to provide the clipboard-history experience that should feel native to the desktop: fast, predictable, keyboard-friendly, and visually consistent with GNOME.

## Install

LionClip ships as a `.deb` for Ubuntu 24.04 and Zorin OS based on `noble`,
`amd64`. You do not need Rust or Cargo to install it.

Download `lionclip_<version>_amd64.deb`, or build it yourself (see
[Development](#development)), then:

```bash
sudo apt install ./lionclip_0.1.0_amd64.deb
```

`apt` pulls in the GTK4, Libadwaita and GDK-Pixbuf libraries LionClip needs.
The package installs:

| Path | What it is |
| --- | --- |
| `/usr/bin/lionclip` | the application |
| `/usr/bin/lionclip-shortcut` | the `Super+V` setup helper |
| `/usr/share/applications/io.github.Pianisuto.LionClip.desktop` | app launcher entry |
| `/etc/xdg/autostart/io.github.Pianisuto.LionClip.desktop` | starts LionClip at login |
| `/usr/share/icons/hicolor/*/apps/io.github.Pianisuto.LionClip.{svg,png}` | icon |
| `/usr/share/metainfo/io.github.Pianisuto.LionClip.metainfo.xml` | AppStream metadata |
| `/usr/share/doc/lionclip/` | `README.Debian`, changelog, copyright |

## First setup

**Autostart** is already configured: the installed autostart entry runs
`lionclip` at each login, which starts the resident instance and its clipboard
monitor *without* opening the popup. Log out and back in once after installing,
or start it now with:

```bash
setsid lionclip >/dev/null 2>&1 &
```

**`Super+V`** is not bound automatically. The helper does it, and refuses to
take a shortcut away from anything without being asked:

```bash
lionclip-shortcut install
```

If GNOME still uses `Super+V` for its notification list, the helper says so and
stops. Re-run it as `lionclip-shortcut install --take-over` to hand the key
over, or bind LionClip to another key yourself in
*Settings → Keyboard → Keyboard Shortcuts → Custom Shortcuts* with the command
`lionclip toggle`.

Check or undo it any time:

```bash
lionclip-shortcut status
lionclip-shortcut remove
```

## Usage

Press `Super+V`. The popup opens near the pointer with the newest item
selected; press `Super+V` again to close it.

- **search** — just type; the list filters as you type
- **navigate** — `Up`/`Down`, or the mouse
- **restore** — `Enter` or click, then paste normally with `Ctrl+V`
- **dismiss** — `Escape` (clears a non-empty search first), or click away
- **pin** — `Ctrl+P`, or the pin button on the row; pinned items stay on top and
  are never dropped by the history limit
- **delete** — `Delete` while a row has focus
- **clear** — the overflow menu next to the search field clears unpinned items
- **images** — screenshots and copied PNG/JPEG images appear as thumbnails and
  are restored as the original image

From a terminal or a script:

```bash
lionclip           # start the resident instance, no popup
lionclip show      # show the popup
lionclip hide      # hide the popup, keep running
lionclip toggle    # show it when hidden, hide it when visible
```

Every invocation talks to the one resident instance, so there is never a second
clipboard monitor.

## Upgrade

```bash
sudo apt install ./lionclip_<newer-version>_amd64.deb
```

Installing over an existing version replaces the files on disk but does not
restart a LionClip that is already running — the running process keeps the code
it started with. Log out and back in, or restart it explicitly:

```bash
pkill -x lionclip && setsid lionclip >/dev/null 2>&1 &
```

Reinstalling the same version works the same way.

## Uninstall

Drop the `Super+V` binding first, while the helper is still installed —
otherwise the shortcut stays behind in GSettings pointing at a command that no
longer exists:

```bash
lionclip-shortcut remove
sudo apt remove lionclip
```

`remove` takes the program, the launcher entry, the icon and the autostart entry
with it. `sudo apt purge lionclip` additionally drops the package's own
bookkeeping; there is nothing else left in `/etc` for it to clean.

Neither touches your clipboard history.

## Remove personal data

Your history lives in `$XDG_DATA_HOME/lionclip`, normally
`~/.local/share/lionclip`: the SQLite database and the stored images. Package
removal deliberately leaves it alone. Delete it yourself when you want it gone:

```bash
rm -rf ~/.local/share/lionclip
```

## Status

**Early development / Phase 5 implemented.**

Phase 0 validated pointer-relative popup placement on the real target machine:
Zorin OS with GNOME/X11. Native GNOME Wayland uses a safe compositor-managed
fallback because exact top-level placement is unavailable through the current
approach. XWayland inside a Wayland session remains experimental and has not
yet been validated. Text and image clipboard history is event-driven,
exact-content deduplicated, bounded, and persisted locally in SQLite across
restarts.

The popup behaves like a small system surface: type to search instantly, arrows
to navigate, `Enter` to restore, `Escape` to clear the search and then dismiss,
`Delete` to remove an item, `Ctrl+P` to pin, and a restrained overflow menu to
clear unpinned history. Pinned items are kept first and are exempt from the
retention limit.

There are no preferences yet: history limits, retention and a pause control are
Phase 6.

See [`docs/PHASE0_VALIDATION.md`](docs/PHASE0_VALIDATION.md) for native build
dependencies, the recorded Phase 0 result, positioning diagnostics, and the
optional Wayland/XWayland test matrix, and
[`docs/PHASE5_VALIDATION.md`](docs/PHASE5_VALIDATION.md) for the desktop
integration and packaging test script.

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

## Development

Build dependencies on Ubuntu/Zorin `noble`:

```bash
sudo apt install build-essential libadwaita-1-dev libgtk-4-dev libx11-dev pkg-config
```

Rust stable, then:

```bash
cargo build
cargo test
cargo run -- show
```

`cargo run` with no arguments starts the resident instance without a popup, the
same as autostart does.

Build the package (needs `dpkg-dev`, and `librsvg2-bin` or
`gdk-pixbuf-thumbnailer` to rasterize the icon):

```bash
packaging/deb/build.sh
```

It writes `target/deb/lionclip_<version>_amd64.deb`, taking the runtime
dependencies from `dpkg-shlibdeps` reading the built binary. Everything the
package installs comes from [`packaging/`](packaging): the desktop entry, the
autostart entry, the AppStream metainfo, the icon source and the maintainer
scripts.

Before pushing:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
cargo build --release
```

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
