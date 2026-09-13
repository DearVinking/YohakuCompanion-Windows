use super::{FocusSample, SharedSample};
use std::sync::mpsc::Sender;

pub struct ForegroundMonitor;

impl ForegroundMonitor {
    pub fn spawn(_tx: Sender<()>, _sample: SharedSample<FocusSample>) -> Self {
        ForegroundMonitor
    }
}
