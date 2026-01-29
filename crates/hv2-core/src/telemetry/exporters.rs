//! Metric Exporters
//!
//! Export metrics in various formats for external consumption.

use super::collector::{MetricCollector, MetricRegistry};
use super::types::{
    HistogramData, MetricFamily, MetricLabels, MetricSample, MetricType, MetricValue,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Error type for exporter operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExporterError {
    /// Serialization error
    SerializationError(String),
    /// IO error
    IoError(String),
    /// Format error
    FormatError(String),
    /// Network error
    NetworkError(String),
}

impl std::fmt::Display for ExporterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExporterError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            ExporterError::IoError(msg) => write!(f, "IO error: {}", msg),
            ExporterError::FormatError(msg) => write!(f, "Format error: {}", msg),
            ExporterError::NetworkError(msg) => write!(f, "Network error: {}", msg),
        }
    }
}

impl std::error::Error for ExporterError {}

/// Result type for exporter operations
pub type ExporterResult<T> = Result<T, ExporterError>;

/// Trait for metric exporters
pub trait MetricExporter {
    /// Export metrics to a string representation
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String>;

    /// Get the content type for this exporter
    fn content_type(&self) -> &'static str;

    /// Get exporter name
    fn name(&self) -> &'static str;
}

/// Prometheus text format exporter
#[derive(Debug, Default)]
pub struct PrometheusExporter {
    /// Include timestamps
    include_timestamps: bool,
}

impl PrometheusExporter {
    /// Create a new Prometheus exporter
    pub fn new() -> Self {
        Self {
            include_timestamps: false,
        }
    }

    /// Enable timestamps
    pub fn with_timestamps(mut self) -> Self {
        self.include_timestamps = true;
        self
    }

    /// Format labels for Prometheus
    fn format_labels(&self, labels: &MetricLabels) -> String {
        if labels.is_empty() {
            return String::new();
        }

        let label_strs: Vec<String> = labels
            .iter()
            .map(|l| format!("{}=\"{}\"", l.name, Self::escape_label_value(&l.value)))
            .collect();

        format!("{{{}}}", label_strs.join(","))
    }

    /// Escape special characters in label values
    fn escape_label_value(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    }

    /// Format a metric line
    fn format_metric_line(
        &self,
        name: &str,
        labels: &MetricLabels,
        value: f64,
        timestamp: Option<u64>,
    ) -> String {
        let label_str = self.format_labels(labels);
        let ts = if self.include_timestamps {
            timestamp.map(|t| format!(" {}", t)).unwrap_or_default()
        } else {
            String::new()
        };
        format!("{}{} {}{}\n", name, label_str, value, ts)
    }

    /// Export histogram buckets
    fn export_histogram(
        &self,
        name: &str,
        labels: &MetricLabels,
        data: &HistogramData,
        timestamp: Option<u64>,
    ) -> String {
        let mut output = String::new();

        // Export each bucket
        for (bound, count) in data.bucket_counts() {
            let mut bucket_labels = labels.clone();
            bucket_labels.insert("le".to_string(), format!("{}", bound));
            output.push_str(&self.format_metric_line(
                &format!("{}_bucket", name),
                &bucket_labels,
                count as f64,
                timestamp,
            ));
        }

        // +Inf bucket
        let mut inf_labels = labels.clone();
        inf_labels.insert("le".to_string(), "+Inf".to_string());
        output.push_str(&self.format_metric_line(
            &format!("{}_bucket", name),
            &inf_labels,
            data.count as f64,
            timestamp,
        ));

        // Sum and count
        output.push_str(&self.format_metric_line(
            &format!("{}_sum", name),
            labels,
            data.sum,
            timestamp,
        ));
        output.push_str(&self.format_metric_line(
            &format!("{}_count", name),
            labels,
            data.count as f64,
            timestamp,
        ));

        output
    }
}

