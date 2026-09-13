use serde::Serialize;

#[derive(Serialize)]
pub(super) struct MetaWire<'a> {
    pub(super) schema: &'static str,
    #[serde(rename = "schemaVersion")]
    pub(super) schema_version: i64,
    #[serde(rename = "requestId")]
    pub(super) request_id: &'a str,
    #[serde(rename = "deviceId")]
    pub(super) device_id: &'a str,
    pub(super) sequence: i64,
    #[serde(rename = "observedAt")]
    pub(super) observed_at: String,
}

#[derive(Serialize)]
pub(super) struct LeaseWire {
    #[serde(rename = "ttlSeconds")]
    pub(super) ttl_seconds: i64,
}

#[derive(Serialize)]
pub(super) struct ActivityWire<'a> {
    pub(super) key: Option<&'a str>,
    #[serde(rename = "customLabel")]
    pub(super) custom_label: Option<&'a str>,
}

#[derive(Serialize)]
pub(super) struct WindowWire<'a> {
    pub(super) title: &'a str,
}

#[derive(Serialize)]
pub(super) struct UrlWire<'a> {
    pub(super) url: &'a str,
}

#[derive(Serialize)]
pub(super) struct ApplicationWire<'a> {
    #[serde(rename = "displayName")]
    pub(super) display_name: &'a str,
    pub(super) activity: Option<ActivityWire<'a>>,
    pub(super) window: Option<WindowWire<'a>>,
    pub(super) icon: Option<UrlWire<'a>>,
}

#[derive(Serialize)]
pub(super) struct PlayerWire<'a> {
    #[serde(rename = "displayName")]
    pub(super) display_name: &'a str,
}

#[derive(Serialize)]
pub(super) struct PlaybackWire {
    pub(super) state: &'static str,
    #[serde(rename = "durationMs")]
    pub(super) duration_ms: Option<i64>,
    #[serde(rename = "positionMs")]
    pub(super) position_ms: Option<i64>,
    #[serde(rename = "sampledAt")]
    pub(super) sampled_at: String,
    pub(super) rate: f64,
}

#[derive(Serialize)]
pub(super) struct MediaWire<'a> {
    #[serde(rename = "sessionId")]
    pub(super) session_id: &'a str,
    pub(super) kind: &'static str,
    pub(super) title: Option<&'a str>,
    pub(super) artist: Option<&'a str>,
    pub(super) album: Option<&'a str>,
    pub(super) player: Option<PlayerWire<'a>>,
    pub(super) playback: PlaybackWire,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) artwork: Option<Option<UrlWire<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) link: Option<Option<UrlWire<'a>>>,
}

#[derive(Serialize)]
pub(super) struct PresenceDataWire<'a> {
    pub(super) availability: &'static str,
    pub(super) lease: LeaseWire,
    pub(super) application: Option<ApplicationWire<'a>>,
    pub(super) media: Option<MediaWire<'a>>,
}

#[derive(Serialize)]
pub(super) struct ClearDataWire {
    pub(super) reason: &'static str,
}

#[derive(Serialize)]
pub(super) struct RequestWire<'a, D: Serialize> {
    pub(super) meta: MetaWire<'a>,
    pub(super) data: D,
}
