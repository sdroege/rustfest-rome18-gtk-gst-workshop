use gtk::{self, prelude::*};

pub fn show_about_dialog(application: &gtk::Application) {
    let dialog = gtk::AboutDialog::new();

    dialog.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
    dialog.set_website_label("GitHub repository");
    dialog.set_website("https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop");
    dialog.set_comments("A webcam viewer written with gtk-rs and gstreamer-rs");
    dialog.set_copyright("This is under MIT license");
    dialog.set_program_name("WebCam Viewer");
    dialog.set_logo_icon_name("camera-photo");

    // Make the about dialog modal and transient for our currently active application window. This
    // prevents the user from sending any events to the main window as long as the dialog is open.
    dialog.set_transient_for(application.get_active_window().as_ref());
    dialog.set_modal(true);

    // When any response on the dialog happens, we simply destroy it.
    //
    // We don't have any custom buttons added so this will only ever handle the close button.
    // Otherwise we could distinguish the buttons by the response
    dialog.connect_response(|dialog, _response| {
        dialog.destroy();
    });

    dialog.show_all();
}
