#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::ForegroundMonitor;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::ForegroundMonitor;

use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSample {
    pub application_key: String,
    pub display_name: String,
    pub window_title: Option<String>,
}

pub type SharedSample<T> = Arc<std::sync::Mutex<Option<T>>>;

pub fn application_key_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

pub fn fallback_display_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_key_normalization() {
        assert_eq!(
            application_key_from_path("C:\\Apps\\Microsoft EDGE.EXE"),
            "microsoft edge.exe"
        );
        assert_eq!(application_key_from_path("/usr/bin/Firefox"), "firefox");
        assert_eq!(application_key_from_path(""), "");
    }

    #[test]
    fn display_fallback_strips_extension() {
        assert_eq!(fallback_display_name("C:\\x\\msedge.exe"), "msedge");
        assert_eq!(fallback_display_name("editor"), "editor");
    }
}
