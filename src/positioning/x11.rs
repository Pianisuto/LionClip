use std::cell::RefCell;

use gdk4_x11::X11Surface;
use gtk::prelude::*;
use x11rb::{
    connection::Connection,
    properties::{WmSizeHints, WmSizeHintsSpecification},
    protocol::{
        randr::{ConnectionExt as _, GetMonitorsReply},
        xproto::{ConfigureWindowAux, ConnectionExt as _},
    },
    rust_connection::RustConnection,
};

use super::{
    PlacementOutcome, PointerAnchor, X11PathStatus,
    geometry::{Point, Rect, Size, clamp_popup_origin, monitor_at_pointer},
};

const POINTER_OFFSET: i32 = 16;
const MONITOR_MARGIN: i32 = 12;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PositionError {
    SurfaceUnavailable,
    Connection,
    Query,
    Placement,
}

thread_local! {
    /// The connection positioning queries run on, opened once and kept.
    ///
    /// Placement deliberately does not share GTK's own display connection: a
    /// still-pending unmap from a previous open could otherwise be processed
    /// after the move and leave the popup at its old position. That
    /// requirement is about *which* connection is used, not about opening a
    /// fresh one every time — and opening one is not free, since it means a
    /// socket connect plus reading the server's entire setup. Placement runs
    /// twice per popup open and [`holds_keyboard_focus`] runs again on every
    /// activation change, so the connection is kept between calls.
    ///
    /// Thread-local because every caller in this module runs on the GTK main
    /// thread. The auto-paste backend deliberately keeps its own per-attempt
    /// connection: it runs on blocking-pool threads and registers for focus
    /// events, and none of that may be shared with placement.
    static CONNECTION: RefCell<Option<(RustConnection, usize)>> = const { RefCell::new(None) };
}

/// Runs `body` on the shared positioning connection, opening it if needed.
///
/// Any failure drops the connection, so a server that closed it is not
/// retried forever through a dead socket; the next call simply reconnects,
/// which is exactly what every call did before it was cached.
fn with_connection<T>(
    body: impl FnOnce(&RustConnection, usize) -> Result<T, PositionError>,
) -> Result<T, PositionError> {
    CONNECTION.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(x11rb::connect(None).map_err(|_| PositionError::Connection)?);
        }
        let (connection, screen_number) = slot.as_ref().ok_or(PositionError::Connection)?;
        let result = body(connection, *screen_number);
        if result.is_err() {
            slot.take();
        }
        result
    })
}

/// The X id of the popup's own surface.
fn surface_xid(window: &adw::ApplicationWindow) -> Result<u32, PositionError> {
    window
        .surface()
        .and_then(|surface| surface.downcast::<X11Surface>().ok())
        .ok_or(PositionError::SurfaceUnavailable)?
        .xid()
        .try_into()
        .map_err(|_| PositionError::SurfaceUnavailable)
}