impl MetricExporter for PrometheusExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        let mut output = String::new();

        for family in families {
            // HELP line
            if !family.help.is_empty() {
                writeln!(output, "# HELP {} {}", family.name, family.help)
                    .map_err(|e| ExporterError::FormatError(e.to_string()))?;
            }

            // TYPE line
            let type_str = match family.metric_type {
                MetricType::Counter => "counter",
                MetricType::Gauge => "gauge",
                MetricType::Histogram => "histogram",
                MetricType::Summary => "summary",
                MetricType::Info => "gauge", // Info exported as gauge in Prometheus
            };
            writeln!(output, "# TYPE {} {}", family.name, type_str)
                .map_err(|e| ExporterError::FormatError(e.to_string()))?;

            // Metric values
            for sample in &family.samples {
                let timestamp = if self.include_timestamps {
                    Some(sample.timestamp.millis())
                } else {
                    None
                };

                match &sample.value {
                    MetricValue::Int(v) => {
                        output.push_str(&self.format_metric_line(
                            &family.name,
                            &sample.labels,
                            *v as f64,
                            timestamp,
                        ));
                    }
                    MetricValue::UInt(v) => {
                        output.push_str(&self.format_metric_line(
                            &family.name,
                            &sample.labels,
                            *v as f64,
                            timestamp,
                        ));
                    }
                    MetricValue::Float(v) => {
                        output.push_str(&self.format_metric_line(
                            &family.name,
                            &sample.labels,
                            *v,
                            timestamp,
                        ));
                    }
                    MetricValue::Histogram(h) => {
                        output.push_str(&self.export_histogram(
                            &family.name,
                            &sample.labels,
                            h,
                            timestamp,
                        ));
                    }
                    MetricValue::Summary(s) => {
                        // Export quantiles
                        for sq in &s.quantiles {
                            let mut q_labels = sample.labels.clone();
                            q_labels.insert("quantile".to_string(), format!("{}", sq.quantile));
                            output.push_str(&self.format_metric_line(
                                &family.name,
                                &q_labels,
                                sq.value,
                                timestamp,
                            ));
                        }
                        output.push_str(&self.format_metric_line(
                            &format!("{}_sum", family.name),
                            &sample.labels,
                            s.sum,
                            timestamp,
                        ));
                        output.push_str(&self.format_metric_line(
                            &format!("{}_count", family.name),
                            &sample.labels,
                            s.count as f64,
                            timestamp,
                        ));
                    }
                    MetricValue::Info(info) => {
                        // Info metrics exported as gauge with value 1 and info labels
                        let mut info_labels = sample.labels.clone();
                        for (k, v) in info {
                            info_labels.insert(k.clone(), v.clone());
                        }
                        output.push_str(&self.format_metric_line(
                            &family.name,
                            &info_labels,
                            1.0,
                            timestamp,
                        ));
                    }
                }
            }

            output.push('\n');
        }

        Ok(output)
    }

    fn content_type(&self) -> &'static str {
        "text/plain; version=0.0.4; charset=utf-8"
    }

    fn name(&self) -> &'static str {
        "prometheus"
    }
}

/// JSON metric format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonMetric {
    /// Metric name
    pub name: String,
    /// Metric type
    #[serde(rename = "type")]
    pub metric_type: String,
    /// Help text
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub help: String,
    /// Metric value
    pub value: serde_json::Value,
    /// Labels
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub labels: HashMap<String, String>,
    /// Timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<u64>,
}

/// JSON exporter
#[derive(Debug, Default)]
pub struct JsonExporter {
    /// Pretty print output
    pretty: bool,
    /// Include timestamps
    include_timestamps: bool,
}

impl JsonExporter {
    /// Create a new JSON exporter
    pub fn new() -> Self {
        Self {
            pretty: false,
            include_timestamps: true,
        }
    }

    /// Enable pretty printing
    pub fn pretty(mut self) -> Self {
        self.pretty = true;
        self
    }

    /// Disable timestamps
    pub fn without_timestamps(mut self) -> Self {
        self.include_timestamps = false;
        self
    }

    /// Convert a metric value to JSON
    fn value_to_json(&self, value: &MetricValue) -> serde_json::Value {
        match value {
            MetricValue::Int(v) => serde_json::json!(*v),
            MetricValue::UInt(v) => serde_json::json!(*v),
            MetricValue::Float(v) => serde_json::json!(*v),
            MetricValue::Histogram(h) => serde_json::json!({
                "count": h.count,
                "sum": h.sum,
                "buckets": h.bucket_counts()
                    .iter()
                    .map(|(bound, count)| serde_json::json!({
                        "le": bound,
                        "count": count
                    }))
                    .collect::<Vec<_>>()
            }),
            MetricValue::Summary(s) => serde_json::json!({
                "count": s.count,
                "sum": s.sum,
                "quantiles": s.quantiles.iter()
                    .map(|sq| serde_json::json!({
                        "quantile": sq.quantile,
                        "value": sq.value
                    }))
                    .collect::<Vec<_>>()
            }),
            MetricValue::Info(info) => serde_json::json!(info),
        }
    }
}

