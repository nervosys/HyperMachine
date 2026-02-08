//! Telemetry Types
//!
//! Core types for the telemetry and observability system including
//! metrics, labels, samples, and histograms.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Metric type classification
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MetricType {
    /// Counter - monotonically increasing value
    Counter,
    /// Gauge - value that can go up or down
    #[default]
    Gauge,
    /// Histogram - distribution of values
    Histogram,
    /// Summary - precomputed quantiles
    Summary,
    /// Info - static metadata
    Info,
}

impl fmt::Display for MetricType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricType::Counter => write!(f, "counter"),
            MetricType::Gauge => write!(f, "gauge"),
            MetricType::Histogram => write!(f, "histogram"),
            MetricType::Summary => write!(f, "summary"),
            MetricType::Info => write!(f, "info"),
        }
    }
}

/// Metric value variants
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricValue {
    /// Integer counter/gauge
    Int(i64),
    /// Unsigned integer counter/gauge
    UInt(u64),
    /// Floating point gauge
    Float(f64),
    /// Histogram data
    Histogram(HistogramData),
    /// Summary data with quantiles
    Summary(SummaryData),
    /// Info labels (key-value pairs)
    Info(HashMap<String, String>),
}

impl MetricValue {
    /// Create an integer metric value
    pub fn int(value: i64) -> Self {
        MetricValue::Int(value)
    }

    /// Create an unsigned integer metric value
    pub fn uint(value: u64) -> Self {
        MetricValue::UInt(value)
    }

    /// Create a floating point metric value
    pub fn float(value: f64) -> Self {
        MetricValue::Float(value)
    }

    /// Get as f64 if possible
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            MetricValue::Int(v) => Some(*v as f64),
            MetricValue::UInt(v) => Some(*v as f64),
            MetricValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    /// Get as u64 if possible
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            MetricValue::Int(v) if *v >= 0 => Some(*v as u64),
            MetricValue::UInt(v) => Some(*v),
            MetricValue::Float(v) if *v >= 0.0 => Some(*v as u64),
            _ => None,
        }
    }

    /// Get as i64 if possible
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            MetricValue::Int(v) => Some(*v),
            MetricValue::UInt(v) if *v <= i64::MAX as u64 => Some(*v as i64),
            MetricValue::Float(v) => Some(*v as i64),
            _ => None,
        }
    }
}

impl Default for MetricValue {
    fn default() -> Self {
        MetricValue::UInt(0)
    }
}

impl fmt::Display for MetricValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetricValue::Int(v) => write!(f, "{}", v),
            MetricValue::UInt(v) => write!(f, "{}", v),
            MetricValue::Float(v) => write!(f, "{}", v),
            MetricValue::Histogram(h) => write!(f, "histogram(count={}, sum={})", h.count, h.sum),
            MetricValue::Summary(s) => write!(f, "summary(count={}, sum={})", s.count, s.sum),
            MetricValue::Info(m) => write!(f, "info({})", m.len()),
        }
    }
}

/// Histogram bucket
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramBucket {
    /// Upper bound (exclusive)
    pub le: f64,
    /// Cumulative count
    pub count: u64,
}

impl HistogramBucket {
    /// Create a new histogram bucket
    pub fn new(le: f64, count: u64) -> Self {
        Self { le, count }
    }
}

/// Histogram data with buckets
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistogramData {
    /// Total count of observations
    pub count: u64,
    /// Sum of all observations
    pub sum: f64,
    /// Buckets
    pub buckets: Vec<HistogramBucket>,
}

impl HistogramData {
    /// Create a new histogram with default buckets
    pub fn new() -> Self {
        Self::with_buckets(&DEFAULT_HISTOGRAM_BUCKETS)
    }

    /// Create a histogram with custom bucket boundaries
    pub fn with_buckets(boundaries: &[f64]) -> Self {
        let buckets = boundaries
            .iter()
            .map(|&le| HistogramBucket::new(le, 0))
            .collect();
        Self {
            count: 0,
            sum: 0.0,
            buckets,
        }
    }

    /// Record an observation
    pub fn observe(&mut self, value: f64) {
        self.count += 1;
        self.sum += value;

        for bucket in &mut self.buckets {
            if value <= bucket.le {
                bucket.count += 1;
            }
        }
    }

