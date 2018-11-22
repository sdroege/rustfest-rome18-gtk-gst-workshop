extern crate gdk;
extern crate gio;
extern crate glib;
extern crate gtk;

extern crate gstreamer as gst;
extern crate gstreamer_video as gst_video;

extern crate fragile;

#[macro_use]
extern crate serde;
extern crate serde_any;

extern crate chrono;

#[macro_use]
mod macros;

mod about_dialog;
pub mod app;
mod headerbar;
mod overlay;
mod pipeline;
pub mod settings;
pub mod utils;

use gio::prelude::*;

use std::env::args;
use std::error;

use app::App;

pub const APPLICATION_NAME: &str = "com.github.rustfest";

fn main() -> Result<(), Box<dyn error::Error>> {
    gst::init()?;
    let application = gtk::Application::new(APPLICATION_NAME, gio::ApplicationFlags::empty())?;

    // On application startup (of the main instance) we create
    // the actions and UI. A second process would not run this
    application.connect_startup(|application| {
        App::on_startup(application);
    });

    // And now run the application until the end
    application.run(&args().collect::<Vec<_>>());

    Ok(())
}
