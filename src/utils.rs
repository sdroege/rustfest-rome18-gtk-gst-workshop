use gio::{self, prelude::*};
use glib;
use gtk::{self, prelude::*};

use std::path::PathBuf;

use serde_any;

use settings::Settings;
use APPLICATION_NAME;

// Get the default path for the settings file
pub fn get_settings_file_path() -> PathBuf {
    let mut path = glib::get_user_config_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(APPLICATION_NAME);
    path.push("settings.toml");
    path
}

// Save the provided settings to the settings path
pub fn save_settings(settings: &Settings) {
    let s = get_settings_file_path();
    if let Err(e) = serde_any::to_file(&s, &settings) {
        show_error_dialog(
            false,
            format!("Error while trying to save file: {}", e).as_str(),
        );
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
                    false,
                    format!("Error while opening '{}': {}", s.display(), e).as_str(),
                );
                Settings::default()
            }
        }
    } else {
        Settings::default()
    }
}

// Shows an error dialog, and if it's fatal it will quit the application once
// the dialog is closed
pub fn show_error_dialog(fatal: bool, text: &str) {
    let app = gio::Application::get_default()
        .expect("No default application")
        .downcast::<gtk::Application>()
        .expect("Default application has wrong type");

    let dialog = gtk::MessageDialog::new(
        app.get_active_window().as_ref(),
        gtk::DialogFlags::MODAL,
        gtk::MessageType::Error,
        gtk::ButtonsType::Ok,
        text,
    );

    dialog.connect_response(move |dialog, _| {
        dialog.destroy();

        if fatal {
            app.quit();
        }
    });

    dialog.set_resizable(false);
    dialog.show_all();
}
