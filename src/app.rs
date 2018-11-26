use gdk;
use gio::{self, prelude::*};
use glib;
use gtk::{self, prelude::*};

use about_dialog::show_about_dialog;
use header_bar::HeaderBar;
use overlay::Overlay;
use pipeline::Pipeline;
use settings::show_settings_dialog;
use utils;

use std::cell::RefCell;
use std::error;
use std::ops;
use std::rc::{Rc, Weak};

// Here we specify our custom, application specific CSS styles for various widgets
const STYLE: &str = "
#countdown-label {
    background-color: rgba(192, 192, 192, 0.8);
    color: black;
    font-size: 42pt;
    font-weight: bold;
}";

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

    header_bar: HeaderBar,
    overlay: Overlay,

    pipeline: Pipeline,

    timer: RefCell<Option<SnapshotTimer>>,
}

// Helper struct for the snapshot timer
//
// Allows counting down and removes the timeout source on Drop
struct SnapshotTimer {
    remaining: u32,
    // This needs to be Option because we need to be able to take
    // the value out in Drop::drop() removing the timeout id
    timeout_id: Option<glib::source::SourceId>,
}

impl SnapshotTimer {
    fn new(remaining: u32, timeout_id: glib::SourceId) -> Self {
        Self {
            remaining,
            timeout_id: Some(timeout_id),
        }
    }

    fn tick(&mut self) -> u32 {
        assert!(self.remaining > 0);
        self.remaining -= 1;

        self.remaining
    }
}

