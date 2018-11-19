use gtk::prelude::*;

use utils;

use std::path::PathBuf;
use std::fs::create_dir_all;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum SnapshotFormat {
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

// Construct the settings dialog and ensure that the settings file exists and is loaded
pub fn create_settings_dialog(parent: &Option<gtk::Window>) {
    let s = utils::get_settings_file_path();

    if !s.exists() {
        if let Some(parent_dir) = s.parent() {
            if !parent_dir.exists() {
                if let Err(e) = create_dir_all(parent_dir) {
                    utils::show_error_dialog(
                        parent.as_ref(),
                        false,
                        format!(
                            "Error when trying to build settings snapshot_directory '{}': {:?}",
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

    //
    // BUILDING UI
    //
    let dialog = gtk::Dialog::new_with_buttons(
        Some("Snapshot settings"),
        parent.as_ref(),
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
        utils::save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry,
                             &record_directory_chooser_but, &record_format);
    });

    save_settings!(snapshot_format, connect_changed,
                   snapshot_directory_chooser_but, timer_entry, record_directory_chooser_but, record_format =>
                   move |snapshot_format| {
        utils::save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry,
                             &record_directory_chooser_but, &record_format);
    });

    save_settings!(snapshot_directory_chooser_but, connect_file_set, timer_entry, snapshot_format,
                   record_directory_chooser_but, record_format =>
                   move |snapshot_directory_chooser_but| {
        utils::save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry,
                             &record_directory_chooser_but, &record_format);
    });

    save_settings!(record_format, connect_changed,
                   snapshot_directory_chooser_but, timer_entry, record_directory_chooser_but, snapshot_format =>
                   move |record_format| {
        utils::save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry,
                             &record_directory_chooser_but, &record_format);
    });

    save_settings!(record_directory_chooser_but, connect_file_set,
                   timer_entry, snapshot_format, snapshot_directory_chooser_but, record_format =>
                   move |record_directory_chooser_but| {
        utils::save_settings(&snapshot_directory_chooser_but, &snapshot_format, &timer_entry,
                             &record_directory_chooser_but, &record_format);
    });

    dialog.connect_response(|dialog, _| {
        dialog.destroy();
    });

    dialog.set_resizable(false);
    dialog.show_all();
}
