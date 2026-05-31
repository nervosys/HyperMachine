//! Performance Optimization Infrastructure
//!
//! This module provides performance monitoring and optimization features
//! for the hypervisor, including:
//!
//! - Interrupt coalescing to reduce VM exits
//! - I/O batching for port and MMIO operations
//! - Exit statistics and optimization hints
//! - Performance counters and timing infrastructure
//!
//! # Interrupt Coalescing
//!
//! Interrupt coalescing delays interrupt injection to batch multiple
//! interrupts together, reducing the number of VM exits. This is
//! particularly effective for high-frequency devices like network
//! cards and disk controllers.
//!
//! # I/O Batching
//!
//! I/O batching groups multiple I/O operations together to reduce
//! the overhead of individual VM exits for each operation.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

/// Default coalescing window in microseconds
pub const DEFAULT_COALESCE_WINDOW_US: u64 = 100;

/// Default maximum interrupts to coalesce
pub const DEFAULT_MAX_COALESCED: usize = 16;

/// Interrupt coalescing configuration
#[derive(Debug, Clone)]
pub struct CoalesceConfig {
    /// Enable interrupt coalescing
    pub enabled: bool,
    /// Maximum time window for coalescing (microseconds)
    pub window_us: u64,
    /// Maximum number of interrupts to coalesce
    pub max_coalesced: usize,
    /// Per-IRQ coalescing enable (IRQ 0-15)
    pub irq_enabled: [bool; 16],
}

impl Default for CoalesceConfig {
    fn default() -> Self {
        let mut irq_enabled = [true; 16];
        // Disable coalescing for keyboard (immediate response needed)
        irq_enabled[1] = false;

        Self {
            enabled: true,
            window_us: DEFAULT_COALESCE_WINDOW_US,
            max_coalesced: DEFAULT_MAX_COALESCED,
            irq_enabled,
        }
    }
}

impl CoalesceConfig {
    /// Create a configuration with coalescing disabled
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// Create a configuration optimized for low latency
    pub fn low_latency() -> Self {
        Self {
            enabled: true,
            window_us: 50,
            max_coalesced: 4,
            irq_enabled: [false; 16], // Only coalesce specific IRQs
        }
    }

    /// Create a configuration optimized for throughput
    pub fn high_throughput() -> Self {
        Self {
            enabled: true,
            window_us: 500,
            max_coalesced: 64,
            irq_enabled: [true; 16],
        }
    }

    /// Enable coalescing for a specific IRQ
    pub fn enable_irq(&mut self, irq: u8) -> &mut Self {
        if irq < 16 {
            self.irq_enabled[irq as usize] = true;
        }
        self
    }

    /// Disable coalescing for a specific IRQ
    pub fn disable_irq(&mut self, irq: u8) -> &mut Self {
        if irq < 16 {
            self.irq_enabled[irq as usize] = false;
        }
        self
    }
}

/// A coalesced interrupt entry
#[derive(Debug, Clone, Copy)]
pub struct CoalescedInterrupt {
    /// IRQ number
    pub irq: u8,
    /// Interrupt vector
    pub vector: u8,
    /// Timestamp when interrupt was raised
    pub timestamp: Instant,
    /// Number of coalesced instances
    pub count: u32,
}

/// Interrupt coalescer for reducing VM exits
#[derive(Debug)]
pub struct InterruptCoalescer {
    /// Configuration
    config: CoalesceConfig,
    /// Pending coalesced interrupts per IRQ
    pending: [Option<CoalescedInterrupt>; 16],
    /// Window start time
    window_start: Instant,
    /// Total coalesced count
    total_coalesced: u64,
    /// Total delivered count
    total_delivered: u64,
}

impl InterruptCoalescer {
    /// Create a new interrupt coalescer
    pub fn new(config: CoalesceConfig) -> Self {
        Self {
            config,
            pending: [None; 16],
            window_start: Instant::now(),
            total_coalesced: 0,
            total_delivered: 0,
        }
    }

