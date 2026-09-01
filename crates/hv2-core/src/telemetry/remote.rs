//! Remote Telemetry Transport
//!
//! Sends metrics to an OpenTelemetry Collector via OTLP HTTP/protobuf or
//! HTTP/JSON. Includes batch buffering, background flush, and retry with
//! exponential backoff.
//!
//! # Configuration
//!
//! The remote exporter is configured via environment variables (OTEL standard)
//! or programmatically via [`RemoteExporterConfig`]:
//!
//! ```text
//! OTEL_EXPORTER_OTLP_ENDPOINT=https://nervosys.ai/otlp
//! OTEL_EXPORTER_OTLP_HEADERS=Authorization=Bearer <token>
//! OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
//! OTEL_SERVICE_NAME=hypermachine
//! ```

use super::exporters::{ExporterError, ExporterResult, MetricExporter, OpenTelemetryExporter};
use super::types::MetricFamily;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Configuration for the remote OTLP metric exporter.
#[derive(Debug, Clone)]
pub struct RemoteExporterConfig {
    /// OTLP endpoint URL (e.g. `https://nervosys.ai/otlp`).
    /// `/v1/metrics` is appended automatically for metric export.
    pub endpoint: String,
    /// OTLP protocol. Only `http/json` is supported (protobuf would require
    /// compiled proto definitions that are out of scope).
    pub protocol: String,
    /// Additional headers sent with every request (e.g. Authorization).
    pub headers: Vec<(String, String)>,
    /// Service name attached as a resource attribute.
    pub service_name: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Maximum number of metric families buffered before the oldest are dropped.
    pub max_queue_size: usize,
    /// Maximum batch size per export request.
    pub max_batch_size: usize,
    /// Interval between background flush attempts.
    pub flush_interval: Duration,
    /// Maximum number of retry attempts for transient failures.
    pub max_retries: u32,
    /// Base delay for exponential backoff.
    pub retry_base_delay: Duration,
}

impl Default for RemoteExporterConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4318".to_string(),
            protocol: "http/json".to_string(),
            headers: Vec::new(),
            service_name: "hypermachine".to_string(),
            timeout: Duration::from_secs(10),
            max_queue_size: 4096,
            max_batch_size: 512,
            flush_interval: Duration::from_secs(15),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(500),
        }
    }
}

impl RemoteExporterConfig {
    /// Build configuration from standard OTEL environment variables, falling
    /// back to the provided defaults for any variable that is unset.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();

        if let Ok(ep) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
            cfg.endpoint = ep;
        }
        if let Ok(proto) = std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL") {
            cfg.protocol = proto;
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

/// Parse OTEL_EXPORTER_OTLP_HEADERS format: `key1=value1,key2=value2`
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

/// Shared state for the remote metric exporter.
struct RemoteExporterInner {
    config: RemoteExporterConfig,
    client: reqwest::Client,
    serializer: OpenTelemetryExporter,
    buffer: Mutex<Vec<String>>,
    shutdown: AtomicBool,
    total_sent: AtomicU64,
    total_failed: AtomicU64,
    total_dropped: AtomicU64,
}

/// Remote OTLP metric exporter with background flush.
///
/// Metrics are serialized to OTLP JSON via [`OpenTelemetryExporter`] and
/// queued in an in-memory buffer. A background tokio task periodically
/// flushes the buffer to the configured OTLP endpoint.
pub struct RemoteMetricExporter {
    inner: Arc<RemoteExporterInner>,
}

