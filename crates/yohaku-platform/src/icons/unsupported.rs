//! 非 Windows 平台的空实现。

pub fn extract_png(_exe_path: &str) -> Option<Vec<u8>> {
    None
}
