use gio::{self, prelude::*};
use glib;
use gst::{self, prelude::*, BinExt};
use gtk;

use std::error;
use std::ops;
use std::rc::{Rc, Weak};

use fragile;

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
    sink: gst::Element,
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
        let pipeline =
            gst::parse_launch("autovideosrc ! queue ! videoconvert ! gtksink name=sink")?;

        // Upcast to a gst::Pipeline as the above function could've also returned an arbitrary
        // gst::Element if a different string was passed
        let pipeline = pipeline
            .downcast::<gst::Pipeline>()
            .expect("Couldn't downcast pipeline");

        // Retrieve sink element from the pipeline for later use
        let sink = pipeline.get_by_name("sink").expect("No sink found");

        let pipeline = Pipeline(Rc::new(PipelineInner { pipeline, sink }));

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
