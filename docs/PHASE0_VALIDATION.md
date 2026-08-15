# Phase 0 popup-placement validation

Phase 0 is a technical spike, not a clipboard manager. It opens a small test
window, reports the active GTK backend, and makes the positioning path visible
both in the window and in the terminal.

## Positioning strategy

GTK/GDK clients cannot choose absolute top-level window coordinates on GNOME
Wayland. LionClip therefore uses these two deliberately isolated paths:

1. **Native Wayland or unsupported GDK backend:** show the window normally and
   let the compositor place it. This is the safe fallback; the diagnostic line
   reports `backend=compositor-fallback status=not-available`.
2. **X11, including an explicitly selected XWayland GTK backend:** after the
   GTK surface maps, query the X11 root pointer and active RandR monitor, offset
   the popup from the pointer, clamp it within that monitor, and send one X11
   configure request. The diagnostic line reports
   `backend=x11-pointer status=experimental`.

X11 code exists only in `src/positioning/x11.rs`. A failed connection, query,
or configure request falls back without terminating the application. Phase 0
does not add polling, a helper process, or a GNOME Shell extension.

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

## Manual validation on Zorin GNOME Wayland

Run every command below from a terminal inside the graphical desktop session.
Do not run them over SSH.

### 1. Record the non-sensitive environment

```bash
printf 'session=%s wayland=%s display=%s\n' \
  "$XDG_SESSION_TYPE" "${WAYLAND_DISPLAY:-unset}" "${DISPLAY:-unset}"
gnome-shell --version
```

Confirm that `session=wayland`. The display socket names are useful platform
diagnostics and contain no clipboard data.

### 2. Validate the native Wayland fallback

Make sure `GDK_BACKEND` is not set. Move the pointer to a memorable position,
then run:

```bash
env -u GDK_BACKEND cargo run 2>&1 | tee /tmp/lionclip-wayland.log
```

Expected result:

- the popup opens reliably and closes with `Esc`;
- its status text says `compositor-managed fallback`;
- the terminal contains lines like:

```text
lionclip: diagnostics session=wayland gdk_backend=wayland gdk_display_type=GdkWaylandDisplay
lionclip: placement backend=compositor-fallback status=not-available reason=wayland-does-not-allow-absolute-toplevel-placement
```

This classifies exact pointer-relative placement on the native Wayland path as
**not available**. The popup itself must still work without a crash.

### 3. Validate the XWayland experiment

The following command opts only this LionClip process into GTK's X11 backend;
it does not change the desktop session:

```bash
GDK_BACKEND=x11 cargo run 2>&1 | tee /tmp/lionclip-xwayland.log
```

Expected successful-path diagnostics:

```text
lionclip: diagnostics session=wayland gdk_backend=x11 gdk_display_type=GdkX11Display
lionclip: placement backend=x11-pointer status=experimental result=placed x=... y=... monitor=...x...+...+...
```

The popup status must say `X11 pointer experiment`. If the line instead says
`compositor-fallback`, record its short `reason` value; the application should
remain usable.

Repeat the command after placing the pointer at each of these locations,
closing with `Esc` between runs:

- center of the primary monitor;
- within about 20 px of every corner of the primary monitor;
- center and corners of every additional monitor;
- a monitor with non-100% scaling, if one is configured.

Confirm that the popup is near the pointer when there is room and remains fully
inside the current monitor at its edges. Then repeat at varied positions for at
least ten launches.

Classify the XWayland path as:

- **working:** all launches are near the pointer and edge-clamped;
- **unreliable:** the experimental log reports success, but one or more launches
  are misplaced, overridden, or appear on another monitor;
- **not available:** every launch uses the fallback or cannot open through the
  X11 backend.

### 4. Report the result

Report these details on the PR without including clipboard content:

- Zorin version and `gnome-shell --version`;
- the three environment values from step 1;
- monitor arrangement and scale factors;
- both `lionclip: diagnostics` and `lionclip: placement` lines;
- native Wayland result;
- XWayland classification (`working`, `unreliable`, or `not available`);
- which pointer/monitor positions failed, if any.

## Known limitations

- Native GNOME Wayland intentionally uses compositor-managed placement because
  the toplevel positioning protocol does not provide absolute coordinates to
  normal clients.
- The X11 path is an experiment. GNOME/XWayland may override a configure request,
  and mixed/fractional scaling may expose coordinate differences.
- RandR monitor rectangles represent monitor bounds, not reserved work areas;
  a panel or dock can overlap an edge-clamped popup.
- This code has not been classified on the primary Zorin machine until the
  manual matrix above is completed.
