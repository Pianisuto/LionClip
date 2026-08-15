# Contributing to LionClip

LionClip is in early development. Small, focused contributions are preferred.

## Before contributing

Read:

- `AGENTS.md`
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`

Check which roadmap phase is currently active before starting a large change.

## Development principles

- Keep the product small.
- Preserve clipboard privacy.
- Prefer event-driven behavior over polling.
- Keep platform-specific code isolated.
- Do not claim compositor-specific behavior works without testing it.
- Follow GTK4/Libadwaita conventions rather than introducing a custom visual system.
- Avoid unrelated refactors in feature PRs.

## Rust checks

Once the Rust project is initialized, contributions are expected to pass:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

Additional checks may be documented as the project grows.

## Pull requests

A good PR should contain:

- one coherent change or roadmap slice;
- a clear description of user-visible/technical behavior;
- tests for non-UI logic;
- exact manual validation steps for desktop/UI behavior;
- explicit notes about Wayland/X11 assumptions and limitations;
- screenshots or a short recording when visual behavior materially changes.

Use the repository PR template.

## Issues

When reporting a bug involving desktop behavior, include:

- distribution/version;
- desktop environment;
- `XDG_SESSION_TYPE`;
- whether XWayland is available/relevant;
- LionClip version/commit;
- reproduction steps;
- expected and actual behavior;
- non-sensitive diagnostic logs if useful.

Never paste sensitive clipboard contents into an issue.
