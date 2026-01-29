//! Tracing and Profiling Module
//!
//! Provides distributed tracing with W3C trace context support,
//! multiple exporter backends, and CPU/memory profiling capabilities.
//!
//! # Features
//!
//! - **Distributed Tracing**: OpenTelemetry-compatible span management
//! - **Context Propagation**: W3C traceparent/tracestate support
//! - **Sampling**: Multiple sampling strategies (always, ratio, parent-based)
//! - **Exporters**: Jaeger, Zipkin, OTLP, and console exporters
//! - **CPU Profiling**: Stack sampling and flame graph generation
//! - **Memory Profiling**: Allocation tracking and hot site detection
//!
//! # Example
//!
//! ```rust,ignore
//! use hv2_core::tracing::{
//!     TracerProvider, InMemorySpanExporter, Context,
//!     CpuProfiler, FlameGraphBuilder,
//! };
//! use std::sync::Arc;
//!
//! // Create a tracer provider with an in-memory exporter
//! let exporter = Arc::new(InMemorySpanExporter::new());
//! let provider = TracerProvider::builder()
//!     .with_simple_exporter(exporter.clone())
//!     .build();
//!
//! // Get a tracer and create spans
//! let tracer = provider.tracer("my-service");
//! let mut span = tracer.start_span("operation");
//! span.set_attribute("key", "value");
//! span.end();
//!
//! // CPU profiling
//! let profiler = CpuProfiler::new();
//! profiler.start().unwrap();
//! // ... do work ...
//! let data = profiler.stop().unwrap();
//!
//! // Generate flame graph
//! let flame_graph = FlameGraphBuilder::from_profile(&data);
//! let svg = flame_graph.to_svg(800, 600);
//! ```

pub mod types;
pub mod tracer;
pub mod exporters;
pub mod profiler;

// Re-export core types
pub use types::{
    TraceId, SpanId, TraceFlags, SpanContext, TraceState,
    SpanKind, StatusCode, SpanStatus, AttributeValue, Attribute,
    SpanEvent, SpanLink, SpanData, SamplingDecision, SamplingResult,
    Sampler, AlwaysOnSampler, AlwaysOffSampler, TraceIdRatioSampler,
    ParentBasedSampler, InstrumentationScope, Resource,
};

// Re-export tracer types
pub use tracer::{
    SpanProcessor, SimpleSpanProcessor, BatchSpanProcessor,
    SpanExporter, InMemorySpanExporter, TracerError, TracerResult,
    SpanBuilder, Span, IdGenerator, DefaultIdGenerator,
    Tracer, TracerProvider, TracerProviderBuilder,
    Context, ContextGuard,
};

// Re-export exporters
pub use exporters::{
    ConsoleSpanExporter, JaegerConfig, JaegerSpanExporter,
    ZipkinConfig, ZipkinSpanExporter, OtlpConfig, OtlpProtocol,
    OtlpSpanExporter, CompositeSpanExporter, FilteredSpanExporter,
};

