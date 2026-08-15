mod geometry;
mod x11;

use std::env;

use gtk::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayBackend {
    Wayland,
    X11,
    Unknown,
}

impl DisplayBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Wayland => "wayland",
            Self::X11 => "x11",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionDiagnostics {
    session_type: String,
    backend: DisplayBackend,
    display_type: String,
}

impl SessionDiagnostics {
    pub fn collect() -> Self {
        let session_type = env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
        let (backend, display_type) = gtk::gdk::Display::default().map_or_else(
            || (DisplayBackend::Unknown, "unavailable".into()),
            |display| {
                let display_type = display.type_().name().to_string();
                let normalized = display_type.to_ascii_lowercase();
                let backend = if normalized.contains("wayland") {
                    DisplayBackend::Wayland
                } else if normalized.contains("x11") {
                    DisplayBackend::X11
                } else {
                    DisplayBackend::Unknown
                };
                (backend, display_type)
            },
        );

        Self {
            session_type,
            backend,
            display_type,
        }
    }

    pub fn backend(&self) -> DisplayBackend {
        self.backend
    }

    pub fn log_line(&self) -> String {
        format!(
            "lionclip: diagnostics session={} gdk_backend={} gdk_display_type={}",
            self.session_type,
            self.backend.label(),
            self.display_type
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlacementOutcome {
    X11Pointer {
        x: i32,
        y: i32,
        monitor: geometry::Rect,
    },
    CompositorFallback {
        reason: String,
    },
}

impl PlacementOutcome {
    pub fn used_pointer_placement(&self) -> bool {
        matches!(self, Self::X11Pointer { .. })
    }

    pub fn display_text(&self) -> String {
        match self {
            Self::X11Pointer { .. } => {
                "Positioning: X11 pointer experiment (verify visually)".into()
            }
            Self::CompositorFallback { .. } => {
                "Positioning: compositor-managed fallback (exact placement unavailable)".into()
            }
        }
    }

    pub fn log_line(&self) -> String {
        match self {
            Self::X11Pointer { x, y, monitor } => format!(
                "lionclip: placement backend=x11-pointer status=experimental result=placed x={x} y={y} monitor={}x{}+{}+{}",
                monitor.width, monitor.height, monitor.x, monitor.y
            ),
            Self::CompositorFallback { reason } => format!(
                "lionclip: placement backend=compositor-fallback status=not-available reason={reason}"
            ),
        }
    }
}

pub struct Positioner {
    backend: DisplayBackend,
}

impl Positioner {
    pub fn new(backend: DisplayBackend) -> Self {
        Self { backend }
    }

    pub fn place(&self, window: &adw::ApplicationWindow) -> PlacementOutcome {
        match self.backend {
            DisplayBackend::X11 => x11::place_near_pointer(window).unwrap_or_else(|error| {
                PlacementOutcome::CompositorFallback {
                    reason: sanitize_reason(&error),
                }
            }),
            DisplayBackend::Wayland => PlacementOutcome::CompositorFallback {
                reason: "wayland-does-not-allow-absolute-toplevel-placement".into(),
            },
            DisplayBackend::Unknown => PlacementOutcome::CompositorFallback {
                reason: "unsupported-gdk-backend".into(),
            },
        }
    }
}

fn sanitize_reason(error: &x11::PositionError) -> String {
    match error {
        x11::PositionError::SurfaceUnavailable => "x11-surface-unavailable",
        x11::PositionError::Connection => "x11-connection-failed",
        x11::PositionError::Query => "x11-query-failed",
        x11::PositionError::Placement => "x11-placement-failed",
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_log_is_structured() {
        let diagnostics = SessionDiagnostics {
            session_type: "wayland".into(),
            backend: DisplayBackend::Wayland,
            display_type: "GdkWaylandDisplay".into(),
        };

        assert_eq!(
            diagnostics.log_line(),
            "lionclip: diagnostics session=wayland gdk_backend=wayland gdk_display_type=GdkWaylandDisplay"
        );
    }

    #[test]
    fn fallback_log_does_not_echo_external_error_text() {
        let outcome = PlacementOutcome::CompositorFallback {
            reason: sanitize_reason(&x11::PositionError::Query),
        };

        assert_eq!(
            outcome.log_line(),
            "lionclip: placement backend=compositor-fallback status=not-available reason=x11-query-failed"
        );
    }
}
