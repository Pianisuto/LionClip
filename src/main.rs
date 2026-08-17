mod app;
mod cli;
mod clipboard;
mod history;
mod image_cleanup;
mod image_store;
mod paste;
mod popup;
mod positioning;
mod settings;
mod storage;
mod unix_signals;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
