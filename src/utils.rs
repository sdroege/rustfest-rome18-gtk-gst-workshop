use gio::prelude::*;
use glib;
use gtk;
use gtk::prelude::*;

use std::path::PathBuf;

use RecordFormat;
use Settings;
use SnapshotFormat;
use APPLICATION_NAME;

pub fn get_settings_file_path() -> PathBuf {
    let mut path = glib::get_user_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(APPLICATION_NAME);
    path.push("settings.toml");
    path
}

// Save the current settings from the values of the various UI elements
pub fn save_settings(
    snapshot_directory_button: &gtk::FileChooserButton,
    snapshot_format: &gtk::ComboBoxText,
    timer_entry: &gtk::SpinButton,
    record_directory_button: &gtk::FileChooserButton,
    record_format: &gtk::ComboBoxText,
) {
    let settings = Settings {
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
    }
}

// Load the current settings
pub fn load_settings() -> Settings {
    let s = get_settings_file_path();
    if s.exists() && s.is_file() {
        match serde_any::from_file::<Settings, _>(&s) {
            Ok(s) => s,
            Err(e) => {
                show_error_dialog(
                    None::<&gtk::Window>,
                    false,
                    format!("Error when opening '{}': {:?}", s.display(), e).as_str(),
                );
                Settings::default()
            }
        }
    } else {
        Settings::default()
    }
}

// Creates an error dialog, and if it's fatal it will quit the application once
// the dialog is closed
pub fn show_error_dialog<P: IsA<gtk::Window>>(parent: Option<&P>, fatal: bool, text: &str) {
    let dialog = gtk::MessageDialog::new(
        parent,
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Ok,
        text,
    );

    dialog.connect_response(move |dialog, _| {
        dialog.destroy();
        if fatal {
            if let Some(app) = gio::Application::get_default() {
                app.quit();
            }
        }
    });

    dialog.set_resizable(false);
    dialog.show_all();
}
