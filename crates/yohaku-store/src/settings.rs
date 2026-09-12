//! 用户设置：`settings.json`。磁盘形态带 schema_version 与 serde 默认值，
//! 内存视图 [`Settings`] 经全 Option 的 [`SettingsPatch`] 逐字段校验后合并。

use crate::error::StoreResult;
use crate::json_io::{read_json_opt, write_json};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const SETTING_FILE: &str = "settings.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    /// 「应用程序」源
    pub share_applications: bool,
    /// 「窗口标题」源（默认关，对齐 macOS windowTitle=hide）
    pub share_window_titles: bool,
    /// 「媒体」源
    pub share_media: bool,
    /// 无歌手的媒体整体不上报
    pub ignore_null_artist: bool,
    pub launch_at_login: bool,
    pub pause_sharing: bool,
    /// 偏好播放器（对媒体会话 SourceAppUserModelId 不区分大小写子串匹配）
    pub preferred_players: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            schema_version: SETTINGS_SCHEMA_VERSION,
            share_applications: true,
            share_window_titles: false,
            share_media: true,
            ignore_null_artist: false,
            launch_at_login: false,
            pause_sharing: false,
            preferred_players: ["spotify", "cloudmusic", "qqmusic"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    pub share_applications: Option<bool>,
    pub share_window_titles: Option<bool>,
    pub share_media: Option<bool>,
    pub ignore_null_artist: Option<bool>,
    pub launch_at_login: Option<bool>,
    pub pause_sharing: Option<bool>,
    pub preferred_players: Option<Vec<String>>,
}

impl Settings {
    pub fn apply(&mut self, patch: &SettingsPatch) {
        let SettingsPatch {
            share_applications,
            share_window_titles,
            share_media,
            ignore_null_artist,
            launch_at_login,
            pause_sharing,
            preferred_players,
        } = patch;
        if let Some(v) = share_applications {
            self.share_applications = *v;
        }
        if let Some(v) = share_window_titles {
            self.share_window_titles = *v;
        }
        if let Some(v) = share_media {
            self.share_media = *v;
        }
        if let Some(v) = ignore_null_artist {
            self.ignore_null_artist = *v;
        }
        if let Some(v) = launch_at_login {
            self.launch_at_login = *v;
        }
        if let Some(v) = pause_sharing {
            self.pause_sharing = *v;
        }
        if let Some(v) = preferred_players {
            // 全部 trim + 非空过滤；全空时保持原值（防止手滑清空）
            let cleaned: Vec<String> = v
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            if !cleaned.is_empty() {
                self.preferred_players = cleaned;
            }
        }
    }

    pub fn load(dir: &Path) -> StoreResult<Settings> {
        let disk: Option<DiskSettings> = read_json_opt(&dir.join(SETTING_FILE))?;
        Ok(match disk {
            Some(disk) => disk.into(),
            None => Settings::default(),
        })
    }

    pub fn save(&self, dir: &Path) -> StoreResult<()> {
        write_json(&dir.join(SETTING_FILE), &DiskSettings::from(self))
    }
}

/// 磁盘形态：字段全部带默认值，历史文件缺字段也能读。
#[derive(Serialize, Deserialize)]
struct DiskSettings {
    #[serde(default = "default_schema_version")]
    schema_version: u32,
    #[serde(default = "default_true")]
    share_applications: bool,
    #[serde(default)]
    share_window_titles: bool,
    #[serde(default = "default_true")]
    share_media: bool,
    #[serde(default)]
    ignore_null_artist: bool,
    #[serde(default)]
    launch_at_login: bool,
    #[serde(default)]
    pause_sharing: bool,
    #[serde(default = "default_preferred_players")]
    preferred_players: Vec<String>,
}

fn default_schema_version() -> u32 {
    SETTINGS_SCHEMA_VERSION
}

fn default_true() -> bool {
    true
}

fn default_preferred_players() -> Vec<String> {
    Settings::default().preferred_players
}

impl From<DiskSettings> for Settings {
    fn from(d: DiskSettings) -> Self {
        Settings {
            schema_version: d.schema_version,
            share_applications: d.share_applications,
            share_window_titles: d.share_window_titles,
            share_media: d.share_media,
            ignore_null_artist: d.ignore_null_artist,
            launch_at_login: d.launch_at_login,
            pause_sharing: d.pause_sharing,
            preferred_players: d.preferred_players,
        }
    }
}

impl From<&Settings> for DiskSettings {
    fn from(s: &Settings) -> Self {
        DiskSettings {
            schema_version: s.schema_version,
            share_applications: s.share_applications,
            share_window_titles: s.share_window_titles,
            share_media: s.share_media,
            ignore_null_artist: s.ignore_null_artist,
            launch_at_login: s.launch_at_login,
            pause_sharing: s.pause_sharing,
            preferred_players: s.preferred_players.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_macos_semantics() {
        let s = Settings::default();
        assert!(s.share_applications);
        assert!(!s.share_window_titles); // macOS 默认 windowTitle=hide
        assert!(s.share_media);
        assert!(!s.ignore_null_artist);
    }

    #[test]
    fn patch_merges_fieldwise() {
        let mut s = Settings::default();
        let patch = SettingsPatch {
            share_window_titles: Some(true),
            pause_sharing: Some(true),
            ..Default::default()
        };
        s.apply(&patch);
        assert!(s.share_window_titles);
        assert!(s.pause_sharing);
        assert!(s.share_applications); // 未触及
        assert!(s.share_media);
    }

    #[test]
    fn patch_normalizes_preferred_players() {
        let mut s = Settings::default();
        s.apply(&SettingsPatch {
            preferred_players: Some(vec!["  Spotify ".into(), " ".into(), "QQMusic".into()]),
            ..Default::default()
        });
        assert_eq!(s.preferred_players, vec!["spotify", "qqmusic"]);
        // 全空不清空
        let before = s.preferred_players.clone();
        s.apply(&SettingsPatch {
            preferred_players: Some(vec![" ".into()]),
            ..Default::default()
        });
        assert_eq!(s.preferred_players, before);
    }

    #[test]
    fn roundtrip_with_defaults_for_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Settings::default();
        s.share_window_titles = true;
        s.save(dir.path()).unwrap();
        assert_eq!(Settings::load(dir.path()).unwrap(), s);
        // 手写缺字段文件
        std::fs::write(dir.path().join(SETTING_FILE), r#"{"schema_version":1}"#).unwrap();
        let loaded = Settings::load(dir.path()).unwrap();
        assert_eq!(loaded, Settings::default());
    }
}
