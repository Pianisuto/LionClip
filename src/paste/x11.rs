//! X11 auto-paste backend.
//!
//! Extends the same isolated `x11rb` usage `src/positioning/x11.rs` already
//! established (per-call connection, no long-lived state, no shelling out to
//! `xdotool`/`ydotool`) rather than building a second X11 stack. Key
//! synthesis uses the XTEST extension, which `x11rb` already speaks with the
//! `xtest` Cargo feature — no new dependency.
//!
//! Every step here is designed to fail safe: any error, a destroyed target,
//! or a focus confirmation that does not arrive within a bounded window
//! simply means no paste is sent. Nothing here ever falls back to sleeping
//! blindly and hoping a key combination lands in the right window.

use std::{
    thread,
    time::{Duration, Instant},
};

use gtk::{gio, glib};
use x11rb::{
    connection::Connection,
    protocol::{
        Event,
        xproto::{
            ChangeWindowAttributesAux, ClientMessageEvent, ConnectionExt as _, EventMask,
            InputFocus, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
        },
        xtest,
    },
};

/// Sent to the previously focused window when the user picks a history item.
const XK_CONTROL_L: u32 = 0xffe3;
const XK_LOWERCASE_V: u32 = 0x0076;

/// How long to wait for the target to report it actually received focus
/// before giving up. Each iteration below re-checks real server state
/// (queued X events); this only bounds how long that checking continues.
const FOCUS_CONFIRMATION_TIMEOUT: Duration = Duration::from_millis(400);
/// The pause between non-blocking event polls while waiting for
/// confirmation. `poll_for_event` never blocks, so without a pause the wait
/// loop would busy-spin a thread-pool thread; the pause only paces how often
/// the server is asked, it is not itself what confirms anything landed.
const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(5);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PasteTarget {
    xid: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PasteError {
    Connection,
    Query,
    NoTarget,
}

/// Captures the window that currently owns the X input focus, walking up to
/// the top-level a window manager would activate. Must run while LionClip's
/// own popup is not visible: an unmapped window can never hold the X input
/// focus, but a popup already mapped once and merely hidden still has a
/// surface, so "not visible" is the precondition that matters here, not
/// "never realized". See the doc comment on
/// [`super::PasteCoordinator::capture_target`].
pub(super) fn capture_target() -> Result<PasteTarget, PasteError> {
    capture_target_on(None)
}

#[cfg(test)]
pub(super) fn capture_target_for_test(display: &str) -> Result<PasteTarget, PasteError> {
    capture_target_on(Some(display))
}

fn capture_target_on(display: Option<&str>) -> Result<PasteTarget, PasteError> {
    let (connection, screen_number) =
        x11rb::connect(display).map_err(|_| PasteError::Connection)?;
    let screen = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or(PasteError::Query)?;
    let root = screen.root;

    let focus = connection
        .get_input_focus()
        .map_err(|_| PasteError::Query)?
        .reply()
        .map_err(|_| PasteError::Query)?
        .focus;

    // 0 and 1 are the protocol's `None`/`PointerRoot` pseudo-window values,
    // never real windows; a focus of the root window itself means no
    // application is meaningfully focused either. None of these are a place
    // it would make sense to paste into.
    if focus == 0 || focus == 1 || focus == root {
        return Err(PasteError::NoTarget);
    }

    let toplevel = toplevel_under_root(&connection, root, focus).ok_or(PasteError::NoTarget)?;

    // Confirm it still exists before treating it as a usable target; a very
    // short-lived window could already be gone by the time this runs.
    connection
        .get_window_attributes(toplevel)
        .map_err(|_| PasteError::Query)?
        .reply()
        .map_err(|_| PasteError::NoTarget)?;

    Ok(PasteTarget { xid: toplevel })
}

/// Walks up `QueryTree` parents from `start` until reaching a window whose
/// parent is `root`, which is what "the top-level a window manager frames"
/// means at the X11 protocol level.
fn toplevel_under_root<C: Connection>(connection: &C, root: u32, start: u32) -> Option<u32> {
    let mut candidate = start;
    for _ in 0..8 {
        let tree = connection.query_tree(candidate).ok()?.reply().ok()?;
        if tree.parent == 0 || tree.parent == root {
            return Some(candidate);
        }
        candidate = tree.parent;
    }
    None
}

pub(super) fn request_paste(target: PasteTarget, on_done: impl FnOnce(bool) + 'static) {
    glib::MainContext::default().spawn_local(async move {
        let sent = match gio::spawn_blocking(move || attempt_paste(target)).await {
            Ok(sent) => sent,
            Err(_) => {
                eprintln!("lionclip: auto-paste failed stage=worker-panic");
                false
            }
        };
        on_done(sent);
    });
}

/// The whole validate/activate/confirm/synthesize sequence, run on a
/// blocking-pool thread with its own connection: this can take up to
/// [`FOCUS_CONFIRMATION_TIMEOUT`], and none of it may run on the GTK main
/// thread.
fn attempt_paste(target: PasteTarget) -> bool {
    attempt_paste_on(None, target)
}

#[cfg(test)]
pub(super) fn attempt_paste_for_test(display: &str, target: PasteTarget) -> bool {
    attempt_paste_on(Some(display), target)
}

fn attempt_paste_on(display: Option<&str>, target: PasteTarget) -> bool {
    let Ok((connection, screen_number)) = x11rb::connect(display) else {
        eprintln!("lionclip: auto-paste failed stage=connection");
        return false;
    };
    let Some(screen) = connection.setup().roots.get(screen_number) else {
        eprintln!("lionclip: auto-paste failed stage=screen");
        return false;
    };
    let root = screen.root;

    if connection.get_window_attributes(target.xid).is_err() {
        eprintln!("lionclip: auto-paste skipped stage=target-gone");
        return false;
    }

    // Any number of clients may independently select FocusChangeMask on the
    // same window, so this cannot disturb the target's own event handling.
    if connection
        .change_window_attributes(
            target.xid,
            &ChangeWindowAttributesAux::new().event_mask(EventMask::FOCUS_CHANGE),
        )
        .ok()
        .and_then(|cookie| cookie.check().ok())
        .is_none()
    {
        eprintln!("lionclip: auto-paste failed stage=select-focus-events");
        return false;
    }

    if !request_activation(&connection, root, target.xid) {
        eprintln!("lionclip: auto-paste failed stage=activation-request");
        return false;
    }

    if !wait_for_focus_confirmation(&connection, target.xid) {
        eprintln!("lionclip: auto-paste skipped stage=focus-not-confirmed");
        return false;
    }

    if synthesize_ctrl_v(&connection, root) {
        true
    } else {
        eprintln!("lionclip: auto-paste failed stage=key-synthesis");
        false
    }
}

/// Asks the window manager to activate `target` via the standard EWMH
/// request, and directly reinforces it with `SetInputFocus` for window
/// managers that do not act on `_NET_ACTIVE_WINDOW`; mature X11 automation
/// tools send both for exactly this compatibility reason. This is only ever
/// asked to hand focus back to a window LionClip itself observed holding it
/// moments earlier (see `capture_target`), which is the specific case
/// focus-stealing prevention exists to allow rather than to block.
fn request_activation<C: Connection>(connection: &C, root: u32, target: u32) -> bool {
    let activation_sent =
        match x11rb::protocol::xproto::intern_atom(connection, false, b"_NET_ACTIVE_WINDOW")
            .ok()
            .and_then(|cookie| cookie.reply().ok())
        {
            Some(reply) => {
                let event = ClientMessageEvent::new(
                    32,
                    target,
                    reply.atom,
                    [1, x11rb::CURRENT_TIME, 0, 0, 0],
                );
                connection
                    .send_event(
                        false,
                        root,
                        EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT,
                        event,
                    )
                    .ok()
                    .and_then(|cookie| cookie.check().ok())
                    .is_some()
            }
            None => false,
        };

    let focus_set = connection
        .set_input_focus(InputFocus::PARENT, target, x11rb::CURRENT_TIME)
        .ok()
        .and_then(|cookie| cookie.check().ok())
        .is_some();

    let _ = connection.flush();
    activation_sent || focus_set
}

fn wait_for_focus_confirmation<C: Connection>(connection: &C, target: u32) -> bool {
    let deadline = Instant::now() + FOCUS_CONFIRMATION_TIMEOUT;
    loop {
        loop {
            match connection.poll_for_event() {
                Ok(Some(Event::FocusIn(event))) if event.event == target => return true,
                Ok(Some(_)) => continue,
                Ok(None) => break,
                Err(_) => return false,
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(FOCUS_POLL_INTERVAL);
    }
}

/// Presses and releases Control then V. Every step after a successful press
/// is attempted regardless of a later failure, so a partial failure can
/// never leave a modifier logically stuck at the X server.
fn synthesize_ctrl_v<C: Connection>(connection: &C, root: u32) -> bool {
    let (Some(control), Some(v)) = (
        keycode_for_keysym(connection, XK_CONTROL_L),
        keycode_for_keysym(connection, XK_LOWERCASE_V),
    ) else {
        return false;
    };

    let send = |type_: u8, keycode: u8| {
        xtest::fake_input(
            connection,
            type_,
            keycode,
            x11rb::CURRENT_TIME,
            root,
            0,
            0,
            0,
        )
        .ok()
        .and_then(|cookie| cookie.check().ok())
        .is_some()
    };

    let control_down = send(KEY_PRESS_EVENT, control);
    let v_down = control_down && send(KEY_PRESS_EVENT, v);
    let v_up = !v_down || send(KEY_RELEASE_EVENT, v);
    let control_up = !control_down || send(KEY_RELEASE_EVENT, control);

    let _ = connection.flush();
    control_down && v_down && v_up && control_up
}

fn keycode_for_keysym<C: Connection>(connection: &C, keysym: u32) -> Option<u8> {
    let setup = connection.setup();
    let min = setup.min_keycode;
    let count = setup.max_keycode.saturating_sub(min).saturating_add(1);
    let mapping = connection
        .get_keyboard_mapping(min, count)
        .ok()?
        .reply()
        .ok()?;
    let per_keycode = usize::from(mapping.keysyms_per_keycode);
    if per_keycode == 0 {
        return None;
    }
    let index = mapping
        .keysyms
        .chunks(per_keycode)
        .position(|chunk| chunk.contains(&keysym))?;
    min.checked_add(u8::try_from(index).ok()?)
}

/// Integration tests against a real, disposable Xvfb X server.
///
/// There is no window manager under Xvfb, so the `_NET_ACTIVE_WINDOW` half
/// of `request_activation` cannot be exercised here: nothing implements the
/// EWMH side of that protocol. The direct `SetInputFocus` reinforcement,
/// however, is honored by the X server itself with no window manager
/// involved, so these tests still cover target capture, the destroyed-target
/// fail-safe, the real FocusIn confirmation wait, and that synthesized keys
/// are delivered to the intended window and no other — everything Xvfb can
/// actually prove without a compositor. Manual QA on the real Zorin/GNOME/X11
/// target covers the `_NET_ACTIVE_WINDOW` path; see the PR description.
#[cfg(test)]
mod xvfb_tests {
    use std::{
        process::{Child, Command, Stdio},
        sync::atomic::{AtomicU32, Ordering},
    };

    use x11rb::{
        protocol::xproto::{
            ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, WindowClass,
        },
        wrapper::ConnectionExt as _,
    };

    use super::*;

    struct XvfbGuard {
        child: Child,
        display: String,
    }

    impl XvfbGuard {
        /// Spawns a fresh, disposable Xvfb on its own display number so
        /// tests never share server state and can run in parallel. Returns
        /// `None` rather than panicking when `Xvfb` is not installed, so
        /// `cargo test` still passes on a machine without it; CI installs it
        /// explicitly (see `.github/workflows/rust.yml`).
        fn spawn() -> Option<Self> {
            static NEXT_DISPLAY: AtomicU32 = AtomicU32::new(90);
            let number = NEXT_DISPLAY.fetch_add(1, Ordering::Relaxed);
            let display = format!(":{number}");

            let child = match Command::new("Xvfb")
                .arg(&display)
                .args(["-screen", "0", "1x1x24"])
                .args(["-nolisten", "tcp"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(_) => return None,
            };
            let mut guard = Self { child, display };

            // Bounded wait for the server to actually accept connections:
            // each iteration is a real connection attempt, not a blind delay.
            let deadline = Instant::now() + Duration::from_secs(5);
            while x11rb::connect(Some(&guard.display)).is_err() {
                if Instant::now() >= deadline {
                    let _ = guard.child.kill();
                    panic!("Xvfb on {} did not become ready in time", guard.display);
                }
                thread::sleep(Duration::from_millis(20));
            }
            Some(guard)
        }
    }

    impl Drop for XvfbGuard {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    fn create_test_window<C: Connection>(connection: &C, root: u32) -> u32 {
        let window = connection.generate_id().expect("generate window id");
        connection
            .create_window(
                x11rb::COPY_DEPTH_FROM_PARENT,
                window,
                root,
                0,
                0,
                8,
                8,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS),
            )
            .expect("create window request")
            .check()
            .expect("create window");
        connection
            .map_window(window)
            .expect("map window request")
            .check()
            .expect("map window");
        window
    }

    fn root_of<C: Connection>(connection: &C) -> u32 {
        connection.setup().roots[0].root
    }

    #[test]
    fn capture_target_identifies_the_focused_window() {
        let Some(xvfb) = XvfbGuard::spawn() else {
            eprintln!("skipping: Xvfb not available");
            return;
        };
        let (connection, _) = x11rb::connect(Some(&xvfb.display)).unwrap();
        let root = root_of(&connection);
        let window = create_test_window(&connection, root);

        connection
            .set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        // Confirms the focus change actually landed before capturing, rather
        // than assuming the request above took effect immediately.
        assert_eq!(
            connection.get_input_focus().unwrap().reply().unwrap().focus,
            window
        );

        let target = capture_target_for_test(&xvfb.display).expect("a target was captured");
        assert_eq!(target.xid, window);
    }

    #[test]
    fn capture_target_rejects_pointer_root_focus() {
        let Some(xvfb) = XvfbGuard::spawn() else {
            eprintln!("skipping: Xvfb not available");
            return;
        };
        let (connection, _) = x11rb::connect(Some(&xvfb.display)).unwrap();
        connection
            .set_input_focus(InputFocus::POINTER_ROOT, 1_u32, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();

        assert_eq!(
            capture_target_for_test(&xvfb.display),
            Err(PasteError::NoTarget)
        );
    }

    #[test]
    fn a_destroyed_target_is_rejected_and_nothing_is_synthesized() {
        let Some(xvfb) = XvfbGuard::spawn() else {
            eprintln!("skipping: Xvfb not available");
            return;
        };
        let (connection, _) = x11rb::connect(Some(&xvfb.display)).unwrap();
        let root = root_of(&connection);
        let window = create_test_window(&connection, root);

        let target = PasteTarget { xid: window };
        connection.destroy_window(window).unwrap().check().unwrap();
        connection.sync().unwrap();

        assert!(!attempt_paste_for_test(&xvfb.display, target));
    }

    #[test]
    fn paste_confirms_focus_and_delivers_keys_only_to_the_intended_window() {
        let Some(xvfb) = XvfbGuard::spawn() else {
            eprintln!("skipping: Xvfb not available");
            return;
        };
        let (connection, _) = x11rb::connect(Some(&xvfb.display)).unwrap();
        let root = root_of(&connection);
        let target_window = create_test_window(&connection, root);
        let decoy_window = create_test_window(&connection, root);

        // Target briefly holds focus, exactly like the app LionClip is about
        // to auto-paste into, and capture_target sees it.
        connection
            .set_input_focus(InputFocus::PARENT, target_window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let target =
            capture_target_for_test(&xvfb.display).expect("target window should be captured");
        assert_eq!(target.xid, target_window);

        // The popup opening takes focus away, the same way it does in the
        // real application between capture and the user's selection.
        connection
            .set_input_focus(InputFocus::PARENT, decoy_window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        connection.sync().unwrap();

        assert!(attempt_paste_for_test(&xvfb.display, target));

        // Focus must have been handed back to the target, not left on the
        // decoy, and the synthesized Control_L/V presses must show up only
        // on the target's own event queue.
        assert_eq!(
            connection.get_input_focus().unwrap().reply().unwrap().focus,
            target_window
        );

        let control = keycode_for_keysym(&connection, XK_CONTROL_L).unwrap();
        let v = keycode_for_keysym(&connection, XK_LOWERCASE_V).unwrap();
        let mut received_control = false;
        let mut received_v = false;
        // The synthesized events are generated by a different connection to
        // the same server, so their delivery to this connection's socket is
        // asynchronous; poll with a short bounded wait rather than a single
        // pass, the same real-confirmation-over-a-bounded-window pattern
        // `wait_for_focus_confirmation` itself uses.
        let deadline = Instant::now() + Duration::from_secs(2);
        while !(received_control && received_v) && Instant::now() < deadline {
            match connection.poll_for_event() {
                Ok(Some(Event::KeyPress(event))) => {
                    assert_eq!(
                        event.event, target_window,
                        "a synthesized key event reached a window other than the intended target"
                    );
                    received_control |= event.detail == control;
                    received_v |= event.detail == v;
                }
                Ok(Some(_)) => {}
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        assert!(
            received_control && received_v,
            "expected both keys to be observed on the target"
        );
    }
}
