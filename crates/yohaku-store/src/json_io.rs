//! JSON 原子读写（所有持久化文件的唯一写入口）。

use crate::error::StoreResult;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::io::Write;
use std::path::Path;

pub fn write_json_text(path: &Path, text: &str) -> StoreResult<()> {
    let mut file = atomic_write_file::AtomicWriteFile::options().open(path)?;
    file.write_all(text.as_bytes())?;
    file.write_all(b"\n")?;
    file.commit()?;
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> StoreResult<()> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    write_json_text(path, &text)
}

pub fn read_json_text(path: &Path) -> StoreResult<String> {
    Ok(std::fs::read_to_string(path)?)
}

/// 文件不存在 → None（首次运行语义）。
pub fn read_json_opt<T: DeserializeOwned>(path: &Path) -> StoreResult<Option<T>> {
    match std::fs::read(path) {
        Ok(bytes) => {
            let value = serde_json::from_slice(&bytes)?;
            Ok(Some(value))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> StoreResult<T> {
    read_json_opt(path)?
        .ok_or_else(|| crate::error::StoreError::Other(format!("missing file {}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    struct Doc {
        name: String,
        counts: BTreeMap<String, i64>,
    }

    #[test]
    fn roundtrip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.json");
        assert!(read_json_opt::<Doc>(&path).unwrap().is_none());
        let doc = Doc {
            name: "测试".into(),
            counts: BTreeMap::from([("a".into(), 1)]),
        };
        write_json(&path, &doc).unwrap();
        assert_eq!(read_json::<Doc>(&path).unwrap(), doc);
        let text = read_json_text(&path).unwrap();
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn overwrite_is_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.json");
        let doc = Doc {
            name: "one".into(),
            counts: BTreeMap::new(),
        };
        write_json(&path, &doc).unwrap();
        let doc2 = Doc {
            name: "two".into(),
            counts: BTreeMap::new(),
        };
        write_json(&path, &doc2).unwrap();
        assert_eq!(read_json::<Doc>(&path).unwrap(), doc2);
    }
}
