//! Remote Span Transport
//!
//! Sends trace spans to an OpenTelemetry Collector via OTLP HTTP/JSON.
//! Includes batch buffering, background flush, and retry with exponential
//! backoff.
//!
//! # Configuration
//!
//! Re-uses the same OTEL environment variables as the metric exporter:
//!
//! ```text
//! OTEL_EXPORTER_OTLP_ENDPOINT=https://nervosys.ai/otlp
//! OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer <token>
//! OTEL_SERVICE_NAME=hypermachine
//! ```

use super::exporters::OtlpSpanExporter;
use super::tracer::{SpanExporter, TracerError};
use super::types::SpanData;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for the remote OTLP span exporter.
#[derive(Debug, Clone)]
pub struct RemoteSpanExporterConfig {
    /// OTLP endpoint URL. `/v1/traces` is appended automatically.
    pub endpoint: String,
    /// Additional headers (e.g. Authorization).
    pub headers: Vec<(String, String)>,
    /// Service name attached as a resource attribute.
    pub service_name: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Maximum spans buffered before the oldest are dropped.
    pub max_queue_size: usize,
    /// Maximum spans per export request.
    pub max_batch_size: usize,
    /// Interval between background flush attempts.
    pub flush_interval: Duration,
    /// Maximum retry attempts for transient failures.
    pub max_retries: u32,
    /// Base delay for exponential backoff.
    pub retry_base_delay: Duration,
}

impl Default for RemoteSpanExporterConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4318".to_string(),
            headers: Vec::new(),
            service_name: "hypermachine".to_string(),
            timeout: Duration::from_secs(10),
            max_queue_size: 8192,
            max_batch_size: 512,
            flush_interval: Duration::from_secs(5),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(500),
        }
    }
}

impl RemoteSpanExporterConfig {
    /// Build configuration from standard OTEL environment variables.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        if let Ok(ep) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            cfg.endpoint = ep;
        }
        if let Ok(hdrs) = std::env::var("OTEL_EXPORTER_OTLP_HEADERS") {
            cfg.headers = parse_headers(&hdrs);
        }
        if let Ok(svc) = std::env::var("OTEL_SERVICE_NAME") {
            cfg.service_name = svc;
        }
        cfg
    }

    /// Set the OTLP endpoint.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Add a header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Set the service name.
    pub fn with_service_name(mut self, name: impl Into<String>) -> Self {
        self.service_name = name.into();
        self
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Parse `key1=value1,key2=value2` header format.
fn parse_headers(raw: &str) -> Vec<(String, String)> {
    raw.split(',')
        .filter_map(|pair| {
            let pair = pair.trim();
            let idx = pair.find('=')?;
            let key = pair[..idx].trim().to_string();
            let value = pair[idx + 1..].trim().to_string();
            if key.is_empty() {
                None
            } else {
                Some((key, value))
            }
        })
        .collect()
}

/// Shared inner state.
struct RemoteSpanInner {
    config: RemoteSpanExporterConfig,
    client: reqwest::Client,
    /// Temporary OTLP serializer (borrows the JSON serialization logic)
    serializer: OtlpSpanExporter,
    buffer: Mutex<Vec<SpanData>>,
    shutdown: AtomicBool,
    total_sent: AtomicU64,
    total_failed: AtomicU64,
    total_dropped: AtomicU64,
}

/// Remote OTLP span exporter with background flush.
///
/// Implements [`SpanExporter`] so it can be used as a drop-in replacement
/// for the stub `OtlpSpanExporter`.
pub struct RemoteSpanExporter {
    inner: Arc<RemoteSpanInner>,
}

impl RemoteSpanExporter {
    /// Create a new remote span exporter.
    pub fn new(config: RemoteSpanExporterConfig) -> Result<Self, TracerError> {
        let mut builder = reqwest::Client::builder()
            .timeout(config.timeout)
            .pool_max_idle_per_host(4);

        if config.endpoint.starts_with("https://") {
            builder = builder.https_only(true);
        }

        let client = builder
            .build()
            .map_err(|e| TracerError::ExportError(format!("HTTP client init: {e}")))?;

        // We create an OtlpSpanExporter just to borrow its serialize_json method
        let otlp_config = super::exporters::OtlpConfig::new(&config.endpoint);
        let serializer = OtlpSpanExporter::new(otlp_config);

        Ok(Self {
            inner: Arc::new(RemoteSpanInner {
                config,
                client,
                serializer,
                buffer: Mutex::new(Vec::new()),
                shutdown: AtomicBool::new(false),
                total_sent: AtomicU64::new(0),
                total_failed: AtomicU64::new(0),
                total_dropped: AtomicU64::new(0),
            }),
        })
    }

    /// Create from OTEL environment variables.
    pub fn from_env() -> Result<Self, TracerError> {
        Self::new(RemoteSpanExporterConfig::from_env())
    }

    /// Spawn a background tokio task that periodically flushes spans.
    pub fn start_flush_task(&self) -> tokio::task::JoinHandle<()> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let interval = inner.config.flush_interval;
            loop {
                tokio::time::sleep(interval).await;
                if inner.shutdown.load(Ordering::Relaxed) {
                    let _ = flush_spans(&inner).await;
                    break;
                }
                if let Err(e) = flush_spans(&inner).await {
                    tracing::warn!(error = %e, "remote span flush failed");
                }
            }
        })
    }

    /// Force an immediate flush.
    pub async fn force_flush(&self) -> Result<(), TracerError> {
        flush_spans(&self.inner).await
    }

    /// Shut down the exporter.
    pub async fn async_shutdown(&self) -> Result<(), TracerError> {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        flush_spans(&self.inner).await
    }

    /// Number of span batches sent successfully.
    pub fn total_sent(&self) -> u64 {
        self.inner.total_sent.load(Ordering::Relaxed)
    }

    /// Number of span batches that failed.
    pub fn total_failed(&self) -> u64 {
        self.inner.total_failed.load(Ordering::Relaxed)
    }

    /// Number of spans dropped due to queue overflow.
    pub fn total_dropped(&self) -> u64 {
        self.inner.total_dropped.load(Ordering::Relaxed)
    }

    /// Current queue depth.
    pub fn queue_depth(&self) -> usize {
        self.inner.buffer.lock().len()
    }
}

