extern crate gdk;
extern crate gio;
extern crate glib;
extern crate gtk;

extern crate gstreamer as gst;
extern crate gstreamer_video as gst_video;

extern crate fragile;

#[macro_use]
extern crate serde;
extern crate serde_any;

extern crate chrono;
use chrono::prelude::*;

use gio::prelude::*;
use gio::MenuExt;
use gtk::prelude::*;

use gst::prelude::*;
use gst::BinExt;

use std::cell::RefCell;
use std::env::args;
use std::error;
use std::fs::{create_dir_all, File};
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
enum SnapshotFormat {
    JPEG,
    PNG,
}

impl<'a> From<&'a str> for SnapshotFormat {
    fn from(s: &'a str) -> Self {
        match s.to_lowercase().as_str() {
            "jpeg" => SnapshotFormat::JPEG,
            "png" => SnapshotFormat::PNG,
            _ => panic!("unsupported output format"),
        }
    }
}

impl From<Option<String>> for SnapshotFormat {
    fn from(s: Option<String>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "jpeg" => SnapshotFormat::JPEG,
                "png" => SnapshotFormat::PNG,
                _ => panic!("unsupported output format"),
            }
        } else {
            SnapshotFormat::default()
        }
    }
}

impl Default for SnapshotFormat {
    fn default() -> Self {
        SnapshotFormat::JPEG
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
enum RecordFormat {
    H264Mp4,
    Vp8WebM,
}

impl<'a> From<&'a str> for RecordFormat {
    fn from(s: &'a str) -> Self {
        match s.to_lowercase().as_str() {
            "h264/mp4" => RecordFormat::H264Mp4,
            "vp8/webm" => RecordFormat::Vp8WebM,
            _ => panic!("unsupported output format"),
        }
    }
}

impl From<Option<String>> for RecordFormat {
    fn from(s: Option<String>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "h264/mp4" => RecordFormat::H264Mp4,
                "vp8/webm" => RecordFormat::Vp8WebM,
                _ => panic!("unsupported output format"),
            }
        } else {
            RecordFormat::default()
        }
    }
}

