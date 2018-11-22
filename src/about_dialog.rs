use gtk::{self, prelude::*};

pub fn create_about_dialog(application: &gtk::Application) {
    let dialog = gtk::AboutDialog::new();

    dialog.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
    dialog.set_website_label("github repository");
    dialog.set_website("https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop");
    dialog.set_comments("A webcam viewer written with gtk-rs and gstreamer-rs");
    dialog.set_copyright("This is under MIT license");
    dialog.set_transient_for(application.get_active_window().as_ref());
    dialog.set_modal(true);
    dialog.set_program_name("RustFest 2018 GTK+ & GStreamer WebCam Viewer");

    // When any response on the dialog happens, we simply destroy it.
    //
    // We don't have any custom buttons added so this will only ever
    // handle the close button, otherwise we could distinguish the
    // buttons by the response
    dialog.connect_response(|dialog, _response| {
        dialog.destroy();
    });

    dialog.show_all();
}
