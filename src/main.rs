mod app;
mod clipboard;
mod history;
mod popup;
mod positioning;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
