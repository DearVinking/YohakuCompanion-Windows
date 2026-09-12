//! 受保护凭据存储（Windows: DPAPI）。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::DpapiSecretStore;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::DpapiSecretStore;