impl MetricExporter for JsonExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        let mut metrics: Vec<JsonMetric> = Vec::new();

        for family in families {
            for sample in &family.samples {
                let metric = JsonMetric {
                    name: sample.name.clone(),
                    metric_type: format!("{}", family.metric_type),
                    help: sample.help.clone().unwrap_or_default(),
                    value: self.value_to_json(&sample.value),
                    labels: sample
                        .labels
                        .iter()
                        .map(|l| (l.name.clone(), l.value.clone()))
                        .collect(),
                    timestamp: if self.include_timestamps {
                        Some(sample.timestamp.millis())
                    } else {
                        None
                    },
                };
                metrics.push(metric);
            }
        }

        if self.pretty {
            serde_json::to_string_pretty(&metrics)
                .map_err(|e| ExporterError::SerializationError(e.to_string()))
        } else {
            serde_json::to_string(&metrics)
                .map_err(|e| ExporterError::SerializationError(e.to_string()))
        }
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn name(&self) -> &'static str {
        "json"
    }
}

/// OpenTelemetry metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpDataPoint {
    /// Start time (unix nano)
    #[serde(rename = "startTimeUnixNano")]
    pub start_time_unix_nano: u64,
    /// Time (unix nano)
    #[serde(rename = "timeUnixNano")]
    pub time_unix_nano: u64,
    /// Value
    #[serde(flatten)]
    pub value: OtlpValue,
    /// Attributes
    pub attributes: Vec<OtlpAttribute>,
}

/// OpenTelemetry value
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OtlpValue {
    /// Integer value
    AsInt {
        #[serde(rename = "asInt")]
        as_int: i64,
    },
    /// Double value
    AsDouble {
        #[serde(rename = "asDouble")]
        as_double: f64,
    },
}

/// OpenTelemetry attribute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpAttribute {
    /// Key
    pub key: String,
    /// Value
    pub value: OtlpAttributeValue,
}

/// OpenTelemetry attribute value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpAttributeValue {
    /// String value
    #[serde(rename = "stringValue")]
    pub string_value: String,
}

/// OpenTelemetry metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpMetric {
    /// Name
    pub name: String,
    /// Description
    pub description: String,
    /// Unit
    pub unit: String,
    /// Metric data
    #[serde(flatten)]
    pub data: OtlpMetricData,
}

/// OpenTelemetry metric data
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OtlpMetricData {
    /// Sum (counter)
    Sum { sum: OtlpSum },
    /// Gauge
    Gauge { gauge: OtlpGauge },
    /// Histogram
    Histogram { histogram: OtlpHistogram },
}

/// OpenTelemetry sum
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpSum {
    /// Data points
    #[serde(rename = "dataPoints")]
    pub data_points: Vec<OtlpDataPoint>,
    /// Aggregation temporality
    #[serde(rename = "aggregationTemporality")]
    pub aggregation_temporality: i32,
    /// Is monotonic
    #[serde(rename = "isMonotonic")]
    pub is_monotonic: bool,
}

/// OpenTelemetry gauge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpGauge {
    /// Data points
    #[serde(rename = "dataPoints")]
    pub data_points: Vec<OtlpDataPoint>,
}

/// OpenTelemetry histogram
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpHistogram {
    /// Data points
    #[serde(rename = "dataPoints")]
    pub data_points: Vec<OtlpHistogramDataPoint>,
    /// Aggregation temporality
    #[serde(rename = "aggregationTemporality")]
    pub aggregation_temporality: i32,
}

/// OpenTelemetry histogram data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtlpHistogramDataPoint {
    /// Start time (unix nano)
    #[serde(rename = "startTimeUnixNano")]
    pub start_time_unix_nano: u64,
    /// Time (unix nano)
    #[serde(rename = "timeUnixNano")]
    pub time_unix_nano: u64,
    /// Count
    pub count: u64,
    /// Sum
    pub sum: f64,
    /// Bucket counts
    #[serde(rename = "bucketCounts")]
    pub bucket_counts: Vec<u64>,
    /// Explicit bounds
    #[serde(rename = "explicitBounds")]
    pub explicit_bounds: Vec<f64>,
    /// Attributes
    pub attributes: Vec<OtlpAttribute>,
}

/// OpenTelemetry exporter
#[derive(Debug, Default)]
pub struct OpenTelemetryExporter {
    /// Service name
    service_name: String,
}

