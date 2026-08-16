use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::{
    clipboard::{ClipboardService, ClipboardWriter, HistoryChangedCallback},
    history::{HistoryPayload, TextHistory},
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

        let history = Rc::new(RefCell::new(load_history(paths.clone())));
        let history_changed: HistoryChangedCallback = Rc::new(RefCell::new(None));
        let clipboard_service = ClipboardService::start(
            display.clipboard(),
            history.clone(),
            history_changed.clone(),
            paths.clone(),
        );
        let writer: ClipboardWriter = clipboard_service.writer();

        let positioner = Positioner::new(&diagnostics);
        let popup = Rc::new(popup::build(
            application,
            history.clone(),
            paths.clone(),
            {
                let history = history.clone();

                move |id| {
                    let payload = history
                        .borrow()
                        .item(id)
                        .map(|item| item.payload().clone());
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

        let pending_anchor = Rc::new(Cell::new(None));
        popup.window.connect_map({
            let positioner = positioner.clone();
            let pending_anchor = pending_anchor.clone();

            move |window| {
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
            self.popup.window.set_opacity(1.0);
            if !self.popup.window.is_active() {
                self.popup.window.present();
            }
            self.popup.focus_search();
            return;
        }

        self.popup.prepare();
        self.popup.window.set_opacity(0.0);
        gtk::prelude::WidgetExt::realize(&self.popup.window);
        if let Some(display) = gdk::Display::default() {
            display.sync();
        }
        self.pending_anchor
            .set(self.positioner.place(&self.popup.window, None).anchor());
        self.popup.present();
    }
}

fn load_history(paths: StoragePaths) -> TextHistory {
    match TextHistory::persistent(paths) {
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
