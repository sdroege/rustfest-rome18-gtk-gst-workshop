use gio::prelude::*;
use glib;
use gtk::{self, prelude::*};

use header_bar::HeaderBar;

use std::cell::RefCell;
use std::ops;
use std::rc::{Rc, Weak};

// Our refcounted application struct for containing all the state we have to carry around.
//
// Once subclassing is possible this would become a gtk::Application subclass instead, which
// would simplify the code below considerably.
//
// This represents our main application window.
#[derive(Clone)]
pub struct App(Rc<AppInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for App {
    type Target = AppInner;

    fn deref(&self) -> &AppInner {
        &*self.0
    }
}

// Weak reference to our application struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
pub struct AppWeak(Weak<AppInner>);

impl AppWeak {
    // Upgrade to a strong reference if it still exists
    pub fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

pub struct AppInner {
    main_window: gtk::ApplicationWindow,

    // We will use this at a later time
    #[allow(dead_code)]
    header_bar: HeaderBar,
}

impl App {
    fn new(application: &gtk::Application) -> App {
        // Here build the UI but don't show it yet
        let window = gtk::ApplicationWindow::new(application);

        window.set_title("WebCam Viewer");
        window.set_border_width(5);
        window.set_position(gtk::WindowPosition::Center);
        window.set_default_size(840, 480);

        // Create headerbar for the application window
        let header_bar = HeaderBar::new(&window);

        App(Rc::new(AppInner {
            main_window: window,
            header_bar,
        }))
    }

    // Downgrade to a weak reference
    pub fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }

    pub fn on_startup(application: &gtk::Application) {
        let app = App::new(application);

        // When the application is activated show the UI. This happens when the first process is
        // started, and in the first process whenever a second process is started
        let app_weak = app.downgrade();
        application.connect_activate(move |_| {
            let app = upgrade_weak!(app_weak);
            app.on_activate();
        });

        // When the application is shut down we drop our app struct
        //
        // It has to be stored in a RefCell<Option<T>> to be able to pass it to a Fn closure. With
        // FnOnce this wouldn't be needed and the closure will only be called once, but the
        // bindings define all signal handlers as Fn.
        //
        // This is a workaround until subclassing can be used. We would then have our app struct
        // directly inside a gtk::Application subclass.
        let app_container = RefCell::new(Some(app));
        application.connect_shutdown(move |_| {
            let app = app_container
                .borrow_mut()
                .take()
                .expect("Shutdown called multiple times");
            app.on_shutdown();
        });
    }

    // Called on the first application instance whenever the first application instance is started,
    // or any future second application instance
    fn on_activate(&self) {
        // Show our window and bring it to the foreground
        self.main_window.show_all();

        // Have to call this instead of present() because of
        // https://gitlab.gnome.org/GNOME/gtk/issues/624
        self.main_window
            .present_with_time((glib::get_monotonic_time() / 1000) as u32);
    }

    // Called when the application shuts down. We drop our app struct here
    fn on_shutdown(self) {}
}
