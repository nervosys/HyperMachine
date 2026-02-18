//! Telemetry and observability for AI agent VM operations
//!
//! This module provides comprehensive monitoring capabilities for AI agents,
//! including metrics collection, tracing, and event tracking.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};

/// Telemetry collection result
pub type TelemetryResult<T> = Result<T, TelemetryError>;

/// Telemetry error types
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TelemetryError {
    /// Metric not found
    #[error("Metric not found: {0}")]
    MetricNotFound(String),
    /// Invalid metric value
    #[error("Invalid value: {0}")]
    InvalidValue(String),
    /// Buffer overflow
    #[error("Telemetry buffer overflow")]
    BufferOverflow,
    /// Export failed
    #[error("Export failed: {0}")]
    ExportFailed(String),
}

/// Metric type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricType {
    /// Monotonically increasing counter
    Counter,
    /// Point-in-time value
    Gauge,
    /// Distribution of values
    Histogram,
    /// Rate of change
    Rate,
}

/// Metric unit for display and aggregation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetricUnit {
    /// No unit
    None,
    /// Count
    Count,
    /// Bytes
    Bytes,
    /// Milliseconds
    Milliseconds,
    /// Microseconds
    Microseconds,
    /// Nanoseconds
    Nanoseconds,
    /// Percentage (0-100)
    Percent,
    /// Operations per second
    OpsPerSec,
    /// Bytes per second
    BytesPerSec,
}

impl std::fmt::Display for MetricUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MetricUnit::None => write!(f, ""),
            MetricUnit::Count => write!(f, "count"),
            MetricUnit::Bytes => write!(f, "bytes"),
            MetricUnit::Milliseconds => write!(f, "ms"),
            MetricUnit::Microseconds => write!(f, "µs"),
            MetricUnit::Nanoseconds => write!(f, "ns"),
            MetricUnit::Percent => write!(f, "%"),
            MetricUnit::OpsPerSec => write!(f, "ops/s"),
            MetricUnit::BytesPerSec => write!(f, "B/s"),
        }
    }
}

/// Metric metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricInfo {
    /// Metric name
    pub name: String,
    /// Human-readable description
    pub description: String,
    /// Metric type
    pub metric_type: MetricType,
    /// Metric unit
    pub unit: MetricUnit,
    /// Labels/tags
    pub labels: HashMap<String, String>,
}

impl MetricInfo {
    /// Create new metric info
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            metric_type: MetricType::Gauge,
            unit: MetricUnit::None,
            labels: HashMap::new(),
        }
    }

    /// Set metric type
    pub fn with_type(mut self, metric_type: MetricType) -> Self {
        self.metric_type = metric_type;
        self
    }

    /// Set metric unit
    pub fn with_unit(mut self, unit: MetricUnit) -> Self {
        self.unit = unit;
        self
    }

    /// Add a label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// A single metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricPoint {
    /// Timestamp when recorded
    pub timestamp: SystemTime,
    /// Metric value
    pub value: f64,
    /// Optional labels for this point
    pub labels: HashMap<String, String>,
}

impl MetricPoint {
    /// Create a new metric point
    pub fn new(value: f64) -> Self {
        Self {
            timestamp: SystemTime::now(),
            value,
            labels: HashMap::new(),
        }
    }

    /// Add a label
    pub fn with_label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.insert(key.into(), value.into());
        self
    }
}

/// Histogram for tracking value distributions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Histogram {
    /// Bucket boundaries
    pub buckets: Vec<f64>,
    /// Count per bucket
    pub counts: Vec<u64>,
    /// Total sum of all values
    pub sum: f64,
    /// Total count
    pub count: u64,
    /// Minimum value
    pub min: f64,
    /// Maximum value
    pub max: f64,
}

impl Histogram {
    /// Create a new histogram with default buckets
    pub fn new() -> Self {
        Self::with_buckets(vec![
            0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ])
    }

    /// Create a histogram with custom buckets
    pub fn with_buckets(buckets: Vec<f64>) -> Self {
        let counts = vec![0; buckets.len() + 1];
        Self {
            buckets,
            counts,
            sum: 0.0,
            count: 0,
            min: f64::MAX,
            max: f64::MIN,
        }
    }

