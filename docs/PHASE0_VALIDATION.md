# Phase 0 popup-placement result

Phase 0 is complete. It validated the riskiest LionClip interaction before
clipboard history work: a compact GTK4/Libadwaita popup that opens near the
pointer on the primary target and falls back safely when exact placement is not
available.

## Conclusion

| Environment | Phase 0 result | V1 policy |
| --- | --- | --- |
| Zorin GNOME/X11 | **Working and validated on the real target machine** | Primary V1 validation target |
| Native GNOME Wayland | **Exact pointer-relative top-level placement unavailable through the current approach** | Keep compositor-managed fallback |
| XWayland inside a Wayland session | **Experimental and not yet validated** | Keep isolated backend; do not depend on it for V1 |

The successful X11 result completes the Phase 0 requirement. Phase 1 is not
blocked on native Wayland or XWayland validation. Neither secondary path was
removed.

## Positioning strategy

LionClip keeps platform behavior behind the positioning boundary:

1. **GNOME/X11:** after the first rendered frame, query the X11 root pointer
   and active RandR monitor, offset the popup from the pointer, clamp it within
   that monitor, and send one X11 configure request. On an X11 session the log
   reports `backend=x11-pointer status=working`.
2. **Native GNOME Wayland:** show the window normally and let the compositor
   place it because GTK/GDK does not expose absolute top-level placement there.
   The log reports `backend=compositor-fallback status=not-available`.
3. **XWayland:** retain the same isolated X11 implementation when GTK is
   explicitly run with its X11 backend inside a Wayland session. This route is
   not yet validated and reports `backend=x11-pointer status=experimental`.

X11 code exists only in `src/positioning/x11.rs`. A failed connection, query,
or configure request falls back without terminating the application. Phase 0
does not add polling, a helper process, or a GNOME Shell extension.

## Recorded real-machine validation

The real target validation used:

- Zorin OS based on Ubuntu 24.04 (`noble`);
- GNOME desktop;
- `XDG_SESSION_TYPE=x11` and `GdkX11Display`;
- two 2560×1440 monitors arranged side by side.

Observed results:

- the popup opened near the pointer reliably;
- the undecorated popup measured 430×250 px;
- center placements matched the pointer plus the configured 16 px offset;
- top, left, right, and bottom monitor edges were clamped successfully on both
  monitors;
- GNOME/X11 additionally respected the reserved panel work area at the lower
  edge;
- `Esc` closed every run;
- no crashes occurred and no sensitive clipboard data was logged.

Representative diagnostics:

```text
lionclip: diagnostics session=x11 gdk_backend=x11 gdk_display_type=GdkX11Display
lionclip: placement backend=x11-pointer status=working result=placed x=... y=... monitor=...x...+...+...
```

This evidence classifies GNOME/X11 as **working** on the primary target.

## Ubuntu/Zorin Noble setup

Install a current Rust stable toolchain with
[rustup](https://rustup.rs/), then install the native build dependencies:

```bash
sudo apt update
sudo apt install --yes \
  build-essential \
  libadwaita-1-dev \
  libgtk-4-dev \
  libx11-dev \
  pkg-config
rustup toolchain install stable --component clippy,rustfmt
rustup default stable
```

Build and check the project from the repository root:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
cargo build
```

## Reproduce the primary X11 validation

Run from a terminal inside the graphical desktop session, not over SSH.

1. Confirm the session and start LionClip:

   ```bash
   printf 'session=%s display=%s\n' "$XDG_SESSION_TYPE" "${DISPLAY:-unset}"
   env -u GDK_BACKEND cargo run 2>&1 | tee /tmp/lionclip-x11.log
   ```

2. Confirm `session=x11`, `gdk_backend=x11`, and
   `backend=x11-pointer status=working` in the output.
3. Repeat after placing the pointer at the center and near every corner of each
   monitor. Close with `Esc` between runs.
4. Confirm the popup stays near the pointer when space permits and remains
   fully visible at monitor edges.

## Optional secondary-backend checks

These checks can improve secondary-platform knowledge but do not block Phase 1.

### Native Wayland fallback

From a real GNOME Wayland session, run:

```bash
env -u GDK_BACKEND cargo run 2>&1 | tee /tmp/lionclip-wayland.log
```

Expected diagnostics:

```text
lionclip: diagnostics session=wayland gdk_backend=wayland gdk_display_type=GdkWaylandDisplay
lionclip: placement backend=compositor-fallback status=not-available reason=wayland-does-not-allow-absolute-toplevel-placement
```

The popup should still open and close without crashing, but exact
pointer-relative top-level placement is not provided by the current approach.

### XWayland experiment

From that same Wayland session, opt only LionClip into GTK's X11 backend:

```bash
GDK_BACKEND=x11 cargo run 2>&1 | tee /tmp/lionclip-xwayland.log
```

Until this route is tested on a real Wayland session, the expected diagnostic
classification is:

```text
lionclip: diagnostics session=wayland gdk_backend=x11 gdk_display_type=GdkX11Display
lionclip: placement backend=x11-pointer status=experimental result=placed x=... y=... monitor=...x...+...+...
```

## Known limitations

- Native GNOME Wayland relies on compositor-managed placement rather than exact
  pointer-relative coordinates.
- XWayland may override configure requests or expose coordinate differences,
  especially with mixed or fractional scaling; it remains unvalidated.
- RandR reports monitor bounds rather than reserved work areas. On the validated
  target, GNOME/X11 applied its own additional work-area adjustment near the
  panel.
