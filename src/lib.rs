pub mod config;
#[cfg(any(windows, test))]
mod releases;

pub const BUILD_VERSION: &str = match option_env!("VOLUME3_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[cfg(windows)]
pub mod app;
#[cfg(windows)]
pub mod audio;
#[cfg(windows)]
pub mod panel;
#[cfg(windows)]
pub mod settings;
#[cfg(windows)]
pub mod startup;
#[cfg(windows)]
pub mod ui;
#[cfg(windows)]
mod updates;