// Re-export profiler types
pub use profiler::{
    ProfileFrame, StackSample, ProfileData, ProfilerError, ProfilerResult,
    ProfilerConfig, ProfilerState, CpuProfiler, FlameGraphNode,
    FlameGraphBuilder, FunctionProfile, InstrumentedProfiler,
    ProfileGuard, AllocationProfile, AllocationSite,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_end_to_end_tracing() {
        // Create provider with in-memory exporter
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_resource(Resource::default_service("test-service", "1.0.0"))
            .with_sampler(AlwaysOnSampler)
            .with_simple_exporter(exporter.clone())
            .build();

        // Create tracer
        let tracer = provider.tracer_with_version("test-lib", "0.1.0");

        // Create parent span
        let parent = tracer.start_span("parent-operation");
        let parent_ctx = parent.context().clone();

        // Create child span
        let mut child = tracer.span_builder("child-operation")
            .with_kind(SpanKind::Internal)
            .with_parent(parent_ctx.clone())
            .with_attribute("key", "value")
            .start(&tracer);

        child.add_event_with_name("processing");
        child.set_ok();
        child.end();

        // End parent
        drop(parent);

        // Verify spans were exported
        let spans = exporter.get_spans();
        assert_eq!(spans.len(), 2);

        // Check child span has correct parent
        let child_span = spans.iter().find(|s| s.name == "child-operation").unwrap();
        assert_eq!(child_span.parent_span_id, Some(parent_ctx.span_id));
        assert_eq!(child_span.context.trace_id, parent_ctx.trace_id);
    }

    #[test]
    fn test_context_propagation() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter)
            .build();

        let tracer = provider.tracer("test");

        // Create a span and attach its context
        let span = tracer.start_span("outer");
        let _guard = Context::attach(span.context().clone());

        // Current context should be the span's context
        let current = Context::current();
        assert!(current.is_some());
        assert_eq!(current.unwrap().trace_id, span.context().trace_id);
    }

    #[test]
    fn test_w3c_trace_context() {
        // Parse a W3C traceparent header
        let traceparent = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let ctx = SpanContext::from_traceparent(traceparent).unwrap();

        assert!(ctx.is_valid());
        assert!(ctx.is_sampled());

        // Convert back to traceparent
        let output = ctx.to_traceparent();
        assert_eq!(output, traceparent);
    }

    #[test]
    fn test_sampling() {
        // Test always-on sampler
        let sampler = AlwaysOnSampler;
        let result = sampler.should_sample(
            None,
            TraceId::new(),
            "test",
            SpanKind::Internal,
            &[],
            &[],
        );
        assert_eq!(result.decision, SamplingDecision::RecordAndSample);

        // Test always-off sampler
        let sampler = AlwaysOffSampler;
        let result = sampler.should_sample(
            None,
            TraceId::new(),
            "test",
            SpanKind::Internal,
            &[],
            &[],
        );
        assert_eq!(result.decision, SamplingDecision::Drop);

        // Test ratio sampler (0% should always drop)
        let sampler = TraceIdRatioSampler::new(0.0);
        let result = sampler.should_sample(
            None,
            TraceId::new(),
            "test",
            SpanKind::Internal,
            &[],
            &[],
        );
        assert_eq!(result.decision, SamplingDecision::Drop);
    }

    #[test]
    fn test_multiple_exporters() {
        let exporter1 = Arc::new(InMemorySpanExporter::new());
        let exporter2 = Arc::new(InMemorySpanExporter::new());

        let composite = CompositeSpanExporter::new()
            .add_exporter(exporter1.clone())
            .add_exporter(exporter2.clone());

        let provider = TracerProvider::builder()
            .with_span_processor(Arc::new(SimpleSpanProcessor::new(Arc::new(composite))))
            .build();

        let tracer = provider.tracer("test");
        tracer.start_span("test-span").end();

        // Both exporters should have the span
        assert_eq!(exporter1.get_spans().len(), 1);
        assert_eq!(exporter2.get_spans().len(), 1);
    }

    #[test]
    fn test_profiler_integration() {
        let profiler = CpuProfiler::with_config(
            ProfilerConfig::default().with_frequency(99)
        );

        profiler.start().unwrap();

        // Add some samples
        for _ in 0..10 {
            let frames = vec![
                ProfileFrame::new("main", "app", 0x1000),
                ProfileFrame::new("process", "app", 0x2000),
            ];
            profiler.add_sample(StackSample::new(frames, 1)).unwrap();
        }

        let data = profiler.stop().unwrap();
        assert_eq!(data.total_samples(), 10);

        // Generate flame graph
        let builder = FlameGraphBuilder::from_profile(&data);
        let svg = builder.to_svg(400, 200);
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn test_instrumented_profiler() {
        let profiler = InstrumentedProfiler::new();

        fn work(profiler: &InstrumentedProfiler) {
            let _guard = profiler.enter("work_function");
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        for _ in 0..5 {
            work(&profiler);
        }

        let profiles = profiler.get_profiles();
        assert_eq!(profiles.len(), 1);

        let func = &profiles[0];
        assert_eq!(func.name, "work_function");
        assert_eq!(func.call_count, 5);
    }

    #[test]
    fn test_span_error_handling() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        let tracer = provider.tracer("test");
        let mut span = tracer.start_span("error-span");

        // Record an exception
        let error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        span.record_exception(&error);
        span.end();

        let spans = exporter.get_spans();
        assert_eq!(spans.len(), 1);
        assert!(spans[0].status.is_error());
        assert!(spans[0].events.iter().any(|e| e.name == "exception"));
    }

    #[test]
    fn test_filtered_exporter() {
        let inner = Arc::new(InMemorySpanExporter::new());
        let filtered = FilteredSpanExporter::by_duration(inner.clone(), 1_000_000); // 1ms

        let provider = TracerProvider::builder()
            .with_span_processor(Arc::new(SimpleSpanProcessor::new(Arc::new(filtered))))
            .build();

        let tracer = provider.tracer("test");

        // Quick span (should be filtered)
        tracer.start_span("quick").end();

        // Slow span (should pass through)
        let span = tracer.start_span("slow");
        std::thread::sleep(std::time::Duration::from_millis(2));
        span.end();

        let spans = inner.get_spans();
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].name, "slow");
    }

    #[test]
    fn test_span_links() {
        let exporter = Arc::new(InMemorySpanExporter::new());
        let provider = TracerProvider::builder()
            .with_simple_exporter(exporter.clone())
            .build();

        let tracer = provider.tracer("test");

        // Create first trace
        let span1 = tracer.start_span("first");
        let ctx1 = span1.context().clone();
        span1.end();

        // Create second trace linked to first
        let link = SpanLink::new(ctx1.clone());
        let span2 = tracer.span_builder("second")
            .with_link(link)
            .start(&tracer);
        span2.end();

        let spans = exporter.get_spans();
        let linked_span = spans.iter().find(|s| s.name == "second").unwrap();
        assert_eq!(linked_span.links.len(), 1);
        assert_eq!(linked_span.links[0].context.trace_id, ctx1.trace_id);
    }

    #[test]
    fn test_trace_state() {
        let mut state = TraceState::new();
        state.set("vendor1", "value1");
        state.set("vendor2", "value2");

        assert_eq!(state.get("vendor1"), Some(&"value1".to_string()));
        assert_eq!(state.get("vendor2"), Some(&"value2".to_string()));
        assert_eq!(state.get("vendor3"), None);

        // Parse from header
        let parsed = TraceState::from_header("vendor1=value1,vendor2=value2").unwrap();
        assert_eq!(parsed.get("vendor1"), Some(&"value1".to_string()));

        // Convert back to header
        let header = state.to_header();
        assert!(header.contains("vendor1=value1"));
        assert!(header.contains("vendor2=value2"));
    }

    #[test]
    fn test_resource() {
        let resource = Resource::default_service("my-service", "1.0.0");
        
        let service_name = resource.attributes.iter()
            .find(|a| a.key == "service.name")
            .map(|a| &a.value);
        
        assert!(matches!(service_name, Some(AttributeValue::String(s)) if s == "my-service"));
    }
}
