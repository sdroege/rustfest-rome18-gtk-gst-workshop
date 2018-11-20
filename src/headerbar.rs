use gio::{self, MenuExt};
use gtk::{self, prelude::*};

pub struct HeaderBar {
    pub container: gtk::HeaderBar,
    menu: gtk::MenuButton,
    menu_model: gio::Menu,
    // FIXME: make them private
    pub snapshot: gtk::ToggleButton,
    pub record: gtk::ToggleButton,
}

// Create headerbar for the application, including the main
// menu and a close button
impl Default for HeaderBar {
    fn default() -> Self {
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

        let record_button = gtk::ToggleButton::new();
        let record_button_image = gtk::Image::new_from_icon_name("media-record", 1);
        record_button.add(&record_button_image);

        // Pack the snapshot/record buttons on the left, the main menu on
        // the right of the header bar and set it on our window
        header_bar.pack_start(&snapshot_button);
        header_bar.pack_start(&record_button);
        header_bar.pack_end(&main_menu);

        HeaderBar {
            container: header_bar,
            menu_model: main_menu_model,
            menu: main_menu,
            snapshot: snapshot_button,
            record: record_button,
        }
    }
}
