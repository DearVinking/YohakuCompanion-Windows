//! 能力协商（macOS 版 CompanionCapabilityNegotiator.negotiatePresence 的移植）。

use crate::capabilities::CapabilityLimits;
use crate::error::CapabilitiesData;
use crate::presence::PRESENCE_SCHEMA_VERSION;
use crate::semver::SemanticVersion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NegotiatedConfig {
    pub limits: CapabilityLimits,
    pub supports_media_timeline: bool,
    pub supports_media_artwork: bool,
    pub supports_media_playback_links: bool,
}

impl NegotiatedConfig {
    /// 心跳间隔 = min(recommended, max(1, 租约/3))；租约为 clamp 后的请求值。
    pub fn heartbeat_seconds(&self, requested_lease: i64) -> i64 {
        let clamped = self.clamp_lease(requested_lease);
        self.limits
            .recommended_heartbeat_seconds
            .min((clamped / 3).max(1))
    }

    pub fn clamp_lease(&self, requested: i64) -> i64 {
        requested.clamp(
            self.limits.presence_lease_min_seconds,
            self.limits.presence_lease_max_seconds,
        )
    }

    pub fn minimum_send_interval_ms(&self) -> i64 {
        60_000 / self.limits.presence_requests_per_minute
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Negotiation {
    Available(NegotiatedConfig),
    ClientUpdateRequired,
    SchemaUnsupported,
    FeatureUnavailable,
    InvalidCapabilities,
}

fn validated_limits(caps: &CapabilitiesData) -> Option<NegotiatedConfig> {
    let limits = caps.limits;
    let ok = limits.presence_payload_bytes > 0
        && limits.presence_requests_per_minute > 0
        && limits.presence_lease_min_seconds > 0
        && limits.presence_lease_min_seconds <= limits.presence_lease_max_seconds
        && limits.recommended_heartbeat_seconds >= limits.presence_lease_min_seconds
        && limits.recommended_heartbeat_seconds <= limits.presence_lease_max_seconds
        && limits.maximum_clock_skew_seconds >= 0
        && caps.presence_schema_versions.iter().all(|v| *v > 0)
        && caps.moment_schema_versions.iter().all(|v| *v > 0);
    if !ok {
        return None;
    }
    Some(NegotiatedConfig {
        limits,
        supports_media_timeline: caps.features.media_timeline,
        supports_media_artwork: caps.features.media_artwork == Some(true),
        supports_media_playback_links: caps.features.media_playback_links == Some(true),
    })
}

pub fn negotiate(caps: &CapabilitiesData, client_version: &str) -> Negotiation {
    // macOS 语义：任何解析失败（含 semver）统一是 invalidCapabilities。
    let Some(client) = SemanticVersion::parse(client_version) else {
        return Negotiation::InvalidCapabilities;
    };
    let Some(minimum) = SemanticVersion::parse(&caps.minimum_client_version) else {
        return Negotiation::InvalidCapabilities;
    };
    let Some(config) = validated_limits(caps) else {
        return Negotiation::InvalidCapabilities;
    };
    if client < minimum {
        return Negotiation::ClientUpdateRequired;
    }
    if !caps
        .presence_schema_versions
        .contains(&PRESENCE_SCHEMA_VERSION)
    {
        return Negotiation::SchemaUnsupported;
    }
    if !caps.features.live_desk {
        return Negotiation::FeatureUnavailable;
    }
    Negotiation::Available(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::FeatureSet;

    fn caps() -> CapabilitiesData {
        CapabilitiesData {
            minimum_client_version: "1.7.3".into(),
            presence_schema_versions: vec![2],
            moment_schema_versions: vec![1],
            features: FeatureSet {
                live_desk: true,
                media_timeline: true,
                moments: false,
                reading_sessions: true,
                media_artwork: Some(true),
                media_playback_links: None,
            },
            limits: CapabilityLimits {
                presence_payload_bytes: 32_768,
                presence_requests_per_minute: 10,
                presence_lease_min_seconds: 30,
                presence_lease_max_seconds: 300,
                recommended_heartbeat_seconds: 90,
                maximum_clock_skew_seconds: 60,
            },
        }
    }

    #[test]
    fn available_with_flags() {
        let result = negotiate(&caps(), "1.7.3");
        let Negotiation::Available(config) = result else {
            panic!("expected available");
        };
        assert!(config.supports_media_timeline);
        assert!(config.supports_media_artwork);
        assert!(!config.supports_media_playback_links);
        assert_eq!(config.heartbeat_seconds(90), 30); // min(90, 90/3)
        assert_eq!(config.heartbeat_seconds(30), 10); // clamp(30)=30 → 30/3
        assert_eq!(config.heartbeat_seconds(300), 90); // min(90, 100) = 90
        assert_eq!(config.minimum_send_interval_ms(), 6_000);
    }

    #[test]
    fn client_too_old() {
        assert_eq!(negotiate(&caps(), "1.7.2"), Negotiation::ClientUpdateRequired);
    }

    #[test]
    fn schema_unsupported() {
        let mut c = caps();
        c.presence_schema_versions = vec![1, 3];
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::SchemaUnsupported);
    }

    #[test]
    fn feature_unavailable() {
        let mut c = caps();
        c.features.live_desk = false;
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::FeatureUnavailable);
    }

    #[test]
    fn invalid_limits() {
        let mut c = caps();
        c.limits.presence_requests_per_minute = 0;
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::InvalidCapabilities);

        let mut c = caps();
        c.limits.recommended_heartbeat_seconds = 10; // < leaseMin(30)
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::InvalidCapabilities);

        let mut c = caps();
        c.limits.presence_lease_min_seconds = 400; // > leaseMax
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::InvalidCapabilities);
    }

    #[test]
    fn invalid_semver_is_invalid_capabilities_not_update_required() {
        assert_eq!(negotiate(&caps(), "not-semver"), Negotiation::InvalidCapabilities);
        let mut c = caps();
        c.minimum_client_version = "bad".into();
        assert_eq!(negotiate(&c, "1.7.3"), Negotiation::InvalidCapabilities);
    }
}
