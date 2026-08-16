use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::{
    clipboard::{ClipboardService, ClipboardWriter, HistoryChangedCallback},
    history::{HistoryPayload, TextHistory},
    image_cleanup::ImageCleanupCoordinator,
    popup::{self, HistoryPopup},
    positioning::{PointerAnchor, Positioner, SessionDiagnostics},
    storage::{self, StoragePaths},
    unix_signals,
};

const APPLICATION_ID: &str = "io.github.Pianisuto.LionClip";

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();
    let state = Rc::new(RefCell::new(None));

    application.connect_activate({
        let state = state.clone();

        move |application| {
            if state.borrow().is_none() {
                let Some(new_state) = AppState::new(application) else {
                    eprintln!("lionclip: graphical display or data path unavailable");
                    application.quit();
                    return;
                };
                *state.borrow_mut() = Some(new_state);
            }

            if let Some(state) = state.borrow().as_ref() {
                state.show_popup();
            }
        }
    });
    application.connect_shutdown({
        let state = state.clone();

        move |_| {
            state.borrow_mut().take();
        }
    });

    let quit_signal_sources = unix_signals::install_quit_handlers(application.upcast_ref());
    let exit_code = application.run();
    quit_signal_sources.remove();
    exit_code
}

struct AppState {
    history: Rc<RefCell<TextHistory>>,
    popup: Rc<HistoryPopup>,
    positioner: Positioner,
    /// Pointer sample of the placement made before the popup was mapped, so the
    /// placement that runs at map time agrees with it.
    pending_anchor: Rc<Cell<Option<PointerAnchor>>>,
    _clipboard_service: ClipboardService,
    _hold: gio::ApplicationHoldGuard,
}

impl Drop for AppState {
    fn drop(&mut self) {
        self.history.borrow_mut().shutdown_persistence();
    }
}

impl AppState {
    fn new(application: &adw::Application) -> Option<Self> {
        let display = gdk::Display::default()?;
        let paths = match storage::paths() {
            Ok(paths) => paths,
            Err(error) => {
                eprintln!("lionclip: storage unavailable stage=data-path error={error}");
                return None;
            }
        };
        let diagnostics = SessionDiagnostics::collect();
        println!("{}", diagnostics.log_line());

        let image_cleanup = ImageCleanupCoordinator::new(paths.clone());
        let history = Rc::new(RefCell::new(load_history(
            paths.clone(),
            image_cleanup.clone(),
        )));
        let history_changed: HistoryChangedCallback = Rc::new(RefCell::new(None));
        let clipboard_service = ClipboardService::start(
            display.clipboard(),
            history.clone(),
            history_changed.clone(),
            paths,
            image_cleanup,
        );
        let writer: ClipboardWriter = clipboard_service.writer();

        let positioner = Positioner::new(&diagnostics);
        let popup = Rc::new(popup::build(
            application,
            history.clone(),
            {
                let history = history.clone();

                move |id| {
                    let payload = history.borrow().item(id).map(|item| item.payload().clone());
                    match payload {
                        Some(HistoryPayload::Text(text)) => writer.restore_text(&text),
                        Some(HistoryPayload::Image(image)) => writer.restore_image(&image),
                        None => {}
                    }
                }
            },
            {
                let positioner = positioner.clone();

                move |window| positioner.holds_keyboard_focus(window)
            },
        ));

        // Revealing happens when the window is mapped, never from a frame
        // clock: a popup that maps without becoming visible gets no frames, and
        // the reveal would never run, leaving a mapped but fully transparent
        // window that `is_visible` still reports as open.
        let pending_anchor = Rc::new(Cell::new(None));
        popup.window.connect_map({
            let positioner = positioner.clone();
            let pending_anchor = pending_anchor.clone();

            move |window| {
                // The mapped size is final here, so this placement is the
                // authoritative one; it reuses the pointer sample of the
                // placement made before mapping.
                let outcome = positioner.place(window, pending_anchor.get());
                println!("{}", outcome.log_line());
                window.set_opacity(1.0);
            }
        });

        *history_changed.borrow_mut() = Some(Box::new({
            let popup = popup.clone();

            move || {
                if popup.window.is_visible() {
                    popup.refresh();
                }
            }
        }));

        Some(Self {
            history,
            popup,
            positioner,
            pending_anchor,
            _clipboard_service: clipboard_service,
            _hold: application.hold(),
        })
    }

    fn show_popup(&self) {
        if self.popup.window.is_visible() {
            // Already open: leave it exactly where it is. Presenting a window
            // that is already on screen lets the compositor lay the toplevel
            // out again, which reads as the popup jumping. The opacity is
            // restored defensively, so an open popup can never stay invisible.
            self.popup.window.set_opacity(1.0);
            if !self.popup.window.is_active() {
                // Open but not focused is not a state the popup should be able
                // to reach, and invoking it is the natural way to ask for it
                // back, so raise it rather than leaving it unusable.
                self.popup.window.present();
            }
            self.popup.focus_search();
            return;
        }

        // Render the final content first, so both placements below measure the
        // popup the user is about to see.
        self.popup.prepare();

        // Nothing may be visible before the popup sits at the pointer, and the
        // window keeps the frame it was hidden with, so hide the content and
        // place the surface while it is still unmapped.
        self.popup.window.set_opacity(0.0);
        // Realizing first gives even the very first open a surface to place
        // before it is mapped.
        gtk::prelude::WidgetExt::realize(&self.popup.window);
        // Placement runs on its own X connection, so a still-pending unmap from
        // a previous open could otherwise be processed after this move and
        // leave the popup at its old position.
        if let Some(display) = gdk::Display::default() {
            display.sync();
        }
        self.pending_anchor
            .set(self.positioner.place(&self.popup.window, None).anchor());

        self.popup.present();
    }
}

fn load_history(paths: StoragePaths, image_cleanup: ImageCleanupCoordinator) -> TextHistory {
    match TextHistory::persistent_with_cleanup(paths, image_cleanup.clone()) {
        Ok(history) => history,
        Err(error) => {
            eprintln!(
                "lionclip: persistence disabled stage={}",
                error.diagnostic()
            );
            TextHistory::in_memory_with_cleanup(image_cleanup)
        }
    }
}