    /// Get the mean value
    pub fn mean(&self) -> Option<f64> {
        if self.count > 0 {
            Some(self.sum / self.count as f64)
        } else {
            None
        }
    }

    /// Estimate a quantile from bucket data
    pub fn estimate_quantile(&self, q: f64) -> Option<f64> {
        if self.count == 0 || q < 0.0 || q > 1.0 {
            return None;
        }

        let target = (q * self.count as f64).ceil() as u64;
        let mut prev_bound = 0.0;
        let mut prev_count = 0u64;

        for bucket in &self.buckets {
            if bucket.count >= target {
                // Linear interpolation within bucket
                let bucket_count = bucket.count - prev_count;
                if bucket_count == 0 {
                    return Some(bucket.le);
                }
                let pos = (target - prev_count) as f64 / bucket_count as f64;
                return Some(prev_bound + pos * (bucket.le - prev_bound));
            }
            prev_bound = bucket.le;
            prev_count = bucket.count;
        }

        // Value is above all buckets
        self.buckets.last().map(|b| b.le)
    }

    /// Reset the histogram
    pub fn reset(&mut self) {
        self.count = 0;
        self.sum = 0.0;
        for bucket in &mut self.buckets {
            bucket.count = 0;
        }
    }

    /// Get bucket counts as (le, count) pairs for export compatibility
    pub fn bucket_counts(&self) -> Vec<(f64, u64)> {
        self.buckets.iter().map(|b| (b.le, b.count)).collect()
    }
}

impl Default for HistogramData {
    fn default() -> Self {
        Self::new()
    }
}

/// Default histogram bucket boundaries
pub const DEFAULT_HISTOGRAM_BUCKETS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Latency histogram bucket boundaries (in seconds)
pub const LATENCY_BUCKETS: [f64; 14] = [
    0.0001, 0.0005, 0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Size histogram bucket boundaries (in bytes)
pub const SIZE_BUCKETS: [f64; 12] = [
    64.0,
    256.0,
    1024.0,
    4096.0,
    16384.0,
    65536.0,
    262144.0,
    1048576.0,
    4194304.0,
    16777216.0,
    67108864.0,
    268435456.0,
];

/// Summary quantile
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryQuantile {
    /// Quantile (0.0 to 1.0)
    pub quantile: f64,
    /// Value at quantile
    pub value: f64,
}

impl SummaryQuantile {
    /// Create a new summary quantile
    pub fn new(quantile: f64, value: f64) -> Self {
        Self { quantile, value }
    }
}

/// Summary data with precomputed quantiles
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SummaryData {
    /// Total count of observations
    pub count: u64,
    /// Sum of all observations
    pub sum: f64,
    /// Precomputed quantiles
    pub quantiles: Vec<SummaryQuantile>,
}

impl SummaryData {
    /// Create a new summary
    pub fn new() -> Self {
        Self {
            count: 0,
            sum: 0.0,
            quantiles: Vec::new(),
        }
    }

    /// Create with quantile targets
    pub fn with_quantiles(targets: &[f64]) -> Self {
        let quantiles = targets
            .iter()
            .map(|&q| SummaryQuantile::new(q, 0.0))
            .collect();
        Self {
            count: 0,
            sum: 0.0,
            quantiles,
        }
    }

    /// Get the mean value
    pub fn mean(&self) -> Option<f64> {
        if self.count > 0 {
            Some(self.sum / self.count as f64)
        } else {
            None
        }
    }

    /// Get a specific quantile value
    pub fn get_quantile(&self, q: f64) -> Option<f64> {
        self.quantiles
            .iter()
            .find(|sq| (sq.quantile - q).abs() < 0.001)
            .map(|sq| sq.value)
    }

    /// Iterate over quantiles as (quantile, value) pairs
    pub fn iter_quantiles(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.quantiles.iter().map(|sq| (sq.quantile, sq.value))
    }
}

impl Default for SummaryData {
    fn default() -> Self {
        Self::new()
    }
}

/// Default summary quantiles
pub const DEFAULT_QUANTILES: [f64; 5] = [0.5, 0.75, 0.9, 0.95, 0.99];

/// Metric label (key-value pair)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetricLabel {
    /// Label name
    pub name: String,
    /// Label value
    pub value: String,
}

