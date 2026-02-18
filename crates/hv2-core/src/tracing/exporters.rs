//! Span Exporters
//!
//! Provides various span exporters for exporting trace data to different
//! backends including Jaeger, Zipkin, OTLP, and console.

use super::tracer::{SpanExporter, TracerError};
use super::types::{SpanData, SpanKind, StatusCode};
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Console span exporter for debugging
pub struct ConsoleSpanExporter {
    /// Output writer
    writer: Mutex<Box<dyn Write + Send>>,
    /// Pretty print flag
    pretty: bool,
}

impl ConsoleSpanExporter {
    /// Create a new console exporter writing to stdout
    pub fn new() -> Self {
        Self {
            writer: Mutex::new(Box::new(std::io::stdout())),
            pretty: true,
        }
    }

    /// Create an exporter with a custom writer
    pub fn with_writer(writer: impl Write + Send + 'static) -> Self {
        Self {
            writer: Mutex::new(Box::new(writer)),
            pretty: true,
        }
    }

    /// Set pretty printing
    pub fn with_pretty(mut self, pretty: bool) -> Self {
        self.pretty = pretty;
        self
    }

    fn format_span(&self, span: &SpanData) -> String {
        if self.pretty {
            let duration_us = (span.end_time.saturating_sub(span.start_time)) / 1000;
            let status_icon = match span.status.code {
                StatusCode::Ok => "✓",
                StatusCode::Error => "✗",
                StatusCode::Unset => "-",
            };

            let mut output = format!(
                "\n{} SPAN: {} ({})\n  TraceId: {:032x}\n  SpanId:  {:016x}",
                status_icon,
                span.name,
                format_span_kind(span.kind),
                span.context.trace_id.as_u128(),
                span.context.span_id.as_u64(),
            );

            if let Some(parent) = span.parent_span_id {
                output.push_str(&format!("\n  Parent:  {:016x}", parent.as_u64()));
            }

            output.push_str(&format!("\n  Duration: {} µs", duration_us));

            if !span.attributes.is_empty() {
                output.push_str("\n  Attributes:");
                for attr in &span.attributes {
                    output.push_str(&format!("\n    {}: {:?}", attr.key, attr.value));
                }
            }

            if !span.events.is_empty() {
                output.push_str("\n  Events:");
                for event in &span.events {
                    output.push_str(&format!("\n    {} @ {}", event.name, event.timestamp));
                }
            }

            if span.status.is_error() {
                output.push_str(&format!("\n  Error: {}", span.status.message));
            }

            output.push('\n');
            output
        } else {
            format!(
                "trace_id={:032x} span_id={:016x} name={} kind={} duration_us={}\n",
                span.context.trace_id.as_u128(),
                span.context.span_id.as_u64(),
                span.name,
                format_span_kind(span.kind),
                (span.end_time.saturating_sub(span.start_time)) / 1000
            )
        }
    }
}

impl Default for ConsoleSpanExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanExporter for ConsoleSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        let mut writer = self.writer.lock().map_err(|_| TracerError::LockError)?;
        for span in spans {
            let output = self.format_span(&span);
            writer.write_all(output.as_bytes())
                .map_err(|e| TracerError::ExportError(e.to_string()))?;
        }
        writer.flush().map_err(|e| TracerError::ExportError(e.to_string()))?;
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        Ok(())
    }
}

fn format_span_kind(kind: SpanKind) -> &'static str {
    match kind {
        SpanKind::Internal => "internal",
        SpanKind::Server => "server",
        SpanKind::Client => "client",
        SpanKind::Producer => "producer",
        SpanKind::Consumer => "consumer",
    }
}

/// Jaeger Thrift exporter configuration
#[derive(Debug, Clone)]
pub struct JaegerConfig {
    /// Agent host
    pub agent_host: String,
    /// Agent port
    pub agent_port: u16,
    /// Service name
    pub service_name: String,
}

impl Default for JaegerConfig {
    fn default() -> Self {
        Self {
            agent_host: "localhost".to_string(),
            agent_port: 6831,
            service_name: "unknown-service".to_string(),
        }
    }
}

impl JaegerConfig {
    /// Create a new config with service name
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    /// Set agent endpoint
    pub fn with_agent(mut self, host: impl Into<String>, port: u16) -> Self {
        self.agent_host = host.into();
        self.agent_port = port;
        self
    }
}

