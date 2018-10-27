extern crate gio;
extern crate glib;
extern crate gstreamer as gst;
extern crate gtk;

extern crate fragile;

use gio::prelude::*;
use gio::MenuExt;
use gtk::prelude::*;

use gst::prelude::*;
use gst::BinExt;

use std::cell::RefCell;
use std::env::args;
use std::error;
use std::rc::{Rc, Weak};

macro_rules! upgrade_weak {
    ($x:ident, $r:expr) => {{
        match $x.upgrade() {
            Some(o) => o,
            None => return $r,
        }
    }};
    ($x:ident) => {
        upgrade_weak!($x, ())
    };
}

// Our refcounted application struct for containing all the
// state we have to carry around
#[derive(Clone)]
struct App(Rc<RefCell<AppInner>>);

struct AppWeak(Weak<RefCell<AppInner>>);

impl App {
    fn new(application: &gtk::Application) -> App {
        App(Rc::new(RefCell::new(AppInner {
            application: application.clone(),
            main_window: None,
            pipeline: None,
            error: None,
        })))
    }

    fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }
}

impl AppWeak {
    fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

struct AppInner {
    application: gtk::Application,
    main_window: Option<gtk::ApplicationWindow>,
    pipeline: Option<gst::Pipeline>,
    error: Option<Box<dyn error::Error>>,
}

fn build_actions(_app: &App, application: &gtk::Application) {
    // Create app.about action for the about dialog
    //
    // This can be activated from anywhere where we have access
    // to the application, not just the main window
    let about = gio::SimpleAction::new("about", None);

    // When activated, show an about dialog
    let weak_application = application.downgrade();
    about.connect_activate(move |_action, _parameter| {
        let application = upgrade_weak!(weak_application);

        let p = gtk::AboutDialog::new();

        p.set_authors(&["Sebastian Dröge", "Guillaume Gomez"]);
        p.set_website_label("github repository");
        p.set_website("https://github.com/sdroege/rustfest-rome18-gtk-gst-workshop");
        p.set_comments("A webcam viewer written with gtk-rs and gstreamer-rs");
        p.set_copyright("This is under MIT license");
        if let Some(window) = application.get_active_window() {
            p.set_transient_for(&window);
        }
        p.set_modal(true);
        p.set_program_name("RustFest 2018 GTK+ & GStreamer WebCam Viewer");

        // When any response on the dialog happens, we simply destroy it.
        //
        // We don't have any custom buttons added so this will only ever
        // handle the close button, otherwise we could distinguish the
        // buttons by the response
        p.connect_response(|dialog, _response| {
            dialog.destroy();
        });

        p.show_all();
    });

    application.add_action(&about);
}

fn create_pipeline(app: &App) -> Result<(gst::Pipeline, gtk::Widget), Box<dyn error::Error>> {
    // Create a new GStreamer pipeline that captures from the default video source,
    // which is usually a camera, converts the output to RGB if needed and then passes
    // it to a GTK video sink
    let pipeline = gst::parse_launch("autovideosrc ! queue ! videoconvert ! gtksink name=sink")?;

    // Upcast to a gst::Pipeline as the above function could've also returned
    // an arbitrary gst::Element if a different string was passed
    let pipeline = pipeline.downcast::<gst::Pipeline>().unwrap();

    // Install a message handler on the pipeline's bus to catch errors
    let bus = pipeline.get_bus().unwrap();

    // GStreamer is thread-safe and it is possible to attach
    // bus watches from any thread, which are then nonetheless
    // called from the main thread. As such we have to make use
    // of fragile::Fragile() here to be able to pass our non-Send
    // application struct into a closure that requires Send.
    //
    // As we are on the main thread and the closure will be called
    // on the main thread, this will not cause a panic and is perfectly
    // safe.
    let app_weak = fragile::Fragile::new(app.downgrade());
    bus.add_watch(move |_bus, msg| {
        use gst::MessageView;

        let app_weak = app_weak.get();
        let app = upgrade_weak!(app_weak, glib::Continue(false));

        // A message can contain various kinds of information but
        // here we are only interested in errors so far
        match msg.view() {
            MessageView::Error(err) => {
                eprintln!(
                    "Error from {:?}: {} ({:?})",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );

                // On errors, we store the error that happened
                // and print it later
                let mut inner = app.0.borrow_mut();

                inner.error = Some(Box::new(err.get_error()));
                inner.application.quit();
            }
            _ => (),
        };

        glib::Continue(true)
    });

    // Get the GTK video sink and retrieve the video display widget from it
    let sink = pipeline.get_by_name("sink").unwrap();
    let widget_value = sink.get_property("widget").unwrap();
    let widget = widget_value.get::<gtk::Widget>().unwrap();

    Ok((pipeline, widget))
}

fn save_screenshot(
    file_chooser: &gtk::FileChooserWidget,
    dialog: &gtk::Dialog,
    options: &gtk::ComboBoxText,
) {
    // Normally, this check shouldn't be needed but better be safe than sorry.
    if file_chooser.get_filename().is_none() {
        return;
    }
    // TODO: save file in the given format.
    // options.get_active_text();
    dialog.destroy();
}

fn build_save_screenshot_window(parent: &gtk::ApplicationWindow) {
    let dialog = gtk::Dialog::new_with_buttons(Some("Save screenshot"),
                                               Some(parent),
                                               gtk::DialogFlags::MODAL,
                                               &[]);

    let options = gtk::ComboBoxText::new();
    options.append_text("BMP");
    options.append_text("JPEG");
    options.append_text("PNG");
    options.set_active(0);

    let save_button = gtk::Button::new_with_label("Save");
    save_button.set_sensitive(false);

    let file_chooser = gtk::FileChooserWidget::new(gtk::FileChooserAction::Save);

    let content_area = dialog.get_content_area();
    content_area.add(&file_chooser);
    content_area.add(&options);
    content_area.add(&save_button);

    let dialog_weak = dialog.downgrade();
    let options_weak = options.downgrade();
    file_chooser.connect_file_activated(move |file_chooser| {
        let dialog = upgrade_weak!(dialog_weak, ());
        let options = upgrade_weak!(options_weak, ());
        save_screenshot(file_chooser, &dialog, &options);
    });
    let dialog_weak = dialog.downgrade();
    let file_chooser_weak = file_chooser.downgrade();
    save_button.connect_clicked(move |_| {
        let dialog = upgrade_weak!(dialog_weak, ());
        let file_chooser = upgrade_weak!(file_chooser_weak, ());
        save_screenshot(&file_chooser, &dialog, &options);
    });
    file_chooser.connect_selection_changed(move |file_chooser| {
        save_button.set_sensitive(file_chooser.get_filename().is_some());
    });

    dialog.show_all();
}

fn build_ui(app: &App, application: &gtk::Application) {
    let window = gtk::ApplicationWindow::new(application);
    app.0.borrow_mut().main_window = Some(window.clone());

    window.set_title("RustFest 2018 GTK+ & GStreamer WebCam Viewer");
    window.set_border_width(5);
    window.set_position(gtk::WindowPosition::Center);
    window.set_default_size(350, 300);

    // When our main window is closed, the whole application should be
    // shut down
    let application_weak = application.downgrade();
    window.connect_delete_event(move |_, _| {
        let application = upgrade_weak!(application_weak, Inhibit(false));
        application.quit();
        Inhibit(false)
    });

    // Create headerbar for the application, including the main
    // menu and a close button
    let header_bar = gtk::HeaderBar::new();
    header_bar.set_show_close_button(true);

    let main_menu = gtk::MenuButton::new();
    let main_menu_image = gtk::Image::new_from_icon_name("open-menu-symbolic", 1);
    main_menu.add(&main_menu_image);

    // For now the main menu only contains the about dialog
    let main_menu_model = gio::Menu::new();
    main_menu_model.append("About", "app.about");
    main_menu.set_menu_model(&main_menu_model);

    let screenshot_button = gtk::Button::new();
    let screenshot_button_image = gtk::Image::new_from_icon_name("camera-photo", 1);
    screenshot_button.add(&screenshot_button_image);

    let window_weak = window.downgrade();
    screenshot_button.connect_clicked(move |_| {
        let window = upgrade_weak!(window_weak, ());
        build_save_screenshot_window(&window);
    });

    header_bar.pack_end(&main_menu);
    header_bar.pack_end(&screenshot_button);
    window.set_titlebar(&header_bar);

    // Create the pipeline and if that fails, shut down and
    // remember the error that happened
    let (pipeline, view) = match create_pipeline(app) {
        Err(err) => {
            app.0.borrow_mut().error = Some(err);
            application.quit();
            return;
        }
        Ok(res) => res,
    };

    // Store the pipeline for later usage and add the view widget
    // to the UI
    app.0.borrow_mut().pipeline = Some(pipeline);

    // A Box allows to place multiple widgets next to each other
    // vertically or horizontally
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    vbox.pack_start(&view, true, true, 0);
    window.add(&vbox);
}

fn main() -> Result<(), Box<dyn error::Error>> {
    gst::init()?;
    let application = gtk::Application::new("com.github.rustfest", gio::ApplicationFlags::empty())?;

    let app = App::new(&application);

    // On application startup (of the main instance) we create
    // the actions and UI. A second process would not run this
    let app_weak = app.downgrade();
    application.connect_startup(move |application| {
        let app = upgrade_weak!(app_weak);
        build_actions(&app, application);
        // Build the UI but don't show it yet
        build_ui(&app, application);
    });

    // When the application is activated show the UI. This happens
    // when the first process is started, and in the first process
    // whenever a second process is started
    let app_weak = app.downgrade();
    application.connect_activate(move |application| {
        let app = upgrade_weak!(app_weak);
        let mut inner = app.0.borrow_mut();
        // We only show our window here once the application
        // is activated. This means that when a second instance
        // is started, the window of the first instance will be
        // brought to the foreground
        if let Some(ref main_window) = inner.main_window {
            main_window.show_all();
            main_window.present();
        }

        // Once the UI is shown, start the GStreamer pipeline. If
        // an error happens, we immediately shut down
        {
            // Needed because we need to borrow two parts of the application struct
            let AppInner {
                ref pipeline,
                ref mut error,
                ..
            } = *inner;

            if let Some(ref pipeline) = pipeline {
                if let Err(err) = pipeline.set_state(gst::State::Playing).into_result() {
                    *error = Some(Box::new(err));
                    application.quit();
                }
            }
        }
    });

    // When the application is shut down, first shut down
    // the GStreamer pipeline so that capturing can gracefully stop
    let app_weak = app.downgrade();
    application.connect_shutdown(move |_| {
        let app = upgrade_weak!(app_weak);
        let inner = app.0.borrow();

        if let Some(ref pipeline) = inner.pipeline {
            let _ = pipeline.set_state(gst::State::Null);
        }
    });

    // And now run the application until the end
    application.run(&args().collect::<Vec<_>>());

    let mut app_inner = app.0.borrow_mut();
    if let Some(err) = app_inner.error.take() {
        Err(err)
    } else {
        Ok(())
    }
}
