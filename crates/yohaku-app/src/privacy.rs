use crate::capture::{
    RawApplicationFocus, RawMediaState, SanitizedApplication, SanitizedMedia,
    SanitizedPresenceSnapshot,
};
use crate::session::MediaSessionTracker;
use std::sync::RwLock;
use yohaku_store::privacy::{Decision, PrivacyRules};
use yohaku_store::settings::{Settings, SettingsPatch};

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

    pub fn capture(
        &self,
        app: Option<&RawApplicationFocus>,
        media: Option<&RawMediaState>,
        tracker: &mut MediaSessionTracker,
    ) -> SanitizedPresenceSnapshot {
        let settings = self.settings();
        let rules = self.rules();

        let application = sanitize_application(app, &settings, &rules);
        let media = sanitize_media(media, &settings, &rules, tracker);

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

fn sanitize_application(
    app: Option<&RawApplicationFocus>,
    settings: &Settings,
    rules: &PrivacyRules,
) -> Option<SanitizedApplication> {
    app.filter(|_| settings.share_applications).and_then(|raw| {
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
    })
}

fn sanitize_media(
    media: Option<&RawMediaState>,
    settings: &Settings,
    rules: &PrivacyRules,
    tracker: &mut MediaSessionTracker,
) -> Option<SanitizedMedia> {
    media
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
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use yohaku_store::privacy::{AppRule, Level};

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
        assert_eq!(a.window_title, None);
        assert_eq!(
            snap.availability,
            yohaku_protocol::presence::Availability::Active
        );
    }

    #[test]
    fn title_shown_when_both_switches_on() {
        let mut rules = PrivacyRules::default();
        rules.defaults.window_title = yohaku_store::privacy::Level::Share;
        let p = PrivacyPipeline::new(Settings::default(), rules);
        let mut tracker = MediaSessionTracker::new();
        let sample = app("msedge.exe", "Edge", Some("文档 — 标题"));
        assert_eq!(
            p.capture(Some(&sample), None, &mut tracker)
                .application
                .unwrap()
                .window_title,
            None
        );
        p.update_settings(&SettingsPatch {
            share_window_titles: Some(true),
            ..Default::default()
        });
        let snap = p.capture(Some(&sample), None, &mut tracker);
        assert_eq!(
            snap.application.unwrap().window_title.as_deref(),
            Some("文档 — 标题")
        );
        p.replace_rules(PrivacyRules::default());
        assert_eq!(
            p.capture(Some(&sample), None, &mut tracker)
                .application
                .unwrap()
                .window_title,
            None
        );
    }

    #[test]
    fn explicit_share_overrides_global_hide_but_respects_source_switches() {
        let mut rules = PrivacyRules::default();
        rules.defaults.application = Level::Hide;
        rules.defaults.media = Level::Hide;
        rules.apps.insert(
            "cloudmusic.exe".into(),
            AppRule {
                application: Level::Share,
                window_title: Level::Share,
                media: Level::Share,
                display_alias: None,
            },
        );
        let p = PrivacyPipeline::new(
            Settings {
                share_window_titles: true,
                ..Settings::default()
            },
            rules,
        );
        let sample = app("cloudmusic.exe", "Player", Some("Window"));
        let mut tracker = MediaSessionTracker::new();
        let snapshot = p.capture(Some(&sample), Some(&media()), &mut tracker);
        assert_eq!(
            snapshot.application.unwrap().window_title.as_deref(),
            Some("Window")
        );
        assert!(snapshot.media.is_some());
        p.update_settings(&SettingsPatch {
            share_applications: Some(false),
            share_media: Some(false),
            ..Default::default()
        });
        let snapshot = p.capture(Some(&sample), Some(&media()), &mut tracker);
        assert!(snapshot.application.is_none());
        assert!(snapshot.media.is_none());
        assert_eq!(
            snapshot.availability,
            yohaku_protocol::presence::Availability::Idle
        );
    }

    #[test]
    fn application_trims_names_and_aliases_but_preserves_window_title() {
        for (alias, name, expected_name) in [
            (None, "  App  ", Some("App")),
            (Some(" \t "), "  App  ", Some("App")),
            (Some("  Alias\n"), "App", Some("Alias")),
            (None, " \t ", None),
            (Some("Alias"), "", Some("Alias")),
        ] {
            let mut rules = PrivacyRules::default();
            rules.defaults.window_title = Level::Share;
            rules.apps.insert(
                "app.exe".into(),
                AppRule {
                    display_alias: alias.map(str::to_string),
                    ..AppRule::default()
                },
            );
            let p = PrivacyPipeline::new(
                Settings {
                    share_window_titles: true,
                    ..Settings::default()
                },
                rules,
            );
            let snapshot = p.capture(
                Some(&app("app.exe", name, Some(" \tWindow\n"))),
                None,
                &mut MediaSessionTracker::new(),
            );
            assert_eq!(
                snapshot.application,
                expected_name.map(|name| SanitizedApplication {
                    display_name: name.into(),
                    window_title: Some(" \tWindow\n".into()),
                })
            );
        }
    }

    #[test]
    fn media_text_eligibility_distinguishes_empty_from_whitespace() {
        for (title, artist, ignore_null_artist, expected) in [
            (None, Some("artist"), false, Some((None, Some("artist")))),
            (Some("title"), None, false, Some((Some("title"), None))),
            (Some("title"), None, true, None),
            (Some("title"), Some(""), true, None),
            (
                Some("title"),
                Some(" "),
                true,
                Some((Some("title"), Some(" "))),
            ),
            (Some(""), Some(""), false, None),
            (Some("  "), None, false, Some((Some("  "), None))),
        ] {
            let p = PrivacyPipeline::new(
                Settings {
                    ignore_null_artist,
                    ..Settings::default()
                },
                PrivacyRules::default(),
            );
            let sample = RawMediaState {
                title: title.map(str::to_string),
                artist: artist.map(str::to_string),
                album: Some(String::new()),
                player_display_name: " \tPlayer ".into(),
                ..media()
            };
            let actual = p
                .capture(None, Some(&sample), &mut MediaSessionTracker::new())
                .media;
            match expected {
                Some((title, artist)) => {
                    let actual = actual.unwrap();
                    assert_eq!(actual.title.as_deref(), title);
                    assert_eq!(actual.artist.as_deref(), artist);
                    assert_eq!(actual.album, None);
                    assert_eq!(actual.player_display_name.as_deref(), Some("Player"));
                }
                None => assert!(actual.is_none()),
            }
        }
    }

    #[test]
    fn rejected_media_does_not_evict_tracker_sessions() {
        enum Gate {
            Source,
            Playback,
            Rule,
            Artist,
            Identity,
        }
        for gate in [
            Gate::Source,
            Gate::Playback,
            Gate::Rule,
            Gate::Artist,
            Gate::Identity,
        ] {
            let p = pipeline();
            let mut tracker = MediaSessionTracker::new();
            let original_id = p
                .capture(None, Some(&media()), &mut tracker)
                .media
                .unwrap()
                .session_id;
            match gate {
                Gate::Source => {
                    p.update_settings(&SettingsPatch {
                        share_media: Some(false),
                        ..Default::default()
                    });
                }
                Gate::Rule => {
                    let mut rules = PrivacyRules::default();
                    rules.defaults.media = Level::Hide;
                    p.replace_rules(rules);
                }
                Gate::Artist => {
                    p.update_settings(&SettingsPatch {
                        ignore_null_artist: Some(true),
                        ..Default::default()
                    });
                }
                _ => {}
            }
            for index in 0..40 {
                let mut rejected = media();
                rejected.album = Some(format!("rejected-{index}"));
                match gate {
                    Gate::Playback => rejected.playing = false,
                    Gate::Artist => rejected.artist = None,
                    Gate::Identity => {
                        rejected.title = None;
                        rejected.artist = None;
                    }
                    _ => {}
                }
                assert!(
                    p.capture(None, Some(&rejected), &mut tracker)
                        .media
                        .is_none()
                );
            }
            p.replace_settings(Settings::default());
            p.replace_rules(PrivacyRules::default());
            assert_eq!(
                p.capture(None, Some(&media()), &mut tracker)
                    .media
                    .unwrap()
                    .session_id,
                original_id
            );
        }
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
        );
        let snap = p.capture(None, Some(&paused), &mut tracker);
        assert!(snap.media.is_none());

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
        );
        assert!(p.capture(None, Some(&blank), &mut tracker).media.is_none());

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

        let snap = p.capture(None, Some(&media()), &mut tracker);
        let m = snap.media.unwrap();
        assert_eq!(m.kind, yohaku_protocol::presence::MediaKind::Music);
        assert_eq!(m.player_display_name.as_deref(), Some("网易云音乐"));
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
        );
        assert!(p.capture(None, Some(&m), &mut tracker).media.is_none());
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
            .capture(
                None,
                Some(&RawMediaState {
                    position_seconds: Some(80.0),
                    duration_seconds: Some(200.0004),
                    ..media()
                }),
                &mut tracker,
            )
            .media
            .unwrap()
            .session_id;
        assert_eq!(s1, s2);
        let rounded_up = p
            .capture(
                None,
                Some(&RawMediaState {
                    duration_seconds: Some(200.0006),
                    ..media()
                }),
                &mut tracker,
            )
            .media
            .unwrap()
            .session_id;
        assert_ne!(s1, rounded_up);
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
        );
        let s3 = p
            .capture(None, Some(&changed), &mut tracker)
            .media
            .unwrap()
            .session_id;
        assert_ne!(s1, s3);
    }
}