    /// Create histogram with linear buckets
    pub fn linear(start: f64, width: f64, count: usize) -> Self {
        let buckets: Vec<f64> = (0..count).map(|i| start + width * i as f64).collect();
        Self::with_buckets(buckets)
    }

    /// Create histogram with exponential buckets
    pub fn exponential(start: f64, factor: f64, count: usize) -> Self {
        let buckets: Vec<f64> = (0..count).map(|i| start * factor.powi(i as i32)).collect();
        Self::with_buckets(buckets)
    }

    /// Record a value
    pub fn observe(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
        self.min = self.min.min(value);
        self.max = self.max.max(value);

        // Find the right bucket
        let mut found = false;
        for (i, &bound) in self.buckets.iter().enumerate() {
            if value <= bound {
                self.counts[i] += 1;
                found = true;
                break;
            }
        }
        if !found {
            // Goes in the overflow bucket
            if let Some(last) = self.counts.last_mut() {
                *last += 1;
            }
        }
    }

    /// Get the mean value
    pub fn mean(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.sum / self.count as f64
        }
    }

    /// Estimate a percentile (approximate)
    pub fn percentile(&self, p: f64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }

        let target = (self.count as f64 * p / 100.0).ceil() as u64;
        let mut cumulative = 0u64;

        for (i, &count) in self.counts.iter().enumerate() {
            cumulative += count;
            if cumulative >= target {
                if i == 0 {
                    return self.buckets.first().copied().unwrap_or(0.0);
                } else if i < self.buckets.len() {
                    return self.buckets[i];
                } else {
                    return self.max;
                }
            }
        }

        self.max
    }

    /// Reset the histogram
    pub fn reset(&mut self) {
        for count in &mut self.counts {
            *count = 0;
        }
        self.sum = 0.0;
        self.count = 0;
        self.min = f64::MAX;
        self.max = f64::MIN;
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Counter metric (monotonically increasing)
#[derive(Debug, Default)]
pub struct Counter {
    value: AtomicU64,
}

impl Counter {
    /// Create a new counter
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Increment by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment by a value
    pub fn add(&self, v: u64) {
        self.value.fetch_add(v, Ordering::Relaxed);
    }

    /// Get the current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to zero
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }
}

/// Gauge metric (point-in-time value)
#[derive(Debug, Default)]
pub struct Gauge {
    value: AtomicU64,
}

impl Gauge {
    /// Create a new gauge
    pub fn new() -> Self {
        Self {
            value: AtomicU64::new(0),
        }
    }

    /// Set the value
    pub fn set(&self, v: f64) {
        self.value.store(v.to_bits(), Ordering::Relaxed);
    }

    /// Get the current value
    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    /// Increment by a value
    pub fn add(&self, v: f64) {
        loop {
            let current = self.value.load(Ordering::Relaxed);
            let new = (f64::from_bits(current) + v).to_bits();
            if self
                .value
                .compare_exchange(current, new, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Decrement by a value
    pub fn sub(&self, v: f64) {
        self.add(-v);
    }
}

/// Span for tracing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Span ID
    pub id: u64,
    /// Parent span ID (0 if root)
    pub parent_id: u64,
    /// Operation name
    pub name: String,
    /// Start time
    pub start_time: SystemTime,
    /// End time (None if still active)
    pub end_time: Option<SystemTime>,
    /// Duration in microseconds (computed when ended)
    pub duration_us: Option<u64>,
    /// Span attributes
    pub attributes: HashMap<String, String>,
    /// Status
    pub status: SpanStatus,
}

/// Span status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanStatus {
    /// Unset status
    Unset,
    /// Operation succeeded
    Ok,
    /// Operation failed
    Error,
}

impl Span {
    /// Create a new span
    pub fn new(name: impl Into<String>) -> Self {
        static SPAN_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: SPAN_ID.fetch_add(1, Ordering::Relaxed),
            parent_id: 0,
            name: name.into(),
            start_time: SystemTime::now(),
            end_time: None,
            duration_us: None,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
        }
    }

    /// Create a child span
    pub fn child(&self, name: impl Into<String>) -> Self {
        let mut span = Self::new(name);
        span.parent_id = self.id;
        span
    }

