use crate::MAX_SAFE_INTEGER;
use crate::capabilities::CapabilityLimits;
use crate::ids::{is_valid_identifier, is_valid_uuid};
use crate::json::to_sorted_json;
use crate::limits::{
    UrlRejection, unicode_scalar_len, valid_activity_key, valid_artwork_url, valid_public_https_url,
};
use crate::time::format_rfc3339_millis;
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::BTreeSet;

pub const PRESENCE_SCHEMA: &str = "yohaku.companion.presence";
pub const PRESENCE_SCHEMA_VERSION: i64 = 2;

pub const MAX_DISPLAY_NAME: usize = 120;
pub const MAX_CUSTOM_LABEL: usize = 80;
pub const MAX_WINDOW_TITLE: usize = 500;
pub const MAX_MEDIA_TEXT: usize = 300;
pub const MAX_PLAYER_NAME: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Idle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Music,
    Podcast,
    Video,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearReason {
    Paused,
    Sleep,
    Shutdown,
    PrivacyChanged,
    ConnectionRemoved,
}

impl ClearReason {
    pub fn wire(&self) -> &'static str {
        match self {
            ClearReason::Paused => "paused",
            ClearReason::Sleep => "sleep",
            ClearReason::Shutdown => "shutdown",
            ClearReason::PrivacyChanged => "privacyChanged",
            ClearReason::ConnectionRemoved => "connectionRemoved",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MapperError {
    #[error("field {0} too long")]
    FieldTooLong(&'static str),
    #[error("invalid {0}")]
    FieldInvalid(&'static str),
    #[error("media identity missing")]
    IdentityMissing,
    #[error("payload too large")]
    PayloadTooLarge,
}

#[derive(Debug, Clone)]
pub struct PresenceSnapshotInput {
    pub request_id: String,
    pub device_id: String,
    pub sequence: i64,
    pub observed_at: DateTime<Utc>,
    pub lease_ttl_seconds: i64,
    pub availability: Availability,
    pub application: Option<ApplicationPart>,
    pub media: Option<MediaPart>,
}

#[derive(Debug, Clone)]
pub struct ApplicationPart {
    pub display_name: String,
    pub activity_key: Option<String>,
    pub activity_custom_label: Option<String>,
    pub window_title: Option<String>,
    pub icon_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MediaPart {
    pub session_id: String,
    pub kind: MediaKind,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player_display_name: Option<String>,
    pub state: PlaybackState,
    pub duration_seconds: Option<f64>,
    pub position_seconds: Option<f64>,
    pub sampled_at: DateTime<Utc>,
    pub rate: f64,
    pub artwork_url: Option<String>,
    pub link_url: Option<String>,
    pub includes_artwork: bool,
    pub includes_link: bool,
}

mod wire;
use wire::{
    ActivityWire, ApplicationWire, ClearDataWire, LeaseWire, MediaWire, MetaWire, PlaybackWire,
    PlayerWire, PresenceDataWire, RequestWire, UrlWire, WindowWire,
};

#[derive(Debug, Clone)]
pub struct Mapper {
    limits: CapabilityLimits,
    allowed_asset_hosts: BTreeSet<String>,
}

fn meaningful(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|v| !v.is_empty())
}

fn seconds_to_ms(v: f64, field: &'static str) -> Result<i64, MapperError> {
    if !v.is_finite() || v < 0.0 {
        return Err(MapperError::FieldInvalid(field));
    }
    let ms = (v * 1000.0).round();
    if ms > MAX_SAFE_INTEGER as f64 {
        Err(MapperError::FieldInvalid(field))
    } else {
        Ok(ms as i64)
    }
}

fn asset_error(reject: UrlRejection) -> MapperError {
    match reject {
        UrlRejection::NotHttps | UrlRejection::HasUserinfo | UrlRejection::TooLong => {
            MapperError::FieldInvalid("assetUrl")
        }
        UrlRejection::HostNotAllowed => MapperError::FieldInvalid("assetHost"),
        UrlRejection::InvalidQuery => MapperError::FieldInvalid("artworkQuery"),
    }
}

impl Mapper {
    pub fn new(limits: CapabilityLimits, allowed_asset_hosts: BTreeSet<String>) -> Self {
        Mapper {
            limits,
            allowed_asset_hosts,
        }
    }

    pub fn clamp_lease(&self, requested: i64) -> i64 {
        requested.clamp(
            self.limits.presence_lease_min_seconds,
            self.limits.presence_lease_max_seconds,
        )
    }

    fn build_meta<'a>(
        &self,
        request_id: &'a str,
        device_id: &'a str,
        sequence: i64,
        observed_at: DateTime<Utc>,
    ) -> Result<MetaWire<'a>, MapperError> {
        if !is_valid_identifier(request_id) {
            return Err(MapperError::FieldInvalid("requestId"));
        }
        if !is_valid_identifier(device_id) {
            return Err(MapperError::FieldInvalid("deviceId"));
        }
        if !(0..=MAX_SAFE_INTEGER).contains(&sequence) {
            return Err(MapperError::FieldInvalid("sequence"));
        }
        Ok(MetaWire {
            schema: PRESENCE_SCHEMA,
            schema_version: PRESENCE_SCHEMA_VERSION,
            request_id,
            device_id,
            sequence,
            observed_at: format_rfc3339_millis(observed_at),
        })
    }

    fn build_application<'a>(
        &self,
        app: &'a ApplicationPart,
    ) -> Result<ApplicationWire<'a>, MapperError> {
        if app.display_name.is_empty() || unicode_scalar_len(&app.display_name) > MAX_DISPLAY_NAME {
            return Err(MapperError::FieldTooLong("displayName"));
        }
        if let Some(key) = &app.activity_key
            && !valid_activity_key(key)
        {
            return Err(MapperError::FieldInvalid("activityKey"));
        }
        if let Some(label) = &app.activity_custom_label
            && unicode_scalar_len(label) > MAX_CUSTOM_LABEL
        {
            return Err(MapperError::FieldTooLong("customLabel"));
        }
        if let Some(title) = &app.window_title
            && unicode_scalar_len(title) > MAX_WINDOW_TITLE
        {
            return Err(MapperError::FieldTooLong("windowTitle"));
        }
        let icon = match &app.icon_url {
            Some(url) => {
                valid_public_https_url(url, &self.allowed_asset_hosts).map_err(asset_error)?;
                Some(UrlWire { url })
            }
            None => None,
        };
        let activity = if app.activity_key.is_some() || app.activity_custom_label.is_some() {
            Some(ActivityWire {
                key: app.activity_key.as_deref(),
                custom_label: app.activity_custom_label.as_deref(),
            })
        } else {
            None
        };
        Ok(ApplicationWire {
            display_name: &app.display_name,
            activity,
            window: app.window_title.as_deref().map(|t| WindowWire { title: t }),
            icon,
        })
    }

