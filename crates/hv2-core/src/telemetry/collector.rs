//! Metric Collector
//!
//! Infrastructure for collecting, registering, and querying metrics.

use super::types::{
    HistogramData, MetricFamily, MetricLabels, MetricSample, MetricType, MetricValue,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Error type for collector operations
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CollectorError {
    /// Metric not found
    #[error("Metric not found: {0}")]
    MetricNotFound(String),
    /// Metric already exists
    #[error("Metric already exists: {0}")]
    MetricAlreadyExists(String),
    /// Type mismatch
    #[error("Type mismatch: expected {expected}, got {actual}")]
    TypeMismatch {
        expected: MetricType,
        actual: MetricType,
    },
    /// Invalid operation
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    /// Registry error
    #[error("Registry error: {0}")]
    RegistryError(String),
}

/// Result type for collector operations
pub type CollectorResult<T> = Result<T, CollectorError>;

/// Atomic counter metric
#[derive(Debug)]
pub struct Counter {
    /// Counter value
    value: AtomicU64,
    /// Metric name
    name: String,
    /// Labels
    labels: MetricLabels,
    /// Help text
    help: String,
}

impl Counter {
    /// Create a new counter
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            value: AtomicU64::new(0),
            name: name.into(),
            labels: MetricLabels::new(),
            help: help.into(),
        }
    }

    /// Create with labels
    pub fn with_labels(mut self, labels: MetricLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Increment by 1
    pub fn inc(&self) {
        self.value.fetch_add(1, Ordering::Relaxed);
    }

    /// Add a value
    pub fn add(&self, value: u64) {
        self.value.fetch_add(value, Ordering::Relaxed);
    }

    /// Get current value
    pub fn get(&self) -> u64 {
        self.value.load(Ordering::Relaxed)
    }

    /// Reset to 0
    pub fn reset(&self) {
        self.value.store(0, Ordering::Relaxed);
    }

    /// Convert to a sample
    pub fn to_sample(&self) -> MetricSample {
        MetricSample::counter(&self.name, self.get())
            .with_labels(self.labels.clone())
            .with_help(&self.help)
    }
}

/// Atomic gauge metric (using f64 bits stored as u64)
#[derive(Debug)]
pub struct Gauge {
    /// Gauge value (f64 bits stored as u64)
    value: AtomicU64,
    /// Metric name
    name: String,
    /// Labels
    labels: MetricLabels,
    /// Help text
    help: String,
}

impl Gauge {
    /// Create a new gauge
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            value: AtomicU64::new(0),
            name: name.into(),
            labels: MetricLabels::new(),
            help: help.into(),
        }
    }

    /// Create with labels
    pub fn with_labels(mut self, labels: MetricLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Set value
    pub fn set(&self, value: f64) {
        self.value.store(value.to_bits(), Ordering::Relaxed);
    }

    /// Increment
    pub fn inc(&self) {
        self.add(1.0);
    }

    /// Decrement
    pub fn dec(&self) {
        self.sub(1.0);
    }

    /// Add a value
    pub fn add(&self, value: f64) {
        loop {
            let current = self.value.load(Ordering::Relaxed);
            let new_val = f64::from_bits(current) + value;
            if self
                .value
                .compare_exchange(
                    current,
                    new_val.to_bits(),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                break;
            }
        }
    }

    /// Subtract a value
    pub fn sub(&self, value: f64) {
        self.add(-value);
    }

    /// Get current value
    pub fn get(&self) -> f64 {
        f64::from_bits(self.value.load(Ordering::Relaxed))
    }

    /// Set to current time
    pub fn set_to_current_time(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.set(now);
    }

    /// Convert to a sample
    pub fn to_sample(&self) -> MetricSample {
        MetricSample::gauge_float(&self.name, self.get())
            .with_labels(self.labels.clone())
            .with_help(&self.help)
    }
}

/// Histogram metric with thread-safe updates
#[derive(Debug)]
pub struct Histogram {
    /// Histogram data (protected by RwLock)
    data: RwLock<HistogramData>,
    /// Metric name
    name: String,
    /// Labels
    labels: MetricLabels,
    /// Help text
    help: String,
}

