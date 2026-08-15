use gio::prelude::ApplicationExt;
use gtk::{gio, glib};

pub struct QuitSignalSources([glib::SourceId; 2]);

impl QuitSignalSources {
    pub fn remove(self) {
        for source in self.0 {
            source.remove();
        }
    }
}

pub fn install_quit_handlers(application: &gio::Application) -> QuitSignalSources {
    QuitSignalSources([
        install_quit_handler(application, libc::SIGTERM, "sigterm"),
        install_quit_handler(application, libc::SIGINT, "sigint"),
    ])
}

fn install_quit_handler(
    application: &gio::Application,
    signal: i32,
    signal_name: &'static str,
) -> glib::SourceId {
    let application = application.clone();
    glib_unix::unix_signal_add_local(signal, move || {
        println!("lionclip: shutdown requested signal={signal_name}");
        application.quit();
        glib::ControlFlow::Continue
    })
}
