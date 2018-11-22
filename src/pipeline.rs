use gio::{self, prelude::*};
use glib;
use gst::{self, prelude::*, BinExt};
use gst_video;
use gtk;

use std::cell::RefCell;
use std::error;
use std::ops;
use std::rc::{Rc, Weak};

use fragile;

use chrono::prelude::*;

use settings::{RecordFormat, SnapshotFormat};
use utils;

// Our refcounted pipeline struct for containing all the media state we have to carry around.
//
// Once subclassing is possible this would become a gst::Pipeline subclass instead, which
// would simplify the code below considerably.
#[derive(Clone)]
pub struct Pipeline(Rc<PipelineInner>);

// Deref into the contained struct to make usage a bit more ergonomic
impl ops::Deref for Pipeline {
    type Target = PipelineInner;

    fn deref(&self) -> &PipelineInner {
        &*self.0
    }
}

pub struct PipelineInner {
    pipeline: gst::Pipeline,
    tee: gst::Element,
    sink: gst::Element,
    recording_bin: RefCell<Option<gst::Bin>>,
}

// Weak reference to our pipeline struct
//
// Weak references are important to prevent reference cycles. Reference cycles are cases where
// struct A references directly or indirectly struct B, and struct B references struct A again
// while both are using reference counting.
pub struct PipelineWeak(Weak<PipelineInner>);
impl PipelineWeak {
    pub fn upgrade(&self) -> Option<Pipeline> {
        self.0.upgrade().map(Pipeline)
    }
}

impl Pipeline {
    pub fn new() -> Result<Self, Box<dyn error::Error>> {
        // Create a new GStreamer pipeline that captures from the default video source, which is
        // usually a camera, converts the output to RGB if needed and then passes it to a GTK video
        // sink
        let pipeline = gst::parse_launch(
            "autovideosrc ! tee name=tee ! queue ! videoconvert ! gtksink name=sink",
        )?;

        // Upcast to a gst::Pipeline as the above function could've also returned an arbitrary
        // gst::Element if a different string was passed
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Couldn't downcast pipeline");

        // Retrieve sink and tee elements from the pipeline for later use
        let tee = pipeline.get_by_name("tee").expect("No tee found");
        let sink = pipeline.get_by_name("sink").expect("No sink found");

        // XXX: Workaround for a bug on macOS
        //
        // When recording is started, the source could potentially reconfigure itself.
        // Unfortunately this causes the camera source on macOS to fail completely instead.
        // To prevent this we drop all Reconfigure events so that the source never tries to
        // reconfigure.
        {
            let sinkpad = tee.get_static_pad("sink").expect("tee has no sinkpad");
            sinkpad.add_probe(gst::PadProbeType::EVENT_UPSTREAM, |_pad, info| {
                match info.data {
                    Some(gst::PadProbeData::Event(ref ev))
                        if ev.get_type() == gst::EventType::Reconfigure =>
                    {
                        return gst::PadProbeReturn::Drop;
                    }
                    _ => (),
                }

                gst::PadProbeReturn::Ok
            });
        }

        let pipeline = Pipeline(Rc::new(PipelineInner {
            pipeline,
            sink,
            tee,
            recording_bin: RefCell::new(None),
        }));

        // Install a message handler on the pipeline's bus to catch errors
        let bus = pipeline.pipeline.get_bus().expect("Pipeline had no bus");

        // GStreamer is thread-safe and it is possible to attach bus watches from any thread, which
        // are then nonetheless called from the main thread. As such we have to make use of
        // fragile::Fragile() here to be able to pass our non-Send application struct into a
        // closure that requires Send.
        //
        // As we are on the main thread and the closure will be called on the main thread, this
        // will not cause a panic and is perfectly safe.
        let pipeline_weak = fragile::Fragile::new(pipeline.downgrade());
        bus.add_watch(move |_bus, msg| {
            let pipeline_weak = pipeline_weak.get();
            let pipeline = upgrade_weak!(pipeline_weak, glib::Continue(false));

            pipeline.on_pipeline_message(msg);

            glib::Continue(true)
        });

        Ok(pipeline)
    }

    // Downgrade to a weak reference
    pub fn downgrade(&self) -> PipelineWeak {
        PipelineWeak(Rc::downgrade(&self.0))
    }

    pub fn get_widget(&self) -> gtk::Widget {
        // Get the GTK video sink and retrieve the video display widget from it
        let widget_value = self
            .sink
            .get_property("widget")
            .expect("Sink had no widget property");

        widget_value
            .get::<gtk::Widget>()
            .expect("Sink's widget propery was of the wrong type")
    }

