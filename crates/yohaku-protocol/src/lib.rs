#![forbid(unsafe_code)]

pub mod capabilities;
pub mod error;
pub mod ids;
pub mod json;
pub mod limits;
pub mod negotiator;
pub mod pairing;
pub mod sequencer;
pub mod presence;
pub mod semver;
pub mod time;

pub const CLIENT_VERSION: &str = "1.7.3";
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
