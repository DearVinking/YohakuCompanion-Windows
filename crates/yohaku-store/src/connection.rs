use crate::error::{StoreError, StoreResult};
use crate::json_io::{read_json_opt, write_json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

pub const CONNECTION_SCHEMA_VERSION: u32 = 1;
pub const CONNECTION_FILE: &str = "connection.json";

pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &[u8]) -> StoreResult<()>;
    fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>>;
    fn remove(&self, key: &str) -> StoreResult<()>;
}

pub const DEVICE_TOKEN_SECRET_KEY: &str = "device-token";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    pub schema_version: u32,
    pub base_url: String,
    pub device_id: String,
    pub scopes: Vec<String>,
    pub pairing_next_sequence: i64,
    pub next_sequence: Option<i64>,
    pub is_live_desk_enabled: bool,
    pub paired_at: Option<DateTime<Utc>>,
}

pub struct ConnectionStore {
    dir: std::path::PathBuf,
    secrets: Arc<dyn SecretStore>,
    transaction_lock: Mutex<()>,
}

impl ConnectionStore {
    pub fn new(dir: &Path, secrets: Arc<dyn SecretStore>) -> Self {
        ConnectionStore {
            dir: dir.to_path_buf(),
            secrets,
            transaction_lock: Mutex::new(()),
        }
    }

    pub fn load_metadata(&self) -> StoreResult<Option<ConnectionMetadata>> {
        let _guard = self.transaction_lock.lock().unwrap();
        self.load_metadata_unlocked()
    }

    fn load_metadata_unlocked(&self) -> StoreResult<Option<ConnectionMetadata>> {
        read_json_opt(&self.dir.join(CONNECTION_FILE))
    }

    fn save_metadata(&self, metadata: &ConnectionMetadata) -> StoreResult<()> {
        write_json(&self.dir.join(CONNECTION_FILE), metadata)
    }