    fn build_media<'a>(&self, media: &'a MediaPart) -> Result<MediaWire<'a>, MapperError> {
        validate_media_identity(media)?;
        let playback = build_playback(media)?;
        let artwork = match (media.includes_artwork, &media.artwork_url) {
            (false, _) => None,
            (true, None) => Some(None),
            (true, Some(url)) => {
                valid_artwork_url(url).map_err(asset_error)?;
                Some(Some(UrlWire { url }))
            }
        };
        let link = match (media.includes_link, &media.link_url) {
            (false, _) => None,
            (true, None) => Some(None),
            (true, Some(url)) => {
                valid_public_https_url(url, &BTreeSet::new()).map_err(asset_error)?;
                Some(Some(UrlWire { url }))
            }
        };
        Ok(MediaWire {
            session_id: &media.session_id,
            kind: match media.kind {
                MediaKind::Music => "music",
                MediaKind::Podcast => "podcast",
                MediaKind::Video => "video",
                MediaKind::Unknown => "unknown",
            },
            title: media.title.as_deref(),
            artist: media.artist.as_deref(),
            album: media.album.as_deref(),
            player: media
                .player_display_name
                .as_deref()
                .map(|p| PlayerWire { display_name: p }),
            playback,
            artwork,
            link,
        })
    }

    fn encode(&self, wire: &impl Serialize) -> Result<String, MapperError> {
        let body = to_sorted_json(wire).map_err(|_| MapperError::FieldInvalid("serialize"))?;
        if body.len() > self.limits.presence_payload_bytes as usize {
            return Err(MapperError::PayloadTooLarge);
        }
        Ok(body)
    }

    pub fn build_presence(&self, input: &PresenceSnapshotInput) -> Result<String, MapperError> {
        let meta = self.build_meta(
            &input.request_id,
            &input.device_id,
            input.sequence,
            input.observed_at,
        )?;
        let data = PresenceDataWire {
            availability: match input.availability {
                Availability::Idle => "idle",
                Availability::Active => "active",
            },
            lease: LeaseWire {
                ttl_seconds: self.clamp_lease(input.lease_ttl_seconds),
            },
            application: input
                .application
                .as_ref()
                .map(|app| self.build_application(app))
                .transpose()?,
            media: input
                .media
                .as_ref()
                .map(|m| self.build_media(m))
                .transpose()?,
        };
        self.encode(&RequestWire { meta, data })
    }

    pub fn build_clear(
        &self,
        request_id: &str,
        device_id: &str,
        sequence: i64,
        reason: ClearReason,
        observed_at: DateTime<Utc>,
    ) -> Result<String, MapperError> {
        let meta = self.build_meta(request_id, device_id, sequence, observed_at)?;
        self.encode(&RequestWire {
            meta,
            data: ClearDataWire {
                reason: reason.wire(),
            },
        })
    }
}

