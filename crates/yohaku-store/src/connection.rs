//! 配对连接存储：非敏感元数据 `connection.json`（明文）+ 敏感 token（受保护存储）。
//! 事务顺序：先 token 落受保护存储，后 metadata 提交（metadata 是提交点）。

use crate::error::{StoreError, StoreResult};
use crate::json_io::{read_json_opt, write_json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

pub const CONNECTION_SCHEMA_VERSION: u32 = 1;
pub const CONNECTION_FILE: &str = "connection.json";

/// 受保护凭据端口：实现方负责 DPAPI/凭据管理器等保护。
/// key 是逻辑名（如 "device-token"），与文件名无关。
pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, value: &[u8]) -> StoreResult<()>;
    fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>>;
    fn remove(&self, key: &str) -> StoreResult<()>;
}

pub const DEVICE_TOKEN_SECRET_KEY: &str = "device-token";
pub const S3_ACCESS_KEY: &str = "s3-access-key";
pub const S3_SECRET_KEY: &str = "s3-secret-key";

/// 非敏感配对元数据。**deviceToken 绝不写入本文件**。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    pub schema_version: u32,
    pub base_url: String,
    pub device_id: String,
    pub scopes: Vec<String>,
    pub pairing_next_sequence: i64,
    /// 运行期序列器种子（由 Sequencer 持久化写入）
    pub next_sequence: Option<i64>,
    /// Live Desk 默认关闭；开启是显式预览确认边界
    pub is_live_desk_enabled: bool,
    /// 开启 Live Desk 时所依据的隐私策略指纹
    pub consent_fingerprint: Option<String>,
    pub paired_at: Option<DateTime<Utc>>,
}

pub struct ConnectionStore {
    dir: std::path::PathBuf,
    secrets: Arc<dyn SecretStore>,
}

impl ConnectionStore {
    pub fn new(dir: &Path, secrets: Arc<dyn SecretStore>) -> Self {
        ConnectionStore {
            dir: dir.to_path_buf(),
            secrets,
        }
    }

    pub fn load_metadata(&self) -> StoreResult<Option<ConnectionMetadata>> {
        read_json_opt(&self.dir.join(CONNECTION_FILE))
    }

    fn save_metadata(&self, metadata: &ConnectionMetadata) -> StoreResult<()> {
        write_json(&self.dir.join(CONNECTION_FILE), metadata)
    }

    /// 安装配对结果：先 token 后 metadata；无论之前状态如何，
    /// 配对后 Live Desk 恒为关闭。
    pub fn install_pairing_claim(
        &self,
        device_id: &str,
        device_token: &str,
        scopes: &[String],
        pairing_next_sequence: i64,
        base_url: &str,
    ) -> StoreResult<ConnectionMetadata> {
        self.secrets
            .set(DEVICE_TOKEN_SECRET_KEY, device_token.as_bytes())?;
        let metadata = ConnectionMetadata {
            schema_version: CONNECTION_SCHEMA_VERSION,
            base_url: base_url.to_string(),
            device_id: device_id.to_string(),
            scopes: scopes.to_vec(),
            pairing_next_sequence,
            next_sequence: Some(pairing_next_sequence),
            is_live_desk_enabled: false,
            consent_fingerprint: None,
            paired_at: Some(Utc::now()),
        };
        self.save_metadata(&metadata)?;
        Ok(metadata)
    }

    pub fn set_device_token(&self, token: &str) -> StoreResult<()> {
        self.secrets.set(DEVICE_TOKEN_SECRET_KEY, token.as_bytes())
    }

    pub fn remove_device_token(&self) -> StoreResult<()> {
        self.secrets.remove(DEVICE_TOKEN_SECRET_KEY)
    }

    pub fn device_token(&self) -> StoreResult<Option<String>> {
        Ok(self
            .secrets
            .get(DEVICE_TOKEN_SECRET_KEY)?
            .map(|bytes| String::from_utf8_lossy(&bytes).to_string()))
    }

    /// 更新非敏感元数据（保持 token 不动）。
    pub fn update_metadata(&self, update: impl FnOnce(&mut ConnectionMetadata)) -> StoreResult<()> {
        let mut metadata = self
            .load_metadata()?
            .ok_or_else(|| StoreError::Other("not paired".into()))?;
        update(&mut metadata);
        self.save_metadata(&metadata)
    }

    /// 仅在 `is_live_desk_enabled == true` 时返回完整连接——fail-closed。
    pub fn load_enabled_connection(&self) -> StoreResult<Option<(String, String, String)>> {
        let Some(metadata) = self.load_metadata()? else {
            return Ok(None);
        };
        if !metadata.is_live_desk_enabled {
            return Ok(None);
        }
        let Some(token) = self.device_token()? else {
            return Ok(None);
        };
        Ok(Some((metadata.base_url, metadata.device_id, token)))
    }

    /// 解除配对：先删 token，再删 metadata。
    pub fn clear(&self) -> StoreResult<()> {
        self.remove_device_token()?;
        let path = self.dir.join(CONNECTION_FILE);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::Mutex;

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
        assert!(!metadata.is_live_desk_enabled); // 配对不开启 Live Desk
        assert_eq!(metadata.pairing_next_sequence, 5);
        // token 不入明文文件
        let raw = std::fs::read_to_string(dir.path().join(CONNECTION_FILE)).unwrap();
        assert!(!raw.contains("secret-token"));
        // enabled=false → fail-closed
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
                m.consent_fingerprint = Some("fp".into());
            })
            .unwrap();
        let conn = store.load_enabled_connection().unwrap().unwrap();
        assert_eq!(conn.0, "https://x");
        assert_eq!(conn.2, "t");
        // 删除 token 后 fail-closed
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
        assert!(store.clear().is_ok()); // 幂等
    }
}
