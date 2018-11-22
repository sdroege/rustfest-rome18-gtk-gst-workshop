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

macro_rules! save_settings {
    ($x:ident, $call:ident, $($to_downgrade:ident),* => move |$($p:tt),*| $body:expr) => {{
        $( let $to_downgrade = $to_downgrade.downgrade(); )*
        $x.$call(move |$($p),*| {
            $( let $to_downgrade = upgrade_weak!($to_downgrade, ()); )*
            $body
        });
    }}
}

macro_rules! async {
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
