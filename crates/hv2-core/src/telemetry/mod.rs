//! Telemetry & Observability Module
//!
//! This module provides comprehensive metrics collection, export, and VM-specific
//! telemetry for the hypervisor.
//!
//! # Features
//!
//! - **Metric Types**: Counter, Gauge, Histogram, Summary, Info
//! - **Thread-safe Collectors**: Atomic counters, gauges, and histograms
//! - **Multiple Export Formats**: Prometheus, JSON, OpenTelemetry, StatsD, Carbon
//! - **VM-specific Metrics**: vCPU, memory, disk, and network statistics
//! - **Hypervisor Metrics**: VM lifecycle, resource overcommit tracking
//!
//! # Example
//!
//! ```rust
//! use hv2_core::telemetry::{
//!     MetricCollector, MetricRegistry, MetricDescriptor, MetricType,
//!     Counter, Gauge, Histogram, Timer,
//!     PrometheusExporter, JsonExporter, MetricExporter,
//!     VmMetrics, HypervisorMetrics,
//! };
//!
//! // Create a collector
//! let collector = MetricCollector::new();
//!
//! // Register metrics
//! collector.register(MetricDescriptor::new(
//!     "vm_exits_total",
//!     MetricType::Counter,
//!     "Total VM exits",
//! )).unwrap();
//!
//! // Create standalone metrics
//! let requests = Counter::new("requests_total", "Total requests");
//! requests.inc();
//!
//! let temperature = Gauge::new("temperature", "Current temperature");
//! temperature.set(23.5);
//!
//! let latency = Histogram::new("request_duration", "Request duration");
//! latency.observe(0.125);
//!
//! // Use timer for automatic duration tracking
//! let timer = Timer::new("operation_duration", "Operation duration");
//! {
//!     let _observation = timer.start();
//!     // ... do work ...
//! } // Duration automatically recorded on drop
//!
//! // Export to Prometheus format
//! let families = collector.collect();
//! let exporter = PrometheusExporter::new();
//! let output = exporter.export(&families).unwrap();
//!
//! // Or JSON format
//! let json_exporter = JsonExporter::new().pretty();
//! let json_output = json_exporter.export(&families).unwrap();
//! ```
//!
//! # VM Metrics Example
//!
//! ```rust
//! use hv2_core::telemetry::{VmMetrics, HypervisorMetrics};
//!
//! // Create VM metrics
//! let vm = VmMetrics::new("my-vm");
//!
//! // Add vCPUs
//! let vcpu0 = vm.add_vcpu(0);
//! vcpu0.record_io_exit();
//! vcpu0.record_run_time(1_000_000);
//!
//! // Add disk
//! let disk = vm.add_disk("vda");
//! disk.record_read(4096, 100); // 4KB read, 100us latency
//!
//! // Add network
//! let net = vm.add_network("eth0");
//! net.record_rx(1500); // 1500 byte packet received
//!
//! // Hypervisor-level metrics
//! let hv = HypervisorMetrics::new();
//! hv.record_vm_created(4); // VM with 4 vCPUs
//! ```

pub mod collector;
pub mod exporters;
#[cfg(feature = "remote-telemetry")]
pub mod remote;
pub mod types;
pub mod vm_metrics;

// Re-export main types
pub use types::{
    Ewma, HistogramData, MetricFamily, MetricLabel, MetricLabels, MetricSample, MetricType,
    MetricValue, MovingAverage, RateCalculator, SummaryData, Timestamp, DEFAULT_HISTOGRAM_BUCKETS,
    DEFAULT_QUANTILES, LATENCY_BUCKETS, SIZE_BUCKETS,
};

pub use collector::{
    CollectorError, CollectorResult, Counter, Gauge, Histogram, MetricCollector, MetricDescriptor,
    MetricRegistry, Timer, TimerObservation,
};

pub use exporters::{
    CarbonExporter, ExporterError, ExporterResult, JsonExporter, JsonMetric, MetricExporter,
    OpenTelemetryExporter, PrometheusExporter, StatsDExporter, StatsDFormat,
};

pub use vm_metrics::{
    DiskMetrics, HypervisorMetrics, MemoryMetrics, NetworkMetrics, VcpuMetrics, VmMetrics,
};

