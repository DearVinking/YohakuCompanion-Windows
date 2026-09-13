use std::sync::Arc;
use std::time::Duration;
use yohaku_app::ports::{HttpRequest, HttpResponse, HttpTransport, TransportError};
use yohaku_store::ConnectionStore;

pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .new_agent();
        UreqTransport { agent }
    }
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTransport for UreqTransport {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let response = match request.method {
            "GET" => {
                let mut b = self.agent.get(&request.url);
                for (key, value) in &request.headers {
                    b = b.header(key, value);
                }
                b.config()
                    .timeout_global(Some(Duration::from_millis(request.timeout_ms)))
                    .build()
                    .call()
            }
            "PUT" | "POST" => {
                let builder = if request.method == "PUT" {
                    self.agent.put(&request.url)
                } else {
                    self.agent.post(&request.url)
                };
                let mut b = builder;
                for (key, value) in &request.headers {
                    b = b.header(key, value);
                }
                let body = request.body.as_deref().unwrap_or_default();
                b.config()
                    .timeout_global(Some(Duration::from_millis(request.timeout_ms)))
                    .build()
                    .send(body)
            }
            other => return Err(TransportError(format!("unsupported method {other}"))),
        }
        .map_err(|e| TransportError(e.to_string()))?;
        let status = response.status().as_u16();
        let body = response
            .into_body()
            .read_to_vec()
            .map_err(|e| TransportError(e.to_string()))?;
        Ok(HttpResponse { status, body })
    }
}

pub(super) struct DurablePresenceTransport {
    inner: Arc<dyn HttpTransport>,
    connection: Arc<ConnectionStore>,
}

#[derive(serde::Deserialize)]
struct PresenceEnvelope {
    meta: PresenceSequence,
    data: PresenceData,
}