    fn remove_metadata(&self) -> StoreResult<()> {
        match std::fs::remove_file(self.dir.join(CONNECTION_FILE)) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result.map_err(StoreError::from),
        }
    }

    pub fn install_pairing_claim(
        &self,
        device_id: &str,
        device_token: &str,
        scopes: &[String],
        pairing_next_sequence: i64,
        base_url: &str,
    ) -> StoreResult<ConnectionMetadata> {
        let _guard = self.transaction_lock.lock().unwrap();
        let previous_metadata = match self.load_metadata_unlocked() {
            Err(StoreError::Json(_)) => None,
            result => result?,
        };
        let previous_token = self.secrets.get(DEVICE_TOKEN_SECRET_KEY)?;
        self.remove_metadata()?;
        let metadata = ConnectionMetadata {
            schema_version: CONNECTION_SCHEMA_VERSION,
            base_url: base_url.to_string(),
            device_id: device_id.to_string(),
            scopes: scopes.to_vec(),
            pairing_next_sequence,
            next_sequence: Some(pairing_next_sequence),
            is_live_desk_enabled: false,
            paired_at: Some(Utc::now()),
        };
        let commit = self
            .secrets
            .set(DEVICE_TOKEN_SECRET_KEY, device_token.as_bytes())
            .and_then(|()| self.save_metadata(&metadata));
        if let Err(error) = commit {
            let restore = match previous_token {
                Some(token) => self.secrets.set(DEVICE_TOKEN_SECRET_KEY, &token),
                None => self.secrets.remove(DEVICE_TOKEN_SECRET_KEY),
            };
            if let Err(restore_error) = restore {
                let _ = self.secrets.remove(DEVICE_TOKEN_SECRET_KEY);
                return Err(StoreError::Other(format!(
                    "{error}; credential rollback failed: {restore_error}"
                )));
            }
            if let Some(previous) = previous_metadata {
                self.save_metadata(&previous)?;
            }
            return Err(error);
        }
        Ok(metadata)
    }

    pub fn set_device_token(&self, token: &str) -> StoreResult<()> {
        let _guard = self.transaction_lock.lock().unwrap();
        self.secrets.set(DEVICE_TOKEN_SECRET_KEY, token.as_bytes())
    }

    pub fn remove_device_token(&self) -> StoreResult<()> {
        let _guard = self.transaction_lock.lock().unwrap();
        self.secrets.remove(DEVICE_TOKEN_SECRET_KEY)
    }

    pub fn device_token(&self) -> StoreResult<Option<String>> {
        let _guard = self.transaction_lock.lock().unwrap();
        self.device_token_unlocked()
    }

    fn device_token_unlocked(&self) -> StoreResult<Option<String>> {
        Ok(self
            .secrets
            .get(DEVICE_TOKEN_SECRET_KEY)?
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string()))
    }

    pub fn update_metadata(&self, update: impl FnOnce(&mut ConnectionMetadata)) -> StoreResult<()> {
        let _guard = self.transaction_lock.lock().unwrap();
        let mut metadata = self
            .load_metadata_unlocked()?
            .ok_or_else(|| StoreError::Other("not paired".into()))?;
        update(&mut metadata);
        self.save_metadata(&metadata)
    }

    pub fn load_enabled_connection(&self) -> StoreResult<Option<(String, String, String)>> {
        let _guard = self.transaction_lock.lock().unwrap();
        let Some(metadata) = self.load_metadata_unlocked()? else {
            return Ok(None);
        };
        if !metadata.is_live_desk_enabled {
            return Ok(None);
        }
        let Some(token) = self.device_token_unlocked()? else {
            return Ok(None);
        };
        Ok(Some((metadata.base_url, metadata.device_id, token)))
    }

    pub fn clear(&self) -> StoreResult<()> {
        let _guard = self.transaction_lock.lock().unwrap();
        self.secrets.remove(DEVICE_TOKEN_SECRET_KEY)?;
        self.remove_metadata()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;

    struct MemorySecrets(Mutex<BTreeMap<String, Vec<u8>>>);

    impl SecretStore for MemorySecrets {
        fn set(&self, key: &str, value: &[u8]) -> StoreResult<()> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_vec());
            Ok(())
        }
        fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn remove(&self, key: &str) -> StoreResult<()> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn store(dir: &Path) -> ConnectionStore {
        ConnectionStore::new(dir, Arc::new(MemorySecrets(Mutex::new(BTreeMap::new()))))
    }

    type AfterSecretWrite = Box<dyn FnOnce() -> StoreResult<()> + Send>;

    struct FileSecrets {
        dir: std::path::PathBuf,
        after_write: Mutex<Option<AfterSecretWrite>>,
        fail_mutations: AtomicBool,
    }

    impl FileSecrets {
        fn new(dir: &Path) -> Self {
            Self {
                dir: dir.to_path_buf(),
                after_write: Mutex::new(None),
                fail_mutations: AtomicBool::new(false),
            }
        }

        fn pause_next_write(
            &self,
            result: StoreResult<()>,
        ) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
            let (written_tx, written_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            *self.after_write.lock().unwrap() = Some(Box::new(move || {
                written_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                result
            }));
            (written_rx, release_tx)
        }

        fn path(&self, key: &str) -> std::path::PathBuf {
            self.dir.join(format!("{key}.test-secret"))
        }
    }

    impl SecretStore for FileSecrets {
        fn set(&self, key: &str, value: &[u8]) -> StoreResult<()> {
            if self.fail_mutations.load(Ordering::SeqCst) {
                return Err(StoreError::Other("credential writes unavailable".into()));
            }
            std::fs::write(self.path(key), value)?;
            let after_write = self.after_write.lock().unwrap().take();
            if let Some(after_write) = after_write {
                after_write()?;
            }
            Ok(())
        }

        fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>> {
            match std::fs::read(self.path(key)) {
                Ok(value) => Ok(Some(value)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error.into()),
            }
        }

        fn remove(&self, key: &str) -> StoreResult<()> {
            if self.fail_mutations.load(Ordering::SeqCst) {
                return Err(StoreError::Other("credential removal unavailable".into()));
            }
            match std::fs::remove_file(self.path(key)) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error.into()),
            }
        }
    }

    fn install_previous_pair(store: &ConnectionStore, enabled: bool) -> ConnectionMetadata {
        store
            .install_pairing_claim(
                "old-device",
                "old-token",
                &["old-scope".into()],
                3,
                "https://old.example",
            )
            .unwrap();
        store
            .update_metadata(|metadata| {
                metadata.next_sequence = Some(41);
                metadata.is_live_desk_enabled = enabled;
            })
            .unwrap();
        store.load_metadata().unwrap().unwrap()
    }

    fn install_replacement(store: &ConnectionStore) -> StoreResult<ConnectionMetadata> {
        store.install_pairing_claim(
            "new-device",
            "new-token",
            &["new-scope".into()],
            7,
            "https://new.example",
        )
    }

    fn assert_interrupted_pairing_cannot_enable_previous_pair(enabled: bool) {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FileSecrets::new(dir.path()));
        let store = ConnectionStore::new(dir.path(), secrets.clone());
        install_previous_pair(&store, enabled);
        let (written_rx, release_tx) = secrets.pause_next_write(Ok(()));

        std::thread::scope(|scope| {
            let installation = scope.spawn(|| install_replacement(&store));
            written_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let reopened = ConnectionStore::new(dir.path(), Arc::new(FileSecrets::new(dir.path())));
            let enable = reopened.update_metadata(|metadata| metadata.is_live_desk_enabled = true);
            let connection = reopened.load_enabled_connection();
            let token = reopened.device_token();
            release_tx.send(()).unwrap();
            installation.join().unwrap().unwrap();

            assert_eq!(token.unwrap().as_deref(), Some("new-token"));
            assert!(
                enable.is_err(),
                "an interrupted replacement left an enableable old pair"
            );
            assert!(
                connection.unwrap().is_none(),
                "an old server received the new token"
            );
        });
    }

    #[test]
    fn interrupted_replacement_of_enabled_pair_cannot_enable_previous_pair() {
        assert_interrupted_pairing_cannot_enable_previous_pair(true);
    }

    #[test]
    fn interrupted_replacement_of_disabled_pair_cannot_enable_previous_pair() {
        assert_interrupted_pairing_cannot_enable_previous_pair(false);
    }

    #[test]
    fn failed_credential_rollback_and_removal_leave_no_enableable_pair() {
        for enabled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let secrets = Arc::new(FileSecrets::new(dir.path()));
            let store = ConnectionStore::new(dir.path(), secrets.clone());
            install_previous_pair(&store, enabled);
            let (written_rx, release_tx) =
                secrets.pause_next_write(Err(StoreError::Other("replacement failed".into())));

            std::thread::scope(|scope| {
                let installation = scope.spawn(|| install_replacement(&store));
                written_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                secrets.fail_mutations.store(true, Ordering::SeqCst);
                release_tx.send(()).unwrap();
                let error = installation.join().unwrap().unwrap_err();
                assert!(error.to_string().contains("credential rollback failed"));
            });

            let reopened = ConnectionStore::new(dir.path(), Arc::new(FileSecrets::new(dir.path())));
            assert_eq!(
                reopened.device_token().unwrap().as_deref(),
                Some("new-token")
            );
            assert!(
                reopened
                    .update_metadata(|metadata| metadata.is_live_desk_enabled = true)
                    .is_err()
            );
            assert!(reopened.load_enabled_connection().unwrap().is_none());
        }
    }

    #[test]
    fn public_snapshots_wait_for_pairing_rollback() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FileSecrets::new(dir.path()));
        let store = ConnectionStore::new(dir.path(), secrets.clone());
        let previous = install_previous_pair(&store, true);
        let (written_rx, release_tx) =
            secrets.pause_next_write(Err(StoreError::Other("replacement failed".into())));
        let (metadata_started_tx, metadata_started_rx) = mpsc::channel();
        let (metadata_tx, metadata_rx) = mpsc::channel();
        let (token_started_tx, token_started_rx) = mpsc::channel();
        let (token_tx, token_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let installation = scope.spawn(|| install_replacement(&store));
            written_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            scope.spawn(|| {
                metadata_started_tx.send(()).unwrap();
                metadata_tx.send(store.load_metadata()).unwrap();
            });
            scope.spawn(|| {
                token_started_tx.send(()).unwrap();
                token_tx.send(store.device_token()).unwrap();
            });
            metadata_started_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            token_started_rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let early_metadata = metadata_rx.recv_timeout(Duration::from_millis(50));
            let early_token = token_rx.recv_timeout(Duration::from_millis(50));
            let metadata_was_blocked =
                matches!(early_metadata, Err(mpsc::RecvTimeoutError::Timeout));
            let token_was_blocked = matches!(early_token, Err(mpsc::RecvTimeoutError::Timeout));
            release_tx.send(()).unwrap();
            assert!(installation.join().unwrap().is_err());
            let metadata = early_metadata
                .or_else(|_| metadata_rx.recv_timeout(Duration::from_secs(5)))
                .unwrap()
                .unwrap();
            let token = early_token
                .or_else(|_| token_rx.recv_timeout(Duration::from_secs(5)))
                .unwrap()
                .unwrap();

            assert!(
                metadata_was_blocked,
                "metadata reader observed the temporary checkpoint"
            );
            assert!(
                token_was_blocked,
                "token reader observed an uncommitted credential"
            );
            assert_eq!(metadata, Some(previous));
            assert_eq!(token.as_deref(), Some("old-token"));
        });

        assert_eq!(
            store.load_enabled_connection().unwrap(),
            Some((
                "https://old.example".into(),
                "old-device".into(),
                "old-token".into()
            ))
        );
    }

    #[test]
    fn failed_replacement_restores_disabled_pair() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FileSecrets::new(dir.path()));
        let store = ConnectionStore::new(dir.path(), secrets.clone());
        let previous = install_previous_pair(&store, false);
        *secrets.after_write.lock().unwrap() = Some(Box::new(|| {
            Err(StoreError::Other("replacement failed".into()))
        }));

        assert!(install_replacement(&store).is_err());
        assert_eq!(store.load_metadata().unwrap(), Some(previous));
        assert_eq!(store.device_token().unwrap().as_deref(), Some("old-token"));
        assert!(store.load_enabled_connection().unwrap().is_none());
        store
            .update_metadata(|metadata| metadata.is_live_desk_enabled = true)
            .unwrap();
        assert_eq!(
            store.load_enabled_connection().unwrap(),
            Some((
                "https://old.example".into(),
                "old-device".into(),
                "old-token".into()
            ))
        );
    }

    #[test]
    fn failed_metadata_commit_and_restore_leave_no_enableable_pair() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FileSecrets::new(dir.path()));
        let store = ConnectionStore::new(dir.path(), secrets.clone());
        install_previous_pair(&store, true);
        let metadata_path = dir.path().join(CONNECTION_FILE);
        let blocked_path = metadata_path.clone();
        *secrets.after_write.lock().unwrap() = Some(Box::new(move || {
            match std::fs::remove_file(&blocked_path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                result => result?,
            }
            std::fs::create_dir(blocked_path)?;
            Ok(())
        }));

        assert!(matches!(
            install_replacement(&store),
            Err(StoreError::Io(_))
        ));
        assert_eq!(store.device_token().unwrap().as_deref(), Some("old-token"));
        assert!(store.load_enabled_connection().is_err());
        std::fs::remove_dir(metadata_path).unwrap();
        assert!(
            store
                .update_metadata(|metadata| metadata.is_live_desk_enabled = true)
                .is_err()
        );
        assert!(store.load_enabled_connection().unwrap().is_none());
    }

    #[test]
    fn fresh_pairing_recovers_malformed_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.set_device_token("stale-token").unwrap();
        std::fs::write(dir.path().join(CONNECTION_FILE), b"{\"schema_version\":").unwrap();

        let metadata = install_replacement(&store).unwrap();
        assert_eq!(metadata.base_url, "https://new.example");
        assert_eq!(metadata.device_id, "new-device");
        assert_eq!(metadata.scopes, ["new-scope"]);
        assert_eq!(metadata.pairing_next_sequence, 7);
        assert_eq!(metadata.next_sequence, Some(7));
        assert!(!metadata.is_live_desk_enabled);
        assert!(store.load_enabled_connection().unwrap().is_none());
        store
            .update_metadata(|metadata| metadata.is_live_desk_enabled = true)
            .unwrap();
        assert_eq!(
            store.load_enabled_connection().unwrap(),
            Some((
                "https://new.example".into(),
                "new-device".into(),
                "new-token".into()
            ))
        );
    }

    #[test]
    fn failed_repair_of_malformed_metadata_leaves_store_unpaired() {
        let dir = tempfile::tempdir().unwrap();
        let secrets = Arc::new(FileSecrets::new(dir.path()));
        let store = ConnectionStore::new(dir.path(), secrets.clone());
        store.set_device_token("stale-token").unwrap();
        std::fs::write(dir.path().join(CONNECTION_FILE), b"{\"schema_version\":").unwrap();
        *secrets.after_write.lock().unwrap() = Some(Box::new(|| {
            Err(StoreError::Other("replacement failed".into()))
        }));

        assert_eq!(
            install_replacement(&store).unwrap_err().to_string(),
            "replacement failed"
        );
        assert!(store.load_metadata().unwrap().is_none());
        assert_eq!(
            store.device_token().unwrap().as_deref(),
            Some("stale-token")
        );
        assert!(
            store
                .update_metadata(|metadata| metadata.is_live_desk_enabled = true)
                .is_err()
        );
        assert!(store.load_enabled_connection().unwrap().is_none());
    }

    #[test]
    fn metadata_read_error_does_not_replace_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store.set_device_token("old-token").unwrap();
        std::fs::create_dir(dir.path().join(CONNECTION_FILE)).unwrap();

        assert!(matches!(
            install_replacement(&store),
            Err(StoreError::Io(_))
        ));
        assert_eq!(store.device_token().unwrap().as_deref(), Some("old-token"));
        assert!(dir.path().join(CONNECTION_FILE).is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn metadata_checkpoint_error_does_not_replace_credentials() {
        use std::os::windows::fs::OpenOptionsExt;

        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        let previous = install_previous_pair(&store, true);
        let path = dir.path().join(CONNECTION_FILE);
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();

        let result = install_replacement(&store);
        drop(held);

        assert!(matches!(result, Err(StoreError::Io(_))));
        assert_eq!(store.load_metadata().unwrap(), Some(previous));
        assert_eq!(store.device_token().unwrap().as_deref(), Some("old-token"));
    }

    #[test]
    fn pairing_disables_live_desk_and_hides_token_from_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .install_pairing_claim(
                "11111111-1111-4111-8111-111111111111",
                "secret-token",
                &["companion:presence:write".to_string()],
                5,
                "https://core.example.com",
            )
            .unwrap();
        let metadata = store.load_metadata().unwrap().unwrap();
        assert!(!metadata.is_live_desk_enabled);
        assert_eq!(metadata.pairing_next_sequence, 5);
        let raw = std::fs::read_to_string(dir.path().join(CONNECTION_FILE)).unwrap();
        assert!(!raw.contains("secret-token"));
        assert!(store.load_enabled_connection().unwrap().is_none());
        assert_eq!(
            store.device_token().unwrap().as_deref(),
            Some("secret-token")
        );
    }

    #[test]
    fn enabled_connection_requires_flag_and_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .install_pairing_claim(
                "11111111-1111-4111-8111-111111111111",
                "t",
                &[],
                0,
                "https://x",
            )
            .unwrap();
        store
            .update_metadata(|m| {
                m.is_live_desk_enabled = true;
            })
            .unwrap();
        let conn = store.load_enabled_connection().unwrap().unwrap();
        assert_eq!(conn.0, "https://x");
        assert_eq!(conn.2, "t");
        store.remove_device_token().unwrap();
        assert!(store.load_enabled_connection().unwrap().is_none());
    }

    #[test]
    fn clear_removes_both() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .install_pairing_claim(
                "11111111-1111-4111-8111-111111111111",
                "t",
                &[],
                0,
                "https://x",
            )
            .unwrap();
        store.clear().unwrap();
        assert!(store.load_metadata().unwrap().is_none());
        assert!(store.device_token().unwrap().is_none());
        assert!(store.clear().is_ok());
    }

    #[test]
    fn failed_pairing_restores_previous_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir.path().join("missing"));
        store.set_device_token("previous-token").unwrap();

        assert!(
            store
                .install_pairing_claim("new-device", "new-token", &[], 7, "https://new")
                .is_err()
        );
        assert_eq!(
            store.device_token().unwrap().as_deref(),
            Some("previous-token")
        );
    }

    #[test]
    fn failed_first_pairing_removes_uncommitted_token() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir.path().join("missing"));

        assert!(
            store
                .install_pairing_claim("device", "new-token", &[], 7, "https://new")
                .is_err()
        );
        assert!(store.device_token().unwrap().is_none());
    }

    #[test]
    fn metadata_updates_are_serialized() {
        use std::sync::mpsc;
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        store
            .install_pairing_claim("device", "token", &[], 0, "https://core")
            .unwrap();
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (second_started_tx, second_started_rx) = mpsc::channel();
        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                store.update_metadata(move |m| {
                    first_entered_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    m.next_sequence = Some(42);
                })
            });
            first_entered_rx.recv().unwrap();
            let second = scope.spawn(|| {
                second_started_tx.send(()).unwrap();
                store.update_metadata(|m| {
                    second_entered_tx.send(()).unwrap();
                    m.is_live_desk_enabled = true;
                })
            });
            second_started_rx.recv().unwrap();
            let overlapped = second_entered_rx
                .recv_timeout(Duration::from_millis(50))
                .is_ok();
            release_tx.send(()).unwrap();
            first.join().unwrap().unwrap();
            second.join().unwrap().unwrap();
            assert!(!overlapped, "read-modify-write operations overlapped");
        });
        let metadata = store.load_metadata().unwrap().unwrap();
        assert_eq!(metadata.next_sequence, Some(42));
        assert!(metadata.is_live_desk_enabled);
    }
}
