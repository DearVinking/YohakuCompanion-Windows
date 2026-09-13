#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapabilityLimits {
    pub presence_payload_bytes: i64,
    pub presence_requests_per_minute: i64,
    pub presence_lease_min_seconds: i64,
    pub presence_lease_max_seconds: i64,
    pub recommended_heartbeat_seconds: i64,
    pub maximum_clock_skew_seconds: i64,
}

impl CapabilityLimits {
    pub fn from_map(map: &serde_json::Map<String, serde_json::Value>) -> Option<CapabilityLimits> {
        Some(CapabilityLimits {
            presence_payload_bytes: crate::json::take_required(map, "presencePayloadBytes").ok()?,
            presence_requests_per_minute: crate::json::take_required(
                map,
                "presenceRequestsPerMinute",
            )
            .ok()?,
            presence_lease_min_seconds: crate::json::take_required(map, "presenceLeaseMinSeconds")
                .ok()?,
            presence_lease_max_seconds: crate::json::take_required(map, "presenceLeaseMaxSeconds")
                .ok()?,
            recommended_heartbeat_seconds: crate::json::take_required(
                map,
                "recommendedHeartbeatSeconds",
            )
            .ok()?,
            maximum_clock_skew_seconds: crate::json::take_required(map, "maximumClockSkewSeconds")
                .ok()?,
        })
    }
}