impl Drop for SnapshotTimer {
    fn drop(&mut self) {
        glib::source::source_remove(self.timeout_id.take().expect("No timeout id"));
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SnapshotState {
    Idle,
    TimerRunning,
}

impl<'a> From<&'a glib::Variant> for SnapshotState {
    fn from(v: &glib::Variant) -> SnapshotState {
        v.get::<bool>().expect("Invalid snapshot state type").into()
    }
}

impl From<bool> for SnapshotState {
    fn from(v: bool) -> SnapshotState {
        match v {
            false => SnapshotState::Idle,
            true => SnapshotState::TimerRunning,
        }
    }
}

impl From<SnapshotState> for glib::Variant {
    fn from(v: SnapshotState) -> glib::Variant {
        match v {
            SnapshotState::Idle => false.to_variant(),
            SnapshotState::TimerRunning => true.to_variant(),
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum RecordState {
    Idle,
    Recording,
}

impl<'a> From<&'a glib::Variant> for RecordState {
    fn from(v: &glib::Variant) -> RecordState {
        v.get::<bool>().expect("Invalid record state type").into()
    }
}

impl From<bool> for RecordState {
    fn from(v: bool) -> RecordState {
        match v {
            false => RecordState::Idle,
            true => RecordState::Recording,
        }
    }
}

impl From<RecordState> for glib::Variant {
    fn from(v: RecordState) -> glib::Variant {
        match v {
            RecordState::Idle => false.to_variant(),
            RecordState::Recording => true.to_variant(),
        }
    }
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

        // Create an overlay for showing the seconds until a snapshot
        // This is hidden while we're not doing a countdown
        let overlay = Overlay::new(&window, &pipeline.get_widget());

        let app = App(Rc::new(AppInner {
            main_window: window,
            header_bar,
            overlay,
            pipeline,
            timer: RefCell::new(None),
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
        // Load our custom CSS style-sheet and set it as the application specific style-sheet for
        // this whole application
        let provider = gtk::CssProvider::new();
        provider
            .load_from_data(STYLE.as_bytes())
            .expect("Failed to load CSS");
        gtk::StyleContext::add_provider_for_screen(
            &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        // Create application and error out if that fails for whatever reason
        let app = match App::new(application) {
            Ok(app) => app,
            Err(err) => {
                utils::show_error_dialog(
                    true,
                    format!("Error creating application: {}", err).as_str(),
                );
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
            utils::show_error_dialog(
                true,
                format!("Failed to set pipeline to playing: {}", err).as_str(),
            );
        }
    }

    // Called when the application shuts down. We drop our app struct here
    fn on_shutdown(self) {
        // This might fail but as we shut down right now anyway this doesn't matter
        // TODO: If a recording is currently running we would like to finish that first
        // before quitting the pipeline and shutting down the pipeline.
        let _ = self.pipeline.stop();
    }

    // When the snapshot button is clicked it triggers the snapshot action, which calls this
    // function here. We have to stop an existing timer here, start a new timer or immediately
    // snapshot.
    fn on_snapshot_state_changed(&self, new_state: SnapshotState) {
        let settings = utils::load_settings();

        // Stop snapshot timer, if any, and return
        if new_state == SnapshotState::Idle {
            let _ = self.timer.borrow_mut().take();
            self.overlay.set_label_visible(false);

            return;
        }

        if settings.timer_length == 0 {
            // Take a snapshot immediately if there's no timer length or start the timer

            // Set the togglebutton unchecked again immediately
            self.header_bar.set_snapshot_active(false);

            if let Err(err) = self.pipeline.take_snapshot() {
                utils::show_error_dialog(
                    false,
                    format!("Failed to take snapshot: {}", err).as_str(),
                );
            }
        } else {
            // Start a snapshot timer

            // Make the overlay visible, remember how much we have to count down and start our
            // timeout for the timer
            self.overlay.set_label_visible(true);
            self.overlay
                .set_label_text(&settings.timer_length.to_string());

            let app_weak = self.downgrade();
            // The closure is called every 1000ms
            let timeout_id = gtk::timeout_add(1000, move || {
                let app = upgrade_weak!(app_weak, glib::Continue(false));

                let remaining = app
                    .timer
                    .borrow_mut()
                    .as_mut()
                    .map(|t| t.tick())
                    .unwrap_or(0);

                if remaining == 0 {
                    // Set the togglebutton unchecked again and make the overlay text invisible
                    app.overlay.set_label_visible(false);

                    // Remove timer
                    let _ = app.timer.borrow_mut().take();

                    // This directly calls the surrounding function again and then removes the
                    // timer
                    app.header_bar.set_snapshot_active(false);

                    if let Err(err) = app.pipeline.take_snapshot() {
                        utils::show_error_dialog(
                            false,
                            format!("Failed to take snapshot: {}", err).as_str(),
                        );
                    }

                    glib::Continue(false)
                } else {
                    app.overlay.set_label_text(&remaining.to_string());
                    glib::Continue(true)
                }
            });

            *self.timer.borrow_mut() = Some(SnapshotTimer::new(settings.timer_length, timeout_id));
        }
    }

    // When the record button is clicked it triggers the record action, which will call this.
    // We have to start or stop recording here
    fn on_record_state_changed(&self, new_state: RecordState) {
        // Start/stop recording based on button active'ness
        match new_state {
            RecordState::Recording => {
                if let Err(err) = self.pipeline.start_recording() {
                    utils::show_error_dialog(
                        false,
                        format!("Failed to start recording: {}", err).as_str(),
                    );
                    self.header_bar.set_record_active(false);
                }
            }
            RecordState::Idle => self.pipeline.stop_recording(),
        }
    }

    // Create our application actions here
    //
    // These are connected to our buttons and can be triggered by the buttons, as well as remotely
    fn create_actions(&self, application: &gtk::Application) {
        // When activated, show a settings dialog
        let settings = gio::SimpleAction::new("settings", None);
        let weak_application = application.downgrade();
        settings.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);

            show_settings_dialog(&application);
        });
        application.add_action(&settings);

        // about action: when activated it will show an about dialog
        let about = gio::SimpleAction::new("about", None);
        let weak_application = application.downgrade();
        about.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            show_about_dialog(&application);
        });
        application.add_action(&about);

        // When activated, shuts down the application
        let quit = gio::SimpleAction::new("quit", None);
        let weak_application = application.downgrade();
        quit.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            application.quit();
        });
        application.add_action(&quit);

        // And add an accelerator for triggering the action on ctrl+q
        application.set_accels_for_action("app.quit", &["<Primary>Q"]);

        // snapshot action: changes state between true/false
        let snapshot =
            gio::SimpleAction::new_stateful("snapshot", None, &SnapshotState::Idle.into());
        let weak_app = self.downgrade();
        snapshot.connect_change_state(move |action, state| {
            let app = upgrade_weak!(weak_app);
            let state = state.as_ref().expect("No state provided");
            app.on_snapshot_state_changed(state.into());

            // Let the action store the new state
            action.set_state(state);
        });
        application.add_action(&snapshot);

        // record action: changes state between true/false
        let record = gio::SimpleAction::new_stateful("record", None, &RecordState::Idle.into());
        let weak_app = self.downgrade();
        record.connect_change_state(move |action, state| {
            let app = upgrade_weak!(weak_app);
            let state = state.as_ref().expect("No state provided");
            app.on_record_state_changed(state.into());

            // Let the action store the new state
            action.set_state(state);
        });
        application.add_action(&record);
    }
}