#[cfg(feature = "remote-telemetry")]
pub use remote::{RemoteExporterConfig, RemoteMetricExporter};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_end_to_end_workflow() {
        // Create registry and collector
        let registry = Arc::new(MetricRegistry::with_prefix("hypervisor"));
        let collector = MetricCollector::with_registry(registry.clone());

        // Register metrics
        collector
            .register(MetricDescriptor::new(
                "vm_exits_total",
                MetricType::Counter,
                "Total VM exits",
            ))
            .unwrap();

        collector
            .register(MetricDescriptor::new(
                "memory_used_bytes",
                MetricType::Gauge,
                "Memory used in bytes",
            ))
            .unwrap();

        // Update metrics
        registry
            .inc_counter("vm_exits_total", MetricLabels::from([("reason", "io")]))
            .unwrap();
        registry
            .inc_counter("vm_exits_total", MetricLabels::from([("reason", "io")]))
            .unwrap();
        registry
            .inc_counter("vm_exits_total", MetricLabels::from([("reason", "mmio")]))
            .unwrap();
        registry
            .set_gauge(
                "memory_used_bytes",
                MetricLabels::new(),
                1024.0 * 1024.0 * 512.0,
            )
            .unwrap();

        // Collect and export
        let families = collector.collect();
        assert!(families.len() >= 2);

        // Export to Prometheus
        let prom_exporter = PrometheusExporter::new();
        let prom_output = prom_exporter.export(&families).unwrap();
        assert!(prom_output.contains("hypervisor_vm_exits_total"));
        assert!(prom_output.contains("hypervisor_memory_used_bytes"));

        // Export to JSON
        let json_exporter = JsonExporter::new();
        let json_output = json_exporter.export(&families).unwrap();
        assert!(json_output.contains("hypervisor_vm_exits_total"));
    }

    #[test]
    fn test_vm_metrics_integration() {
        let vm = VmMetrics::new("test-vm");

        // Setup vCPUs
        let vcpu0 = vm.add_vcpu(0);
        let vcpu1 = vm.add_vcpu(1);

        // Simulate activity
        for _ in 0..100 {
            vcpu0.record_io_exit();
            vcpu1.record_mmio_exit();
        }

        vcpu0.record_run_time(50_000_000); // 50ms
        vcpu1.record_run_time(60_000_000); // 60ms

        // Verify
        assert_eq!(vcpu0.io_exits(), 100);
        assert_eq!(vcpu1.mmio_exits(), 100);
        assert_eq!(vm.vcpus().len(), 2);
    }

    #[test]
    fn test_disk_metrics_integration() {
        let vm = VmMetrics::new("test-vm");
        let disk = vm.add_disk("vda");

        // Simulate IO
        for i in 0..50 {
            disk.record_read(4096, 100 + i);
            disk.record_write(8192, 200 + i);
        }

        assert_eq!(disk.read_ops(), 50);
        assert_eq!(disk.write_ops(), 50);
        assert_eq!(disk.bytes_read(), 50 * 4096);
        assert_eq!(disk.bytes_written(), 50 * 8192);
        assert_eq!(vm.total_disk_iops(), 100);
    }

    #[test]
    fn test_network_metrics_integration() {
        let vm = VmMetrics::new("test-vm");
        let eth0 = vm.add_network("eth0");
        let eth1 = vm.add_network("eth1");

        // Simulate traffic
        for _ in 0..100 {
            eth0.record_rx(1500);
            eth0.record_tx(500);
            eth1.record_rx(1000);
        }

        assert_eq!(eth0.rx_packets(), 100);
        assert_eq!(eth0.tx_packets(), 100);
        assert_eq!(eth1.rx_packets(), 100);
        assert_eq!(vm.total_network_throughput(), 100 * (1500 + 500 + 1000));
    }

    #[test]
    fn test_hypervisor_metrics_integration() {
        let hv = HypervisorMetrics::new();

        // Create VMs
        hv.record_vm_created(4); // 4 vCPUs
        hv.record_vm_created(2); // 2 vCPUs
        hv.record_vm_created(8); // 8 vCPUs

        assert_eq!(hv.active_vms(), 3);
        assert_eq!(hv.total_vcpus(), 14);

        // Destroy one
        hv.record_vm_destroyed(4);

        assert_eq!(hv.active_vms(), 2);
        assert_eq!(hv.total_vcpus(), 10);

        // Track overcommit
        hv.set_memory_overcommit(1.2);
        hv.set_cpu_overcommit(2.5);

        assert!((hv.memory_overcommit() - 1.2).abs() < 0.01);
        assert!((hv.cpu_overcommit() - 2.5).abs() < 0.01);
    }

    #[test]
    fn test_standalone_metrics() {
        let counter = Counter::new("requests", "Total requests");
        counter.add(100);
        assert_eq!(counter.get(), 100);

        let gauge = Gauge::new("temperature", "Temperature");
        gauge.set(23.5);
        assert!((gauge.get() - 23.5).abs() < 0.01);

        let histogram = Histogram::new("latency", "Latency");
        histogram.observe(0.1);
        histogram.observe(0.2);
        histogram.observe(0.3);
        assert_eq!(histogram.count(), 3);
        assert!((histogram.sum() - 0.6).abs() < 0.01);
    }

    #[test]
    fn test_timer_metrics() {
        let timer = Timer::new("operation", "Operation duration");

        let obs = timer.start();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let duration = obs.stop();

        assert!(duration.as_millis() >= 10);
        assert_eq!(timer.histogram().count(), 1);
    }

    #[test]
    fn test_multi_format_export() {
        let registry = MetricRegistry::new();

        registry
            .set_counter("requests", MetricLabels::from([("method", "GET")]), 100)
            .unwrap();
        registry
            .set_gauge("temperature", MetricLabels::new(), 23.5)
            .unwrap();

        let families = registry.collect();

        // All exporters should work
        let prom = PrometheusExporter::new().export(&families);
        assert!(prom.is_ok());

        let json = JsonExporter::new().export(&families);
        assert!(json.is_ok());

        let otel = OpenTelemetryExporter::new("test").export(&families);
        assert!(otel.is_ok());

        let statsd = StatsDExporter::new().export(&families);
        assert!(statsd.is_ok());

        let carbon = CarbonExporter::new().export(&families);
        assert!(carbon.is_ok());
    }

    #[test]
    fn test_rate_calculator() {
        let mut calc = RateCalculator::new();

        calc.update(0);
        std::thread::sleep(std::time::Duration::from_millis(50));
        calc.update(100);

        let rate = calc.rate();
        // Rate should be approximately 100 / 0.05 = 2000 per second.
        // Use a generous lower bound to avoid flakiness on slow/loaded systems.
        assert!(rate > 200.0, "expected rate > 200, got {rate}");
    }

    #[test]
    fn test_moving_average() {
        let mut avg = MovingAverage::new(5);

        avg.add_sample(10.0);
        avg.add_sample(20.0);
        avg.add_sample(30.0);

        let result = avg.average();
        assert!((result - 20.0).abs() < 0.01);
    }

    #[test]
    fn test_ewma() {
        let mut ewma = Ewma::new(0.5);

        ewma.add_sample(10.0);
        assert!(ewma.value() != 0.0); // Value is initialized

        ewma.add_sample(20.0);
        let val = ewma.value();
        // With alpha=0.5: 0.5 * 20 + 0.5 * 10 = 15
        assert!((val - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_histogram_quantiles() {
        // Create histogram with buckets appropriate for values 1-100
        let mut hist = HistogramData::with_buckets(&[10.0, 25.0, 50.0, 75.0, 90.0, 100.0]);

        // Add values 1-100
        for i in 1..=100 {
            hist.observe(i as f64);
        }

        let p50 = hist.estimate_quantile(0.5);
        let p90 = hist.estimate_quantile(0.9);
        let p99 = hist.estimate_quantile(0.99);

        // Approximate checks (histogram quantiles are estimates)
        assert!(p50.is_some());
        assert!(p90.is_some());
        assert!(p99.is_some());
        // p50 should be around 50, p90 around 90, p99 around 99
        assert!(p50.unwrap() < p90.unwrap());
        assert!(p90.unwrap() < p99.unwrap());
    }

    #[test]
    fn test_metric_labels() {
        let labels = MetricLabels::from([("method", "GET"), ("status", "200"), ("path", "/api")]);

        assert_eq!(labels.len(), 3);
        assert_eq!(labels.get("method"), Some("GET"));
        assert_eq!(labels.get("status"), Some("200"));

        let canonical = labels.canonical();
        // Canonical form should be sorted
        let keys: Vec<_> = canonical.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(keys, vec!["method", "path", "status"]);
    }

    #[test]
    fn test_memory_metrics_utilization() {
        let mem = MemoryMetrics::new();

        mem.set_total(8 * 1024 * 1024 * 1024); // 8 GB
        mem.set_used(6 * 1024 * 1024 * 1024); // 6 GB

        let util = mem.utilization();
        assert!((util - 75.0).abs() < 0.1);
    }
}
