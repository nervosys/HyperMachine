//! VM-specific Metrics
//!
//! Metrics and statistics specific to virtual machines and hypervisor operations.

use super::types::MovingAverage;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// vCPU metrics
#[derive(Debug)]
pub struct VcpuMetrics {
    /// vCPU ID
    pub vcpu_id: u32,
    /// Total run time in nanoseconds
    run_time_ns: AtomicU64,
    /// Total halt time in nanoseconds
    halt_time_ns: AtomicU64,
    /// Number of exits
    exits: AtomicU64,
    /// Exit reasons
    exit_reasons: RwLock<HashMap<String, u64>>,
    /// Instructions retired
    instructions_retired: AtomicU64,
    /// Interrupts delivered
    interrupts: AtomicU64,
    /// IO port exits
    io_exits: AtomicU64,
    /// MMIO exits
    mmio_exits: AtomicU64,
    /// Hypercall exits
    hypercall_exits: AtomicU64,
    /// Last update time
    last_update: RwLock<Instant>,
    /// CPU utilization (moving average)
    utilization: RwLock<MovingAverage>,
}

impl VcpuMetrics {
    /// Create new vCPU metrics
    pub fn new(vcpu_id: u32) -> Self {
        Self {
            vcpu_id,
            run_time_ns: AtomicU64::new(0),
            halt_time_ns: AtomicU64::new(0),
            exits: AtomicU64::new(0),
            exit_reasons: RwLock::new(HashMap::new()),
            instructions_retired: AtomicU64::new(0),
            interrupts: AtomicU64::new(0),
            io_exits: AtomicU64::new(0),
            mmio_exits: AtomicU64::new(0),
            hypercall_exits: AtomicU64::new(0),
            last_update: RwLock::new(Instant::now()),
            utilization: RwLock::new(MovingAverage::new(10)),
        }
    }

    /// Record run time
    pub fn record_run_time(&self, ns: u64) {
        self.run_time_ns.fetch_add(ns, Ordering::Relaxed);
        self.update_utilization();
    }

    /// Record halt time
    pub fn record_halt_time(&self, ns: u64) {
        self.halt_time_ns.fetch_add(ns, Ordering::Relaxed);
    }

