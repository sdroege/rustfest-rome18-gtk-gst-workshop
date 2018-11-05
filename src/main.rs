extern crate gio;
extern crate glib;
extern crate gstreamer as gst;
extern crate gtk;

extern crate fragile;

#[macro_use]
extern crate serde;
extern crate serde_any;

use gio::prelude::*;
use gio::MenuExt;
use gtk::prelude::*;

use gst::prelude::*;
use gst::BinExt;

use std::cell::RefCell;
use std::env::args;
use std::error;
use std::fs::create_dir_all;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

const APPLICATION_NAME: &'static str = "com.github.rustfest";

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

macro_rules! save_settings {
    ($x:ident, $call:ident, $($to_downgrade:ident),* => move |$($p:tt),*| $body:expr) => {{
        $( let $to_downgrade = $to_downgrade.downgrade(); )*
        $x.$call(move |$($p),*| {
            $( let $to_downgrade = upgrade_weak!($to_downgrade, ()); )*
            $body
        });
    }}
}

fn get_settings_file_path() -> PathBuf {
    let mut path = glib::get_user_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(APPLICATION_NAME);
    path.push("settings.toml");
    path
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
enum OutputFormat {
    JPEG,
    PNG,
}

impl<'a> From<&'a str> for OutputFormat {
    fn from(s: &'a str) -> Self {
        match s.to_lowercase().as_str() {
            "jpeg" => OutputFormat::JPEG,
            "png" => OutputFormat::PNG,
            _ => panic!("unsupported output format"),
        }
    }
}

impl From<Option<String>> for OutputFormat {
    fn from(s: Option<String>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "jpeg" => OutputFormat::JPEG,
                "png" => OutputFormat::PNG,
                _ => panic!("unsupported output format"),
            }
        } else {
            OutputFormat::default()
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::JPEG
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct SnapshotSettings {
    /// By default, the current one.
    folder: PathBuf,
    /// Format in which to save the snapshot.
    format: OutputFormat,
    /// Timer length in seconds.
    timer_length: u32,
}

impl Default for SnapshotSettings {
    fn default() -> SnapshotSettings {
        SnapshotSettings {
            folder: glib::get_home_dir().unwrap_or_else(|| PathBuf::from(".")),
            format: OutputFormat::default(),
            timer_length: 3,
        }
    }
}

// Our refcounted application struct for containing all the
// state we have to carry around
#[derive(Clone)]
struct App(Rc<RefCell<AppInner>>);

struct AppWeak(Weak<RefCell<AppInner>>);

impl App {
    fn new(application: &gtk::Application) -> App {
        App(Rc::new(RefCell::new(AppInner {
            application: application.clone(),
            main_window: None,
            pipeline: None,
            error: None,
            timeout: None,
            remaining_secs_before_snapshot: 0,
        })))
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
    application: gtk::Application,
    main_window: Option<gtk::ApplicationWindow>,
    pipeline: Option<gst::Pipeline>,
    error: Option<Box<dyn error::Error>>,
    timeout: Option<glib::source::SourceId>,
    remaining_secs_before_snapshot: u32,
}

fn build_actions(_app: &App, application: &gtk::Application) {
    // Create app.about action for the about dialog
    //
    // This can be activated from anywhere where we have access
    // to the application, not just the main window
    let settings = gio::SimpleAction::new("settings", None);
    let about = gio::SimpleAction::new("about", None);

    // When activated, show an about dialog
    let weak_application = application.downgrade();
    settings.connect_activate(move |_action, _parameter| {
        let application = upgrade_weak!(weak_application);

        if let Some(window) = application.get_active_window() {
            build_snapshot_settings_window(&window);
        }
    });

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

    application.add_action(&settings);
    application.add_action(&about);
}

fn create_pipeline(app: &App) -> Result<(gst::Pipeline, gtk::Widget), Box<dyn error::Error>> {
    // Create a new GStreamer pipeline that captures from the default video source,
    // which is usually a camera, converts the output to RGB if needed and then passes
    // it to a GTK video sink
    let pipeline = gst::parse_launch("autovideosrc ! queue ! videoconvert ! gtksink name=sink")?;

    // Upcast to a gst::Pipeline as the above function could've also returned
    // an arbitrary gst::Element if a different string was passed
    let pipeline = pipeline.downcast::<gst::Pipeline>().unwrap();

    // Install a message handler on the pipeline's bus to catch errors
    let bus = pipeline.get_bus().unwrap();

    // GStreamer is thread-safe and it is possible to attach
    // bus watches from any thread, which are then nonetheless
    // called from the main thread. As such we have to make use
    // of fragile::Fragile() here to be able to pass our non-Send
    // application struct into a closure that requires Send.
    //
    // As we are on the main thread and the closure will be called
    // on the main thread, this will not cause a panic and is perfectly
    // safe.
    let app_weak = fragile::Fragile::new(app.downgrade());
    bus.add_watch(move |_bus, msg| {
        use gst::MessageView;

        let app_weak = app_weak.get();
        let app = upgrade_weak!(app_weak, glib::Continue(false));

        // A message can contain various kinds of information but
        // here we are only interested in errors so far
        match msg.view() {
            MessageView::Error(err) => {
                eprintln!(
                    "Error from {:?}: {} ({:?})",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );

                // On errors, we store the error that happened
                // and print it later
                let mut inner = app.0.borrow_mut();

                inner.error = Some(Box::new(err.get_error()));
                inner.application.quit();
            }
            _ => (),
        };

        glib::Continue(true)
    });

    // Get the GTK video sink and retrieve the video display widget from it
    let sink = pipeline.get_by_name("sink").unwrap();
    let widget_value = sink.get_property("widget").unwrap();
    let widget = widget_value.get::<gtk::Widget>().unwrap();

    Ok((pipeline, widget))
}

fn save_settings(
    folder_button: &gtk::FileChooserButton,
    options: &gtk::ComboBoxText,
    timer_entry: &gtk::SpinButton,
) {
    let settings = SnapshotSettings {
        folder: PathBuf::from(
            folder_button
                .get_filename()
                .unwrap_or_else(|| glib::get_home_dir().unwrap_or_else(|| PathBuf::from("."))),
        ),
        format: OutputFormat::from(options.get_active_text()),
        timer_length: timer_entry.get_value_as_int() as _,
    };
    let s = get_settings_file_path();
    if let Err(e) = serde_any::to_file(&s, &settings) {
        eprintln!("Error when trying to save file: {:?}", e);
    } else {
        println!("Saved settings {:?} in '{}'", settings, s.display());
    }
}

fn load_settings() -> SnapshotSettings {
    let s = get_settings_file_path();
    if s.exists() && s.is_file() {
        match serde_any::from_file::<SnapshotSettings, _>(&s) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error when opening '{}': {:?}", s.display(), e);
                SnapshotSettings::default()
            }
        }
    } else {
        SnapshotSettings::default()
    }
}

fn build_snapshot_settings_window(parent: &gtk::Window) {
    let s = get_settings_file_path();

    if !s.exists() {
        if let Some(parent) = s.parent() {
            if !parent.exists() {
                if let Err(e) = create_dir_all(parent) {
                    eprintln!(
                        "Error when trying to build settings folder '{}': {:?}",
                        parent.display(),
                        e
                    );
                }
            }
        }
    }

    let settings = load_settings();

    //
    // BUILDING UI
    //
    let dialog = gtk::Dialog::new_with_buttons(
        Some("Snapshot settings"),
        Some(parent),
        gtk::DialogFlags::MODAL,
        &[("Close", gtk::ResponseType::Close.into())],
    );

    let grid = gtk::Grid::new();
    grid.set_column_spacing(4);
    grid.set_row_spacing(4);
    grid.set_margin_bottom(10);

    //
    // OUTPUT FOLDER
    //
    let folder_label = gtk::Label::new("Output folder");
    let folder_chooser_but = gtk::FileChooserButton::new(
        "Pick a directory to save snapshots",
        gtk::FileChooserAction::SelectFolder,
    );

    folder_label.set_halign(gtk::Align::Start);
    folder_chooser_but.set_filename(settings.folder);

    grid.attach(&folder_label, 0, 0, 1, 1);
    grid.attach(&folder_chooser_but, 1, 0, 3, 1);

    //
    // OUTPUT FORMAT OPTIONS
    //
    let format_label = gtk::Label::new("Output format");
    let options = gtk::ComboBoxText::new();

    format_label.set_halign(gtk::Align::Start);

    options.append_text("JPEG");
    options.append_text("PNG");
    options.set_active(match settings.format {
        OutputFormat::JPEG => 0,
        OutputFormat::PNG => 1,
    });
    options.set_hexpand(true);

    grid.attach(&format_label, 0, 1, 1, 1);
    grid.attach(&options, 1, 1, 3, 1);

    //
    // TIMER LENGTH
    //
    let timer_label = gtk::Label::new("Timer length (in seconds)");
    let timer_entry = gtk::SpinButton::new_with_range(0., 15., 1.);

    timer_label.set_halign(gtk::Align::Start);
    timer_label.set_hexpand(true);

    timer_entry.set_value(settings.timer_length as f64);

    grid.attach(&timer_label, 0, 2, 1, 1);
    grid.attach(&timer_entry, 1, 2, 3, 1);

    //
    // PUTTING WIDGETS INTO DIALOG
    //
    let content_area = dialog.get_content_area();
    content_area.pack_start(&grid, true, true, 0);
    content_area.set_border_width(10);

    //
    // ADDING SETTINGS "AUTOMATIC" SAVE
    //
    save_settings!(timer_entry, connect_value_changed, folder_chooser_but, options =>
                   move |timer_entry| {
        save_settings(&folder_chooser_but, &options, &timer_entry);
    });
    save_settings!(options, connect_changed, folder_chooser_but, timer_entry =>
                   move |options| {
        save_settings(&folder_chooser_but, &options, &timer_entry);
    });
    save_settings!(folder_chooser_but, connect_file_set, timer_entry, options =>
                   move |folder_chooser_but| {
        save_settings(&folder_chooser_but, &options, &timer_entry);
    });

    dialog.connect_response(|dialog, _| {
        dialog.destroy();
    });

    dialog.set_resizable(false);
    dialog.show_all();
}

fn take_snapshot() {
    println!("tadam!");
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
    main_menu_model.append("Settings", "app.settings");
    main_menu_model.append("About", "app.about");
    main_menu.set_menu_model(&main_menu_model);

    // Create an overlay for showing the seconds until a snapshot
    // This is hidden while we're not doing a countdown
    let overlay = gtk::Overlay::new();
    let overlay_text = gtk::Label::new("0");

    overlay.add_overlay(&overlay_text);
    overlay_text.set_halign(gtk::Align::End);
    overlay_text.set_valign(gtk::Align::Start);
    if let Some(style) = overlay_text.get_style_context() {
        let css_provider = gtk::CssProvider::new();
        css_provider
            .load_from_data(
                "GtkLabel {
            background-color: #fff;
            border: 1px solid black;
            border-radius: 2px;
            color: black;
        }"
                .as_bytes(),
            )
            .expect("load_from_data failed");
        style.add_provider(&css_provider, gtk::STYLE_PROVIDER_PRIORITY_USER);
    }
    overlay_text.set_no_show_all(true);
    overlay_text.set_visible(false);

    let snapshot_button = gtk::Button::new();
    let snapshot_button_image = gtk::Image::new_from_icon_name("camera-photo", 1);
    snapshot_button.add(&snapshot_button_image);

    let app_weak = Rc::downgrade(&app.0);
    snapshot_button.connect_clicked(move |_| {
        let app = upgrade_weak!(app_weak);
        let settings = load_settings();

        let mut inner = app.borrow_mut();

        // If we're currently doing a countdown, cancel it
        if let Some(t) = inner.timeout.take() {
            glib::source::source_remove(t);
            overlay_text.set_visible(false);
            return;
        }

        // Otherwise take a snapshot immediately if there's
        // no timer length or start the timer
        if settings.timer_length == 0 {
            take_snapshot();
        } else {
            overlay_text.set_visible(true);
            overlay_text.set_text(&settings.timer_length.to_string());

            inner.remaining_secs_before_snapshot = settings.timer_length;

            let overlay_text_weak = overlay_text.downgrade();
            let app_weak = Rc::downgrade(&app);
            let source = gtk::timeout_add_seconds(1, move || {
                let app = upgrade_weak!(app_weak, glib::Continue(false));
                let overlay_text = upgrade_weak!(overlay_text_weak, glib::Continue(false));

                let mut inner = app.borrow_mut();

                if inner.remaining_secs_before_snapshot < 2 {
                    overlay_text.set_visible(false);
                    take_snapshot();
                    inner.timeout = None;
                    glib::Continue(false)
                } else {
                    overlay_text.set_text(&(inner.remaining_secs_before_snapshot - 1).to_string());
                    inner.remaining_secs_before_snapshot -= 1;
                    glib::Continue(true)
                }
            });
            inner.timeout = Some(source);
        }
    });

    header_bar.pack_end(&main_menu);
    header_bar.pack_end(&snapshot_button);
    window.set_titlebar(&header_bar);

    // Create the pipeline and if that fails, shut down and
    // remember the error that happened
    let (pipeline, view) = match create_pipeline(app) {
        Err(err) => {
            app.0.borrow_mut().error = Some(err);
            application.quit();
            return;
        }
        Ok(res) => res,
    };

    // Store the pipeline for later usage and add the view widget
    // to the UI
    app.0.borrow_mut().pipeline = Some(pipeline);

    // A Box allows to place multiple widgets next to each other
    // vertically or horizontally
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&view, true, true, 0);

    overlay.add(&vbox);
    window.add(&overlay);
}

