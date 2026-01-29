//! Tracer Implementation
//!
//! Provides the main tracer interface for creating and managing spans,
//! including context propagation and span lifecycle management.

use super::types::{
    AlwaysOnSampler, Attribute, AttributeValue, InstrumentationScope, Resource, Sampler,
    SamplingDecision, SpanContext, SpanData, SpanEvent, SpanId, SpanKind, SpanLink, SpanStatus,
    StatusCode, TraceFlags, TraceId,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Span processor trait for handling completed spans
pub trait SpanProcessor: Send + Sync {
    /// Called when a span starts
    fn on_start(&self, span: &SpanData, parent_context: Option<&SpanContext>);

    /// Called when a span ends
    fn on_end(&self, span: SpanData);

    /// Force flush any buffered spans
    fn force_flush(&self) -> Result<(), TracerError>;

    /// Shutdown the processor
    fn shutdown(&self) -> Result<(), TracerError>;
}

/// Simple span processor that immediately exports spans
#[derive(Debug)]
pub struct SimpleSpanProcessor {
    exporter: Arc<dyn SpanExporter>,
}

impl SimpleSpanProcessor {
    /// Create a new simple processor with an exporter
    pub fn new(exporter: Arc<dyn SpanExporter>) -> Self {
        Self { exporter }
    }
}

impl SpanProcessor for SimpleSpanProcessor {
    fn on_start(&self, _span: &SpanData, _parent: Option<&SpanContext>) {
        // Simple processor doesn't do anything on start
    }

    fn on_end(&self, span: SpanData) {
        let _ = self.exporter.export(vec![span]);
    }

    fn force_flush(&self) -> Result<(), TracerError> {
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        self.exporter.shutdown()
    }
}

/// Batch span processor that buffers and exports spans in batches
pub struct BatchSpanProcessor {
    exporter: Arc<dyn SpanExporter>,
    buffer: Mutex<Vec<SpanData>>,
    max_batch_size: usize,
    max_queue_size: usize,
}

impl BatchSpanProcessor {
    /// Create a new batch processor
    pub fn new(exporter: Arc<dyn SpanExporter>) -> Self {
        Self {
            exporter,
            buffer: Mutex::new(Vec::new()),
            max_batch_size: 512,
            max_queue_size: 2048,
        }
    }

    /// Set max batch size
    pub fn with_max_batch_size(mut self, size: usize) -> Self {
        self.max_batch_size = size;
        self
    }

    /// Set max queue size
    pub fn with_max_queue_size(mut self, size: usize) -> Self {
        self.max_queue_size = size;
        self
    }

    fn flush_batch(&self) -> Result<(), TracerError> {
        let batch: Vec<SpanData> = {
            let mut buffer = self.buffer.lock().map_err(|_| TracerError::LockError)?;
            if buffer.is_empty() {
                return Ok(());
            }
            let drain_count = buffer.len().min(self.max_batch_size);
            buffer.drain(..drain_count).collect()
        };

        if !batch.is_empty() {
            self.exporter.export(batch)?;
        }
        Ok(())
    }
}

impl SpanProcessor for BatchSpanProcessor {
    fn on_start(&self, _span: &SpanData, _parent: Option<&SpanContext>) {
        // Batch processor doesn't do anything on start
    }

    fn on_end(&self, span: SpanData) {
        if let Ok(mut buffer) = self.buffer.lock() {
            if buffer.len() < self.max_queue_size {
                buffer.push(span);
            }
            // Auto-flush if batch size reached
            if buffer.len() >= self.max_batch_size {
                drop(buffer);
                let _ = self.flush_batch();
            }
        }
    }

    fn force_flush(&self) -> Result<(), TracerError> {
        while {
            let buffer = self.buffer.lock().map_err(|_| TracerError::LockError)?;
            !buffer.is_empty()
        } {
            self.flush_batch()?;
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        self.force_flush()?;
        self.exporter.shutdown()
    }
}

/// Span exporter trait
pub trait SpanExporter: Send + Sync {
    /// Export a batch of spans
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError>;

    /// Shutdown the exporter
    fn shutdown(&self) -> Result<(), TracerError>;
}

/// In-memory span exporter for testing
#[derive(Debug, Default)]
pub struct InMemorySpanExporter {
    spans: Mutex<Vec<SpanData>>,
}

impl InMemorySpanExporter {
    /// Create a new in-memory exporter
    pub fn new() -> Self {
        Self::default()
    }

    /// Get exported spans
    pub fn get_spans(&self) -> Vec<SpanData> {
        self.spans.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Clear exported spans
    pub fn clear(&self) {
        if let Ok(mut spans) = self.spans.lock() {
            spans.clear();
        }
    }
}

impl SpanExporter for InMemorySpanExporter {
    fn export(&self, spans: Vec<SpanData>) -> Result<(), TracerError> {
        if let Ok(mut buffer) = self.spans.lock() {
            buffer.extend(spans);
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), TracerError> {
        Ok(())
    }
}

/// Tracer error types
#[derive(Debug, Clone)]
pub enum TracerError {
    /// Lock acquisition failed
    LockError,
    /// Export failed
    ExportError(String),
    /// Invalid operation
    InvalidOperation(String),
    /// Shutdown error
    ShutdownError(String),
}

impl std::fmt::Display for TracerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TracerError::LockError => write!(f, "failed to acquire lock"),
            TracerError::ExportError(msg) => write!(f, "export error: {}", msg),
            TracerError::InvalidOperation(msg) => write!(f, "invalid operation: {}", msg),
            TracerError::ShutdownError(msg) => write!(f, "shutdown error: {}", msg),
        }
    }
}

impl std::error::Error for TracerError {}

/// Tracer result type
pub type TracerResult<T> = Result<T, TracerError>;

/// Active span builder
pub struct SpanBuilder {
    name: String,
    kind: SpanKind,
    parent: Option<SpanContext>,
    attributes: Vec<Attribute>,
    links: Vec<SpanLink>,
    start_time: Option<u64>,
}

impl SpanBuilder {
    /// Create a new span builder
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            kind: SpanKind::Internal,
            parent: None,
            attributes: Vec::new(),
            links: Vec::new(),
            start_time: None,
        }
    }

    /// Set span kind
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Set parent context
    pub fn with_parent(mut self, parent: SpanContext) -> Self {
        self.parent = Some(parent);
        self
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
    pub fn with_attributes(mut self, attrs: Vec<Attribute>) -> Self {
        self.attributes.extend(attrs);
        self
    }

    /// Add a link
    pub fn with_link(mut self, link: SpanLink) -> Self {
        self.links.push(link);
        self
    }

    /// Set explicit start time
    pub fn with_start_time(mut self, timestamp: u64) -> Self {
        self.start_time = Some(timestamp);
        self
    }

    /// Start the span
    pub fn start(self, tracer: &Tracer) -> Span {
        tracer.start_span_from_builder(self)
    }
}

