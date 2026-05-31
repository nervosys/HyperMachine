//! Resource limits and enforcement for AI agent operations
//!
//! This module provides configurable resource quotas and real-time enforcement
//! to prevent runaway agents from consuming excessive system resources.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Resource limit result
pub type LimitResult<T> = Result<T, LimitError>;

/// Resource limit errors
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LimitError {
    /// Memory limit exceeded
    #[error("Memory limit exceeded: requested {requested} bytes, limit {limit} bytes")]
    MemoryExceeded { limit: u64, requested: u64 },
    /// CPU time limit exceeded
    #[error("CPU time limit exceeded: used {used:?}, limit {limit:?}")]
    CpuTimeExceeded { limit: Duration, used: Duration },
    /// Execution time limit exceeded
    #[error("Execution time limit exceeded: elapsed {elapsed:?}, limit {limit:?}")]
    ExecutionTimeExceeded { limit: Duration, elapsed: Duration },
    /// Operation count exceeded
    #[error("Operation count limit exceeded: count {count}, limit {limit}")]
    OperationCountExceeded { limit: u64, count: u64 },
    /// Rate limit exceeded
    #[error("Rate limit exceeded: {current} ops in {window:?} (limit: {limit} ops)")]
    RateLimitExceeded {
        limit: u64,
        current: u64,
        window: Duration,
    },
    /// Concurrent operation limit exceeded
    #[error("Concurrency limit exceeded: {current} concurrent (limit: {limit})")]
    ConcurrencyExceeded { limit: u32, current: u32 },
    /// IO operations limit exceeded
    #[error("IO limit exceeded: {used} bytes used, limit {limit} bytes")]
    IoLimitExceeded { limit: u64, used: u64 },
    /// Network bandwidth limit exceeded
    #[error("Network limit exceeded: {used} bytes used, limit {limit} bytes")]
    NetworkLimitExceeded { limit: u64, used: u64 },
    /// Custom limit exceeded
    #[error("Custom limit exceeded: {0}")]
    Custom(String),
}

/// Resource limits configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory: u64,
    /// Maximum CPU time
    pub max_cpu_time: Duration,
    /// Maximum wall-clock execution time
    pub max_execution_time: Duration,
    /// Maximum operation count
    pub max_operations: u64,
    /// Maximum concurrent operations
    pub max_concurrency: u32,
    /// Maximum IO bytes per second
    pub max_io_bytes_per_sec: u64,
    /// Maximum network bytes per second
    pub max_network_bytes_per_sec: u64,
    /// Rate limit: operations per window
    pub rate_limit_ops: u64,
    /// Rate limit: time window
    pub rate_limit_window: Duration,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 512 * 1024 * 1024,                // 512 MB
            max_cpu_time: Duration::from_secs(60),        // 1 minute CPU time
            max_execution_time: Duration::from_secs(300), // 5 minutes wall-clock
            max_operations: 1_000_000,                    // 1M operations
            max_concurrency: 16,                          // 16 concurrent ops
            max_io_bytes_per_sec: 100 * 1024 * 1024,      // 100 MB/s
            max_network_bytes_per_sec: 10 * 1024 * 1024,  // 10 MB/s
            rate_limit_ops: 1000,                         // 1000 ops
            rate_limit_window: Duration::from_secs(1),    // per second
        }
    }
}

impl ResourceLimits {
    /// Create new resource limits
    pub fn new() -> Self {
        Self::default()
    }

