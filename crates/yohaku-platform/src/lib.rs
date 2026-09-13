#![cfg_attr(windows, deny(clippy::undocumented_unsafe_blocks))]

pub mod credentials;
pub mod foreground;
pub mod logger;
pub mod media;
pub mod system;

#[cfg(windows)]
mod message_loop;

#[cfg(windows)]
mod com;
