//! Tracing Types
//!
//! Core types for distributed tracing and span management including
//! trace/span identifiers, context propagation, events, and attributes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Trace ID - 128-bit unique identifier for a trace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceId {
    high: u64,
    low: u64,
}

impl TraceId {
    /// Create a new random trace ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);

        Self {
            high: timestamp,
            low: counter ^ (std::process::id() as u64) << 32,
        }
    }

    /// Create from high and low parts
    pub fn from_parts(high: u64, low: u64) -> Self {
        Self { high, low }
    }

    /// Create from bytes (16 bytes, big-endian)
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        let high = u64::from_be_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let low = u64::from_be_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        Self { high, low }
    }

    /// Convert to bytes (16 bytes, big-endian)
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.high.to_be_bytes());
        bytes[8..16].copy_from_slice(&self.low.to_be_bytes());
        bytes
    }

    /// Get high 64 bits
    pub fn high(&self) -> u64 {
        self.high
    }

    /// Get low 64 bits
    pub fn low(&self) -> u64 {
        self.low
    }

    /// Get as u128
    pub fn as_u128(&self) -> u128 {
        ((self.high as u128) << 64) | (self.low as u128)
    }

    /// Check if trace ID is valid (non-zero)
    pub fn is_valid(&self) -> bool {
        self.high != 0 || self.low != 0
    }

    /// Create an invalid/empty trace ID
    pub fn invalid() -> Self {
        Self { high: 0, low: 0 }
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}{:016x}", self.high, self.low)
    }
}

/// Span ID - 64-bit unique identifier for a span within a trace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanId(u64);

impl SpanId {
    /// Create a new random span ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        Self(counter ^ (timestamp & 0xFFFF_FFFF))
    }

    /// Create from u64
    pub fn from_u64(value: u64) -> Self {
        Self(value)
    }

    /// Create from bytes (8 bytes, big-endian)
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        Self(u64::from_be_bytes(bytes))
    }

    /// Convert to bytes (8 bytes, big-endian)
    pub fn to_bytes(&self) -> [u8; 8] {
        self.0.to_be_bytes()
    }

    /// Get as u64
    pub fn as_u64(&self) -> u64 {
        self.0
    }

    /// Check if span ID is valid (non-zero)
    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    /// Create an invalid/empty span ID
    pub fn invalid() -> Self {
        Self(0)
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:016x}", self.0)
    }
}

/// Trace flags for sampling and debugging
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceFlags(u8);

impl TraceFlags {
    /// No flags set
    pub const NONE: TraceFlags = TraceFlags(0);
    /// Trace is sampled
    pub const SAMPLED: TraceFlags = TraceFlags(0x01);
    /// Random sampling decision
    pub const RANDOM: TraceFlags = TraceFlags(0x02);

    /// Create new trace flags
    pub fn new(value: u8) -> Self {
        Self(value)
    }

    /// Check if sampled flag is set
    pub fn is_sampled(&self) -> bool {
        self.0 & 0x01 != 0
    }

    /// Set sampled flag
    pub fn set_sampled(&mut self, sampled: bool) {
        if sampled {
            self.0 |= 0x01;
        } else {
            self.0 &= !0x01;
        }
    }

    /// Get raw value
    pub fn as_u8(&self) -> u8 {
        self.0
    }
}

impl Default for TraceFlags {
    fn default() -> Self {
        Self::SAMPLED
    }
}

/// Span context - carries trace identity across process boundaries
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SpanContext {
    /// Trace ID
    pub trace_id: TraceId,
    /// Span ID
    pub span_id: SpanId,
    /// Trace flags
    pub trace_flags: TraceFlags,
    /// Trace state (vendor-specific key-value pairs)
    pub trace_state: TraceState,
    /// Whether this context is from a remote parent
    pub is_remote: bool,
}

impl SpanContext {
    /// Create a new span context
    pub fn new(trace_id: TraceId, span_id: SpanId) -> Self {
        Self {
            trace_id,
            span_id,
            trace_flags: TraceFlags::SAMPLED,
            trace_state: TraceState::new(),
            is_remote: false,
        }
    }

    /// Create a new root span context
    pub fn root() -> Self {
        Self::new(TraceId::new(), SpanId::new())
    }