    /// Create minimal limits for sandboxed execution
    pub fn minimal() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024, // 64 MB
            max_cpu_time: Duration::from_secs(5),
            max_execution_time: Duration::from_secs(30),
            max_operations: 100_000,
            max_concurrency: 4,
            max_io_bytes_per_sec: 10 * 1024 * 1024,
            max_network_bytes_per_sec: 1024 * 1024,
            rate_limit_ops: 100,
            rate_limit_window: Duration::from_secs(1),
        }
    }

    /// Create generous limits for trusted agents
    pub fn generous() -> Self {
        Self {
            max_memory: 4 * 1024 * 1024 * 1024,             // 4 GB
            max_cpu_time: Duration::from_secs(3600),        // 1 hour
            max_execution_time: Duration::from_secs(86400), // 24 hours
            max_operations: 1_000_000_000,                  // 1B operations
            max_concurrency: 64,
            max_io_bytes_per_sec: 1024 * 1024 * 1024, // 1 GB/s
            max_network_bytes_per_sec: 100 * 1024 * 1024, // 100 MB/s
            rate_limit_ops: 10000,
            rate_limit_window: Duration::from_secs(1),
        }
    }

    /// Set memory limit
    pub fn with_memory(mut self, bytes: u64) -> Self {
        self.max_memory = bytes;
        self
    }

    /// Set CPU time limit
    pub fn with_cpu_time(mut self, duration: Duration) -> Self {
        self.max_cpu_time = duration;
        self
    }

    /// Set execution time limit
    pub fn with_execution_time(mut self, duration: Duration) -> Self {
        self.max_execution_time = duration;
        self
    }

    /// Set operation count limit
    pub fn with_max_operations(mut self, ops: u64) -> Self {
        self.max_operations = ops;
        self
    }

    /// Set concurrency limit
    pub fn with_concurrency(mut self, max: u32) -> Self {
        self.max_concurrency = max;
        self
    }

    /// Set rate limit
    pub fn with_rate_limit(mut self, ops: u64, window: Duration) -> Self {
        self.rate_limit_ops = ops;
        self.rate_limit_window = window;
        self
    }
}

/// Resource usage tracking
#[derive(Debug, Default)]
pub struct ResourceUsage {
    /// Memory currently allocated
    pub memory_allocated: AtomicU64,
    /// Peak memory usage
    pub memory_peak: AtomicU64,
    /// Total CPU time used (microseconds)
    pub cpu_time_us: AtomicU64,
    /// Total operations performed
    pub operation_count: AtomicU64,
    /// Current concurrent operations
    pub current_concurrency: AtomicU64,
    /// Total IO bytes
    pub io_bytes: AtomicU64,
    /// Total network bytes
    pub network_bytes: AtomicU64,
}

impl ResourceUsage {
    /// Create new usage tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Get current memory usage
    pub fn memory(&self) -> u64 {
        self.memory_allocated.load(Ordering::Relaxed)
    }

    /// Get peak memory usage
    pub fn peak_memory(&self) -> u64 {
        self.memory_peak.load(Ordering::Relaxed)
    }

    /// Get CPU time as duration
    pub fn cpu_time(&self) -> Duration {
        Duration::from_micros(self.cpu_time_us.load(Ordering::Relaxed))
    }

    /// Get operation count
    pub fn operations(&self) -> u64 {
        self.operation_count.load(Ordering::Relaxed)
    }

    /// Get current concurrency
    pub fn concurrency(&self) -> u32 {
        self.current_concurrency.load(Ordering::Relaxed) as u32
    }

    /// Reset all usage counters
    pub fn reset(&self) {
        self.memory_allocated.store(0, Ordering::Relaxed);
        self.memory_peak.store(0, Ordering::Relaxed);
        self.cpu_time_us.store(0, Ordering::Relaxed);
        self.operation_count.store(0, Ordering::Relaxed);
        self.current_concurrency.store(0, Ordering::Relaxed);
        self.io_bytes.store(0, Ordering::Relaxed);
        self.network_bytes.store(0, Ordering::Relaxed);
    }
}

/// Rate limiter using sliding window
#[derive(Debug)]
pub struct RateLimiter {
    /// Maximum operations per window
    limit: u64,
    /// Window duration
    window: Duration,
    /// Operation timestamps
    timestamps: RwLock<Vec<Instant>>,
}

impl RateLimiter {
    /// Create a new rate limiter
    pub fn new(limit: u64, window: Duration) -> Self {
        Self {
            limit,
            window,
            timestamps: RwLock::new(Vec::new()),
        }
    }

    /// Try to acquire a permit
    pub fn try_acquire(&self) -> LimitResult<()> {
        let now = Instant::now();
        let mut timestamps = self.timestamps.write().unwrap_or_else(|e| e.into_inner());

        // Remove expired timestamps
        let cutoff = now - self.window;
        timestamps.retain(|t| *t > cutoff);

        // Check limit
        if timestamps.len() as u64 >= self.limit {
            return Err(LimitError::RateLimitExceeded {
                limit: self.limit,
                current: timestamps.len() as u64,
                window: self.window,
            });
        }

        // Record this operation
        timestamps.push(now);
        Ok(())
    }

