# CLAUDE.md

Claude agents working in LionClip must follow `AGENTS.md` as the primary repository instruction set.

Read, in order:

1. `AGENTS.md`
2. `docs/ARCHITECTURE.md`
3. `docs/ROADMAP.md`
4. the current issue/PR/task

## Claude-specific working rules

- Do not redesign the project before understanding the current roadmap phase.
- Prefer a small, working vertical slice over a broad partial implementation.
- Do not assume GNOME Wayland permits arbitrary global pointer coordinates or top-level window positioning.
- Preserve the validated, isolated X11 positioning backend on the primary Zorin target; treat only XWayland positioning as experimental until it is validated in a real Wayland session.
- Keep UI in GTK4/Libadwaita and avoid introducing a second UI stack.
- Do not add framework-style abstractions for hypothetical future needs.
- Never log clipboard payloads.
- Never add telemetry or network calls to core functionality.
- When desktop behavior cannot be validated in the current environment, say so and provide exact manual test steps instead of guessing.
- Before finishing, run the repository's formatting, lint, build, and test commands that are available for the current phase.

When responding after implementation, include: changed files/behavior, tests run, manual validation steps, limitations, and the next roadmap step. Keep the summary concise.
