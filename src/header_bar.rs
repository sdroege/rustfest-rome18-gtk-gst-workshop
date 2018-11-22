use gio::{self, MenuExt};
use gtk::{self, prelude::*};

pub struct HeaderBar {}

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

        // Insert the headerbar as titlebar into the window
        window.set_titlebar(&header_bar);

        HeaderBar {}
    }
}
