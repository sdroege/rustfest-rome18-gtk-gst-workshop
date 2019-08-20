// Macro for upgrading a weak reference or returning the given value
//
// This works for glib/gtk objects as well as anything else providing an upgrade method
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

// Macro for asynchronously calling some code with an element
//
// When using GStreamer >= 1.10 this will make use of a thread-pool
macro_rules! call_async {
    ($x:ident => |$($p:tt),*| $body:expr) => {{
        #[cfg(feature = "v1_10")]
        {
            $x.call_async(move |$($p),*| {
                $body
            });
        }
        #[cfg(not(feature = "v1_10"))]
        {
            #[allow(unused_variables)]
            let $x = $x.clone();
            ::std::thread::spawn(move || {
                $body
            });
        }
    }}
}