/// Active span
pub struct Span {
    /// Span data being built
    data: SpanData,
    /// Start instant for duration calculation
    start_instant: Instant,
    /// Tracer reference for ending
    tracer: Arc<TracerInner>,
    /// Whether span has ended
    ended: bool,
}

impl Span {
    /// Get span context
    pub fn context(&self) -> &SpanContext {
        &self.data.context
    }

    /// Get span name
    pub fn name(&self) -> &str {
        &self.data.name
    }

    /// Check if recording
    pub fn is_recording(&self) -> bool {
        !self.ended && self.data.context.is_sampled()
    }

    /// Set span name
    pub fn set_name(&mut self, name: impl Into<String>) {
        if self.is_recording() {
            self.data.name = name.into();
        }
    }

    /// Add an attribute
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<AttributeValue>) {
        if self.is_recording() {
            let key = key.into();
            // Update existing or add new
            if let Some(attr) = self.data.attributes.iter_mut().find(|a| a.key == key) {
                attr.value = value.into();
            } else {
                self.data.attributes.push(Attribute::new(key, value));
            }
        }
    }

    /// Add multiple attributes
    pub fn set_attributes(&mut self, attrs: impl IntoIterator<Item = Attribute>) {
        if self.is_recording() {
            for attr in attrs {
                self.set_attribute(attr.key, attr.value);
            }
        }
    }

    /// Add an event
    pub fn add_event(&mut self, event: SpanEvent) {
        if self.is_recording() {
            self.data.events.push(event);
        }
    }

    /// Add a named event
    pub fn add_event_with_name(&mut self, name: impl Into<String>) {
        self.add_event(SpanEvent::new(name));
    }

    /// Set status
    pub fn set_status(&mut self, status: SpanStatus) {
        if self.is_recording() {
            // Only upgrade status: Unset -> Ok -> Error
            match (&self.data.status.code, &status.code) {
                (StatusCode::Unset, _) => self.data.status = status,
                (StatusCode::Ok, StatusCode::Error) => self.data.status = status,
                _ => {}
            }
        }
    }

    /// Set OK status
    pub fn set_ok(&mut self) {
        self.set_status(SpanStatus::ok());
    }

    /// Set error status
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.set_status(SpanStatus::error(message));
    }

    /// Record an exception
    pub fn record_exception(&mut self, error: &dyn std::error::Error) {
        if self.is_recording() {
            let event = SpanEvent::new("exception")
                .with_attribute("exception.type", std::any::type_name_of_val(error))
                .with_attribute("exception.message", error.to_string());
            self.add_event(event);
            self.set_error(error.to_string());
        }
    }

    /// End the span
    pub fn end(mut self) {
        self.end_with_timestamp(current_timestamp_nanos());
    }

    /// End with explicit timestamp
    pub fn end_with_timestamp(mut self, timestamp: u64) {
        if !self.ended {
            self.ended = true;
            self.data.end_time = timestamp;
            self.tracer.end_span(self.data);
        }
    }

    /// Get elapsed duration
    pub fn elapsed(&self) -> Duration {
        self.start_instant.elapsed()
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if !self.ended {
            self.ended = true;
            self.data.end_time = current_timestamp_nanos();
            // Clone data since we can't move out of Drop
            let data = SpanData {
                context: self.data.context.clone(),
                parent_span_id: self.data.parent_span_id,
                name: self.data.name.clone(),
                kind: self.data.kind,
                start_time: self.data.start_time,
                end_time: self.data.end_time,
                attributes: self.data.attributes.clone(),
                events: self.data.events.clone(),
                links: self.data.links.clone(),
                status: self.data.status.clone(),
                resource: self.data.resource.clone(),
                instrumentation_name: self.data.instrumentation_name.clone(),
                instrumentation_version: self.data.instrumentation_version.clone(),
            };
            self.tracer.end_span(data);
        }
    }
}