impl SpanExporter for RemoteSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return Err(TracerError::ExportError("exporter is shut down".into()));
        }

        let mut buf = self.inner.buffer.lock();
        let overflow = (buf.len() + spans.len()).saturating_sub(self.inner.config.max_queue_size);
        if overflow > 0 {
            buf.drain(..overflow);
            self.inner
                .total_dropped
                .fetch_add(overflow as u64, Ordering::Relaxed);
        }
        buf.extend(spans);
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        Ok(())
    }
}

/// Drain buffered spans and POST them in batches to the OTLP endpoint.
async fn flush_spans(inner: &RemoteSpanInner) -> Result<(), TracerError> {
    let spans: Vec<SpanData> = {
        let mut buf = inner.buffer.lock();
        buf.drain(..).collect()
    };

    if spans.is_empty() {
        return Ok(());
    }

    let url = format!(
        "{}/v1/traces",
        inner.config.endpoint.trim_end_matches('/')
    );

    // Send in batches
    for chunk in spans.chunks(inner.config.max_batch_size) {
        let payload = inner.serializer.serialize_json(chunk);

        if let Err(e) = send_with_retry(inner, &url, &payload).await {
            inner.total_failed.fetch_add(1, Ordering::Relaxed);
            tracing::error!(error = %e, "failed to export spans after retries");
        } else {
            inner.total_sent.fetch_add(1, Ordering::Relaxed);
        }
    }

    Ok(())
}

