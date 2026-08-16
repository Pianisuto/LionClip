use gdk4_x11::X11Surface;
use gtk::prelude::*;
use x11rb::{
    connection::Connection,
    properties::{WmSizeHints, WmSizeHintsSpecification},
    protocol::{
        randr::ConnectionExt as _,
        xproto::{ConfigureWindowAux, ConnectionExt as _},
    },
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

pub fn place_near_pointer(
    window: &adw::ApplicationWindow,
    status: X11PathStatus,
    anchor: Option<PointerAnchor>,
) -> Result<PlacementOutcome, PositionError> {
    let surface = window
        .surface()
        .and_then(|surface| surface.downcast::<X11Surface>().ok())
        .ok_or(PositionError::SurfaceUnavailable)?;
    let xid = surface
        .xid()
        .try_into()
        .map_err(|_| PositionError::SurfaceUnavailable)?;

    let (connection, screen_number) =
        x11rb::connect(None).map_err(|_| PositionError::Connection)?;
    let screen = connection
        .setup()
        .roots
        .get(screen_number)
        .ok_or(PositionError::Query)?;

    let pointer = match anchor {
        Some(PointerAnchor(pointer)) => pointer,
        None => {
            let pointer_reply = connection
                .query_pointer(screen.root)
                .map_err(|_| PositionError::Query)?
                .reply()
                .map_err(|_| PositionError::Query)?;
            Point {
                x: i32::from(pointer_reply.root_x),
                y: i32::from(pointer_reply.root_y),
            }
        }
    };

    let window_geometry = connection
        .get_geometry(xid)
        .map_err(|_| PositionError::Query)?
        .reply()
        .map_err(|_| PositionError::Query)?;
    let popup_size = Size {
        width: i32::from(window_geometry.width),
        height: i32::from(window_geometry.height),
    };

    let monitors = query_monitors(&connection, screen.root).unwrap_or_else(|| {
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

    announce_position(&connection, xid, origin)?;
    connection
        .configure_window(xid, &ConfigureWindowAux::new().x(origin.x).y(origin.y))
        .map_err(|_| PositionError::Placement)?
        .check()
        .map_err(|_| PositionError::Placement)?;
    connection.flush().map_err(|_| PositionError::Placement)?;

    Ok(PlacementOutcome::X11Pointer {
        x: origin.x,
        y: origin.y,
        monitor,
        status,
        anchor: PointerAnchor(pointer),
    })
}

/// Whether the popup's surface still owns the X keyboard focus.
///
/// A keyboard grab — a desktop shortcut being pressed, a menu opening —
/// deactivates the toplevel without taking the input focus away from it, while
/// another window taking over does move the focus. Telling those apart is what
/// keeps the popup open while its own shortcut is pressed.
pub fn holds_keyboard_focus(window: &adw::ApplicationWindow) -> Result<bool, PositionError> {
    let surface = window
        .surface()
        .and_then(|surface| surface.downcast::<X11Surface>().ok())
        .ok_or(PositionError::SurfaceUnavailable)?;
    let xid: u32 = surface
        .xid()
        .try_into()
        .map_err(|_| PositionError::SurfaceUnavailable)?;

    let (connection, _) = x11rb::connect(None).map_err(|_| PositionError::Connection)?;
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
    // manager may also reparent the toplevel into a frame.
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
}

/// Records the placement as a user-specified position in `WM_NORMAL_HINTS`.
///
/// A window manager places a window it has not managed yet by its own policy
/// and ignores the coordinates the client set before mapping. Announcing the
/// position as user-specified makes it honour the placement instead, so the
/// popup is mapped where it belongs rather than moved there afterwards. The
/// existing hints are read back first so GTK's own size constraints survive.
fn announce_position<C: Connection>(
    connection: &C,
    window: u32,
    origin: Point,
) -> Result<(), PositionError> {
    let mut hints = WmSizeHints::get_normal_hints(connection, window)
        .map_err(|_| PositionError::Placement)?
        .reply()
        .map_err(|_| PositionError::Placement)?
        .unwrap_or_default();
    hints.position = Some((WmSizeHintsSpecification::UserSpecified, origin.x, origin.y));
    hints
        .set_normal_hints(connection, window)
        .map_err(|_| PositionError::Placement)?
        .check()
        .map_err(|_| PositionError::Placement)?;
    Ok(())
}

fn query_monitors<C: Connection>(connection: &C, root: u32) -> Option<Vec<Rect>> {
    let reply = connection
        .randr_get_monitors(root, true)
        .ok()?
        .reply()
        .ok()?;
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