    /// Set an attribute
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.attributes.insert(key.into(), value.into());
    }

    /// Set status
    pub fn set_status(&mut self, status: SpanStatus) {
        self.status = status;
    }

    /// End the span
    pub fn end(&mut self) {
        self.end_time = Some(SystemTime::now());
        if let (Ok(start), Some(Ok(end))) = (
            self.start_time.duration_since(SystemTime::UNIX_EPOCH),
            self.end_time
                .map(|t| t.duration_since(SystemTime::UNIX_EPOCH)),
        ) {
            self.duration_us = Some((end - start).as_micros() as u64);
        }
    }

    /// Check if span is still active
    pub fn is_active(&self) -> bool {
        self.end_time.is_none()
    }
}

/// Event record for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEvent {
    /// Event ID
    pub id: u64,
    /// Event timestamp
    pub timestamp: SystemTime,
    /// Event name/type
    pub name: String,
    /// Severity level
    pub level: EventLevel,
    /// Event message
    pub message: String,
    /// Event attributes
    pub attributes: HashMap<String, serde_json::Value>,
    /// Related span ID
    pub span_id: Option<u64>,
}

/// Event severity level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventLevel {
    /// Trace-level detail
    Trace,
    /// Debug information
    Debug,
    /// Informational
    Info,
    /// Warning
    Warn,
    /// Error
    Error,
    /// Fatal error
    Fatal,
}

impl std::fmt::Display for EventLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventLevel::Trace => write!(f, "TRACE"),
            EventLevel::Debug => write!(f, "DEBUG"),
            EventLevel::Info => write!(f, "INFO"),
            EventLevel::Warn => write!(f, "WARN"),
            EventLevel::Error => write!(f, "ERROR"),
            EventLevel::Fatal => write!(f, "FATAL"),
        }
    }
}

impl AgentEvent {
    /// Create a new event
    pub fn new(name: impl Into<String>, level: EventLevel, message: impl Into<String>) -> Self {
        static EVENT_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            id: EVENT_ID.fetch_add(1, Ordering::Relaxed),
            timestamp: SystemTime::now(),
            name: name.into(),
            level,
            message: message.into(),
            attributes: HashMap::new(),
            span_id: None,
        }
    }

    /// Add an attribute
    pub fn with_attribute(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.attributes.insert(key.into(), value);
        self
    }

    /// Set span ID
    pub fn with_span(mut self, span_id: u64) -> Self {
        self.span_id = Some(span_id);
        self
    }
}

/// Telemetry collector configuration
#[derive(Debug, Clone)]
pub struct TelemetryConfig {
    /// Maximum metrics to retain
    pub max_metrics: usize,
    /// Maximum events to retain
    pub max_events: usize,
    /// Maximum spans to retain
    pub max_spans: usize,
    /// Metric retention duration
    pub metric_retention: Duration,
    /// Event retention duration
    pub event_retention: Duration,
    /// Enable detailed tracing
    pub enable_tracing: bool,
    /// Minimum event level to record
    pub min_event_level: EventLevel,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            max_metrics: 10000,
            max_events: 10000,
            max_spans: 1000,
            metric_retention: Duration::from_secs(3600),
            event_retention: Duration::from_secs(3600),
            enable_tracing: true,
            min_event_level: EventLevel::Info,
        }
    }
}

/// Telemetry collector for AI agent operations
#[derive(Debug)]
pub struct TelemetryCollector {
    /// Configuration
    config: TelemetryConfig,
    /// Metric definitions
    metric_info: RwLock<HashMap<String, MetricInfo>>,
    /// Counter metrics
    counters: RwLock<HashMap<String, Arc<Counter>>>,
    /// Gauge metrics
    gauges: RwLock<HashMap<String, Arc<Gauge>>>,
    /// Histogram metrics
    histograms: RwLock<HashMap<String, Histogram>>,
    /// Metric time series
    metric_points: RwLock<HashMap<String, VecDeque<MetricPoint>>>,
    /// Event buffer
    events: RwLock<VecDeque<AgentEvent>>,
    /// Span buffer
    spans: RwLock<VecDeque<Span>>,
    /// Active spans
    active_spans: RwLock<HashMap<u64, Span>>,
    /// Creation time
    created_at: Instant,
}

