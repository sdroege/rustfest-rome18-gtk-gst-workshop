use glib;

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
        eprintln!("Error while trying to save file: {:?}", e);
    }
}

// Load the current settings
pub fn load_settings() -> Settings {
    let s = get_settings_file_path();
    if s.exists() && s.is_file() {
        match serde_any::from_file::<Settings, _>(&s) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error while opening '{}': {:?}", s.display(), e);
                Settings::default()
            }
        }
    } else {
        Settings::default()
    }
}
