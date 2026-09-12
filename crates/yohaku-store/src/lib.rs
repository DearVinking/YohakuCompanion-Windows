#![forbid(unsafe_code)]

pub mod connection;
pub mod error;
pub mod history;
pub mod json_io;
pub mod paths;
pub mod privacy;
pub mod settings;

pub use connection::{
    ConnectionMetadata, ConnectionStore, S3_ACCESS_KEY, S3_SECRET_KEY, SecretStore,
};
pub use error::{StoreError, StoreResult};