fn validate_media_identity(media: &MediaPart) -> Result<(), MapperError> {
    if !is_valid_uuid(&media.session_id) {
        return Err(MapperError::FieldInvalid("sessionId"));
    }
    for (name, value) in [
        ("mediaTitle", &media.title),
        ("mediaArtist", &media.artist),
        ("mediaAlbum", &media.album),
    ] {
        if let Some(v) = value
            && unicode_scalar_len(v) > MAX_MEDIA_TEXT
        {
            return Err(MapperError::FieldTooLong(name));
        }
    }
    if let Some(player) = &media.player_display_name
        && unicode_scalar_len(player) > MAX_PLAYER_NAME
    {
        return Err(MapperError::FieldTooLong("playerDisplayName"));
    }
    if !meaningful(&media.title) && !meaningful(&media.artist) {
        return Err(MapperError::IdentityMissing);
    }
    Ok(())
}

fn build_playback(media: &MediaPart) -> Result<PlaybackWire, MapperError> {
    let duration_ms = media
        .duration_seconds
        .map(|v| seconds_to_ms(v, "durationMs"))
        .transpose()?;
    let mut position_ms = media
        .position_seconds
        .map(|v| seconds_to_ms(v, "positionMs"))
        .transpose()?;
    if let (Some(duration), Some(position)) = (duration_ms, position_ms) {
        position_ms = Some(position.min(duration));
    }
    if !media.rate.is_finite() || !(0.0..=4.0).contains(&media.rate) {
        return Err(MapperError::FieldInvalid("rate"));
    }
    let state = match media.state {
        PlaybackState::Playing => {
            if media.rate <= 0.0 {
                return Err(MapperError::FieldInvalid("rate"));
            }
            "playing"
        }
        PlaybackState::Paused => {
            if media.rate != 0.0 {
                return Err(MapperError::FieldInvalid("rate"));
            }
            "paused"
        }
    };
    Ok(PlaybackWire {
        state,
        duration_ms,
        position_ms,
        sampled_at: format_rfc3339_millis(media.sampled_at),
        rate: media.rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn uuid() -> String {
        "11111111-1111-4111-8111-111111111111".to_string()
    }

    fn t() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap()
    }

    fn limits() -> CapabilityLimits {
        CapabilityLimits {
            presence_payload_bytes: 32_768,
            presence_requests_per_minute: 10,
            presence_lease_min_seconds: 30,
            presence_lease_max_seconds: 300,
            recommended_heartbeat_seconds: 15,
            maximum_clock_skew_seconds: 60,
        }
    }

    fn hosts() -> BTreeSet<String> {
        BTreeSet::from(["assets.example.com".to_string()])
    }

    fn base_input() -> PresenceSnapshotInput {
        PresenceSnapshotInput {
            request_id: uuid(),
            device_id: uuid(),
            sequence: 7,
            observed_at: t(),
            lease_ttl_seconds: 90,
            availability: Availability::Active,
            application: Some(ApplicationPart {
                display_name: "Edge".into(),
                activity_key: None,
                activity_custom_label: None,
                window_title: Some("Doc — 编辑".into()),
                icon_url: None,
            }),
            media: None,
        }
    }

    fn media_for_test() -> MediaPart {
        MediaPart {
            session_id: uuid(),
            kind: MediaKind::Music,
            title: Some("Song".into()),
            artist: None,
            album: None,
            player_display_name: None,
            state: PlaybackState::Playing,
            duration_seconds: None,
            position_seconds: None,
            sampled_at: t(),
            rate: 1.0,
            artwork_url: None,
            link_url: None,
            includes_artwork: false,
            includes_link: false,
        }
    }

    fn art_url() -> String {
        format!(
            "https://assets.example.com/m/current.png?v={}",
            "ab".repeat(32)
        )
    }

    #[test]
    fn idle_shape_is_byte_exact() {
        let mapper = Mapper::new(limits(), hosts());
        let input = PresenceSnapshotInput {
            availability: Availability::Idle,
            application: None,
            media: None,
            ..base_input()
        };
        let body = mapper.build_presence(&input).unwrap();
        assert_eq!(
            body,
            r#"{"data":{"application":null,"availability":"idle","lease":{"ttlSeconds":90},"media":null},"meta":{"deviceId":"11111111-1111-4111-8111-111111111111","observedAt":"2026-01-02T03:04:05.000Z","requestId":"11111111-1111-4111-8111-111111111111","schema":"yohaku.companion.presence","schemaVersion":2,"sequence":7}}"#
        );
    }

    #[test]
    fn application_null_semantics() {
        let mapper = Mapper::new(limits(), hosts());
        let body = mapper.build_presence(&base_input()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let app = &v["data"]["application"];
        assert_eq!(app["displayName"], "Edge");
        assert_eq!(app["activity"], serde_json::Value::Null);
        assert_eq!(app["window"]["title"], "Doc — 编辑");
        assert_eq!(app["icon"], serde_json::Value::Null);
    }

    #[test]
    fn full_media_shape() {
        let mapper = Mapper::new(limits(), hosts());
        let input = PresenceSnapshotInput {
            media: Some(MediaPart {
                artist: Some("Artist".into()),
                album: None,
                player_display_name: Some("Spotify".into()),
                duration_seconds: Some(201.4),
                position_seconds: Some(40.567),
                artwork_url: Some(art_url()),
                includes_artwork: true,
                ..media_for_test()
            }),
            ..base_input()
        };
        let body = mapper.build_presence(&input).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let media = &v["data"]["media"];
        assert_eq!(media["kind"], "music");
        assert_eq!(media["sessionId"], uuid());
        assert_eq!(media["playback"]["durationMs"], 201400);
        assert_eq!(media["playback"]["positionMs"], 40567);
        assert_eq!(media["playback"]["rate"], 1.0);
        assert_eq!(media["playback"]["state"], "playing");
        assert_eq!(media["artwork"]["url"], art_url());
        assert!(media.get("link").is_none());
        assert_eq!(media["album"], serde_json::Value::Null);
        assert_eq!(media["player"]["displayName"], "Spotify");
        assert!(body.contains(r#""artist":"Artist""#));
    }

    #[test]
    fn artwork_key_semantics() {
        let mapper = Mapper::new(limits(), hosts());
        let media = |includes_artwork, url| MediaPart {
            artwork_url: url,
            includes_artwork,
            ..media_for_test()
        };
        let body_for = |m: &MediaPart| {
            let input = PresenceSnapshotInput {
                media: Some(m.clone()),
                ..base_input()
            };
            let v: serde_json::Value =
                serde_json::from_str(&mapper.build_presence(&input).unwrap()).unwrap();
            v["data"]["media"].clone()
        };
        assert!(body_for(&media(false, None)).get("artwork").is_none());
        assert_eq!(
            body_for(&media(true, None))["artwork"],
            serde_json::Value::Null
        );
        assert!(body_for(&media(true, Some(art_url())))["artwork"]["url"].is_string());
    }

    #[test]
    fn lease_clamped() {
        let mapper = Mapper::new(limits(), hosts());
        assert_eq!(mapper.clamp_lease(1), 30);
        assert_eq!(mapper.clamp_lease(90), 90);
        assert_eq!(mapper.clamp_lease(9_000), 300);
    }

    #[test]
    fn media_validation_keeps_first_error() {
        let mapper = Mapper::new(limits(), hosts());
        let cases = [
            (
                MediaPart {
                    session_id: "invalid".into(),
                    title: Some("x".repeat(1000)),
                    ..media_for_test()
                },
                MapperError::FieldInvalid("sessionId"),
            ),
            (
                MediaPart {
                    title: None,
                    duration_seconds: Some(-1.0),
                    ..media_for_test()
                },
                MapperError::IdentityMissing,
            ),
            (
                MediaPart {
                    duration_seconds: Some(-1.0),
                    rate: f64::NAN,
                    ..media_for_test()
                },
                MapperError::FieldInvalid("durationMs"),
            ),
            (
                MediaPart {
                    rate: f64::NAN,
                    artwork_url: Some("http://invalid".into()),
                    includes_artwork: true,
                    ..media_for_test()
                },
                MapperError::FieldInvalid("rate"),
            ),
        ];
        for (media, expected) in cases {
            let input = PresenceSnapshotInput {
                media: Some(media),
                ..base_input()
            };
            assert_eq!(mapper.build_presence(&input).unwrap_err(), expected);
        }
    }

    #[test]
    fn media_shape_is_byte_exact() {
        let mapper = Mapper::new(limits(), hosts());
        let input = PresenceSnapshotInput {
            application: None,
            media: Some(MediaPart {
                includes_artwork: true,
                includes_link: true,
                ..media_for_test()
            }),
            ..base_input()
        };
        assert_eq!(
            mapper.build_presence(&input).unwrap(),
            r#"{"data":{"application":null,"availability":"active","lease":{"ttlSeconds":90},"media":{"album":null,"artist":null,"artwork":null,"kind":"music","link":null,"playback":{"durationMs":null,"positionMs":null,"rate":1.0,"sampledAt":"2026-01-02T03:04:05.000Z","state":"playing"},"player":null,"sessionId":"11111111-1111-4111-8111-111111111111","title":"Song"}},"meta":{"deviceId":"11111111-1111-4111-8111-111111111111","observedAt":"2026-01-02T03:04:05.000Z","requestId":"11111111-1111-4111-8111-111111111111","schema":"yohaku.companion.presence","schemaVersion":2,"sequence":7}}"#
        );
    }

    #[test]
    fn rejections() {
        let mapper = Mapper::new(limits(), hosts());
        let assert_err = |input: PresenceSnapshotInput, expected: &MapperError| {
            assert_eq!(mapper.build_presence(&input).as_ref(), Err(expected));
        };
        let mut input = base_input();
        input.application.as_mut().unwrap().display_name = "界".repeat(121);
        assert_err(input, &MapperError::FieldTooLong("displayName"));
        let mut input = base_input();
        input.application.as_mut().unwrap().display_name = String::new();
        assert_err(input, &MapperError::FieldTooLong("displayName"));
        let mut input = base_input();
        input.application.as_mut().unwrap().activity_key = Some("1Bad".into());
        assert_err(input, &MapperError::FieldInvalid("activityKey"));
        let mut input = base_input();
        input.application.as_mut().unwrap().icon_url =
            Some("http://assets.example.com/i.png".into());
        assert_err(input, &MapperError::FieldInvalid("assetUrl"));
        let mut input = base_input();
        input.application.as_mut().unwrap().icon_url = Some("https://evil.com/i.png".into());
        assert_err(input, &MapperError::FieldInvalid("assetHost"));
        let mut input = base_input();
        input.media = Some(MediaPart {
            rate: 0.0,
            ..media_for_test()
        });
        assert_err(input, &MapperError::FieldInvalid("rate"));
        let mut input = base_input();
        input.media = Some(MediaPart {
            state: PlaybackState::Paused,
            rate: 1.0,
            ..media_for_test()
        });
        assert_err(input, &MapperError::FieldInvalid("rate"));
        let mut input = base_input();
        input.media = Some(MediaPart {
            duration_seconds: Some(100.0),
            position_seconds: Some(120.0),
            ..media_for_test()
        });
        let body = mapper.build_presence(&input).unwrap();
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["data"]["media"]["playback"]["positionMs"], 100000);
        let mut input = base_input();
        input.media = Some(MediaPart {
            duration_seconds: Some(-1.0),
            ..media_for_test()
        });
        assert_err(input, &MapperError::FieldInvalid("durationMs"));
        let mut input = base_input();
        input.media = Some(MediaPart {
            duration_seconds: Some(f64::NAN),
            ..media_for_test()
        });
        assert_err(input, &MapperError::FieldInvalid("durationMs"));
        let mut input = base_input();
        input.media = Some(MediaPart {
            title: None,
            artist: None,
            ..media_for_test()
        });
        assert_err(input, &MapperError::IdentityMissing);
        let mut input = base_input();
        input.media = Some(MediaPart {
            title: Some(String::new()),
            artist: Some(String::new()),
            ..media_for_test()
        });
        assert_err(input, &MapperError::IdentityMissing);
        let mut input = base_input();
        input.media = Some(MediaPart {
            session_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            ..media_for_test()
        });
        assert_err(input, &MapperError::FieldInvalid("sessionId"));
        let mut input = base_input();
        input.request_id = "not-an-id".into();
        assert_err(input, &MapperError::FieldInvalid("requestId"));
        let mut input = base_input();
        input.sequence = -1;
        assert_err(input, &MapperError::FieldInvalid("sequence"));
        let small = Mapper::new(
            CapabilityLimits {
                presence_payload_bytes: 64,
                ..limits()
            },
            hosts(),
        );
        assert_eq!(
            small.build_presence(&base_input()).unwrap_err(),
            MapperError::PayloadTooLarge
        );
    }

    #[test]
    fn clear_shape() {
        let mapper = Mapper::new(limits(), hosts());
        let body = mapper
            .build_clear(&uuid(), &uuid(), 8, ClearReason::Sleep, t())
            .unwrap();
        assert!(body.contains(r#""data":{"reason":"sleep"}"#));
        assert!(body.contains(r#""schema":"yohaku.companion.presence""#));
    }
}