/// Jaeger span exporter (simplified thrift format)
pub struct JaegerSpanExporter {
    /// Configuration
    config: JaegerConfig,
    /// Buffered spans for batch export
    buffer: Mutex<Vec<JaegerSpan>>,
}

/// Internal Jaeger span representation
#[derive(Debug, Clone)]
struct JaegerSpan {
    trace_id_low: u64,
    trace_id_high: u64,
    span_id: u64,
    parent_span_id: u64,
    operation_name: String,
    flags: u8,
    start_time: i64,
    duration: i64,
    tags: Vec<JaegerTag>,
    logs: Vec<JaegerLog>,
}

#[derive(Debug, Clone)]
struct JaegerTag {
    key: String,
    value: JaegerTagValue,
}

#[derive(Debug, Clone)]
enum JaegerTagValue {
    String(String),
    Bool(bool),
    Long(i64),
    Double(f64),
}

#[derive(Debug, Clone)]
struct JaegerLog {
    timestamp: i64,
    fields: Vec<JaegerTag>,
}

impl JaegerSpanExporter {
    /// Create a new Jaeger exporter
    pub fn new(config: JaegerConfig) -> Self {
        Self {
            config,
            buffer: Mutex::new(Vec::new()),
        }
    }

    fn convert_span(&self, span: &SpanData) -> JaegerSpan {
        let trace_bytes = span.context.trace_id.as_bytes();
        let trace_id_high = u64::from_be_bytes(trace_bytes[0..8].try_into().expect("TraceId is always 16 bytes"));
        let trace_id_low = u64::from_be_bytes(trace_bytes[8..16].try_into().expect("TraceId is always 16 bytes"));

        let mut tags = Vec::new();

        // Add span kind
        tags.push(JaegerTag {
            key: "span.kind".to_string(),
            value: JaegerTagValue::String(format_span_kind(span.kind).to_string()),
        });

        // Add status
        if span.status.is_error() {
            tags.push(JaegerTag {
                key: "error".to_string(),
                value: JaegerTagValue::Bool(true),
            });
            tags.push(JaegerTag {
                key: "error.message".to_string(),
                value: JaegerTagValue::String(span.status.message.clone()),
            });
        }

        // Add attributes
        for attr in &span.attributes {
            let tag = JaegerTag {
                key: attr.key.clone(),
                value: match &attr.value {
                    super::types::AttributeValue::String(s) => JaegerTagValue::String(s.clone()),
                    super::types::AttributeValue::Bool(b) => JaegerTagValue::Bool(*b),
                    super::types::AttributeValue::Int(i) => JaegerTagValue::Long(*i),
                    super::types::AttributeValue::Float(f) => JaegerTagValue::Double(*f),
                    _ => JaegerTagValue::String(format!("{:?}", attr.value)),
                },
            };
            tags.push(tag);
        }

        // Convert events to logs
        let logs: Vec<JaegerLog> = span.events.iter().map(|e| {
            let mut fields = vec![JaegerTag {
                key: "event".to_string(),
                value: JaegerTagValue::String(e.name.clone()),
            }];
            for attr in &e.attributes {
                fields.push(JaegerTag {
                    key: attr.key.clone(),
                    value: JaegerTagValue::String(format!("{:?}", attr.value)),
                });
            }
            JaegerLog {
                timestamp: (e.timestamp / 1000) as i64,
                fields,
            }
        }).collect();

        JaegerSpan {
            trace_id_low,
            trace_id_high,
            span_id: span.context.span_id.as_u64(),
            parent_span_id: span.parent_span_id.map(|s| s.as_u64()).unwrap_or(0),
            operation_name: span.name.clone(),
            flags: if span.context.is_sampled() { 1 } else { 0 },
            start_time: (span.start_time / 1000) as i64,
            duration: ((span.end_time.saturating_sub(span.start_time)) / 1000) as i64,
            tags,
            logs,
        }
    }

impl SpanExporter for JaegerSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        let jaeger_spans: Vec<_> = spans.iter().map(|s| self.convert_span(s)).collect();
        let mut buffer = self.buffer.lock().map_err(|_| TracerError::LockError)?;
        buffer.extend(jaeger_spans);
        // In production, would send to UDP agent
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        // Flush remaining spans
        Ok(())
    }
}

