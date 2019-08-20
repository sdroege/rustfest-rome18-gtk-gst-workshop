use glib;
use gtk::{self, prelude::*};

use crate::utils;

use std::cell::RefCell;
use std::fs::create_dir_all;
use std::ops;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum SnapshotFormat {
    JPEG,
    PNG,
}

// Convenience for converting from the strings in the combobox
impl From<Option<glib::GString>> for SnapshotFormat {
    fn from(s: Option<glib::GString>) -> Self {
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
pub enum RecordFormat {
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

impl From<Option<glib::GString>> for RecordFormat {
    fn from(s: Option<glib::GString>) -> Self {
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

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct Settings {
    // By default, the user's picture directory.
    pub snapshot_directory: PathBuf,
    // Format in which to save the snapshot.
    pub snapshot_format: SnapshotFormat,
    // Timer length in seconds.
    pub timer_length: u32,

    // By default, the user's video directory.
    pub record_directory: PathBuf,
    // Format to use for recording videos.
    pub record_format: RecordFormat,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
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

// Our refcounted settings struct for containing all the widgets we have to carry around.
//
// Once subclassing is possible this would become a gtk::Dialog subclass instead, which
// would simplify the code below considerably.
//
// This represents our settings dialog.
#[derive(Clone)]
struct SettingsDialog(Rc<SettingsDialogInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for SettingsDialog {
    type Target = SettingsDialogInner;

    fn deref(&self) -> &SettingsDialogInner {
        &*self.0
    }
}

// Weak reference to our settings dialog struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
struct SettingsDialogWeak(Weak<SettingsDialogInner>);

impl SettingsDialogWeak {
    // Upgrade to a strong reference if it still exists
    pub fn upgrade(&self) -> Option<SettingsDialog> {
        self.0.upgrade().map(SettingsDialog)
    }
}

struct SettingsDialogInner {
    snapshot_directory_chooser: gtk::FileChooserButton,
    snapshot_format: gtk::ComboBoxText,
    timer_entry: gtk::SpinButton,
    record_directory_chooser: gtk::FileChooserButton,
    record_format: gtk::ComboBoxText,
}

impl SettingsDialog {
    // Downgrade to a weak reference
    fn downgrade(&self) -> SettingsDialogWeak {
        SettingsDialogWeak(Rc::downgrade(&self.0))
    }

    // Take current settings value from all our widgets and store into the configuration file
    fn save_settings(&self) {
        let settings = Settings {
            snapshot_directory: self
                .snapshot_directory_chooser
                .get_filename()
                .unwrap_or_else(|| {
                    glib::get_user_special_dir(glib::UserDirectory::Pictures)
                        .unwrap_or_else(|| PathBuf::from("."))
                }),
            snapshot_format: SnapshotFormat::from(self.snapshot_format.get_active_text()),
            timer_length: self.timer_entry.get_value_as_int() as _,
            record_directory: self
                .record_directory_chooser
                .get_filename()
                .unwrap_or_else(|| {
                    glib::get_user_special_dir(glib::UserDirectory::Videos)
                        .unwrap_or_else(|| PathBuf::from("."))
                }),
            record_format: RecordFormat::from(self.record_format.get_active_text()),
        };

        utils::save_settings(&settings);
    }
}

// Construct the settings dialog and ensure that the settings file exists and is loaded
pub fn show_settings_dialog(application: &gtk::Application) {
    let s = utils::get_settings_file_path();

    if !s.exists() {
        if let Some(parent_dir) = s.parent() {
            if !parent_dir.exists() {
                if let Err(e) = create_dir_all(parent_dir) {
                    utils::show_error_dialog(
                        false,
                        format!(
                            "Error while trying to build settings snapshot_directory '{}': {}",
                            parent_dir.display(),
                            e
                        )
                        .as_str(),
                    );
                }
            }
        }
    }

    let settings = utils::load_settings();

    // Create an empty dialog with close button
    let dialog = gtk::Dialog::new_with_buttons(
        Some("WebCam Viewer settings"),
        application.get_active_window().as_ref(),
        gtk::DialogFlags::MODAL,
        &[("Close", gtk::ResponseType::Close)],
    );

    // All the UI widgets are going to be stored in a grid
    let grid = gtk::Grid::new();
    grid.set_column_spacing(4);
    grid.set_row_spacing(4);
    grid.set_margin_bottom(12);

    // File chooser for selecting the snapshot directory plus the label
    // next to it
    let snapshot_directory_label = gtk::Label::new(Some("Snapshot directory"));
    let snapshot_directory_chooser = gtk::FileChooserButton::new(
        "Pick a directory to save snapshots",
        gtk::FileChooserAction::SelectFolder,
    );

    snapshot_directory_label.set_halign(gtk::Align::Start);
    snapshot_directory_chooser.set_filename(settings.snapshot_directory);

    grid.attach(&snapshot_directory_label, 0, 0, 1, 1);
    grid.attach(&snapshot_directory_chooser, 1, 0, 3, 1);

    // Snapshot format combobox plus the label next to it
    let format_label = gtk::Label::new(Some("Snapshot format"));
    let snapshot_format = gtk::ComboBoxText::new();

    format_label.set_halign(gtk::Align::Start);

    // We'll add our 2 support snapshot formats as text here and select
    // the configured one
    snapshot_format.append_text("JPEG");
    snapshot_format.append_text("PNG");
    snapshot_format.set_active(match settings.snapshot_format {
        SnapshotFormat::JPEG => Some(0),
        SnapshotFormat::PNG => Some(1),
    });
    snapshot_format.set_hexpand(true);

    grid.attach(&format_label, 0, 1, 1, 1);
    grid.attach(&snapshot_format, 1, 1, 3, 1);

    // Snapshot timer length spin button plus the label next to it
    let timer_label = gtk::Label::new(Some("Timer length (in seconds)"));
    // We allow 0 to 15 seconds, in 1 second steps
    let timer_entry = gtk::SpinButton::new_with_range(0., 15., 1.);

    timer_label.set_halign(gtk::Align::Start);
    timer_label.set_hexpand(true);

    timer_entry.set_value(settings.timer_length as f64);

    grid.attach(&timer_label, 0, 2, 1, 1);
    grid.attach(&timer_entry, 1, 2, 3, 1);

    // File chooser for selecting the record directory plus the label
    // next to it
    let record_directory_label = gtk::Label::new(Some("Record directory"));
    let record_directory_chooser = gtk::FileChooserButton::new(
        "Pick a directory to save records",
        gtk::FileChooserAction::SelectFolder,
    );

    record_directory_label.set_halign(gtk::Align::Start);
    record_directory_chooser.set_filename(settings.record_directory);

    grid.attach(&record_directory_label, 0, 3, 1, 1);
    grid.attach(&record_directory_chooser, 1, 3, 3, 1);

    // Record format combobox plus the label next to it
    let format_label = gtk::Label::new(Some("Record format"));
    let record_format = gtk::ComboBoxText::new();

    format_label.set_halign(gtk::Align::Start);

    record_format.append_text("H264/MP4");
    record_format.append_text("VP8/WebM");
    record_format.set_active(match settings.record_format {
        RecordFormat::H264Mp4 => Some(0),
        RecordFormat::Vp8WebM => Some(1),
    });
    record_format.set_hexpand(true);

    grid.attach(&format_label, 0, 4, 1, 1);
    grid.attach(&record_format, 1, 4, 3, 1);

    // Put the grid into the dialog's content area
    let content_area = dialog.get_content_area();
    content_area.pack_start(&grid, true, true, 0);
    content_area.set_border_width(10);

    let settings_dialog = SettingsDialog(Rc::new(SettingsDialogInner {
        snapshot_directory_chooser,
        snapshot_format,
        timer_entry,
        record_directory_chooser,
        record_format,
    }));

    // Finally connect to all kinds of change notification signals for the different UI widgets.
    // Whenever something is changing we directly save the configuration file with the new values.
    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog
        .snapshot_directory_chooser
        .connect_file_set(move |_| {
            let settings_dialog = upgrade_weak!(settings_dialog_weak);
            settings_dialog.save_settings();
        });

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog.snapshot_format.connect_changed(move |_| {
        let settings_dialog = upgrade_weak!(settings_dialog_weak);
        settings_dialog.save_settings();
    });

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog.timer_entry.connect_value_changed(move |_| {
        let settings_dialog = upgrade_weak!(settings_dialog_weak);
        settings_dialog.save_settings();
    });

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog
        .record_directory_chooser
        .connect_file_set(move |_| {
            let settings_dialog = upgrade_weak!(settings_dialog_weak);
            settings_dialog.save_settings();
        });

    let settings_dialog_weak = settings_dialog.downgrade();
    settings_dialog.record_format.connect_changed(move |_| {
        let settings_dialog = upgrade_weak!(settings_dialog_weak);
        settings_dialog.save_settings();
    });

    // Close the dialog when the close button is clicked. We don't need to save the settings here
    // as we already did that whenever the user changed something in the UI.
    //
    // The closure keeps the one and only strong reference to our settings dialog struct and it
    // will be freed once the dialog is destroyed
    let settings_dialog_storage = RefCell::new(Some(settings_dialog));
    dialog.connect_response(move |dialog, _| {
        dialog.destroy();

        let _ = settings_dialog_storage.borrow_mut().take();
    });

    dialog.set_resizable(false);
    dialog.show_all();
}