impl TelemetryCollector {
    /// Create a new telemetry collector
    pub fn new(config: TelemetryConfig) -> Self {
        Self {
            config,
            metric_info: RwLock::new(HashMap::new()),
            counters: RwLock::new(HashMap::new()),
            gauges: RwLock::new(HashMap::new()),
            histograms: RwLock::new(HashMap::new()),
            metric_points: RwLock::new(HashMap::new()),
            events: RwLock::new(VecDeque::new()),
            spans: RwLock::new(VecDeque::new()),
            active_spans: RwLock::new(HashMap::new()),
            created_at: Instant::now(),
        }
    }

    /// Register a counter metric
    pub fn register_counter(&self, info: MetricInfo) -> Arc<Counter> {
        let name = info.name.clone();
        let counter = Arc::new(Counter::new());

        self.metric_info.write().expect("lock poisoned").insert(name.clone(), info);
        self.counters
            .write()
            .expect("lock poisoned")
            .insert(name, Arc::clone(&counter));

        counter
    }

    /// Register a gauge metric
    pub fn register_gauge(&self, info: MetricInfo) -> Arc<Gauge> {
        let name = info.name.clone();
        let gauge = Arc::new(Gauge::new());

        self.metric_info.write().expect("lock poisoned").insert(name.clone(), info);
        self.gauges
            .write()
            .expect("lock poisoned")
            .insert(name, Arc::clone(&gauge));

        gauge
    }

    /// Register a histogram metric
    pub fn register_histogram(&self, info: MetricInfo, histogram: Histogram) {
        let name = info.name.clone();
        self.metric_info.write().expect("lock poisoned").insert(name.clone(), info);
        self.histograms.write().expect("lock poisoned").insert(name, histogram);
    }

    /// Get or create a counter
    pub fn counter(&self, name: &str) -> Arc<Counter> {
        if let Some(counter) = self.counters.read().expect("lock poisoned").get(name) {
            return Arc::clone(counter);
        }

        let info = MetricInfo::new(name, "").with_type(MetricType::Counter);
        self.register_counter(info)
    }

    /// Get or create a gauge
    pub fn gauge(&self, name: &str) -> Arc<Gauge> {
        if let Some(gauge) = self.gauges.read().expect("lock poisoned").get(name) {
            return Arc::clone(gauge);
        }

        let info = MetricInfo::new(name, "").with_type(MetricType::Gauge);
        self.register_gauge(info)
    }

    /// Record a metric point
    pub fn record(&self, name: &str, value: f64) {
        let point = MetricPoint::new(value);
        let mut points = self.metric_points.write().expect("lock poisoned");
        let series = points.entry(name.to_string()).or_default();

        // Enforce size limit
        while series.len() >= self.config.max_metrics {
            series.pop_front();
        }

        series.push_back(point);
    }

    /// Record a histogram observation
    pub fn observe(&self, name: &str, value: f64) {
        if let Some(histogram) = self.histograms.write().expect("lock poisoned").get_mut(name) {
            histogram.observe(value);
        }
    }

    /// Record an event
    pub fn record_event(&self, event: AgentEvent) {
        if event.level < self.config.min_event_level {
            return;
        }

        let mut events = self.events.write().expect("lock poisoned");

        // Enforce size limit
        while events.len() >= self.config.max_events {
            events.pop_front();
        }

        events.push_back(event);
    }

    /// Log helper methods
    pub fn trace(&self, name: &str, message: &str) {
        self.record_event(AgentEvent::new(name, EventLevel::Trace, message));
    }

    pub fn debug(&self, name: &str, message: &str) {
        self.record_event(AgentEvent::new(name, EventLevel::Debug, message));
    }

    pub fn info(&self, name: &str, message: &str) {
        self.record_event(AgentEvent::new(name, EventLevel::Info, message));
    }

    pub fn warn(&self, name: &str, message: &str) {
        self.record_event(AgentEvent::new(name, EventLevel::Warn, message));
    }

    pub fn error(&self, name: &str, message: &str) {
        self.record_event(AgentEvent::new(name, EventLevel::Error, message));
    }