/// Inner tracer state
struct TracerInner {
    /// Instrumentation scope
    scope: InstrumentationScope,
    /// Resource
    resource: Resource,
    /// Sampler
    sampler: Box<dyn Sampler>,
    /// Span processors
    processors: RwLock<Vec<Arc<dyn SpanProcessor>>>,
    /// ID generator
    id_generator: Box<dyn IdGenerator>,
}

impl TracerInner {
    fn end_span(&self, span: SpanData) {
        if let Ok(processors) = self.processors.read() {
            for processor in processors.iter() {
                processor.on_end(span.clone());
            }
        }
    }
}

/// ID generator trait
pub trait IdGenerator: Send + Sync {
    /// Generate a new trace ID
    fn new_trace_id(&self) -> TraceId;
    /// Generate a new span ID
    fn new_span_id(&self) -> SpanId;
}

/// Default ID generator
#[derive(Debug, Default)]
pub struct DefaultIdGenerator;

impl IdGenerator for DefaultIdGenerator {
    fn new_trace_id(&self) -> TraceId {
        TraceId::new()
    }

    fn new_span_id(&self) -> SpanId {
        SpanId::new()
    }
}

/// Tracer for creating spans
#[derive(Clone)]
pub struct Tracer {
    inner: Arc<TracerInner>,
}

impl Tracer {
    /// Create a span builder
    pub fn span_builder(&self, name: impl Into<String>) -> SpanBuilder {
        SpanBuilder::new(name)
    }

    /// Start a span with the given name
    pub fn start_span(&self, name: impl Into<String>) -> Span {
        SpanBuilder::new(name).start(self)
    }