    /// Create a child span context
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id,
            span_id: SpanId::new(),
            trace_flags: self.trace_flags,
            trace_state: self.trace_state.clone(),
            is_remote: false,
        }
    }

    /// Check if context is valid
    pub fn is_valid(&self) -> bool {
        self.trace_id.is_valid() && self.span_id.is_valid()
    }

    /// Check if sampled
    pub fn is_sampled(&self) -> bool {
        self.trace_flags.is_sampled()
    }

    /// Create an invalid context
    pub fn invalid() -> Self {
        Self {
            trace_id: TraceId::invalid(),
            span_id: SpanId::invalid(),
            trace_flags: TraceFlags::NONE,
            trace_state: TraceState::new(),
            is_remote: false,
        }
    }

    /// Parse from W3C traceparent header
    pub fn from_traceparent(header: &str) -> Option<Self> {
        let parts: Vec<&str> = header.split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        // Version must be "00"
        if parts[0] != "00" {
            return None;
        }

        // Parse trace ID (32 hex chars)
        if parts[1].len() != 32 {
            return None;
        }
        let trace_high = u64::from_str_radix(&parts[1][0..16], 16).ok()?;
        let trace_low = u64::from_str_radix(&parts[1][16..32], 16).ok()?;

        // Parse span ID (16 hex chars)
        if parts[2].len() != 16 {
            return None;
        }
        let span_id = u64::from_str_radix(parts[2], 16).ok()?;

        // Parse flags (2 hex chars)
        if parts[3].len() != 2 {
            return None;
        }
        let flags = u8::from_str_radix(parts[3], 16).ok()?;

        Some(Self {
            trace_id: TraceId::from_parts(trace_high, trace_low),
            span_id: SpanId::from_u64(span_id),
            trace_flags: TraceFlags::new(flags),
            trace_state: TraceState::new(),
            is_remote: true,
        })
    }

    /// Format as W3C traceparent header
    pub fn to_traceparent(&self) -> String {
        format!(
            "00-{:016x}{:016x}-{:016x}-{:02x}",
            self.trace_id.high(),
            self.trace_id.low(),
            self.span_id.as_u64(),
            self.trace_flags.as_u8()
        )
    }
}

impl Default for SpanContext {
    fn default() -> Self {
        Self::root()
    }
}

/// Trace state - vendor-specific key-value pairs
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TraceState {
    entries: Vec<(String, String)>,
}

impl TraceState {
    /// Create empty trace state
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Get a value by key
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// Set a value
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();

        // Remove existing entry with same key
        self.entries.retain(|(k, _)| k != &key);

        // Add new entry at the front (most recent)
        self.entries.insert(0, (key, value));

        // Limit to 32 entries per spec
        self.entries.truncate(32);
    }

    /// Remove a value
    pub fn delete(&mut self, key: &str) {
        self.entries.retain(|(k, _)| k != key);
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Iterate over entries
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// Parse from tracestate header
    pub fn from_header(header: &str) -> Self {
        let mut state = Self::new();
        for entry in header.split(',') {
            let entry = entry.trim();
            if let Some((key, value)) = entry.split_once('=') {
                state.entries.push((key.to_string(), value.to_string()));
            }
        }
        state
    }

    /// Format as tracestate header
    pub fn to_header(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }
}

/// Span kind - describes the relationship of the span
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum SpanKind {
    /// Internal operation (default)
    #[default]
    Internal,
    /// Server handling a request
    Server,
    /// Client making a request
    Client,
    /// Producer sending a message
    Producer,
    /// Consumer receiving a message
    Consumer,
}

impl fmt::Display for SpanKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SpanKind::Internal => write!(f, "internal"),
            SpanKind::Server => write!(f, "server"),
            SpanKind::Client => write!(f, "client"),
            SpanKind::Producer => write!(f, "producer"),
            SpanKind::Consumer => write!(f, "consumer"),
        }
    }
}

/// Span status code
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum StatusCode {
    /// Unset (default)
    #[default]
    Unset,
    /// Operation completed successfully
    Ok,
    /// Operation failed with an error
    Error,
}

/// Span status
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanStatus {
    /// Status code
    pub code: StatusCode,
    /// Description (only used for Error status)
    pub message: String,
}

impl SpanStatus {
    /// Create unset status
    pub fn unset() -> Self {
        Self {
            code: StatusCode::Unset,
            message: String::new(),
        }
    }

    /// Create OK status
    pub fn ok() -> Self {
        Self {
            code: StatusCode::Ok,
            message: String::new(),
        }
    }