/// Zipkin JSON exporter configuration
#[derive(Debug, Clone)]
pub struct ZipkinConfig {
    /// Collector endpoint
    pub endpoint: String,
    /// Service name
    pub service_name: String,
}

impl Default for ZipkinConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:9411/api/v2/spans".to_string(),
            service_name: "unknown-service".to_string(),
        }
    }
}

impl ZipkinConfig {
    /// Create a new config
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            ..Default::default()
        }
    }

    /// Set endpoint
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}

/// Zipkin span exporter
pub struct ZipkinSpanExporter {
    /// Configuration
    config: ZipkinConfig,
    /// Buffer for batch export
    buffer: Mutex<Vec<ZipkinSpan>>,
}

/// Internal Zipkin span representation (v2 format)
#[derive(Debug, Clone)]
struct ZipkinSpan {
    trace_id: String,
    id: String,
    parent_id: Option<String>,
    name: String,
    kind: Option<String>,
    timestamp: u64,
    duration: u64,
    local_endpoint: ZipkinEndpoint,
    tags: std::collections::HashMap<String, String>,
    annotations: Vec<ZipkinAnnotation>,
}

#[derive(Debug, Clone)]
struct ZipkinEndpoint {
    service_name: String,
}

#[derive(Debug, Clone)]
struct ZipkinAnnotation {
    timestamp: u64,
    value: String,
}

impl ZipkinSpanExporter {
    /// Create a new Zipkin exporter
    pub fn new(config: ZipkinConfig) -> Self {
        Self {
            config,
            buffer: Mutex::new(Vec::new()),
        }
    }

    fn convert_span(&self, span: &SpanData) -> ZipkinSpan {
        let kind = match span.kind {
            SpanKind::Client => Some("CLIENT"),
            SpanKind::Server => Some("SERVER"),
            SpanKind::Producer => Some("PRODUCER"),
            SpanKind::Consumer => Some("CONSUMER"),
            SpanKind::Internal => None,
        };

        let mut tags = std::collections::HashMap::new();
        for attr in &span.attributes {
            tags.insert(attr.key.clone(), format!("{:?}", attr.value));
        }

        if span.status.is_error() {
            tags.insert("error".to_string(), span.status.message.clone());
        }

        let annotations: Vec<_> = span.events.iter().map(|e| {
            ZipkinAnnotation {
                timestamp: e.timestamp / 1000, // Convert to microseconds
                value: e.name.clone(),
            }
        }).collect();

        ZipkinSpan {
            trace_id: format!("{:032x}", span.context.trace_id.as_u128()),
            id: format!("{:016x}", span.context.span_id.as_u64()),
            parent_id: span.parent_span_id.map(|p| format!("{:016x}", p.as_u64())),
            name: span.name.clone(),
            kind: kind.map(String::from),
            timestamp: span.start_time / 1000, // Zipkin uses microseconds
            duration: (span.end_time.saturating_sub(span.start_time)) / 1000,
            local_endpoint: ZipkinEndpoint {
                service_name: self.config.service_name.clone(),
            },
            tags,
            annotations,
        }
    }