    /// Start a span with a parent context
    pub fn start_span_with_parent(&self, name: impl Into<String>, parent: &SpanContext) -> Span {
        SpanBuilder::new(name)
            .with_parent(parent.clone())
            .start(self)
    }

    /// Start a span from a builder
    fn start_span_from_builder(&self, builder: SpanBuilder) -> Span {
        let start_time = builder.start_time.unwrap_or_else(current_timestamp_nanos);

        // Determine parent context
        let (trace_id, parent_span_id) = match &builder.parent {
            Some(parent) if parent.is_valid() => (parent.trace_id, Some(parent.span_id)),
            _ => (self.inner.id_generator.new_trace_id(), None),
        };

        let span_id = self.inner.id_generator.new_span_id();

        // Make sampling decision
        let sampling_result = self.inner.sampler.should_sample(
            builder.parent.as_ref(),
            trace_id,
            &builder.name,
            builder.kind,
            &builder.attributes,
            &builder.links,
        );

        let mut trace_flags = TraceFlags::NONE;
        if sampling_result.decision == SamplingDecision::RecordAndSample {
            trace_flags.set_sampled(true);
        }

        let context = SpanContext {
            trace_id,
            span_id,
            trace_flags,
            trace_state: sampling_result.trace_state,
            is_remote: false,
        };

        let mut attributes = builder.attributes;
        attributes.extend(sampling_result.attributes);

        let data = SpanData {
            context,
            parent_span_id,
            name: builder.name,
            kind: builder.kind,
            start_time,
            end_time: 0,
            attributes,
            events: Vec::new(),
            links: builder.links,
            status: SpanStatus::unset(),
            resource: self.inner.resource.attributes.clone(),
            instrumentation_name: self.inner.scope.name.clone(),
            instrumentation_version: self.inner.scope.version.clone(),
        };

        // Notify processors
        if let Ok(processors) = self.inner.processors.read() {
            for processor in processors.iter() {
                processor.on_start(&data, builder.parent.as_ref());
            }
        }

        Span {
            data,
            start_instant: Instant::now(),
            tracer: self.inner.clone(),
            ended: false,
        }
    }

    /// Force flush all span processors
    pub fn force_flush(&self) -> TracerResult<()> {
        if let Ok(processors) = self.inner.processors.read() {
            for processor in processors.iter() {
                processor.force_flush()?;
            }
        }
        Ok(())
    }
}

/// Tracer provider for creating tracers
pub struct TracerProvider {
    /// Resource
    resource: Resource,
    /// Sampler
    sampler: Box<dyn Sampler>,
    /// Span processors
    processors: Vec<Arc<dyn SpanProcessor>>,
    /// ID generator
    id_generator: Box<dyn IdGenerator>,
}

impl TracerProvider {
    /// Create a new tracer provider builder
    pub fn builder() -> TracerProviderBuilder {
        TracerProviderBuilder::default()
    }

    /// Get a tracer
    pub fn tracer(&self, name: impl Into<String>) -> Tracer {
        self.tracer_with_scope(InstrumentationScope::new(name))
    }

    /// Get a tracer with version
    pub fn tracer_with_version(
        &self,
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Tracer {
        self.tracer_with_scope(InstrumentationScope::new(name).with_version(version))
    }

    /// Get a tracer with full scope
    pub fn tracer_with_scope(&self, scope: InstrumentationScope) -> Tracer {
        Tracer {
            inner: Arc::new(TracerInner {
                scope,
                resource: self.resource.clone(),
                sampler: Box::new(AlwaysOnSampler),
                processors: RwLock::new(self.processors.clone()),
                id_generator: Box::new(DefaultIdGenerator),
            }),
        }
    }

    /// Shutdown the provider
    pub fn shutdown(&self) -> TracerResult<()> {
        for processor in &self.processors {
            processor.shutdown()?;
        }
        Ok(())
    }
}

impl Default for TracerProvider {
    fn default() -> Self {
        Self {
            resource: Resource::new(),
            sampler: Box::new(AlwaysOnSampler),
            processors: Vec::new(),
            id_generator: Box::new(DefaultIdGenerator),
        }
    }
}

/// Tracer provider builder
#[derive(Default)]
pub struct TracerProviderBuilder {
    resource: Option<Resource>,
    sampler: Option<Box<dyn Sampler>>,
    processors: Vec<Arc<dyn SpanProcessor>>,
    id_generator: Option<Box<dyn IdGenerator>>,
}

impl TracerProviderBuilder {
    /// Set resource
    pub fn with_resource(mut self, resource: Resource) -> Self {
        self.resource = Some(resource);
        self
    }

