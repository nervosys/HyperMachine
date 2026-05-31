//! GPU Observability & Metrics
//!
//! Provides unified GPU utilization metrics, fleet-level health aggregation,
//! and Prometheus-compatible exposition for monitoring dashboards.
//!
//! # Metric Categories
//!
//! | Category       | Metrics                                              |
//! |----------------|------------------------------------------------------|
//! | Utilization    | GPU compute %, memory %, encoder/decoder %            |
//! | Memory         | Used/total VRAM, allocation rate, OOM events          |
//! | Thermal        | Temperature, throttle events, fan speed %             |
//! | Power          | Current draw (W), TDP %, power cap events             |
//! | Fleet health   | Healthy/degraded/offline GPU count per host           |
//! | Fabric         | Topology link utilization, placement latency          |
//!
//! # Integration
//!
//! GPU metrics are collected per-device and aggregated into
//! [`GpuFleetSnapshot`] for fleet-wide dashboards. Individual device
//! metrics can be scraped via the `/api/v1/gpu-fabric/metrics` endpoint.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::SystemTime;

use parking_lot::RwLock;

// ============================================================================
// Per-Device GPU Metrics
// ============================================================================

/// Health state of an individual GPU device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuHealthState {
    /// GPU is operating normally
    Healthy,
    /// GPU is degraded (thermal throttling, ECC errors, etc.)
    Degraded,
    /// GPU is offline or unreachable
    Offline,
    /// Health state unknown (no data yet)
    Unknown,
}

/// Live metrics for a single GPU device.
#[derive(Debug)]
pub struct GpuDeviceMetrics {
    // ── Utilization ──
    /// GPU compute utilization (0-100, stored as value * 100 for atomics)
    compute_utilization: AtomicU64,
    /// GPU memory utilization (0-100)
    memory_utilization: AtomicU64,
    /// Encoder utilization (0-100)
    encoder_utilization: AtomicU64,
    /// Decoder utilization (0-100)
    decoder_utilization: AtomicU64,

    // ── Memory ──
    /// Used VRAM in MiB
    vram_used_mib: AtomicU64,
    /// Total VRAM in MiB
    vram_total_mib: AtomicU64,
    /// Cumulative memory allocation count
    alloc_count: AtomicU64,
    /// Out-of-memory events
    oom_events: AtomicU64,

    // ── Thermal ──
    /// Temperature in degrees Celsius (stored as value * 10 for 0.1°C precision)
    temperature_decicelsius: AtomicU64,
    /// Thermal throttle event count
    throttle_events: AtomicU64,
    /// Fan speed percent (0-100)
    fan_speed_pct: AtomicU64,

    // ── Power ──
    /// Current power draw in milliwatts
    power_draw_mw: AtomicU64,
    /// TDP (thermal design power) in milliwatts
    tdp_mw: AtomicU64,
    /// Power cap applied events
    power_cap_events: AtomicU64,
}

impl GpuDeviceMetrics {
    /// Create a new set of GPU device metrics.
    pub fn new(vram_total_mib: u64, tdp_mw: u64) -> Self {
        Self {
            compute_utilization: AtomicU64::new(0),
            memory_utilization: AtomicU64::new(0),
            encoder_utilization: AtomicU64::new(0),
            decoder_utilization: AtomicU64::new(0),
            vram_used_mib: AtomicU64::new(0),
            vram_total_mib: AtomicU64::new(vram_total_mib),
            alloc_count: AtomicU64::new(0),
            oom_events: AtomicU64::new(0),
            temperature_decicelsius: AtomicU64::new(0),
            throttle_events: AtomicU64::new(0),
            fan_speed_pct: AtomicU64::new(0),
            power_draw_mw: AtomicU64::new(0),
            tdp_mw: AtomicU64::new(tdp_mw),
            power_cap_events: AtomicU64::new(0),
        }
    }

    /// Update compute utilization (0-100).
    pub fn set_compute_utilization(&self, pct: u64) {
        self.compute_utilization
            .store(pct.min(100), Ordering::Relaxed);
    }

    /// Update memory utilization (0-100).
    pub fn set_memory_utilization(&self, pct: u64) {
        self.memory_utilization
            .store(pct.min(100), Ordering::Relaxed);
    }

