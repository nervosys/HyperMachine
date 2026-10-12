//! Metrics pushed to an OpenTelemetry collector, over OTLP/HTTP with JSON.
//!
//! A daemon's `/metrics` is for a scraper that can reach it. This is the
//! other way round: every interval the daemon sends the same numbers, as
//! [`crate::metrics::Exposition::otlp`] encodes them, to an endpoint its
//! operator names. Nothing else is sent: no logs, no traces.
//!
//! # Configuration
//!
//! A flag where one is given, and otherwise OpenTelemetry's own variables:
//!
//! | | Flag | Variable |
//! |---|---|---|
//! | Where | `--otlp-endpoint URL` | `OTEL_EXPORTER_OTLP_METRICS_ENDPOINT`, used as it is; or `OTEL_EXPORTER_OTLP_ENDPOINT`, with `/v1/metrics` added |
//! | Headers | `--otlp-header KEY=VALUE`, repeated | `OTEL_EXPORTER_OTLP_HEADERS`, `KEY=VALUE` separated by commas, values percent-encoded |
//! | How often | `--otlp-interval SECONDS` | `OTEL_METRIC_EXPORT_INTERVAL`, in milliseconds |
//! | Service | | `OTEL_SERVICE_NAME` |
//!
//! A flag's endpoint is the whole URL, as the metrics variable's is. Only
//! `http/json` is spoken: a `OTEL_EXPORTER_OTLP_PROTOCOL` of anything else is
//! refused when the daemon starts, rather than sending JSON to a port that
//! expects gRPC.
//!
//! Headers commonly carry a collector's key, so they are never logged.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::metrics::Exposition;

/// Where and how a daemon pushes its metrics.
#[derive(Clone, PartialEq, Eq)]
pub struct Config {
    /// The URL metrics are POSTed to, `/v1/metrics` and all.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub interval: Duration,
    pub service_name: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Header values are secrets as often as not; their names are not.
        f.debug_struct("Config")
            .field("url", &self.url)
            .field(
                "headers",
                &self.headers.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            )
            .field("interval", &self.interval)
            .field("service_name", &self.service_name)
            .finish()
    }
}

/// The least interval: a collector is not to be flooded by a typo.
pub const MIN_INTERVAL: Duration = Duration::from_secs(1);
/// What the interval is unless said: OpenTelemetry's own default is 60 s,
/// and a fleet's dashboards want fresher than that.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(15);

/// What a daemon's flags said, before the environment is consulted.
#[derive(Debug, Default, Clone)]
pub struct Flags {
    pub endpoint: Option<String>,
    pub headers: Vec<String>,
    pub interval_secs: Option<u64>,
}

/// One `KEY=VALUE` header.
fn header(text: &str) -> Result<(String, String), String> {
    let (key, value) = text
        .split_once('=')
        .ok_or_else(|| "a header is KEY=VALUE".to_string())?;
    let key = key.trim();
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b));
    if !valid {
        return Err(format!("{key:?} is not a header name"));
    }
    let value = value.trim();
    if value.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(format!("the value of header {key} has a line break in it"));
    }
    Ok((key.to_string(), value.to_string()))
}

/// `%XX` decoded, as `OTEL_EXPORTER_OTLP_HEADERS` values are.
fn percent_decoded(text: &str) -> Result<String, String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
                .ok_or_else(|| format!("{text:?}: a % is not followed by two hex digits"))?;
            out.push(hex);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| format!("{text:?} is not UTF-8 once decoded"))
}

/// Whether `url` is one this pushes to: HTTP or HTTPS, with a host.
fn check_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("{url}: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("{url}: OTLP/HTTP is http or https"));
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return Err(format!("{url}: no host"));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(format!(
            "{url}: put credentials in a header, not in the URL, which is logged"
        ));
    }
    Ok(())
}

