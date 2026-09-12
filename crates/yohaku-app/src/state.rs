//! 协调器状态快照（前端轮询的载体）。

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CoordinatorState {
    /// 未配对或未开启
    Disabled,
    /// 能力协商中
    Connecting,
    Active,
    Degraded,
    /// 客户端版本低于服务端最低要求
    UpdateRequired,
    /// 服务端 schema/特性不可用
    ServerFeatureUnavailable,
    /// 睡眠/锁屏/手动暂停
    Suspended,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LiveDeskStatus {
    pub state: CoordinatorState,
    /// 固定错误码（刻意最小化，不含快照内容/端点/响应体）
    pub last_error_code: Option<String>,
    /// RFC3339 UTC 毫秒（chrono DateTime 不便直穿 specta，前端再解析）
    pub last_sent_at: Option<String>,
    pub media_capable: bool,
    pub artwork_capable: bool,
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
            artwork_capable: false,
            server_base_url: None,
            device_id: None,
        }
    }
}