    /// Set sampler
    pub fn with_sampler(mut self, sampler: impl Sampler + 'static) -> Self {
        self.sampler = Some(Box::new(sampler));
        self
    }

    /// Add a span processor
    pub fn with_span_processor(mut self, processor: Arc<dyn SpanProcessor>) -> Self {
        self.processors.push(processor);
        self
    }

    /// Add a simple exporter (wraps in SimpleSpanProcessor)
    pub fn with_simple_exporter(self, exporter: Arc<dyn SpanExporter>) -> Self {
        self.with_span_processor(Arc::new(SimpleSpanProcessor::new(exporter)))
    }

    /// Add a batch exporter (wraps in BatchSpanProcessor)
    pub fn with_batch_exporter(self, exporter: Arc<dyn SpanExporter>) -> Self {
        self.with_span_processor(Arc::new(BatchSpanProcessor::new(exporter)))
    }

    /// Set ID generator
    pub fn with_id_generator(mut self, generator: impl IdGenerator + 'static) -> Self {
        self.id_generator = Some(Box::new(generator));
        self
    }

    /// Build the tracer provider
    pub fn build(self) -> TracerProvider {
        TracerProvider {
            resource: self.resource.unwrap_or_default(),
            sampler: self.sampler.unwrap_or_else(|| Box::new(AlwaysOnSampler)),
            processors: self.processors,
            id_generator: self
                .id_generator
                .unwrap_or_else(|| Box::new(DefaultIdGenerator)),
        }
    }
}

/// Get current timestamp in nanoseconds
fn current_timestamp_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

// Thread-local context storage
thread_local! {
    static CURRENT_CONTEXT: RefCell<Vec<SpanContext>> = RefCell::new(Vec::new());
}

/// Context management
pub struct Context;

impl Context {
    /// Get the current span context
    pub fn current() -> Option<SpanContext> {
        CURRENT_CONTEXT.with(|ctx| ctx.borrow().last().cloned())
    }

    /// Set the current span context, returning a guard
    pub fn attach(context: SpanContext) -> ContextGuard {
        CURRENT_CONTEXT.with(|ctx| ctx.borrow_mut().push(context));
        ContextGuard { _private: () }
    }

    /// Execute a closure with a span context
    pub fn with<F, R>(context: SpanContext, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _guard = Self::attach(context);
        f()
    }
}

/// Guard for attached context
pub struct ContextGuard {
    _private: (),
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|ctx| ctx.borrow_mut().pop());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_provider() -> TracerProvider {
        let exporter = Arc::new(InMemorySpanExporter::new());
        TracerProvider::builder()
            .with_resource(Resource::default_service("test-service", "1.0.0"))
            .with_simple_exporter(exporter)
            .build()
    }

    #[test]
    fn test_span_builder() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let span = tracer
            .span_builder("my-span")
            .with_kind(SpanKind::Server)
            .with_attribute("key", "value")
            .start(&tracer);

