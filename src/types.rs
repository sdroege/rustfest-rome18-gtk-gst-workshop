use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use gst;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum SnapshotFormat {
    JPEG,
    PNG,
}

impl<'a> From<&'a str> for SnapshotFormat {
    fn from(s: &'a str) -> Self {
        match s.to_lowercase().as_str() {
            "jpeg" => SnapshotFormat::JPEG,
            "png" => SnapshotFormat::PNG,
            _ => panic!("unsupported output format"),
        }
    }
}

impl From<Option<String>> for SnapshotFormat {
    fn from(s: Option<String>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "jpeg" => SnapshotFormat::JPEG,
                "png" => SnapshotFormat::PNG,
                _ => panic!("unsupported output format"),
            }
        } else {
            SnapshotFormat::default()
        }
    }
}

impl Default for SnapshotFormat {
    fn default() -> Self {
        SnapshotFormat::JPEG
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Serialize, Deserialize)]
pub enum RecordFormat {
    H264Mp4,
    Vp8WebM,
}

impl<'a> From<&'a str> for RecordFormat {
    fn from(s: &'a str) -> Self {
        match s.to_lowercase().as_str() {
            "h264/mp4" => RecordFormat::H264Mp4,
            "vp8/webm" => RecordFormat::Vp8WebM,
            _ => panic!("unsupported output format"),
        }
    }
}

impl From<Option<String>> for RecordFormat {
    fn from(s: Option<String>) -> Self {
        if let Some(s) = s {
            match s.to_lowercase().as_str() {
                "h264/mp4" => RecordFormat::H264Mp4,
                "vp8/webm" => RecordFormat::Vp8WebM,
                _ => panic!("unsupported output format"),
            }
        } else {
            RecordFormat::default()
        }
    }
}

impl Default for RecordFormat {
    fn default() -> Self {
        RecordFormat::H264Mp4
    }
}

#[derive(Deserialize, Serialize, Debug)]
pub struct Settings {
    // By default, the user's picture directory.
    pub snapshot_directory: PathBuf,
    // Format in which to save the snapshot.
    pub snapshot_format: SnapshotFormat,
    // Timer length in seconds.
    pub timer_length: u32,

    // By default, the user's video directory.
    pub record_directory: PathBuf,
    // Format to use for recording videos.
    pub record_format: RecordFormat,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            snapshot_directory: glib::get_user_special_dir(glib::UserDirectory::Pictures)
                .unwrap_or_else(|| PathBuf::from(".")),
            snapshot_format: SnapshotFormat::default(),
            timer_length: 3,
            record_directory: glib::get_user_special_dir(glib::UserDirectory::Videos)
                .unwrap_or_else(|| PathBuf::from(".")),
            record_format: RecordFormat::default(),
        }
    }
}

// Our refcounted application struct for containing all the
// state we have to carry around
#[derive(Clone)]
pub struct App(pub Rc<RefCell<AppInner>>);

pub struct AppWeak(pub Weak<RefCell<AppInner>>);

impl App {
    pub fn new() -> App {
        App(Rc::new(RefCell::new(AppInner {
            main_window: None,
            pipeline: None,
            timeout: None,
            remaining_secs_before_snapshot: 0,
        })))
    }

    pub fn downgrade(&self) -> AppWeak {
        AppWeak(Rc::downgrade(&self.0))
    }
}

impl AppWeak {
    pub fn upgrade(&self) -> Option<App> {
        self.0.upgrade().map(App)
    }
}

pub struct AppInner {
    pub main_window: Option<gtk::ApplicationWindow>,
    pub pipeline: Option<gst::Pipeline>,

    // Snapshot timer state
    pub timeout: Option<glib::source::SourceId>,
    pub remaining_secs_before_snapshot: u32,
}
