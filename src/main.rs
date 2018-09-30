extern crate gio;
extern crate gtk;

use gio::prelude::*;
use gio::MenuExt;
use gtk::prelude::*;

use std::env::args;

macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => return $r,
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

fn build_menu(application: &gtk::Application, window: &gtk::ApplicationWindow) {
    let menu = gio::Menu::new();

    menu.append("Quit", "app.quit");

    let about_menu = gio::MenuItem::new("About", "app.about");
    menu.append_item(&about_menu);

    // Connect the about menu entry click event.
    let about = gio::SimpleAction::new("about", None);
    let weak_window = window.downgrade();
    about.connect_activate(move |_, _| {
        let window = upgrade_weak!(weak_window);
        let p = gtk::AboutDialog::new();
        p.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
        p.set_website_label(Some("github repository"));
        p.set_website(Some(
            "https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop",
        ));
        p.set_comments(Some("A webcam viewer written with gtk-rs and gstreamer-rs"));
        p.set_copyright(Some("This is under MIT license"));
        p.set_transient_for(Some(&window));
        p.set_modal(true);
        p.set_program_name("RustFest GTK+GStreamer");
        p.show_all();
    });
    let quit = gio::SimpleAction::new("quit", None);
    let weak_window = window.downgrade();
    quit.connect_activate(move |_, _| {
        let window = upgrade_weak!(weak_window);
        window.destroy();
    });

    application.add_action(&about);
    application.add_action(&quit);

    application.set_app_menu(&menu);
}

fn build_ui(application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);

    window.set_title("RustFest 2018 GTK+GStreamer");
    window.set_border_width(5);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(350, 300);

    window.connect_delete_event(move |win, _| {
        win.destroy();
        Inhibit(false)
    });

    let combo_box = gtk::ComboBoxText::new();
    combo_box.append_text("<Pick a video input>");
    combo_box.set_active(0);

    // TODO: put different possible video inputs here. For now it's just for the show up
    combo_box.append_text("Video input 3");
    combo_box.append_text("Video input 1337");

    combo_box.connect_property_active_notify(|cb| {
        if let Some(active) = cb.get_active_text() {
            if active != "<Pick a video input>" {
                println!("new active: {}", active);
            }
        }
    });

    let vertical_layout = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vertical_layout.pack_start(&combo_box, false, true, 0);
    let draw_area = gtk::DrawingArea::new();
    vertical_layout.pack_start(&draw_area, true, true, 0);

    window.add(&vertical_layout);

    build_menu(application, &window);

    window.show_all();
}

fn main() {
    let application = gtk::Application::new("com.github.rustfest",
                                            gio::ApplicationFlags::empty())
                                       .expect("Initialization failed...");

    application.connect_startup(|app| {
        build_ui(app);
    });

    application.run(&args().collect::<Vec<_>>());
}
