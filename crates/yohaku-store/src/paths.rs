//! 数据目录解析：`%APPDATA%\YohakuCompanion`（debug 构建 `.debug` 后缀隔离）。

use std::path::PathBuf;

pub const DIR_NAME: &str = "YohakuCompanion";
pub const DIR_NAME_DEBUG: &str = "YohakuCompanion.debug";

pub fn dir_name() -> &'static str {
    if cfg!(debug_assertions) {
        DIR_NAME_DEBUG
    } else {
        DIR_NAME
    }
}

pub fn data_dir() -> PathBuf {
    let base = dirs::config_dir()
        .unwrap_or_else(|| std::env::temp_dir().join("yohaku-companion-fallback"));
    data_dir_under(&base)
}

pub fn data_dir_under(base: &std::path::Path) -> PathBuf {
    let dir = base.join(dir_name());
    let _ = std::fs::create_dir_all(&dir);
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_is_created_and_namespaced() {
        let dir = data_dir();
        assert!(dir.ends_with(dir_name()));
        assert!(dir.is_dir());
    }
}