impl Histogram {
    /// Create a new histogram with default buckets
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            data: RwLock::new(HistogramData::new()),
            name: name.into(),
            labels: MetricLabels::new(),
            help: help.into(),
        }
    }

    /// Create with custom buckets
    pub fn with_buckets(name: impl Into<String>, help: impl Into<String>, buckets: &[f64]) -> Self {
        Self {
            data: RwLock::new(HistogramData::with_buckets(buckets)),
            name: name.into(),
            labels: MetricLabels::new(),
            help: help.into(),
        }
    }

    /// Create with labels
    pub fn with_labels(mut self, labels: MetricLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Observe a value
    pub fn observe(&self, value: f64) {
        if let Ok(mut data) = self.data.write() {
            data.observe(value);
        }
    }

    /// Get count
    pub fn count(&self) -> u64 {
        self.data.read().map(|d| d.count).unwrap_or(0)
    }

    /// Get sum
    pub fn sum(&self) -> f64 {
        self.data.read().map(|d| d.sum).unwrap_or(0.0)
    }

    /// Get mean
    pub fn mean(&self) -> Option<f64> {
        self.data.read().ok()?.mean()
    }

    /// Reset the histogram
    pub fn reset(&self) {
        if let Ok(mut data) = self.data.write() {
            data.reset();
        }
    }

    /// Convert to a sample
    pub fn to_sample(&self) -> MetricSample {
        let data = self.data.read().map(|d| d.clone()).unwrap_or_default();
        MetricSample::histogram(&self.name, data)
            .with_labels(self.labels.clone())
            .with_help(&self.help)
    }
}

/// Timer for measuring duration (uses Histogram internally)
#[derive(Debug)]
pub struct Timer {
    /// Underlying histogram
    histogram: Histogram,
}

impl Timer {
    /// Create a new timer
    pub fn new(name: impl Into<String>, help: impl Into<String>) -> Self {
        Self {
            histogram: Histogram::with_buckets(name, help, &super::types::LATENCY_BUCKETS),
        }
    }

    /// Create with labels
    pub fn with_labels(mut self, labels: MetricLabels) -> Self {
        self.histogram = self.histogram.with_labels(labels);
        self
    }

    /// Start a new timer observation
    pub fn start(&self) -> TimerObservation<'_> {
        TimerObservation {
            timer: self,
            start: Instant::now(),
            stopped: false,
        }
    }

    /// Observe a duration directly
    pub fn observe_duration(&self, duration: Duration) {
        self.histogram.observe(duration.as_secs_f64());
    }

    /// Get underlying histogram
    pub fn histogram(&self) -> &Histogram {
        &self.histogram
    }
}

/// Timer observation in progress
pub struct TimerObservation<'a> {
    timer: &'a Timer,
    start: Instant,
    stopped: bool,
}

impl<'a> TimerObservation<'a> {
    /// Stop and record the duration
    pub fn stop(mut self) -> Duration {
        let duration = self.start.elapsed();
        self.timer.observe_duration(duration);
        self.stopped = true;
        duration
    }

    /// Get elapsed time without stopping
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl<'a> Drop for TimerObservation<'a> {
    fn drop(&mut self) {
        if !self.stopped {
            let duration = self.start.elapsed();
            self.timer.observe_duration(duration);
        }
    }
}

/// Metric descriptor for registration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricDescriptor {
    /// Metric name
    pub name: String,
    /// Metric type
    pub metric_type: MetricType,
    /// Help text
    pub help: String,
    /// Unit (optional)
    pub unit: Option<String>,
    /// Label names
    pub label_names: Vec<String>,
}

impl MetricDescriptor {
    /// Create a new metric descriptor
    pub fn new(name: impl Into<String>, metric_type: MetricType, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metric_type,
            help: help.into(),
            unit: None,
            label_names: Vec::new(),
        }
    }

    /// Set unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Set label names
    pub fn with_label_names(mut self, names: Vec<String>) -> Self {
        self.label_names = names;
        self
    }
}

/// Stored metric value in the registry
#[derive(Debug, Clone)]
enum StoredMetric {
    Counter(u64),
    Gauge(f64),
    Histogram(HistogramData),
    Summary(super::types::SummaryData),
    Info(HashMap<String, String>),
}

/// Metric registry for storing and querying metrics
#[derive(Debug)]
pub struct MetricRegistry {
    /// Registered metric descriptors
    descriptors: RwLock<HashMap<String, MetricDescriptor>>,
    /// Stored metrics by name and labels
    metrics: RwLock<HashMap<(String, MetricLabels), StoredMetric>>,
    /// Registry name/prefix
    prefix: Option<String>,
}

impl MetricRegistry {
    /// Create a new registry
    pub fn new() -> Self {
        Self {
            descriptors: RwLock::new(HashMap::new()),
            metrics: RwLock::new(HashMap::new()),
            prefix: None,
        }
    }

