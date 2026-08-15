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

## Supported versions

There are no stable releases yet. Security fixes currently target the latest development branch.
