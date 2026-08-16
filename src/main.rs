mod app;
mod clipboard;
mod history;
mod image_cleanup;
mod image_store;
mod popup;
mod positioning;
mod storage;
mod unix_signals;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