    /// Create with a prefix
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            descriptors: RwLock::new(HashMap::new()),
            metrics: RwLock::new(HashMap::new()),
            prefix: Some(prefix.into()),
        }
    }

    /// Get the prefixed name
    fn prefixed_name(&self, name: &str) -> String {
        match &self.prefix {
            Some(p) => format!("{}_{}", p, name),
            None => name.to_string(),
        }
    }

    /// Register a metric descriptor
    pub fn register(&self, descriptor: MetricDescriptor) -> CollectorResult<()> {
        let name = self.prefixed_name(&descriptor.name);

        let mut descriptors = self
            .descriptors
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        if descriptors.contains_key(&name) {
            return Err(CollectorError::MetricAlreadyExists(name));
        }

        let mut desc = descriptor;
        desc.name = name.clone();
        descriptors.insert(name, desc);
        Ok(())
    }

    /// Unregister a metric
    pub fn unregister(&self, name: &str) -> CollectorResult<()> {
        let name = self.prefixed_name(name);

        let mut descriptors = self
            .descriptors
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        descriptors
            .remove(&name)
            .ok_or_else(|| CollectorError::MetricNotFound(name.clone()))?;

        // Also remove all metric values with this name
        let mut metrics = self
            .metrics
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        metrics.retain(|(n, _), _| n != &name);

        Ok(())
    }

    /// Get a metric descriptor
    pub fn get_descriptor(&self, name: &str) -> Option<MetricDescriptor> {
        let name = self.prefixed_name(name);
        self.descriptors.read().ok()?.get(&name).cloned()
    }

    /// List all registered metric names
    pub fn list_metrics(&self) -> Vec<String> {
        self.descriptors
            .read()
            .map(|d| d.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Set a counter value
    pub fn set_counter(&self, name: &str, labels: MetricLabels, value: u64) -> CollectorResult<()> {
        let name = self.prefixed_name(name);

        // Verify type if registered
        if let Some(desc) = self
            .descriptors
            .read()
            .ok()
            .and_then(|d| d.get(&name).cloned())
        {
            if desc.metric_type != MetricType::Counter {
                return Err(CollectorError::TypeMismatch {
                    expected: MetricType::Counter,
                    actual: desc.metric_type,
                });
            }
        }

        let mut metrics = self
            .metrics
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        metrics.insert((name, labels.canonical()), StoredMetric::Counter(value));
        Ok(())
    }

    /// Increment a counter
    pub fn inc_counter(&self, name: &str, labels: MetricLabels) -> CollectorResult<()> {
        self.add_counter(name, labels, 1)
    }

    /// Add to a counter
    pub fn add_counter(&self, name: &str, labels: MetricLabels, value: u64) -> CollectorResult<()> {
        let name = self.prefixed_name(name);
        let key = (name, labels.canonical());

        let mut metrics = self
            .metrics
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        let entry = metrics.entry(key).or_insert(StoredMetric::Counter(0));

        if let StoredMetric::Counter(ref mut v) = entry {
            *v = v.saturating_add(value);
            Ok(())
        } else {
            Err(CollectorError::TypeMismatch {
                expected: MetricType::Counter,
                actual: MetricType::Gauge,
            })
        }
    }

    /// Set a gauge value
    pub fn set_gauge(&self, name: &str, labels: MetricLabels, value: f64) -> CollectorResult<()> {
        let name = self.prefixed_name(name);

        let mut metrics = self
            .metrics
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        metrics.insert((name, labels.canonical()), StoredMetric::Gauge(value));
        Ok(())
    }

    /// Record a histogram observation
    pub fn observe_histogram(
        &self,
        name: &str,
        labels: MetricLabels,
        value: f64,
    ) -> CollectorResult<()> {
        let name = self.prefixed_name(name);
        let key = (name, labels.canonical());

        let mut metrics = self
            .metrics
            .write()
            .map_err(|_| CollectorError::RegistryError("Lock poisoned".to_string()))?;

        let entry = metrics
            .entry(key)
            .or_insert_with(|| StoredMetric::Histogram(HistogramData::new()));

        if let StoredMetric::Histogram(ref mut h) = entry {
            h.observe(value);
            Ok(())
        } else {
            Err(CollectorError::TypeMismatch {
                expected: MetricType::Histogram,
                actual: MetricType::Counter,
            })
        }
    }

    /// Get a counter value
    pub fn get_counter(&self, name: &str, labels: MetricLabels) -> Option<u64> {
        let name = self.prefixed_name(name);
        let key = (name, labels.canonical());

        self.metrics.read().ok()?.get(&key).and_then(|m| {
            if let StoredMetric::Counter(v) = m {
                Some(*v)
            } else {
                None
            }
        })
    }

    /// Get a gauge value
    pub fn get_gauge(&self, name: &str, labels: MetricLabels) -> Option<f64> {
        let name = self.prefixed_name(name);
        let key = (name, labels.canonical());

        self.metrics.read().ok()?.get(&key).and_then(|m| {
            if let StoredMetric::Gauge(v) = m {
                Some(*v)
            } else {
                None
            }
        })
    }

    /// Collect all metrics as families
    pub fn collect(&self) -> Vec<MetricFamily> {
        let descriptors = match self.descriptors.read() {
            Ok(d) => d.clone(),
            Err(_) => return Vec::new(),
        };

        let metrics = match self.metrics.read() {
            Ok(m) => m.clone(),
            Err(_) => return Vec::new(),
        };

        let mut families: HashMap<String, MetricFamily> = HashMap::new();

        for ((name, labels), value) in metrics {
            let desc = descriptors
                .get(&name)
                .cloned()
                .unwrap_or_else(|| MetricDescriptor::new(&name, MetricType::Gauge, ""));

            let family = families
                .entry(name.clone())
                .or_insert_with(|| MetricFamily::new(&name, desc.metric_type, &desc.help));

            let sample = match value {
                StoredMetric::Counter(v) => MetricSample::counter(&name, v).with_labels(labels),
                StoredMetric::Gauge(v) => MetricSample::gauge_float(&name, v).with_labels(labels),
                StoredMetric::Histogram(h) => MetricSample::histogram(&name, h).with_labels(labels),
                StoredMetric::Summary(s) => {
                    MetricSample::new(&name, MetricType::Summary, MetricValue::Summary(s))
                        .with_labels(labels)
                }
                StoredMetric::Info(i) => {
                    MetricSample::new(&name, MetricType::Info, MetricValue::Info(i))
                        .with_labels(labels)
                }
            };

            family.add_sample(sample);
        }

        families.into_values().collect()
    }

    /// Clear all metrics (but keep descriptors)
    pub fn clear(&self) {
        if let Ok(mut metrics) = self.metrics.write() {
            metrics.clear();
        }
    }

    /// Get number of registered metrics
    pub fn metric_count(&self) -> usize {
        self.descriptors.read().map(|d| d.len()).unwrap_or(0)
    }

    /// Get number of stored values
    pub fn value_count(&self) -> usize {
        self.metrics.read().map(|m| m.len()).unwrap_or(0)
    }
}

impl Default for MetricRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global metric collector
#[derive(Debug)]
pub struct MetricCollector {
    /// Underlying registry
    registry: Arc<MetricRegistry>,
    /// Collection interval
    collection_interval: Duration,
    /// Last collection time
    last_collection: RwLock<Option<Instant>>,
}

/// Default metric collection interval.
const DEFAULT_COLLECTION_INTERVAL: Duration = Duration::from_secs(15);

impl MetricCollector {
    /// Create a new collector
    pub fn new() -> Self {
        Self {
            registry: Arc::new(MetricRegistry::new()),
            collection_interval: DEFAULT_COLLECTION_INTERVAL,
            last_collection: RwLock::new(None),
        }
    }

    /// Create with a custom registry
    pub fn with_registry(registry: Arc<MetricRegistry>) -> Self {
        Self {
            registry,
            collection_interval: DEFAULT_COLLECTION_INTERVAL,
            last_collection: RwLock::new(None),
        }
    }

    /// Set collection interval
    pub fn set_collection_interval(&mut self, interval: Duration) {
        self.collection_interval = interval;
    }

    /// Get the registry
    pub fn registry(&self) -> &Arc<MetricRegistry> {
        &self.registry
    }

    /// Register a metric
    pub fn register(&self, descriptor: MetricDescriptor) -> CollectorResult<()> {
        self.registry.register(descriptor)
    }

    /// Collect all metrics
    pub fn collect(&self) -> Vec<MetricFamily> {
        if let Ok(mut last) = self.last_collection.write() {
            *last = Some(Instant::now());
        }
        self.registry.collect()
    }

    /// Check if collection is due
    pub fn should_collect(&self) -> bool {
        if let Ok(last) = self.last_collection.read() {
            match *last {
                Some(t) => t.elapsed() >= self.collection_interval,
                None => true,
            }
        } else {
            true
        }
    }

    /// Get time until next collection
    pub fn time_until_collection(&self) -> Duration {
        if let Ok(last) = self.last_collection.read() {
            if let Some(t) = *last {
                let elapsed = t.elapsed();
                if elapsed < self.collection_interval {
                    return self.collection_interval - elapsed;
                }
            }
        }
        Duration::ZERO
    }
}

impl Default for MetricCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter_basic() {
        let counter = Counter::new("requests_total", "Total requests");
        assert_eq!(counter.get(), 0);

        counter.inc();
        assert_eq!(counter.get(), 1);

        counter.add(10);
        assert_eq!(counter.get(), 11);

        counter.reset();
        assert_eq!(counter.get(), 0);
    }

    #[test]
    fn test_counter_with_labels() {
        let counter = Counter::new("requests_total", "Total requests")
            .with_labels(MetricLabels::from([("method", "GET")]));

        counter.inc();
        let sample = counter.to_sample();

        assert_eq!(sample.name, "requests_total");
        assert_eq!(sample.labels.get("method"), Some("GET"));
    }

    #[test]
    fn test_gauge_basic() {
        let gauge = Gauge::new("temperature", "Current temperature");
        assert_eq!(gauge.get(), 0.0);

        gauge.set(23.5);
        assert!((gauge.get() - 23.5).abs() < 0.001);

        gauge.add(1.5);
        assert!((gauge.get() - 25.0).abs() < 0.001);

        gauge.sub(5.0);
        assert!((gauge.get() - 20.0).abs() < 0.001);
    }

    #[test]
    fn test_gauge_inc_dec() {
        let gauge = Gauge::new("connections", "Active connections");

        gauge.inc();
        assert!((gauge.get() - 1.0).abs() < 0.001);

        gauge.inc();
        assert!((gauge.get() - 2.0).abs() < 0.001);

        gauge.dec();
        assert!((gauge.get() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_histogram_basic() {
        let hist = Histogram::new("request_duration", "Request duration");

        hist.observe(0.1);
        hist.observe(0.5);
        hist.observe(1.0);

        assert_eq!(hist.count(), 3);
        assert!((hist.sum() - 1.6).abs() < 0.001);
    }

    #[test]
    fn test_histogram_with_custom_buckets() {
        let hist = Histogram::with_buckets("sizes", "Request sizes", &[100.0, 500.0, 1000.0]);

        hist.observe(50.0);
        hist.observe(250.0);
        hist.observe(750.0);

        assert_eq!(hist.count(), 3);
    }

    #[test]
    fn test_timer() {
        let timer = Timer::new("operation_duration", "Operation duration");

        let obs = timer.start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let duration = obs.stop();

        assert!(duration.as_millis() >= 10);
        assert_eq!(timer.histogram().count(), 1);
    }

    #[test]
    fn test_timer_drop() {
        let timer = Timer::new("operation_duration", "Operation duration");

        {
            let _obs = timer.start();
            std::thread::sleep(std::time::Duration::from_millis(5));
            // Drops here, auto-recording
        }

        assert_eq!(timer.histogram().count(), 1);
    }

    #[test]
    fn test_metric_descriptor() {
        let desc = MetricDescriptor::new("requests", MetricType::Counter, "Total requests")
            .with_unit("1")
            .with_label_names(vec!["method".to_string(), "status".to_string()]);

        assert_eq!(desc.name, "requests");
        assert_eq!(desc.metric_type, MetricType::Counter);
        assert_eq!(desc.unit, Some("1".to_string()));
        assert_eq!(desc.label_names.len(), 2);
    }

    #[test]
    fn test_registry_register() {
        let registry = MetricRegistry::new();

        let desc = MetricDescriptor::new("requests", MetricType::Counter, "Total requests");
        assert!(registry.register(desc.clone()).is_ok());

        // Duplicate registration should fail
        assert!(matches!(
            registry.register(desc),
            Err(CollectorError::MetricAlreadyExists(_))
        ));
    }

    #[test]
    fn test_registry_with_prefix() {
        let registry = MetricRegistry::with_prefix("myapp");

        let desc = MetricDescriptor::new("requests", MetricType::Counter, "Total requests");
        registry.register(desc).unwrap();

        assert!(registry.get_descriptor("requests").is_some());
        assert!(registry
            .list_metrics()
            .contains(&"myapp_requests".to_string()));
    }

    #[test]
    fn test_registry_counter_operations() {
        let registry = MetricRegistry::new();

        let labels = MetricLabels::from([("method", "GET")]);

        registry
            .set_counter("requests", labels.clone(), 100)
            .unwrap();
        assert_eq!(registry.get_counter("requests", labels.clone()), Some(100));

        registry.inc_counter("requests", labels.clone()).unwrap();
        assert_eq!(registry.get_counter("requests", labels.clone()), Some(101));

        registry
            .add_counter("requests", labels.clone(), 10)
            .unwrap();
        assert_eq!(registry.get_counter("requests", labels), Some(111));
    }

    #[test]
    fn test_registry_gauge_operations() {
        let registry = MetricRegistry::new();

        let labels = MetricLabels::new();

        registry
            .set_gauge("temperature", labels.clone(), 23.5)
            .unwrap();
        assert!((registry.get_gauge("temperature", labels).unwrap() - 23.5).abs() < 0.001);
    }

    #[test]
    fn test_registry_histogram_operations() {
        let registry = MetricRegistry::new();

        let labels = MetricLabels::from([("endpoint", "/api")]);

        registry
            .observe_histogram("latency", labels.clone(), 0.1)
            .unwrap();
        registry
            .observe_histogram("latency", labels.clone(), 0.5)
            .unwrap();
        registry.observe_histogram("latency", labels, 1.0).unwrap();

        // Can't directly check histogram, but collection should work
        let families = registry.collect();
        assert!(!families.is_empty());
    }

    #[test]
    fn test_registry_collect() {
        let registry = MetricRegistry::new();

        registry
            .set_counter("requests", MetricLabels::from([("method", "GET")]), 100)
            .unwrap();
        registry
            .set_counter("requests", MetricLabels::from([("method", "POST")]), 50)
            .unwrap();
        registry
            .set_gauge("memory", MetricLabels::new(), 1024.0)
            .unwrap();

        let families = registry.collect();
        assert_eq!(families.len(), 2); // requests, memory

        let req_family = families.iter().find(|f| f.name == "requests").unwrap();
        assert_eq!(req_family.samples.len(), 2);
    }

    #[test]
    fn test_registry_clear() {
        let registry = MetricRegistry::new();

        registry
            .set_counter("requests", MetricLabels::new(), 100)
            .unwrap();
        assert!(registry.value_count() > 0);

        registry.clear();
        assert_eq!(registry.value_count(), 0);
    }

    #[test]
    fn test_registry_unregister() {
        let registry = MetricRegistry::new();

        let desc = MetricDescriptor::new("requests", MetricType::Counter, "Total requests");
        registry.register(desc).unwrap();
        registry
            .set_counter("requests", MetricLabels::new(), 100)
            .unwrap();

        assert!(registry.unregister("requests").is_ok());
        assert!(registry.get_descriptor("requests").is_none());

        // Unregister non-existent should fail
        assert!(matches!(
            registry.unregister("nonexistent"),
            Err(CollectorError::MetricNotFound(_))
        ));
    }

    #[test]
    fn test_collector_basic() {
        let collector = MetricCollector::new();

        let desc = MetricDescriptor::new("requests", MetricType::Counter, "Total requests");
        collector.register(desc).unwrap();

        collector
            .registry()
            .set_counter("requests", MetricLabels::new(), 100)
            .unwrap();

        let families = collector.collect();
        assert!(!families.is_empty());
    }

    #[test]
    fn test_collector_should_collect() {
        let mut collector = MetricCollector::new();
        collector.set_collection_interval(Duration::from_millis(50));

        assert!(collector.should_collect());

        collector.collect();
        assert!(!collector.should_collect());

        std::thread::sleep(Duration::from_millis(60));
        assert!(collector.should_collect());
    }

    #[test]
    fn test_collector_time_until_collection() {
        let mut collector = MetricCollector::new();
        collector.set_collection_interval(Duration::from_millis(100));

        collector.collect();
        let time_left = collector.time_until_collection();
        assert!(time_left.as_millis() > 0);
        assert!(time_left.as_millis() <= 100);
    }

    #[test]
    fn test_collector_error_display() {
        let err = CollectorError::MetricNotFound("test".to_string());
        assert!(format!("{}", err).contains("test"));

        let err = CollectorError::TypeMismatch {
            expected: MetricType::Counter,
            actual: MetricType::Gauge,
        };
        assert!(format!("{}", err).contains("counter"));
        assert!(format!("{}", err).contains("gauge"));
    }
}
