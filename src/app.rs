use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::{
    clipboard::{ClipboardService, ClipboardWriter, HistoryChangedCallback},
    history::TextHistory,
    popup::{self, HistoryPopup},
    positioning::{Positioner, SessionDiagnostics},
    storage, unix_signals,
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
                    eprintln!("lionclip: graphical display unavailable");
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
        let diagnostics = SessionDiagnostics::collect();
        println!("{}", diagnostics.log_line());

        let history = Rc::new(RefCell::new(load_history()));
        let history_changed: HistoryChangedCallback = Rc::new(RefCell::new(None));
        let clipboard_service = ClipboardService::start(
            display.clipboard(),
            history.clone(),
            history_changed.clone(),
        );
        let writer: ClipboardWriter = clipboard_service.writer();

        let popup = Rc::new(popup::build(application, history.clone(), {
            let history = history.clone();

            move |id| {
                if let Some(item) = history.borrow().item(id) {
                    writer.restore_text(item.text());
                }
            }
        }));

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
            positioner: Positioner::new(&diagnostics),
            _clipboard_service: clipboard_service,
            _hold: application.hold(),
        })
    }

    fn show_popup(&self) {
        // Render the final content first, so both placements below measure the
        // popup the user is about to see.
        self.popup.prepare();

        if !self.popup.window.is_visible() {
            // Nothing may be visible before the popup sits at the pointer, and
            // the window keeps the frame it was hidden with, so hide the
            // content and place the surface while it is still unmapped.
            self.popup.window.set_opacity(0.0);
            // Realizing first gives even the very first open a surface to
            // place before it is mapped.
            gtk::prelude::WidgetExt::realize(&self.popup.window);
            let _ = self.positioner.place(&self.popup.window);

            // The surface may not exist yet on the very first open, and its
            // size is only final once mapped, so place again on the first
            // frame. That placement is the authoritative one, and revealing
            // after it means neither placement is ever seen happening.
            let positioner = self.positioner.clone();
            self.popup.window.add_tick_callback(move |window, _| {
                let outcome = positioner.place(window);
                println!("{}", outcome.log_line());
                window.set_opacity(1.0);
                glib::ControlFlow::Break
            });
        }

        self.popup.present();
    }
}

fn load_history() -> TextHistory {
    let path = match storage::database_path() {
        Ok(path) => path,
        Err(error) => {
            eprintln!("lionclip: persistence disabled stage=data-path error={error}");
            return TextHistory::default();
        }
    };

    match TextHistory::persistent(path) {
        Ok(history) => history,
        Err(error) => {
            eprintln!(
                "lionclip: persistence disabled stage={}",
                error.diagnostic()
            );
            TextHistory::default()
        }
    }
}