impl MetricLabel {
    /// Create a new metric label
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl fmt::Display for MetricLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}=\"{}\"", self.name, self.value)
    }
}

/// Collection of metric labels
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MetricLabels {
    labels: Vec<MetricLabel>,
}

impl MetricLabels {
    /// Create empty labels
    pub fn new() -> Self {
        Self { labels: Vec::new() }
    }

    /// Create from a single label
    pub fn single(name: impl Into<String>, value: impl Into<String>) -> Self {
        let mut labels = Self::new();
        labels.add(name, value);
        labels
    }

    /// Add a label
    pub fn add(&mut self, name: impl Into<String>, value: impl Into<String>) {
        self.labels.push(MetricLabel::new(name, value));
    }

    /// Add multiple labels from an iterator
    pub fn extend<I, N, V>(&mut self, iter: I)
    where
        I: IntoIterator<Item = (N, V)>,
        N: Into<String>,
        V: Into<String>,
    {
        for (name, value) in iter {
            self.add(name, value);
        }
    }

    /// Get number of labels
    pub fn len(&self) -> usize {
        self.labels.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Insert a label (HashMap-style API for compatibility)
    pub fn insert(&mut self, name: String, value: String) {
        // Check if label already exists and update it
        for label in &mut self.labels {
            if label.name == name {
                label.value = value;
                return;
            }
        }
        // Otherwise add new label
        self.labels.push(MetricLabel::new(name, value));
    }

    /// Get a label value by name
    pub fn get(&self, name: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.value.as_str())
    }

    /// Iterate over labels
    pub fn iter(&self) -> impl Iterator<Item = &MetricLabel> {
        self.labels.iter()
    }

    /// Iterate over labels as (name, value) tuples for compatibility
    pub fn iter_pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.labels
            .iter()
            .map(|l| (l.name.as_str(), l.value.as_str()))
    }

    /// Convert to a sorted, canonical form for comparison
    pub fn canonical(&self) -> Self {
        let mut labels = self.labels.clone();
        labels.sort_by(|a, b| a.name.cmp(&b.name));
        Self { labels }
    }
}

impl fmt::Display for MetricLabels {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.labels.is_empty() {
            return Ok(());
        }
        write!(f, "{{")?;
        for (i, label) in self.labels.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{}", label)?;
        }
        write!(f, "}}")
    }
}

impl<const N: usize> From<[(&str, &str); N]> for MetricLabels {
    fn from(arr: [(&str, &str); N]) -> Self {
        let mut labels = Self::new();
        for (name, value) in arr {
            labels.add(name, value);
        }
        labels
    }
}

/// Timestamp for metric samples
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Timestamp(u64);

impl Timestamp {
    /// Create from Unix milliseconds
    pub fn from_millis(millis: u64) -> Self {
        Self(millis)
    }

    /// Create from SystemTime
    pub fn from_system_time(time: SystemTime) -> Self {
        let millis = time
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self(millis)
    }

    /// Get current timestamp
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// Get as Unix milliseconds
    pub fn as_millis(&self) -> u64 {
        self.0
    }

    /// Get as Unix milliseconds (alias for compatibility)
    pub fn millis(&self) -> u64 {
        self.0
    }

    /// Get as Unix seconds
    pub fn as_secs(&self) -> u64 {
        self.0 / 1000
    }

    /// Get as SystemTime
    pub fn as_system_time(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.0)
    }
}

impl Default for Timestamp {
    fn default() -> Self {
        Self::now()
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A single metric sample
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    /// Metric name
    pub name: String,
    /// Metric type
    pub metric_type: MetricType,
    /// Labels
    pub labels: MetricLabels,
    /// Value
    pub value: MetricValue,
    /// Timestamp
    pub timestamp: Timestamp,
    /// Help text / description
    pub help: Option<String>,
    /// Unit (if applicable)
    pub unit: Option<String>,
}

impl MetricSample {
    /// Create a new metric sample
    pub fn new(name: impl Into<String>, metric_type: MetricType, value: MetricValue) -> Self {
        Self {
            name: name.into(),
            metric_type,
            labels: MetricLabels::new(),
            value,
            timestamp: Timestamp::now(),
            help: None,
            unit: None,
        }
    }

    /// Create a counter sample
    pub fn counter(name: impl Into<String>, value: u64) -> Self {
        Self::new(name, MetricType::Counter, MetricValue::UInt(value))
    }

