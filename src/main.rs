mod app;
mod clipboard;
mod history;
mod popup;
mod positioning;
mod storage;
mod unix_signals;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
