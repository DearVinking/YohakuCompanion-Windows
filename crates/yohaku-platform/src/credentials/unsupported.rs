use yohaku_store::{SecretStore, StoreError, StoreResult};

pub struct DpapiSecretStore;

impl DpapiSecretStore {
    pub fn new(_dir: &std::path::Path) -> Self {
        DpapiSecretStore
    }
}

impl SecretStore for DpapiSecretStore {
    fn set(&self, _key: &str, _value: &[u8]) -> StoreResult<()> {
        Err(StoreError::Other(
            "credential storage requires Windows".into(),
        ))
    }
    fn get(&self, _key: &str) -> StoreResult<Option<Vec<u8>>> {
        Ok(None)
    }
    fn remove(&self, _key: &str) -> StoreResult<()> {
        Ok(())
    }
}