    /// Create a gauge sample (integer)
    pub fn gauge_int(name: impl Into<String>, value: i64) -> Self {
        Self::new(name, MetricType::Gauge, MetricValue::Int(value))
    }

    /// Create a gauge sample (float)
    pub fn gauge_float(name: impl Into<String>, value: f64) -> Self {
        Self::new(name, MetricType::Gauge, MetricValue::Float(value))
    }

    /// Create a histogram sample
    pub fn histogram(name: impl Into<String>, data: HistogramData) -> Self {
        Self::new(name, MetricType::Histogram, MetricValue::Histogram(data))
    }

    /// Add labels
    pub fn with_labels(mut self, labels: MetricLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Add a single label
    pub fn with_label(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.add(name, value);
        self
    }

    /// Set timestamp
    pub fn with_timestamp(mut self, timestamp: Timestamp) -> Self {
        self.timestamp = timestamp;
        self
    }

    /// Set help text
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Set unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Get full metric name with labels (Prometheus format)
    pub fn full_name(&self) -> String {
        if self.labels.is_empty() {
            self.name.clone()
        } else {
            format!("{}{}", self.name, self.labels)
        }
    }
}

impl fmt::Display for MetricSample {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.full_name(), self.value, self.timestamp)
    }
}

/// Metric family - a collection of samples for the same metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricFamily {
    /// Metric name
    pub name: String,
    /// Metric type
    pub metric_type: MetricType,
    /// Help text
    pub help: String,
    /// Unit (optional)
    pub unit: Option<String>,
    /// Samples
    pub samples: Vec<MetricSample>,
}

impl MetricFamily {
    /// Create a new metric family
    pub fn new(name: impl Into<String>, metric_type: MetricType, help: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            metric_type,
            help: help.into(),
            unit: None,
            samples: Vec::new(),
        }
    }

    /// Set unit
    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    /// Add a sample
    pub fn add_sample(&mut self, sample: MetricSample) {
        self.samples.push(sample);
    }

    /// Get number of samples
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Iterate over samples
    pub fn iter(&self) -> impl Iterator<Item = &MetricSample> {
        self.samples.iter()
    }
}

/// Rate calculator for counters
#[derive(Debug, Clone)]
pub struct RateCalculator {
    /// Previous value
    prev_value: Option<u64>,
    /// Previous timestamp
    prev_time: Option<Instant>,
    /// Current rate
    rate: f64,
}

impl RateCalculator {
    /// Create a new rate calculator
    pub fn new() -> Self {
        Self {
            prev_value: None,
            prev_time: None,
            rate: 0.0,
        }
    }

    /// Update with a new value and return the rate
    pub fn update(&mut self, value: u64) -> f64 {
        let now = Instant::now();

        if let (Some(prev_value), Some(prev_time)) = (self.prev_value, self.prev_time) {
            let elapsed = now.duration_since(prev_time).as_secs_f64();
            if elapsed > 0.0 {
                let delta = value.saturating_sub(prev_value);
                self.rate = delta as f64 / elapsed;
            }
        }

        self.prev_value = Some(value);
        self.prev_time = Some(now);

        self.rate
    }

    /// Get the current rate
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// Reset the calculator
    pub fn reset(&mut self) {
        self.prev_value = None;
        self.prev_time = None;
        self.rate = 0.0;
    }
}

impl Default for RateCalculator {
    fn default() -> Self {
        Self::new()
    }
}

/// Moving average calculator
#[derive(Debug, Clone)]
pub struct MovingAverage {
    /// Window size
    window_size: usize,
    /// Values in the window
    values: Vec<f64>,
    /// Current sum
    sum: f64,
    /// Current index
    index: usize,
    /// Whether the window is full
    full: bool,
}