    /// Add an interrupt to the coalescer
    ///
    /// Returns `Some(vector)` if the interrupt should be delivered immediately,
    /// `None` if it was coalesced.
    pub fn add_interrupt(&mut self, irq: u8, vector: u8) -> Option<u8> {
        if irq >= 16 {
            return Some(vector);
        }

        // Check if coalescing is enabled for this IRQ
        if !self.config.enabled || !self.config.irq_enabled[irq as usize] {
            self.total_delivered += 1;
            return Some(vector);
        }

        let now = Instant::now();

        // Check if we need to flush due to window expiry
        if now.duration_since(self.window_start).as_micros() as u64 > self.config.window_us {
            self.flush_window();
            self.window_start = now;
        }

        // Coalesce with existing pending interrupt
        if let Some(ref mut pending) = self.pending[irq as usize] {
            pending.count += 1;
            self.total_coalesced += 1;

            // Check if we've hit the coalesce limit
            if pending.count as usize >= self.config.max_coalesced {
                let result = pending.vector;
                self.pending[irq as usize] = None;
                self.total_delivered += 1;
                return Some(result);
            }

            return None;
        }

        // First interrupt in this window for this IRQ
        self.pending[irq as usize] = Some(CoalescedInterrupt {
            irq,
            vector,
            timestamp: now,
            count: 1,
        });

        None
    }

    /// Flush all pending interrupts
    ///
    /// Returns an iterator over vectors to deliver
    pub fn flush(&mut self) -> Vec<u8> {
        let mut result = Vec::new();

        for i in 0..16 {
            if let Some(pending) = self.pending[i].take() {
                result.push(pending.vector);
                self.total_delivered += 1;
            }
        }

        self.window_start = Instant::now();
        result
    }

    /// Flush the current window and start a new one
    fn flush_window(&mut self) {
        for i in 0..16 {
            if self.pending[i].is_some() {
                self.pending[i] = None;
            }
        }
    }

    /// Get pending interrupt count
    pub fn pending_count(&self) -> usize {
        self.pending.iter().filter(|p| p.is_some()).count()
    }

    /// Get coalescing statistics
    pub fn stats(&self) -> CoalesceStats {
        CoalesceStats {
            total_coalesced: self.total_coalesced,
            total_delivered: self.total_delivered,
            current_pending: self.pending_count() as u64,
            coalesce_ratio: if self.total_delivered > 0 {
                self.total_coalesced as f64 / self.total_delivered as f64
            } else {
                0.0
            },
        }
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.total_coalesced = 0;
        self.total_delivered = 0;
    }
}

/// Interrupt coalescing statistics
#[derive(Debug, Clone, Copy)]
pub struct CoalesceStats {
    /// Total number of coalesced interrupts
    pub total_coalesced: u64,
    /// Total number of delivered interrupts
    pub total_delivered: u64,
    /// Current pending count
    pub current_pending: u64,
    /// Coalesce ratio (coalesced / delivered)
    pub coalesce_ratio: f64,
}

/// I/O operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoOpType {
    /// Port I/O read
    PortIn,
    /// Port I/O write
    PortOut,
    /// MMIO read
    MmioRead,
    /// MMIO write
    MmioWrite,
}

/// A batched I/O operation
#[derive(Debug, Clone)]
pub struct BatchedIoOp {
    /// Operation type
    pub op_type: IoOpType,
    /// Port or address
    pub address: u64,
    /// Data size in bytes
    pub size: u8,
    /// Data (for writes) or buffer offset (for reads)
    pub data: u64,
    /// Timestamp
    pub timestamp: Instant,
}