    /// Create error status
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            code: StatusCode::Error,
            message: message.into(),
        }
    }

    /// Check if error
    pub fn is_error(&self) -> bool {
        self.code == StatusCode::Error
    }
}

/// Attribute value types
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttributeValue {
    /// String value
    String(String),
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// Floating point value
    Float(f64),
    /// String array
    StringArray(Vec<String>),
    /// Boolean array
    BoolArray(Vec<bool>),
    /// Integer array
    IntArray(Vec<i64>),
    /// Float array
    FloatArray(Vec<f64>),
}

impl AttributeValue {
    /// Create string attribute
    pub fn string(value: impl Into<String>) -> Self {
        AttributeValue::String(value.into())
    }

    /// Create bool attribute
    pub fn bool(value: bool) -> Self {
        AttributeValue::Bool(value)
    }

    /// Create int attribute
    pub fn int(value: i64) -> Self {
        AttributeValue::Int(value)
    }

    /// Create float attribute
    pub fn float(value: f64) -> Self {
        AttributeValue::Float(value)
    }
}

impl fmt::Display for AttributeValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AttributeValue::String(s) => write!(f, "{}", s),
            AttributeValue::Bool(b) => write!(f, "{}", b),
            AttributeValue::Int(i) => write!(f, "{}", i),
            AttributeValue::Float(fl) => write!(f, "{}", fl),
            AttributeValue::StringArray(arr) => write!(f, "{:?}", arr),
            AttributeValue::BoolArray(arr) => write!(f, "{:?}", arr),
            AttributeValue::IntArray(arr) => write!(f, "{:?}", arr),
            AttributeValue::FloatArray(arr) => write!(f, "{:?}", arr),
        }
    }
}

impl From<&str> for AttributeValue {
    fn from(s: &str) -> Self {
        AttributeValue::String(s.to_string())
    }
}

impl From<String> for AttributeValue {
    fn from(s: String) -> Self {
        AttributeValue::String(s)
    }
}

impl From<bool> for AttributeValue {
    fn from(b: bool) -> Self {
        AttributeValue::Bool(b)
    }
}

impl From<i64> for AttributeValue {
    fn from(i: i64) -> Self {
        AttributeValue::Int(i)
    }
}

impl From<i32> for AttributeValue {
    fn from(i: i32) -> Self {
        AttributeValue::Int(i as i64)
    }
}

impl From<u64> for AttributeValue {
    fn from(i: u64) -> Self {
        AttributeValue::Int(i as i64)
    }
}

impl From<u32> for AttributeValue {
    fn from(i: u32) -> Self {
        AttributeValue::Int(i as i64)
    }
}

impl From<f64> for AttributeValue {
    fn from(f: f64) -> Self {
        AttributeValue::Float(f)
    }
}

impl From<f32> for AttributeValue {
    fn from(f: f32) -> Self {
        AttributeValue::Float(f as f64)
    }
}

/// Span attribute
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attribute {
    /// Attribute key
    pub key: String,
    /// Attribute value
    pub value: AttributeValue,
}

impl Attribute {
    /// Create a new attribute
    pub fn new(key: impl Into<String>, value: impl Into<AttributeValue>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Span event - a time-stamped annotation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    /// Event name
    pub name: String,
    /// Timestamp (Unix nanoseconds)
    pub timestamp: u64,
    /// Event attributes
    pub attributes: Vec<Attribute>,
}

impl SpanEvent {
    /// Create a new event
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as u64,
            attributes: Vec::new(),
        }
    }

    /// Add an attribute
    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<AttributeValue>,
    ) -> Self {
        self.attributes.push(Attribute::new(key, value));
        self
    }

    /// Add multiple attributes
    pub fn with_attributes(mut self, attrs: impl IntoIterator<Item = Attribute>) -> Self {
        self.attributes.extend(attrs);
        self
    }
}

/// Span link - a reference to another span
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLink {
    /// Linked span context
    pub context: SpanContext,
    /// Link attributes
    pub attributes: Vec<Attribute>,
}

impl SpanLink {
    /// Create a new link
    pub fn new(context: SpanContext) -> Self {
        Self {
            context,
            attributes: Vec::new(),
        }
    }

    /// Add an attribute
    pub fn with_attribute(
        mut self,
        key: impl Into<String>,
        value: impl Into<AttributeValue>,
    ) -> Self {
        self.attributes.push(Attribute::new(key, value));
        self
    }
}