impl Default for RecordFormat {
    fn default() -> Self {
        RecordFormat::H264Mp4
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct SnapshotSettings {
    // By default, the user's picture directory.
    snapshot_directory: PathBuf,
    // Format in which to save the snapshot.
    snapshot_format: SnapshotFormat,
    // Timer length in seconds.
    timer_length: u32,

    // By default, the user's video directory.
    record_directory: PathBuf,
    // Format to use for recording videos.
    record_format: RecordFormat,
}

impl Default for SnapshotSettings {
    fn default() -> SnapshotSettings {
        SnapshotSettings {
            snapshot_directory: glib::get_user_special_dir(glib::UserDirectory::Pictures)
                .unwrap_or_else(|| PathBuf::from(".")),
            snapshot_format: SnapshotFormat::default(),
            timer_length: 3,
            record_directory: glib::get_user_special_dir(glib::UserDirectory::Videos)
                .unwrap_or_else(|| PathBuf::from(".")),
            record_format: RecordFormat::default(),
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

    // Any error that happened during runtime and should be
    // reported before the application quits
    error: Option<Box<dyn error::Error>>,

    // Snapshot timer state
    timeout: Option<glib::source::SourceId>,
    remaining_secs_before_snapshot: u32,
}

// Save the current settings from the values of the various UI elements
fn save_settings(
    snapshot_directory_button: &gtk::FileChooserButton,
    snapshot_format: &gtk::ComboBoxText,
    timer_entry: &gtk::SpinButton,
    record_directory_button: &gtk::FileChooserButton,
    record_format: &gtk::ComboBoxText,
) {
    let settings = SnapshotSettings {
        snapshot_directory: PathBuf::from(
            snapshot_directory_button
                .get_filename()
                .unwrap_or_else(|| glib::get_home_dir().unwrap_or_else(|| PathBuf::from("."))),
        ),
        snapshot_format: SnapshotFormat::from(snapshot_format.get_active_text()),
        timer_length: timer_entry.get_value_as_int() as _,
        record_directory: PathBuf::from(
            record_directory_button
                .get_filename()
                .unwrap_or_else(|| glib::get_home_dir().unwrap_or_else(|| PathBuf::from("."))),
        ),
        record_format: RecordFormat::from(record_format.get_active_text()),
    };
    let s = get_settings_file_path();
    if let Err(e) = serde_any::to_file(&s, &settings) {
        eprintln!("Error when trying to save file: {:?}", e);
    } else {
        println!("Saved settings {:?} in '{}'", settings, s.display());
    }
}

// Load the current settings
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

// Construct the settings dialog and ensure that the settings file exists and is loaded
fn build_settings_window(parent: &gtk::Window) {
    let s = get_settings_file_path();

    if !s.exists() {
        if let Some(parent) = s.parent() {
            if !parent.exists() {
                if let Err(e) = create_dir_all(parent) {
                    eprintln!(
                        "Error when trying to build settings snapshot_directory '{}': {:?}",
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
    // SNAPSHOT FOLDER
    //
    let snapshot_directory_label = gtk::Label::new("Snapshot directory");
    let snapshot_directory_chooser_but = gtk::FileChooserButton::new(
        "Pick a directory to save snapshots",
        gtk::FileChooserAction::SelectFolder,
    );

    snapshot_directory_label.set_halign(gtk::Align::Start);
    snapshot_directory_chooser_but.set_filename(settings.snapshot_directory);

    grid.attach(&snapshot_directory_label, 0, 0, 1, 1);
    grid.attach(&snapshot_directory_chooser_but, 1, 0, 3, 1);

    //
    // SNAPSHOT FORMAT OPTIONS
    //
    let format_label = gtk::Label::new("Snapshot format");
    let snapshot_format = gtk::ComboBoxText::new();

    format_label.set_halign(gtk::Align::Start);

    snapshot_format.append_text("JPEG");
    snapshot_format.append_text("PNG");
    snapshot_format.set_active(match settings.snapshot_format {
        SnapshotFormat::JPEG => 0,
        SnapshotFormat::PNG => 1,
    });
    snapshot_format.set_hexpand(true);

    grid.attach(&format_label, 0, 1, 1, 1);
    grid.attach(&snapshot_format, 1, 1, 3, 1);

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
    // RECORD FOLDER
    //
    let record_directory_label = gtk::Label::new("Record directory");
    let record_directory_chooser_but = gtk::FileChooserButton::new(
        "Pick a directory to save records",
        gtk::FileChooserAction::SelectFolder,
    );

    record_directory_label.set_halign(gtk::Align::Start);
    record_directory_chooser_but.set_filename(settings.record_directory);

    grid.attach(&record_directory_label, 0, 3, 1, 1);
    grid.attach(&record_directory_chooser_but, 1, 3, 3, 1);

    //
    // RECORD FORMAT OPTIONS
    //
    let format_label = gtk::Label::new("Record format");
    let record_format = gtk::ComboBoxText::new();

    format_label.set_halign(gtk::Align::Start);

    record_format.append_text("H264/MP4");
    record_format.append_text("VP8/WebM");
    record_format.set_active(match settings.record_format {
        RecordFormat::H264Mp4 => 0,
        RecordFormat::Vp8WebM => 1,
    });
    record_format.set_hexpand(true);

    grid.attach(&format_label, 0, 4, 1, 1);
    grid.attach(&record_format, 1, 4, 3, 1);

    //
    // PUTTING WIDGETS INTO DIALOG
    //
    let content_area = dialog.get_content_area();
    content_area.pack_start(&grid, true, true, 0);
    content_area.set_border_width(10);

    //
    // ADDING SETTINGS "AUTOMATIC" SAVE
    //
    save_settings!(timer_entry, connect_value_changed,
                   snapshot_directory_chooser_but, snapshot_format, record_directory_chooser_but, record_format =>
                   move |timer_entry| {
        save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry, &record_directory_chooser_but, &record_format);
    });

    save_settings!(snapshot_format, connect_changed,
                   snapshot_directory_chooser_but, timer_entry, record_directory_chooser_but, record_format =>
                   move |snapshot_format| {
        save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry, &record_directory_chooser_but, &record_format);
    });

    save_settings!(snapshot_directory_chooser_but, connect_file_set, timer_entry, snapshot_format,
                   record_directory_chooser_but, record_format =>
                   move |snapshot_directory_chooser_but| {
        save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry, &record_directory_chooser_but, &record_format);
    });

    save_settings!(record_format, connect_changed,
                   snapshot_directory_chooser_but, timer_entry, record_directory_chooser_but, snapshot_format =>
                   move |record_format| {
        save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry, &record_directory_chooser_but, &record_format);
    });

    save_settings!(record_directory_chooser_but, connect_file_set,
                   timer_entry, snapshot_format, snapshot_directory_chooser_but, record_format =>
                   move |record_directory_chooser_but| {
        save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry, &record_directory_chooser_but, &record_format);
    });

    dialog.connect_response(|dialog, _| {
        dialog.destroy();
    });

    dialog.set_resizable(false);
    dialog.show_all();
}

impl App {
    fn build_actions(&self, application: &gtk::Application) {
        // Create actions for our settings and about dialogs
        //
        // This can be activated from anywhere where we have access
        // to the application, not just the main window
        let settings = gio::SimpleAction::new("settings", None);

        // When activated, show a settings dialog
        let weak_application = application.downgrade();
        settings.connect_activate(move |_action, _parameter| {
            let application = upgrade_weak!(weak_application);

            if let Some(window) = application.get_active_window() {
                build_settings_window(&window);
            }
        });

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

        application.add_action(&settings);
        application.add_action(&about);
    }

    // Here we handle all message we get from the GStreamer pipeline. These are
    // notifications sent from GStreamer, including errors that happend at
    // runtime.
    fn on_pipeline_message(&self, msg: &gst::MessageRef) {
        use gst::MessageView;

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
                let mut inner = self.0.borrow_mut();

                inner.error = Some(Box::new(err.get_error()));
                inner.application.quit();
            }
            MessageView::Element(msg) => {
                // Catch the end-of-stream messages from our filesink. Because the other sink,
                // gtksink, will never receive end-of-stream we will never get a normal
                // end-of-stream message from the bus.
                //
                // The normal end-of-stream message would only be sent once *all*
                // sinks had their end-of-stream message posted.
                match msg.get_structure() {
                    Some(s) if s.get_name() == "GstBinForwarded" => {
                        // The forwarded, original message from the bin is stored in the
                        // message field of its structure
                        let msg = s
                            .get::<gst::Message>("message")
                            .expect("Failed to get forwarded message");

                        if let MessageView::Eos(..) = msg.view() {
                            let inner = self.0.borrow();

                            // Get our pipeline and the recording bin
                            let pipeline = match inner.pipeline {
                                Some(ref pipeline) => pipeline.clone(),
                                None => return,
                            };
                            let bin = match msg
                                .get_src()
                                .and_then(|src| src.clone().downcast::<gst::Element>().ok())
                            {
                                Some(src) => src,
                                None => return,
                            };

                            // And then asynchronously remove it and set its state to Null
                            pipeline.call_async(move |pipeline| {
                                // Ignore if the bin was not in the pipeline anymore for whatever
                                // reason. It's not a problem
                                let _ = pipeline.remove(&bin);

                                // TODO error dialog
                                if let Err(err) = bin.set_state(gst::State::Null).into_result() {
                                    eprintln!("Failed to stop recording: {}", err);
                                }
                            });
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        };
    }

    fn create_pipeline(&self) -> Result<(gst::Pipeline, gtk::Widget), Box<dyn error::Error>> {
        // Create a new GStreamer pipeline that captures from the default video source,
        // which is usually a camera, converts the output to RGB if needed and then passes
        // it to a GTK video sink
        let pipeline = gst::parse_launch(
            "autovideosrc ! tee name=tee ! queue ! videoconvert ! gtksink name=sink",
        )?;

        // Upcast to a gst::Pipeline as the above function could've also returned
        // an arbitrary gst::Element if a different string was passed
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Couldn't downcast pipeline");

        // Request that the pipeline forwards us all messages, even those that it would otherwise
        // aggregate first
        pipeline.set_property_message_forward(true);

        // Install a message handler on the pipeline's bus to catch errors
        let bus = pipeline.get_bus().expect("Pipeline had no bus");

        // GStreamer is thread-safe and it is possible to attach
        // bus watches from any thread, which are then nonetheless
        // called from the main thread. As such we have to make use
        // of fragile::Fragile() here to be able to pass our non-Send
        // application struct into a closure that requires Send.
        //
        // As we are on the main thread and the closure will be called
        // on the main thread, this will not cause a panic and is perfectly
        // safe.
        let app_weak = fragile::Fragile::new(self.downgrade());
        bus.add_watch(move |_bus, msg| {
            let app_weak = app_weak.get();
            let app = upgrade_weak!(app_weak, glib::Continue(false));

            app.on_pipeline_message(msg);

            glib::Continue(true)
        });

        // Get the GTK video sink and retrieve the video display widget from it
        let sink = pipeline
            .get_by_name("sink")
            .expect("Pipeline had no sink element");
        let widget_value = sink
            .get_property("widget")
            .expect("Sink had no widget property");
        let widget = widget_value
            .get::<gtk::Widget>()
            .expect("Sink's widget propery was of the wrong type");

        Ok((pipeline, widget))
    }

    // Take a snapshot of the current image and write it to the configured location
    fn take_snapshot(&self) {
        let settings = load_settings();

        // If we have no pipeline there's nothing to snapshot
        let pipeline = match self.0.borrow().pipeline {
            None => return,
            Some(ref pipeline) => pipeline.clone(),
        };

        // Create the GStreamer caps for the output format
        let (caps, extension) = match settings.snapshot_format {
            SnapshotFormat::JPEG => (gst::Caps::new_simple("image/jpeg", &[]), "jpg"),
            SnapshotFormat::PNG => (gst::Caps::new_simple("image/png", &[]), "png"),
        };

        let sink = pipeline.get_by_name("sink").expect("sink not found");
        let last_sample = sink
            .get_property("last-sample")
            .expect("Sink had no last-sample property");
        let last_sample = match last_sample.get::<gst::Sample>() {
            None => {
                // We have no sample to store yet
                return;
            }
            Some(sample) => sample,
        };

        // Create the filename and open the file writable
        let mut filename = settings.snapshot_directory.clone();
        let now = Local::now();
        filename.push(format!(
            "{}.{}",
            now.format("Snapshot %Y-%m-%d %H:%M:%S"),
            extension
        ));

        // TODO error dialogs
        let mut file = match File::create(&filename) {
            Err(err) => {
                eprintln!(
                    "Failed to create snapshot file {}: {}",
                    filename.display(),
                    err
                );
                return;
            }
            Ok(file) => file,
        };

        // Then convert it from whatever format we got to PNG or JPEG as requested
        // and write it out
        println!("Writing snapshot to {}", filename.display());
        gst_video::convert_sample_async(&last_sample, &caps, 5 * gst::SECOND, move |res| {
            use std::io::Write;

            let sample = match res {
                Err(err) => {
                    // TODO error dialogs
                    eprintln!("Failed to convert sample: {}", err);
                    return;
                }
                Ok(sample) => sample,
            };

            let buffer = sample.get_buffer().expect("Failed to get buffer");
            let map = buffer
                .map_readable()
                .expect("Failed to map buffer readable");

            if let Err(err) = file.write_all(&map) {
                // TODO error dialogs
                eprintln!(
                    "Failed to write snapshot file {}: {}",
                    filename.display(),
                    err
                );
            }
        });
    }

    // When the snapshot button is clicked, we have to start the timer, stop the timer or directly
    // snapshot
    fn on_snapshot_button_clicked(
        &self,
        snapshot_button: &gtk::ToggleButton,
        overlay_text: &gtk::Label,
    ) {
        let settings = load_settings();
        let mut inner = self.0.borrow_mut();

        // If we're currently doing a countdown, cancel it
        if let Some(t) = inner.timeout.take() {
            glib::source::source_remove(t);
            overlay_text.set_visible(false);
            return;
        } else if settings.timer_length == 0 {
            // Otherwise take a snapshot immediately if there's
            // no timer length or start the timer
            //
            // Set the togglebutton unchecked again
            snapshot_button.set_state_flags(
                snapshot_button.get_state_flags() & !gtk::StateFlags::CHECKED,
                true,
            );

            // Make sure to drop the borrow before calling any other
            // app methods
            drop(inner);

            self.take_snapshot();
        } else {
            // Make the overlay visible, remember how much we have to count
            // down and start our timeout for the timer
            overlay_text.set_visible(true);
            overlay_text.set_text(&settings.timer_length.to_string());

            inner.remaining_secs_before_snapshot = settings.timer_length;

            let overlay_text_weak = overlay_text.downgrade();
            let snapshot_button_weak = snapshot_button.downgrade();
            let app_weak = self.downgrade();
            // The closure is called every 1000ms
            let source = gtk::timeout_add(1000, move || {
                let app = upgrade_weak!(app_weak, glib::Continue(false));
                let snapshot_button = upgrade_weak!(snapshot_button_weak, glib::Continue(false));
                let overlay_text = upgrade_weak!(overlay_text_weak, glib::Continue(false));

                let mut inner = app.0.borrow_mut();

                inner.remaining_secs_before_snapshot -= 1;
                if inner.remaining_secs_before_snapshot == 0 {
                    // Set the togglebutton unchecked again and make
                    // the overlay text invisible
                    overlay_text.set_visible(false);
                    snapshot_button.set_state_flags(
                        snapshot_button.get_state_flags() & !gtk::StateFlags::CHECKED,
                        true,
                    );
                    inner.timeout = None;
                } else {
                    overlay_text.set_text(&inner.remaining_secs_before_snapshot.to_string());
                }

                if inner.remaining_secs_before_snapshot == 0 {
                    // Make sure to drop the borrow before calling any other
                    // app methods
                    drop(inner);

                    app.take_snapshot();
                    glib::Continue(false)
                } else {
                    glib::Continue(true)
                }
            });

            inner.timeout = Some(source);
        }
    }

    // When the record button is clicked, we have to start or stop recording
    fn on_record_button_clicked(&self, record_button: &gtk::ToggleButton) {
        let settings = load_settings();

        // If we have no pipeline (can't really happen) just return
        let pipeline = match self.0.borrow().pipeline {
            Some(ref pipeline) => pipeline.clone(),
            None => return,
        };

        // Start/stop recording based on button active'ness
        if record_button.get_active() {
            // If we already have a record-bin (i.e. we still finish the previous one)
            // just return for now and deactivate the button again
            if pipeline.get_by_name("record-bin").is_some() {
                record_button.set_state_flags(
                    record_button.get_state_flags() & !gtk::StateFlags::CHECKED,
                    true,
                );
                return;
            }

            let (bin_description, extension) = match settings.record_format {
                RecordFormat::H264Mp4 => ("name=record-bin queue ! videoconvert ! x264enc ! video/x-h264,profile=baseline ! mp4mux ! filesink name=sink", "mp4"),
                RecordFormat::Vp8WebM => ("name=record-bin queue ! videoconvert ! vp8enc ! webmmux ! filesink name=sink", "webm"),
            };

            let bin = match gst::parse_bin_from_description(bin_description, true) {
                Err(err) => {
                    // TODO error dialogs
                    eprintln!("Failed to create recording pipeline: {}", err);
                    return;
                }
                Ok(bin) => bin,
            };

            // Get our file sink element by its name and set the location where to write the recording
            let sink = bin
                .get_by_name("sink")
                .expect("Recording bin has no sink element");
            let mut filename = settings.record_directory.clone();
            let now = Local::now();
            filename.push(format!(
                "{}.{}",
                now.format("Recording %Y-%m-%d %H:%M:%S"),
                extension
            ));
            // All strings in GStreamer are UTF8, we need to convert the path to UTF8
            // which in theory can fail
            sink.set_property("location", &(filename.to_str().unwrap()))
                .expect("Filesink had no location property");

            // First try setting the recording bin to playing: if this fails
            // we know this before it potentially interferred with the other
            // part of the pipeline
            if let Err(_) = bin.set_state(gst::State::Playing).into_result() {
                // TODO error dialogs
                eprintln!("Failed to start recording bin");
                return;
            }

            // Add the bin to the pipeline. This would only fail if there was already
            // a bin with the same name, which we ensured can't happen
            pipeline.add(&bin).expect("Failed to add recording bin");

            // Get our tee element by name, request a new source pad from it and
            // then link that to our recording bin to actually start receiving data
            let tee = pipeline
                .get_by_name("tee")
                .expect("Pipeline had no tee element");
            let srcpad = tee
                .get_request_pad("src_%u")
                .expect("Failed to request new pad from tee");
            let sinkpad = bin
                .get_static_pad("sink")
                .expect("Failed to get sink pad from recording bin");

            // If linking fails, we just undo what we did above
            if let Err(err) = srcpad.link(&sinkpad).into_result() {
                // TODO error dialogs
                eprintln!("Failed to link recording bin: {}", err);
                // This might fail but we don't care anymore: we're in an error path
                let _ = pipeline.remove(&bin);
                let _ = bin.set_state(gst::State::Null);
            }
        } else {
            // Get our recording bin, if it does not exist then nothing
            // has to be stopped actually. This shouldn't really happen
            let bin = pipeline
                .get_by_name("record-bin")
                .expect("Pipeline had no recording bin");

            // Get the source pad of the tee that is connected to the recording bin
            let sinkpad = bin
                .get_static_pad("sink")
                .expect("Failed to get sink pad from recording bin");
            let srcpad = match sinkpad.get_peer() {
                Some(peer) => peer,
                None => return,
            };

            // Once the tee source pad is idle and we wouldn't interfere with
            // any data flow, unlink the tee and the recording bin and finalize
            // the recording bin by sending it an end-of-stream event
            //
            // Once the end-of-stream event is handled by the whole recording bin,
            // we get an end-of-stream message from it in the message handler and
            // the shut down the recording bin and remove it from the pipeline
            //
            // The closure below might be called directly from the main UI thread
            // here or at a later time from a GStreamer streaming thread
            srcpad.add_probe(gst::PadProbeType::IDLE, move |srcpad, _| {
                // Get the parent of the tee source pad, i.e. the tee itself
                let tee = srcpad
                    .get_parent()
                    .and_then(|parent| parent.downcast::<gst::Element>().ok())
                    .expect("Failed to get tee source pad parent");

                // Unlink the tee source pad and then release it
                //
                // If unlinking fails we don't care, just make sure that the
                // pad is actually released
                let _ = srcpad.unlink(&sinkpad);
                tee.release_request_pad(srcpad);

                // Asynchronously send the end-of-stream event to the sinkpad as
                // this might block for a while and our closure here
                // might've been called from the main UI thread
                let sinkpad = sinkpad.clone();
                bin.call_async(move |_| {
                    sinkpad.send_event(gst::Event::new_eos().build());
                });

                // Don't block the pad but remove the probe to let everything
                // continue as normal
                gst::PadProbeReturn::Remove
            });
        }
    }

    fn build_ui(&self, application: &gtk::Application) {
        let window = gtk::ApplicationWindow::new(application);
        self.0.borrow_mut().main_window = Some(window.clone());

        window.set_title("RustFest 2018 GTK+ & GStreamer WebCam Viewer");
        window.set_border_width(5);
        window.set_position(gtk::WindowPosition::Center);
        window.set_default_size(350, 300);

        // Create headerbar for the application, including the main
        // menu and a close button
        let header_bar = gtk::HeaderBar::new();
        header_bar.set_show_close_button(true);

        let main_menu = gtk::MenuButton::new();
        let main_menu_image = gtk::Image::new_from_icon_name("open-menu-symbolic", 1);
        main_menu.add(&main_menu_image);

        // For now the main menu only contains the settings and about dialog
        let main_menu_model = gio::Menu::new();
        main_menu_model.append("Settings", "app.settings");
        main_menu_model.append("About", "app.about");
        main_menu.set_menu_model(&main_menu_model);

        let snapshot_button = gtk::ToggleButton::new();
        let snapshot_button_image = gtk::Image::new_from_icon_name("camera-photo", 1);
        snapshot_button.add(&snapshot_button_image);

        let record_button = gtk::ToggleButton::new();
        let record_button_image = gtk::Image::new_from_icon_name("media-record", 1);
        record_button.add(&record_button_image);

        // Pack the snapshot/record buttons on the left, the main menu on
        // the right of the header bar and set it on our window
        header_bar.pack_start(&snapshot_button);
        header_bar.pack_start(&record_button);
        header_bar.pack_end(&main_menu);
        window.set_titlebar(&header_bar);

        // Create an overlay for showing the seconds until a snapshot
        // This is hidden while we're not doing a countdown
        let overlay = gtk::Overlay::new();

        let overlay_text = gtk::Label::new("0");
        // Our label should have the countdown-label style from the stylesheet
        gtk::WidgetExt::set_name(&overlay_text, "countdown-label");

        // Center the label in the overlay and give it a width of 3 characters
        // to always have the same width independent of the width of the current
        // number
        overlay_text.set_halign(gtk::Align::Center);
        overlay_text.set_valign(gtk::Align::Center);
        overlay_text.set_width_chars(3);
        overlay_text.set_no_show_all(true);
        overlay_text.set_visible(false);

        overlay.add_overlay(&overlay_text);

        // When the snapshot button is clicked we need to start the
        // countdown, stop the countdown or directly do a snapshot
        let app_weak = self.downgrade();
        snapshot_button.connect_clicked(move |snapshot_button| {
            let app = upgrade_weak!(app_weak);
            app.on_snapshot_button_clicked(&snapshot_button, &overlay_text);
        });

        // When the record button is clicked we need to start or stop
        // recording based on its state
        let app_weak = self.downgrade();
        record_button.connect_clicked(move |record_button| {
            let app = upgrade_weak!(app_weak);
            app.on_record_button_clicked(&record_button);
        });

        // Create the pipeline and if that fails, shut down and
        // remember the error that happened
        let (pipeline, view) = match self.create_pipeline() {
            Err(err) => {
                self.0.borrow_mut().error = Some(err);
                application.quit();
                return;
            }
            Ok(res) => res,
        };

        // Store the pipeline for later usage and add the view widget
        // to the UI
        self.0.borrow_mut().pipeline = Some(pipeline);

        // A Box allows to place multiple widgets next to each other
        // vertically or horizontally
        let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
        vbox.pack_start(&view, true, true, 0);

        overlay.add(&vbox);
        window.add(&overlay);
    }

    fn on_startup(&self, application: &gtk::Application) {
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

        // Create our UI actions
        self.build_actions(application);

        // Build the UI but don't show it yet
        self.build_ui(application);
    }

    fn on_activate(&self, application: &gtk::Application) {
        let mut inner = self.0.borrow_mut();
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
    }

    fn on_shutdown(&self) {
        if let Some(ref pipeline) = self.0.borrow().pipeline {
            // This might fail but as we shut down right now anyway this
            // doesn't matter
            let _ = pipeline.set_state(gst::State::Null);
        }
    }
}

// Here we specify our custom, application specific CSS styles for various widgets
const STYLE: &'static str = "
#countdown-label {
    background-color: rgba(192, 192, 192, 0.8);
    color: black;
    font-size: 42pt;
    font-weight: bold;
}";

fn main() -> Result<(), Box<dyn error::Error>> {
    gst::init()?;
    let application = gtk::Application::new(APPLICATION_NAME, gio::ApplicationFlags::empty())?;

    let app = App::new(&application);

    // On application startup (of the main instance) we create
    // the actions and UI. A second process would not run this
    let app_weak = app.downgrade();
    application.connect_startup(move |application| {
        let app = upgrade_weak!(app_weak);
        app.on_startup(application);
    });

    // When the application is activated show the UI. This happens
    // when the first process is started, and in the first process
    // whenever a second process is started
    let app_weak = app.downgrade();
    application.connect_activate(move |application| {
        let app = upgrade_weak!(app_weak);
        app.on_activate(application);
    });

    // When the application is shut down, first shut down
    // the GStreamer pipeline so that capturing can gracefully stop
    let app_weak = app.downgrade();
    application.connect_shutdown(move |_| {
        let app = upgrade_weak!(app_weak);
        app.on_shutdown();
    });

    // And now run the application until the end
    application.run(&args().collect::<Vec<_>>());

    // If an error happened some time during the application,
    // return it here
    let mut app_inner = app.0.borrow_mut();
    if let Some(err) = app_inner.error.take() {
        Err(err)
    } else {
        Ok(())
    }
}