    /// Update encoder utilization (0-100).
    pub fn set_encoder_utilization(&self, pct: u64) {
        self.encoder_utilization
            .store(pct.min(100), Ordering::Relaxed);
    }

    /// Update decoder utilization (0-100).
    pub fn set_decoder_utilization(&self, pct: u64) {
        self.decoder_utilization
            .store(pct.min(100), Ordering::Relaxed);
    }

    /// Update VRAM used (MiB).
    pub fn set_vram_used_mib(&self, mib: u64) {
        self.vram_used_mib.store(mib, Ordering::Relaxed);
    }

    /// Record a memory allocation.
    pub fn record_allocation(&self) {
        self.alloc_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an OOM event.
    pub fn record_oom(&self) {
        self.oom_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Update temperature in Celsius (0.1°C precision).
    pub fn set_temperature(&self, celsius: f64) {
        self.temperature_decicelsius
            .store((celsius * 10.0) as u64, Ordering::Relaxed);
    }

    /// Record a thermal throttle event.
    pub fn record_throttle(&self) {
        self.throttle_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Update fan speed (0-100%).
    pub fn set_fan_speed_pct(&self, pct: u64) {
        self.fan_speed_pct.store(pct.min(100), Ordering::Relaxed);
    }

    /// Update power draw in milliwatts.
    pub fn set_power_draw_mw(&self, mw: u64) {
        self.power_draw_mw.store(mw, Ordering::Relaxed);
    }

    /// Record a power cap event.
    pub fn record_power_cap(&self) {
        self.power_cap_events.fetch_add(1, Ordering::Relaxed);
    }

    /// Take an immutable snapshot of current metrics.
    pub fn snapshot(&self, device_id: &str, host_id: &str) -> GpuDeviceSnapshot {
        let compute = self.compute_utilization.load(Ordering::Relaxed);
        let vram_used = self.vram_used_mib.load(Ordering::Relaxed);
        let vram_total = self.vram_total_mib.load(Ordering::Relaxed);
        let temp_deci = self.temperature_decicelsius.load(Ordering::Relaxed);
        let power_mw = self.power_draw_mw.load(Ordering::Relaxed);
        let tdp_mw = self.tdp_mw.load(Ordering::Relaxed);

        let health = if compute == 0
            && vram_used == 0
            && temp_deci == 0
            && self.alloc_count.load(Ordering::Relaxed) == 0
        {
            GpuHealthState::Unknown
        } else if self.throttle_events.load(Ordering::Relaxed) > 0
            || self.oom_events.load(Ordering::Relaxed) > 0
        {
            GpuHealthState::Degraded
        } else {
            GpuHealthState::Healthy
        };

        GpuDeviceSnapshot {
            device_id: device_id.to_string(),
            host_id: host_id.to_string(),
            health,
            compute_utilization_pct: compute,
            memory_utilization_pct: self.memory_utilization.load(Ordering::Relaxed),
            encoder_utilization_pct: self.encoder_utilization.load(Ordering::Relaxed),
            decoder_utilization_pct: self.decoder_utilization.load(Ordering::Relaxed),
            vram_used_mib: vram_used,
            vram_total_mib: vram_total,
            alloc_count: self.alloc_count.load(Ordering::Relaxed),
            oom_events: self.oom_events.load(Ordering::Relaxed),
            temperature_celsius: temp_deci as f64 / 10.0,
            throttle_events: self.throttle_events.load(Ordering::Relaxed),
            fan_speed_pct: self.fan_speed_pct.load(Ordering::Relaxed),
            power_draw_watts: power_mw as f64 / 1000.0,
            tdp_watts: tdp_mw as f64 / 1000.0,
            power_cap_events: self.power_cap_events.load(Ordering::Relaxed),
            collected_at: SystemTime::now(),
        }
    }
}

/// Immutable snapshot of a single GPU device's metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuDeviceSnapshot {
    pub device_id: String,
    pub host_id: String,
    pub health: GpuHealthState,

    // Utilization
    pub compute_utilization_pct: u64,
    pub memory_utilization_pct: u64,
    pub encoder_utilization_pct: u64,
    pub decoder_utilization_pct: u64,

    // Memory
    pub vram_used_mib: u64,
    pub vram_total_mib: u64,
    pub alloc_count: u64,
    pub oom_events: u64,

    // Thermal
    pub temperature_celsius: f64,
    pub throttle_events: u64,
    pub fan_speed_pct: u64,

    // Power
    pub power_draw_watts: f64,
    pub tdp_watts: f64,
    pub power_cap_events: u64,

    pub collected_at: SystemTime,
}

// ============================================================================
// Fleet-Wide Aggregation
// ============================================================================

/// Fleet-wide GPU health & metrics aggregation.
pub struct GpuMetricsCollector {
    /// Per-device metrics keyed by (host_id, device_id)
    devices: RwLock<HashMap<(String, String), Arc<GpuDeviceMetrics>>>,
}

impl GpuMetricsCollector {
    /// Create a new collector.
    pub fn new() -> Self {
        Self {
            devices: RwLock::new(HashMap::new()),
        }
    }

    /// Register a GPU device for metrics collection.
    pub fn register_device(
        &self,
        host_id: &str,
        device_id: &str,
        vram_total_mib: u64,
        tdp_mw: u64,
    ) -> Arc<GpuDeviceMetrics> {
        let metrics = Arc::new(GpuDeviceMetrics::new(vram_total_mib, tdp_mw));
        self.devices.write().insert(
            (host_id.to_string(), device_id.to_string()),
            metrics.clone(),
        );
        metrics
    }

    /// Unregister a GPU device.
    pub fn unregister_device(&self, host_id: &str, device_id: &str) {
        self.devices
            .write()
            .remove(&(host_id.to_string(), device_id.to_string()));
    }

    /// Get the metrics handle for a specific device.
    pub fn device_metrics(&self, host_id: &str, device_id: &str) -> Option<Arc<GpuDeviceMetrics>> {
        self.devices
            .read()
            .get(&(host_id.to_string(), device_id.to_string()))
            .cloned()
    }

    /// Collect a fleet-wide snapshot of all GPU metrics.
    pub fn collect_fleet_snapshot(&self) -> GpuFleetSnapshot {
        let devices = self.devices.read();
        let mut device_snapshots = Vec::with_capacity(devices.len());
        let mut healthy = 0u64;
        let mut degraded = 0u64;
        let mut offline = 0u64;
        let mut unknown = 0u64;
        let mut total_vram_used = 0u64;
        let mut total_vram_total = 0u64;
        let mut total_power_mw = 0u64;
        let mut utilization_sum = 0u64;

        for ((host_id, device_id), metrics) in devices.iter() {
            let snap = metrics.snapshot(device_id, host_id);
            match snap.health {
                GpuHealthState::Healthy => healthy += 1,
                GpuHealthState::Degraded => degraded += 1,
                GpuHealthState::Offline => offline += 1,
                GpuHealthState::Unknown => unknown += 1,
            }
            total_vram_used += snap.vram_used_mib;
            total_vram_total += snap.vram_total_mib;
            total_power_mw += (snap.power_draw_watts * 1000.0) as u64;
            utilization_sum += snap.compute_utilization_pct;
            device_snapshots.push(snap);
        }

        let device_count = device_snapshots.len() as u64;

        GpuFleetSnapshot {
            total_devices: device_count,
            healthy_devices: healthy,
            degraded_devices: degraded,
            offline_devices: offline,
            unknown_devices: unknown,
            avg_compute_utilization_pct: if device_count > 0 {
                utilization_sum as f64 / device_count as f64
            } else {
                0.0
            },
            total_vram_used_mib: total_vram_used,
            total_vram_total_mib: total_vram_total,
            total_power_draw_watts: total_power_mw as f64 / 1000.0,
            devices: device_snapshots,
            collected_at: SystemTime::now(),
        }
    }

    /// Render fleet GPU metrics in Prometheus text exposition format.
    pub fn to_prometheus(&self) -> String {
        let snapshot = self.collect_fleet_snapshot();
        let mut out = String::with_capacity(8192);

        // Fleet-level gauges
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_total_devices",
            "Total GPU devices in fleet",
            snapshot.total_devices as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_healthy_devices",
            "Healthy GPU devices",
            snapshot.healthy_devices as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_degraded_devices",
            "Degraded GPU devices",
            snapshot.degraded_devices as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_offline_devices",
            "Offline GPU devices",
            snapshot.offline_devices as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_avg_utilization_pct",
            "Average GPU compute utilization across fleet",
            snapshot.avg_compute_utilization_pct,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_vram_used_mib",
            "Total VRAM used across fleet (MiB)",
            snapshot.total_vram_used_mib as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_vram_total_mib",
            "Total VRAM capacity across fleet (MiB)",
            snapshot.total_vram_total_mib as f64,
        );
        write_prometheus_gauge(
            &mut out,
            "hm_gpu_fleet_power_draw_watts",
            "Total GPU power draw across fleet (W)",
            snapshot.total_power_draw_watts,
        );

        // Per-device gauges
        for dev in &snapshot.devices {
            let labels = format!(
                "device_id=\"{}\",host_id=\"{}\"",
                dev.device_id, dev.host_id
            );

            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_compute_utilization_pct",
                "GPU compute utilization",
                &labels,
                dev.compute_utilization_pct as f64,
            );
            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_memory_utilization_pct",
                "GPU memory utilization",
                &labels,
                dev.memory_utilization_pct as f64,
            );
            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_vram_used_mib",
                "VRAM used (MiB)",
                &labels,
                dev.vram_used_mib as f64,
            );
            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_temperature_celsius",
                "GPU temperature (°C)",
                &labels,
                dev.temperature_celsius,
            );
            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_power_draw_watts",
                "GPU power draw (W)",
                &labels,
                dev.power_draw_watts,
            );
            write_prometheus_gauge_labeled(
                &mut out,
                "hm_gpu_fan_speed_pct",
                "GPU fan speed (%)",
                &labels,
                dev.fan_speed_pct as f64,
            );
        }

        out
    }
}

