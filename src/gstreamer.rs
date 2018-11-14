use App;
use RecordFormat;
use Settings;
use SnapshotFormat;

use utils::{load_settings, show_error_dialog};

use chrono::prelude::*;

use gtk;
use gtk::prelude::*;

use gst;
use gst::prelude::*;
use gst::BinExt;
use gst::MessageView;
use gst_video;

use std::error;
use std::fs::File;

impl App {
    // Here we handle all message we get from the GStreamer pipeline. These are
    // notifications sent from GStreamer, including errors that happend at
    // runtime.
    pub fn on_pipeline_message(&self, msg: &gst::MessageRef) {
        // A message can contain various kinds of information but
        // here we are only interested in errors so far
        match msg.view() {
            MessageView::Error(err) => {
                show_error_dialog(
                    self.0.borrow().main_window.as_ref(),
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
                    show_error_dialog(self.0.borrow().main_window.as_ref(), false, text);
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
                            let inner = self.0.borrow();

                            // Get our pipeline and the recording bin
                            let pipeline = match inner.pipeline {
                                Some(ref pipeline) => pipeline.clone(),
                                None => return,
                            };
                            let bin = match msg
                                .get_src()
                                .and_then(|src| src.clone().downcast::<gst::Element>().ok())
                            {
                                Some(src) => src,
                                None => return,
                            };

                            // And then asynchronously remove it and set its state to Null
                            pipeline.call_async(move |pipeline| {
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

    pub fn create_pipeline(&self) -> Result<(gst::Pipeline, gtk::Widget), Box<dyn error::Error>> {
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

        // Install a message handler on the pipeline's bus to catch errors
        let bus = pipeline.get_bus().expect("Pipeline had no bus");

        // GStreamer is thread-safe and it is possible to attach
        // bus watches from any thread, which are then nonetheless
        // called from the main thread. As such we have to make use
        // of fragile::Fragile() here to be able to pass our non-Send
        // application struct into a closure that requires Send.
        //
        // As we are on the main thread and the closure will be called
        // on the main thread, this will not cause a panic and is perfectly
        // safe.
        let app_weak = fragile::Fragile::new(self.downgrade());
        bus.add_watch(move |_bus, msg| {
            let app_weak = app_weak.get();
            let app = upgrade_weak!(app_weak, glib::Continue(false));

            app.on_pipeline_message(msg);

            glib::Continue(true)
        });

        // Get the GTK video sink and retrieve the video display widget from it
        let sink = pipeline
            .get_by_name("sink")
            .expect("Pipeline had no sink element");
        let widget_value = sink
            .get_property("widget")
            .expect("Sink had no widget property");
        let widget = widget_value
            .get::<gtk::Widget>()
            .expect("Sink's widget propery was of the wrong type");

        Ok((pipeline, widget))
    }

    pub fn start_recording(&self, record_button: &gtk::ToggleButton, settings: Settings) {
        // If we have no pipeline (can't really happen) just return
        let pipeline = match self.0.borrow().pipeline {
            Some(ref pipeline) => pipeline.clone(),
            None => return,
        };

        // If we already have a record-bin (i.e. we still finish the previous one)
        // just return for now and deactivate the button again
        if pipeline.get_by_name("record-bin").is_some() {
            record_button.set_state_flags(
                record_button.get_state_flags() & !gtk::StateFlags::CHECKED,
                true,
            );
            return;
        }

        let (bin_description, extension) = match settings.record_format {
            RecordFormat::H264Mp4 => ("name=record-bin queue ! videoconvert ! x264enc ! video/x-h264,profile=baseline ! mp4mux ! filesink name=sink", "mp4"),
            RecordFormat::Vp8WebM => ("name=record-bin queue ! videoconvert ! vp8enc ! webmmux ! filesink name=sink", "webm"),
        };

        let bin = match gst::parse_bin_from_description(bin_description, true) {
            Err(err) => {
                show_error_dialog(
                    self.0.borrow().main_window.as_ref(),
                    false,
                    format!("Failed to create recording pipeline: {}", err).as_str(),
                );
                return;
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
            now.format("Recording %Y-%m-%d %H:%M:%S"),
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
            show_error_dialog(
                self.0.borrow().main_window.as_ref(),
                false,
                "Failed to start recording",
            );
            return;
        }

        // Add the bin to the pipeline. This would only fail if there was already
        // a bin with the same name, which we ensured can't happen
        pipeline.add(&bin).expect("Failed to add recording bin");

        // Get our tee element by name, request a new source pad from it and
        // then link that to our recording bin to actually start receiving data
        let tee = pipeline
            .get_by_name("tee")
            .expect("Pipeline had no tee element");
        let srcpad = tee
            .get_request_pad("src_%u")
            .expect("Failed to request new pad from tee");
        let sinkpad = bin
            .get_static_pad("sink")
            .expect("Failed to get sink pad from recording bin");

        // If linking fails, we just undo what we did above
        if let Err(err) = srcpad.link(&sinkpad).into_result() {
            show_error_dialog(
                self.0.borrow().main_window.as_ref(),
                false,
                format!("Failed to link recording bin: {}", err).as_str(),
            );
            // This might fail but we don't care anymore: we're in an error path
            let _ = pipeline.remove(&bin);
            let _ = bin.set_state(gst::State::Null);
        }

        println!("Recording to {}", filename.display());
    }

    pub fn stop_recording(&self) {
        // If we have no pipeline (can't really happen) just return
        let pipeline = match self.0.borrow().pipeline {
            Some(ref pipeline) => pipeline.clone(),
            None => return,
        };

        // Get our recording bin, if it does not exist then nothing
        // has to be stopped actually. This shouldn't really happen
        let bin = pipeline
            .get_by_name("record-bin")
            .expect("Pipeline had no recording bin");

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
            bin.call_async(move |_| {
                sinkpad.send_event(gst::Event::new_eos().build());
            });

            // Don't block the pad but remove the probe to let everything
            // continue as normal
            gst::PadProbeReturn::Remove
        });
    }

    // Take a snapshot of the current image and write it to the configured location
    pub fn take_snapshot(&self) {
        let settings = load_settings();

        // If we have no pipeline there's nothing to snapshot
        let pipeline = match self.0.borrow().pipeline {
            None => return,
            Some(ref pipeline) => pipeline.clone(),
        };

        // Create the GStreamer caps for the output format
        let (caps, extension) = match settings.snapshot_format {
            SnapshotFormat::JPEG => (gst::Caps::new_simple("image/jpeg", &[]), "jpg"),
            SnapshotFormat::PNG => (gst::Caps::new_simple("image/png", &[]), "png"),
        };

        let sink = pipeline.get_by_name("sink").expect("sink not found");
        let last_sample = sink
            .get_property("last-sample")
            .expect("Sink had no last-sample property");
        let last_sample = match last_sample.get::<gst::Sample>() {
            None => {
                // We have no sample to store yet
                return;
            }
            Some(sample) => sample,
        };

        // Create the filename and open the file writable
        let mut filename = settings.snapshot_directory.clone();
        let now = Local::now();
        filename.push(format!(
            "{}.{}",
            now.format("Snapshot %Y-%m-%d %H:%M:%S"),
            extension
        ));

        let mut file = match File::create(&filename) {
            Err(err) => {
                show_error_dialog(
                    self.0.borrow().main_window.as_ref(),
                    false,
                    format!(
                        "Failed to create snapshot file {}: {}",
                        filename.display(),
                        err
                    )
                    .as_str(),
                );
                return;
            }
            Ok(file) => file,
        };

        // Then convert it from whatever format we got to PNG or JPEG as requested
        // and write it out
        println!("Writing snapshot to {}", filename.display());
        let bus = pipeline.get_bus().expect("Pipeline has no bus");
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
    }
}