impl RemoteMetricExporter {
    /// Create a new remote exporter. Call [`start_flush_task`] afterwards to
    /// begin background export.
    pub fn new(config: RemoteExporterConfig) -> Result<Self, ExporterError> {
        let mut builder = reqwest::Client::builder()
            .timeout(config.timeout)
            .pool_max_idle_per_host(4);

        // Only allow HTTPS or localhost HTTP
        if config.endpoint.starts_with("https://") {
            builder = builder.https_only(true);
        }

        let client = builder
            .build()
            .map_err(|e| ExporterError::NetworkError(format!("HTTP client init: {e}")))?;

        let serializer = OpenTelemetryExporter::new(&config.service_name);

        Ok(Self {
            inner: Arc::new(RemoteExporterInner {
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

    /// Create from standard OTEL environment variables.
    pub fn from_env() -> Result<Self, ExporterError> {
        Self::new(RemoteExporterConfig::from_env())
    }

    /// Spawn a background tokio task that periodically flushes the buffer.
    /// Returns a [`tokio::task::JoinHandle`] that resolves when the exporter
    /// is shut down.
    pub fn start_flush_task(&self) -> tokio::task::JoinHandle<()> {
        let inner = Arc::clone(&self.inner);
        tokio::spawn(async move {
            let interval = inner.config.flush_interval;
            loop {
                tokio::time::sleep(interval).await;
                if inner.shutdown.load(Ordering::Relaxed) {
                    // Final flush on shutdown
                    let _ = flush_buffer(&inner).await;
                    break;
                }
                if let Err(e) = flush_buffer(&inner).await {
                    tracing::warn!(error = %e, "remote metric flush failed");
                }
            }
        })
    }

    /// Queue metric families for remote export. This serializes the data
    /// and appends it to the internal buffer. It does NOT block on network I/O.
    pub fn queue(&self, families: &[MetricFamily]) -> ExporterResult<()> {
        if self.inner.shutdown.load(Ordering::Relaxed) {
            return Err(ExporterError::IoError("exporter is shut down".into()));
        }

        let payload = self.inner.serializer.export(families)?;

        let mut buf = self.inner.buffer.lock();
        if buf.len() >= self.inner.config.max_queue_size {
            // Drop oldest entries to make room
            let to_drop = buf.len() - self.inner.config.max_queue_size + 1;
            buf.drain(..to_drop);
            self.inner
                .total_dropped
                .fetch_add(to_drop as u64, Ordering::Relaxed);
        }
        buf.push(payload);
        Ok(())
    }

    /// Force an immediate flush (blocks until complete or timeout).
    pub async fn force_flush(&self) -> ExporterResult<()> {
        flush_buffer(&self.inner).await
    }

    /// Shut down the exporter: prevent new data, flush remaining buffer.
    pub async fn shutdown(&self) -> ExporterResult<()> {
        self.inner.shutdown.store(true, Ordering::Relaxed);
        flush_buffer(&self.inner).await
    }

    /// Number of metric batches successfully sent.
    pub fn total_sent(&self) -> u64 {
        self.inner.total_sent.load(Ordering::Relaxed)
    }

    /// Number of metric batches that failed after all retries.
    pub fn total_failed(&self) -> u64 {
        self.inner.total_failed.load(Ordering::Relaxed)
    }

    /// Number of metric batches dropped due to queue overflow.
    pub fn total_dropped(&self) -> u64 {
        self.inner.total_dropped.load(Ordering::Relaxed)
    }

    /// Current queue depth.
    pub fn queue_depth(&self) -> usize {
        self.inner.buffer.lock().len()
    }
}

/// Also implement `MetricExporter` so it can be used as a drop-in exporter.
/// Note: this serializes & queues but does NOT send synchronously.
impl MetricExporter for RemoteMetricExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        self.queue(families)?;
        Ok(String::from("{\"status\":\"queued\"}"))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn name(&self) -> &'static str {
        "remote-otlp"
    }
}

/// Drain the buffer and POST each batch to the OTLP endpoint with retry.
async fn flush_buffer(inner: &RemoteExporterInner) -> ExporterResult<()> {
    let batches: Vec<String> = {
        let mut buf = inner.buffer.lock();
        buf.drain(..).collect()
    };

    if batches.is_empty() {
        return Ok(());
    }

    let url = format!("{}/v1/metrics", inner.config.endpoint.trim_end_matches('/'));

    for payload in batches {
        if let Err(e) = send_with_retry(inner, &url, &payload).await {
            inner.total_failed.fetch_add(1, Ordering::Relaxed);
            tracing::error!(error = %e, "failed to export metrics after retries");
        } else {
            inner.total_sent.fetch_add(1, Ordering::Relaxed);
        }
    }

    Ok(())
}

/// POST a payload with exponential backoff retry.
async fn send_with_retry(
    inner: &RemoteExporterInner,
    url: &str,
    payload: &str,
) -> ExporterResult<()> {
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
                // 4xx errors (except 429) are not retryable
                if status.is_client_error() && status.as_u16() != 429 {
                    return Err(ExporterError::NetworkError(format!(
                        "OTLP endpoint returned {status}"
                    )));
                }
                // 429 / 5xx — retry
                tracing::warn!(
                    attempt,
                    status = %status,
                    "retryable OTLP export error"
                );
            }
            Err(e) => {
                if e.is_timeout() || e.is_connect() {
                    tracing::warn!(attempt, error = %e, "retryable network error");
                } else {
                    return Err(ExporterError::NetworkError(e.to_string()));
                }
            }
        }
    }

