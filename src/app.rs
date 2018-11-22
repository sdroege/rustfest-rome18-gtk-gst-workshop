use gio::{self, prelude::*};
use gtk::{self, prelude::*};

use about_dialog::create_about_dialog;
use headerbar::HeaderBar;
use overlay::Overlay;
use pipeline::Pipeline;
use settings::create_settings_dialog;
use utils;

use std::cell::RefCell;
use std::error;
use std::rc::{Rc, Weak};

// Our refcounted application struct for containing all the
// state we have to carry around
#[derive(Clone)]
pub struct App(Rc<AppInner>);

pub struct AppWeak(Weak<AppInner>);

impl AppWeak {
    pub fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

pub struct AppInner {
    main_window: gtk::ApplicationWindow,
    pipeline: Pipeline,

    header_bar: HeaderBar,
    overlay: Overlay,

    // Snapshot timer state
    timer: RefCell<Option<SnapshotTimer>>,
}

struct SnapshotTimer {
    remaining: u32,
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

// Here we specify our custom, application specific CSS styles for various widgets
const STYLE: &str = "
#countdown-label {
    background-color: rgba(192, 192, 192, 0.8);
    color: black;
    font-size: 42pt;
    font-weight: bold;
}";

impl App {
    pub fn new(application: &gtk::Application) -> Result<App, Box<dyn error::Error>> {
        // Build the UI but don't show it yet

        let window = gtk::ApplicationWindow::new(application);

        window.set_title("RustFest 2018 GTK+ & GStreamer WebCam Viewer");
        window.set_border_width(5);
        window.set_position(gtk::WindowPosition::Center);
        window.set_default_size(848, 480);

        // Create headerbar for the application, including the main
        // menu and a close button
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

        // Create our UI actions
        app.connect_actions(application);

        Ok(app)
    }

    pub fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }

    pub fn on_startup(application: &gtk::Application) {
        // Load our custom CSS style-sheet and set it as the application
        // specific style-sheet for this whole application
        let provider = gtk::CssProvider::new();
        provider
            .load_from_data(STYLE.as_bytes())
            .expect("Failed to load CSS");
        gtk::StyleContext::add_provider_for_screen(
            &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let app = match App::new(application) {
            Ok(app) => app,
            Err(err) => {
                utils::show_error_dialog(
                    true,
                    format!("Error creating application: {:?}", err).as_str(),
                );
                return;
            }
        };

        // When the application is activated show the UI. This happens
        // when the first process is started, and in the first process
        // whenever a second process is started
        let app_weak = app.downgrade();
        application.connect_activate(move |_| {
            let app = upgrade_weak!(app_weak);
            app.on_activate();
        });

        // When the application is shut down, first shut down
        // the GStreamer pipeline so that capturing can gracefully stop
        let app_container = RefCell::new(Some(app));
        application.connect_shutdown(move |_| {
            let app = app_container
                .borrow_mut()
                .take()
                .expect("Shutdown called multiple times");
            app.on_shutdown();
        });
    }

    pub fn on_activate(&self) {
        // We only show our window here once the application
        // is activated. This means that when a second instance
        // is started, the window of the first instance will be
        // brought to the foreground
        self.0.main_window.show_all();

        // Have to call this instead of present() because of
        // https://gitlab.gnome.org/GNOME/gtk/issues/624
        self.0
            .main_window
            .present_with_time((glib::get_monotonic_time() / 1000) as u32);

        // Once the UI is shown, start the GStreamer pipeline. If
        // an error happens, we immediately shut down
        if let Err(err) = self.0.pipeline.start() {
            utils::show_error_dialog(
                true,
                format!("Failed to set pipeline to playing: {:?}", err).as_str(),
            );
        }
    }

    pub fn on_shutdown(self) {
        // This might fail but as we shut down right now anyway this
        // doesn't matter
        let _ = self.0.pipeline.stop();
    }

    fn connect_actions(&self, application: &gtk::Application) {
        // Create actions for our settings and about dialogs
        //
        // This can be activated from anywhere where we have access
        // to the application, not just the main window
        let settings = gio::SimpleAction::new("settings", None);

        // When activated, show a settings dialog
        let weak_application = application.downgrade();
        settings.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);

            create_settings_dialog(&application);
        });

        let about = gio::SimpleAction::new("about", None);

        // When activated, show an about dialog
        let weak_application = application.downgrade();
        about.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);
            create_about_dialog(&application);
        });

        let snapshot = gio::SimpleAction::new_stateful("snapshot", None, &false.to_variant());
        let weak_app = self.downgrade();
        snapshot.connect_change_state(move |action, state| {
            let app = upgrade_weak!(weak_app);
            let state = state.as_ref().expect("No state provided");
            app.on_snapshot_state_changed(
                state.get::<bool>().expect("Invalid snapshot state type"),
            );
            action.set_state(state);
        });

        let record = gio::SimpleAction::new_stateful("record", None, &false.to_variant());
        let weak_app = self.downgrade();
        record.connect_change_state(move |action, state| {
            let app = upgrade_weak!(weak_app);
            let state = state.as_ref().expect("No state provided");
            app.on_record_state_changed(state.get::<bool>().expect("Invalid record state type"));
            action.set_state(state);
        });

        application.add_action(&settings);
        application.add_action(&about);
        application.add_action(&snapshot);
        application.add_action(&record);
    }

    // When the snapshot button is clicked, we have to start the timer, stop the timer or directly
    // snapshot
    fn on_snapshot_state_changed(&self, snapshot: bool) {
        let settings = utils::load_settings();

        // Stop snapshot timer, if any, and return
        if !snapshot {
            let _ = self.0.timer.borrow_mut().take();
            self.0.overlay.set_label_visible(false);

            return;
        }

        if settings.timer_length == 0 {
            // Take a snapshot immediately if there's
            // no timer length or start the timer

            // Set the togglebutton unchecked again immediately
            self.0.header_bar.set_snapshot_active(false);

            if let Err(err) = self.0.pipeline.take_snapshot() {
                utils::show_error_dialog(
                    false,
                    format!("Failed to take snapshot: {}", err).as_str(),
                );
            }
        } else {
            // Start a snapshot timer

            // Make the overlay visible, remember how much we have to count
            // down and start our timeout for the timer
            self.0.overlay.set_label_visible(true);
            self.0
                .overlay
                .set_label_text(&settings.timer_length.to_string());

            let app_weak = self.downgrade();
            // The closure is called every 1000ms
            let timeout_id = gtk::timeout_add(1000, move || {
                let app = upgrade_weak!(app_weak, glib::Continue(false));

                let remaining = app
                    .0
                    .timer
                    .borrow_mut()
                    .as_mut()
                    .map(|t| t.tick())
                    .unwrap_or(0);

                if remaining == 0 {
                    // Set the togglebutton unchecked again and make
                    // the overlay text invisible
                    app.0.overlay.set_label_visible(false);

                    // Remove timer
                    let _ = app.0.timer.borrow_mut().take();

                    // This directly calls the surrounding function again
                    // and then removes the timer
                    app.0.header_bar.set_snapshot_active(false);

                    if let Err(err) = app.0.pipeline.take_snapshot() {
                        utils::show_error_dialog(
                            false,
                            format!("Failed to take snapshot: {}", err).as_str(),
                        );
                    }

                    glib::Continue(false)
                } else {
                    app.0.overlay.set_label_text(&remaining.to_string());
                    glib::Continue(true)
                }
            });

            *self.0.timer.borrow_mut() =
                Some(SnapshotTimer::new(settings.timer_length, timeout_id));
        }
    }

    // When the record button is clicked, we have to start or stop recording
    fn on_record_state_changed(&self, record: bool) {
        // Start/stop recording based on button active'ness
        if record {
            if let Err(err) = self.0.pipeline.start_recording() {
                utils::show_error_dialog(
                    false,
                    format!("Failed to start recording: {}", err).as_str(),
                );
                self.0.header_bar.set_record_active(false);
            }
        } else {
            self.0.pipeline.stop_recording();
        }
    }
}
