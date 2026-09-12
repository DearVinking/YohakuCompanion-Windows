//! 应用层错误：领域错误（thiserror）与命令边界错误（ApiError，序列化到前端）。

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("not paired")]
    NotPaired,
    #[error("live desk is disabled")]
    LiveDeskDisabled,
    #[error("preview consent required")]
    ConsentRequired,
    #[error("capability negotiation failed")]
    NegotiationFailed,
    #[error("transport error: {0}")]
    Transport(String),
    #[error("server error {code}: {message}")]
    Server { code: String, message: String },
    #[error("store error: {0}")]
    Store(#[from] yohaku_store::StoreError),
    #[error("{0}")]
    Other(String),
}

/// 命令边界错误：code 是前端分支依据（对齐 NTEye 的 ApiError 约定）。
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
            AppError::ConsentRequired => "CONSENT_REQUIRED",
            AppError::NegotiationFailed => "NEGOTIATION_FAILED",
            AppError::Transport(_) => "TRANSPORT",
            AppError::Server { .. } => "SERVER",
            AppError::Store(_) => "STORE",
            AppError::Other(_) => "INTERNAL",
        };
        ApiError::new(code, e.to_string())
    }
}

impl From<StoreErrorAlias> for ApiError {
    fn from(e: StoreErrorAlias) -> Self {
        ApiError::new("STORE", e.0)
    }
}

/// 避免在公共 API 中暴露 thiserror 类型别名的一层薄别名。
pub struct StoreErrorAlias(pub String);

impl From<yohaku_store::StoreError> for StoreErrorAlias {
    fn from(e: yohaku_store::StoreError) -> Self {
        StoreErrorAlias(e.to_string())
    }
}