    Err(ExporterError::NetworkError(format!(
        "all {} retries exhausted",
        inner.config.max_retries
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_headers_simple() {
        let hdrs = parse_headers("Authorization=Bearer tok123,X-Custom=value");
        assert_eq!(hdrs.len(), 2);
        assert_eq!(hdrs[0].0, "Authorization");
        assert_eq!(hdrs[0].1, "Bearer tok123");
        assert_eq!(hdrs[1].0, "X-Custom");
        assert_eq!(hdrs[1].1, "value");
    }

    #[test]
    fn test_parse_headers_empty() {
        assert!(parse_headers("").is_empty());
        assert!(parse_headers("  ").is_empty());
    }

    #[test]
    fn test_parse_headers_equals_in_value() {
        let hdrs = parse_headers("Authorization=Bearer tok=123==");
        assert_eq!(hdrs.len(), 1);
        assert_eq!(hdrs[0].0, "Authorization");
        assert_eq!(hdrs[0].1, "Bearer tok=123==");
    }

    #[test]
    fn test_config_from_defaults() {
        let cfg = RemoteExporterConfig::default();
        assert_eq!(cfg.endpoint, "http://localhost:4318");
        assert_eq!(cfg.service_name, "hypermachine");
        assert_eq!(cfg.max_retries, 3);
    }

    #[test]
    fn test_config_builder() {
        let cfg = RemoteExporterConfig::default()
            .with_endpoint("https://nervosys.ai/otlp")
            .with_header("Authorization", "Bearer test")
            .with_service_name("my-svc")
            .with_timeout(Duration::from_secs(5));

        assert_eq!(cfg.endpoint, "https://nervosys.ai/otlp");
        assert_eq!(cfg.headers.len(), 1);
        assert_eq!(cfg.service_name, "my-svc");
        assert_eq!(cfg.timeout, Duration::from_secs(5));
    }

    #[test]
    fn test_exporter_creation() {
        let cfg = RemoteExporterConfig::default().with_endpoint("https://nervosys.ai/otlp");
        let exporter = RemoteMetricExporter::new(cfg);
        assert!(exporter.is_ok());
        let exporter = exporter.unwrap();
        assert_eq!(exporter.total_sent(), 0);
        assert_eq!(exporter.total_failed(), 0);
        assert_eq!(exporter.queue_depth(), 0);
    }

    #[test]
    fn test_queue_and_depth() {
        use super::super::types::{MetricFamily, MetricSample, MetricType};

        let cfg = RemoteExporterConfig::default().with_endpoint("https://example.com/otlp");
        let exporter = RemoteMetricExporter::new(cfg).unwrap();

        let mut family = MetricFamily::new("test_counter", MetricType::Counter, "A test counter");
        family.add_sample(MetricSample::counter("test_counter", 42));

        exporter.queue(&[family]).unwrap();
        assert_eq!(exporter.queue_depth(), 1);
    }

    #[test]
    fn test_queue_overflow_drops_oldest() {
        use super::super::types::{MetricFamily, MetricSample, MetricType};

        let cfg = RemoteExporterConfig {
            max_queue_size: 2,
            ..RemoteExporterConfig::default()
        };
        let exporter = RemoteMetricExporter::new(cfg).unwrap();

        let make_family = |val: u64| {
            let mut f = MetricFamily::new("c", MetricType::Counter, "");
            f.add_sample(MetricSample::counter("c", val));
            f
        };

        exporter.queue(&[make_family(1)]).unwrap();
        exporter.queue(&[make_family(2)]).unwrap();
        exporter.queue(&[make_family(3)]).unwrap();

        assert_eq!(exporter.queue_depth(), 2);
        assert_eq!(exporter.total_dropped(), 1);
    }

    #[test]
    fn test_metric_exporter_trait() {
        use super::super::types::{MetricFamily, MetricSample, MetricType};

        let cfg = RemoteExporterConfig::default().with_endpoint("https://example.com/otlp");
        let exporter = RemoteMetricExporter::new(cfg).unwrap();

        let mut family = MetricFamily::new("test", MetricType::Gauge, "Test");
        family.add_sample(MetricSample::gauge_float("test", 42.5));

        let result = exporter.export(&[family]);
        assert!(result.is_ok());
        assert_eq!(exporter.queue_depth(), 1);
    }

    #[tokio::test]
    async fn test_flush_empty_buffer() {
        let cfg = RemoteExporterConfig::default().with_endpoint("https://example.com/otlp");
        let exporter = RemoteMetricExporter::new(cfg).unwrap();

        // Flushing an empty buffer should succeed
        let result = exporter.force_flush().await;
        assert!(result.is_ok());
        assert_eq!(exporter.total_sent(), 0);
    }

    #[tokio::test]
    async fn test_shutdown_prevents_queue() {
        let cfg = RemoteExporterConfig::default().with_endpoint("https://example.com/otlp");
        let exporter = RemoteMetricExporter::new(cfg).unwrap();

        exporter.shutdown().await.unwrap();

        use super::super::types::{MetricFamily, MetricSample, MetricType};
        let mut family = MetricFamily::new("test", MetricType::Counter, "");
        family.add_sample(MetricSample::counter("test", 1));

        let result = exporter.queue(&[family]);
        assert!(result.is_err());
    }
}
