#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::MediaMonitor;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::MediaMonitor;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, PartialEq)]
pub struct MediaSample {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player_key: String,
    pub player_display_name: String,
    pub playing: bool,
    pub duration_seconds: Option<f64>,
    pub position_seconds: Option<f64>,
    pub position_rate: f64,
    pub position_updated_at: DateTime<Utc>,
}

impl MediaSample {
    pub fn current_position(&self, now: DateTime<Utc>) -> Option<f64> {
        let base = self.position_seconds?;
        if !self.playing || self.position_rate <= 0.0 {
            return Some(base);
        }
        let elapsed = (now - self.position_updated_at).num_milliseconds() as f64 / 1000.0;
        let mut position = base + elapsed * self.position_rate;
        if let Some(duration) = self.duration_seconds {
            position = position.min(duration);
        }
        Some(position)
    }
}
