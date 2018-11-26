use gio::{self, prelude::*, MenuExt};
use gtk::{self, prelude::*};

use app::{RecordState, SnapshotState};

pub struct HeaderBar {
    snapshot: gtk::ToggleButton,
    record: gtk::ToggleButton,
}

// Create headerbar for the application
//
// This includes the close button and in the future will include also various buttons
impl HeaderBar {
    pub fn new<P: gtk::GtkWindowExt>(window: &P) -> Self {
        let header_bar = gtk::HeaderBar::new();

        // Without this the headerbar will have no close button
        header_bar.set_show_close_button(true);

        // Create a menu button with the hamburger menu
        let main_menu = gtk::MenuButton::new();
        let main_menu_image = gtk::Image::new_from_icon_name("open-menu-symbolic", 1);
        main_menu.set_image(&main_menu_image);

        // Create the menu model with the menu items. These directly activate our application
        // actions by their name
        let main_menu_model = gio::Menu::new();
        main_menu_model.append("Settings", "app.settings");
        main_menu_model.append("About", "app.about");
        main_menu.set_menu_model(&main_menu_model);

        // And place it on the right (end) side of the header bar
        header_bar.pack_end(&main_menu);

        // Create snapshot button and let it trigger the snapshot action
        let snapshot_button = gtk::ToggleButton::new();
        let snapshot_button_image = gtk::Image::new_from_icon_name("camera-photo-symbolic", 1);
        snapshot_button.set_image(&snapshot_button_image);

        snapshot_button.connect_toggled(|snapshot_button| {
            let app = gio::Application::get_default().expect("No default application");

            app.change_action_state(
                "snapshot",
                &SnapshotState::from(snapshot_button.get_active()).into(),
            );
        });

        // Place the snapshot button on the left
        header_bar.pack_start(&snapshot_button);

        // Create record button and let it trigger the record action
        let record_button = gtk::ToggleButton::new();
        let record_button_image = gtk::Image::new_from_icon_name("media-record", 1);
        record_button.set_image(&record_button_image);

        record_button.connect_toggled(|record_button| {
            let app = gio::Application::get_default().expect("No default application");

            app.change_action_state(
                "record",
                &RecordState::from(record_button.get_active()).into(),
            );
        });

        // Place the record button on the left, right of the snapshot button
        header_bar.pack_start(&record_button);

        // Insert the headerbar as titlebar into the window
        window.set_titlebar(&header_bar);

        HeaderBar {
            snapshot: snapshot_button,
            record: record_button,
        }
    }

    pub fn set_snapshot_active(&self, active: bool) {
        self.snapshot.set_active(active);
    }

    pub fn set_record_active(&self, active: bool) {
        self.record.set_active(active);
    }
}
