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
    pub fn new(
        persistence: Arc<dyn SequencePersistence>,
        device_id: &str,
        pairing_next: i64,
    ) -> Self {
        let persisted = persistence.load_next(device_id).unwrap_or(0);
        let seed = pairing_next.max(persisted);
        persistence.save_next(device_id, seed);
        Sequencer {
            persistence,
            device_id: device_id.to_string(),
            next: Mutex::new(seed),
        }
    }

    pub fn reserve(&self) -> i64 {
        let mut next = self.next.lock().unwrap();
        let reserved = *next;
        *next += 1;
        self.persistence.save_next(&self.device_id, *next);
        reserved
    }

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
        seq.reserve();
        seq.reconcile(9);
        assert_eq!(seq.peek(), 10);
        assert_eq!(p.0.lock().unwrap()["d"], 10);
        seq.reconcile(3);
        assert_eq!(seq.peek(), 10);
    }
}
