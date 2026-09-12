//! 捕获模型：原始状态（永不离开本 crate 的隐私边界）与净化快照（唯一出口）。

use chrono::{DateTime, Utc};
use yohaku_protocol::presence::{Availability, MediaKind};

/// 平台捕获的前台应用原始样本。applicationKey/exe 路径属于原始身份，
/// 不进入 [`SanitizedPresenceSnapshot`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawApplicationFocus {
    /// exe 文件名小写 / UWP AUMID 小写——隐私规则的键
    pub application_key: String,
    pub display_name: String,
    pub window_title: Option<String>,
}

/// 平台捕获的媒体原始样本。构造时即归一化时间线：
/// 非有限/负值 → None；position 超出 duration → None（"无可靠值"语义）。
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
    /// 原始封面字节（由 S3 host 在发送前消费，不进入净化快照）
    pub artwork_bytes: Option<Vec<u8>>,
}

fn normalize_seconds(v: Option<f64>) -> Option<f64> {
    match v {
        Some(v) if v.is_finite() && v >= 0.0 => Some(v),
        _ => None,
    }
}

impl RawMediaState {
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
        artwork_bytes: Option<Vec<u8>>,
    ) -> Self {
        let duration_seconds = normalize_seconds(duration_seconds);
        let mut position_seconds = normalize_seconds(position_seconds);
        if let (Some(duration), Some(position)) = (duration_seconds, position_seconds)
            && position > duration {
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
            artwork_bytes,
        }
    }
}

/// 净化后的应用部分：只有展示名与（可选）窗口标题。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedApplication {
    pub display_name: String,
    pub window_title: Option<String>,
}

/// 净化后的媒体部分：只有协议可见字段 + 稳定 sessionId。
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

/// 唯一允许离开隐私管道的快照形态。
#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedPresenceSnapshot {
    pub availability: Availability,
    pub application: Option<SanitizedApplication>,
    pub media: Option<SanitizedMedia>,
}

/// 生成协议输入的应用部分（复制净化值，不携带原始键）。
pub fn protocol_application_part(
    app: &SanitizedApplication,
    icon_url: Option<String>,
) -> yohaku_protocol::presence::ApplicationPart {
    yohaku_protocol::presence::ApplicationPart {
        display_name: app.display_name.clone(),
        activity_key: None,
        activity_custom_label: None,
        window_title: app.window_title.clone(),
        icon_url,
    }
}

/// 生成协议输入的媒体部分。`artwork_url`/能力标志由发送方注入。
pub fn protocol_media_part(
    media: &SanitizedMedia,
    artwork_url: Option<String>,
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
        artwork_url,
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
            Some(-1.0),     // 负 duration → None
            Some(f64::NAN), // 非有限 → None
            None,
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
            Some(150.0), // position > duration → 钳到 duration
            None,
            None,
        );
        assert_eq!(m.position_seconds, Some(100.0));
    }
}
