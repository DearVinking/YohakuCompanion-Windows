#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::SystemEvents;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::SystemEvents;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemEvent {
    SleepOrLock,
    Wake,
    NetworkUp,
}