impl MovingAverage {
    /// Create a new moving average calculator
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            values: vec![0.0; window_size],
            sum: 0.0,
            index: 0,
            full: false,
        }
    }

    /// Add a value and return the new average
    pub fn update(&mut self, value: f64) -> f64 {
        // Remove old value from sum
        self.sum -= self.values[self.index];

        // Add new value
        self.values[self.index] = value;
        self.sum += value;

        // Advance index
        self.index = (self.index + 1) % self.window_size;
        if self.index == 0 {
            self.full = true;
        }

        self.average()
    }

    /// Add a sample (alias for update)
    pub fn add_sample(&mut self, value: f64) -> f64 {
        self.update(value)
    }

    /// Get the current average
    pub fn average(&self) -> f64 {
        let count = if self.full {
            self.window_size
        } else {
            self.index
        };
        if count > 0 {
            self.sum / count as f64
        } else {
            0.0
        }
    }

    /// Reset the calculator
    pub fn reset(&mut self) {
        self.values.fill(0.0);
        self.sum = 0.0;
        self.index = 0;
        self.full = false;
    }

    /// Get the window size
    pub fn window_size(&self) -> usize {
        self.window_size
    }

    /// Get current sample count
    pub fn count(&self) -> usize {
        if self.full {
            self.window_size
        } else {
            self.index
        }
    }
}

/// Exponentially weighted moving average (EWMA)
#[derive(Debug, Clone)]
pub struct Ewma {
    /// Alpha (smoothing factor, 0 < alpha <= 1)
    alpha: f64,
    /// Current value
    value: f64,
    /// Whether initialized
    initialized: bool,
}