/// Span data - completed span information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanData {
    /// Span context
    pub context: SpanContext,
    /// Parent span ID (if any)
    pub parent_span_id: Option<SpanId>,
    /// Span name
    pub name: String,
    /// Span kind
    pub kind: SpanKind,
    /// Start time (Unix nanoseconds)
    pub start_time: u64,
    /// End time (Unix nanoseconds)
    pub end_time: u64,
    /// Span attributes
    pub attributes: Vec<Attribute>,
    /// Span events
    pub events: Vec<SpanEvent>,
    /// Span links
    pub links: Vec<SpanLink>,
    /// Span status
    pub status: SpanStatus,
    /// Resource attributes (service info)
    pub resource: HashMap<String, AttributeValue>,
    /// Instrumentation scope name
    pub instrumentation_name: String,
    /// Instrumentation scope version
    pub instrumentation_version: String,
}

impl SpanData {
    /// Get duration in nanoseconds
    pub fn duration_nanos(&self) -> u64 {
        self.end_time.saturating_sub(self.start_time)
    }

    /// Get duration
    pub fn duration(&self) -> Duration {
        Duration::from_nanos(self.duration_nanos())
    }

    /// Check if span has error
    pub fn is_error(&self) -> bool {
        self.status.is_error()
    }
}

/// Sampling decision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SamplingDecision {
    /// Drop span
    Drop,
    /// Record span but don't export
    RecordOnly,
    /// Record and export span
    RecordAndSample,
}

/// Sampling result
#[derive(Debug, Clone)]
pub struct SamplingResult {
    /// Sampling decision
    pub decision: SamplingDecision,
    /// Attributes to add to span
    pub attributes: Vec<Attribute>,
    /// Updated trace state
    pub trace_state: TraceState,
}

impl SamplingResult {
    /// Create a drop result
    pub fn drop() -> Self {
        Self {
            decision: SamplingDecision::Drop,
            attributes: Vec::new(),
            trace_state: TraceState::new(),
        }
    }

    /// Create a record-only result
    pub fn record_only() -> Self {
        Self {
            decision: SamplingDecision::RecordOnly,
            attributes: Vec::new(),
            trace_state: TraceState::new(),
        }
    }

    /// Create a record-and-sample result
    pub fn record_and_sample() -> Self {
        Self {
            decision: SamplingDecision::RecordAndSample,
            attributes: Vec::new(),
            trace_state: TraceState::new(),
        }
    }
}

/// Sampler trait for sampling decisions
pub trait Sampler: Send + Sync {
    /// Make a sampling decision
    fn should_sample(
        &self,
        parent_context: Option<&SpanContext>,
        trace_id: TraceId,
        name: &str,
        kind: SpanKind,
        attributes: &[Attribute],
        links: &[SpanLink],
    ) -> SamplingResult;

    /// Get sampler description
    fn description(&self) -> &str;
}

/// Always-on sampler
#[derive(Debug, Default)]
pub struct AlwaysOnSampler;

impl Sampler for AlwaysOnSampler {
    fn should_sample(
        &self,
        _parent: Option<&SpanContext>,
        _trace_id: TraceId,
        _name: &str,
        _kind: SpanKind,
        _attributes: &[Attribute],
        _links: &[SpanLink],
    ) -> SamplingResult {
        SamplingResult::record_and_sample()
    }

    fn description(&self) -> &str {
        "AlwaysOnSampler"
    }
}

/// Always-off sampler
#[derive(Debug, Default)]
pub struct AlwaysOffSampler;

impl Sampler for AlwaysOffSampler {
    fn should_sample(
        &self,
        _parent: Option<&SpanContext>,
        _trace_id: TraceId,
        _name: &str,
        _kind: SpanKind,
        _attributes: &[Attribute],
        _links: &[SpanLink],
    ) -> SamplingResult {
        SamplingResult::drop()
    }

    fn description(&self) -> &str {
        "AlwaysOffSampler"
    }
}

/// Trace ID ratio sampler
#[derive(Debug)]
pub struct TraceIdRatioSampler {
    /// Sampling ratio (0.0 to 1.0)
    ratio: f64,
    /// Upper bound for trace ID comparison
    upper_bound: u64,
}

impl TraceIdRatioSampler {
    /// Create a new ratio sampler
    pub fn new(ratio: f64) -> Self {
        let ratio = ratio.clamp(0.0, 1.0);
        let upper_bound = (ratio * u64::MAX as f64) as u64;
        Self { ratio, upper_bound }
    }

