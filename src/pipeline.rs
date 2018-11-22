use settings::RecordFormat;
use settings::SnapshotFormat;

use utils::load_settings;

use chrono::prelude::*;

use gst::{self, prelude::*, BinExt};
use gst_video;

use std::cell::RefCell;
use std::error;
use std::fs::File;
use std::ops;
use std::rc::{Rc, Weak};

#[derive(Clone)]
pub struct Pipeline(Rc<PipelineInner>);

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

pub struct PipelineWeak(Weak<PipelineInner>);
impl PipelineWeak {
    pub fn upgrade(&self) -> Option<Pipeline> {
        self.0.upgrade().map(Pipeline)
    }
}

impl Pipeline {
    pub fn new() -> Result<Self, Box<dyn error::Error>> {
        // Create a new GStreamer pipeline that captures from the default video source,
        // which is usually a camera, converts the output to RGB if needed and then passes
        // it to a GTK video sink
        let pipeline = gst::parse_launch(
            "autovideosrc ! tee name=tee ! queue ! videoconvert ! gtksink name=sink",
        )?;

        // Upcast to a gst::Pipeline as the above function could've also returned
        // an arbitrary gst::Element if a different string was passed
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Couldn't downcast pipeline");

        // Request that the pipeline forwards us all messages, even those that it would otherwise
        // aggregate first
        pipeline.set_property_message_forward(true);

        let tee = pipeline.get_by_name("tee").expect("No tee found");
        let sink = pipeline.get_by_name("sink").expect("No sink found");

        let pipeline = Pipeline(Rc::new(PipelineInner {
            pipeline,
            tee,
            sink,
            recording_bin: RefCell::new(None),
        }));

        // Install a message handler on the pipeline's bus to catch errors
        let bus = pipeline.pipeline.get_bus().expect("Pipeline had no bus");

        // GStreamer is thread-safe and it is possible to attach
        // bus watches from any thread, which are then nonetheless
        // called from the main thread. As such we have to make use
        // of fragile::Fragile() here to be able to pass our non-Send
        // application struct into a closure that requires Send.
        //
        // As we are on the main thread and the closure will be called
        // on the main thread, this will not cause a panic and is perfectly
        // safe.
        let pipeline_weak = fragile::Fragile::new(pipeline.downgrade());
        bus.add_watch(move |_bus, msg| {
            let pipeline_weak = pipeline_weak.get();
            let pipeline = upgrade_weak!(pipeline_weak, glib::Continue(false));

            pipeline.on_pipeline_message(msg);

            glib::Continue(true)
        });

        Ok(pipeline)
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
        // TODO: If a recording is currently running we should stop that first
        self.pipeline.set_state(gst::State::Null).into_result()
    }

