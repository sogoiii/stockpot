//! Configuration management.

mod settings;
mod xdg;

pub use settings::{PdfMode, Settings, SettingsError};
pub use xdg::XdgDirs;
