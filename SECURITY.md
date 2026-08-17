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
`XTEST`) aimed at the application that held keyboard focus immediately before
LionClip opened — never at an arbitrary or attacker-influenced window. See
`docs/ARCHITECTURE.md`'s "Auto-paste" section for the full design. In short:

- the target is captured once, when the popup opens, and never derived from
  whatever happens to be focused after it closes;
- the target's existence is re-checked before use, and focus is confirmed
  against real X server state rather than assumed;
- if a window that is neither the target nor LionClip's own closing popup
  holds the focus, the attempt is **aborted** rather than pulling focus away
  from whatever the user moved on to;
- the focus check is repeated immediately before the keys are synthesized;
- every failure path — destroyed target, foreign focus, unconfirmed focus,
  disabled setting, a failed clipboard restore — results in no key synthesis
  at all rather than a guess.

What this is **not** is an atomic guarantee, and X11 does not offer one.
`XTEST` delivers to whichever window owns the focus when the server processes
the request, not to a window named in it, and there is no "send this key only
if window W still has focus" primitive. The final re-check narrows the
check-then-act window to a single round trip; it does not eliminate it. A
focus change landing inside that window could still deliver the keystroke
elsewhere. Users who consider that residual risk unacceptable for their
clipboard contents should leave the setting off, which is the default.

This capability is X11-only and does nothing on Wayland. It never reads or
logs clipboard content, window titles, or search queries.

## Supported versions

There are no stable releases yet. Security fixes currently target the latest development branch.