impl OpenTelemetryExporter {
    /// Create a new OpenTelemetry exporter
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    /// Get current time in nanoseconds
    fn current_time_nanos() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    }

    /// Convert labels to attributes
    fn labels_to_attributes(labels: &MetricLabels) -> Vec<OtlpAttribute> {
        labels
            .iter()
            .map(|l| OtlpAttribute {
                key: l.name.clone(),
                value: OtlpAttributeValue {
                    string_value: l.value.clone(),
                },
            })
            .collect()
    }
}

impl MetricExporter for OpenTelemetryExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        let mut metrics: Vec<OtlpMetric> = Vec::new();
        let now = Self::current_time_nanos();

        for family in families {
            for sample in &family.samples {
                let attributes = Self::labels_to_attributes(&sample.labels);
                let time_nanos = sample.timestamp.millis() * 1_000_000;

                let metric = match &sample.value {
                    MetricValue::Int(v) => OtlpMetric {
                        name: sample.name.clone(),
                        description: sample.help.clone().unwrap_or_default(),
                        unit: String::new(),
                        data: if family.metric_type == MetricType::Counter {
                            OtlpMetricData::Sum {
                                sum: OtlpSum {
                                    data_points: vec![OtlpDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        value: OtlpValue::AsInt { as_int: *v },
                                        attributes,
                                    }],
                                    aggregation_temporality: 2, // CUMULATIVE
                                    is_monotonic: true,
                                },
                            }
                        } else {
                            OtlpMetricData::Gauge {
                                gauge: OtlpGauge {
                                    data_points: vec![OtlpDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        value: OtlpValue::AsInt { as_int: *v },
                                        attributes,
                                    }],
                                },
                            }
                        },
                    },
                    MetricValue::UInt(v) => OtlpMetric {
                        name: sample.name.clone(),
                        description: sample.help.clone().unwrap_or_default(),
                        unit: String::new(),
                        data: if family.metric_type == MetricType::Counter {
                            OtlpMetricData::Sum {
                                sum: OtlpSum {
                                    data_points: vec![OtlpDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        value: OtlpValue::AsInt { as_int: *v as i64 },
                                        attributes,
                                    }],
                                    aggregation_temporality: 2,
                                    is_monotonic: true,
                                },
                            }
                        } else {
                            OtlpMetricData::Gauge {
                                gauge: OtlpGauge {
                                    data_points: vec![OtlpDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        value: OtlpValue::AsInt { as_int: *v as i64 },
                                        attributes,
                                    }],
                                },
                            }
                        },
                    },
                    MetricValue::Float(v) => OtlpMetric {
                        name: sample.name.clone(),
                        description: sample.help.clone().unwrap_or_default(),
                        unit: String::new(),
                        data: OtlpMetricData::Gauge {
                            gauge: OtlpGauge {
                                data_points: vec![OtlpDataPoint {
                                    start_time_unix_nano: 0,
                                    time_unix_nano: time_nanos,
                                    value: OtlpValue::AsDouble { as_double: *v },
                                    attributes,
                                }],
                            },
                        },
                    },
                    MetricValue::Histogram(h) => {
                        let bounds: Vec<f64> = h.bucket_counts().iter().map(|(b, _)| *b).collect();
                        let counts: Vec<u64> = h.bucket_counts().iter().map(|(_, c)| *c).collect();

                        OtlpMetric {
                            name: sample.name.clone(),
                            description: sample.help.clone().unwrap_or_default(),
                            unit: String::new(),
                            data: OtlpMetricData::Histogram {
                                histogram: OtlpHistogram {
                                    data_points: vec![OtlpHistogramDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        count: h.count,
                                        sum: h.sum,
                                        bucket_counts: counts,
                                        explicit_bounds: bounds,
                                        attributes,
                                    }],
                                    aggregation_temporality: 2,
                                },
                            },
                        }
                    }
                    MetricValue::Summary(s) => {
                        // Export summary as gauge with quantile labels
                        OtlpMetric {
                            name: sample.name.clone(),
                            description: sample.help.clone().unwrap_or_default(),
                            unit: String::new(),
                            data: OtlpMetricData::Gauge {
                                gauge: OtlpGauge {
                                    data_points: s
                                        .quantiles
                                        .iter()
                                        .map(|sq| {
                                            let mut attrs = attributes.clone();
                                            attrs.push(OtlpAttribute {
                                                key: "quantile".to_string(),
                                                value: OtlpAttributeValue {
                                                    string_value: format!("{}", sq.quantile),
                                                },
                                            });
                                            OtlpDataPoint {
                                                start_time_unix_nano: 0,
                                                time_unix_nano: time_nanos,
                                                value: OtlpValue::AsDouble {
                                                    as_double: sq.value,
                                                },
                                                attributes: attrs,
                                            }
                                        })
                                        .collect(),
                                },
                            },
                        }
                    }
                    MetricValue::Info(info) => {
                        let mut attrs = attributes;
                        for (k, v) in info {
                            attrs.push(OtlpAttribute {
                                key: k.clone(),
                                value: OtlpAttributeValue {
                                    string_value: v.clone(),
                                },
                            });
                        }
                        OtlpMetric {
                            name: sample.name.clone(),
                            description: sample.help.clone().unwrap_or_default(),
                            unit: String::new(),
                            data: OtlpMetricData::Gauge {
                                gauge: OtlpGauge {
                                    data_points: vec![OtlpDataPoint {
                                        start_time_unix_nano: 0,
                                        time_unix_nano: time_nanos,
                                        value: OtlpValue::AsDouble { as_double: 1.0 },
                                        attributes: attrs,
                                    }],
                                },
                            },
                        }
                    }
                };

                metrics.push(metric);
            }
        }

        serde_json::to_string(&metrics)
            .map_err(|e| ExporterError::SerializationError(e.to_string()))
    }

    fn content_type(&self) -> &'static str {
        "application/json"
    }

    fn name(&self) -> &'static str {
        "opentelemetry"
    }
}