    /// Record an exit
    pub fn record_exit(&self, reason: &str) {
        self.exits.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut reasons) = self.exit_reasons.write() {
            *reasons.entry(reason.to_string()).or_insert(0) += 1;
        }
    }

    /// Record IO exit
    pub fn record_io_exit(&self) {
        self.io_exits.fetch_add(1, Ordering::Relaxed);
        self.record_exit("io");
    }

    /// Record MMIO exit
    pub fn record_mmio_exit(&self) {
        self.mmio_exits.fetch_add(1, Ordering::Relaxed);
        self.record_exit("mmio");
    }

    /// Record hypercall exit
    pub fn record_hypercall_exit(&self) {
        self.hypercall_exits.fetch_add(1, Ordering::Relaxed);
        self.record_exit("hypercall");
    }

    /// Record interrupt delivery
    pub fn record_interrupt(&self) {
        self.interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Record instructions retired
    pub fn add_instructions(&self, count: u64) {
        self.instructions_retired
            .fetch_add(count, Ordering::Relaxed);
    }

    /// Update utilization
    fn update_utilization(&self) {
        /// Minimum elapsed time between utilization recalculations.
        const UTILIZATION_UPDATE_INTERVAL: Duration = Duration::from_millis(100);

        if let (Ok(mut last), Ok(mut util)) = (self.last_update.write(), self.utilization.write()) {
            let elapsed = last.elapsed();
            if elapsed > UTILIZATION_UPDATE_INTERVAL {
                let run_time = self.run_time_ns.load(Ordering::Relaxed) as f64;
                let halt_time = self.halt_time_ns.load(Ordering::Relaxed) as f64;
                let total = run_time + halt_time;

                if total > 0.0 {
                    let utilization = run_time / total * 100.0;
                    util.add_sample(utilization);
                }

                *last = Instant::now();
            }
        }
    }

    /// Get total run time
    pub fn run_time_ns(&self) -> u64 {
        self.run_time_ns.load(Ordering::Relaxed)
    }

    /// Get total halt time
    pub fn halt_time_ns(&self) -> u64 {
        self.halt_time_ns.load(Ordering::Relaxed)
    }

    /// Get total exits
    pub fn exits(&self) -> u64 {
        self.exits.load(Ordering::Relaxed)
    }

    /// Get IO exits
    pub fn io_exits(&self) -> u64 {
        self.io_exits.load(Ordering::Relaxed)
    }

    /// Get MMIO exits
    pub fn mmio_exits(&self) -> u64 {
        self.mmio_exits.load(Ordering::Relaxed)
    }

    /// Get hypercall exits
    pub fn hypercall_exits(&self) -> u64 {
        self.hypercall_exits.load(Ordering::Relaxed)
    }

    /// Get interrupts
    pub fn interrupts(&self) -> u64 {
        self.interrupts.load(Ordering::Relaxed)
    }

    /// Get instructions retired
    pub fn instructions_retired(&self) -> u64 {
        self.instructions_retired.load(Ordering::Relaxed)
    }

    /// Get exit reasons
    pub fn exit_reasons(&self) -> HashMap<String, u64> {
        self.exit_reasons
            .read()
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// Get current utilization
    pub fn utilization(&self) -> f64 {
        self.utilization
            .read()
            .ok()
            .map(|u| u.average())
            .unwrap_or(0.0)
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.run_time_ns.store(0, Ordering::Relaxed);
        self.halt_time_ns.store(0, Ordering::Relaxed);
        self.exits.store(0, Ordering::Relaxed);
        self.io_exits.store(0, Ordering::Relaxed);
        self.mmio_exits.store(0, Ordering::Relaxed);
        self.hypercall_exits.store(0, Ordering::Relaxed);
        self.interrupts.store(0, Ordering::Relaxed);
        self.instructions_retired.store(0, Ordering::Relaxed);

        if let Ok(mut reasons) = self.exit_reasons.write() {
            reasons.clear();
        }
    }
}

/// Memory metrics
#[derive(Debug)]
pub struct MemoryMetrics {
    /// Total memory bytes
    total_bytes: AtomicU64,
    /// Used memory bytes
    used_bytes: AtomicU64,
    /// Free memory bytes
    free_bytes: AtomicU64,
    /// Page faults
    page_faults: AtomicU64,
    /// Major page faults
    major_page_faults: AtomicU64,
    /// Pages swapped in
    swap_in_pages: AtomicU64,
    /// Pages swapped out
    swap_out_pages: AtomicU64,
    /// Memory balloon target
    balloon_target: AtomicU64,
    /// Memory balloon actual
    balloon_actual: AtomicU64,
    /// DMA allocations
    dma_allocations: AtomicU64,
    /// DMA allocation failures
    dma_failures: AtomicU64,
}

impl MemoryMetrics {
    /// Create new memory metrics
    pub fn new() -> Self {
        Self {
            total_bytes: AtomicU64::new(0),
            used_bytes: AtomicU64::new(0),
            free_bytes: AtomicU64::new(0),
            page_faults: AtomicU64::new(0),
            major_page_faults: AtomicU64::new(0),
            swap_in_pages: AtomicU64::new(0),
            swap_out_pages: AtomicU64::new(0),
            balloon_target: AtomicU64::new(0),
            balloon_actual: AtomicU64::new(0),
            dma_allocations: AtomicU64::new(0),
            dma_failures: AtomicU64::new(0),
        }
    }

    /// Set total memory
    pub fn set_total(&self, bytes: u64) {
        self.total_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Set used memory
    pub fn set_used(&self, bytes: u64) {
        self.used_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Set free memory
    pub fn set_free(&self, bytes: u64) {
        self.free_bytes.store(bytes, Ordering::Relaxed);
    }

    /// Record page fault
    pub fn record_page_fault(&self, major: bool) {
        self.page_faults.fetch_add(1, Ordering::Relaxed);
        if major {
            self.major_page_faults.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record swap in
    pub fn record_swap_in(&self, pages: u64) {
        self.swap_in_pages.fetch_add(pages, Ordering::Relaxed);
    }

    /// Record swap out
    pub fn record_swap_out(&self, pages: u64) {
        self.swap_out_pages.fetch_add(pages, Ordering::Relaxed);
    }

    /// Set balloon target
    pub fn set_balloon_target(&self, bytes: u64) {
        self.balloon_target.store(bytes, Ordering::Relaxed);
    }

    /// Set balloon actual
    pub fn set_balloon_actual(&self, bytes: u64) {
        self.balloon_actual.store(bytes, Ordering::Relaxed);
    }

    /// Record DMA allocation
    pub fn record_dma_allocation(&self, success: bool) {
        if success {
            self.dma_allocations.fetch_add(1, Ordering::Relaxed);
        } else {
            self.dma_failures.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Get total bytes
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    /// Get used bytes
    pub fn used_bytes(&self) -> u64 {
        self.used_bytes.load(Ordering::Relaxed)
    }

    /// Get free bytes
    pub fn free_bytes(&self) -> u64 {
        self.free_bytes.load(Ordering::Relaxed)
    }

    /// Get page faults
    pub fn page_faults(&self) -> u64 {
        self.page_faults.load(Ordering::Relaxed)
    }

    /// Get major page faults
    pub fn major_page_faults(&self) -> u64 {
        self.major_page_faults.load(Ordering::Relaxed)
    }

    /// Get swap in pages
    pub fn swap_in_pages(&self) -> u64 {
        self.swap_in_pages.load(Ordering::Relaxed)
    }

    /// Get swap out pages
    pub fn swap_out_pages(&self) -> u64 {
        self.swap_out_pages.load(Ordering::Relaxed)
    }

    /// Get memory utilization percentage
    pub fn utilization(&self) -> f64 {
        let total = self.total_bytes.load(Ordering::Relaxed) as f64;
        if total > 0.0 {
            let used = self.used_bytes.load(Ordering::Relaxed) as f64;
            used / total * 100.0
        } else {
            0.0
        }
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.page_faults.store(0, Ordering::Relaxed);
        self.major_page_faults.store(0, Ordering::Relaxed);
        self.swap_in_pages.store(0, Ordering::Relaxed);
        self.swap_out_pages.store(0, Ordering::Relaxed);
        self.dma_allocations.store(0, Ordering::Relaxed);
        self.dma_failures.store(0, Ordering::Relaxed);
    }
}

impl Default for MemoryMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Disk metrics
#[derive(Debug)]
pub struct DiskMetrics {
    /// Disk name/ID
    pub name: String,
    /// Read operations
    read_ops: AtomicU64,
    /// Write operations
    write_ops: AtomicU64,
    /// Bytes read
    bytes_read: AtomicU64,
    /// Bytes written
    bytes_written: AtomicU64,
    /// Read latency tracker
    read_latency: RwLock<super::types::HistogramData>,
    /// Write latency tracker
    write_latency: RwLock<super::types::HistogramData>,
    /// Queue depth
    queue_depth: AtomicU64,
    /// Flush operations
    flush_ops: AtomicU64,
    /// Discard operations
    discard_ops: AtomicU64,
    /// Errors
    errors: AtomicU64,
}

impl DiskMetrics {
    /// Create new disk metrics
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            read_ops: AtomicU64::new(0),
            write_ops: AtomicU64::new(0),
            bytes_read: AtomicU64::new(0),
            bytes_written: AtomicU64::new(0),
            read_latency: RwLock::new(super::types::HistogramData::new()),
            write_latency: RwLock::new(super::types::HistogramData::new()),
            queue_depth: AtomicU64::new(0),
            flush_ops: AtomicU64::new(0),
            discard_ops: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    /// Record read operation
    pub fn record_read(&self, bytes: u64, latency_us: u64) {
        self.read_ops.fetch_add(1, Ordering::Relaxed);
        self.bytes_read.fetch_add(bytes, Ordering::Relaxed);

        if let Ok(mut hist) = self.read_latency.write() {
            hist.observe(latency_us as f64);
        }
    }

    /// Record write operation
    pub fn record_write(&self, bytes: u64, latency_us: u64) {
        self.write_ops.fetch_add(1, Ordering::Relaxed);
        self.bytes_written.fetch_add(bytes, Ordering::Relaxed);

        if let Ok(mut hist) = self.write_latency.write() {
            hist.observe(latency_us as f64);
        }
    }

    /// Record flush operation
    pub fn record_flush(&self) {
        self.flush_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Record discard operation
    pub fn record_discard(&self) {
        self.discard_ops.fetch_add(1, Ordering::Relaxed);
    }

    /// Record error
    pub fn record_error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Set queue depth
    pub fn set_queue_depth(&self, depth: u64) {
        self.queue_depth.store(depth, Ordering::Relaxed);
    }

    /// Get read operations
    pub fn read_ops(&self) -> u64 {
        self.read_ops.load(Ordering::Relaxed)
    }

    /// Get write operations
    pub fn write_ops(&self) -> u64 {
        self.write_ops.load(Ordering::Relaxed)
    }

    /// Get bytes read
    pub fn bytes_read(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed)
    }

    /// Get bytes written
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written.load(Ordering::Relaxed)
    }

    /// Get total IOPS
    pub fn total_iops(&self) -> u64 {
        self.read_ops.load(Ordering::Relaxed) + self.write_ops.load(Ordering::Relaxed)
    }

    /// Get total throughput
    pub fn total_throughput(&self) -> u64 {
        self.bytes_read.load(Ordering::Relaxed) + self.bytes_written.load(Ordering::Relaxed)
    }

    /// Get average read latency
    pub fn avg_read_latency(&self) -> Option<f64> {
        self.read_latency.read().ok()?.mean()
    }

    /// Get average write latency
    pub fn avg_write_latency(&self) -> Option<f64> {
        self.write_latency.read().ok()?.mean()
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.read_ops.store(0, Ordering::Relaxed);
        self.write_ops.store(0, Ordering::Relaxed);
        self.bytes_read.store(0, Ordering::Relaxed);
        self.bytes_written.store(0, Ordering::Relaxed);
        self.flush_ops.store(0, Ordering::Relaxed);
        self.discard_ops.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);

        if let Ok(mut hist) = self.read_latency.write() {
            hist.reset();
        }
        if let Ok(mut hist) = self.write_latency.write() {
            hist.reset();
        }
    }
}

/// Network metrics
#[derive(Debug)]
pub struct NetworkMetrics {
    /// Interface name
    pub name: String,
    /// Packets received
    rx_packets: AtomicU64,
    /// Bytes received
    rx_bytes: AtomicU64,
    /// Packets transmitted
    tx_packets: AtomicU64,
    /// Bytes transmitted
    tx_bytes: AtomicU64,
    /// Receive errors
    rx_errors: AtomicU64,
    /// Transmit errors
    tx_errors: AtomicU64,
    /// Dropped packets (receive)
    rx_dropped: AtomicU64,
    /// Dropped packets (transmit)
    tx_dropped: AtomicU64,
    /// Multicast packets
    multicast: AtomicU64,
    /// Collisions
    collisions: AtomicU64,
}

impl NetworkMetrics {
    /// Create new network metrics
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            rx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
            rx_dropped: AtomicU64::new(0),
            tx_dropped: AtomicU64::new(0),
            multicast: AtomicU64::new(0),
            collisions: AtomicU64::new(0),
        }
    }

    /// Record packet received
    pub fn record_rx(&self, bytes: u64) {
        self.rx_packets.fetch_add(1, Ordering::Relaxed);
        self.rx_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record packet transmitted
    pub fn record_tx(&self, bytes: u64) {
        self.tx_packets.fetch_add(1, Ordering::Relaxed);
        self.tx_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record receive error
    pub fn record_rx_error(&self) {
        self.rx_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record transmit error
    pub fn record_tx_error(&self) {
        self.tx_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record dropped receive packet
    pub fn record_rx_dropped(&self) {
        self.rx_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record dropped transmit packet
    pub fn record_tx_dropped(&self) {
        self.tx_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record multicast packet
    pub fn record_multicast(&self) {
        self.multicast.fetch_add(1, Ordering::Relaxed);
    }

    /// Record collision
    pub fn record_collision(&self) {
        self.collisions.fetch_add(1, Ordering::Relaxed);
    }

    /// Get receive packets
    pub fn rx_packets(&self) -> u64 {
        self.rx_packets.load(Ordering::Relaxed)
    }

    /// Get receive bytes
    pub fn rx_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed)
    }

    /// Get transmit packets
    pub fn tx_packets(&self) -> u64 {
        self.tx_packets.load(Ordering::Relaxed)
    }

    /// Get transmit bytes
    pub fn tx_bytes(&self) -> u64 {
        self.tx_bytes.load(Ordering::Relaxed)
    }

    /// Get receive errors
    pub fn rx_errors(&self) -> u64 {
        self.rx_errors.load(Ordering::Relaxed)
    }

    /// Get transmit errors
    pub fn tx_errors(&self) -> u64 {
        self.tx_errors.load(Ordering::Relaxed)
    }

    /// Get total packets
    pub fn total_packets(&self) -> u64 {
        self.rx_packets.load(Ordering::Relaxed) + self.tx_packets.load(Ordering::Relaxed)
    }

    /// Get total bytes
    pub fn total_bytes(&self) -> u64 {
        self.rx_bytes.load(Ordering::Relaxed) + self.tx_bytes.load(Ordering::Relaxed)
    }

    /// Reset metrics
    pub fn reset(&self) {
        self.rx_packets.store(0, Ordering::Relaxed);
        self.rx_bytes.store(0, Ordering::Relaxed);
        self.tx_packets.store(0, Ordering::Relaxed);
        self.tx_bytes.store(0, Ordering::Relaxed);
        self.rx_errors.store(0, Ordering::Relaxed);
        self.tx_errors.store(0, Ordering::Relaxed);
        self.rx_dropped.store(0, Ordering::Relaxed);
        self.tx_dropped.store(0, Ordering::Relaxed);
        self.multicast.store(0, Ordering::Relaxed);
        self.collisions.store(0, Ordering::Relaxed);
    }
}

/// Hypervisor-level metrics
#[derive(Debug)]
pub struct HypervisorMetrics {
    /// Active VMs
    active_vms: AtomicU64,
    /// Total VMs created
    total_vms_created: AtomicU64,
    /// Total VMs destroyed
    total_vms_destroyed: AtomicU64,
    /// Total vCPUs
    total_vcpus: AtomicU64,
    /// Memory overcommit ratio
    memory_overcommit: RwLock<f64>,
    /// CPU overcommit ratio
    cpu_overcommit: RwLock<f64>,
    /// Hypervisor uptime (seconds)
    uptime_secs: AtomicU64,
    /// Start time
    start_time: Instant,
}

impl HypervisorMetrics {
    /// Create new hypervisor metrics
    pub fn new() -> Self {
        Self {
            active_vms: AtomicU64::new(0),
            total_vms_created: AtomicU64::new(0),
            total_vms_destroyed: AtomicU64::new(0),
            total_vcpus: AtomicU64::new(0),
            memory_overcommit: RwLock::new(1.0),
            cpu_overcommit: RwLock::new(1.0),
            uptime_secs: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record VM created
    pub fn record_vm_created(&self, vcpus: u32) {
        self.active_vms.fetch_add(1, Ordering::Relaxed);
        self.total_vms_created.fetch_add(1, Ordering::Relaxed);
        self.total_vcpus.fetch_add(vcpus as u64, Ordering::Relaxed);
    }

    /// Record VM destroyed
    pub fn record_vm_destroyed(&self, vcpus: u32) {
        self.active_vms.fetch_sub(1, Ordering::Relaxed);
        self.total_vms_destroyed.fetch_add(1, Ordering::Relaxed);
        self.total_vcpus.fetch_sub(vcpus as u64, Ordering::Relaxed);
    }

    /// Set memory overcommit ratio
    pub fn set_memory_overcommit(&self, ratio: f64) {
        if let Ok(mut r) = self.memory_overcommit.write() {
            *r = ratio;
        }
    }

    /// Set CPU overcommit ratio
    pub fn set_cpu_overcommit(&self, ratio: f64) {
        if let Ok(mut r) = self.cpu_overcommit.write() {
            *r = ratio;
        }
    }

    /// Get active VMs
    pub fn active_vms(&self) -> u64 {
        self.active_vms.load(Ordering::Relaxed)
    }

    /// Get total VMs created
    pub fn total_vms_created(&self) -> u64 {
        self.total_vms_created.load(Ordering::Relaxed)
    }

    /// Get total vCPUs
    pub fn total_vcpus(&self) -> u64 {
        self.total_vcpus.load(Ordering::Relaxed)
    }

    /// Get uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get memory overcommit ratio
    pub fn memory_overcommit(&self) -> f64 {
        self.memory_overcommit.read().map(|r| *r).unwrap_or(1.0)
    }

    /// Get CPU overcommit ratio
    pub fn cpu_overcommit(&self) -> f64 {
        self.cpu_overcommit.read().map(|r| *r).unwrap_or(1.0)
    }
}

impl Default for HypervisorMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Complete VM metrics collection
#[derive(Debug)]
pub struct VmMetrics {
    /// VM name/ID
    pub name: String,
    /// vCPU metrics
    vcpus: RwLock<Vec<Arc<VcpuMetrics>>>,
    /// Memory metrics
    pub memory: MemoryMetrics,
    /// Disk metrics
    disks: RwLock<HashMap<String, Arc<DiskMetrics>>>,
    /// Network metrics
    networks: RwLock<HashMap<String, Arc<NetworkMetrics>>>,
    /// VM creation time
    created_at: Instant,
    /// Boot time (if booted)
    boot_time: RwLock<Option<Duration>>,
    /// Snapshot count
    snapshot_count: AtomicU64,
    /// Migration count
    migration_count: AtomicU64,
}

impl VmMetrics {
    /// Create new VM metrics
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            vcpus: RwLock::new(Vec::new()),
            memory: MemoryMetrics::new(),
            disks: RwLock::new(HashMap::new()),
            networks: RwLock::new(HashMap::new()),
            created_at: Instant::now(),
            boot_time: RwLock::new(None),
            snapshot_count: AtomicU64::new(0),
            migration_count: AtomicU64::new(0),
        }
    }

    /// Add a vCPU
    pub fn add_vcpu(&self, vcpu_id: u32) -> Arc<VcpuMetrics> {
        let metrics = Arc::new(VcpuMetrics::new(vcpu_id));
        if let Ok(mut vcpus) = self.vcpus.write() {
            vcpus.push(metrics.clone());
        }
        metrics
    }

    /// Get vCPU metrics
    pub fn get_vcpu(&self, vcpu_id: u32) -> Option<Arc<VcpuMetrics>> {
        self.vcpus
            .read()
            .ok()?
            .iter()
            .find(|v| v.vcpu_id == vcpu_id)
            .cloned()
    }

    /// Get all vCPU metrics
    pub fn vcpus(&self) -> Vec<Arc<VcpuMetrics>> {
        self.vcpus.read().map(|v| v.clone()).unwrap_or_default()
    }

    /// Add a disk
    pub fn add_disk(&self, name: impl Into<String>) -> Arc<DiskMetrics> {
        let name = name.into();
        let metrics = Arc::new(DiskMetrics::new(&name));
        if let Ok(mut disks) = self.disks.write() {
            disks.insert(name, metrics.clone());
        }
        metrics
    }

    /// Get disk metrics
    pub fn get_disk(&self, name: &str) -> Option<Arc<DiskMetrics>> {
        self.disks.read().ok()?.get(name).cloned()
    }

    /// Get all disk metrics
    pub fn disks(&self) -> HashMap<String, Arc<DiskMetrics>> {
        self.disks.read().map(|d| d.clone()).unwrap_or_default()
    }

    /// Add a network interface
    pub fn add_network(&self, name: impl Into<String>) -> Arc<NetworkMetrics> {
        let name = name.into();
        let metrics = Arc::new(NetworkMetrics::new(&name));
        if let Ok(mut networks) = self.networks.write() {
            networks.insert(name, metrics.clone());
        }
        metrics
    }

    /// Get network metrics
    pub fn get_network(&self, name: &str) -> Option<Arc<NetworkMetrics>> {
        self.networks.read().ok()?.get(name).cloned()
    }

    /// Get all network metrics
    pub fn networks(&self) -> HashMap<String, Arc<NetworkMetrics>> {
        self.networks.read().map(|n| n.clone()).unwrap_or_default()
    }

    /// Record boot complete
    pub fn record_boot_complete(&self) {
        if let Ok(mut boot_time) = self.boot_time.write() {
            *boot_time = Some(self.created_at.elapsed());
        }
    }

    /// Get boot time
    pub fn boot_time(&self) -> Option<Duration> {
        *self.boot_time.read().ok()?
    }

    /// Record snapshot
    pub fn record_snapshot(&self) {
        self.snapshot_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record migration
    pub fn record_migration(&self) {
        self.migration_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot count
    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count.load(Ordering::Relaxed)
    }

    /// Get migration count
    pub fn migration_count(&self) -> u64 {
        self.migration_count.load(Ordering::Relaxed)
    }

    /// Get VM age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get aggregate CPU utilization
    pub fn cpu_utilization(&self) -> f64 {
        let vcpus = match self.vcpus.read() {
            Ok(v) => v,
            Err(_) => return 0.0,
        };

        if vcpus.is_empty() {
            return 0.0;
        }

        let total: f64 = vcpus.iter().map(|v| v.utilization()).sum();
        total / vcpus.len() as f64
    }

    /// Get total disk IOPS
    pub fn total_disk_iops(&self) -> u64 {
        self.disks
            .read()
            .map(|d| d.values().map(|disk| disk.total_iops()).sum())
            .unwrap_or(0)
    }

    /// Get total network throughput
    pub fn total_network_throughput(&self) -> u64 {
        self.networks
            .read()
            .map(|n| n.values().map(|net| net.total_bytes()).sum())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vcpu_metrics_basic() {
        let metrics = VcpuMetrics::new(0);

        assert_eq!(metrics.vcpu_id, 0);
        assert_eq!(metrics.exits(), 0);
        assert_eq!(metrics.run_time_ns(), 0);
    }

    #[test]
    fn test_vcpu_metrics_exits() {
        let metrics = VcpuMetrics::new(0);

        metrics.record_exit("io");
        metrics.record_exit("io");
        metrics.record_exit("mmio");

        assert_eq!(metrics.exits(), 3);

        let reasons = metrics.exit_reasons();
        assert_eq!(reasons.get("io"), Some(&2));
        assert_eq!(reasons.get("mmio"), Some(&1));
    }

    #[test]
    fn test_vcpu_metrics_io_mmio_hypercall() {
        let metrics = VcpuMetrics::new(0);

        metrics.record_io_exit();
        metrics.record_io_exit();
        metrics.record_mmio_exit();
        metrics.record_hypercall_exit();

        assert_eq!(metrics.io_exits(), 2);
        assert_eq!(metrics.mmio_exits(), 1);
        assert_eq!(metrics.hypercall_exits(), 1);
        assert_eq!(metrics.exits(), 4);
    }

    #[test]
    fn test_vcpu_metrics_time() {
        let metrics = VcpuMetrics::new(0);

        metrics.record_run_time(1_000_000);
        metrics.record_halt_time(500_000);

        assert_eq!(metrics.run_time_ns(), 1_000_000);
        assert_eq!(metrics.halt_time_ns(), 500_000);
    }

    #[test]
    fn test_vcpu_metrics_reset() {
        let metrics = VcpuMetrics::new(0);

        metrics.record_io_exit();
        metrics.record_run_time(1000);
        metrics.record_interrupt();

        metrics.reset();

        assert_eq!(metrics.exits(), 0);
        assert_eq!(metrics.run_time_ns(), 0);
        assert_eq!(metrics.interrupts(), 0);
    }

    #[test]
    fn test_memory_metrics_basic() {
        let metrics = MemoryMetrics::new();

        metrics.set_total(8 * 1024 * 1024 * 1024); // 8 GB
        metrics.set_used(4 * 1024 * 1024 * 1024); // 4 GB
        metrics.set_free(4 * 1024 * 1024 * 1024); // 4 GB

        assert_eq!(metrics.total_bytes(), 8 * 1024 * 1024 * 1024);
        assert_eq!(metrics.used_bytes(), 4 * 1024 * 1024 * 1024);
        assert!((metrics.utilization() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_memory_metrics_page_faults() {
        let metrics = MemoryMetrics::new();

        metrics.record_page_fault(false);
        metrics.record_page_fault(false);
        metrics.record_page_fault(true);

        assert_eq!(metrics.page_faults(), 3);
        assert_eq!(metrics.major_page_faults(), 1);
    }

    #[test]
    fn test_memory_metrics_swap() {
        let metrics = MemoryMetrics::new();

        metrics.record_swap_in(10);
        metrics.record_swap_out(5);

        assert_eq!(metrics.swap_in_pages(), 10);
        assert_eq!(metrics.swap_out_pages(), 5);
    }

    #[test]
    fn test_disk_metrics_basic() {
        let metrics = DiskMetrics::new("vda");

        metrics.record_read(4096, 100);
        metrics.record_read(4096, 150);
        metrics.record_write(8192, 200);

        assert_eq!(metrics.read_ops(), 2);
        assert_eq!(metrics.write_ops(), 1);
        assert_eq!(metrics.bytes_read(), 8192);
        assert_eq!(metrics.bytes_written(), 8192);
        assert_eq!(metrics.total_iops(), 3);
    }

    #[test]
    fn test_disk_metrics_latency() {
        let metrics = DiskMetrics::new("vda");

        metrics.record_read(4096, 100);
        metrics.record_read(4096, 200);

        let avg = metrics.avg_read_latency().unwrap();
        assert!((avg - 150.0).abs() < 0.1);
    }

    #[test]
    fn test_disk_metrics_flush_discard() {
        let metrics = DiskMetrics::new("vda");

        metrics.record_flush();
        metrics.record_flush();
        metrics.record_discard();

        // These are tracked internally
        assert_eq!(metrics.total_iops(), 0); // Flush/discard don't count as IOPS
    }

    #[test]
    fn test_network_metrics_basic() {
        let metrics = NetworkMetrics::new("eth0");

        metrics.record_rx(1500);
        metrics.record_rx(1500);
        metrics.record_tx(500);

        assert_eq!(metrics.rx_packets(), 2);
        assert_eq!(metrics.tx_packets(), 1);
        assert_eq!(metrics.rx_bytes(), 3000);
        assert_eq!(metrics.tx_bytes(), 500);
        assert_eq!(metrics.total_packets(), 3);
        assert_eq!(metrics.total_bytes(), 3500);
    }

    #[test]
    fn test_network_metrics_errors() {
        let metrics = NetworkMetrics::new("eth0");

        metrics.record_rx_error();
        metrics.record_tx_error();
        metrics.record_rx_dropped();
        metrics.record_tx_dropped();

        assert_eq!(metrics.rx_errors(), 1);
        assert_eq!(metrics.tx_errors(), 1);
    }

    #[test]
    fn test_hypervisor_metrics_basic() {
        let metrics = HypervisorMetrics::new();

        assert_eq!(metrics.active_vms(), 0);
        assert_eq!(metrics.total_vcpus(), 0);
    }

    #[test]
    fn test_hypervisor_metrics_vm_lifecycle() {
        let metrics = HypervisorMetrics::new();

        metrics.record_vm_created(4);
        metrics.record_vm_created(2);

        assert_eq!(metrics.active_vms(), 2);
        assert_eq!(metrics.total_vms_created(), 2);
        assert_eq!(metrics.total_vcpus(), 6);

        metrics.record_vm_destroyed(4);

        assert_eq!(metrics.active_vms(), 1);
        assert_eq!(metrics.total_vcpus(), 2);
    }

    #[test]
    fn test_hypervisor_metrics_overcommit() {
        let metrics = HypervisorMetrics::new();

        metrics.set_memory_overcommit(1.5);
        metrics.set_cpu_overcommit(2.0);

        assert!((metrics.memory_overcommit() - 1.5).abs() < 0.01);
        assert!((metrics.cpu_overcommit() - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_vm_metrics_basic() {
        let metrics = VmMetrics::new("test-vm");

        assert_eq!(metrics.name, "test-vm");
        assert!(metrics.vcpus().is_empty());
    }

    #[test]
    fn test_vm_metrics_vcpus() {
        let metrics = VmMetrics::new("test-vm");

        let vcpu0 = metrics.add_vcpu(0);
        let vcpu1 = metrics.add_vcpu(1);

        vcpu0.record_io_exit();
        vcpu1.record_mmio_exit();

        assert_eq!(metrics.vcpus().len(), 2);
        assert_eq!(metrics.get_vcpu(0).unwrap().io_exits(), 1);
        assert_eq!(metrics.get_vcpu(1).unwrap().mmio_exits(), 1);
    }

    #[test]
    fn test_vm_metrics_disks() {
        let metrics = VmMetrics::new("test-vm");

        let vda = metrics.add_disk("vda");
        let vdb = metrics.add_disk("vdb");

        vda.record_read(4096, 100);
        vdb.record_write(8192, 200);

        assert_eq!(metrics.disks().len(), 2);
        assert_eq!(metrics.get_disk("vda").unwrap().read_ops(), 1);
        assert_eq!(metrics.get_disk("vdb").unwrap().write_ops(), 1);
    }

    #[test]
    fn test_vm_metrics_networks() {
        let metrics = VmMetrics::new("test-vm");

        let eth0 = metrics.add_network("eth0");
        eth0.record_rx(1500);
        eth0.record_tx(500);

        assert_eq!(metrics.networks().len(), 1);
        assert_eq!(metrics.get_network("eth0").unwrap().total_bytes(), 2000);
    }

    #[test]
    fn test_vm_metrics_boot_time() {
        let metrics = VmMetrics::new("test-vm");

        assert!(metrics.boot_time().is_none());

        std::thread::sleep(Duration::from_millis(10));
        metrics.record_boot_complete();

        let boot_time = metrics.boot_time().unwrap();
        assert!(boot_time.as_millis() >= 10);
    }

    #[test]
    fn test_vm_metrics_snapshot_migration() {
        let metrics = VmMetrics::new("test-vm");

        metrics.record_snapshot();
        metrics.record_snapshot();
        metrics.record_migration();

        assert_eq!(metrics.snapshot_count(), 2);
        assert_eq!(metrics.migration_count(), 1);
    }

    #[test]
    fn test_vm_metrics_totals() {
        let metrics = VmMetrics::new("test-vm");

        let vda = metrics.add_disk("vda");
        let vdb = metrics.add_disk("vdb");
        vda.record_read(4096, 100);
        vdb.record_read(4096, 100);

        let eth0 = metrics.add_network("eth0");
        eth0.record_rx(1000);
        eth0.record_tx(500);

        assert_eq!(metrics.total_disk_iops(), 2);
        assert_eq!(metrics.total_network_throughput(), 1500);
    }

    #[test]
    fn test_vm_metrics_age() {
        let metrics = VmMetrics::new("test-vm");

        std::thread::sleep(Duration::from_millis(10));
        let age = metrics.age();

        assert!(age.as_millis() >= 10);
    }
}