        assert_eq!(span.name(), "my-span");
        assert!(span.is_recording());
        assert!(span.context().is_valid());
    }

    #[test]
    fn test_span_lifecycle() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let mut span = tracer.start_span("test-span");
        span.set_attribute("http.method", "GET");
        span.add_event_with_name("processing");
        span.set_ok();
        span.end();

        assert!(!span.is_recording());
    }

    #[test]
    fn test_span_with_parent() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let parent = tracer.start_span("parent");
        let parent_ctx = parent.context().clone();

        let child = tracer.start_span_with_parent("child", &parent_ctx);

        assert_eq!(child.context().trace_id, parent_ctx.trace_id);
        assert_ne!(child.context().span_id, parent_ctx.span_id);
    }

    #[test]
    fn test_span_error_recording() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let mut span = tracer.start_span("error-span");
        span.set_error("something went wrong");

        // Status should be error
        assert!(span.data.status.is_error());
        assert_eq!(span.data.status.message, "something went wrong");
    }

    #[test]
    fn test_span_events() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let mut span = tracer.start_span("event-span");
        span.add_event(SpanEvent::new("event1").with_attribute("key", "value"));
        span.add_event_with_name("event2");

        assert_eq!(span.data.events.len(), 2);
        assert_eq!(span.data.events[0].name, "event1");
        assert_eq!(span.data.events[1].name, "event2");
    }

    #[test]
    fn test_in_memory_exporter() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        let tracer = provider.tracer("test");
        let span = tracer.start_span("exported-span");
        span.end();

        let spans = exporter.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "exported-span");
    }

    #[test]
    fn test_batch_processor() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let processor = Arc::new(BatchSpanProcessor::new(exporter.clone()).with_max_batch_size(5));

        let provider = TracerProvider::builder()
            .with_span_processor(processor)
            .build();

        let tracer = provider.tracer("test");

        // Create spans
        for i in 0..3 {
            let span = tracer.start_span(format!("span-{}", i));
            span.end();
        }

        // Force flush
        provider.shutdown().unwrap();

        let spans = exporter.get_spans();
        assert_eq!(spans.len(), 3);
    }

    #[test]
    fn test_context_propagation() {
        let ctx1 = SpanContext::root();
        let ctx1_trace = ctx1.trace_id;

        {
            let _guard = Context::attach(ctx1);
            let current = Context::current();
            assert!(current.is_some());
            assert_eq!(current.unwrap().trace_id, ctx1_trace);

            // Nested context
            let ctx2 = SpanContext::root();
            let ctx2_trace = ctx2.trace_id;
            {
                let _guard2 = Context::attach(ctx2);
                let current = Context::current();
                assert_eq!(current.unwrap().trace_id, ctx2_trace);
            }

            // After inner drops, back to outer
            let current = Context::current();
            assert_eq!(current.unwrap().trace_id, ctx1_trace);
        }

        // After all guards drop, no context
        assert!(Context::current().is_none());
    }

    #[test]
    fn test_context_with() {
        let ctx = SpanContext::root();
        let trace_id = ctx.trace_id;

        let result = Context::with(ctx, || {
            let current = Context::current();
            assert_eq!(current.unwrap().trace_id, trace_id);
            42
        });

        assert_eq!(result, 42);
        assert!(Context::current().is_none());
    }

    #[test]
    fn test_tracer_provider_builder() {
        let resource = Resource::default_service("my-service", "1.0.0");
        let exporter = Arc::new(InMemorySpanExporter::new());

        let provider = TracerProvider::builder()
            .with_resource(resource)
            .with_sampler(AlwaysOnSampler)
            .with_simple_exporter(exporter)
            .build();

        let tracer = provider.tracer_with_version("my-lib", "0.1.0");
        let span = tracer.start_span("test");

        assert!(span.is_recording());
    }

    #[test]
    fn test_span_duration() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let span = tracer.start_span("duration-test");
        std::thread::sleep(std::time::Duration::from_millis(10));
        let elapsed = span.elapsed();

        assert!(elapsed.as_millis() >= 10);
    }

    #[test]
    fn test_span_auto_end_on_drop() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        let tracer = provider.tracer("test");

        {
            let _span = tracer.start_span("auto-end-span");
            // span is dropped here
        }

        let spans = exporter.get_spans();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].end_time > spans[0].start_time);
    }

    #[test]
    fn test_span_attributes() {
        let provider = create_test_provider();
        let tracer = provider.tracer("test");

        let mut span = tracer
            .span_builder("attr-span")
            .with_attribute("initial", "value")
            .start(&tracer);

        span.set_attribute("added", "later");
        span.set_attribute("initial", "updated"); // Update existing

        assert_eq!(span.data.attributes.len(), 2);
    }

    #[test]
    fn test_default_id_generator() {
        let gen = DefaultIdGenerator;

        let trace1 = gen.new_trace_id();
        let trace2 = gen.new_trace_id();
        assert_ne!(trace1, trace2);

        let span1 = gen.new_span_id();
        let span2 = gen.new_span_id();
        assert_ne!(span1, span2);
    }
}
