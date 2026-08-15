use adw::prelude::*;
use gtk::{gdk, glib};

pub struct PlacementPopup {
    pub window: adw::ApplicationWindow,
    pub placement_label: gtk::Label,
}

pub fn build(application: &adw::Application) -> PlacementPopup {
    let window = adw::ApplicationWindow::builder()
        .application(application)
        .title("LionClip placement test")
        .default_width(430)
        .default_height(250)
        .resizable(false)
        .build();

    let title = gtk::Label::builder()
        .label("LionClip placement test")
        .halign(gtk::Align::Start)
        .build();
    title.add_css_class("title-2");

    let description = gtk::Label::builder()
        .label("Move the pointer before opening this window, then check whether the popup appears nearby.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .build();
    description.add_css_class("dim-label");

    let placement_label = gtk::Label::builder()
        .label("Positioning: waiting for the window to map…")
        .halign(gtk::Align::Start)
        .wrap(true)
        .xalign(0.0)
        .build();
    placement_label.add_css_class("heading");

    let hint = gtk::Label::builder()
        .label("Press Esc to close")
        .halign(gtk::Align::Start)
        .build();
    hint.add_css_class("dim-label");

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(28)
        .margin_bottom(24)
        .margin_start(28)
        .margin_end(28)
        .build();
    content.append(&title);
    content.append(&description);
    content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    content.append(&placement_label);
    content.append(&hint);

    window.set_content(Some(&content));

    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key == gdk::Key::Escape {
                window.close();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    window.add_controller(keys);

    PlacementPopup {
        window,
        placement_label,
    }
}