pub fn place_near_pointer(
    window: &adw::ApplicationWindow,
    status: X11PathStatus,
    anchor: Option<PointerAnchor>,
) -> Result<PlacementOutcome, PositionError> {
    let xid = surface_xid(window)?;

    with_connection(|connection, screen_number| {
        let screen = connection
            .setup()
            .roots
            .get(screen_number)
            .ok_or(PositionError::Query)?;
        let root = screen.root;

        // Nothing the placement needs to read depends on anything else it
        // reads, so every request goes out before the first reply is taken.
        // The four round trips this used to cost sequentially are one.
        let pointer_cookie = anchor
            .is_none()
            .then(|| connection.query_pointer(root))
            .transpose()
            .map_err(|_| PositionError::Query)?;
        let geometry_cookie = connection
            .get_geometry(xid)
            .map_err(|_| PositionError::Query)?;
        let monitors_cookie = connection.randr_get_monitors(root, true).ok();
        // Read back so GTK's own size constraints survive the hints write
        // below, which only replaces the position field.
        let hints_cookie =
            WmSizeHints::get_normal_hints(connection, xid).map_err(|_| PositionError::Placement)?;

        let pointer = match anchor {
            Some(PointerAnchor(pointer)) => pointer,
            None => {
                let reply = pointer_cookie
                    .ok_or(PositionError::Query)?
                    .reply()
                    .map_err(|_| PositionError::Query)?;
                Point {
                    x: i32::from(reply.root_x),
                    y: i32::from(reply.root_y),
                }
            }
        };

        let window_geometry = geometry_cookie.reply().map_err(|_| PositionError::Query)?;
        let popup_size = Size {
            width: i32::from(window_geometry.width),
            height: i32::from(window_geometry.height),
        };

        let monitors = monitors_cookie
            .and_then(|cookie| cookie.reply().ok())
            .and_then(usable_monitors)
            .unwrap_or_else(|| {
                vec![Rect {
                    x: 0,
                    y: 0,
                    width: i32::from(screen.width_in_pixels),
                    height: i32::from(screen.height_in_pixels),
                }]
            });
        let monitor = monitor_at_pointer(&monitors, pointer).ok_or(PositionError::Query)?;
        let desired = Point {
            x: pointer.x.saturating_add(POINTER_OFFSET),
            y: pointer.y.saturating_add(POINTER_OFFSET),
        };
        let origin = clamp_popup_origin(desired, popup_size, monitor, MONITOR_MARGIN);

        let mut hints = hints_cookie
            .reply()
            .map_err(|_| PositionError::Placement)?
            .unwrap_or_default();
        // Recorded as a user-specified position because a window manager
        // places a window it has not managed yet by its own policy and ignores
        // coordinates the client set before mapping. Saying the position came
        // from the user makes it honour the placement, so the popup is mapped
        // where it belongs rather than moved there afterwards.
        hints.position = Some((WmSizeHintsSpecification::UserSpecified, origin.x, origin.y));

        // Both writes are queued before either is checked. The server still
        // processes them in the order they were sent, so the window manager
        // sees the hints before the move, and confirming them costs one round
        // trip instead of two.
        let hints_written = hints
            .set_normal_hints(connection, xid)
            .map_err(|_| PositionError::Placement)?;
        let configured = connection
            .configure_window(xid, &ConfigureWindowAux::new().x(origin.x).y(origin.y))
            .map_err(|_| PositionError::Placement)?;
        hints_written
            .check()
            .map_err(|_| PositionError::Placement)?;
        configured.check().map_err(|_| PositionError::Placement)?;
        connection.flush().map_err(|_| PositionError::Placement)?;

        Ok(PlacementOutcome::X11Pointer {
            x: origin.x,
            y: origin.y,
            monitor,
            status,
            anchor: PointerAnchor(pointer),
        })
    })
}

/// Whether the popup's surface still owns the X keyboard focus.
///
/// A keyboard grab — a desktop shortcut being pressed, a menu opening —
/// deactivates the toplevel without taking the input focus away from it, while
/// another window taking over does move the focus. Telling those apart is what
/// keeps the popup open while its own shortcut is pressed.
pub fn holds_keyboard_focus(window: &adw::ApplicationWindow) -> Result<bool, PositionError> {
    let xid = surface_xid(window)?;

    with_connection(|connection, _| {
        let focus = connection
            .get_input_focus()
            .map_err(|_| PositionError::Query)?
            .reply()
            .map_err(|_| PositionError::Query)?
            .focus;
        if focus == xid {
            return Ok(true);
        }

        // The focus usually sits on a descendant of the toplevel, and a window
        // manager may also reparent the toplevel into a frame. Each step
        // depends on the previous reply, so these cannot be pipelined.
        let mut candidate = focus;
        for _ in 0..8 {
            let tree = connection
                .query_tree(candidate)
                .map_err(|_| PositionError::Query)?
                .reply()
                .map_err(|_| PositionError::Query)?;
            if tree.parent == 0 || tree.parent == tree.root {
                return Ok(false);
            }
            if tree.parent == xid {
                return Ok(true);
            }
            candidate = tree.parent;
        }
        Ok(false)
    })
}

/// Turns a RandR monitor reply into the usable monitor rectangles, or `None`
/// when it describes none, so the caller falls back to the whole screen.
fn usable_monitors(reply: GetMonitorsReply) -> Option<Vec<Rect>> {
    let monitors: Vec<_> = reply
        .monitors
        .into_iter()
        .filter_map(|monitor| {
            let rect = Rect {
                x: i32::from(monitor.x),
                y: i32::from(monitor.y),
                width: i32::from(monitor.width),
                height: i32::from(monitor.height),
            };
            (rect.width > 0 && rect.height > 0).then_some(rect)
        })
        .collect();

    (!monitors.is_empty()).then_some(monitors)
}
