use gio::{self, prelude::*};
use glib;
use gtk::{self, prelude::*};

use about_dialog::show_about_dialog;
use header_bar::HeaderBar;
use pipeline::Pipeline;

use std::cell::RefCell;
use std::error;
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

    pipeline: Pipeline,
}

impl App {
    fn new(application: &gtk::Application) -> Result<App, Box<dyn error::Error>> {
        // Here build the UI but don't show it yet
        let window = gtk::ApplicationWindow::new(application);

        window.set_title("WebCam Viewer");
        window.set_border_width(5);
        window.set_position(gtk::WindowPosition::Center);
        window.set_default_size(840, 480);

        // Create headerbar for the application window
        let header_bar = HeaderBar::new(&window);

        // Create the pipeline and if that fail return
        let pipeline =
            Pipeline::new().map_err(|err| format!("Error creating pipeline: {:?}", err))?;

        window.add(&pipeline.get_widget());

        let app = App(Rc::new(AppInner {
            main_window: window,
            header_bar,
            pipeline,
        }));

        // Create the application actions
        app.create_actions(application);

        Ok(app)
    }

    // Downgrade to a weak reference
    pub fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }

    pub fn on_startup(application: &gtk::Application) {
        // Create application and error out if that fails for whatever reason
        let app = match App::new(application) {
            Ok(app) => app,
            Err(err) => {
                eprintln!("Error creating application: {:?}", err);
                application.quit();
                return;
            }
        };

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

        // Once the UI is shown, start the GStreamer pipeline. If
        // an error happens, we immediately shut down
        if let Err(err) = self.pipeline.start() {
            eprintln!("Failed to set pipeline to playing: {:?}", err);
            gio::Application::get_default().map(|app| app.quit());
        }
    }

    // Called when the application shuts down. We drop our app struct here
    fn on_shutdown(self) {
        // This might fail but as we shut down right now anyway this doesn't matter
        let _ = self.pipeline.stop();
    }

    // Create our application actions here
    //
    // These are connected to our buttons and can be triggered by the buttons, as well as remotely
    fn create_actions(&self, application: &gtk::Application) {
        // about action: when activated it will show an about dialog
        let about = gio::SimpleAction::new("about", None);
        let weak_application = application.downgrade();
        about.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            show_about_dialog(&application);
        });
        application.add_action(&about);
    }
}