    pub fn start(&self) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        // This has no effect if called multiple times
        self.pipeline.set_state(gst::State::Playing).into_result()
    }

    pub fn stop(&self) -> Result<gst::StateChangeSuccess, gst::StateChangeError> {
        // This has no effect if called multiple times
        self.pipeline.set_state(gst::State::Null).into_result()
    }

    // Take a snapshot of the current image and write it to the configured location
    pub fn take_snapshot(&self) -> Result<(), Box<dyn error::Error>> {
        use std::fs::File;

        let settings = utils::load_settings();

        // Create the GStreamer caps for the output format
        let (caps, extension) = match settings.snapshot_format {
            SnapshotFormat::JPEG => (gst::Caps::new_simple("image/jpeg", &[]), "jpg"),
            SnapshotFormat::PNG => (gst::Caps::new_simple("image/png", &[]), "png"),
        };

        let last_sample = self
            .sink
            .get_property("last-sample")
            .expect("Sink had no last-sample property");
        let last_sample = match last_sample.get::<gst::Sample>() {
            None => {
                // We have no sample to store yet
                return Ok(());
            }
            Some(sample) => sample,
        };

        // Create the filename and open the file writable
        let mut filename = settings.snapshot_directory.clone();
        let now = Local::now();
        filename.push(format!(
            "{}.{}",
            now.format("Snapshot %Y-%m-%d %H-%M-%S"),
            extension
        ));

        let mut file = match File::create(&filename) {
            Err(err) => {
                return Err(format!(
                    "Failed to create snapshot file {}: {}",
                    filename.display(),
                    err
                )
                .into());
            }
            Ok(file) => file,
        };

        // Then convert it from whatever format we got to PNG or JPEG as requested and write it out
        println!("Writing snapshot to {}", filename.display());
        gst_video::convert_sample_async(&last_sample, &caps, 5 * gst::SECOND, move |res| {
            use std::io::Write;

            let sample = match res {
                Err(err) => {
                    eprintln!("Failed to convert sample: {}", err);
                    return;
                }
                Ok(sample) => sample,
            };

            let buffer = sample.get_buffer().expect("Failed to get buffer");
            let map = buffer
                .map_readable()
                .expect("Failed to map buffer readable");

            if let Err(err) = file.write_all(&map) {
                eprintln!(
                    "Failed to write snapshot file {}: {}",
                    filename.display(),
                    err
                );
            }
        });

        Ok(())
    }

    // Start recording to the configured location
    pub fn start_recording(&self) -> Result<(), Box<dyn error::Error>> {
        let settings = utils::load_settings();

        let (bin_description, extension) = match settings.record_format {
            RecordFormat::H264Mp4 => ("queue ! videoconvert ! x264enc tune=zerolatency ! video/x-h264,profile=baseline ! mp4mux ! filesink name=sink", "mp4"),
            RecordFormat::Vp8WebM => ("queue ! videoconvert ! vp8enc deadline=1 ! webmmux ! filesink name=sink", "webm"),
        };

        let bin = match gst::parse_bin_from_description(bin_description, true) {
            Err(err) => {
                return Err(format!("Failed to create recording pipeline: {}", err).into());
            }
            Ok(bin) => bin,
        };

        // Get our file sink element by its name and set the location where to write the recording
        let sink = bin
            .get_by_name("sink")
            .expect("Recording bin has no sink element");
        let mut filename = settings.record_directory.clone();
        let now = Local::now();
        filename.push(format!(
            "{}.{}",
            now.format("Recording %Y-%m-%d %H-%M-%S"),
            extension
        ));

        // All strings in GStreamer are UTF8, we need to convert the path to UTF8 which in theory
        // can fail
        sink.set_property("location", &(filename.to_str().unwrap()))
            .expect("Filesink had no location property");

        // First try setting the recording bin to playing: if this fails we know this before it
        // potentially interferred with the other part of the pipeline
        if let Err(_) = bin.set_state(gst::State::Playing).into_result() {
            return Err("Failed to start recording".into());
        }

        // Add the bin to the pipeline. This would only fail if there was already a bin with the
        // same name, which we ensured can't happen
        self.pipeline
            .add(&bin)
            .expect("Failed to add recording bin");

        // Get our tee element by name, request a new source pad from it and then link that to our
        // recording bin to actually start receiving data
        let srcpad = self
            .tee
            .get_request_pad("src_%u")
            .expect("Failed to request new pad from tee");
        let sinkpad = bin
            .get_static_pad("sink")
            .expect("Failed to get sink pad from recording bin");

        // If linking fails, we just undo what we did above
        if let Err(err) = srcpad.link(&sinkpad).into_result() {
            // This might fail but we don't care anymore: we're in an error path
            let _ = self.pipeline.remove(&bin);
            let _ = bin.set_state(gst::State::Null);

            return Err(format!("Failed to link recording bin: {}", err)
                .as_str()
                .into());
        }

        *self.recording_bin.borrow_mut() = Some(bin);

        println!("Recording to {}", filename.display());

        Ok(())
    }

    // Stop recording if any recording was currently ongoing
    pub fn stop_recording(&self) {
        println!("stop recording");
    }

    // Here we handle all message we get from the GStreamer pipeline. These are notifications sent
    // from GStreamer, including errors that happend at runtime.
    //
    // This is always called from the main application thread by construction.
    fn on_pipeline_message(&self, msg: &gst::MessageRef) {
        use gst::MessageView;

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
                gio::Application::get_default().map(|app| app.quit());
            }
            _ => (),
        };
    }
}
