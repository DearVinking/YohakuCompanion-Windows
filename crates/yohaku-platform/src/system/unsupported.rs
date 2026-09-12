//! 非 Windows 平台的空实现。

use super::SystemEvent;
use std::sync::mpsc::Sender;

pub struct SystemEvents;

impl SystemEvents {
    pub fn spawn(_tx: Sender<SystemEvent>) -> Self {
        SystemEvents
    }
}