    /// Get sampling ratio
    pub fn ratio(&self) -> f64 {
        self.ratio
    }
}

impl Sampler for TraceIdRatioSampler {
    fn should_sample(
        &self,
        _parent: Option<&SpanContext>,
        trace_id: TraceId,
        _name: &str,
        _kind: SpanKind,
        _attributes: &[Attribute],
        _links: &[SpanLink],
    ) -> SamplingResult {
        // Use low bits of trace ID for consistent sampling
        if trace_id.low() < self.upper_bound {
            SamplingResult::record_and_sample()
        } else {
            SamplingResult::drop()
        }
    }

    fn description(&self) -> &str {
        "TraceIdRatioSampler"
    }
}

/// Parent-based sampler
pub struct ParentBasedSampler {
    /// Root sampler (when no parent)
    root: Box<dyn Sampler>,
    /// Remote parent sampled
    remote_parent_sampled: Box<dyn Sampler>,
    /// Remote parent not sampled
    remote_parent_not_sampled: Box<dyn Sampler>,
    /// Local parent sampled
    local_parent_sampled: Box<dyn Sampler>,
    /// Local parent not sampled
    local_parent_not_sampled: Box<dyn Sampler>,
}

impl fmt::Debug for ParentBasedSampler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ParentBasedSampler")
            .field("root", &self.root.description())
            .finish()
    }
}

impl ParentBasedSampler {
    /// Create with root sampler
    pub fn new(root: impl Sampler + 'static) -> Self {
        Self {
            root: Box::new(root),
            remote_parent_sampled: Box::new(AlwaysOnSampler),
            remote_parent_not_sampled: Box::new(AlwaysOffSampler),
            local_parent_sampled: Box::new(AlwaysOnSampler),
            local_parent_not_sampled: Box::new(AlwaysOffSampler),
        }
    }
}

impl Sampler for ParentBasedSampler {
    fn should_sample(
        &self,
        parent: Option<&SpanContext>,
        trace_id: TraceId,
        name: &str,
        kind: SpanKind,
        attributes: &[Attribute],
        links: &[SpanLink],
    ) -> SamplingResult {
        match parent {
            None => self
                .root
                .should_sample(None, trace_id, name, kind, attributes, links),
            Some(ctx) => {
                let sampler = if ctx.is_remote {
                    if ctx.is_sampled() {
                        &self.remote_parent_sampled
                    } else {
                        &self.remote_parent_not_sampled
                    }
                } else if ctx.is_sampled() {
                    &self.local_parent_sampled
                } else {
                    &self.local_parent_not_sampled
                };
                sampler.should_sample(parent, trace_id, name, kind, attributes, links)
            }
        }
    }

    fn description(&self) -> &str {
        "ParentBasedSampler"
    }
}

/// Instrumentation scope
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentationScope {
    /// Scope name (e.g., library name)
    pub name: String,
    /// Scope version
    pub version: String,
    /// Schema URL
    pub schema_url: Option<String>,
    /// Scope attributes
    pub attributes: Vec<Attribute>,
}

impl InstrumentationScope {
    /// Create a new scope
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: String::new(),
            schema_url: None,
            attributes: Vec::new(),
        }
    }

    /// Set version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Set schema URL
    pub fn with_schema_url(mut self, url: impl Into<String>) -> Self {
        self.schema_url = Some(url.into());
        self
    }
}

/// Resource - information about the entity producing telemetry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Resource {
    /// Resource attributes
    pub attributes: HashMap<String, AttributeValue>,
    /// Schema URL
    pub schema_url: Option<String>,
}

impl Resource {
    /// Create empty resource
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with attributes
    pub fn with_attributes(attrs: impl IntoIterator<Item = (String, AttributeValue)>) -> Self {
        Self {
            attributes: attrs.into_iter().collect(),
            schema_url: None,
        }
    }