/// I/O batch configuration
#[derive(Debug, Clone)]
pub struct IoBatchConfig {
    /// Enable I/O batching
    pub enabled: bool,
    /// Maximum batch size
    pub max_batch_size: usize,
    /// Maximum batch latency in microseconds
    pub max_latency_us: u64,
    /// Enable port I/O batching
    pub batch_port_io: bool,
    /// Enable MMIO batching
    pub batch_mmio: bool,
}

impl Default for IoBatchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_batch_size: 32,
            max_latency_us: 100,
            batch_port_io: true,
            batch_mmio: true,
        }
    }
}

/// I/O batcher for reducing VM exits
#[derive(Debug)]
pub struct IoBatcher {
    /// Configuration
    config: IoBatchConfig,
    /// Pending operations
    pending: VecDeque<BatchedIoOp>,
    /// Batch start time
    batch_start: Instant,
    /// Total batched operations
    total_batched: u64,
    /// Total immediate operations
    total_immediate: u64,
}

impl IoBatcher {
    /// Create a new I/O batcher
    pub fn new(config: IoBatchConfig) -> Self {
        Self {
            config,
            pending: VecDeque::with_capacity(32),
            batch_start: Instant::now(),
            total_batched: 0,
            total_immediate: 0,
        }
    }

    /// Add a port I/O operation
    ///
    /// Returns `true` if the batch should be flushed
    pub fn add_port_io(&mut self, is_write: bool, port: u16, size: u8, data: u64) -> bool {
        if !self.config.enabled || !self.config.batch_port_io {
            self.total_immediate += 1;
            return true;
        }

        let op = BatchedIoOp {
            op_type: if is_write {
                IoOpType::PortOut
            } else {
                IoOpType::PortIn
            },
            address: port as u64,
            size,
            data,
            timestamp: Instant::now(),
        };

        self.add_op(op)
    }

    /// Add an MMIO operation
    ///
    /// Returns `true` if the batch should be flushed
    pub fn add_mmio(&mut self, is_write: bool, address: u64, size: u8, data: u64) -> bool {
        if !self.config.enabled || !self.config.batch_mmio {
            self.total_immediate += 1;
            return true;
        }

        let op = BatchedIoOp {
            op_type: if is_write {
                IoOpType::MmioWrite
            } else {
                IoOpType::MmioRead
            },
            address,
            size,
            data,
            timestamp: Instant::now(),
        };

        self.add_op(op)
    }

    fn add_op(&mut self, op: BatchedIoOp) -> bool {
        let now = Instant::now();

        // Check if we need to flush due to latency
        if !self.pending.is_empty() {
            let elapsed = now.duration_since(self.batch_start).as_micros() as u64;
            if elapsed > self.config.max_latency_us {
                return true;
            }
        } else {
            self.batch_start = now;
        }

        self.pending.push_back(op);
        self.total_batched += 1;

        // Check if we've hit the batch size limit
        self.pending.len() >= self.config.max_batch_size
    }

    /// Flush the batch and return pending operations
    pub fn flush(&mut self) -> Vec<BatchedIoOp> {
        let result: Vec<_> = self.pending.drain(..).collect();
        self.batch_start = Instant::now();
        result
    }

    /// Get pending operation count
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get batching statistics
    pub fn stats(&self) -> IoBatchStats {
        IoBatchStats {
            total_batched: self.total_batched,
            total_immediate: self.total_immediate,
            current_pending: self.pending.len() as u64,
            batch_ratio: if self.total_immediate > 0 {
                self.total_batched as f64 / self.total_immediate as f64
            } else if self.total_batched > 0 {
                f64::INFINITY
            } else {
                0.0
            },
        }
    }
}

/// I/O batching statistics
#[derive(Debug, Clone, Copy)]
pub struct IoBatchStats {
    /// Total number of batched operations
    pub total_batched: u64,
    /// Total number of immediate operations
    pub total_immediate: u64,
    /// Current pending count
    pub current_pending: u64,
    /// Batch ratio
    pub batch_ratio: f64,
}

