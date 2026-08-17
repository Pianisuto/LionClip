//! Auto-paste: restoring the selected item to the clipboard and, only when
//! it is safe to do so, asking the application that had focus before
//! LionClip opened to paste it.
//!
//! This mirrors `src/positioning/mod.rs`'s shape on purpose: a small,
//! concrete coordinator picks a backend once from session diagnostics, and
//! all platform-specific work lives in an isolated submodule so it cannot
//! leak into UI code. There is exactly one real backend (X11) and one
//! degenerate "unavailable" case, which is why this is a concrete `enum`
//! dispatch rather than a trait object.

mod x11;

use crate::positioning::SessionDiagnostics;

pub use x11::PasteTarget;

/// How the popup's selection should be handled once the clipboard has been
/// updated with the chosen item. Pure and fully unit-testable: it never
/// touches GTK or X11.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionBehavior {
    RestoreOnly,
    RestoreAndPaste,
}

/// Decides whether an auto-paste attempt should follow a restore.
///
/// `has_target` is `false` whenever no target was captured at all (backend
/// unavailable, or nothing meaningfully focused before LionClip opened).
/// Whether the target is *still* valid by the time the user picks an item is
/// a separate, later question the backend itself answers when it tries to
/// act, because the answer can only be known at that moment.
pub fn decide(
    auto_paste_enabled: bool,
    has_target: bool,
    restore_succeeded: bool,
) -> SelectionBehavior {
    if auto_paste_enabled && has_target && restore_succeeded {
        SelectionBehavior::RestoreAndPaste
    } else {
        SelectionBehavior::RestoreOnly
    }
}

#[derive(Clone, Copy)]
enum Backend {
    X11,
    Unavailable,
}

/// Picks a paste backend once per process, the same way `Positioner` picks a
/// placement backend, and dispatches to it.
#[derive(Clone, Copy)]
pub struct PasteCoordinator {
    backend: Backend,
}

impl PasteCoordinator {
    pub fn new(diagnostics: &SessionDiagnostics) -> Self {
        Self {
            backend: if diagnostics.is_x11() {
                Backend::X11
            } else {
                Backend::Unavailable
            },
        }
    }

    /// Whether this session can attempt auto-paste at all. The preferences
    /// window uses this to explain an unavailable toggle rather than let the
    /// user turn on a setting that can never do anything on this session.
    pub fn is_available(&self) -> bool {
        matches!(self.backend, Backend::X11)
    }

    /// Captures the paste target. Must be called while LionClip's own popup
    /// is not visible, so the query cannot observe LionClip's own window as
    /// the focused one — an unmapped window can never hold the X input
    /// focus regardless of whether it was realized in an earlier open.
    /// `None` covers both an unavailable backend and a backend that could
    /// not identify a safe target.
    pub fn capture_target(&self) -> Option<PasteTarget> {
        match self.backend {
            Backend::X11 => x11::capture_target().ok(),
            Backend::Unavailable => None,
        }
    }

    /// Restores focus to `target` and, only once the server confirms the
    /// target still owns it, synthesizes Ctrl+V. `own_window` is LionClip's
    /// own popup, so the backend can tell "our window is still closing" from
    /// "the user moved to another application" and decline to steal focus
    /// back in the latter case. Calls `on_done(true)` only if the key
    /// combination was actually sent; every failure path (destroyed target,
    /// activation not confirmed in time, backend unavailable) calls
    /// `on_done(false)` instead of guessing. Runs off the GTK main thread.
    pub fn request_paste(
        &self,
        target: PasteTarget,
        own_window: &adw::ApplicationWindow,
        on_done: impl FnOnce(bool) + 'static,
    ) {
        match self.backend {
            Backend::X11 => x11::request_paste(target, x11::window_xid(own_window), on_done),
            Backend::Unavailable => on_done(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_paste_off_never_pastes_regardless_of_target_or_restore_outcome() {
        assert_eq!(decide(false, true, true), SelectionBehavior::RestoreOnly);
        assert_eq!(decide(false, false, true), SelectionBehavior::RestoreOnly);
    }

    #[test]
    fn auto_paste_on_with_a_valid_target_and_successful_restore_pastes() {
        assert_eq!(decide(true, true, true), SelectionBehavior::RestoreAndPaste);
    }

    #[test]
    fn auto_paste_on_without_a_captured_target_only_restores() {
        assert_eq!(decide(true, false, true), SelectionBehavior::RestoreOnly);
    }

    #[test]
    fn a_failed_restore_never_pastes_even_with_auto_paste_on_and_a_valid_target() {
        assert_eq!(decide(true, true, false), SelectionBehavior::RestoreOnly);
    }
}