/// StatsD line format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsDFormat {
    /// Standard StatsD
    Standard,
    /// DogStatsD (DataDog extended format)
    DogStatsD,
}

/// StatsD exporter
#[derive(Debug)]
pub struct StatsDExporter {
    /// Metric prefix
    prefix: String,
    /// Format
    format: StatsDFormat,
}

impl StatsDExporter {
    /// Create a new StatsD exporter
    pub fn new() -> Self {
        Self {
            prefix: String::new(),
            format: StatsDFormat::Standard,
        }
    }

    /// Set prefix
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Use DogStatsD format
    pub fn dogstatsd(mut self) -> Self {
        self.format = StatsDFormat::DogStatsD;
        self
    }

    /// Format a metric name
    fn format_name(&self, name: &str) -> String {
        let name = name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_");
        if self.prefix.is_empty() {
            name
        } else {
            format!("{}.{}", self.prefix, name)
        }
    }

    /// Format tags for DogStatsD
    fn format_tags(&self, labels: &MetricLabels) -> String {
        if labels.is_empty() || self.format != StatsDFormat::DogStatsD {
            return String::new();
        }

        let tags: Vec<String> = labels
            .iter()
            .map(|l| format!("{}:{}", l.name, l.value))
            .collect();

        format!("|#{}", tags.join(","))
    }

    /// Format a StatsD line
    fn format_line(
        &self,
        name: &str,
        value: f64,
        metric_type: &str,
        labels: &MetricLabels,
    ) -> String {
        let formatted_name = self.format_name(name);
        let tags = self.format_tags(labels);
        format!("{}:{}|{}{}\n", formatted_name, value, metric_type, tags)
    }
}

impl Default for StatsDExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricExporter for StatsDExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        let mut output = String::new();

        for family in families {
            for sample in &family.samples {
                match &sample.value {
                    MetricValue::Int(v) => {
                        let type_char = match family.metric_type {
                            MetricType::Counter => "c",
                            MetricType::Gauge => "g",
                            _ => "g",
                        };
                        output.push_str(&self.format_line(
                            &sample.name,
                            *v as f64,
                            type_char,
                            &sample.labels,
                        ));
                    }
                    MetricValue::UInt(v) => {
                        let type_char = match family.metric_type {
                            MetricType::Counter => "c",
                            MetricType::Gauge => "g",
                            _ => "g",
                        };
                        output.push_str(&self.format_line(
                            &sample.name,
                            *v as f64,
                            type_char,
                            &sample.labels,
                        ));
                    }
                    MetricValue::Float(v) => {
                        output.push_str(&self.format_line(&sample.name, *v, "g", &sample.labels));
                    }
                    MetricValue::Histogram(h) => {
                        // Export histogram as multiple timing values
                        // In StatsD, histograms are typically timers
                        if h.count > 0 {
                            let mean = h.sum / h.count as f64;
                            output.push_str(&self.format_line(
                                &format!("{}.mean", sample.name),
                                mean,
                                "g",
                                &sample.labels,
                            ));
                        }
                        output.push_str(&self.format_line(
                            &format!("{}.count", sample.name),
                            h.count as f64,
                            "c",
                            &sample.labels,
                        ));
                        output.push_str(&self.format_line(
                            &format!("{}.sum", sample.name),
                            h.sum,
                            "g",
                            &sample.labels,
                        ));
                    }
                    MetricValue::Summary(s) => {
                        for sq in &s.quantiles {
                            output.push_str(&self.format_line(
                                &format!("{}.p{}", sample.name, (sq.quantile * 100.0) as u32),
                                sq.value,
                                "g",
                                &sample.labels,
                            ));
                        }
                        output.push_str(&self.format_line(
                            &format!("{}.count", sample.name),
                            s.count as f64,
                            "c",
                            &sample.labels,
                        ));
                    }
                    MetricValue::Info(_) => {
                        // Info metrics don't map well to StatsD, skip
                    }
                }
            }
        }

        Ok(output)
    }

    fn content_type(&self) -> &'static str {
        "text/plain"
    }

    fn name(&self) -> &'static str {
        "statsd"
    }
}

