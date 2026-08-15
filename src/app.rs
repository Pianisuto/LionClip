use std::{cell::Cell, rc::Rc};

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

    let positioner = Rc::new(Positioner::new(diagnostics.backend()));
    let placement_attempted = Rc::new(Cell::new(false));

    popup.window.connect_map({
        let positioner = Rc::clone(&positioner);
        let placement_attempted = Rc::clone(&placement_attempted);
        let placement_label = popup.placement_label.clone();

        move |window| {
            if placement_attempted.replace(true) {
                return;
            }

            let outcome = positioner.place(window);
            println!("{}", outcome.log_line());
            placement_label.set_label(&outcome.display_text());
            apply_outcome_style(&placement_label, &outcome);
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
