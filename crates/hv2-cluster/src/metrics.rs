//! Prometheus text-format metrics, without a metrics library.
//!
//! A handful of counters and one histogram per process is all either daemon
//! exports, and the exposition format is plain text; a dependency for that
//! would be larger than the code it saves.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// A monotonically increasing count.
#[derive(Debug, Default)]
pub struct Counter(AtomicU64);

impl Counter {
    pub fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    #[must_use]
    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

/// Upper bounds, in seconds, of the latency buckets. From a template restore
/// (~0.01 s) to a cold boot under load (seconds).
pub const LATENCY_BUCKETS: [f64; 10] = [0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0];

/// A latency histogram with [`LATENCY_BUCKETS`].
#[derive(Debug, Default)]
pub struct Histogram {
    buckets: [AtomicU64; LATENCY_BUCKETS.len()],
    count: AtomicU64,
    /// In microseconds, so it can be atomic.
    sum_us: AtomicU64,
}

impl Histogram {
    pub fn observe(&self, elapsed: Duration) {
        let secs = elapsed.as_secs_f64();
        for (bucket, bound) in self.buckets.iter().zip(LATENCY_BUCKETS) {
            if secs <= bound {
                bucket.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum_us
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);
    }
}

/// Builds one exposition document.
#[derive(Default)]
pub struct Exposition(String);

impl Exposition {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn header(&mut self, name: &str, help: &str, kind: &str) {
        let _ = writeln!(self.0, "# HELP {name} {help}");
        let _ = writeln!(self.0, "# TYPE {name} {kind}");
    }

    pub fn gauge(&mut self, name: &str, help: &str, value: f64) {
        self.header(name, help, "gauge");
        let _ = writeln!(self.0, "{name} {value}");
    }

    /// A counter with one label, one line per value.
    pub fn counters(&mut self, name: &str, help: &str, label: &str, values: &[(&str, u64)]) {
        self.header(name, help, "counter");
        for (value, count) in values {
            let _ = writeln!(self.0, "{name}{{{label}=\"{value}\"}} {count}");
        }
    }

    pub fn histogram(&mut self, name: &str, help: &str, h: &Histogram) {
        self.header(name, help, "histogram");
        // Buckets are cumulative, as the format requires: `observe` adds to
        // every bucket whose bound the value is within.
        for (bucket, bound) in h.buckets.iter().zip(LATENCY_BUCKETS) {
            let _ = writeln!(
                self.0,
                "{name}_bucket{{le=\"{bound}\"}} {}",
                bucket.load(Ordering::Relaxed)
            );
        }
        let count = h.count.load(Ordering::Relaxed);
        let _ = writeln!(self.0, "{name}_bucket{{le=\"+Inf\"}} {count}");
        let _ = writeln!(
            self.0,
            "{name}_sum {}",
            h.sum_us.load(Ordering::Relaxed) as f64 / 1e6
        );
        let _ = writeln!(self.0, "{name}_count {count}");
    }

    #[must_use]
    pub fn finish(self) -> String {
        self.0
    }
}

/// The content type Prometheus expects for this format.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_histogram_is_cumulative_and_carries_sum_and_count() {
        let h = Histogram::default();
        h.observe(Duration::from_millis(8));
        h.observe(Duration::from_millis(40));
        h.observe(Duration::from_secs(9));
        let mut e = Exposition::new();
        e.histogram("t", "test", &h);
        let text = e.finish();
        assert!(text.contains("t_bucket{le=\"0.005\"} 0"));
        assert!(text.contains("t_bucket{le=\"0.01\"} 1"));
        assert!(text.contains("t_bucket{le=\"0.05\"} 2"));
        assert!(text.contains("t_bucket{le=\"5\"} 2"));
        assert!(text.contains("t_bucket{le=\"+Inf\"} 3"));
        assert!(text.contains("t_count 3"));
        assert!(text.contains("t_sum 9.048"));
    }

    #[test]
    fn counters_render_one_line_per_label_value() {
        let mut e = Exposition::new();
        e.counters("c", "test", "result", &[("ok", 3), ("full", 1)]);
        let text = e.finish();
        assert!(text.contains("# TYPE c counter"));
        assert!(text.contains("c{result=\"ok\"} 3"));
        assert!(text.contains("c{result=\"full\"} 1"));
    }
}
