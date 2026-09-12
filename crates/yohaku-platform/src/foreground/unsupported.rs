//! 非 Windows 平台的空实现（仅保证 workspace 可编译）。

use super::{FocusSample, SharedSample};
use std::sync::mpsc::Sender;

pub struct ForegroundMonitor;

impl ForegroundMonitor {
    pub fn spawn(_tx: Sender<()>, _sample: SharedSample<FocusSample>) -> Self {
        ForegroundMonitor
    }
}
