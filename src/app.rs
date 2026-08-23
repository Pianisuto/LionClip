use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Instant,
};

use adw::prelude::*;
use gtk::{gdk, gio, gio::prelude::ApplicationCommandLineExt, glib};

use crate::{
    cli::{self, Answer, Command, PopupIntent},
    clipboard::{ClipboardService, ClipboardWriter, HistoryChangedCallback},
    history::{HistoryPayload, TextHistory},
    image_cleanup::ImageCleanupCoordinator,
    paste::{self, PasteCoordinator, PasteTarget},
    popup::{self, HistoryPopup},
    positioning::{PointerAnchor, Positioner, SessionDiagnostics},
    settings::{PreferencesWindow, SettingsService, build_preferences_window},
    storage::{self, StoragePaths},
    unix_signals,
};

pub fn run() -> glib::ExitCode {
    match cli::parse(std::env::args_os().skip(1)) {
        Ok(_) => run_application(),
        Err(answer) => report(&answer),
    }
}

/// Prints what the process can answer on its own and turns it into an exit
/// code. Errors go to stderr so `lionclip --help` stays pipeable.
fn report(answer: &Answer) -> glib::ExitCode {
    if answer.is_error() {
        eprint!("{}", answer.text());
    } else {
        print!("{}", answer.text());
    }
    glib::ExitCode::from(answer.exit_code())
}

