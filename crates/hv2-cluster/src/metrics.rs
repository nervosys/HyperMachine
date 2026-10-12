//! Prometheus text-format metrics, without a metrics library.
//!
//! A handful of counters and one histogram per process is all either daemon
//! exports, and the exposition format is plain text; a dependency for that
//! would be larger than the code it saves.
//!
//! The same document is also OTLP: [`Exposition::otlp`] gives what it holds
//! in OpenTelemetry's JSON encoding, for [`crate::otlp`] to push to a
//! collector. One set of numbers, two ways out.

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

/// One metric of an exposition, as it was given: what its OTLP form is
/// made from.
enum Family {
    Gauge {
        name: String,
        help: String,
        value: f64,
    },
    Counters {
        name: String,
        help: String,
        label: String,
        values: Vec<(String, u64)>,
    },
    Histogram {
        name: String,
        help: String,
        /// Cumulative, one per bound of [`LATENCY_BUCKETS`], as read.
        cumulative: Vec<u64>,
        count: u64,
        /// In seconds.
        sum: f64,
    },
}

/// Builds one exposition document.
#[derive(Default)]
pub struct Exposition {
    text: String,
    families: Vec<Family>,
}

impl Exposition {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn header(&mut self, name: &str, help: &str, kind: &str) {
        let _ = writeln!(self.text, "# HELP {name} {help}");
        let _ = writeln!(self.text, "# TYPE {name} {kind}");
    }

    pub fn gauge(&mut self, name: &str, help: &str, value: f64) {
        self.header(name, help, "gauge");
        let _ = writeln!(self.text, "{name} {value}");
        self.families.push(Family::Gauge {
            name: name.to_string(),
            help: help.to_string(),
            value,
        });
    }

    /// A counter with one label, one line per value.
    pub fn counters(&mut self, name: &str, help: &str, label: &str, values: &[(&str, u64)]) {
        self.header(name, help, "counter");
        for (value, count) in values {
            let _ = writeln!(self.text, "{name}{{{label}=\"{value}\"}} {count}");
        }
        self.families.push(Family::Counters {
            name: name.to_string(),
            help: help.to_string(),
            label: label.to_string(),
            values: values
                .iter()
                .map(|(value, count)| ((*value).to_string(), *count))
                .collect(),
        });
    }

    pub fn histogram(&mut self, name: &str, help: &str, h: &Histogram) {
        self.header(name, help, "histogram");
        // Read once, so the text and the OTLP form are the same numbers.
        let cumulative: Vec<u64> = h
            .buckets
            .iter()
            .map(|bucket| bucket.load(Ordering::Relaxed))
            .collect();
        let count = h.count.load(Ordering::Relaxed);
        let sum = h.sum_us.load(Ordering::Relaxed) as f64 / 1e6;
        // Buckets are cumulative, as the format requires: `observe` adds to
        // every bucket whose bound the value is within.
        for (bucket, bound) in cumulative.iter().zip(LATENCY_BUCKETS) {
            let _ = writeln!(self.text, "{name}_bucket{{le=\"{bound}\"}} {bucket}");
        }
        let _ = writeln!(self.text, "{name}_bucket{{le=\"+Inf\"}} {count}");
        let _ = writeln!(self.text, "{name}_sum {sum}");
        let _ = writeln!(self.text, "{name}_count {count}");
        self.families.push(Family::Histogram {
            name: name.to_string(),
            help: help.to_string(),
            cumulative,
            count,
            sum,
        });
    }

    /// The Prometheus text.
    #[must_use]
    pub fn finish(self) -> String {
        self.text
    }

