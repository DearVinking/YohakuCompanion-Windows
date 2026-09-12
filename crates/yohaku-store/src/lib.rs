#![forbid(unsafe_code)]

pub mod connection;
pub mod error;
pub mod history;
pub mod json_io;
pub mod paths;
pub mod privacy;
pub mod settings;

pub use error::{StoreError, StoreResult};
