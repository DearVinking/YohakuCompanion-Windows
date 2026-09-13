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

pub trait MonotonicClock: Send + Sync {
    fn now_millis(&self) -> u64;
}

pub struct RealMonotonic {
    start: Instant,
}

impl RealMonotonic {
    pub fn new() -> Self {
        RealMonotonic {
            start: Instant::now(),
        }
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