#[derive(serde::Deserialize)]
struct PresenceData {
    reason: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct PresenceSequence {
    device_id: String,
    sequence: i64,
}
impl DurablePresenceTransport {
    pub(super) fn new(inner: Arc<dyn HttpTransport>, connection: Arc<ConnectionStore>) -> Self {
        Self { inner, connection }
    }
}
impl HttpTransport for DurablePresenceTransport {
    fn send(&self, mut request: HttpRequest) -> Result<HttpResponse, TransportError> {
        if request.method == "PUT" {
            let started = std::time::Instant::now();
            let envelope: PresenceEnvelope =
                serde_json::from_slice(request.body.as_deref().unwrap_or_default())
                    .map_err(|_| TransportError("invalid presence request metadata".into()))?;
            let persisted = self.connection.load_metadata().map_err(|error| {
                TransportError(format!("presence persistence unavailable: {error}"))
            })?;
            let removed_connection_clear = persisted
                .as_ref()
                .is_none_or(|metadata| metadata.device_id != envelope.meta.device_id)
                && envelope.data.reason.as_deref()
                    == Some(yohaku_protocol::presence::ClearReason::ConnectionRemoved.wire());
            let durable = persisted.is_some_and(|metadata| {
                metadata.device_id == envelope.meta.device_id
                    && metadata
                        .next_sequence
                        .is_some_and(|next| next > envelope.meta.sequence)
            });
            if !durable && !removed_connection_clear {
                return Err(TransportError(
                    "presence sequence has not been persisted for this device".into(),
                ));
            }
            let remaining = Duration::from_millis(request.timeout_ms)
                .checked_sub(started.elapsed())
                .filter(|duration| !duration.is_zero())
                .ok_or_else(|| TransportError("presence request budget exhausted".into()))?;
            request.timeout_ms = (remaining.as_millis() as u64).max(1);
        }
        self.inner.send(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    fn response_server(status: u16, delay: Duration) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                if stream.read(&mut byte).unwrap() == 0 {
                    return;
                }
                headers.push(byte[0]);
            }
            std::thread::sleep(delay);
            let response = format!(
                "HTTP/1.1 {status} Fixture\r\nContent-Length: 7\r\nConnection: close\r\n\r\nfixture"
            );
            let _ = stream.write_all(response.as_bytes());
        });
        (format!("http://{address}"), worker)
    }

    fn request(url: String, timeout_ms: u64) -> HttpRequest {
        HttpRequest {
            method: "GET",
            url,
            headers: Vec::new(),
            body: None,
            timeout_ms,
        }
    }

    #[test]
    fn transport_preserves_error_status_and_body() {
        let transport = UreqTransport::new();
        for status in [200, 401, 409, 426, 429, 500, 503] {
            let (url, worker) = response_server(status, Duration::ZERO);
            let result = transport.send(request(url, 1_000));
            worker.join().unwrap();
            let response = result
                .unwrap_or_else(|error| panic!("HTTP {status} became transport error: {error}"));
            assert_eq!(response.status, status);
            assert_eq!(response.body, b"fixture");
        }
    }

    #[test]
    fn transport_honors_request_timeout() {
        let (url, worker) = response_server(200, Duration::from_millis(300));
        let result = UreqTransport::new().send(request(url, 30));
        worker.join().unwrap();
        assert!(result.is_err(), "slow response ignored per-request timeout");
    }

    struct RecordingTransport(std::sync::atomic::AtomicUsize);
    impl HttpTransport for RecordingTransport {
        fn send(&self, _: HttpRequest) -> Result<HttpResponse, TransportError> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(HttpResponse {
                status: 200,
                body: Vec::new(),
            })
        }
    }
    struct MemorySecret(std::sync::Mutex<Option<Vec<u8>>>);
    impl yohaku_store::SecretStore for MemorySecret {
        fn set(&self, _: &str, value: &[u8]) -> yohaku_store::StoreResult<()> {
            *self.0.lock().unwrap() = Some(value.to_vec());
            Ok(())
        }
        fn get(&self, _: &str) -> yohaku_store::StoreResult<Option<Vec<u8>>> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn remove(&self, _: &str) -> yohaku_store::StoreResult<()> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    fn presence_request(device_id: &str, sequence: i64) -> HttpRequest {
        let mut request = request("https://core/companion/presence".into(), 1_000);
        request.method = "PUT";
        request.body = Some(
            serde_json::to_vec(&serde_json::json!({
                "meta": { "deviceId": device_id, "sequence": sequence }, "data": {}
            }))
            .unwrap(),
        );
        request
    }

    #[test]
    fn unpersisted_or_wrong_device_sequence_never_reaches_transport() {
        let dir = tempfile::tempdir().unwrap();
        let connection = Arc::new(ConnectionStore::new(
            dir.path(),
            Arc::new(MemorySecret(std::sync::Mutex::new(None))),
        ));
        connection
            .install_pairing_claim("device", "token", &[], 7, "https://core")
            .unwrap();
        let inner = Arc::new(RecordingTransport(std::sync::atomic::AtomicUsize::new(0)));
        let transport = DurablePresenceTransport::new(inner.clone(), connection.clone());
        assert!(transport.send(presence_request("device", 7)).is_err());
        assert!(transport.send(presence_request("other", 6)).is_err());
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        connection
            .update_metadata(|m| m.next_sequence = Some(8))
            .unwrap();
        assert!(transport.send(presence_request("device", 7)).is_ok());
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 1);
        std::fs::remove_file(dir.path().join(yohaku_store::connection::CONNECTION_FILE)).unwrap();
        assert!(transport.send(presence_request("device", 7)).is_err());
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn removed_connection_can_send_its_final_clear_only() {
        let dir = tempfile::tempdir().unwrap();
        let connection = Arc::new(ConnectionStore::new(
            dir.path(),
            Arc::new(MemorySecret(std::sync::Mutex::new(None))),
        ));
        let inner = Arc::new(RecordingTransport(std::sync::atomic::AtomicUsize::new(0)));
        let transport = DurablePresenceTransport::new(inner.clone(), connection);
        let mut request = presence_request("removed-device", 7);
        request.body = Some(br#"{"meta":{"deviceId":"removed-device","sequence":7},"data":{"reason":"connectionRemoved"}}"#.to_vec());
        assert!(transport.send(request).is_ok());
        assert!(
            transport
                .send(presence_request("removed-device", 7))
                .is_err()
        );
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn retired_connection_can_clear_after_a_new_pair_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        let connection = Arc::new(ConnectionStore::new(
            dir.path(),
            Arc::new(MemorySecret(std::sync::Mutex::new(None))),
        ));
        connection
            .install_pairing_claim("new-device", "new-token", &[], 1, "https://new-core")
            .unwrap();
        let inner = Arc::new(RecordingTransport(std::sync::atomic::AtomicUsize::new(0)));
        let transport = DurablePresenceTransport::new(inner.clone(), connection);
        let mut request = presence_request("old-device", 7);
        request.body = Some(br#"{"meta":{"deviceId":"old-device","sequence":7},"data":{"reason":"connectionRemoved"}}"#.to_vec());
        assert!(transport.send(request).is_ok());
        assert!(transport.send(presence_request("old-device", 7)).is_err());
        assert_eq!(inner.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
