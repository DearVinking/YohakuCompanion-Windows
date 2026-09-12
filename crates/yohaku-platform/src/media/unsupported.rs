//! 非 Windows 平台的空实现。

use super::MediaSample;
use std::sync::mpsc::Sender;

pub struct MediaMonitor;

impl MediaMonitor {
    pub fn spawn(_notify: Sender<()>) -> Result<Self, String> {
        Err("media monitoring requires Windows".into())
    }

    pub fn current_media(&self) -> Option<MediaSample> {
        None
    }

    pub fn set_preferred_players(&self, _keys: Vec<String>) {}
}
