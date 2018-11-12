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
