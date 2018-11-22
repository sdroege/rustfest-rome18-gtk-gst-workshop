use gio::{self, prelude::*, MenuExt};
use gtk::{self, prelude::*};

pub struct HeaderBar {
    snapshot: gtk::ToggleButton,
    record: gtk::ToggleButton,
}

// Create headerbar for the application, including the main
// menu and a close button
impl HeaderBar {
    pub fn new<P: gtk::GtkWindowExt>(window: &P) -> Self {
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
        let snapshot_button_image = gtk::Image::new_from_icon_name("camera-photo-symbolic", 1);
        snapshot_button.add(&snapshot_button_image);

        snapshot_button.connect_toggled(|snapshot_button| {
            let app = gio::Application::get_default().expect("No default application");

            let action = app
                .lookup_action("snapshot")
                .expect("Snapshot action not found");
            action.change_state(&snapshot_button.get_active().to_variant());
        });

        let record_button = gtk::ToggleButton::new();
        let record_button_image = gtk::Image::new_from_icon_name("media-record", 1);
        record_button.add(&record_button_image);

        record_button.connect_toggled(|record_button| {
            let app = gio::Application::get_default().expect("No default application");

            let action = app
                .lookup_action("record")
                .expect("Record action not found");
            action.change_state(&record_button.get_active().to_variant());
        });

        // Pack the snapshot/record buttons on the left, the main menu on
        // the right of the header bar and set it on our window
        header_bar.pack_start(&snapshot_button);
        header_bar.pack_start(&record_button);
        header_bar.pack_end(&main_menu);

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
