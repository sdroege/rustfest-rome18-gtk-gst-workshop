use gtk::{self, prelude::*};

pub struct Overlay {
    // Our overlay widget
    pub container: gtk::Overlay,
    // The Countdown label... lift off!
    pub label: gtk::Label,
    // The container that will hold the gstreamer widget
    content: gtk::Box,
}

impl Default for Overlay {
    fn default() -> Self {
        // Create an overlay for showing the seconds until a snapshot
        // This is hidden while we're not doing a countdown
        let overlay = gtk::Overlay::new();
        let label = gtk::Label::new("0");

        // Our label should have the countdown-label style from the stylesheet
        gtk::WidgetExt::set_name(&label, "countdown-label");

        // Center the label in the overlay and give it a width of 3 characters
        // to always have the same width independent of the width of the current
        // number
        label.set_halign(gtk::Align::Center);
        label.set_valign(gtk::Align::Center);
        label.set_width_chars(3);
        label.set_no_show_all(true);
        label.set_visible(false);

        // Add the label to our overlay
        overlay.add_overlay(&label);

        // A Box allows to place multiple widgets next to each other
        // vertically or horizontally
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        overlay.add(&content);

        Overlay {
            container: overlay,
            label,
            content,
        }
    }
}

impl Overlay {
    // Add the widget to the content container
    pub fn initialize_content<P: IsA<gtk::Widget>>(&self, widget: &P) {
        self.content.pack_start(widget, true, true, 0);
    }
}