/// VM exit type for statistics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExitType {
    /// I/O port access
    IoPort,
    /// MMIO access
    Mmio,
    /// HLT instruction
    Hlt,
    /// CPUID instruction
    Cpuid,
    /// MSR access
    Msr,
    /// Interrupt window
    InterruptWindow,
    /// External interrupt
    ExternalInterrupt,
    /// Exception
    Exception,
    /// EPT violation
    EptViolation,
    /// Preemption timer
    PreemptionTimer,
    /// Other/unknown
    Other,
}

impl ExitType {
    /// Get all exit types
    pub const fn all() -> [ExitType; 11] {
        [
            ExitType::IoPort,
            ExitType::Mmio,
            ExitType::Hlt,
            ExitType::Cpuid,
            ExitType::Msr,
            ExitType::InterruptWindow,
            ExitType::ExternalInterrupt,
            ExitType::Exception,
            ExitType::EptViolation,
            ExitType::PreemptionTimer,
            ExitType::Other,
        ]
    }
}

/// Exit statistics tracker
#[derive(Debug)]
pub struct ExitStats {
    /// Per-exit-type counters
    counts: [AtomicU64; 11],
    /// Per-exit-type total time (nanoseconds)
    times_ns: [AtomicU64; 11],
    /// Total exits
    total_exits: AtomicU64,
    /// Start time for rate calculation
    start_time: Instant,
}

impl ExitStats {
    /// Create a new exit statistics tracker
    pub fn new() -> Self {
        Self {
            counts: Default::default(),
            times_ns: Default::default(),
            total_exits: AtomicU64::new(0),
            start_time: Instant::now(),
        }
    }

    /// Record an exit
    pub fn record_exit(&self, exit_type: ExitType, duration_ns: u64) {
        let idx = exit_type as usize;
        self.counts[idx].fetch_add(1, Ordering::Relaxed);
        self.times_ns[idx].fetch_add(duration_ns, Ordering::Relaxed);
        self.total_exits.fetch_add(1, Ordering::Relaxed);
    }

    /// Get count for a specific exit type
    pub fn get_count(&self, exit_type: ExitType) -> u64 {
        self.counts[exit_type as usize].load(Ordering::Relaxed)
    }

    /// Get total time for a specific exit type
    pub fn get_time_ns(&self, exit_type: ExitType) -> u64 {
        self.times_ns[exit_type as usize].load(Ordering::Relaxed)
    }

    /// Get average time for a specific exit type
    pub fn get_avg_time_ns(&self, exit_type: ExitType) -> u64 {
        let count = self.get_count(exit_type);
        self.get_time_ns(exit_type).checked_div(count).unwrap_or(0)
    }

    /// Get total exit count
    pub fn total_exits(&self) -> u64 {
        self.total_exits.load(Ordering::Relaxed)
    }

    /// Get exits per second
    pub fn exits_per_second(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.total_exits() as f64 / elapsed
        } else {
            0.0
        }
    }

    /// Get a summary of all exit statistics
    pub fn summary(&self) -> ExitStatsSummary {
        let mut by_type = Vec::new();

        for exit_type in ExitType::all() {
            let count = self.get_count(exit_type);
            if count > 0 {
                by_type.push(ExitTypeStat {
                    exit_type,
                    count,
                    total_time_ns: self.get_time_ns(exit_type),
                    avg_time_ns: self.get_avg_time_ns(exit_type),
                });
            }
        }

        // Sort by count descending
        by_type.sort_by_key(|e| std::cmp::Reverse(e.count));

        ExitStatsSummary {
            total_exits: self.total_exits(),
            elapsed_secs: self.start_time.elapsed().as_secs_f64(),
            exits_per_second: self.exits_per_second(),
            by_type,
        }
    }

    /// Reset all statistics
    pub fn reset(&self) {
        for i in 0..11 {
            self.counts[i].store(0, Ordering::Relaxed);
            self.times_ns[i].store(0, Ordering::Relaxed);
        }
        self.total_exits.store(0, Ordering::Relaxed);
    }
}