    /// The same metrics as an OTLP `ExportMetricsServiceRequest`, in its
    /// JSON encoding.
    ///
    /// - `resource` is the resource's attributes: `service.name` and
    ///   whatever else identifies the process.
    /// - Counters and histograms are cumulative from `start_ns`, when the
    ///   process started counting; every point is at `now_ns`.
    /// - A counter is named without Prometheus's `_total`, as OpenTelemetry
    ///   names them: a collector writing Prometheus adds it back.
    /// - A histogram's buckets are counted each on its own, not
    ///   cumulatively, with one more than there are bounds, as OTLP
    ///   requires; and its unit is seconds.
    /// - 64-bit integers are strings, as protobuf's JSON mapping has them.
    #[must_use]
    pub fn otlp(&self, resource: &[(&str, &str)], start_ns: u64, now_ns: u64) -> serde_json::Value {
        use serde_json::json;
        let (start, now) = (start_ns.to_string(), now_ns.to_string());
        let metrics: Vec<serde_json::Value> = self
            .families
            .iter()
            .map(|family| match family {
                Family::Gauge { name, help, value } => json!({
                    "name": name,
                    "description": help,
                    "gauge": {"dataPoints": [{"timeUnixNano": now, "asDouble": value}]},
                }),
                Family::Counters {
                    name,
                    help,
                    label,
                    values,
                } => json!({
                    "name": name.strip_suffix("_total").unwrap_or(name),
                    "description": help,
                    "sum": {
                        "aggregationTemporality": 2,
                        "isMonotonic": true,
                        "dataPoints": values.iter().map(|(value, count)| json!({
                            "attributes": [{"key": label, "value": {"stringValue": value}}],
                            "startTimeUnixNano": start,
                            "timeUnixNano": now,
                            "asInt": count.to_string(),
                        })).collect::<Vec<_>>(),
                    },
                }),
                Family::Histogram {
                    name,
                    help,
                    cumulative,
                    count,
                    sum,
                } => {
                    // Each bucket's own count: what is under its bound and
                    // not under the one before; the last is above them all.
                    let mut below = 0;
                    let mut each: Vec<String> = cumulative
                        .iter()
                        .map(|under| {
                            let own = under.saturating_sub(below);
                            below = *under;
                            own.to_string()
                        })
                        .collect();
                    each.push(count.saturating_sub(below).to_string());
                    json!({
                        "name": name.strip_suffix("_seconds").unwrap_or(name),
                        "description": help,
                        "unit": "s",
                        "histogram": {
                            "aggregationTemporality": 2,
                            "dataPoints": [{
                                "startTimeUnixNano": start,
                                "timeUnixNano": now,
                                "count": count.to_string(),
                                "sum": sum,
                                "bucketCounts": each,
                                "explicitBounds": LATENCY_BUCKETS,
                            }],
                        },
                    })
                }
            })
            .collect();
        json!({
            "resourceMetrics": [{
                "resource": {
                    "attributes": resource.iter().map(|(key, value)| json!({
                        "key": key, "value": {"stringValue": value},
                    })).collect::<Vec<_>>(),
                },
                "scopeMetrics": [{
                    "scope": {"name": "hypermachine", "version": env!("CARGO_PKG_VERSION")},
                    "metrics": metrics,
                }],
            }],
        })
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
    fn the_otlp_form_is_the_same_numbers_in_otlp_s_terms() {
        let h = Histogram::default();
        h.observe(Duration::from_millis(8));
        h.observe(Duration::from_millis(40));
        h.observe(Duration::from_secs(9));
        let mut e = Exposition::new();
        e.gauge("g_running", "running", 3.0);
        e.counters("c_total", "made", "result", &[("ok", 7), ("full", 1)]);
        e.histogram("h_seconds", "took", &h);
        let otlp = e.otlp(&[("service.name", "node"), ("host.name", "n1")], 100, 200);
        let resource = &otlp["resourceMetrics"][0];
        assert_eq!(
            resource["resource"]["attributes"][0],
            serde_json::json!({"key": "service.name", "value": {"stringValue": "node"}})
        );
        let metrics = &resource["scopeMetrics"][0]["metrics"];
        assert_eq!(metrics.as_array().unwrap().len(), 3);

        let gauge = &metrics[0];
        assert_eq!(gauge["name"], "g_running");
        assert_eq!(gauge["gauge"]["dataPoints"][0]["asDouble"], 3.0);
        assert_eq!(gauge["gauge"]["dataPoints"][0]["timeUnixNano"], "200");

        // `_total` is Prometheus's; the points are cumulative from the start.
        let counter = &metrics[1];
        assert_eq!(counter["name"], "c");
        assert_eq!(counter["sum"]["isMonotonic"], true);
        assert_eq!(counter["sum"]["aggregationTemporality"], 2);
        let points = counter["sum"]["dataPoints"].as_array().unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(points[0]["asInt"], "7");
        assert_eq!(points[0]["startTimeUnixNano"], "100");
        assert_eq!(
            points[0]["attributes"][0],
            serde_json::json!({"key": "result", "value": {"stringValue": "ok"}})
        );

        // Buckets each on their own, one more than the bounds, adding up to
        // the count.
        let histogram = &metrics[2];
        assert_eq!(histogram["name"], "h");
        assert_eq!(histogram["unit"], "s");
        let point = &histogram["histogram"]["dataPoints"][0];
        let each: Vec<u64> = point["bucketCounts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap().parse().unwrap())
            .collect();
        assert_eq!(each, [0, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1]);
        assert_eq!(each.len(), LATENCY_BUCKETS.len() + 1);
        assert_eq!(
            point["explicitBounds"].as_array().unwrap().len(),
            LATENCY_BUCKETS.len()
        );
        assert_eq!(point["count"], "3");
        assert!((point["sum"].as_f64().unwrap() - 9.048).abs() < 1e-9);

        // The text is unchanged by it.
        let text = e.finish();
        assert!(text.contains("c_total{result=\"ok\"} 7"));
        assert!(text.contains("h_seconds_bucket{le=\"+Inf\"} 3"));
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
