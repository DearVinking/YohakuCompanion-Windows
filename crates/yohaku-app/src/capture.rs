use chrono::{DateTime, Utc};
use yohaku_protocol::presence::{Availability, MediaKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawApplicationFocus {
    pub application_key: String,
    pub display_name: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawMediaState {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player_key: String,
    pub player_display_name: String,
    pub playing: bool,
    pub duration_seconds: Option<f64>,
    pub position_seconds: Option<f64>,
    pub position_sampled_at: Option<DateTime<Utc>>,
}

fn normalize_seconds(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite() && *v >= 0.0)
}

impl RawMediaState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: Option<String>,
        artist: Option<String>,
        album: Option<String>,
        player_key: String,
        player_display_name: String,
        playing: bool,
        duration_seconds: Option<f64>,
        position_seconds: Option<f64>,
        position_sampled_at: Option<DateTime<Utc>>,
    ) -> Self {
        let duration_seconds = normalize_seconds(duration_seconds);
        let mut position_seconds = normalize_seconds(position_seconds);
        if let (Some(duration), Some(position)) = (duration_seconds, position_seconds)
            && position > duration
        {
            position_seconds = Some(duration);
        }
        RawMediaState {
            title,
            artist,
            album,
            player_key,
            player_display_name,
            playing,
            duration_seconds,
            position_seconds,
            position_sampled_at,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedApplication {
    pub display_name: String,
    pub window_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedMedia {
    pub session_id: String,
    pub kind: MediaKind,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub player_display_name: Option<String>,
    pub playing: bool,
    pub duration_seconds: Option<f64>,
    pub position_seconds: Option<f64>,
    pub position_sampled_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedPresenceSnapshot {
    pub availability: Availability,
    pub application: Option<SanitizedApplication>,
    pub media: Option<SanitizedMedia>,
}

pub fn protocol_application_part(
    app: &SanitizedApplication,
) -> yohaku_protocol::presence::ApplicationPart {
    yohaku_protocol::presence::ApplicationPart {
        display_name: app.display_name.clone(),
        activity_key: None,
        activity_custom_label: None,
        window_title: app.window_title.clone(),
        icon_url: None,
    }
}

pub fn protocol_media_part(
    media: &SanitizedMedia,
    includes_artwork: bool,
) -> yohaku_protocol::presence::MediaPart {
    yohaku_protocol::presence::MediaPart {
        session_id: media.session_id.clone(),
        kind: media.kind,
        title: media.title.clone(),
        artist: media.artist.clone(),
        album: media.album.clone(),
        player_display_name: media.player_display_name.clone(),
        state: if media.playing {
            yohaku_protocol::presence::PlaybackState::Playing
        } else {
            yohaku_protocol::presence::PlaybackState::Paused
        },
        duration_seconds: media.duration_seconds,
        position_seconds: media.position_seconds,
        sampled_at: media.position_sampled_at.unwrap_or_else(Utc::now),
        rate: if media.playing { 1.0 } else { 0.0 },
        artwork_url: None,
        link_url: None,
        includes_artwork,
        includes_link: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_timeline_normalization() {
        let m = RawMediaState::new(
            Some("t".into()),
            None,
            None,
            "player".into(),
            "Player".into(),
            true,
            Some(-1.0),
            Some(f64::NAN),
            None,
        );
        assert_eq!(m.duration_seconds, None);
        assert_eq!(m.position_seconds, None);

        let m = RawMediaState::new(
            Some("t".into()),
            None,
            None,
            "player".into(),
            "Player".into(),
            true,
            Some(100.0),
            Some(150.0),
            None,
        );
        assert_eq!(m.position_seconds, Some(100.0));
    }
}