impl Default for ExitStats {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics for a single exit type
#[derive(Debug, Clone)]
pub struct ExitTypeStat {
    /// Exit type
    pub exit_type: ExitType,
    /// Total count
    pub count: u64,
    /// Total time in nanoseconds
    pub total_time_ns: u64,
    /// Average time in nanoseconds
    pub avg_time_ns: u64,
}

/// Summary of exit statistics
#[derive(Debug, Clone)]
pub struct ExitStatsSummary {
    /// Total number of exits
    pub total_exits: u64,
    /// Elapsed time in seconds
    pub elapsed_secs: f64,
    /// Exits per second
    pub exits_per_second: f64,
    /// Per-type statistics (sorted by count)
    pub by_type: Vec<ExitTypeStat>,
}

/// Performance counter
#[derive(Debug)]
pub struct PerfCounter {
    name: &'static str,
    count: AtomicU64,
    total_ns: AtomicU64,
    min_ns: AtomicU64,
    max_ns: AtomicU64,
}

impl PerfCounter {
    /// Create a new performance counter
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            count: AtomicU64::new(0),
            total_ns: AtomicU64::new(0),
            min_ns: AtomicU64::new(u64::MAX),
            max_ns: AtomicU64::new(0),
        }
    }

    /// Record a measurement
    pub fn record(&self, duration_ns: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.total_ns.fetch_add(duration_ns, Ordering::Relaxed);

        // Update min (using compare-exchange loop)
        let mut current_min = self.min_ns.load(Ordering::Relaxed);
        while duration_ns < current_min {
            match self.min_ns.compare_exchange_weak(
                current_min,
                duration_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_min = x,
            }
        }

        // Update max
        let mut current_max = self.max_ns.load(Ordering::Relaxed);
        while duration_ns > current_max {
            match self.max_ns.compare_exchange_weak(
                current_max,
                duration_ns,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(x) => current_max = x,
            }
        }
    }

    /// Get counter name
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// Get count
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    /// Get total time in nanoseconds
    pub fn total_ns(&self) -> u64 {
        self.total_ns.load(Ordering::Relaxed)
    }

    /// Get average time in nanoseconds
    pub fn avg_ns(&self) -> u64 {
        let count = self.count();
        self.total_ns().checked_div(count).unwrap_or(0)
    }

    /// Get minimum time in nanoseconds
    pub fn min_ns(&self) -> u64 {
        let min = self.min_ns.load(Ordering::Relaxed);
        if min == u64::MAX {
            0
        } else {
            min
        }
    }

    /// Get maximum time in nanoseconds
    pub fn max_ns(&self) -> u64 {
        self.max_ns.load(Ordering::Relaxed)
    }

    /// Reset the counter
    pub fn reset(&self) {
        self.count.store(0, Ordering::Relaxed);
        self.total_ns.store(0, Ordering::Relaxed);
        self.min_ns.store(u64::MAX, Ordering::Relaxed);
        self.max_ns.store(0, Ordering::Relaxed);
    }

    /// Get a summary
    pub fn summary(&self) -> PerfCounterSummary {
        PerfCounterSummary {
            name: self.name,
            count: self.count(),
            total_ns: self.total_ns(),
            avg_ns: self.avg_ns(),
            min_ns: self.min_ns(),
            max_ns: self.max_ns(),
        }
    }
}

/// Performance counter summary
#[derive(Debug, Clone)]
pub struct PerfCounterSummary {
    /// Counter name
    pub name: &'static str,
    /// Total count
    pub count: u64,
    /// Total time in nanoseconds
    pub total_ns: u64,
    /// Average time in nanoseconds
    pub avg_ns: u64,
    /// Minimum time in nanoseconds
    pub min_ns: u64,
    /// Maximum time in nanoseconds
    pub max_ns: u64,
}

