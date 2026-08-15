# Maintainer Repository Setup

This file records recommended GitHub repository settings for LionClip.

Some repository-level settings cannot be represented as files, so the repository owner should apply them in GitHub Settings.

## Repository metadata

Recommended description:

> Fast, native clipboard history for GNOME/Zorin OS — built with Rust, GTK4 and Libadwaita.

Recommended topics:

- `rust`
- `gtk4`
- `libadwaita`
- `clipboard-manager`
- `gnome`
- `zorin-os`
- `linux-desktop`
- `wayland`
- `x11`

Homepage can remain empty until there is a project site or release documentation worth linking.

## Features

Recommended during early development:

- Issues: **enabled**
- Projects: **disabled** unless actively used for roadmap management
- Wiki: **disabled**; keep canonical docs versioned in the repository
- Discussions: optional; keep disabled until there is a real community need

## Pull request / merge policy

Recommended:

- allow **squash merging**;
- disable merge commits;
- disable rebase merging unless a future contributor workflow needs it;
- automatically delete head branches after merge;
- allow pull requests to update their branch when GitHub supports it for the current ruleset;
- use PR title as the squash commit title where practical.

The repository roadmap is phase-oriented, so one clean squash commit per focused PR keeps history readable.

## `main` protection

Do not require CI checks until Phase 0 has created a real workflow and the checks have stable names.

After CI exists, protect `main` with a ruleset or branch protection that approximately enforces:

- changes through pull requests for normal feature work;
- required status checks for formatting/lint/tests/build;
- branch must be up to date before merge if that remains reliable;
- no force pushes;
- no branch deletion;
- conversation resolution before merge when review threads exist.

For a solo-maintained early project, requiring an external approval is optional and may add friction without value. The important part is passing checks and keeping normal feature work in PRs.

## Security

Recommended for a public clipboard manager:

- enable Dependabot alerts;
- enable dependency graph;
- enable private vulnerability reporting if available;
- enable secret scanning/push protection if GitHub makes them available for the repository/account;
- keep `SECURITY.md` current once a stable release/support policy exists.

Clipboard payloads must never be included in public issues, CI artifacts, crash dumps, or normal logs.

## License

A license is intentionally **not** selected during bootstrap.

Public visibility alone does not choose an open-source license. Before the first public release, explicitly choose the intended licensing model (for example MIT, Apache-2.0, GPL-3.0, or another license) and add the corresponding `LICENSE` file.

Do not let a coding agent choose or change the project license without an explicit maintainer decision.

## Release policy

Before the first tagged release:

- choose the license;
- verify installation/uninstallation on the primary Zorin target;
- publish a reproducible package/build path;
- document known Wayland/X11 limitations;
- ensure CI passes from a clean checkout;
- verify that no test fixtures/logs contain real clipboard data.
