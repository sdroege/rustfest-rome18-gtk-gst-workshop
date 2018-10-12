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

// Our refcounted application struct for containing all the
// state we have to carry around
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

fn build_actions(_app: &App, application: &gtk::Application) {
    // Create app.about action for the about dialog
    //
    // This can be activated from anywhere where we have access
    // to the application, not just the main window
    let about = gio::SimpleAction::new("about", None);

    // When activated, show an about dialog
    let weak_application = application.downgrade();
    about.connect_activate(move |_action, _parameter| {
        let application = upgrade_weak!(weak_application);

        let p = gtk::AboutDialog::new();

        p.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
        p.set_website_label("github repository");
        p.set_website("https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop");
        p.set_comments("A webcam viewer written with gtk-rs and gstreamer-rs");
        p.set_copyright("This is under MIT license");
        if let Some(window) = application.get_active_window() {
            p.set_transient_for(&window);
        }
        p.set_modal(true);
        p.set_program_name("RustFest 2018 GTK+ & GStreamer WebCam Viewer");

        // When any response on the dialog happens, we simply destroy it.
        //
        // We don't have any custom buttons added so this will only ever
        // handle the close button, otherwise we could distinguish the
        // buttons by the response
        p.connect_response(|dialog, _response| {
            dialog.destroy();
        });

        p.show_all();
    });

    application.add_action(&about);
}

fn build_ui(app: &App, application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);
    app.0.borrow_mut().main_window = Some(window.clone());

    window.set_title("RustFest 2018 GTK+ & GStreamer WebCam Viewer");
    window.set_border_width(5);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(350, 300);

    // When our main window is closed, the whole application should be
    // shut down
    let application_weak = application.downgrade();
    window.connect_delete_event(move |_, _| {
        let application = upgrade_weak!(application_weak, Inhibit(false));
        application.quit();
        Inhibit(false)
    });

    // Create headerbar for the application, including the main
    // menu and a close button
    let header_bar = gtk::HeaderBar::new();
    header_bar.set_show_close_button(true);

    let main_menu = gtk::MenuButton::new();
    let main_menu_image = gtk::Image::new_from_icon_name("open-menu-symbolic", 1);
    main_menu.add(&main_menu_image);

    // For now the main menu only contains the about dialog
    let main_menu_model = gio::Menu::new();
    main_menu_model.append("About", "app.about");
    main_menu.set_menu_model(&main_menu_model);

    header_bar.pack_end(&main_menu);
    window.set_titlebar(&header_bar);
}

fn main() {
    let app = App::new();

    let application = gtk::Application::new("com.github.rustfest", gio::ApplicationFlags::empty())
        .expect("Initialization failed...");

    // On application startup (of the main instance) we create
    // the actions and UI. A second process would not run this
    let app_weak = app.downgrade();
    application.connect_startup(move |application| {
        let app = upgrade_weak!(app_weak);
        build_actions(&app, application);
        // Build the UI but don't show it yet
        build_ui(&app, application);
    });

    // When the application is activated show the UI. This happens
    // when the first process is started, and in the first process
    // whenever a second process is started
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

    // This takes ownership of our App struct so it stays
    // alive as long as the application does
    application.connect_shutdown(move |_| {
        let _ = app;
    });

    // And now run the application until the end
    application.run(&args().collect::<Vec<_>>());
}