/// Timer guard for automatic duration measurement
pub struct TimerGuard<'a> {
    counter: &'a PerfCounter,
    start: Instant,
}

impl<'a> TimerGuard<'a> {
    /// Create a new timer guard
    pub fn new(counter: &'a PerfCounter) -> Self {
        Self {
            counter,
            start: Instant::now(),
        }
    }
}

impl<'a> Drop for TimerGuard<'a> {
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_nanos() as u64;
        self.counter.record(elapsed);
    }
}

/// Macro for timing a block of code
#[macro_export]
macro_rules! time_block {
    ($counter:expr, $block:block) => {{
        let _guard = $crate::perf::TimerGuard::new($counter);
        $block
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_coalesce_config_default() {
        let config = CoalesceConfig::default();
        assert!(config.enabled);
        assert_eq!(config.window_us, DEFAULT_COALESCE_WINDOW_US);
        assert!(!config.irq_enabled[1]); // Keyboard disabled
        assert!(config.irq_enabled[0]); // Timer enabled
    }

    #[test]
    fn test_coalesce_config_low_latency() {
        let config = CoalesceConfig::low_latency();
        assert!(config.enabled);
        assert_eq!(config.window_us, 50);
        assert_eq!(config.max_coalesced, 4);
    }

    #[test]
    fn test_interrupt_coalescer_disabled() {
        let config = CoalesceConfig::disabled();
        let mut coalescer = InterruptCoalescer::new(config);

        // Should return immediately when disabled
        assert_eq!(coalescer.add_interrupt(0, 0x20), Some(0x20));
        assert_eq!(coalescer.add_interrupt(0, 0x20), Some(0x20));
    }

    #[test]
    fn test_interrupt_coalescer_coalesce() {
        let mut config = CoalesceConfig::default();
        config.enabled = true;
        config.irq_enabled[0] = true;
        config.max_coalesced = 4;

        let mut coalescer = InterruptCoalescer::new(config);

        // First interrupt starts coalescing
        assert_eq!(coalescer.add_interrupt(0, 0x20), None);
        assert_eq!(coalescer.pending_count(), 1);

        // Second and third are coalesced
        assert_eq!(coalescer.add_interrupt(0, 0x20), None);
        assert_eq!(coalescer.add_interrupt(0, 0x20), None);

        // Fourth hits the limit and delivers
        assert_eq!(coalescer.add_interrupt(0, 0x20), Some(0x20));
        assert_eq!(coalescer.pending_count(), 0);
    }

    #[test]
    fn test_interrupt_coalescer_flush() {
        let mut config = CoalesceConfig::default();
        config.irq_enabled[0] = true;
        config.irq_enabled[4] = true;

        let mut coalescer = InterruptCoalescer::new(config);

        coalescer.add_interrupt(0, 0x20);
        coalescer.add_interrupt(4, 0x24);

        let flushed = coalescer.flush();
        assert_eq!(flushed.len(), 2);
        assert!(flushed.contains(&0x20));
        assert!(flushed.contains(&0x24));
    }

    #[test]
    fn test_interrupt_coalescer_stats() {
        let mut config = CoalesceConfig::default();
        config.irq_enabled[0] = true;
        config.max_coalesced = 4;

        let mut coalescer = InterruptCoalescer::new(config);

        // Add some interrupts
        coalescer.add_interrupt(0, 0x20);
        coalescer.add_interrupt(0, 0x20);
        coalescer.add_interrupt(0, 0x20);
        coalescer.add_interrupt(0, 0x20); // Delivers

        let stats = coalescer.stats();
        assert_eq!(stats.total_delivered, 1);
        assert_eq!(stats.total_coalesced, 3);
    }

    #[test]
    fn test_io_batcher_disabled() {
        let mut config = IoBatchConfig::default();
        config.enabled = false;

        let mut batcher = IoBatcher::new(config);

        // Should return true (flush) when disabled
        assert!(batcher.add_port_io(true, 0x60, 1, 0x42));
    }

    #[test]
    fn test_io_batcher_batch() {
        let mut config = IoBatchConfig::default();
        config.max_batch_size = 4;

        let mut batcher = IoBatcher::new(config);

        assert!(!batcher.add_port_io(true, 0x60, 1, 0x42));
        assert!(!batcher.add_port_io(true, 0x61, 1, 0x43));
        assert!(!batcher.add_port_io(true, 0x62, 1, 0x44));
        assert!(batcher.add_port_io(true, 0x63, 1, 0x45)); // Hits limit

        assert_eq!(batcher.pending_count(), 4);
    }

    #[test]
    fn test_io_batcher_flush() {
        let config = IoBatchConfig::default();
        let mut batcher = IoBatcher::new(config);

        batcher.add_port_io(true, 0x60, 1, 0x42);
        batcher.add_mmio(false, 0xFEE00000, 4, 0);

        let ops = batcher.flush();
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[0].op_type, IoOpType::PortOut);
        assert_eq!(ops[1].op_type, IoOpType::MmioRead);
    }

    #[test]
    fn test_exit_stats_record() {
        let stats = ExitStats::new();

        stats.record_exit(ExitType::IoPort, 1000);
        stats.record_exit(ExitType::IoPort, 2000);
        stats.record_exit(ExitType::Hlt, 500);

        assert_eq!(stats.get_count(ExitType::IoPort), 2);
        assert_eq!(stats.get_count(ExitType::Hlt), 1);
        assert_eq!(stats.get_time_ns(ExitType::IoPort), 3000);
        assert_eq!(stats.get_avg_time_ns(ExitType::IoPort), 1500);
        assert_eq!(stats.total_exits(), 3);
    }

    #[test]
    fn test_exit_stats_summary() {
        let stats = ExitStats::new();

        stats.record_exit(ExitType::IoPort, 1000);
        stats.record_exit(ExitType::IoPort, 2000);
        stats.record_exit(ExitType::Mmio, 500);

        let summary = stats.summary();
        assert_eq!(summary.total_exits, 3);
        assert!(!summary.by_type.is_empty());

        // Should be sorted by count descending
        assert_eq!(summary.by_type[0].exit_type, ExitType::IoPort);
    }

    #[test]
    fn test_perf_counter() {
        let counter = PerfCounter::new("test");

        counter.record(100);
        counter.record(200);
        counter.record(50);

        assert_eq!(counter.count(), 3);
        assert_eq!(counter.total_ns(), 350);
        assert_eq!(counter.avg_ns(), 116); // 350 / 3
        assert_eq!(counter.min_ns(), 50);
        assert_eq!(counter.max_ns(), 200);
    }

    #[test]
    fn test_perf_counter_reset() {
        let counter = PerfCounter::new("test");

        counter.record(100);
        counter.reset();

        assert_eq!(counter.count(), 0);
        assert_eq!(counter.total_ns(), 0);
        assert_eq!(counter.min_ns(), 0);
        assert_eq!(counter.max_ns(), 0);
    }

    #[test]
    fn test_timer_guard() {
        let counter = PerfCounter::new("test");

        {
            let _guard = TimerGuard::new(&counter);
            thread::sleep(Duration::from_micros(100));
        }

        assert_eq!(counter.count(), 1);
        // Time should be at least 100us = 100000ns
        assert!(counter.total_ns() >= 50000); // Allow some slack
    }

    #[test]
    fn test_exit_type_all() {
        let all = ExitType::all();
        assert_eq!(all.len(), 11);
    }

    #[test]
    fn test_coalesce_config_enable_disable_irq() {
        let mut config = CoalesceConfig::default();

        config.disable_irq(0);
        assert!(!config.irq_enabled[0]);

        config.enable_irq(0);
        assert!(config.irq_enabled[0]);
    }
}
