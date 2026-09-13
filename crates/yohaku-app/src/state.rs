use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CoordinatorState {
    Disabled,
    Connecting,
    Active,
    Degraded,
    UpdateRequired,
    ServerFeatureUnavailable,
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveDeskStatus {
    pub state: CoordinatorState,
    pub last_error_code: Option<String>,
    pub last_sent_at: Option<String>,
    pub media_capable: bool,
    pub server_base_url: Option<String>,
    pub device_id: Option<String>,
}

impl Default for LiveDeskStatus {
    fn default() -> Self {
        LiveDeskStatus {
            state: CoordinatorState::Disabled,
            last_error_code: None,
            last_sent_at: None,
            media_capable: false,
            server_base_url: None,
            device_id: None,
        }
    }
}
