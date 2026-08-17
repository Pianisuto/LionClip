# Security Policy

LionClip handles clipboard data, which may contain credentials, tokens, personal information, source code, and other sensitive material.

## Reporting a vulnerability

Please do **not** open a public issue for a vulnerability that could expose clipboard contents, bypass privacy controls, execute copied data, or disclose local history.

Until a dedicated security contact/process is published, use GitHub's private vulnerability reporting feature for this repository if it is enabled. If that feature is unavailable, contact the repository owner privately through an appropriate GitHub contact channel rather than publishing exploit details.

## Security expectations

LionClip's core functionality should:

- remain local-only;
- avoid telemetry by default;
- avoid network dependencies for clipboard history;
- never execute copied content;
- never include clipboard payloads in normal logs;
- keep storage inside the user's standard application data locations;
- provide deletion/retention controls as the persistence features mature.

## Auto-paste (input synthesis)

Phase 6 added an opt-in, default-off "automatically paste selected items"
setting. When enabled, LionClip synthesizes a Ctrl+V key combination (X11
`XTEST`) directed at the application that held keyboard focus immediately
before LionClip opened — never at an arbitrary or attacker-influenced
window. See `docs/ARCHITECTURE.md`'s "Auto-paste" section for the full
fail-safe design: the target is captured once at popup-open time, re-checked
for existence before use, focus is requested and then confirmed against real
X server state before any key is synthesized, and every failure path
(destroyed target, unconfirmed focus, disabled setting, a failed clipboard
restore) results in no key synthesis at all rather than a guess. This
capability is X11-only and does nothing on Wayland. It never reads or logs
clipboard content, window titles, or search queries.

## Supported versions

There are no stable releases yet. Security fixes currently target the latest development branch.
