#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("请先配对这台电脑。")]
    NotPaired,
    #[error("尚未开启 Live Desk 同步。")]
    LiveDeskDisabled,
    #[error("暂时无法确认服务器是否支持同步，请稍后重试。")]
    NegotiationFailed,
    #[error("无法连接服务器：{0}")]
    Transport(String),
    #[error("服务器无法完成操作（{code}）：{message}")]
    Server { code: String, message: String },
    #[error("无法读取或保存本机数据：{0}")]
    Store(#[from] yohaku_store::StoreError),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, specta::Type)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        ApiError {
            code: code.to_string(),
            message: message.into(),
        }
    }
}

impl From<AppError> for ApiError {
    fn from(e: AppError) -> Self {
        let code = match &e {
            AppError::NotPaired => "NOT_PAIRED",
            AppError::LiveDeskDisabled => "LIVE_DESK_DISABLED",
            AppError::NegotiationFailed => "NEGOTIATION_FAILED",
            AppError::Transport(_) => "TRANSPORT",
            AppError::Server { .. } => "SERVER",
            AppError::Store(_) => "STORE",
            AppError::Other(_) => "INTERNAL",
        };
        ApiError::new(code, e.to_string())
    }
}