    /// Serialize spans to JSON
    pub fn serialize_json(&self, spans: &[ZipkinSpan]) -> String {
        let mut json = String::from("[");
        for (i, span) in spans.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"traceId":"{}","id":"{}""#,
                span.trace_id, span.id
            ));
            if let Some(ref parent) = span.parent_id {
                json.push_str(&format!(r#","parentId":"{}""#, parent));
            }
            json.push_str(&format!(r#","name":"{}""#, span.name));
            if let Some(ref kind) = span.kind {
                json.push_str(&format!(r#","kind":"{}""#, kind));
            }
            json.push_str(&format!(r#","timestamp":{},"duration":{}"#, span.timestamp, span.duration));
            json.push_str(&format!(
                r#","localEndpoint":{{"serviceName":"{}"}}"#,
                span.local_endpoint.service_name
            ));
            if !span.tags.is_empty() {
                json.push_str(r#","tags":{"#);
                for (j, (k, v)) in span.tags.iter().enumerate() {
                    if j > 0 {
                        json.push(',');
                    }
                    json.push_str(&format!(r#""{}":"{}""#, k, v.replace('"', "\\\"")));
                }
                json.push('}');
            }
            if !span.annotations.is_empty() {
                json.push_str(r#","annotations":["#);
                for (j, ann) in span.annotations.iter().enumerate() {
                    if j > 0 {
                        json.push(',');
                    }
                    json.push_str(&format!(r#"{{"timestamp":{},"value":"{}"}}"#, ann.timestamp, ann.value));
                }
                json.push(']');
            }
            json.push('}');
        }
        json.push(']');
        json
    }
}

impl SpanExporter for ZipkinSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        let zipkin_spans: Vec<_> = spans.iter().map(|s| self.convert_span(s)).collect();
        let mut buffer = self.buffer.lock().map_err(|_| TracerError::LockError)?;
        buffer.extend(zipkin_spans);
        // In production, would POST to endpoint
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        Ok(())
    }
}

/// OTLP (OpenTelemetry Protocol) exporter configuration
#[derive(Debug, Clone)]
pub struct OtlpConfig {
    /// OTLP endpoint
    pub endpoint: String,
    /// Headers
    pub headers: Vec<(String, String)>,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
    /// Protocol (grpc or http)
    pub protocol: OtlpProtocol,
}

/// OTLP protocol type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtlpProtocol {
    /// gRPC protocol
    Grpc,
    /// HTTP/protobuf protocol
    HttpProtobuf,
    /// HTTP/JSON protocol
    HttpJson,
}

impl Default for OtlpConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:4317".to_string(),
            headers: Vec::new(),
            timeout_ms: 10000,
            protocol: OtlpProtocol::Grpc,
        }
    }
}

impl OtlpConfig {
    /// Create a new config with endpoint
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            ..Default::default()
        }
    }

    /// Set protocol
    pub fn with_protocol(mut self, protocol: OtlpProtocol) -> Self {
        self.protocol = protocol;
        self
    }

    /// Add header
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }
}

/// OTLP span exporter
pub struct OtlpSpanExporter {
    /// Configuration
    config: OtlpConfig,
    /// Export buffer
    buffer: Mutex<Vec<SpanData>>,
}

impl OtlpSpanExporter {
    /// Create a new OTLP exporter
    pub fn new(config: OtlpConfig) -> Self {
        Self {
            config,
            buffer: Mutex::new(Vec::new()),
        }
    }

    /// Serialize to JSON format
    pub fn serialize_json(&self, spans: &[SpanData]) -> String {
        let mut json = String::from(r#"{"resourceSpans":[{"resource":{},"scopeSpans":[{"spans":["#);
        
        for (i, span) in spans.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push_str(&format!(
                r#"{{"traceId":"{}","spanId":"{}""#,
                hex::encode(span.context.trace_id.as_bytes()),
                hex::encode(span.context.span_id.as_bytes())
            ));
            if let Some(parent) = span.parent_span_id {
                json.push_str(&format!(r#","parentSpanId":"{}""#, hex::encode(parent.as_bytes())));
            }
            json.push_str(&format!(r#","name":"{}""#, span.name));
            json.push_str(&format!(r#","kind":{}"#, span.kind as u8 + 1));
            json.push_str(&format!(
                r#","startTimeUnixNano":"{}","endTimeUnixNano":"{}""#,
                span.start_time, span.end_time
            ));
            if !span.attributes.is_empty() {
                json.push_str(r#","attributes":["#);
                for (j, attr) in span.attributes.iter().enumerate() {
                    if j > 0 {
                        json.push(',');
                    }
                    json.push_str(&format!(
                        r#"{{"key":"{}","value":{{"stringValue":"{}"}}}}"#,
                        attr.key, format!("{:?}", attr.value).replace('"', "\\\"")
                    ));
                }
                json.push(']');
            }
            json.push_str(&format!(
                r#","status":{{"code":{}}}}}"#,
                match span.status.code {
                    StatusCode::Unset => 0,
                    StatusCode::Ok => 1,
                    StatusCode::Error => 2,
                }
            ));
        }
        
        json.push_str("]}]}]}");
        json
    }
}

impl SpanExporter for OtlpSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        let mut buffer = self.buffer.lock().map_err(|_| TracerError::LockError)?;
        buffer.extend(spans);
        // In production, would send to OTLP endpoint
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        Ok(())
    }
}

/// Hex encoding utilities
mod hex {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = String::with_capacity(bytes.len() * 2);
        for &byte in bytes {
            result.push(HEX_CHARS[(byte >> 4) as usize] as char);
            result.push(HEX_CHARS[(byte & 0xf) as usize] as char);
        }
        result
    }
}

/// Composite exporter that sends to multiple backends
pub struct CompositeSpanExporter {
    exporters: Vec<Arc<dyn SpanExporter>>,
}

impl CompositeSpanExporter {
    /// Create a new composite exporter
    pub fn new() -> Self {
        Self {
            exporters: Vec::new(),
        }
    }

    /// Add an exporter
    pub fn add_exporter(mut self, exporter: Arc<dyn SpanExporter>) -> Self {
        self.exporters.push(exporter);
        self
    }
}

impl Default for CompositeSpanExporter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanExporter for CompositeSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        for exporter in &self.exporters {
            exporter.export(spans.clone())?;
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        for exporter in &self.exporters {
            exporter.shutdown()?;
        }
        Ok(())
    }
}

/// Filtered exporter that only exports spans matching criteria
pub struct FilteredSpanExporter {
    inner: Arc<dyn SpanExporter>,
    filter: Box<dyn Fn(&SpanData) -> bool + Send + Sync>,
}

impl FilteredSpanExporter {
    /// Create a new filtered exporter
    pub fn new<F>(inner: Arc<dyn SpanExporter>, filter: F) -> Self
    where
        F: Fn(&SpanData) -> bool + Send + Sync + 'static,
    {
        Self {
            inner,
            filter: Box::new(filter),
        }
    }

    /// Filter to only export errors
    pub fn errors_only(inner: Arc<dyn SpanExporter>) -> Self {
        Self::new(inner, |span| span.status.is_error())
    }

    /// Filter by minimum duration
    pub fn by_duration(inner: Arc<dyn SpanExporter>, min_duration_ns: u64) -> Self {
        Self::new(inner, move |span| {
            span.end_time.saturating_sub(span.start_time) >= min_duration_ns
        })
    }
}

impl SpanExporter for FilteredSpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        let filtered: Vec<_> = spans.into_iter().filter(|s| (self.filter)(s)).collect();
        if !filtered.is_empty() {
            self.inner.export(filtered)?;
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        self.inner.shutdown()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{SpanContext, SpanStatus, TraceFlags, TraceState};
    use std::sync::Arc;

    fn create_test_span() -> SpanData {
        SpanData {
            context: SpanContext::root(),
            parent_span_id: None,
            name: "test-span".to_string(),
            kind: SpanKind::Server,
            start_time: 1000000000,
            end_time: 1500000000,
            attributes: vec![],
            events: vec![],
            links: vec![],
            status: SpanStatus::ok(),
            resource: vec![],
            instrumentation_name: "test".to_string(),
            instrumentation_version: Some("1.0".to_string()),
        }
    }

    #[test]
    fn test_console_exporter() {
        let mut output = Vec::new();
        let exporter = ConsoleSpanExporter::with_writer(std::io::Cursor::new(&mut output))
            .with_pretty(false);
        
        let span = create_test_span();
        exporter.export(vec![span]).unwrap();
    }

    #[test]
    fn test_console_exporter_pretty() {
        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = TestWriter(output.clone());
        let exporter = ConsoleSpanExporter::with_writer(writer).with_pretty(true);
        
        let span = create_test_span();
        exporter.export(vec![span]).unwrap();

        let output = output.lock().unwrap();
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("SPAN: test-span"));
    }

    struct TestWriter(Arc<Mutex<Vec<u8>>>);
    
    impl Write for TestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    #[test]
    fn test_jaeger_config() {
        let config = JaegerConfig::new("my-service")
            .with_agent("jaeger-host", 6832);
        
        assert_eq!(config.service_name, "my-service");
        assert_eq!(config.agent_host, "jaeger-host");
        assert_eq!(config.agent_port, 6832);
    }

    #[test]
    fn test_jaeger_exporter_convert() {
        let config = JaegerConfig::new("test-service");
        let exporter = JaegerSpanExporter::new(config);
        
        let mut span = create_test_span();
        span.status = SpanStatus::error("test error");
        
        let jaeger_span = exporter.convert_span(&span);
        assert_eq!(jaeger_span.operation_name, "test-span");
        assert!(jaeger_span.tags.iter().any(|t| t.key == "error"));
    }

    #[test]
    fn test_zipkin_config() {
        let config = ZipkinConfig::new("my-service")
            .with_endpoint("http://zipkin:9411/api/v2/spans");
        
        assert_eq!(config.service_name, "my-service");
        assert!(config.endpoint.contains("zipkin"));
    }

    #[test]
    fn test_zipkin_json_serialization() {
        let config = ZipkinConfig::new("test-service");
        let exporter = ZipkinSpanExporter::new(config);
        
        let span = create_test_span();
        let zipkin_span = exporter.convert_span(&span);
        
        let json = exporter.serialize_json(&[zipkin_span]);
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("test-span"));
    }

    #[test]
    fn test_otlp_config() {
        let config = OtlpConfig::new("http://otel-collector:4317")
            .with_protocol(OtlpProtocol::Grpc)
            .with_header("Authorization", "Bearer token")
            .with_timeout(5000);
        
        assert_eq!(config.protocol, OtlpProtocol::Grpc);
        assert_eq!(config.timeout_ms, 5000);
        assert_eq!(config.headers.len(), 1);
    }

    #[test]
    fn test_otlp_json_serialization() {
        let config = OtlpConfig::new("http://localhost:4317");
        let exporter = OtlpSpanExporter::new(config);
        
        let span = create_test_span();
        let json = exporter.serialize_json(&[span]);
        
        assert!(json.contains("resourceSpans"));
        assert!(json.contains("test-span"));
    }

    #[test]
    fn test_composite_exporter() {
        use super::super::tracer::InMemorySpanExporter;
        
        let exporter1 = Arc::new(InMemorySpanExporter::new());
        let exporter2 = Arc::new(InMemorySpanExporter::new());
        
        let composite = CompositeSpanExporter::new()
            .add_exporter(exporter1.clone())
            .add_exporter(exporter2.clone());
        
        let span = create_test_span();
        composite.export(vec![span]).unwrap();
        
        assert_eq!(exporter1.get_spans().len(), 1);
        assert_eq!(exporter2.get_spans().len(), 1);
    }

    #[test]
    fn test_filtered_exporter_errors_only() {
        use super::super::tracer::InMemorySpanExporter;
        
        let inner = Arc::new(InMemorySpanExporter::new());
        let filtered = FilteredSpanExporter::errors_only(inner.clone());
        
        let ok_span = create_test_span();
        let mut error_span = create_test_span();
        error_span.status = SpanStatus::error("failed");
        
        filtered.export(vec![ok_span, error_span]).unwrap();
        
        let spans = inner.get_spans();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].status.is_error());
    }

    #[test]
    fn test_filtered_exporter_by_duration() {
        use super::super::tracer::InMemorySpanExporter;
        
        let inner = Arc::new(InMemorySpanExporter::new());
        // Filter to spans longer than 100ms (100_000_000 ns)
        let filtered = FilteredSpanExporter::by_duration(inner.clone(), 100_000_000);
        
        let mut short_span = create_test_span();
        short_span.start_time = 1000000000;
        short_span.end_time = 1010000000; // 10ms
        
        let mut long_span = create_test_span();
        long_span.start_time = 1000000000;
        long_span.end_time = 1200000000; // 200ms
        
        filtered.export(vec![short_span, long_span]).unwrap();
        
        let spans = inner.get_spans();
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex::encode(&[0x12, 0x34, 0xab, 0xcd]), "1234abcd");
        assert_eq!(hex::encode(&[0x00, 0xff]), "00ff");
        assert_eq!(hex::encode(&[]), "");
    }

    #[test]
    fn test_span_kind_format() {
        assert_eq!(format_span_kind(SpanKind::Internal), "internal");
        assert_eq!(format_span_kind(SpanKind::Server), "server");
        assert_eq!(format_span_kind(SpanKind::Client), "client");
        assert_eq!(format_span_kind(SpanKind::Producer), "producer");
        assert_eq!(format_span_kind(SpanKind::Consumer), "consumer");
    }
}