    /// Start a new span
    pub fn start_span(&self, name: impl Into<String>) -> Span {
        if !self.config.enable_tracing {
            return Span::new(name);
        }

        let span = Span::new(name);
        self.active_spans
            .write()
            .expect("lock poisoned")
            .insert(span.id, span.clone());
        span
    }

    /// End a span
    pub fn end_span(&self, mut span: Span) {
        span.end();

        // Remove from active
        self.active_spans.write().expect("lock poisoned").remove(&span.id);

        // Add to completed spans
        let mut spans = self.spans.write().expect("lock poisoned");
        while spans.len() >= self.config.max_spans {
            spans.pop_front();
        }
        spans.push_back(span);
    }

    /// Get recent events
    pub fn recent_events(&self, limit: usize) -> Vec<AgentEvent> {
        let events = self.events.read().expect("lock poisoned");
        events.iter().rev().take(limit).cloned().collect()
    }

    /// Get events by level
    pub fn events_by_level(&self, min_level: EventLevel) -> Vec<AgentEvent> {
        let events = self.events.read().expect("lock poisoned");
        events
            .iter()
            .filter(|e| e.level >= min_level)
            .cloned()
            .collect()
    }

    /// Get recent spans
    pub fn recent_spans(&self, limit: usize) -> Vec<Span> {
        let spans = self.spans.read().expect("lock poisoned");
        spans.iter().rev().take(limit).cloned().collect()
    }

    /// Get metric points
    pub fn get_metric_points(&self, name: &str) -> Option<Vec<MetricPoint>> {
        self.metric_points
            .read()
            .expect("lock poisoned")
            .get(name)
            .map(|v| v.iter().cloned().collect())
    }

    /// Get histogram
    pub fn get_histogram(&self, name: &str) -> Option<Histogram> {
        self.histograms.read().expect("lock poisoned").get(name).cloned()
    }