    /// Add attribute
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<AttributeValue>) {
        self.attributes.insert(key.into(), value.into());
    }

    /// Get attribute
    pub fn get(&self, key: &str) -> Option<&AttributeValue> {
        self.attributes.get(key)
    }

    /// Merge with another resource
    pub fn merge(&mut self, other: &Resource) {
        for (k, v) in &other.attributes {
            self.attributes
                .entry(k.clone())
                .or_insert_with(|| v.clone());
        }
    }

    /// Create default resource with service info
    pub fn default_service(name: &str, version: &str) -> Self {
        let mut resource = Self::new();
        resource.insert("service.name", name);
        resource.insert("service.version", version);
        resource.insert("telemetry.sdk.name", "aethervm");
        resource.insert("telemetry.sdk.language", "rust");
        resource.insert("telemetry.sdk.version", env!("CARGO_PKG_VERSION"));
        resource
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_id_generation() {
        let id1 = TraceId::new();
        let id2 = TraceId::new();
        assert_ne!(id1, id2);
        assert!(id1.is_valid());
        assert!(id2.is_valid());
    }

    #[test]
    fn test_trace_id_bytes() {
        let id = TraceId::from_parts(0x1234567890abcdef, 0xfedcba0987654321);
        let bytes = id.to_bytes();
        let restored = TraceId::from_bytes(bytes);
        assert_eq!(id, restored);
    }

    #[test]
    fn test_trace_id_display() {
        let id = TraceId::from_parts(0x0123456789abcdef, 0xfedcba9876543210);
        assert_eq!(format!("{}", id), "0123456789abcdeffedcba9876543210");
    }

    #[test]
    fn test_trace_id_invalid() {
        let invalid = TraceId::invalid();
        assert!(!invalid.is_valid());
        assert_eq!(invalid.high(), 0);
        assert_eq!(invalid.low(), 0);
    }

    #[test]
    fn test_span_id_generation() {
        let id1 = SpanId::new();
        let id2 = SpanId::new();
        assert_ne!(id1, id2);
        assert!(id1.is_valid());
    }

    #[test]
    fn test_span_id_bytes() {
        let id = SpanId::from_u64(0x1234567890abcdef);
        let bytes = id.to_bytes();
        let restored = SpanId::from_bytes(bytes);
        assert_eq!(id, restored);
    }

    #[test]
    fn test_trace_flags() {
        let mut flags = TraceFlags::NONE;
        assert!(!flags.is_sampled());

        flags.set_sampled(true);
        assert!(flags.is_sampled());
        assert_eq!(flags.as_u8(), 0x01);

        flags.set_sampled(false);
        assert!(!flags.is_sampled());
    }

    #[test]
    fn test_span_context_creation() {
        let ctx = SpanContext::root();
        assert!(ctx.is_valid());
        assert!(ctx.is_sampled());
        assert!(!ctx.is_remote);
    }

    #[test]
    fn test_span_context_child() {
        let parent = SpanContext::root();
        let child = parent.child();

        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.span_id, parent.span_id);
        assert!(child.is_valid());
    }

    #[test]
    fn test_traceparent_parsing() {
        let header = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let ctx = SpanContext::from_traceparent(header).unwrap();

        assert_eq!(ctx.trace_id.high(), 0x0af7651916cd43dd);
        assert_eq!(ctx.trace_id.low(), 0x8448eb211c80319c);
        assert_eq!(ctx.span_id.as_u64(), 0xb7ad6b7169203331);
        assert!(ctx.is_sampled());
        assert!(ctx.is_remote);
    }

    #[test]
    fn test_traceparent_roundtrip() {
        let ctx = SpanContext::root();
        let header = ctx.to_traceparent();
        let parsed = SpanContext::from_traceparent(&header).unwrap();

        assert_eq!(ctx.trace_id, parsed.trace_id);
        assert_eq!(ctx.span_id, parsed.span_id);
        assert_eq!(ctx.trace_flags.as_u8(), parsed.trace_flags.as_u8());
    }

    #[test]
    fn test_trace_state() {
        let mut state = TraceState::new();
        assert!(state.is_empty());

        state.set("vendor", "value");
        assert_eq!(state.get("vendor"), Some("value"));
        assert_eq!(state.len(), 1);

        state.delete("vendor");
        assert!(state.is_empty());
    }

    #[test]
    fn test_trace_state_header() {
        let mut state = TraceState::new();
        state.set("foo", "1");
        state.set("bar", "2");

        let header = state.to_header();
        // Most recent first
        assert!(header.contains("bar=2"));
        assert!(header.contains("foo=1"));

        let parsed = TraceState::from_header(&header);
        assert_eq!(parsed.get("foo"), Some("1"));
        assert_eq!(parsed.get("bar"), Some("2"));
    }

    #[test]
    fn test_span_kind_display() {
        assert_eq!(format!("{}", SpanKind::Internal), "internal");
        assert_eq!(format!("{}", SpanKind::Server), "server");
        assert_eq!(format!("{}", SpanKind::Client), "client");
        assert_eq!(format!("{}", SpanKind::Producer), "producer");
        assert_eq!(format!("{}", SpanKind::Consumer), "consumer");
    }

    #[test]
    fn test_span_status() {
        let ok = SpanStatus::ok();
        assert!(!ok.is_error());

        let error = SpanStatus::error("something failed");
        assert!(error.is_error());
        assert_eq!(error.message, "something failed");
    }

    #[test]
    fn test_attribute_values() {
        let s: AttributeValue = "hello".into();
        assert!(matches!(s, AttributeValue::String(_)));

        let b: AttributeValue = true.into();
        assert!(matches!(b, AttributeValue::Bool(true)));

        let i: AttributeValue = 42i64.into();
        assert!(matches!(i, AttributeValue::Int(42)));

        let f: AttributeValue = 2.78f64.into();
        assert!(matches!(f, AttributeValue::Float(_)));
    }

    #[test]
    fn test_span_event() {
        let event = SpanEvent::new("my-event")
            .with_attribute("key", "value")
            .with_attribute("count", 42i64);

        assert_eq!(event.name, "my-event");
        assert_eq!(event.attributes.len(), 2);
        assert!(event.timestamp > 0);
    }

    #[test]
    fn test_span_link() {
        let ctx = SpanContext::root();
        let link = SpanLink::new(ctx.clone()).with_attribute("reason", "retry");

        assert_eq!(link.context.trace_id, ctx.trace_id);
        assert_eq!(link.attributes.len(), 1);
    }

    #[test]
    fn test_always_on_sampler() {
        let sampler = AlwaysOnSampler;
        let result =
            sampler.should_sample(None, TraceId::new(), "test", SpanKind::Internal, &[], &[]);
        assert_eq!(result.decision, SamplingDecision::RecordAndSample);
    }

    #[test]
    fn test_always_off_sampler() {
        let sampler = AlwaysOffSampler;
        let result =
            sampler.should_sample(None, TraceId::new(), "test", SpanKind::Internal, &[], &[]);
        assert_eq!(result.decision, SamplingDecision::Drop);
    }

    #[test]
    fn test_ratio_sampler() {
        let sampler = TraceIdRatioSampler::new(1.0);
        let result =
            sampler.should_sample(None, TraceId::new(), "test", SpanKind::Internal, &[], &[]);
        assert_eq!(result.decision, SamplingDecision::RecordAndSample);

        let sampler = TraceIdRatioSampler::new(0.0);
        let result =
            sampler.should_sample(None, TraceId::new(), "test", SpanKind::Internal, &[], &[]);
        assert_eq!(result.decision, SamplingDecision::Drop);
    }

    #[test]
    fn test_instrumentation_scope() {
        let scope = InstrumentationScope::new("my-library")
            .with_version("1.0.0")
            .with_schema_url("https://example.com/schema");

        assert_eq!(scope.name, "my-library");
        assert_eq!(scope.version, "1.0.0");
        assert_eq!(
            scope.schema_url,
            Some("https://example.com/schema".to_string())
        );
    }

    #[test]
    fn test_resource() {
        let mut resource = Resource::default_service("my-service", "1.0.0");
        assert_eq!(
            resource.get("service.name"),
            Some(&AttributeValue::String("my-service".to_string()))
        );

        resource.insert("custom.attr", "value");
        assert_eq!(
            resource.get("custom.attr"),
            Some(&AttributeValue::String("value".to_string()))
        );
    }

    #[test]
    fn test_resource_merge() {
        let mut r1 = Resource::new();
        r1.insert("key1", "value1");
        r1.insert("key2", "original");

        let mut r2 = Resource::new();
        r2.insert("key2", "updated");
        r2.insert("key3", "value3");

        r1.merge(&r2);

        // key1 should remain
        assert_eq!(
            r1.get("key1"),
            Some(&AttributeValue::String("value1".to_string()))
        );
        // key2 should remain original (merge doesn't overwrite)
        assert_eq!(
            r1.get("key2"),
            Some(&AttributeValue::String("original".to_string()))
        );
        // key3 should be added
        assert_eq!(
            r1.get("key3"),
            Some(&AttributeValue::String("value3".to_string()))
        );
    }
}
