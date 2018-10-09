extern crate gio;
extern crate gtk;

use gio::prelude::*;
use gio::MenuExt;
use gtk::prelude::*;

use std::cell::RefCell;
use std::env::args;
use std::rc::{Rc, Weak};

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

#[derive(Clone)]
struct App(Rc<RefCell<AppInner>>);

struct AppWeak(Weak<RefCell<AppInner>>);

impl App {
    fn new() -> App {
        App(Rc::new(RefCell::new(AppInner { main_window: None })))
    }

    fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }
}

impl AppWeak {
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

struct AppInner {
    main_window: Option<gtk::ApplicationWindow>,
}

fn build_menu(_app: &App, application: &gtk::Application) {
    let menu = gio::Menu::new();

    menu.append("Quit", "app.quit");

    let about_menu = gio::MenuItem::new("About", "app.about");
    menu.append_item(&about_menu);

    // Connect the about menu entry click event.
    let about = gio::SimpleAction::new("about", None);
    let weak_application = application.downgrade();
    about.connect_activate(move |_, _| {
        let application = upgrade_weak!(weak_application);
        let p = gtk::AboutDialog::new();
        p.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
        p.set_website_label(Some("github repository"));
        p.set_website(Some(
            "https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop",
        ));
        p.set_comments(Some("A webcam viewer written with gtk-rs and gstreamer-rs"));
        p.set_copyright(Some("This is under MIT license"));
        if let Some(window) = application.get_active_window() {
            p.set_transient_for(Some(&window));
        }
        p.set_modal(true);
        p.set_program_name("RustFest GTK+GStreamer");
        p.show_all();
    });

    let quit = gio::SimpleAction::new("quit", None);
    let weak_application = application.downgrade();
    quit.connect_activate(move |_, _| {
        let application = upgrade_weak!(weak_application);
        application.quit();
    });

    application.add_action(&about);
    application.add_action(&quit);

    application.set_app_menu(&menu);
}

fn build_ui(app: &App, application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);
    app.0.borrow_mut().main_window = Some(window.clone());

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
}

fn main() {
    let app = App::new();

    let application = gtk::Application::new("com.github.rustfest", gio::ApplicationFlags::empty())
        .expect("Initialization failed...");

    let app_weak = app.downgrade();
    application.connect_startup(move |application| {
        let app = upgrade_weak!(app_weak);
        // Here we build the application menu, which is application global
        build_menu(&app, application);

        // And then build the UI but don't show it yet
        build_ui(&app, application);
    });

    let app_weak = app.downgrade();
    application.connect_activate(move |_| {
        let app = upgrade_weak!(app_weak);
        let inner = app.0.borrow();
        // We only show our window here once the application
        // is activated. This means that when a second instance
        // is started, the window of the first instance will be
        // brought to the foreground
        if let Some(ref main_window) = inner.main_window {
            main_window.show_all();
            main_window.present();
        }
    });

    // This takes ownership of our App struct
    let app = Some(app);
    application.connect_shutdown(move |_| {
        let _ = app;
    });

    application.run(&args().collect::<Vec<_>>());
}
