//! 设备序列号：发送前先持久化 next（崩溃只会留下合法空洞，永不复用），
//! 服务端 acceptedSequence 单调 reconcile（macOS 版 CompanionPresenceSequencer 语义）。

use std::sync::{Arc, Mutex};

pub trait SequencePersistence: Send + Sync {
    fn load_next(&self, device_id: &str) -> Option<i64>;
    fn save_next(&self, device_id: &str, next: i64);
}

pub struct Sequencer {
    persistence: Arc<dyn SequencePersistence>,
    device_id: String,
    next: Mutex<i64>,
}

impl Sequencer {
    /// 种子 = max(pairing_next, persisted_next)：以较大者为准，
    /// 保证跨重装/重配对不复用任何已用过的序列号。
    pub fn new(
        persistence: Arc<dyn SequencePersistence>,
        device_id: &str,
        pairing_next: i64,
    ) -> Self {
        let persisted = persistence.load_next(device_id).unwrap_or(0);
        let seed = pairing_next.max(persisted);
        // 立即把种子落盘，使下次启动的 persisted ≥ seed
        persistence.save_next(device_id, seed);
        Sequencer {
            persistence,
            device_id: device_id.to_string(),
            next: Mutex::new(seed),
        }
    }

    /// 预留当前序列号并返回；返回前先把 next+1 持久化。
    pub fn reserve(&self) -> i64 {
        let mut next = self.next.lock().unwrap();
        let reserved = *next;
        *next += 1;
        self.persistence.save_next(&self.device_id, *next);
        reserved
    }

    /// 服务端确认 accepted：next 至少为 accepted+1（只前进，不后退）。
    pub fn reconcile(&self, accepted: i64) {
        let mut next = self.next.lock().unwrap();
        let target = accepted.saturating_add(1);
        if target > *next {
            *next = target;
            self.persistence.save_next(&self.device_id, *next);
        }
    }

    pub fn peek(&self) -> i64 {
        *self.next.lock().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex as StdMutex;

    struct MemoryPersistence(StdMutex<BTreeMap<String, i64>>);

    impl SequencePersistence for MemoryPersistence {
        fn load_next(&self, device_id: &str) -> Option<i64> {
            self.0.lock().unwrap().get(device_id).copied()
        }
        fn save_next(&self, device_id: &str, next: i64) {
            self.0.lock().unwrap().insert(device_id.to_string(), next);
        }
    }

    fn persistence() -> Arc<MemoryPersistence> {
        Arc::new(MemoryPersistence(StdMutex::new(BTreeMap::new())))
    }

    #[test]
    fn reserve_is_sequential_and_persisted_first() {
        let p = persistence();
        let seq = Sequencer::new(p.clone(), "device-a", 5);
        assert_eq!(seq.reserve(), 5);
        assert_eq!(seq.reserve(), 6);
        // 崩溃场景：进程消失后持久层已含 next=7，重建后从 7 继续（空洞合法）
        assert_eq!(p.0.lock().unwrap()["device-a"], 7);
        let resumed = Sequencer::new(p.clone(), "device-a", 0);
        assert_eq!(resumed.reserve(), 7);
    }

    #[test]
    fn seed_takes_max_of_pairing_and_persisted() {
        let p = persistence();
        p.save_next("d", 100);
        let seq = Sequencer::new(p, "d", 50);
        assert_eq!(seq.reserve(), 100);
    }

    #[test]
    fn reconcile_only_moves_forward() {
        let p = persistence();
        let seq = Sequencer::new(p.clone(), "d", 0);
        seq.reserve(); // 0 → next=1
        seq.reconcile(9);
        assert_eq!(seq.peek(), 10);
        assert_eq!(p.0.lock().unwrap()["d"], 10);
        seq.reconcile(3); // 过期确认不得回退
        assert_eq!(seq.peek(), 10);
    }
}
