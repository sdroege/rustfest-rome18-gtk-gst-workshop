use gtk::{self, prelude::*};

pub struct Overlay {
    // The Countdown label, hidden by default
    label: gtk::Label,
}

impl Overlay {
    pub fn new<W: IsA<gtk::Container>, U: IsA<gtk::Widget>>(container: &W, content: &U) -> Self {
        // Create an overlay for showing the seconds until a snapshot This is hidden while we're
        // not doing a countdown
        let overlay = gtk::Overlay::new();
        let label = gtk::Label::new(Some("0"));

        // Our label should have the countdown-label style from the stylesheet
        //
        // We have to call the trait function directly because label implements multiple traits
        // that provide a set_name() function
        gtk::WidgetExt::set_name(&label, "countdown-label");

        // Center the label in the overlay and give it a width of 3 characters to always have the
        // same width independent of the width of the current number
        label.set_halign(gtk::Align::Center);
        label.set_valign(gtk::Align::Center);
        label.set_width_chars(3);
        label.set_no_show_all(true);
        label.set_visible(false);

        // Add the label to our overlay
        overlay.add_overlay(&label);

        // Add the actual window content
        overlay.add(content);

        // Add ourselves to the container, i.e. our window
        container.add(&overlay);

        Overlay { label }
    }

    pub fn set_label_visible(&self, visible: bool) {
        self.label.set_visible(visible);
    }

    pub fn set_label_text(&self, text: &str) {
        self.label.set_text(text);
    }
}