/// Carbon (Graphite) exporter
#[derive(Debug)]
pub struct CarbonExporter {
    /// Metric prefix
    prefix: String,
}

impl CarbonExporter {
    /// Create a new Carbon exporter
    pub fn new() -> Self {
        Self {
            prefix: String::new(),
        }
    }

    /// Set prefix
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = prefix.into();
        self
    }

    /// Format a metric name (Carbon uses dots as separators)
    fn format_name(&self, name: &str, labels: &MetricLabels) -> String {
        let mut path = if self.prefix.is_empty() {
            name.to_string()
        } else {
            format!("{}.{}", self.prefix, name)
        };

        // Append label values to the path
        for l in labels.iter() {
            path.push_str(&format!(".{}.{}", l.name, l.value));
        }

        // Clean up invalid characters
        path.replace(|c: char| !c.is_alphanumeric() && c != '.' && c != '_', "_")
    }

    /// Format a Carbon line
    fn format_line(&self, name: &str, labels: &MetricLabels, value: f64, timestamp: u64) -> String {
        let formatted_name = self.format_name(name, labels);
        let ts_secs = timestamp / 1000;
        format!("{} {} {}\n", formatted_name, value, ts_secs)
    }
}

impl Default for CarbonExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricExporter for CarbonExporter {
    fn export(&self, families: &[MetricFamily]) -> ExporterResult<String> {
        let mut output = String::new();

        for family in families {
            for sample in &family.samples {
                let ts = sample.timestamp.millis();

                match &sample.value {
                    MetricValue::Int(v) => {
                        output.push_str(&self.format_line(
                            &sample.name,
                            &sample.labels,
                            *v as f64,
                            ts,
                        ));
                    }
                    MetricValue::UInt(v) => {
                        output.push_str(&self.format_line(
                            &sample.name,
                            &sample.labels,
                            *v as f64,
                            ts,
                        ));
                    }
                    MetricValue::Float(v) => {
                        output.push_str(&self.format_line(&sample.name, &sample.labels, *v, ts));
                    }
                    MetricValue::Histogram(h) => {
                        if h.count > 0 {
                            output.push_str(&self.format_line(
                                &format!("{}.mean", sample.name),
                                &sample.labels,
                                h.sum / h.count as f64,
                                ts,
                            ));
                        }
                        output.push_str(&self.format_line(
                            &format!("{}.count", sample.name),
                            &sample.labels,
                            h.count as f64,
                            ts,
                        ));
                        output.push_str(&self.format_line(
                            &format!("{}.sum", sample.name),
                            &sample.labels,
                            h.sum,
                            ts,
                        ));
                    }
                    MetricValue::Summary(s) => {
                        for sq in &s.quantiles {
                            output.push_str(&self.format_line(
                                &format!("{}.p{}", sample.name, (sq.quantile * 100.0) as u32),
                                &sample.labels,
                                sq.value,
                                ts,
                            ));
                        }
                    }
                    MetricValue::Info(_) => {
                        // Info metrics don't map well to Carbon
                    }
                }
            }
        }

        Ok(output)
    }

