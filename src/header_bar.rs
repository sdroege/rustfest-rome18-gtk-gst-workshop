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

        // Insert the headerbar as titlebar into the window
        window.set_titlebar(&header_bar);

        HeaderBar {}
    }
}