    /// Get all metric names
    pub fn metric_names(&self) -> Vec<String> {
        self.metric_info.read().expect("lock poisoned").keys().cloned().collect()
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Export all telemetry as JSON
    pub fn export_json(&self) -> serde_json::Value {
        let counters: HashMap<String, u64> = self
            .counters
            .read()
            .expect("lock poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), v.get()))
            .collect();

        let gauges: HashMap<String, f64> = self
            .gauges
            .read()
            .expect("lock poisoned")
            .iter()
            .map(|(k, v)| (k.clone(), v.get()))
            .collect();

        serde_json::json!({
            "uptime_seconds": self.uptime().as_secs(),
            "counters": counters,
            "gauges": gauges,
            "event_count": self.events.read().expect("lock poisoned").len(),
            "span_count": self.spans.read().expect("lock poisoned").len(),
            "active_span_count": self.active_spans.read().expect("lock poisoned").len(),
        })
    }

    /// Clear all telemetry data
    pub fn clear(&self) {
        for counter in self.counters.read().expect("lock poisoned").values() {
            counter.reset();
        }
        self.histograms
            .write()
            .expect("lock poisoned")
            .values_mut()
            .for_each(|h| h.reset());
        self.metric_points.write().expect("lock poisoned").clear();
        self.events.write().expect("lock poisoned").clear();
        self.spans.write().expect("lock poisoned").clear();
        self.active_spans.write().expect("lock poisoned").clear();
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new(TelemetryConfig::default())
    }
}

/// Telemetry snapshot for export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    /// Snapshot timestamp
    pub timestamp: SystemTime,
    /// Uptime in seconds
    pub uptime_seconds: u64,
    /// Counter values
    pub counters: HashMap<String, u64>,
    /// Gauge values
    pub gauges: HashMap<String, f64>,
    /// Histogram summaries
    pub histograms: HashMap<String, HistogramSummary>,
    /// Recent events
    pub recent_events: Vec<AgentEvent>,
    /// Recent spans
    pub recent_spans: Vec<Span>,
}

/// Histogram summary for export
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramSummary {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p90: f64,
    pub p99: f64,
}

impl From<&Histogram> for HistogramSummary {
    fn from(h: &Histogram) -> Self {
        Self {
            count: h.count,
            sum: h.sum,
            min: if h.count > 0 { h.min } else { 0.0 },
            max: if h.count > 0 { h.max } else { 0.0 },
            mean: h.mean(),
            p50: h.percentile(50.0),
            p90: h.percentile(90.0),
            p99: h.percentile(99.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_telemetry_error_display() {
        let err = TelemetryError::MetricNotFound("test".to_string());
        assert!(format!("{}", err).contains("test"));
    }

    #[test]
    fn test_metric_unit_display() {
        assert_eq!(format!("{}", MetricUnit::Bytes), "bytes");
        assert_eq!(format!("{}", MetricUnit::Milliseconds), "ms");
    }

    #[test]
    fn test_metric_info_builder() {
        let info = MetricInfo::new("test_metric", "A test metric")
            .with_type(MetricType::Counter)
            .with_unit(MetricUnit::Count)
            .with_label("env", "test");

        assert_eq!(info.name, "test_metric");
        assert_eq!(info.metric_type, MetricType::Counter);
        assert_eq!(info.labels.get("env"), Some(&"test".to_string()));
    }

    #[test]
    fn test_metric_point() {
        let point = MetricPoint::new(42.0).with_label("host", "localhost");

        assert_eq!(point.value, 42.0);
        assert_eq!(point.labels.get("host"), Some(&"localhost".to_string()));
    }

    #[test]
    fn test_histogram_observe() {
        let mut hist = Histogram::new();
        hist.observe(0.1);
        hist.observe(0.5);
        hist.observe(1.0);

        assert_eq!(hist.count, 3);
        assert!((hist.mean() - 0.533).abs() < 0.01);
        assert_eq!(hist.min, 0.1);
        assert_eq!(hist.max, 1.0);
    }

    #[test]
    fn test_histogram_percentile() {
        let mut hist = Histogram::new();
        for i in 1..=100 {
            hist.observe(i as f64 / 100.0);
        }

        let p50 = hist.percentile(50.0);
        let p99 = hist.percentile(99.0);
        assert!(p50 > 0.0);
        assert!(p99 > p50);
    }

    #[test]
    fn test_histogram_linear() {
        let hist = Histogram::linear(0.0, 10.0, 5);
        assert_eq!(hist.buckets, vec![0.0, 10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn test_histogram_exponential() {
        let hist = Histogram::exponential(1.0, 2.0, 4);
        assert_eq!(hist.buckets, vec![1.0, 2.0, 4.0, 8.0]);
    }

    #[test]
    fn test_counter() {
        let counter = Counter::new();
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.add(5);
        assert_eq!(counter.get(), 6);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_gauge() {
        let gauge = Gauge::new();
        assert_eq!(gauge.get(), 0.0);

        gauge.set(42.5);
        assert_eq!(gauge.get(), 42.5);

        gauge.add(7.5);
        assert_eq!(gauge.get(), 50.0);

        gauge.sub(10.0);
        assert_eq!(gauge.get(), 40.0);
    }

    #[test]
    fn test_span_creation() {
        let span = Span::new("test_operation");
        assert!(span.id > 0);
        assert_eq!(span.parent_id, 0);
        assert!(span.is_active());
    }

    #[test]
    fn test_span_child() {
        let parent = Span::new("parent");
        let child = parent.child("child");

        assert_eq!(child.parent_id, parent.id);
    }

    #[test]
    fn test_span_end() {
        let mut span = Span::new("test");
        assert!(span.is_active());

        span.end();
        assert!(!span.is_active());
        assert!(span.end_time.is_some());
    }

    #[test]
    fn test_span_attributes() {
        let mut span = Span::new("test");
        span.set_attribute("key", "value");
        span.set_status(SpanStatus::Ok);

        assert_eq!(span.attributes.get("key"), Some(&"value".to_string()));
        assert_eq!(span.status, SpanStatus::Ok);
    }

    #[test]
    fn test_event_level_ordering() {
        assert!(EventLevel::Trace < EventLevel::Debug);
        assert!(EventLevel::Debug < EventLevel::Info);
        assert!(EventLevel::Info < EventLevel::Warn);
        assert!(EventLevel::Warn < EventLevel::Error);
        assert!(EventLevel::Error < EventLevel::Fatal);
    }

    #[test]
    fn test_event_level_display() {
        assert_eq!(format!("{}", EventLevel::Info), "INFO");
    }

    #[test]
    fn test_agent_event() {
        let event = AgentEvent::new("test_event", EventLevel::Info, "Test message")
            .with_attribute("key", serde_json::json!("value"))
            .with_span(123);

        assert_eq!(event.name, "test_event");
        assert_eq!(event.level, EventLevel::Info);
        assert_eq!(event.span_id, Some(123));
    }

    #[test]
    fn test_telemetry_config_default() {
        let config = TelemetryConfig::default();
        assert_eq!(config.max_metrics, 10000);
        assert_eq!(config.min_event_level, EventLevel::Info);
    }

    #[test]
    fn test_telemetry_collector_counter() {
        let collector = TelemetryCollector::default();
        let counter = collector.counter("test_counter");

        counter.inc();
        counter.inc();
        counter.add(3);

        assert_eq!(counter.get(), 5);
    }

    #[test]
    fn test_telemetry_collector_gauge() {
        let collector = TelemetryCollector::default();
        let gauge = collector.gauge("test_gauge");

        gauge.set(100.0);
        assert_eq!(gauge.get(), 100.0);
    }

    #[test]
    fn test_telemetry_collector_record() {
        let collector = TelemetryCollector::default();
        collector.record("cpu_usage", 50.0);
        collector.record("cpu_usage", 60.0);

        let points = collector.get_metric_points("cpu_usage").unwrap();
        assert_eq!(points.len(), 2);
    }

    #[test]
    fn test_telemetry_collector_events() {
        let collector = TelemetryCollector::default();
        collector.info("test", "Test info message");
        collector.warn("test", "Test warning");
        collector.error("test", "Test error");

        let events = collector.recent_events(10);
        assert_eq!(events.len(), 3);
    }

    #[test]
    fn test_telemetry_collector_event_filtering() {
        let config = TelemetryConfig {
            min_event_level: EventLevel::Warn,
            ..Default::default()
        };
        let collector = TelemetryCollector::new(config);

        collector.info("test", "Should be filtered");
        collector.warn("test", "Should be recorded");

        let events = collector.recent_events(10);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn test_telemetry_collector_spans() {
        let collector = TelemetryCollector::default();

        let span = collector.start_span("test_operation");
        assert!(span.is_active());

        collector.end_span(span);

        let spans = collector.recent_spans(10);
        assert_eq!(spans.len(), 1);
        assert!(!spans[0].is_active());
    }

    #[test]
    fn test_telemetry_collector_export_json() {
        let collector = TelemetryCollector::default();
        let counter = collector.counter("requests");
        counter.add(100);

        let json = collector.export_json();
        assert!(json.get("counters").is_some());
        assert!(json.get("uptime_seconds").is_some());
    }

    #[test]
    fn test_telemetry_collector_clear() {
        let collector = TelemetryCollector::default();
        let counter = collector.counter("test");
        counter.add(100);
        collector.info("test", "Message");

        collector.clear();

        assert_eq!(counter.get(), 0);
        assert!(collector.recent_events(10).is_empty());
    }

    #[test]
    fn test_histogram_summary() {
        let mut hist = Histogram::new();
        for i in 1..=100 {
            hist.observe(i as f64);
        }

        let summary = HistogramSummary::from(&hist);
        assert_eq!(summary.count, 100);
        assert_eq!(summary.min, 1.0);
        assert_eq!(summary.max, 100.0);
    }

    #[test]
    fn test_telemetry_collector_register_histogram() {
        let collector = TelemetryCollector::default();
        let info = MetricInfo::new("latency", "Request latency")
            .with_type(MetricType::Histogram)
            .with_unit(MetricUnit::Milliseconds);

        collector.register_histogram(info, Histogram::new());
        collector.observe("latency", 10.0);
        collector.observe("latency", 20.0);

        let hist = collector.get_histogram("latency").unwrap();
        assert_eq!(hist.count, 2);
    }

    #[test]
    fn test_telemetry_collector_metric_names() {
        let collector = TelemetryCollector::default();
        collector.counter("counter1");
        collector.gauge("gauge1");

        let names = collector.metric_names();
        assert!(names.contains(&"counter1".to_string()));
        assert!(names.contains(&"gauge1".to_string()));
    }
}
