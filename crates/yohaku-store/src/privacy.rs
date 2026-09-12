//! 隐私规则：全局默认 + 逐应用规则（inherit/share/hide ×3 + 别名）。
//! 解析序：逐应用 Level → Inherit 落到全局默认；Hide 恒优先。

use crate::error::StoreResult;
use crate::json_io::{read_json_opt, write_json};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const PRIVACY_SCHEMA_VERSION: u32 = 1;
pub const PRIVACY_FILE: &str = "privacy-rules.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[default]
    Inherit,
    Share,
    Hide,
}

/// 全局默认三开关（对齐 macOS：application=share / windowTitle=hide / media=share）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalDefaults {
    pub application: Level,
    pub window_title: Level,
    pub media: Level,
}

impl Default for GlobalDefaults {
    fn default() -> Self {
        GlobalDefaults {
            application: Level::Share,
            window_title: Level::Hide,
            media: Level::Share,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppRule {
    pub application: Level,
    pub window_title: Level,
    pub media: Level,
    pub display_alias: Option<String>,
}

/// 键 = applicationKey（exe 文件名小写 / UWP AUMID 小写）。
pub type AppRules = BTreeMap<String, AppRule>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacyRules {
    pub schema_version: u32,
    #[serde(default)]
    pub defaults: GlobalDefaults,
    #[serde(default)]
    pub apps: AppRules,
}

impl Default for PrivacyRules {
    fn default() -> Self {
        PrivacyRules {
            schema_version: PRIVACY_SCHEMA_VERSION,
            defaults: GlobalDefaults::default(),
            apps: AppRules::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// 可见；alias 为显式别名（可空）
    Visible {
        alias: Option<String>,
    },
    Hidden,
}

impl PrivacyRules {
    fn resolve_level(app_level: Level, default_level: Level) -> bool {
        match app_level {
            Level::Hide => false,
            Level::Share => true,
            Level::Inherit => match default_level {
                Level::Hide => false,
                _ => true,
            },
        }
    }

    fn rule_for(&self, key: &str) -> Option<&AppRule> {
        self.apps.get(&key.to_lowercase())
    }

    pub fn resolve_application(&self, key: &str) -> Decision {
        let rule = self.rule_for(key);
        let visible = Self::resolve_level(
            rule.map_or(Level::Inherit, |r| r.application),
            self.defaults.application,
        );
        if visible {
            Decision::Visible {
                alias: rule.and_then(|r| {
                    r.display_alias.as_ref().and_then(|a| {
                        let t = a.trim();
                        (!t.is_empty()).then(|| t.to_string())
                    })
                }),
            }
        } else {
            Decision::Hidden
        }
    }

    pub fn shares_window_title(&self, key: &str) -> bool {
        Self::resolve_level(
            self.rule_for(key)
                .map_or(Level::Inherit, |r| r.window_title),
            self.defaults.window_title,
        )
    }

    pub fn resolve_media(&self, key: &str) -> Decision {
        let rule = self.rule_for(key);
        let visible = Self::resolve_level(
            rule.map_or(Level::Inherit, |r| r.media),
            self.defaults.media,
        );
        if visible {
            Decision::Visible { alias: None }
        } else {
            Decision::Hidden
        }
    }

    pub fn load(dir: &Path) -> StoreResult<PrivacyRules> {
        Ok(read_json_opt::<PrivacyRules>(&dir.join(PRIVACY_FILE))?.unwrap_or_default())
    }

    pub fn save(&self, dir: &Path) -> StoreResult<()> {
        write_json(&dir.join(PRIVACY_FILE), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> PrivacyRules {
        PrivacyRules::default()
    }

    #[test]
    fn unknown_app_uses_defaults() {
        let r = rules();
        assert_eq!(
            r.resolve_application("unknown.exe"),
            Decision::Visible { alias: None }
        );
        assert!(!r.shares_window_title("unknown.exe")); // 默认 hide
        assert!(matches!(
            r.resolve_media("unknown.exe"),
            Decision::Visible { .. }
        ));
    }

    #[test]
    fn hide_wins_over_everything() {
        let mut r = rules();
        r.apps.insert(
            "secret.exe".into(),
            AppRule {
                application: Level::Hide,
                window_title: Level::Hide,
                media: Level::Hide,
                display_alias: None,
            },
        );
        r.defaults.application = Level::Share;
        assert_eq!(r.resolve_application("secret.exe"), Decision::Hidden);
        assert!(!r.shares_window_title("secret.exe"));
        assert_eq!(r.resolve_media("secret.exe"), Decision::Hidden);
    }

    #[test]
    fn inherit_and_share_and_alias() {
        let mut r = rules();
        r.defaults.window_title = Level::Hide;
        r.apps.insert(
            "editor.exe".into(),
            AppRule {
                application: Level::Inherit,
                window_title: Level::Share, // 覆盖全局 hide
                media: Level::Inherit,
                display_alias: Some("  编辑器 ".into()),
            },
        );
        assert_eq!(
            r.resolve_application("EDITOR.EXE"), // 键大小写不敏感
            Decision::Visible {
                alias: Some("编辑器".into())
            }
        );
        assert!(r.shares_window_title("editor.exe"));
        // 全局 share 下显式 alias 为空串 → 无别名
        r.apps.get_mut("editor.exe").unwrap().display_alias = Some("  ".into());
        assert_eq!(
            r.resolve_application("editor.exe"),
            Decision::Visible { alias: None }
        );
    }

    #[test]
    fn global_default_flip() {
        let mut r = rules();
        r.defaults.media = Level::Hide;
        assert_eq!(r.resolve_media("anything.exe"), Decision::Hidden);
        r.apps.insert(
            "music.exe".into(),
            AppRule {
                application: Level::Inherit,
                window_title: Level::Inherit,
                media: Level::Share,
                display_alias: None,
            },
        );
        assert!(matches!(
            r.resolve_media("music.exe"),
            Decision::Visible { .. }
        ));
    }

    #[test]
    fn roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = rules();
        r.apps.insert(
            "a.exe".into(),
            AppRule {
                application: Level::Hide,
                window_title: Level::Inherit,
                media: Level::Share,
                display_alias: Some("X".into()),
            },
        );
        r.save(dir.path()).unwrap();
        assert_eq!(PrivacyRules::load(dir.path()).unwrap(), r);
        assert_eq!(
            PrivacyRules::load(&dir.path().join("nope")).unwrap_or_default(),
            PrivacyRules::default()
        );
    }
}
