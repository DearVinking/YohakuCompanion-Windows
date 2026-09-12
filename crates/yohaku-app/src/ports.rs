//! 宿主端口：HTTP 传输与时钟。实现方在壳层注入（ureq / 测试假件）。

use chrono::{DateTime, Utc};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// 传输层失败。DNS/TLS/超时等错误意味着「服务端可能已提交」——
/// 调用方（presence_client）据此做同请求体的一次幂等重试。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("transport failure: {0}")]
pub struct TransportError(pub String);

pub trait HttpTransport: Send + Sync {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError>;
}

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}


/// 单调时钟（毫秒）：限速/心跳/重试等待的计时基准。
/// 独立于 Clock（墙上时间），测试可推进。
pub trait MonotonicClock: Send + Sync {
    fn now_millis(&self) -> u64;
}

pub struct RealMonotonic {
    start: Instant,
}

impl RealMonotonic {
    pub fn new() -> Self {
        RealMonotonic { start: Instant::now() }
    }
}

impl Default for RealMonotonic {
    fn default() -> Self {
        Self::new()
    }
}

impl MonotonicClock for RealMonotonic {
    fn now_millis(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}