    /// Get current count within window
    pub fn current_count(&self) -> u64 {
        let now = Instant::now();
        let timestamps = self.timestamps.read().unwrap_or_else(|e| e.into_inner());
        let cutoff = now - self.window;
        timestamps.iter().filter(|t| **t > cutoff).count() as u64
    }

    /// Reset the rate limiter
    pub fn reset(&self) {
        self.timestamps
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Get the limit
    pub fn limit(&self) -> u64 {
        self.limit
    }

    /// Get the window duration
    pub fn window(&self) -> Duration {
        self.window
    }
}

/// Concurrency limiter using semaphore-like semantics
#[derive(Debug)]
pub struct ConcurrencyLimiter {
    /// Maximum concurrent operations
    limit: u32,
    /// Current count
    current: AtomicU64,
}

impl ConcurrencyLimiter {
    /// Create a new concurrency limiter
    pub fn new(limit: u32) -> Self {
        Self {
            limit,
            current: AtomicU64::new(0),
        }
    }

    /// Try to acquire a permit
    pub fn try_acquire(&self) -> LimitResult<ConcurrencyGuard<'_>> {
        loop {
            let current = self.current.load(Ordering::Relaxed);
            if current >= self.limit as u64 {
                return Err(LimitError::ConcurrencyExceeded {
                    limit: self.limit,
                    current: current as u32,
                });
            }

            if self
                .current
                .compare_exchange(current, current + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return Ok(ConcurrencyGuard {
                    counter: &self.current,
                });
            }
        }
    }

    /// Get current concurrency
    pub fn current(&self) -> u32 {
        self.current.load(Ordering::Relaxed) as u32
    }

    /// Get the limit
    pub fn limit(&self) -> u32 {
        self.limit
    }

    /// Check if any permits available
    pub fn available(&self) -> u32 {
        let current = self.current.load(Ordering::Relaxed) as u32;
        self.limit.saturating_sub(current)
    }
}

/// Guard that releases concurrency permit on drop
#[derive(Debug)]
pub struct ConcurrencyGuard<'a> {
    counter: &'a AtomicU64,
}

impl<'a> Drop for ConcurrencyGuard<'a> {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Token bucket rate limiter for bandwidth limiting
#[derive(Debug)]
pub struct TokenBucket {
    /// Bucket capacity (max tokens)
    capacity: u64,
    /// Current tokens
    tokens: AtomicU64,
    /// Tokens added per second
    rate: u64,
    /// Last refill time
    last_refill: RwLock<Instant>,
}

impl TokenBucket {
    /// Create a new token bucket
    pub fn new(capacity: u64, rate: u64) -> Self {
        Self {
            capacity,
            tokens: AtomicU64::new(capacity),
            rate,
            last_refill: RwLock::new(Instant::now()),
        }
    }

