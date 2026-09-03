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

use gtk::{gio, glib, prelude::*};
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
    wrapper::ConnectionExt as _,
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
///
/// It is also the granularity at which focus landing is noticed, and that
/// delay lands directly on what the user perceives between choosing an item
/// and seeing it pasted, so it is kept short. The cost is bounded: at worst
/// one cheap round trip per interval, for at most
/// [`FOCUS_CONFIRMATION_TIMEOUT`], on a background thread.
const FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(1);

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

pub(super) fn request_paste(
    target: PasteTarget,
    own_window: Option<u32>,
    on_done: impl FnOnce(bool) + 'static,
) {
    glib::MainContext::default().spawn_local(async move {
        let sent = match gio::spawn_blocking(move || attempt_paste(target, own_window)).await {
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
fn attempt_paste(target: PasteTarget, own_window: Option<u32>) -> bool {
    attempt_paste_on(None, target, own_window)
}

#[cfg(test)]
pub(super) fn attempt_paste_for_test(
    display: &str,
    target: PasteTarget,
    own_window: Option<u32>,
) -> bool {
    attempt_paste_on(Some(display), target, own_window)
}

fn attempt_paste_on(display: Option<&str>, target: PasteTarget, own_window: Option<u32>) -> bool {
    let started = Instant::now();
    let Ok((connection, screen_number)) = x11rb::connect(display) else {
        eprintln!("lionclip: auto-paste failed stage=connection");
        return false;
    };
    let Some(screen) = connection.setup().roots.get(screen_number) else {
        eprintln!("lionclip: auto-paste failed stage=screen");
        return false;
    };
    let root = screen.root;

    // The reply has to be read: sending the request only queues it, and a
    // `BadWindow` for a destroyed target arrives with the reply, not from
    // the send. Checking only the send would let a gone target through.
    if connection
        .get_window_attributes(target.xid)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_none()
    {
        eprintln!("lionclip: auto-paste skipped stage=target-gone");
        return false;
    }

    // Hiding the popup already makes the window manager hand focus back to
    // the target, and in practice that has landed by the time this runs. So
    // ask first, and only try to activate a window that is not already
    // focused: telling a compositor to activate the window it just activated
    // is not free, it makes it run a whole activation cycle whose visible
    // cost lands on the popup the user is watching disappear.
    let focus_wait_started = Instant::now();
    if !holds_focus(&connection, root, target.xid) {
        // Somebody other than the target and other than LionClip's own popup
        // holds the focus, which means the user moved on while the popup was
        // closing. Pulling focus away from whatever they are now using would
        // be worse than not pasting, so this stops instead of activating.
        if !focus_is_vacant_or(&connection, root, own_window) {
            eprintln!("lionclip: auto-paste skipped stage=foreign-window-focused");
            return false;
        }

        // Selected before requesting activation, so the confirmation event
        // cannot be generated before this client is listening for it. Any
        // number of clients may independently select FocusChangeMask on the
        // same window, so this cannot disturb the target's own handling.
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

        if !wait_for_focus_confirmation(&connection, root, target.xid) {
            eprintln!("lionclip: auto-paste skipped stage=focus-not-confirmed");
            return false;
        }
    }
    let focus_wait = focus_wait_started.elapsed();

    // Depends on nothing the focus handling above establishes, and the
    // keyboard-mapping reply it needs is the largest exchange here, so it
    // stays off the stretch between focus landing and the keys arriving.
    let Some(keys) = paste_keys(&connection) else {
        eprintln!("lionclip: auto-paste failed stage=keycode-lookup");
        return false;
    };

    // Re-confirmed immediately before synthesizing, against the server
    // rather than against what was true earlier.
    //
    // A `FocusIn` only says the target held the focus at the instant it was
    // emitted; anything could have taken it since, and XTEST delivers to
    // whoever owns the focus when the server processes the request, not to a
    // window named in it. Without this, an arbitrarily long gap between
    // confirmation and synthesis could put clipboard contents into a window
    // the user never chose.
    //
    // This narrows that gap to a single round trip rather than closing it:
    // X offers no atomic "send this key only if window W still has focus",
    // so a check-then-act window remains, bounded by the time between this
    // reply and the server processing the events queued right after it.
    if !holds_focus(&connection, root, target.xid) {
        eprintln!("lionclip: auto-paste skipped stage=focus-lost-before-synthesis");
        return false;
    }

    if !synthesize_ctrl_v(&connection, root, keys) {
        eprintln!("lionclip: auto-paste failed stage=key-synthesis");
        return false;
    }

    // Timings only, no payload: `focus_wait` is how long the window manager
    // took to hand focus back, which is the part LionClip waits on and
    // cannot skip, and `total` is everything this attempt did. Together they
    // separate compositor latency from LionClip's own.
    eprintln!(
        "lionclip: auto-paste sent focus_wait_ms={} total_ms={}",
        focus_wait.as_millis(),
        started.elapsed().as_millis()
    );
    true
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

/// Confirms the target actually owns the keyboard focus before any key is
/// synthesized, by either of two pieces of real server state: it already
/// holds the focus, or a `FocusIn` event says it just gained it.
///
/// Both are needed. Hiding the popup unmaps it, which makes the window
/// manager hand focus back to the previously focused window — the target —
/// on its own. When that lands before this runs, the activation request
/// changes nothing and the server generates no `FocusIn` at all, so waiting
/// only for the event would time out and skip a paste whose target is
/// already exactly where it needs to be.
fn wait_for_focus_confirmation<C: Connection>(connection: &C, root: u32, target: u32) -> bool {
    let deadline = Instant::now() + FOCUS_CONFIRMATION_TIMEOUT;
    loop {
        if holds_focus(connection, root, target) {
            return true;
        }
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

/// Whether nobody else has taken the focus: it is either unset/on the root
/// (normal while a window is being unmapped) or still on LionClip's own
/// popup, which is closing. Any other window means the user moved on, and
/// activating the target would take the focus away from them.
///
/// `own_window` being `None` means LionClip's own toplevel could not be
/// identified; the check then only accepts a vacant focus rather than
/// assuming an unknown focused window is ours.
fn focus_is_vacant_or<C: Connection>(connection: &C, root: u32, own_window: Option<u32>) -> bool {
    let Some(focus) = current_focus(connection) else {
        return false;
    };
    if focus == 0 || focus == 1 || focus == root {
        return true;
    }
    own_window.is_some_and(|own| {
        focus == own || toplevel_under_root(connection, root, focus) == Some(own)
    })
}

/// Whether the keyboard focus currently sits on `target` or on one of its
/// descendants — an application's focus normally lives on a child window of
/// the top-level, so an exact match alone would miss the common case.
fn holds_focus<C: Connection>(connection: &C, root: u32, target: u32) -> bool {
    let Some(focus) = current_focus(connection) else {
        return false;
    };
    if focus == target {
        return true;
    }
    // `None`/`PointerRoot` and the root window itself are never the target.
    if focus == 0 || focus == 1 || focus == root {
        return false;
    }
    toplevel_under_root(connection, root, focus) == Some(target)
}

fn current_focus<C: Connection>(connection: &C) -> Option<u32> {
    connection
        .get_input_focus()
        .ok()?
        .reply()
        .ok()
        .map(|reply| reply.focus)
}

/// LionClip's own toplevel, so focus checks can tell "our popup is still
/// closing" from "the user moved to another application".
pub(super) fn window_xid(window: &adw::ApplicationWindow) -> Option<u32> {
    window
        .surface()
        .and_then(|surface| surface.downcast::<gdk4_x11::X11Surface>().ok())
        .and_then(|surface| surface.xid().try_into().ok())
}

/// The keycodes the paste combination is made of, resolved once per attempt
/// from a single keyboard-mapping request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PasteKeys {
    control: u8,
    v: u8,
}

/// Presses and releases Control and V.
///
/// The four events are queued without a per-event round trip and confirmed
/// by a single `sync` at the end, rather than four sequential round trips on
/// the latency-sensitive stretch between focus landing and the keys
/// arriving. The X server processes one connection's requests in the order
/// they were sent, so the releases still follow their presses; queuing them
/// unconditionally rather than only after a confirmed press is what
/// guarantees a modifier can never be left logically stuck.
///
/// The closing `sync` is not decoration: it is what makes the server have
/// actually processed the events before this reports success, and before the
/// connection is dropped.
fn synthesize_ctrl_v<C: Connection>(connection: &C, root: u32, keys: PasteKeys) -> bool {
    let mut queued = true;
    let mut send = |type_: u8, keycode: u8| {
        queued &= xtest::fake_input(
            connection,
            type_,
            keycode,
            x11rb::CURRENT_TIME,
            root,
            0,
            0,
            0,
        )
        .is_ok();
    };

    send(KEY_PRESS_EVENT, keys.control);
    send(KEY_PRESS_EVENT, keys.v);
    send(KEY_RELEASE_EVENT, keys.v);
    send(KEY_RELEASE_EVENT, keys.control);

    queued && connection.sync().is_ok()
}

/// Resolves both keycodes from one `GetKeyboardMapping` reply.
///
/// The reply covers the whole keycode range and is the largest single
/// exchange in a paste attempt, so asking for it once for both keysyms
/// rather than once each matters more than the lookup itself does.
fn paste_keys<C: Connection>(connection: &C) -> Option<PasteKeys> {
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

    let keycode_of = |keysym: u32| -> Option<u8> {
        let index = mapping
            .keysyms
            .chunks(per_keycode)
            .position(|chunk| chunk.contains(&keysym))?;
        min.checked_add(u8::try_from(index).ok()?)
    };

    Some(PasteKeys {
        control: keycode_of(XK_CONTROL_L)?,
        v: keycode_of(XK_LOWERCASE_V)?,
    })
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

    // `ConnectionExt` — both the `xproto` and the `wrapper` one — already
    // arrives through `use super::*` below. Naming an anonymous trait import
    // again cannot shadow the glob the way a named type does, so repeating it
    // here is redundant and newer rustc rejects it under `-D warnings`.
    use x11rb::protocol::xproto::{CreateWindowAux, EventMask, InputFocus, WindowClass};

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
        ///
        /// A display number can already be taken — by a leftover lock file,
        /// or by anything else on a CI runner — in which case Xvfb exits
        /// instead of serving it. That is detected and the next number tried,
        /// rather than waiting out the readiness timeout and failing the test
        /// for an environmental reason.
        fn spawn() -> Option<Self> {
            static NEXT_DISPLAY: AtomicU32 = AtomicU32::new(90);
            for _ in 0..16 {
                let number = NEXT_DISPLAY.fetch_add(1, Ordering::Relaxed);
                if let Some(guard) = Self::spawn_on(format!(":{number}"))? {
                    return Some(guard);
                }
            }
            panic!("no free display number for Xvfb after 16 attempts");
        }

        /// `None` means Xvfb is not installed at all (give up entirely);
        /// `Some(None)` means this display number did not work out (try the
        /// next one).
        fn spawn_on(display: String) -> Option<Option<Self>> {
            let child = match Command::new("Xvfb")
                .arg(&display)
                .args(["-screen", "0", "1x1x24"])
                .args(["-nolisten", "tcp"])
                // Without this the server resets itself the moment its last
                // client disconnects, destroying every resource and dropping
                // connections with "Connection reset by peer". Both the
                // readiness probe below and the paste attempts themselves use
                // short-lived connections, so the client count legitimately
                // reaches zero between steps; `-noreset` is what makes that
                // harmless instead of a race.
                .arg("-noreset")
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
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if x11rb::connect(Some(&guard.display)).is_ok() {
                    return Some(Some(guard));
                }
                // Xvfb exiting means this display number is unusable; there
                // is nothing to wait for, so stop waiting for it.
                if matches!(guard.child.try_wait(), Ok(Some(_))) {
                    return Some(None);
                }
                if Instant::now() >= deadline {
                    let _ = guard.child.kill();
                    return Some(None);
                }
                thread::sleep(Duration::from_millis(20));
            }
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

    /// The popup unmapping on hide makes the window manager hand focus back
    /// to the target on its own. When that lands before the paste attempt
    /// starts, the target is *already* focused and the activation request
    /// changes nothing, so no `FocusIn` event is ever generated. Waiting for
    /// one would time out and skip the paste even though the target is
    /// exactly where it needs to be.
    #[test]
    fn a_target_that_already_holds_focus_is_confirmed_without_a_focus_event() {
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
        let target =
            capture_target_for_test(&xvfb.display).expect("target window should be captured");
        assert_eq!(target.xid, window);

        connection.sync().unwrap();

        // No focus change happens between here and the paste attempt.
        //
        // That an already-focused target is also not sent a redundant
        // activation request is deliberately *not* asserted here: X only
        // emits focus events when focus actually changes, so re-focusing an
        // already-focused window is a protocol no-op that leaves nothing to
        // observe. The cost that motivates skipping it is the compositor's
        // reaction to `_NET_ACTIVE_WINDOW`, and Xvfb has no compositor. See
        // `docs/PHASE6_VALIDATION.md` for the real-session check.
        let started = Instant::now();
        assert!(attempt_paste_for_test(&xvfb.display, target, None));
        assert!(
            started.elapsed() < FOCUS_CONFIRMATION_TIMEOUT,
            "an already-focused target must be confirmed immediately, not after the timeout"
        );

        let keys = paste_keys(&connection).unwrap();
        let (control, v) = (keys.control, keys.v);
        let mut received_control = false;
        let mut received_v = false;
        let deadline = Instant::now() + Duration::from_secs(2);
        while !(received_control && received_v) && Instant::now() < deadline {
            match connection.poll_for_event() {
                Ok(Some(Event::KeyPress(event))) => {
                    assert_eq!(event.event, window);
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

    /// The user moving to a different application while the popup closes
    /// must not have the focus yanked back, nor the clipboard pasted
    /// somewhere they never chose.
    #[test]
    fn a_foreign_window_holding_focus_aborts_without_stealing_it_back() {
        let Some(xvfb) = XvfbGuard::spawn() else {
            eprintln!("skipping: Xvfb not available");
            return;
        };
        let (connection, _) = x11rb::connect(Some(&xvfb.display)).unwrap();
        let root = root_of(&connection);
        let target_window = create_test_window(&connection, root);
        let foreign_window = create_test_window(&connection, root);

        connection
            .set_input_focus(InputFocus::PARENT, target_window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        let target =
            capture_target_for_test(&xvfb.display).expect("target window should be captured");

        // A third window takes the focus, and it is not LionClip's own
        // popup, so it stands for the user having moved on.
        connection
            .set_input_focus(InputFocus::PARENT, foreign_window, x11rb::CURRENT_TIME)
            .unwrap()
            .check()
            .unwrap();
        connection.sync().unwrap();
        while matches!(connection.poll_for_event(), Ok(Some(_))) {}

        assert!(!attempt_paste_for_test(
            &xvfb.display,
            target,
            Some(create_test_window(&connection, root))
        ));

        // The foreign window must still hold the focus, and no key may have
        // been delivered anywhere.
        assert_eq!(
            connection.get_input_focus().unwrap().reply().unwrap().focus,
            foreign_window
        );
        while let Ok(Some(event)) = connection.poll_for_event() {
            assert!(
                !matches!(event, Event::KeyPress(_) | Event::KeyRelease(_)),
                "nothing may be synthesized when a foreign window holds the focus"
            );
        }
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

        assert!(!attempt_paste_for_test(&xvfb.display, target, None));
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

        assert!(attempt_paste_for_test(
            &xvfb.display,
            target,
            Some(decoy_window)
        ));

        // Focus must have been handed back to the target, not left on the
        // decoy, and the synthesized Control_L/V presses must show up only
        // on the target's own event queue.
        assert_eq!(
            connection.get_input_focus().unwrap().reply().unwrap().focus,
            target_window
        );

        let keys = paste_keys(&connection).unwrap();
        let (control, v) = (keys.control, keys.v);
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
