#[cfg(feature = "log")]
macro_rules! error {
    ($($arg:tt)*) => {
        bevy_log::error!($($arg)*);
    };
}

#[cfg(feature = "log")]
macro_rules! info {
    ($($arg:tt)*) => {
        bevy_log::info!($($arg)*);
    };
}

#[cfg(not(feature = "log"))]
macro_rules! error {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}

#[cfg(not(feature = "log"))]
macro_rules! info {
    ($($arg:tt)*) => {
        let _ = format_args!($($arg)*);
    };
}

pub(crate) use {error, info};