/// POST a payload with exponential backoff retry.
async fn send_with_retry(
    inner: &RemoteSpanInner,
    url: &str,
    payload: &str,
) -> Result<(), TracerError> {
    let mut delay = inner.config.retry_base_delay;

    for attempt in 0..=inner.config.max_retries {
        if attempt > 0 {
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(30));
        }

        let mut req = inner
            .client
            .post(url)
            .header("Content-Type", "application/json")
            .body(payload.to_owned());

        for (k, v) in &inner.config.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    return Ok(());
                }
                if status.is_client_error() && status.as_u16() != 429 {
                    return Err(TracerError::ExportError(format!(
                        "OTLP endpoint returned {status}"
                    )));
                }
                tracing::warn!(attempt, status = %status, "retryable OTLP trace export error");
            }
            Err(e) => {
                if e.is_timeout() || e.is_connect() {
                    tracing::warn!(attempt, error = %e, "retryable network error");
                } else {
                    return Err(TracerError::ExportError(e.to_string()));
                }
            }
        }
    }

    Err(TracerError::ExportError(format!(
        "all {} retries exhausted",
        inner.config.max_retries
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_headers() {
        let hdrs = parse_headers("Authorization=Bearer tok,X-Custom=val");
        assert_eq!(hdrs.len(), 2);
        assert_eq!(hdrs[0].0, "Authorization");
        assert_eq!(hdrs[0].1, "Bearer tok");
    }

    #[test]
    fn test_config_defaults() {
        let cfg = RemoteSpanExporterConfig::default();
        assert_eq!(cfg.endpoint, "http://localhost:4318");
        assert_eq!(cfg.service_name, "hypermachine");
        assert_eq!(cfg.max_retries, 3);
    }

    #[test]
    fn test_config_builder() {
        let cfg = RemoteSpanExporterConfig::default()
            .with_endpoint("https://nervosys.ai/otlp")
            .with_header("Authorization", "Bearer test")
            .with_service_name("my-svc")
            .with_timeout(Duration::from_secs(5));

        assert_eq!(cfg.endpoint, "https://nervosys.ai/otlp");
        assert_eq!(cfg.headers.len(), 1);
        assert_eq!(cfg.service_name, "my-svc");
    }

    #[test]
    fn test_exporter_creation() {
        let cfg = RemoteSpanExporterConfig::default()
            .with_endpoint("https://nervosys.ai/otlp");
        let exporter = RemoteSpanExporter::new(cfg);
        assert!(exporter.is_ok());
    }

    #[test]
    fn test_span_buffer_and_overflow() {
        use super::super::types::{SpanContext, SpanKind, SpanStatus, StatusCode};
        use std::collections::HashMap;

        let cfg = RemoteSpanExporterConfig {
            max_queue_size: 3,
            ..RemoteSpanExporterConfig::default()
        };
        let exporter = RemoteSpanExporter::new(cfg).unwrap();

        let make_span = |name: &str| SpanData {
            context: SpanContext::root(),
            parent_span_id: None,
            name: name.to_string(),
            kind: SpanKind::Internal,
            start_time: 1000,
            end_time: 2000,
            attributes: vec![],
            events: vec![],
            links: vec![],
            status: SpanStatus {
                code: StatusCode::Ok,
                message: String::new(),
            },
            resource: HashMap::new(),
            instrumentation_name: String::new(),
            instrumentation_version: String::new(),
        };

        // Fill buffer to capacity
        exporter
            .export(vec![make_span("a"), make_span("b"), make_span("c")])
            .unwrap();
        assert_eq!(exporter.queue_depth(), 3);

        // Overflow: should drop oldest
        exporter.export(vec![make_span("d"), make_span("e")]).unwrap();
        assert_eq!(exporter.queue_depth(), 3);
        assert_eq!(exporter.total_dropped(), 2);
    }

    #[test]
    fn test_shutdown_prevents_export() {
        let cfg = RemoteSpanExporterConfig::default();
        let exporter = RemoteSpanExporter::new(cfg).unwrap();
        exporter.shutdown().unwrap();

        use super::super::types::{SpanContext, SpanKind, SpanStatus, StatusCode};
        use std::collections::HashMap;
        let span = SpanData {
            context: SpanContext::root(),
            parent_span_id: None,
            name: "test".to_string(),
            kind: SpanKind::Internal,
            start_time: 1000,
            end_time: 2000,
            attributes: vec![],
            events: vec![],
            links: vec![],
            status: SpanStatus {
                code: StatusCode::Ok,
                message: String::new(),
            },
            resource: HashMap::new(),
            instrumentation_name: String::new(),
            instrumentation_version: String::new(),
        };
        assert!(exporter.export(vec![span]).is_err());
    }

    #[tokio::test]
    async fn test_flush_empty() {
        let cfg = RemoteSpanExporterConfig::default()
            .with_endpoint("https://example.com/otlp");
        let exporter = RemoteSpanExporter::new(cfg).unwrap();
        assert!(exporter.force_flush().await.is_ok());
        assert_eq!(exporter.total_sent(), 0);
    }
}
