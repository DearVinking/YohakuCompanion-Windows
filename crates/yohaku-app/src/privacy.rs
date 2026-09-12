//! 隐私管道：原始捕获 → 净化快照的唯一边界（fail-closed）。
//! 每次捕获都重读设置与规则；策略指纹变更即作废发布同意并触发重发。

use crate::capture::{
    RawApplicationFocus, RawMediaState, SanitizedApplication, SanitizedMedia,
    SanitizedPresenceSnapshot,
};
use crate::session::MediaSessionTracker;
use sha2::{Digest, Sha256};
use std::sync::RwLock;
use yohaku_store::privacy::{Decision, PrivacyRules};
use yohaku_store::settings::{Settings, SettingsPatch};

/// 已知音乐播放器（对 player_key 不区分大小写子串匹配）→ kind=music。
const MUSIC_PLAYERS: &[&str] = &["spotify", "cloudmusic", "qqmusic", "foobar2000", "musicbee"];

pub struct PrivacyPipeline {
    settings: RwLock<Settings>,
    rules: RwLock<PrivacyRules>,
}

fn has_text(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| !v.is_empty())
}

impl PrivacyPipeline {
    pub fn new(settings: Settings, rules: PrivacyRules) -> Self {
        PrivacyPipeline {
            settings: RwLock::new(settings),
            rules: RwLock::new(rules),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn rules(&self) -> PrivacyRules {
        self.rules.read().unwrap().clone()
    }

    /// 应用设置补丁并返回新值。
    pub fn update_settings(&self, patch: &SettingsPatch) -> Settings {
        let mut settings = self.settings.write().unwrap();
        settings.apply(patch);
        settings.clone()
    }

    pub fn replace_settings(&self, settings: Settings) {
        *self.settings.write().unwrap() = settings;
    }

    pub fn replace_rules(&self, rules: PrivacyRules) {
        *self.rules.write().unwrap() = rules;
    }

    /// 隐私策略指纹：覆盖源开关、全局默认与逐应用规则的排序 JSON 的 SHA-256。
    /// 用于发布同意作废与协调器刷新触发。
    pub fn policy_fingerprint(&self) -> String {
        let settings = self.settings.read().unwrap();
        let rules = self.rules.read().unwrap();
        #[derive(serde::Serialize)]
        struct Fingerprint<'a> {
            share_applications: bool,
            share_window_titles: bool,
            share_media: bool,
            ignore_null_artist: bool,
            rules: &'a PrivacyRules,
        }
        let value = serde_json::to_value(&Fingerprint {
            share_applications: settings.share_applications,
            share_window_titles: settings.share_window_titles,
            share_media: settings.share_media,
            ignore_null_artist: settings.ignore_null_artist,
            rules: &rules,
        })
        .expect("fingerprint is serializable");
        drop(rules);
        drop(settings);
        // serde_json Value::Object 是 BTreeMap → 键有序
        let canonical = serde_json::to_string(&value).expect("canonical json");
        let hash = Sha256::digest(canonical.as_bytes());
        hex::encode(hash)
    }

    /// 原始捕获 → 净化快照。任一环节不通过即整体丢弃对应部分（fail-closed）。
    pub fn capture(
        &self,
        app: Option<&RawApplicationFocus>,
        media: Option<&RawMediaState>,
        tracker: &mut MediaSessionTracker,
    ) -> SanitizedPresenceSnapshot {
        let settings = self.settings();
        let rules = self.rules();

        let application = app.filter(|_| settings.share_applications).and_then(|raw| {
            match rules.resolve_application(&raw.application_key) {
                Decision::Hidden => None,
                Decision::Visible { alias } => {
                    let display_name = alias.unwrap_or_else(|| raw.display_name.trim().to_string());
                    if display_name.is_empty() {
                        return None;
                    }
                    let window_title = if settings.share_window_titles
                        && rules.shares_window_title(&raw.application_key)
                    {
                        raw.window_title.clone()
                    } else {
                        None
                    };
                    Some(SanitizedApplication {
                        display_name,
                        window_title,
                    })
                }
            }
        });

        let media = media
            .filter(|_| settings.share_media)
            .filter(|m| m.playing)
            .and_then(|raw| match rules.resolve_media(&raw.player_key) {
                Decision::Hidden => None,
                Decision::Visible { .. } => {
                    if settings.ignore_null_artist && !has_text(&raw.artist) {
                        return None;
                    }
                    if !has_text(&raw.title) && !has_text(&raw.artist) {
                        return None;
                    }
                    let player_key_lower = raw.player_key.to_lowercase();
                    let kind = if MUSIC_PLAYERS.iter().any(|p| player_key_lower.contains(p)) {
                        yohaku_protocol::presence::MediaKind::Music
                    } else {
                        yohaku_protocol::presence::MediaKind::Unknown
                    };
                    let title = raw.title.clone().filter(|t| !t.is_empty());
                    let artist = raw.artist.clone().filter(|t| !t.is_empty());
                    let album = raw.album.clone().filter(|t| !t.is_empty());
                    let player_display_name =
                        Some(raw.player_display_name.trim().to_string()).filter(|p| !p.is_empty());
                    let session_id = tracker.session_id(&crate::session::MediaIdentity {
                        kind,
                        title: title.clone(),
                        artist: artist.clone(),
                        album: album.clone(),
                        player: player_display_name.clone(),
                        duration_ms: raw.duration_seconds.map(|d| (d * 1000.0).round() as i64),
                    });
                    Some(SanitizedMedia {
                        session_id,
                        kind,
                        title,
                        artist,
                        album,
                        player_display_name,
                        playing: raw.playing,
                        duration_seconds: raw.duration_seconds,
                        position_seconds: raw.position_seconds,
                        position_sampled_at: raw.position_sampled_at,
                    })
                }
            });

        let availability = if application.is_some() || media.is_some() {
            yohaku_protocol::presence::Availability::Active
        } else {
            yohaku_protocol::presence::Availability::Idle
        };

        SanitizedPresenceSnapshot {
            availability,
            application,
            media,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn app(key: &str, name: &str, title: Option<&str>) -> RawApplicationFocus {
        RawApplicationFocus {
            application_key: key.into(),
            display_name: name.into(),
            window_title: title.map(|t| t.into()),
        }
    }

    fn media() -> RawMediaState {
        RawMediaState::new(
            Some("歌名".into()),
            Some("歌手".into()),
            Some("专辑".into()),
            "cloudmusic.exe".into(),
            "网易云音乐".into(),
            true,
            Some(200.0),
            Some(30.0),
            Some(Utc::now()),
            Some(vec![1, 2, 3]),
        )
    }

    fn pipeline() -> PrivacyPipeline {
        PrivacyPipeline::new(Settings::default(), PrivacyRules::default())
    }

    #[test]
    fn app_visible_title_hidden_by_default() {
        let p = pipeline();
        let mut tracker = MediaSessionTracker::new();
        let snap = p.capture(
            Some(&app("msedge.exe", "Microsoft Edge", Some("文档"))),
            None,
            &mut tracker,
        );
        let a = snap.application.unwrap();
        assert_eq!(a.display_name, "Microsoft Edge");
        assert_eq!(a.window_title, None); // 全局默认 hide
        assert_eq!(
            snap.availability,
            yohaku_protocol::presence::Availability::Active
        );
    }

    #[test]
    fn title_shown_when_both_switches_on() {
        // 设置开关 + 隐私规则默认都要放行（macOS 语义：sharesWindowTitle && 全局开关）
        let mut rules = PrivacyRules::default();
        rules.defaults.window_title = yohaku_store::privacy::Level::Share;
        let p = PrivacyPipeline::new(Settings::default(), rules);
        p.update_settings(&SettingsPatch {
            share_window_titles: Some(true),
            ..Default::default()
        });
        let mut tracker = MediaSessionTracker::new();
        let snap = p.capture(
            Some(&app("msedge.exe", "Edge", Some("文档 — 标题"))),
            None,
            &mut tracker,
        );
        assert_eq!(
            snap.application.unwrap().window_title.as_deref(),
            Some("文档 — 标题")
        );
    }

    #[test]
    fn per_app_hide_and_alias() {
        let mut rules = PrivacyRules::default();
        rules.apps.insert(
            "secret.exe".into(),
            yohaku_store::privacy::AppRule {
                application: yohaku_store::privacy::Level::Hide,
                window_title: yohaku_store::privacy::Level::Hide,
                media: yohaku_store::privacy::Level::Hide,
                display_alias: None,
            },
        );
        rules.apps.insert(
            "code.exe".into(),
            yohaku_store::privacy::AppRule {
                application: yohaku_store::privacy::Level::Inherit,
                window_title: yohaku_store::privacy::Level::Share,
                media: yohaku_store::privacy::Level::Inherit,
                display_alias: Some("码农机".into()),
            },
        );
        let p = PrivacyPipeline::new(Settings::default(), rules);
        let mut tracker = MediaSessionTracker::new();
        // 隐藏应用：application None，即使开着标题开关
        p.update_settings(&SettingsPatch {
            share_window_titles: Some(true),
            ..Default::default()
        });
        let snap = p.capture(
            Some(&app("secret.exe", "Secret", Some("top secret"))),
            None,
            &mut tracker,
        );
        assert!(snap.application.is_none());
        // 别名覆盖显示名
        let snap = p.capture(
            Some(&app("code.exe", "Visual Studio Code", None)),
            None,
            &mut tracker,
        );
        assert_eq!(snap.application.unwrap().display_name, "码农机");
    }

    #[test]
    fn source_switches_gate_capture() {
        let p = pipeline();
        let mut tracker = MediaSessionTracker::new();
        p.update_settings(&SettingsPatch {
            share_applications: Some(false),
            share_media: Some(false),
            ..Default::default()
        });
        let snap = p.capture(Some(&app("a.exe", "A", None)), Some(&media()), &mut tracker);
        assert!(snap.application.is_none());
        assert!(snap.media.is_none());
        assert_eq!(
            snap.availability,
            yohaku_protocol::presence::Availability::Idle
        );
    }

    #[test]
    fn media_rules() {
        let p = pipeline();
        let mut tracker = MediaSessionTracker::new();
        // 暂停 → 不上报
        let paused = RawMediaState::new(
            Some("t".into()),
            Some("a".into()),
            None,
            "spotify.exe".into(),
            "Spotify".into(),
            false,
            Some(1.0),
            None,
            None,
            None,
        );
        let snap = p.capture(None, Some(&paused), &mut tracker);
        assert!(snap.media.is_none());

        // 无标题无歌手 → 整体无效
        let blank = RawMediaState::new(
            None,
            None,
            None,
            "spotify.exe".into(),
            "Spotify".into(),
            true,
            Some(1.0),
            None,
            None,
            None,
        );
        assert!(p.capture(None, Some(&blank), &mut tracker).media.is_none());

        // ignoreNullArtist
        let no_artist = RawMediaState::new(
            Some("t".into()),
            None,
            None,
            "spotify.exe".into(),
            "Spotify".into(),
            true,
            Some(1.0),
            None,
            None,
            None,
        );
        assert!(
            p.capture(None, Some(&no_artist), &mut tracker)
                .media
                .is_some()
        );
        p.update_settings(&SettingsPatch {
            ignore_null_artist: Some(true),
            ..Default::default()
        });
        assert!(
            p.capture(None, Some(&no_artist), &mut tracker)
                .media
                .is_none()
        );

        // kind 识别
        let snap = p.capture(None, Some(&media()), &mut tracker);
        let m = snap.media.unwrap();
        assert_eq!(m.kind, yohaku_protocol::presence::MediaKind::Music);
        assert_eq!(m.player_display_name.as_deref(), Some("网易云音乐"));
        // 原始键不进入净化域
        assert!(!format!("{m:?}").contains("cloudmusic.exe"));
    }

    #[test]
    fn player_hidden_by_rule() {
        let mut rules = PrivacyRules::default();
        rules.apps.insert(
            "spotify.exe".into(),
            yohaku_store::privacy::AppRule {
                application: yohaku_store::privacy::Level::Inherit,
                window_title: yohaku_store::privacy::Level::Inherit,
                media: yohaku_store::privacy::Level::Hide,
                display_alias: None,
            },
        );
        let p = PrivacyPipeline::new(Settings::default(), rules);
        let mut tracker = MediaSessionTracker::new();
        let m = RawMediaState::new(
            Some("t".into()),
            Some("a".into()),
            None,
            "spotify.exe".into(),
            "Spotify".into(),
            true,
            Some(1.0),
            None,
            None,
            None,
        );
        assert!(p.capture(None, Some(&m), &mut tracker).media.is_none());
    }

    #[test]
    fn fingerprint_tracks_policy_changes() {
        let p = pipeline();
        let fp1 = p.policy_fingerprint();
        p.update_settings(&SettingsPatch {
            ignore_null_artist: Some(true),
            ..Default::default()
        });
        let fp2 = p.policy_fingerprint();
        assert_ne!(fp1, fp2);
        // 非隐私设置（如开机自启）不影响指纹
        let fp3 = p.policy_fingerprint();
        p.update_settings(&SettingsPatch {
            launch_at_login: Some(true),
            ..Default::default()
        });
        assert_eq!(fp3, p.policy_fingerprint());
    }

    #[test]
    fn session_stable_across_captures() {
        let p = pipeline();
        let mut tracker = MediaSessionTracker::new();
        let s1 = p
            .capture(None, Some(&media()), &mut tracker)
            .media
            .unwrap()
            .session_id;
        let s2 = p
            .capture(None, Some(&media()), &mut tracker)
            .media
            .unwrap()
            .session_id;
        assert_eq!(s1, s2);
        let changed = RawMediaState::new(
            Some("另一首".into()),
            Some("歌手".into()),
            Some("专辑".into()),
            "cloudmusic.exe".into(),
            "网易云音乐".into(),
            true,
            Some(200.0),
            Some(30.0),
            Some(Utc::now()),
            None,
        );
        let s3 = p
            .capture(None, Some(&changed), &mut tracker)
            .media
            .unwrap()
            .session_id;
        assert_ne!(s1, s3);
    }
}
