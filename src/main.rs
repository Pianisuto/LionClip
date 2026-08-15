mod app;
mod clipboard;
mod history;
mod popup;
mod positioning;
mod storage;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