    /// Try to consume tokens
    pub fn try_consume(&self, tokens: u64) -> bool {
        self.refill();

        loop {
            let current = self.tokens.load(Ordering::Relaxed);
            if current < tokens {
                return false;
            }

            if self
                .tokens
                .compare_exchange(
                    current,
                    current - tokens,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&self) {
        let now = Instant::now();
        let mut last_refill = self.last_refill.write().unwrap_or_else(|e| e.into_inner());
        let elapsed = now.duration_since(*last_refill);

        let new_tokens = (elapsed.as_secs_f64() * self.rate as f64) as u64;
        if new_tokens > 0 {
            loop {
                let current = self.tokens.load(Ordering::Relaxed);
                let new_count = (current + new_tokens).min(self.capacity);

                if self
                    .tokens
                    .compare_exchange(current, new_count, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
                {
                    break;
                }
            }
            *last_refill = now;
        }
    }

    /// Get current token count
    pub fn tokens(&self) -> u64 {
        self.tokens.load(Ordering::Relaxed)
    }

    /// Get capacity
    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    /// Get fill rate
    pub fn rate(&self) -> u64 {
        self.rate
    }
}

/// Resource enforcer that combines all limit types
#[derive(Debug)]
pub struct ResourceEnforcer {
    /// Resource limits configuration
    limits: ResourceLimits,
    /// Current usage tracking
    usage: Arc<ResourceUsage>,
    /// Execution start time
    start_time: Instant,
    /// Rate limiter
    rate_limiter: RateLimiter,
    /// Concurrency limiter
    concurrency_limiter: ConcurrencyLimiter,
    /// IO bandwidth limiter
    io_bucket: TokenBucket,
    /// Network bandwidth limiter
    network_bucket: TokenBucket,
}

impl ResourceEnforcer {
    /// Create a new resource enforcer
    pub fn new(limits: ResourceLimits) -> Self {
        let rate_limiter = RateLimiter::new(limits.rate_limit_ops, limits.rate_limit_window);
        let concurrency_limiter = ConcurrencyLimiter::new(limits.max_concurrency);
        let io_bucket = TokenBucket::new(limits.max_io_bytes_per_sec, limits.max_io_bytes_per_sec);
        let network_bucket = TokenBucket::new(
            limits.max_network_bytes_per_sec,
            limits.max_network_bytes_per_sec,
        );

        Self {
            limits,
            usage: Arc::new(ResourceUsage::new()),
            start_time: Instant::now(),
            rate_limiter,
            concurrency_limiter,
            io_bucket,
            network_bucket,
        }
    }

    /// Get the limits
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Get usage tracking
    pub fn usage(&self) -> Arc<ResourceUsage> {
        Arc::clone(&self.usage)
    }

    /// Check if execution time limit exceeded
    pub fn check_execution_time(&self) -> LimitResult<()> {
        let elapsed = self.start_time.elapsed();
        if elapsed > self.limits.max_execution_time {
            return Err(LimitError::ExecutionTimeExceeded {
                limit: self.limits.max_execution_time,
                elapsed,
            });
        }
        Ok(())
    }

    /// Check if CPU time limit exceeded
    pub fn check_cpu_time(&self) -> LimitResult<()> {
        let used = self.usage.cpu_time();
        if used > self.limits.max_cpu_time {
            return Err(LimitError::CpuTimeExceeded {
                limit: self.limits.max_cpu_time,
                used,
            });
        }
        Ok(())
    }

    /// Check if operation count limit exceeded
    pub fn check_operation_count(&self) -> LimitResult<()> {
        let count = self.usage.operations();
        if count >= self.limits.max_operations {
            return Err(LimitError::OperationCountExceeded {
                limit: self.limits.max_operations,
                count,
            });
        }
        Ok(())
    }

    /// Try to allocate memory
    pub fn try_allocate_memory(&self, bytes: u64) -> LimitResult<()> {
        let current = self.usage.memory_allocated.load(Ordering::Relaxed);
        let new_total = current + bytes;

        if new_total > self.limits.max_memory {
            return Err(LimitError::MemoryExceeded {
                limit: self.limits.max_memory,
                requested: bytes,
            });
        }

        self.usage
            .memory_allocated
            .fetch_add(bytes, Ordering::Relaxed);

        // Update peak
        loop {
            let peak = self.usage.memory_peak.load(Ordering::Relaxed);
            if new_total <= peak {
                break;
            }
            if self
                .usage
                .memory_peak
                .compare_exchange(peak, new_total, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        Ok(())
    }

    /// Free memory
    pub fn free_memory(&self, bytes: u64) {
        self.usage
            .memory_allocated
            .fetch_sub(bytes.min(self.usage.memory()), Ordering::Relaxed);
    }

    /// Record CPU time
    pub fn record_cpu_time(&self, duration: Duration) {
        self.usage
            .cpu_time_us
            .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
    }

    /// Try to perform an operation
    pub fn try_operation(&self) -> LimitResult<()> {
        // Check all relevant limits
        self.check_execution_time()?;
        self.check_cpu_time()?;
        self.check_operation_count()?;
        self.rate_limiter.try_acquire()?;

        // Increment operation count
        self.usage.operation_count.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }

    /// Try to acquire concurrency permit
    pub fn try_concurrent(&self) -> LimitResult<ConcurrencyGuard<'_>> {
        self.concurrency_limiter.try_acquire()
    }

    /// Try to perform IO
    pub fn try_io(&self, bytes: u64) -> LimitResult<()> {
        if !self.io_bucket.try_consume(bytes) {
            let used = self.usage.io_bytes.load(Ordering::Relaxed);
            return Err(LimitError::IoLimitExceeded {
                limit: self.limits.max_io_bytes_per_sec,
                used,
            });
        }
        self.usage.io_bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(())
    }

    /// Try to perform network IO
    pub fn try_network(&self, bytes: u64) -> LimitResult<()> {
        if !self.network_bucket.try_consume(bytes) {
            let used = self.usage.network_bytes.load(Ordering::Relaxed);
            return Err(LimitError::NetworkLimitExceeded {
                limit: self.limits.max_network_bytes_per_sec,
                used,
            });
        }
        self.usage.network_bytes.fetch_add(bytes, Ordering::Relaxed);
        Ok(())
    }

    /// Get elapsed execution time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get remaining execution time
    pub fn remaining_time(&self) -> Duration {
        self.limits
            .max_execution_time
            .saturating_sub(self.elapsed())
    }

    /// Reset all usage and start time
    pub fn reset(&mut self) {
        self.usage.reset();
        self.start_time = Instant::now();
        self.rate_limiter.reset();
    }

    /// Get a summary of resource usage
    pub fn summary(&self) -> ResourceSummary {
        ResourceSummary {
            memory_used: self.usage.memory(),
            memory_peak: self.usage.peak_memory(),
            memory_limit: self.limits.max_memory,
            cpu_time: self.usage.cpu_time(),
            cpu_time_limit: self.limits.max_cpu_time,
            execution_time: self.elapsed(),
            execution_time_limit: self.limits.max_execution_time,
            operations: self.usage.operations(),
            operations_limit: self.limits.max_operations,
            concurrency: self.usage.concurrency(),
            concurrency_limit: self.limits.max_concurrency,
            io_bytes: self.usage.io_bytes.load(Ordering::Relaxed),
            network_bytes: self.usage.network_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Summary of resource usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSummary {
    pub memory_used: u64,
    pub memory_peak: u64,
    pub memory_limit: u64,
    pub cpu_time: Duration,
    pub cpu_time_limit: Duration,
    pub execution_time: Duration,
    pub execution_time_limit: Duration,
    pub operations: u64,
    pub operations_limit: u64,
    pub concurrency: u32,
    pub concurrency_limit: u32,
    pub io_bytes: u64,
    pub network_bytes: u64,
}

impl ResourceSummary {
    /// Calculate memory utilization as percentage
    pub fn memory_utilization(&self) -> f64 {
        if self.memory_limit == 0 {
            0.0
        } else {
            self.memory_used as f64 / self.memory_limit as f64 * 100.0
        }
    }

    /// Calculate CPU time utilization as percentage
    pub fn cpu_utilization(&self) -> f64 {
        if self.cpu_time_limit.is_zero() {
            0.0
        } else {
            self.cpu_time.as_secs_f64() / self.cpu_time_limit.as_secs_f64() * 100.0
        }
    }

    /// Calculate operation utilization as percentage
    pub fn operation_utilization(&self) -> f64 {
        if self.operations_limit == 0 {
            0.0
        } else {
            self.operations as f64 / self.operations_limit as f64 * 100.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limit_error_display() {
        let err = LimitError::MemoryExceeded {
            limit: 1024,
            requested: 2048,
        };
        assert!(format!("{}", err).contains("Memory limit"));
    }

    #[test]
    fn test_resource_limits_default() {
        let limits = ResourceLimits::default();
        assert_eq!(limits.max_memory, 512 * 1024 * 1024);
        assert_eq!(limits.max_concurrency, 16);
    }

    #[test]
    fn test_resource_limits_minimal() {
        let limits = ResourceLimits::minimal();
        assert_eq!(limits.max_memory, 64 * 1024 * 1024);
        assert_eq!(limits.max_concurrency, 4);
    }

    #[test]
    fn test_resource_limits_generous() {
        let limits = ResourceLimits::generous();
        assert!(limits.max_memory > ResourceLimits::default().max_memory);
    }

    #[test]
    fn test_resource_limits_builder() {
        let limits = ResourceLimits::new()
            .with_memory(1024 * 1024)
            .with_cpu_time(Duration::from_secs(10))
            .with_max_operations(1000)
            .with_concurrency(8);

        assert_eq!(limits.max_memory, 1024 * 1024);
        assert_eq!(limits.max_cpu_time, Duration::from_secs(10));
        assert_eq!(limits.max_operations, 1000);
        assert_eq!(limits.max_concurrency, 8);
    }

    #[test]
    fn test_resource_usage() {
        let usage = ResourceUsage::new();
        assert_eq!(usage.memory(), 0);
        assert_eq!(usage.operations(), 0);

        usage.memory_allocated.store(1024, Ordering::Relaxed);
        usage.operation_count.store(100, Ordering::Relaxed);

        assert_eq!(usage.memory(), 1024);
        assert_eq!(usage.operations(), 100);
    }

    #[test]
    fn test_resource_usage_reset() {
        let usage = ResourceUsage::new();
        usage.memory_allocated.store(1024, Ordering::Relaxed);
        usage.operation_count.store(100, Ordering::Relaxed);

        usage.reset();

        assert_eq!(usage.memory(), 0);
        assert_eq!(usage.operations(), 0);
    }

    #[test]
    fn test_rate_limiter() {
        let limiter = RateLimiter::new(5, Duration::from_secs(1));

        for _ in 0..5 {
            assert!(limiter.try_acquire().is_ok());
        }

        assert!(matches!(
            limiter.try_acquire(),
            Err(LimitError::RateLimitExceeded { .. })
        ));
    }

    #[test]
    fn test_rate_limiter_count() {
        let limiter = RateLimiter::new(10, Duration::from_secs(1));
        limiter.try_acquire().unwrap();
        limiter.try_acquire().unwrap();

        assert_eq!(limiter.current_count(), 2);
    }

    #[test]
    fn test_rate_limiter_reset() {
        let limiter = RateLimiter::new(2, Duration::from_secs(1));
        limiter.try_acquire().unwrap();
        limiter.try_acquire().unwrap();

        limiter.reset();
        assert_eq!(limiter.current_count(), 0);
    }

    #[test]
    fn test_concurrency_limiter() {
        let limiter = ConcurrencyLimiter::new(2);

        let _g1 = limiter.try_acquire().unwrap();
        let _g2 = limiter.try_acquire().unwrap();

        assert!(matches!(
            limiter.try_acquire(),
            Err(LimitError::ConcurrencyExceeded { .. })
        ));
    }

    #[test]
    fn test_concurrency_guard_drop() {
        let limiter = ConcurrencyLimiter::new(1);

        {
            let _g = limiter.try_acquire().unwrap();
            assert_eq!(limiter.current(), 1);
        }

        assert_eq!(limiter.current(), 0);
        assert!(limiter.try_acquire().is_ok());
    }

    #[test]
    fn test_concurrency_available() {
        let limiter = ConcurrencyLimiter::new(3);
        assert_eq!(limiter.available(), 3);

        let _g = limiter.try_acquire().unwrap();
        assert_eq!(limiter.available(), 2);
    }

    #[test]
    fn test_token_bucket() {
        let bucket = TokenBucket::new(100, 100);
        assert!(bucket.try_consume(50));
        assert!(bucket.try_consume(50));
        assert!(!bucket.try_consume(1)); // Empty
    }

    #[test]
    fn test_token_bucket_properties() {
        let bucket = TokenBucket::new(100, 50);
        assert_eq!(bucket.capacity(), 100);
        assert_eq!(bucket.rate(), 50);
        assert_eq!(bucket.tokens(), 100);
    }

    #[test]
    fn test_resource_enforcer_memory() {
        let limits = ResourceLimits::new().with_memory(1024);
        let enforcer = ResourceEnforcer::new(limits);

        assert!(enforcer.try_allocate_memory(512).is_ok());
        assert!(enforcer.try_allocate_memory(512).is_ok());
        assert!(matches!(
            enforcer.try_allocate_memory(1),
            Err(LimitError::MemoryExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_enforcer_free_memory() {
        let limits = ResourceLimits::new().with_memory(1024);
        let enforcer = ResourceEnforcer::new(limits);

        enforcer.try_allocate_memory(1024).unwrap();
        enforcer.free_memory(512);

        assert_eq!(enforcer.usage().memory(), 512);
        assert!(enforcer.try_allocate_memory(512).is_ok());
    }

    #[test]
    fn test_resource_enforcer_cpu_time() {
        let limits = ResourceLimits::new().with_cpu_time(Duration::from_secs(1));
        let enforcer = ResourceEnforcer::new(limits);

        enforcer.record_cpu_time(Duration::from_millis(500));
        assert!(enforcer.check_cpu_time().is_ok());

        enforcer.record_cpu_time(Duration::from_millis(600));
        assert!(matches!(
            enforcer.check_cpu_time(),
            Err(LimitError::CpuTimeExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_enforcer_operations() {
        let limits = ResourceLimits::new()
            .with_max_operations(5)
            .with_rate_limit(100, Duration::from_secs(1));
        let enforcer = ResourceEnforcer::new(limits);

        for _ in 0..5 {
            assert!(enforcer.try_operation().is_ok());
        }

        assert!(matches!(
            enforcer.try_operation(),
            Err(LimitError::OperationCountExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_enforcer_concurrency() {
        let limits = ResourceLimits::new().with_concurrency(2);
        let enforcer = ResourceEnforcer::new(limits);

        let _g1 = enforcer.try_concurrent().unwrap();
        let _g2 = enforcer.try_concurrent().unwrap();

        assert!(matches!(
            enforcer.try_concurrent(),
            Err(LimitError::ConcurrencyExceeded { .. })
        ));
    }

    #[test]
    fn test_resource_enforcer_summary() {
        let limits = ResourceLimits::new();
        let enforcer = ResourceEnforcer::new(limits);

        enforcer.try_allocate_memory(1024).unwrap();
        let summary = enforcer.summary();

        assert_eq!(summary.memory_used, 1024);
        assert!(summary.memory_utilization() < 1.0);
    }

    #[test]
    fn test_resource_summary_utilization() {
        let summary = ResourceSummary {
            memory_used: 512,
            memory_peak: 512,
            memory_limit: 1024,
            cpu_time: Duration::from_secs(30),
            cpu_time_limit: Duration::from_secs(60),
            execution_time: Duration::from_secs(10),
            execution_time_limit: Duration::from_secs(300),
            operations: 500,
            operations_limit: 1000,
            concurrency: 4,
            concurrency_limit: 8,
            io_bytes: 0,
            network_bytes: 0,
        };

        assert!((summary.memory_utilization() - 50.0).abs() < 0.01);
        assert!((summary.cpu_utilization() - 50.0).abs() < 0.01);
        assert!((summary.operation_utilization() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_resource_enforcer_elapsed() {
        let enforcer = ResourceEnforcer::new(ResourceLimits::default());
        std::thread::sleep(Duration::from_millis(10));
        assert!(enforcer.elapsed() >= Duration::from_millis(10));
    }

    #[test]
    fn test_resource_enforcer_remaining_time() {
        let limits = ResourceLimits::new().with_execution_time(Duration::from_secs(60));
        let enforcer = ResourceEnforcer::new(limits);

        let remaining = enforcer.remaining_time();
        assert!(remaining <= Duration::from_secs(60));
        assert!(remaining > Duration::from_secs(59));
    }

    #[test]
    fn test_resource_enforcer_reset() {
        let limits = ResourceLimits::new();
        let mut enforcer = ResourceEnforcer::new(limits);

        enforcer.try_allocate_memory(1024).unwrap();
        enforcer.record_cpu_time(Duration::from_secs(1));

        enforcer.reset();

        assert_eq!(enforcer.usage().memory(), 0);
        assert_eq!(enforcer.usage().cpu_time(), Duration::ZERO);
    }

    #[test]
    fn test_limit_error_variants() {
        let errors = vec![
            LimitError::MemoryExceeded {
                limit: 100,
                requested: 200,
            },
            LimitError::CpuTimeExceeded {
                limit: Duration::from_secs(1),
                used: Duration::from_secs(2),
            },
            LimitError::ExecutionTimeExceeded {
                limit: Duration::from_secs(1),
                elapsed: Duration::from_secs(2),
            },
            LimitError::OperationCountExceeded {
                limit: 100,
                count: 200,
            },
            LimitError::RateLimitExceeded {
                limit: 10,
                current: 20,
                window: Duration::from_secs(1),
            },
            LimitError::ConcurrencyExceeded {
                limit: 4,
                current: 8,
            },
            LimitError::IoLimitExceeded {
                limit: 1024,
                used: 2048,
            },
            LimitError::NetworkLimitExceeded {
                limit: 1024,
                used: 2048,
            },
            LimitError::Custom("test".to_string()),
        ];

        for err in errors {
            assert!(!format!("{}", err).is_empty());
        }
    }

    #[test]
    fn test_peak_memory_tracking() {
        let limits = ResourceLimits::new().with_memory(4096);
        let enforcer = ResourceEnforcer::new(limits);

        enforcer.try_allocate_memory(1024).unwrap();
        assert_eq!(enforcer.usage().peak_memory(), 1024);

        enforcer.try_allocate_memory(1024).unwrap();
        assert_eq!(enforcer.usage().peak_memory(), 2048);

        enforcer.free_memory(1024);
        // Peak should remain at 2048
        assert_eq!(enforcer.usage().peak_memory(), 2048);
    }
}
