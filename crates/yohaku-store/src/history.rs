//! 同步历史：有界投递审计（上限裁剪最旧），只存安全投影——
//! 触发原因、目的地状态、固定错误码、成功时的最终渲染摘要。
//! 不含原始捕获、凭据、端点、请求体与响应体。

use crate::error::StoreResult;
use crate::json_io::{read_json_opt, write_json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const HISTORY_FILE: &str = "history.json";
pub const HISTORY_CAP: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncTrigger {
    SemanticChange,
    Heartbeat,
    Lifecycle,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    Succeeded,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncEvent {
    pub id: String,
    pub destination: String,
    pub trigger: SyncTrigger,
    pub state: SyncState,
    pub error_code: Option<String>,
    /// 仅 Succeeded 存在；来自最终渲染的安全摘要
    pub output_summary: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

pub struct HistoryStore {
    cap: usize,
}

impl Default for HistoryStore {
    fn default() -> Self {
        HistoryStore { cap: HISTORY_CAP }
    }
}

impl HistoryStore {
    pub fn new(cap: usize) -> Self {
        HistoryStore { cap }
    }

    pub fn list(&self, dir: &Path) -> StoreResult<Vec<SyncEvent>> {
        Ok(read_json_opt::<Vec<SyncEvent>>(&dir.join(HISTORY_FILE))?.unwrap_or_default())
    }

    /// 追加并按上限裁剪最旧。failed/skipped 不允许携带 output_summary。
    pub fn append(&self, dir: &Path, mut event: SyncEvent) -> StoreResult<()> {
        if event.state != SyncState::Succeeded {
            event.output_summary = None;
        }
        let mut events = self.list(dir)?;
        events.push(event);
        if events.len() > self.cap {
            let drop_count = events.len() - self.cap;
            events.drain(0..drop_count);
        }
        write_json(&dir.join(HISTORY_FILE), &events)
    }

    pub fn clear(&self, dir: &Path) -> StoreResult<()> {
        write_json(&dir.join(HISTORY_FILE), &Vec::<SyncEvent>::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn event(id: &str, state: SyncState) -> SyncEvent {
        SyncEvent {
            id: id.into(),
            destination: "liveDesk".into(),
            trigger: SyncTrigger::SemanticChange,
            state,
            error_code: (state == SyncState::Failed).then(|| "SERVER".into()),
            output_summary: Some("summary".into()),
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 1).unwrap(),
        }
    }

    #[test]
    fn append_caps_oldest_first() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::new(3);
        for i in 0..5 {
            store.append(dir.path(), event(&i.to_string(), SyncState::Succeeded)).unwrap();
        }
        let events = store.list(dir.path()).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, "2"); // 最旧两个被裁剪
        assert_eq!(events[2].id, "4");
    }

    #[test]
    fn failed_events_have_no_summary() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::default();
        store.append(dir.path(), event("f", SyncState::Failed)).unwrap();
        let events = store.list(dir.path()).unwrap();
        assert_eq!(events[0].output_summary, None);
        assert_eq!(events[0].error_code.as_deref(), Some("SERVER"));
    }

    #[test]
    fn clear_empties() {
        let dir = tempfile::tempdir().unwrap();
        let store = HistoryStore::default();
        store.append(dir.path(), event("a", SyncState::Skipped)).unwrap();
        store.clear(dir.path()).unwrap();
        assert!(store.list(dir.path()).unwrap().is_empty());
    }
}