impl Default for GpuMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Fleet-wide GPU metrics snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuFleetSnapshot {
    pub total_devices: u64,
    pub healthy_devices: u64,
    pub degraded_devices: u64,
    pub offline_devices: u64,
    pub unknown_devices: u64,
    pub avg_compute_utilization_pct: f64,
    pub total_vram_used_mib: u64,
    pub total_vram_total_mib: u64,
    pub total_power_draw_watts: f64,
    pub devices: Vec<GpuDeviceSnapshot>,
    pub collected_at: SystemTime,
}

// ============================================================================
// Prometheus helpers
// ============================================================================

fn write_prometheus_gauge(out: &mut String, name: &str, help: &str, value: f64) {
    use std::fmt::Write;
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    let _ = writeln!(out, "{name} {value}");
}

fn write_prometheus_gauge_labeled(
    out: &mut String,
    name: &str,
    help: &str,
    labels: &str,
    value: f64,
) {
    use std::fmt::Write;
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} gauge");
    let _ = writeln!(out, "{name}{{{labels}}} {value}");
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_snapshot_device() {
        let collector = GpuMetricsCollector::new();
        let metrics = collector.register_device("host-1", "gpu-0", 81920, 300_000);

        metrics.set_compute_utilization(75);
        metrics.set_memory_utilization(60);
        metrics.set_vram_used_mib(49152);
        metrics.set_temperature(72.5);
        metrics.set_power_draw_mw(250_000);
        metrics.set_fan_speed_pct(80);

        let snap = metrics.snapshot("gpu-0", "host-1");
        assert_eq!(snap.compute_utilization_pct, 75);
        assert_eq!(snap.memory_utilization_pct, 60);
        assert_eq!(snap.vram_used_mib, 49152);
        assert_eq!(snap.vram_total_mib, 81920);
        assert!((snap.temperature_celsius - 72.5).abs() < 0.2);
        assert!((snap.power_draw_watts - 250.0).abs() < 0.01);
        assert_eq!(snap.fan_speed_pct, 80);
        assert_eq!(snap.health, GpuHealthState::Healthy);
    }

    #[test]
    fn degraded_on_throttle() {
        let collector = GpuMetricsCollector::new();
        let metrics = collector.register_device("host-1", "gpu-0", 16384, 250_000);
        metrics.set_compute_utilization(90);
        metrics.record_throttle();

        let snap = metrics.snapshot("gpu-0", "host-1");
        assert_eq!(snap.health, GpuHealthState::Degraded);
        assert_eq!(snap.throttle_events, 1);
    }

    #[test]
    fn degraded_on_oom() {
        let collector = GpuMetricsCollector::new();
        let metrics = collector.register_device("host-1", "gpu-0", 16384, 250_000);
        metrics.set_compute_utilization(50);
        metrics.record_oom();

        let snap = metrics.snapshot("gpu-0", "host-1");
        assert_eq!(snap.health, GpuHealthState::Degraded);
    }

    #[test]
    fn unknown_when_no_data_reported() {
        let collector = GpuMetricsCollector::new();
        let metrics = collector.register_device("host-1", "gpu-0", 16384, 250_000);
        let snap = metrics.snapshot("gpu-0", "host-1");
        assert_eq!(snap.health, GpuHealthState::Unknown);
    }

    #[test]
    fn fleet_snapshot_aggregates() {
        let collector = GpuMetricsCollector::new();

        let m0 = collector.register_device("host-1", "gpu-0", 81920, 300_000);
        m0.set_compute_utilization(80);
        m0.set_vram_used_mib(40960);
        m0.set_power_draw_mw(250_000);

        let m1 = collector.register_device("host-1", "gpu-1", 81920, 300_000);
        m1.set_compute_utilization(60);
        m1.set_vram_used_mib(20480);
        m1.set_power_draw_mw(200_000);

        let fleet = collector.collect_fleet_snapshot();
        assert_eq!(fleet.total_devices, 2);
        assert_eq!(fleet.healthy_devices, 2);
        assert_eq!(fleet.total_vram_used_mib, 40960 + 20480);
        assert_eq!(fleet.total_vram_total_mib, 81920 * 2);
        assert!((fleet.avg_compute_utilization_pct - 70.0).abs() < 0.01);
        assert!((fleet.total_power_draw_watts - 450.0).abs() < 0.01);
    }

    #[test]
    fn unregister_removes_device() {
        let collector = GpuMetricsCollector::new();
        collector.register_device("host-1", "gpu-0", 16384, 250_000);
        assert!(collector.device_metrics("host-1", "gpu-0").is_some());
        collector.unregister_device("host-1", "gpu-0");
        assert!(collector.device_metrics("host-1", "gpu-0").is_none());
    }

    #[test]
    fn prometheus_output_contains_fleet_metrics() {
        let collector = GpuMetricsCollector::new();
        let m = collector.register_device("host-1", "gpu-0", 81920, 300_000);
        m.set_compute_utilization(50);
        m.set_temperature(65.0);
        m.set_power_draw_mw(200_000);

        let prom = collector.to_prometheus();
        assert!(prom.contains("hm_gpu_fleet_total_devices"));
        assert!(prom.contains("hm_gpu_fleet_healthy_devices"));
        assert!(prom.contains("hm_gpu_compute_utilization_pct"));
        assert!(prom.contains("hm_gpu_temperature_celsius"));
        assert!(prom.contains("hm_gpu_power_draw_watts"));
        assert!(prom.contains("device_id=\"gpu-0\""));
        assert!(prom.contains("host_id=\"host-1\""));
    }

    #[test]
    fn utilization_clamped_to_100() {
        let collector = GpuMetricsCollector::new();
        let m = collector.register_device("host-1", "gpu-0", 16384, 250_000);
        m.set_compute_utilization(150);
        let snap = m.snapshot("gpu-0", "host-1");
        assert_eq!(snap.compute_utilization_pct, 100);
    }

    #[test]
    fn alloc_counter_increments() {
        let collector = GpuMetricsCollector::new();
        let m = collector.register_device("host-1", "gpu-0", 16384, 250_000);
        m.record_allocation();
        m.record_allocation();
        m.record_allocation();
        let snap = m.snapshot("gpu-0", "host-1");
        assert_eq!(snap.alloc_count, 3);
    }
}
