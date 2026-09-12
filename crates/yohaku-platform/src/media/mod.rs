//! 系统媒体会话监控（Windows: GSMTC / System Media Transport Controls）。

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::MediaMonitor;

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use unsupported::MediaMonitor;

use chrono::{DateTime, Utc};

/// 媒体会话样本。位置字段为「采样时刻」的值；
/// 外推请用 [`MediaSample::current_position`]。
#[derive(Debug, Clone, PartialEq)]
pub struct MediaSample {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// 播放器标识（SourceAppUserModelId 小写）
    pub player_key: String,
    pub player_display_name: String,
    pub playing: bool,
    pub duration_seconds: Option<f64>,
    /// 采样时刻的位置（外推前的基准值）
    pub position_seconds: Option<f64>,
    /// 位置外推速率（秒/秒）
    pub position_rate: f64,
    pub position_updated_at: DateTime<Utc>,
    /// 原始封面字节（GSMTC Thumbnail，可能缺失）
    pub artwork_bytes: Option<Vec<u8>>,
}

impl MediaSample {
    /// 当前时刻的位置外推（playing 且有基准位置时按速率推进，钳到时长）。
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