    pub fn start_recording(&self) -> Result<(), Box<dyn error::Error>> {
        let settings = load_settings();

        let (bin_description, extension) = match settings.record_format {
            RecordFormat::H264Mp4 => ("queue ! videoconvert ! x264enc ! video/x-h264,profile=baseline ! mp4mux ! filesink name=sink", "mp4"),
            RecordFormat::Vp8WebM => ("queue ! videoconvert ! vp8enc ! webmmux ! filesink name=sink", "webm"),
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

        // All strings in GStreamer are UTF8, we need to convert the path to UTF8
        // which in theory can fail
        sink.set_property("location", &(filename.to_str().unwrap()))
            .expect("Filesink had no location property");

        // First try setting the recording bin to playing: if this fails
        // we know this before it potentially interferred with the other
        // part of the pipeline
        if let Err(_) = bin.set_state(gst::State::Playing).into_result() {
            return Err("Failed to start recording".into());
        }

        // Add the bin to the pipeline. This would only fail if there was already
        // a bin with the same name, which we ensured can't happen
        self.pipeline
            .add(&bin)
            .expect("Failed to add recording bin");

        // Get our tee element by name, request a new source pad from it and
        // then link that to our recording bin to actually start receiving data
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

    pub fn stop_recording(&self) {
        // Get our recording bin, if it does not exist then nothing
        // has to be stopped actually. This shouldn't really happen
        let bin = match self.recording_bin.borrow_mut().take() {
            None => return,
            Some(bin) => bin,
        };

        // Get the source pad of the tee that is connected to the recording bin
        let sinkpad = bin
            .get_static_pad("sink")
            .expect("Failed to get sink pad from recording bin");
        let srcpad = match sinkpad.get_peer() {
            Some(peer) => peer,
            None => return,
        };

        println!("Stopping recording");

        // Once the tee source pad is idle and we wouldn't interfere with
        // any data flow, unlink the tee and the recording bin and finalize
        // the recording bin by sending it an end-of-stream event
        //
        // Once the end-of-stream event is handled by the whole recording bin,
        // we get an end-of-stream message from it in the message handler and
        // the shut down the recording bin and remove it from the pipeline
        //
        // The closure below might be called directly from the main UI thread
        // here or at a later time from a GStreamer streaming thread
        srcpad.add_probe(gst::PadProbeType::IDLE, move |srcpad, _| {
            // Get the parent of the tee source pad, i.e. the tee itself
            let tee = srcpad
                .get_parent()
                .and_then(|parent| parent.downcast::<gst::Element>().ok())
                .expect("Failed to get tee source pad parent");

            // Unlink the tee source pad and then release it
            //
            // If unlinking fails we don't care, just make sure that the
            // pad is actually released
            let _ = srcpad.unlink(&sinkpad);
            tee.release_request_pad(srcpad);

            // Asynchronously send the end-of-stream event to the sinkpad as
            // this might block for a while and our closure here
            // might've been called from the main UI thread
            let sinkpad = sinkpad.clone();
            async!(bin => |_| {
                sinkpad.send_event(gst::Event::new_eos().build());
            });

            // Don't block the pad but remove the probe to let everything
            // continue as normal
            gst::PadProbeReturn::Remove
        });
    }

    // Take a snapshot of the current image and write it to the configured location
    pub fn take_snapshot(&self) -> Result<(), Box<dyn error::Error>> {
        let settings = load_settings();

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

        // Then convert it from whatever format we got to PNG or JPEG as requested
        // and write it out
        println!("Writing snapshot to {}", filename.display());
        let bus = self.pipeline.get_bus().expect("Pipeline has no bus");
        gst_video::convert_sample_async(&last_sample, &caps, 5 * gst::SECOND, move |res| {
            use std::io::Write;

            let sample = match res {
                Err(err) => {
                    let _ = bus.post(
                        &gst::Message::new_application(
                            gst::Structure::builder("warning")
                                .field("text", &format!("Failed to convert sample: {}", err))
                                .build(),
                        )
                        .build(),
                    );
                    return;
                }
                Ok(sample) => sample,
            };

            let buffer = sample.get_buffer().expect("Failed to get buffer");
            let map = buffer
                .map_readable()
                .expect("Failed to map buffer readable");

            if let Err(err) = file.write_all(&map) {
                let _ = bus.post(
                    &gst::Message::new_application(
                        gst::Structure::builder("warning")
                            .field(
                                "text",
                                &format!(
                                    "Failed to write snapshot file {}: {}",
                                    filename.display(),
                                    err
                                ),
                            )
                            .build(),
                    )
                    .build(),
                );
            }
        });

        Ok(())
    }

    // Here we handle all message we get from the GStreamer pipeline. These are
    // notifications sent from GStreamer, including errors that happend at
    // runtime. These are all handled from the application thread and show
    // dialogs as needed or shut down the application
    fn on_pipeline_message(&self, msg: &gst::MessageRef) {
        use gst::MessageView;
        use utils::show_error_dialog;

        // A message can contain various kinds of information but
        // here we are only interested in errors so far
        match msg.view() {
            MessageView::Error(err) => {
                show_error_dialog(
                    true,
                    format!(
                        "Error from {:?}: {} ({:?})",
                        err.get_src().map(|s| s.get_path_string()),
                        err.get_error(),
                        err.get_debug()
                    )
                    .as_str(),
                );
            }
            MessageView::Application(msg) => match msg.get_structure() {
                // Here we can send ourselves warning messages from any thread and show them
                // to the user in the UI in case something goes wrong
                Some(s) if s.get_name() == "warning" => {
                    let text = s.get::<&str>("text").expect("Warning message without text");
                    show_error_dialog(false, text);
                }
                _ => (),
            },
            MessageView::Element(msg) => {
                // Catch the end-of-stream messages from our filesink. Because the other sink,
                // gtksink, will never receive end-of-stream we will never get a normal
                // end-of-stream message from the bus.
                //
                // The normal end-of-stream message would only be sent once *all*
                // sinks had their end-of-stream message posted.
                match msg.get_structure() {
                    Some(s) if s.get_name() == "GstBinForwarded" => {
                        // The forwarded, original message from the bin is stored in the
                        // message field of its structure
                        let msg = s
                            .get::<gst::Message>("message")
                            .expect("Failed to get forwarded message");

                        if let MessageView::Eos(..) = msg.view() {
                            let bin = match msg
                                .get_src()
                                .and_then(|src| src.clone().downcast::<gst::Element>().ok())
                            {
                                Some(src) => src,
                                None => return,
                            };

                            // And then asynchronously remove it and set its state to Null
                            let pipeline = &self.pipeline;
                            async!(pipeline => |pipeline| {
                                // Ignore if the bin was not in the pipeline anymore for whatever
                                // reason. It's not a problem
                                let _ = pipeline.remove(&bin);

                                if let Err(err) = bin.set_state(gst::State::Null).into_result() {
                                    let bus = pipeline.get_bus().expect("Pipeline has no bus");
                                    let _ = bus.post(
                                        &gst::Message::new_application(
                                            gst::Structure::builder("warning")
                                                .field(
                                                    "text",
                                                    &format!("Failed to stop recording: {}", err),
                                                )
                                                .build(),
                                        )
                                        .build(),
                                    );
                                }
                            });
                        }
                    }
                    _ => (),
                }
            }
            _ => (),
        };
    }

    pub fn downgrade(&self) -> PipelineWeak {
        PipelineWeak(Rc::downgrade(&self.0))
    }
}