/// Runs the command through GIO's single-instance machinery.
///
/// The first process to own the application ID becomes the resident instance
/// and builds the one clipboard monitor in `startup`; every later invocation
/// finds the name taken, hands its command line to that instance over D-Bus and
/// exits. So repeated `lionclip toggle` presses are commands to one process,
/// not new processes.
///
/// `activate` is deliberately left unconnected: the command line is the only
/// way in, which is what keeps autostart from showing the popup at login.
fn run_application() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(cli::APPLICATION_ID)
        .flags(gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();
    let state: Rc<RefCell<Option<AppState>>> = Rc::new(RefCell::new(None));

    application.connect_startup({
        let state = state.clone();

        move |application| {
            *state.borrow_mut() = AppState::new(application);
        }
    });
    application.connect_command_line({
        let state = state.clone();

        move |application, command_line| {
            let arguments = command_line.arguments();

            match cli::parse(arguments.iter().skip(1)) {
                Ok(command) => match state.borrow().as_ref() {
                    Some(state) => {
                        state.apply(command);
                        glib::ExitCode::SUCCESS
                    }
                    // Only the process that just failed to build its own state
                    // can land here: an instance without state never holds the
                    // application alive, so it is never the one a later
                    // invocation reaches. Reporting locally is therefore
                    // reporting to the caller.
                    None => {
                        eprintln!("lionclip: graphical display or data path unavailable");
                        application.quit();
                        glib::ExitCode::FAILURE
                    }
                },
                // Unreachable in practice, because every process answers these
                // before it registers, but the remote argv is still input.
                Err(answer) => report(&answer),
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
    paste: PasteCoordinator,
    /// The single preferences window instance. Like the popup, it hides
    /// rather than closes, so every `lionclip settings` invocation and every
    /// "Preferences" menu click reuses it instead of building a second one.
    settings_window: Rc<PreferencesWindow>,
    /// Pointer sample of the placement made before the popup was mapped, so the
    /// placement that runs at map time agrees with it.
    pending_anchor: Rc<Cell<Option<PointerAnchor>>>,
    /// The auto-paste target captured the moment the popup was shown, before
    /// its own window had a surface. See `PasteCoordinator::capture_target`.
    pending_paste_target: Rc<Cell<Option<PasteTarget>>>,
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

        let settings = Rc::new(SettingsService::open());
        let image_cleanup = ImageCleanupCoordinator::new(paths.clone());
        let history = Rc::new(RefCell::new(load_history(
            paths.clone(),
            settings.history_limit() as usize,
            image_cleanup.clone(),
        )));
        let history_changed: HistoryChangedCallback = Rc::new(RefCell::new(None));
        let clipboard_service = ClipboardService::start(
            display.clipboard(),
            history.clone(),
            history_changed.clone(),
            paths,
            image_cleanup,
            settings.clone(),
        );
        let writer: ClipboardWriter = clipboard_service.writer();

        let positioner = Positioner::new(&diagnostics);
        let paste = PasteCoordinator::new(&diagnostics);
        let pending_paste_target: Rc<Cell<Option<PasteTarget>>> = Rc::new(Cell::new(None));

        // Built before the popup so the popup's "Preferences" menu item can
        // hold a handle to it; it only ever needs `history_changed`'s shared
        // callback cell to ask the popup to refresh, not the popup itself, so
        // construction order between the two does not otherwise matter.
        let settings_window = Rc::new(build_preferences_window(
            application,
            settings.clone(),
            history.clone(),
            writer.clone(),
            paste.is_available(),
            {
                let history_changed = history_changed.clone();

                move || {
                    if let Some(callback) = history_changed.borrow().as_ref() {
                        callback();
                    }
                }
            },
        ));

        // The restore closure needs the popup's own window to tell "our popup
        // is still closing" from "the user moved to another application", but
        // it is built before the popup exists, so the window is bound right
        // after `popup::build` returns and read only when an item is chosen.
        let popup_window: Rc<RefCell<Option<adw::ApplicationWindow>>> = Rc::new(RefCell::new(None));

        let popup = Rc::new(popup::build(
            application,
            history.clone(),
            settings.clone(),
            writer.clone(),
            {
                let history = history.clone();
                let settings = settings.clone();
                let pending_paste_target = pending_paste_target.clone();
                let popup_window = popup_window.clone();

                move |id| {
                    // Measured from the moment the user's choice reaches the
                    // application, so the diagnostic below covers everything
                    // LionClip is responsible for — not just the X11 exchange
                    // the paste backend times on its own.
                    let activated = Instant::now();
                    let payload = history.borrow().item(id).map(|item| item.payload().clone());
                    // The target was captured once, when the popup opened;
                    // it is not consumed here, because Up/Down navigation and
                    // other non-activating interactions in between must not
                    // change what a later activation pastes into.
                    let target = pending_paste_target.get();
                    let restored = match payload {
                        Some(HistoryPayload::Text(text)) => Some(writer.restore_text(&text)),
                        Some(HistoryPayload::Image(image)) => Some(writer.restore_image(&image)),
                        None => None,
                    };
                    let Some(restore_succeeded) = restored else {
                        return;
                    };
                    // Image restore reads the stored blob synchronously, so
                    // this is the one part of the path whose cost depends on
                    // the item rather than on the session.
                    let restore_ms = activated.elapsed().as_millis();

                    let behavior =
                        paste::decide(settings.auto_paste(), target.is_some(), restore_succeeded);
                    if let (paste::SelectionBehavior::RestoreAndPaste, Some(target), Some(window)) =
                        (behavior, target, popup_window.borrow().as_ref())
                    {
                        paste.request_paste(target, window, move |sent| {
                            println!(
                                "lionclip: auto-paste result sent={sent} restore_ms={restore_ms} \
                                 activation_to_keys_ms={}",
                                activated.elapsed().as_millis()
                            );
                        });
                    }
                }
            },
            {
                let positioner = positioner.clone();

                move |window| positioner.holds_keyboard_focus(window)
            },
            {
                let settings_window = settings_window.clone();

                move || settings_window.present()
            },
        ));

        *popup_window.borrow_mut() = Some(popup.window.clone());

        // A mapped popup has its final size, so this is the authoritative
        // placement. Rendering is never gated on a map or frame callback: a
        // missed callback must not leave a window reported as visible but with
        // no usable contents.
        let pending_anchor = Rc::new(Cell::new(None));
        popup.window.connect_map({
            let positioner = positioner.clone();
            let pending_anchor = pending_anchor.clone();

            move |window| {
                // The mapped size is final here, so this placement is the
                // authoritative one; it reuses the pointer sample of the
                // placement made before mapping.
                let started = Instant::now();
                let outcome = positioner.place(window, pending_anchor.take());
                println!(
                    "{} map_place_us={}",
                    outcome.log_line(),
                    started.elapsed().as_micros()
                );
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

        // The resident history, not the preferences window, is what has to
        // follow the stored limit: a `gsettings`/dconf write from outside
        // this process changes the persisted value and what Preferences
        // shows the next time it opens, but nothing else would tell the
        // running `TextHistory` to shrink. Subscribing to the key itself
        // covers both origins with one path, and needs no polling because
        // GSettings already delivers the change.
        settings.connect_history_limit_changed({
            let history = history.clone();
            let history_changed = history_changed.clone();

            move |limit| {
                history.borrow_mut().set_unpinned_limit(limit as usize);
                if let Some(callback) = history_changed.borrow().as_ref() {
                    callback();
                }
            }
        });

        Some(Self {
            history,
            popup,
            positioner,
            paste,
            settings_window,
            pending_anchor,
            pending_paste_target,
            _clipboard_service: clipboard_service,
            _hold: application.hold(),
        })
    }

    /// Applies one command to the popup of the resident instance.
    fn apply(&self, command: Command) {
        if command == Command::Settings {
            self.settings_window.present();
            return;
        }
        match command.intent(self.popup.window.is_visible()) {
            PopupIntent::Show => self.show_popup(),
            PopupIntent::Hide => self.popup.hide(),
            PopupIntent::Leave => {}
        }
    }

    fn show_popup(&self) {
        if self.popup.window.is_visible() {
            // Already open: leave it exactly where it is. Presenting a window
            // that is already on screen lets the compositor lay the toplevel
            // out again, which reads as the popup jumping.
            if !self.popup.window.is_active() {
                // Open but not focused is not a state the popup should be able
                // to reach, and invoking it is the natural way to ask for it
                // back, so raise it rather than leaving it unusable.
                self.popup.window.present();
            }
            self.popup.focus_search();
            return;
        }

        // Every step below runs on the GTK main thread and several of them
        // are synchronous X round trips, so each is timed separately: this
        // is the stretch between the shortcut being pressed and the popup
        // appearing, and anything slow here is felt directly.
        let started = Instant::now();

        // Captured first, while the popup is confirmed not visible: the
        // target is whatever held X input focus right before LionClip
        // opened, not whatever ends up focused after it closes.
        self.pending_paste_target.set(self.paste.capture_target());
        let capture_target = started.elapsed();

        // Render the final content first, so both placements below measure the
        // popup the user is about to see.
        let phase = Instant::now();
        self.popup.prepare();
        let prepare = phase.elapsed();

        // Realizing first gives even the very first open a surface to place
        // before it is mapped. Do not hide it with window opacity while it
        // opens: the compositor can keep a mapped window in that state even
        // after GTK considers it visible, making later commands unreachable.
        let phase = Instant::now();
        gtk::prelude::WidgetExt::realize(&self.popup.window);
        let realize = phase.elapsed();
        // Placement runs on its own X connection, so a still-pending unmap from
        // a previous open could otherwise be processed after this move and
        // leave the popup at its old position.
        let phase = Instant::now();
        if let Some(display) = gdk::Display::default() {
            display.sync();
        }
        let display_sync = phase.elapsed();

        let phase = Instant::now();
        self.pending_anchor
            .set(self.positioner.place(&self.popup.window, None).anchor());
        let place = phase.elapsed();

        let phase = Instant::now();
        self.popup.present();
        let present = phase.elapsed();

        println!(
            "lionclip: popup open capture_target_us={} prepare_us={} realize_us={} \
             display_sync_us={} place_us={} present_us={} total_us={}",
            capture_target.as_micros(),
            prepare.as_micros(),
            realize.as_micros(),
            display_sync.as_micros(),
            place.as_micros(),
            present.as_micros(),
            started.elapsed().as_micros()
        );
    }
}

fn load_history(
    paths: StoragePaths,
    unpinned_limit: usize,
    image_cleanup: ImageCleanupCoordinator,
) -> TextHistory {
    match TextHistory::persistent_with_cleanup(paths, unpinned_limit, image_cleanup.clone()) {
        Ok(history) => history,
        Err(error) => {
            eprintln!(
                "lionclip: persistence disabled stage={}",
                error.diagnostic()
            );
            TextHistory::in_memory_with_cleanup(unpinned_limit, image_cleanup)
        }
    }
}
