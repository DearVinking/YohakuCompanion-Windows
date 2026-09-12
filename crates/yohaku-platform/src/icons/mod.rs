//! 应用图标提取（exe → PNG）。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::extract_png;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::extract_png;