/// The configuration `flags` and `env` give, or `None` when neither names an
/// endpoint. `env` is a variable's value by name, so a test can give its own.
///
/// # Errors
///
/// What is wrong with what was given, for the daemon to refuse to start
/// with: an endpoint that is not HTTP, a header that is not one, a protocol
/// other than JSON, an interval under [`MIN_INTERVAL`].
pub fn configure(
    flags: &Flags,
    env: impl Fn(&str) -> Option<String>,
    default_service: &str,
) -> Result<Option<Config>, String> {
    let set = |name: &str| env(name).filter(|v| !v.trim().is_empty());
    let url = match &flags.endpoint {
        Some(url) => url.trim().to_string(),
        None => match set("OTEL_EXPORTER_OTLP_METRICS_ENDPOINT") {
            Some(url) => url.trim().to_string(),
            None => match set("OTEL_EXPORTER_OTLP_ENDPOINT") {
                Some(base) => format!("{}/v1/metrics", base.trim().trim_end_matches('/')),
                None => return Ok(None),
            },
        },
    };
    check_url(&url)?;
    for name in [
        "OTEL_EXPORTER_OTLP_METRICS_PROTOCOL",
        "OTEL_EXPORTER_OTLP_PROTOCOL",
    ] {
        if let Some(protocol) = set(name) {
            if protocol.trim() != "http/json" {
                return Err(format!(
                    "{name}={protocol}: metrics are pushed as http/json only"
                ));
            }
            break;
        }
    }
    let headers = if flags.headers.is_empty() {
        match set("OTEL_EXPORTER_OTLP_METRICS_HEADERS")
            .or_else(|| set("OTEL_EXPORTER_OTLP_HEADERS"))
        {
            Some(list) => list
                .split(',')
                .filter(|part| !part.trim().is_empty())
                .map(|part| {
                    let (key, value) = header(part)?;
                    Ok((key, percent_decoded(&value)?))
                })
                .collect::<Result<Vec<_>, String>>()
                .map_err(|e| format!("OTEL_EXPORTER_OTLP_HEADERS: {e}"))?,
            None => Vec::new(),
        }
    } else {
        flags
            .headers
            .iter()
            .map(|text| header(text).map_err(|e| format!("--otlp-header: {e}")))
            .collect::<Result<Vec<_>, String>>()?
    };
    let interval = match flags.interval_secs {
        Some(secs) => Duration::from_secs(secs),
        None => match set("OTEL_METRIC_EXPORT_INTERVAL") {
            Some(ms) => Duration::from_millis(
                ms.trim()
                    .parse()
                    .map_err(|_| format!("OTEL_METRIC_EXPORT_INTERVAL={ms}: milliseconds"))?,
            ),
            None => DEFAULT_INTERVAL,
        },
    };
    if interval < MIN_INTERVAL {
        return Err(format!(
            "the OTLP interval is at least {} s",
            MIN_INTERVAL.as_secs()
        ));
    }
    Ok(Some(Config {
        url,
        headers,
        interval,
        service_name: set("OTEL_SERVICE_NAME").unwrap_or_else(|| default_service.to_string()),
    }))
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// What became of pushes so far, for a daemon to show.
#[derive(Debug, Default)]
pub struct Outcomes {
    pub sent: crate::metrics::Counter,
    pub failed: crate::metrics::Counter,
}

/// Push what `collect` gives every interval, for as long as the task runs.
///
/// `instance` is `service.instance.id`, telling one daemon's numbers from
/// another's. A failed push is logged and counted, and the next interval
/// tries again with what is current then: the numbers are cumulative, so a
/// missed push loses nothing a later one does not carry.
pub async fn push(
    config: Config,
    instance: String,
    outcomes: std::sync::Arc<Outcomes>,
    collect: impl Fn() -> Exposition + Send + 'static,
) {
    let start_ns = now_ns();
    let client = match reqwest::Client::builder()
        .timeout(config.interval.min(Duration::from_secs(10)))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("OTLP: no HTTP client: {e}");
            return;
        }
    };
    tracing::info!(
        url = %config.url,
        interval_secs = config.interval.as_secs_f64(),
        "pushing metrics over OTLP"
    );
    let mut ticker = tokio::time::interval(config.interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    // Failures are logged when they start and when they stop, not each time.
    let mut failing = false;
    loop {
        ticker.tick().await;
        let body = collect().otlp(
            &[
                ("service.name", &config.service_name),
                ("service.instance.id", &instance),
            ],
            start_ns,
            now_ns(),
        );
        let mut request = client.post(&config.url).json(&body);
        for (key, value) in &config.headers {
            request = request.header(key, value);
        }
        let outcome = match request.send().await {
            Ok(response) if response.status().is_success() => Ok(()),
            Ok(response) => {
                let status = response.status();
                let said = response.text().await.unwrap_or_default();
                Err(format!(
                    "{status}: {}",
                    said.chars().take(300).collect::<String>()
                ))
            }
            Err(e) => Err(e.without_url().to_string()),
        };
        match outcome {
            Ok(()) => {
                outcomes.sent.inc();
                if failing {
                    tracing::info!("OTLP: pushing again");
                    failing = false;
                }
            }
            Err(why) => {
                outcomes.failed.inc();
                if !failing {
                    tracing::warn!("OTLP: push to {} failed: {why}", config.url);
                    failing = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn nothing_named_is_nothing_pushed() {
        assert_eq!(configure(&Flags::default(), env(&[]), "node"), Ok(None));
        assert_eq!(
            configure(
                &Flags::default(),
                env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", " ")]),
                "node"
            ),
            Ok(None)
        );
    }

    #[test]
    fn the_endpoint_comes_from_a_flag_then_the_metrics_variable_then_the_base_one() {
        let base = env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318/")]);
        let config = configure(&Flags::default(), base, "node").unwrap().unwrap();
        assert_eq!(config.url, "http://collector:4318/v1/metrics");
        assert_eq!(config.interval, DEFAULT_INTERVAL);
        assert_eq!(config.service_name, "node");

        let both = env(&[
            ("OTEL_EXPORTER_OTLP_ENDPOINT", "http://collector:4318"),
            (
                "OTEL_EXPORTER_OTLP_METRICS_ENDPOINT",
                "https://m.example/ingest",
            ),
            ("OTEL_SERVICE_NAME", "fleet-a"),
            ("OTEL_METRIC_EXPORT_INTERVAL", "30000"),
        ]);
        let config = configure(&Flags::default(), both, "node").unwrap().unwrap();
        assert_eq!(config.url, "https://m.example/ingest");
        assert_eq!(config.interval, Duration::from_secs(30));
        assert_eq!(config.service_name, "fleet-a");

        let flags = Flags {
            endpoint: Some("http://127.0.0.1:9/v1/metrics".into()),
            interval_secs: Some(5),
            ..Flags::default()
        };
        let config = configure(
            &flags,
            env(&[("OTEL_EXPORTER_OTLP_ENDPOINT", "http://x")]),
            "node",
        )
        .unwrap()
        .unwrap();
        assert_eq!(config.url, "http://127.0.0.1:9/v1/metrics");
        assert_eq!(config.interval, Duration::from_secs(5));
    }

    #[test]
    fn headers_come_from_flags_or_from_the_variable_decoded() {
        let flags = Flags {
            endpoint: Some("http://c/v1/metrics".into()),
            headers: vec!["Authorization=Bearer a=b".into(), " x-team = blue ".into()],
            ..Flags::default()
        };
        let config = configure(
            &flags,
            env(&[("OTEL_EXPORTER_OTLP_HEADERS", "ignored=1")]),
            "n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            config.headers,
            [
                ("Authorization".to_string(), "Bearer a=b".to_string()),
                ("x-team".to_string(), "blue".to_string())
            ]
        );
        let config = configure(
            &Flags {
                endpoint: Some("http://c/v1/metrics".into()),
                ..Flags::default()
            },
            env(&[("OTEL_EXPORTER_OTLP_HEADERS", "api-key=s%3Dcr%2Cet,x-org=7")]),
            "n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            config.headers,
            [
                ("api-key".to_string(), "s=cr,et".to_string()),
                ("x-org".to_string(), "7".to_string())
            ]
        );
        // And never in what is printed of the configuration.
        let shown = format!("{config:?}");
        assert!(
            shown.contains("api-key") && !shown.contains("s=cr,et"),
            "{shown}"
        );
    }

    #[test]
    fn what_cannot_be_sent_is_refused_at_the_start() {
        let at = |url: &str| Flags {
            endpoint: Some(url.into()),
            ..Flags::default()
        };
        for (flags, vars, expect) in [
            (at("grpc://c:4317"), vec![], "http or https"),
            (at("not a url"), vec![], ""),
            (at("http://user:pw@c/v1/metrics"), vec![], "header"),
            (
                at("http://c/v1/metrics"),
                vec![("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc")],
                "http/json",
            ),
            (
                at("http://c/v1/metrics"),
                vec![("OTEL_EXPORTER_OTLP_PROTOCOL", "http/protobuf")],
                "http/json",
            ),
            (
                Flags {
                    headers: vec!["no-equals".into()],
                    ..at("http://c/v1/metrics")
                },
                vec![],
                "KEY=VALUE",
            ),
            (
                Flags {
                    headers: vec!["bad name=1".into()],
                    ..at("http://c/v1/metrics")
                },
                vec![],
                "header name",
            ),
            (
                Flags {
                    headers: vec!["x=a\nb".into()],
                    ..at("http://c/v1/metrics")
                },
                vec![],
                "line break",
            ),
            (
                Flags {
                    interval_secs: Some(0),
                    ..at("http://c/v1/metrics")
                },
                vec![],
                "at least",
            ),
            (
                at("http://c/v1/metrics"),
                vec![("OTEL_METRIC_EXPORT_INTERVAL", "soon")],
                "milliseconds",
            ),
            (
                at("http://c/v1/metrics"),
                vec![("OTEL_EXPORTER_OTLP_HEADERS", "k=%zz")],
                "hex",
            ),
        ] {
            let refused = configure(&flags, env(&vars), "n").unwrap_err();
            assert!(refused.contains(expect), "{flags:?} {vars:?}: {refused}");
        }
        // The protocol it speaks is accepted when named.
        let named = env(&[("OTEL_EXPORTER_OTLP_PROTOCOL", "http/json")]);
        assert!(configure(&at("http://c/v1/metrics"), named, "n")
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn a_push_carries_the_headers_and_the_numbers_and_counts_how_it_went() {
        use axum::{extract::Request, http::StatusCode, routing::post, Router};
        use std::sync::{Arc, Mutex};
        let seen = Arc::new(Mutex::new(Vec::<(Option<String>, serde_json::Value)>::new()));
        let kept = Arc::clone(&seen);
        let answers = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = Arc::clone(&answers);
        let app = Router::new().route(
            "/v1/metrics",
            post(move |request: Request| {
                let kept = Arc::clone(&kept);
                let count = Arc::clone(&count);
                async move {
                    let key = request
                        .headers()
                        .get("x-collector-key")
                        .and_then(|v| v.to_str().ok())
                        .map(str::to_string);
                    let body = axum::body::to_bytes(request.into_body(), 1 << 20)
                        .await
                        .unwrap();
                    kept.lock()
                        .unwrap()
                        .push((key, serde_json::from_slice(&body).unwrap()));
                    // The second push is refused, to be counted as failed.
                    if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 1 {
                        StatusCode::SERVICE_UNAVAILABLE
                    } else {
                        StatusCode::OK
                    }
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1/metrics", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let outcomes = Arc::new(Outcomes::default());
        let pushing = tokio::spawn(push(
            Config {
                url,
                headers: vec![("x-collector-key".into(), "k-123".into())],
                interval: Duration::from_millis(100),
                service_name: "node".into(),
            },
            "node-a".into(),
            Arc::clone(&outcomes),
            || {
                let mut e = Exposition::new();
                e.gauge("g", "a gauge", 4.0);
                e
            },
        ));
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while outcomes.sent.get() < 2 {
            assert!(
                std::time::Instant::now() < deadline,
                "pushes did not arrive"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        pushing.abort();
        server.abort();
        assert_eq!(outcomes.failed.get(), 1);
        let seen = seen.lock().unwrap();
        let (key, body) = &seen[0];
        assert_eq!(key.as_deref(), Some("k-123"));
        let resource = &body["resourceMetrics"][0];
        let attributes = resource["resource"]["attributes"].as_array().unwrap();
        assert!(attributes.contains(&serde_json::json!(
            {"key": "service.instance.id", "value": {"stringValue": "node-a"}}
        )));
        assert_eq!(
            resource["scopeMetrics"][0]["metrics"][0]["gauge"]["dataPoints"][0]["asDouble"],
            4.0
        );
    }
}