    fn content_type(&self) -> &'static str {
        "text/plain"
    }

    fn name(&self) -> &'static str {
        "carbon"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_families() -> Vec<MetricFamily> {
        let mut counter_family =
            MetricFamily::new("requests_total", MetricType::Counter, "Total requests");
        counter_family.add_sample(
            MetricSample::counter("requests_total", 100)
                .with_labels(MetricLabels::from([("method", "GET")])),
        );
        counter_family.add_sample(
            MetricSample::counter("requests_total", 50)
                .with_labels(MetricLabels::from([("method", "POST")])),
        );

        let mut gauge_family =
            MetricFamily::new("temperature", MetricType::Gauge, "Current temperature");
        gauge_family.add_sample(MetricSample::gauge_float("temperature", 23.5));

        let mut hist = HistogramData::with_buckets(&[0.1, 0.5, 1.0, 5.0]);
        hist.observe(0.05);
        hist.observe(0.25);
        hist.observe(0.75);

        let mut hist_family =
            MetricFamily::new("latency", MetricType::Histogram, "Request latency");
        hist_family.add_sample(MetricSample::histogram("latency", hist));

        vec![counter_family, gauge_family, hist_family]
    }

    #[test]
    fn test_prometheus_exporter() {
        let exporter = PrometheusExporter::new();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        assert!(output.contains("# HELP requests_total Total requests"));
        assert!(output.contains("# TYPE requests_total counter"));
        assert!(output.contains("requests_total{method=\"GET\"} 100"));
        assert!(output.contains("requests_total{method=\"POST\"} 50"));
        assert!(output.contains("# TYPE temperature gauge"));
        assert!(output.contains("temperature 23.5"));
        assert!(output.contains("latency_bucket"));
        assert!(output.contains("latency_sum"));
        assert!(output.contains("latency_count"));
    }

    #[test]
    fn test_prometheus_exporter_with_timestamps() {
        let exporter = PrometheusExporter::new().with_timestamps();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();
        // Should contain timestamp values
        assert!(output.contains("requests_total{method=\"GET\"} 100 "));
    }

    #[test]
    fn test_prometheus_label_escaping() {
        let exporter = PrometheusExporter::new();

        let mut family = MetricFamily::new("test", MetricType::Counter, "Test");
        family.add_sample(
            MetricSample::counter("test", 1).with_labels(MetricLabels::from([("path", "/api/v1")])),
        );

        let output = exporter.export(&[family]).unwrap();
        assert!(output.contains("path=\"/api/v1\""));
    }

    #[test]
    fn test_json_exporter() {
        let exporter = JsonExporter::new();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        // Should be valid JSON
        let parsed: Vec<JsonMetric> = serde_json::from_str(&output).unwrap();
        assert!(!parsed.is_empty());

        // Check structure
        let request_metric = parsed.iter().find(|m| m.name == "requests_total");
        assert!(request_metric.is_some());
    }

    #[test]
    fn test_json_exporter_pretty() {
        let exporter = JsonExporter::new().pretty();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();
        assert!(output.contains('\n'));
        assert!(output.contains("  "));
    }

    #[test]
    fn test_json_exporter_without_timestamps() {
        let exporter = JsonExporter::new().without_timestamps();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();
        let parsed: Vec<JsonMetric> = serde_json::from_str(&output).unwrap();

        for metric in parsed {
            assert!(metric.timestamp.is_none());
        }
    }

    #[test]
    fn test_opentelemetry_exporter() {
        let exporter = OpenTelemetryExporter::new("test-service");
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        // Should be valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert!(parsed.is_array());
    }

    #[test]
    fn test_statsd_exporter() {
        let exporter = StatsDExporter::new();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        assert!(output.contains("requests_total:100|c"));
        assert!(output.contains("temperature:23.5|g"));
        assert!(output.contains("latency_count:3|c")); // Using underscore
    }

    #[test]
    fn test_statsd_exporter_with_prefix() {
        let exporter = StatsDExporter::new().with_prefix("myapp");
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        assert!(output.contains("myapp.requests_total"));
        assert!(output.contains("myapp.temperature"));
    }

    #[test]
    fn test_statsd_dogstatsd_format() {
        let exporter = StatsDExporter::new().dogstatsd();

        let mut family = MetricFamily::new("requests", MetricType::Counter, "Requests");
        family.add_sample(
            MetricSample::counter("requests", 100)
                .with_labels(MetricLabels::from([("method", "GET")])),
        );

        let output = exporter.export(&[family]).unwrap();
        assert!(output.contains("|#method:GET"));
    }

    #[test]
    fn test_carbon_exporter() {
        let exporter = CarbonExporter::new();
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        // Carbon format: metric.path value timestamp
        let lines: Vec<&str> = output.lines().collect();
        assert!(!lines.is_empty());

        // Check format
        for line in lines {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(parts.len(), 3); // name, value, timestamp
        }
    }

    #[test]
    fn test_carbon_exporter_with_prefix() {
        let exporter = CarbonExporter::new().with_prefix("hypervisor");
        let families = create_test_families();

        let output = exporter.export(&families).unwrap();

        assert!(output.contains("hypervisor.requests_total"));
        assert!(output.contains("hypervisor.temperature"));
    }

    #[test]
    fn test_exporter_content_types() {
        assert_eq!(
            PrometheusExporter::new().content_type(),
            "text/plain; version=0.0.4; charset=utf-8"
        );
        assert_eq!(JsonExporter::new().content_type(), "application/json");
        assert_eq!(
            OpenTelemetryExporter::new("test").content_type(),
            "application/json"
        );
        assert_eq!(StatsDExporter::new().content_type(), "text/plain");
        assert_eq!(CarbonExporter::new().content_type(), "text/plain");
    }

    #[test]
    fn test_exporter_names() {
        assert_eq!(PrometheusExporter::new().name(), "prometheus");
        assert_eq!(JsonExporter::new().name(), "json");
        assert_eq!(OpenTelemetryExporter::new("test").name(), "opentelemetry");
        assert_eq!(StatsDExporter::new().name(), "statsd");
        assert_eq!(CarbonExporter::new().name(), "carbon");
    }

    #[test]
    fn test_exporter_error_display() {
        let err = ExporterError::SerializationError("test".to_string());
        assert!(format!("{}", err).contains("Serialization"));

        let err = ExporterError::IoError("test".to_string());
        assert!(format!("{}", err).contains("IO"));

        let err = ExporterError::FormatError("test".to_string());
        assert!(format!("{}", err).contains("Format"));

        let err = ExporterError::NetworkError("test".to_string());
        assert!(format!("{}", err).contains("Network"));
    }

    #[test]
    fn test_histogram_prometheus_export() {
        let exporter = PrometheusExporter::new();

        let mut hist = HistogramData::with_buckets(&[0.1, 0.5, 1.0]);
        hist.observe(0.05);
        hist.observe(0.25);
        hist.observe(2.0);

        let mut family = MetricFamily::new("latency", MetricType::Histogram, "Request latency");
        family.add_sample(MetricSample::histogram("latency", hist));

        let output = exporter.export(&[family]).unwrap();

        assert!(output.contains("latency_bucket{le=\"0.1\"} 1"));
        assert!(output.contains("latency_bucket{le=\"0.5\"} 2"));
        assert!(output.contains("latency_bucket{le=\"1\"} 2"));
        assert!(output.contains("latency_bucket{le=\"+Inf\"} 3"));
        assert!(output.contains("latency_sum"));
        assert!(output.contains("latency_count 3"));
    }

    #[test]
    fn test_summary_prometheus_export() {
        let exporter = PrometheusExporter::new();

        let summary = super::super::types::SummaryData {
            count: 100,
            sum: 250.0,
            quantiles: vec![
                super::super::types::SummaryQuantile::new(0.5, 2.0),
                super::super::types::SummaryQuantile::new(0.9, 5.0),
                super::super::types::SummaryQuantile::new(0.99, 10.0),
            ],
        };

        let mut family = MetricFamily::new("response_time", MetricType::Summary, "Response time");
        family.add_sample(MetricSample::new(
            "response_time",
            MetricType::Summary,
            MetricValue::Summary(summary),
        ));

        let output = exporter.export(&[family]).unwrap();

        assert!(output.contains("response_time{quantile=\"0.5\"} 2"));
        assert!(output.contains("response_time{quantile=\"0.9\"} 5"));
        assert!(output.contains("response_time{quantile=\"0.99\"} 10"));
        assert!(output.contains("response_time_sum 250"));
        assert!(output.contains("response_time_count 100"));
    }

    #[test]
    fn test_info_prometheus_export() {
        let exporter = PrometheusExporter::new();

        let mut info = HashMap::new();
        info.insert("version".to_string(), "1.0.0".to_string());
        info.insert("commit".to_string(), "abc123".to_string());

        let mut family = MetricFamily::new("build_info", MetricType::Info, "Build information");
        family.add_sample(MetricSample::new(
            "build_info",
            MetricType::Info,
            MetricValue::Info(info),
        ));

        let output = exporter.export(&[family]).unwrap();

        assert!(output.contains("build_info"));
        assert!(output.contains("version=\"1.0.0\""));
        assert!(output.contains("commit=\"abc123\""));
        assert!(output.contains("} 1"));
    }
}