impl Ewma {
    /// Create a new EWMA with given alpha
    pub fn new(alpha: f64) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            value: 0.0,
            initialized: false,
        }
    }

    /// Create EWMA for a given span (number of periods)
    pub fn with_span(span: usize) -> Self {
        let alpha = 2.0 / (span as f64 + 1.0);
        Self::new(alpha)
    }

    /// Update with a new value
    pub fn update(&mut self, value: f64) -> f64 {
        if !self.initialized {
            self.value = value;
            self.initialized = true;
        } else {
            self.value = self.alpha * value + (1.0 - self.alpha) * self.value;
        }
        self.value
    }

    /// Add a sample (alias for update)
    pub fn add_sample(&mut self, value: f64) -> f64 {
        self.update(value)
    }

    /// Get the current value
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Reset the EWMA
    pub fn reset(&mut self) {
        self.value = 0.0;
        self.initialized = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_type_display() {
        assert_eq!(format!("{}", MetricType::Counter), "counter");
        assert_eq!(format!("{}", MetricType::Gauge), "gauge");
        assert_eq!(format!("{}", MetricType::Histogram), "histogram");
        assert_eq!(format!("{}", MetricType::Summary), "summary");
        assert_eq!(format!("{}", MetricType::Info), "info");
    }

    #[test]
    fn test_metric_value_conversions() {
        let int_val = MetricValue::Int(42);
        assert_eq!(int_val.as_f64(), Some(42.0));
        assert_eq!(int_val.as_u64(), Some(42));
        assert_eq!(int_val.as_i64(), Some(42));

        let uint_val = MetricValue::UInt(100);
        assert_eq!(uint_val.as_f64(), Some(100.0));
        assert_eq!(uint_val.as_u64(), Some(100));
        assert_eq!(uint_val.as_i64(), Some(100));

        let float_val = MetricValue::Float(3.14);
        assert_eq!(float_val.as_f64(), Some(3.14));
        assert_eq!(float_val.as_u64(), Some(3));
        assert_eq!(float_val.as_i64(), Some(3));

        let negative = MetricValue::Int(-10);
        assert_eq!(negative.as_u64(), None);
    }

    #[test]
    fn test_histogram_data() {
        let mut hist = HistogramData::new();
        assert_eq!(hist.count, 0);
        assert_eq!(hist.sum, 0.0);

        hist.observe(0.1);
        hist.observe(0.5);
        hist.observe(1.0);

        assert_eq!(hist.count, 3);
        assert!((hist.sum - 1.6).abs() < 0.001);
        assert!((hist.mean().unwrap() - 0.533).abs() < 0.01);
    }

    #[test]
    fn test_histogram_buckets() {
        let mut hist = HistogramData::with_buckets(&[0.1, 0.5, 1.0, 5.0]);

        hist.observe(0.05); // <= 0.1, 0.5, 1.0, 5.0
        hist.observe(0.3); // <= 0.5, 1.0, 5.0
        hist.observe(0.8); // <= 1.0, 5.0
        hist.observe(2.0); // <= 5.0

        assert_eq!(hist.buckets[0].count, 1); // le=0.1
        assert_eq!(hist.buckets[1].count, 2); // le=0.5
        assert_eq!(hist.buckets[2].count, 3); // le=1.0
        assert_eq!(hist.buckets[3].count, 4); // le=5.0
    }

    #[test]
    fn test_histogram_quantile_estimation() {
        let mut hist = HistogramData::with_buckets(&[1.0, 2.0, 5.0, 10.0]);

        for _ in 0..50 {
            hist.observe(0.5); // 50 values <= 1.0
        }
        for _ in 0..30 {
            hist.observe(3.0); // 30 values in (2.0, 5.0]
        }
        for _ in 0..20 {
            hist.observe(7.0); // 20 values in (5.0, 10.0]
        }

        // Median should be around 1.0
        let p50 = hist.estimate_quantile(0.5).unwrap();
        assert!(p50 <= 2.0);

        // 90th percentile should be higher
        let p90 = hist.estimate_quantile(0.9).unwrap();
        assert!(p90 >= 2.0);
    }

    #[test]
    fn test_histogram_reset() {
        let mut hist = HistogramData::new();
        hist.observe(1.0);
        hist.observe(2.0);
        assert_eq!(hist.count, 2);

        hist.reset();
        assert_eq!(hist.count, 0);
        assert_eq!(hist.sum, 0.0);
    }

    #[test]
    fn test_summary_data() {
        let mut summary = SummaryData::with_quantiles(&[0.5, 0.9, 0.99]);
        assert_eq!(summary.count, 0);
        assert_eq!(summary.quantiles.len(), 3);

        summary.count = 100;
        summary.sum = 500.0;
        summary.quantiles[0].value = 4.5;
        summary.quantiles[1].value = 8.0;
        summary.quantiles[2].value = 9.5;

        assert!((summary.mean().unwrap() - 5.0).abs() < 0.001);
        assert_eq!(summary.get_quantile(0.5), Some(4.5));
        assert_eq!(summary.get_quantile(0.9), Some(8.0));
        assert_eq!(summary.get_quantile(0.99), Some(9.5));
    }

    #[test]
    fn test_metric_label() {
        let label = MetricLabel::new("env", "production");
        assert_eq!(label.name, "env");
        assert_eq!(label.value, "production");
        assert_eq!(format!("{}", label), "env=\"production\"");
    }

    #[test]
    fn test_metric_labels() {
        let mut labels = MetricLabels::new();
        assert!(labels.is_empty());

        labels.add("env", "prod");
        labels.add("region", "us-east");
        assert_eq!(labels.len(), 2);
        assert_eq!(labels.get("env"), Some("prod"));
        assert_eq!(labels.get("region"), Some("us-east"));
        assert_eq!(labels.get("missing"), None);
    }

    #[test]
    fn test_metric_labels_display() {
        let labels = MetricLabels::from([("env", "prod"), ("region", "us-east")]);
        let display = format!("{}", labels);
        assert!(display.contains("env=\"prod\""));
        assert!(display.contains("region=\"us-east\""));
    }

    #[test]
    fn test_metric_labels_canonical() {
        let mut labels = MetricLabels::new();
        labels.add("z", "3");
        labels.add("a", "1");
        labels.add("m", "2");

        let canonical = labels.canonical();
        let names: Vec<&str> = canonical.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, vec!["a", "m", "z"]);
    }

    #[test]
    fn test_timestamp() {
        let ts = Timestamp::from_millis(1000);
        assert_eq!(ts.as_millis(), 1000);
        assert_eq!(ts.as_secs(), 1);

        let now = Timestamp::now();
        assert!(now.as_millis() > 0);
    }

    #[test]
    fn test_metric_sample_counter() {
        let sample = MetricSample::counter("requests_total", 42)
            .with_label("method", "GET")
            .with_help("Total number of requests");

        assert_eq!(sample.name, "requests_total");
        assert_eq!(sample.metric_type, MetricType::Counter);
        assert_eq!(sample.value.as_u64(), Some(42));
        assert_eq!(sample.labels.get("method"), Some("GET"));
        assert_eq!(sample.help, Some("Total number of requests".to_string()));
    }

    #[test]
    fn test_metric_sample_gauge() {
        let sample = MetricSample::gauge_float("temperature", 23.5).with_unit("celsius");

        assert_eq!(sample.name, "temperature");
        assert_eq!(sample.metric_type, MetricType::Gauge);
        assert_eq!(sample.value.as_f64(), Some(23.5));
        assert_eq!(sample.unit, Some("celsius".to_string()));
    }

    #[test]
    fn test_metric_sample_full_name() {
        let sample = MetricSample::counter("requests_total", 100);
        assert_eq!(sample.full_name(), "requests_total");

        let sample = MetricSample::counter("requests_total", 100).with_label("method", "GET");
        assert_eq!(sample.full_name(), "requests_total{method=\"GET\"}");
    }

    #[test]
    fn test_metric_family() {
        let mut family = MetricFamily::new(
            "http_requests_total",
            MetricType::Counter,
            "Total HTTP requests",
        );

        assert!(family.is_empty());

        family.add_sample(
            MetricSample::counter("http_requests_total", 100).with_label("method", "GET"),
        );
        family.add_sample(
            MetricSample::counter("http_requests_total", 50).with_label("method", "POST"),
        );

        assert_eq!(family.len(), 2);
    }

    #[test]
    fn test_rate_calculator() {
        let mut calc = RateCalculator::new();

        // First update establishes baseline
        let rate1 = calc.update(100);
        assert_eq!(rate1, 0.0);

        // Simulate time passing and value increase
        std::thread::sleep(std::time::Duration::from_millis(10));
        let rate2 = calc.update(200);
        assert!(rate2 > 0.0); // Should have positive rate
    }

    #[test]
    fn test_rate_calculator_reset() {
        let mut calc = RateCalculator::new();
        calc.update(100);
        calc.update(200);

        calc.reset();
        assert_eq!(calc.rate(), 0.0);
    }

    #[test]
    fn test_moving_average() {
        let mut ma = MovingAverage::new(3);

        assert_eq!(ma.update(10.0), 10.0);
        assert_eq!(ma.update(20.0), 15.0);
        assert_eq!(ma.update(30.0), 20.0); // (10+20+30)/3 = 20
        assert_eq!(ma.update(40.0), 30.0); // (20+30+40)/3 = 30 (10 dropped)

        assert_eq!(ma.count(), 3);
        assert!(ma.full);
    }

    #[test]
    fn test_moving_average_reset() {
        let mut ma = MovingAverage::new(3);
        ma.update(10.0);
        ma.update(20.0);

        ma.reset();
        assert_eq!(ma.average(), 0.0);
        assert_eq!(ma.count(), 0);
    }

    #[test]
    fn test_ewma() {
        let mut ewma = Ewma::new(0.5);

        // First value is taken as-is
        assert_eq!(ewma.update(10.0), 10.0);

        // Subsequent values are smoothed
        let v2 = ewma.update(20.0);
        assert!((v2 - 15.0).abs() < 0.001); // 0.5*20 + 0.5*10 = 15

        let v3 = ewma.update(30.0);
        assert!((v3 - 22.5).abs() < 0.001); // 0.5*30 + 0.5*15 = 22.5
    }

    #[test]
    fn test_ewma_with_span() {
        let ewma = Ewma::with_span(10);
        // Alpha should be 2/(10+1) = 0.1818...
        assert!((ewma.alpha - 0.1818).abs() < 0.01);
    }

    #[test]
    fn test_ewma_reset() {
        let mut ewma = Ewma::new(0.5);
        ewma.update(100.0);

        ewma.reset();
        assert_eq!(ewma.value(), 0.0);
        assert!(!ewma.initialized);
    }

    #[test]
    fn test_default_buckets() {
        assert_eq!(DEFAULT_HISTOGRAM_BUCKETS.len(), 11);
        assert!(DEFAULT_HISTOGRAM_BUCKETS.windows(2).all(|w| w[0] < w[1]));

        assert_eq!(LATENCY_BUCKETS.len(), 14);
        assert!(LATENCY_BUCKETS.windows(2).all(|w| w[0] < w[1]));

        assert_eq!(SIZE_BUCKETS.len(), 12);
        assert!(SIZE_BUCKETS.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn test_default_quantiles() {
        assert_eq!(DEFAULT_QUANTILES.len(), 5);
        assert!(DEFAULT_QUANTILES.iter().all(|&q| q >= 0.0 && q <= 1.0));
    }
}
