//! DPAPI（CryptProtectData/CryptUnprotectData，CurrentUser 作用域）+
//! 原子文件存储。同一用户下只有本进程组能解密；文件本身不可移植。

use std::io::Write;
use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use yohaku_store::{SecretStore, StoreError, StoreResult};

pub struct DpapiSecretStore {
    dir: PathBuf,
}

impl DpapiSecretStore {
    pub fn new(dir: &Path) -> Self {
        let _ = std::fs::create_dir_all(dir);
        DpapiSecretStore {
            dir: dir.to_path_buf(),
        }
    }

    fn path_for(&self, key: &str) -> PathBuf {
        // key 是内部逻辑名，仅含 [a-z0-9-]
        self.dir.join(format!("{key}.bin"))
    }

    fn key_is_safe(key: &str) -> bool {
        !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    }
}

impl SecretStore for DpapiSecretStore {
    fn set(&self, key: &str, value: &[u8]) -> StoreResult<()> {
        if !Self::key_is_safe(key) {
            return Err(StoreError::Other(format!("invalid secret key `{key}`")));
        }
        // SAFETY: input 指向调用方提供的有效缓冲区；output 由 DPAPI 分配，
        // 成功后读出内容并用 LocalFree 释放。
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: value.len() as u32,
                pbData: value.as_ptr() as *mut u8,
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                return Err(StoreError::Other("CryptProtectData failed".into()));
            }
            let encrypted =
                std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData as HLOCAL);

            let mut file =
                atomic_write_file::AtomicWriteFile::options().open(self.path_for(key))?;
            file.write_all(&encrypted)?;
            file.commit()?;
            Ok(())
        }
    }

    fn get(&self, key: &str) -> StoreResult<Option<Vec<u8>>> {
        if !Self::key_is_safe(key) {
            return Err(StoreError::Other(format!("invalid secret key `{key}`")));
        }
        let encrypted = match std::fs::read(self.path_for(key)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        // SAFETY: input 指向刚读入的密文缓冲区；output 由 DPAPI 分配，
        // 成功后读出内容并用 LocalFree 释放。
        unsafe {
            let input = CRYPT_INTEGER_BLOB {
                cbData: encrypted.len() as u32,
                pbData: encrypted.as_ptr() as *mut u8,
            };
            let mut output = CRYPT_INTEGER_BLOB {
                cbData: 0,
                pbData: std::ptr::null_mut(),
            };
            let ok = CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            );
            if ok == 0 {
                return Err(StoreError::Other("CryptUnprotectData failed".into()));
            }
            let plain = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            LocalFree(output.pbData as HLOCAL);
            Ok(Some(plain))
        }
    }

    fn remove(&self, key: &str) -> StoreResult<()> {
        if !Self::key_is_safe(key) {
            return Err(StoreError::Other(format!("invalid secret key `{key}`")));
        }
        match std::fs::remove_file(self.path_for(key)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn dpapi_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = DpapiSecretStore::new(dir.path());
        store.set("device-token", b"secret-token-1").unwrap();
        assert_eq!(
            store.get("device-token").unwrap().as_deref(),
            Some(b"secret-token-1".as_slice())
        );
        // 密文文件不含明文
        let raw = std::fs::read(dir.path().join("device-token.bin")).unwrap();
        assert!(!windows(&raw, b"secret-token-1"));
        store.remove("device-token").unwrap();
        assert_eq!(store.get("device-token").unwrap(), None);
    }

    #[cfg(windows)]
    fn windows(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[cfg(windows)]
    #[test]
    fn rejects_bad_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = DpapiSecretStore::new(dir.path());
        assert!(store.set("../evil", b"x").is_err());
        assert!(store.set("", b"x").is_err());
    }
}
