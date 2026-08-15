use adw::prelude::*;
use gtk::glib;

use crate::{
    popup,
    positioning::{PlacementOutcome, Positioner, SessionDiagnostics},
};

const APPLICATION_ID: &str = "io.github.Pianisuto.LionClip";

pub fn run() -> glib::ExitCode {
    let application = adw::Application::builder()
        .application_id(APPLICATION_ID)
        .build();

    application.connect_activate(activate);
    application.run()
}

fn activate(application: &adw::Application) {
    if let Some(window) = application.active_window() {
        window.present();
        return;
    }

    let popup = popup::build(application);
    let diagnostics = SessionDiagnostics::collect();
    println!("{}", diagnostics.log_line());

    let positioner = Positioner::new(diagnostics.backend());

    popup.window.add_tick_callback({
        let placement_label = popup.placement_label.clone();

        move |window, _| {
            let outcome = positioner.place(window);
            println!("{}", outcome.log_line());
            placement_label.set_label(&outcome.display_text());
            apply_outcome_style(&placement_label, &outcome);
            glib::ControlFlow::Break
        }
    });

    popup.window.present();
}

fn apply_outcome_style(label: &gtk::Label, outcome: &PlacementOutcome) {
    label.remove_css_class("accent");
    label.remove_css_class("warning");

    if outcome.used_pointer_placement() {
        label.add_css_class("accent");
    } else {
        label.add_css_class("warning");
    }
}