fn main() -> Result<(), Box<dyn error::Error>> {
    gst::init()?;
    let application = gtk::Application::new(APPLICATION_NAME, gio::ApplicationFlags::empty())?;

    let app = App::new(&application);

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
    application.connect_activate(move |application| {
        let app = upgrade_weak!(app_weak);
        let mut inner = app.0.borrow_mut();
        // We only show our window here once the application
        // is activated. This means that when a second instance
        // is started, the window of the first instance will be
        // brought to the foreground
        if let Some(ref main_window) = inner.main_window {
            main_window.show_all();
            main_window.present();
        }

        // Once the UI is shown, start the GStreamer pipeline. If
        // an error happens, we immediately shut down
        {
            // Needed because we need to borrow two parts of the application struct
            let AppInner {
                ref pipeline,
                ref mut error,
                ..
            } = *inner;

            if let Some(ref pipeline) = pipeline {
                if let Err(err) = pipeline.set_state(gst::State::Playing).into_result() {
                    *error = Some(Box::new(err));
                    application.quit();
                }
            }
        }
    });

    // When the application is shut down, first shut down
    // the GStreamer pipeline so that capturing can gracefully stop
    let app_weak = app.downgrade();
    application.connect_shutdown(move |_| {
        let app = upgrade_weak!(app_weak);
        let inner = app.0.borrow();

        if let Some(ref pipeline) = inner.pipeline {
            let _ = pipeline.set_state(gst::State::Null);
        }
    });

    // And now run the application until the end
    application.run(&args().collect::<Vec<_>>());

    let mut app_inner = app.0.borrow_mut();
    if let Some(err) = app_inner.error.take() {
        Err(err)
    } else {
        Ok(())
    }
}
