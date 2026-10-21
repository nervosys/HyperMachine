//! Telemetry a team sets for itself: the control plane pushes a team's own
//! numbers to a collector the team names, over OTLP/HTTP with JSON.
//!
//! [`crate::otlp`] pushes a *node's* metrics to a collector its operator
//! names. This is the other level. A team puts a destination in the cluster
//! store, like an environment, and the control plane -- which sees every
//! node -- sends that team's numbers to it:
//!
//! | Metric | |
//! |---|---|
//! | `hm_team_sandboxes_running` | the team's sandboxes running now |
//! | `hm_team_sandboxes_paused` | the team's sandboxes paused |
//! | `hm_team_vcpus_running` | vCPUs they run with |
//! | `hm_team_memory_mib_running` | MiB they run with |
//! | `hm_team_running_seconds_24h` | running time in the last 24 hours |
//! | `hm_team_vcpu_seconds_24h` | vCPU-seconds in the last 24 hours |
//! | `hm_team_memory_gib_seconds_24h` | GiB-seconds in the last 24 hours |
//!
//! # Logs
//!
//! A destination whose URL ends in `/v1/metrics` is also sent the team's
//! sandbox events as OTLP logs, to the same URL with `/v1/logs` in its
//! place, under the same operator prefix and with the same headers: one log
//! record for each `sandbox-created`, `-paused`, `-resumed`, `-deleted`,
//! `-expired`, `-lost` and `-forked` that happened to one of the team's
//! sandboxes. Records go out oldest first, at most [`MOST_LOG_RECORDS`] to a
//! push, and a record that could not be delivered is sent again by the next
//! push. Nothing before the control plane started, or before the destination
//! was stored, is sent: a restart does not replay what a collector has.
//!
//! # Traces
//!
//! The same destinations are sent a span for each sandbox that has ended, to
//! `/v1/traces` where `/v1/metrics` is: from its `sandbox-created` or
//! `-forked` event to its `-deleted`, `-expired` or `-lost`, named `sandbox`,
//! with its pauses and resumes as the span's events. A sandbox is a trace of
//! its own (the trace ID is a hash of the sandbox ID). A span is sent when
//! the sandbox ends, once, and not at all when the start has aged out of the
//! event tail.
//!
//! The usage figures are a rolling 24 hours, not an all-time total: a total
//! would mean reading the whole usage ledger, which never shrinks, on every
//! push. Each push is a resource of its own with `hypermachine.team` on it,
//! so a collector shared by several teams tells them apart, and one team's
//! destination is sent that team's numbers alone.
//!
//! # What an operator allows
//!
//! The control plane makes the request, from inside the operator's network,
//! to an address a team chose. So a destination is accepted only under a
//! prefix the operator listed with `--team-otlp-endpoint` (see
//! [`crate::allowlist`]); with none listed, no team telemetry is sent.
//! It is checked when the destination is stored and again each time it is
//! sent to, so removing a prefix stops what was stored under it. No redirect
//! is followed. A destination carries no credentials or query in its URL;
//! a key goes in a header, which is kept as the rest of the store is and
//! never returned.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::metrics::Exposition;
use crate::model::{now_ms, rfc3339, ClusterEvent, SandboxRecord};
use crate::ownership::{RequestTeam, TeamId};
use crate::store::{ClusterStore, EVENT_TAIL};
use crate::usage::{self, By};

/// The most headers a destination carries.
pub const MOST_HEADERS: usize = 8;
/// The longest a header's value may be.
pub const MOST_HEADER_VALUE: usize = 2048;
/// The least a team may ask to be pushed to, in seconds.
pub const MIN_INTERVAL_SECS: u64 = 15;
/// The most.
pub const MAX_INTERVAL_SECS: u64 = 3600;
/// What it is unless said.
pub const DEFAULT_INTERVAL_SECS: u64 = 60;

/// How far back the usage figures look.
const WINDOW_MS: u64 = 24 * 60 * 60 * 1000;
/// How often the control plane looks for a destination that is due.
const TICK: Duration = Duration::from_secs(5);
/// How long one push may take.
const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// Pushes in flight at once.
const AT_ONCE: usize = 16;
/// The most log records one push carries; the rest wait for the next.
pub const MOST_LOG_RECORDS: usize = 500;

/// Headers the control plane sets itself or that change how a request is
/// framed; a team cannot override them.
const RESERVED: [&str; 9] = [
    "host",
    "content-length",
    "content-type",
    "transfer-encoding",
    "connection",
    "upgrade",
    "te",
    "trailer",
    "expect",
];

/// A team's destination, as kept.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamTelemetry {
    #[serde(default, rename = "teamID", skip_serializing_if = "Option::is_none")]
    pub team: Option<TeamId>,
    /// Where it is POSTed to, `/v1/metrics` and all, normalised as it was
    /// checked against the operator's list.
    pub endpoint: String,
    /// Sent with every push, by lower-case name. Secrets as often as not.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub interval_secs: u64,
    pub created_ms: u64,
    pub updated_ms: u64,
}

impl std::fmt::Debug for TeamTelemetry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TeamTelemetry")
            .field("team", &self.team)
            .field("endpoint", &self.endpoint)
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("interval_secs", &self.interval_secs)
            .finish()
    }
}

impl TeamTelemetry {
    /// How a team's destination is keyed in a store: a team has one.
    #[must_use]
    pub fn key(team: Option<&TeamId>) -> String {
        team.map_or("-", TeamId::as_str).to_string()
    }

    fn describe(&self, last: Option<&Push>) -> Value {
        json!({
            "endpoint": self.endpoint,
            // Names, never values.
            "headers": self.headers.keys().collect::<Vec<_>>(),
            "intervalSeconds": self.interval_secs,
            "createdAt": rfc3339(self.created_ms),
            "updatedAt": rfc3339(self.updated_ms),
            "lastPush": last.map(|push| json!({
                "at": rfc3339(push.at_ms),
                "ok": push.ok,
                "error": push.error,
            })),
        })
    }
}

/// How the last push to a destination went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Push {
    pub at_ms: u64,
    pub ok: bool,
    pub error: Option<String>,
}

/// What the routes and the pusher share: the operator's list, and how the
/// last pushes went.
#[derive(Default)]
pub struct Shared {
    allowed: parking_lot::RwLock<Vec<String>>,
    last: parking_lot::Mutex<HashMap<String, Push>>,
    /// How far each team's logs have been sent.
    logs: parking_lot::Mutex<HashMap<String, LogCursor>>,
}

/// How far a team's event log has been sent: the time of the newest record
/// delivered, and the ids of those at that very millisecond, so that two
/// events in one millisecond are neither skipped nor sent twice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogCursor {
    since_ms: u64,
    at_since: HashSet<String>,
}

impl LogCursor {
    fn starting(at_ms: u64) -> Self {
        Self {
            since_ms: at_ms,
            at_since: HashSet::new(),
        }
    }

    fn is_after(&self, event: &ClusterEvent) -> bool {
        event.at_ms > self.since_ms
            || (event.at_ms == self.since_ms && !self.at_since.contains(&event.id))
    }

    fn advanced_over(&self, sent: &[ClusterEvent]) -> Self {
        let mut next = self.clone();
        for event in sent {
            if event.at_ms > next.since_ms {
                next.since_ms = event.at_ms;
                next.at_since.clear();
            }
            if event.at_ms == next.since_ms {
                next.at_since.insert(event.id.clone());
            }
        }
        next
    }
}

/// Where a destination's logs go: its URL with `/v1/logs` where `/v1/metrics`
/// is, or nowhere when it does not end that way.
#[must_use]
pub fn logs_endpoint(endpoint: &str) -> Option<String> {
    endpoint
        .strip_suffix("/v1/metrics")
        .map(|base| format!("{base}/v1/logs"))
}

/// Where a destination's traces go, as [`logs_endpoint`] says for logs.
#[must_use]
pub fn traces_endpoint(endpoint: &str) -> Option<String> {
    endpoint
        .strip_suffix("/v1/metrics")
        .map(|base| format!("{base}/v1/traces"))
}

/// The kinds of event that end a sandbox.
fn ends_a_sandbox(kind: &str) -> bool {
    matches!(kind, "sandbox-deleted" | "sandbox-expired" | "sandbox-lost")
}

/// The first `bytes` bytes of SHA-256 of `text`, as lower-case hex.
fn hash_hex(text: &str, bytes: usize) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .take(bytes)
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A span for each of `ended`, from the start found for it in `all`, as an
/// OTLP/HTTP JSON traces request; `None` when none has a start.
fn otlp_spans(
    resource: &[(&str, &str)],
    all: &[ClusterEvent],
    ended: &[ClusterEvent],
) -> Option<Value> {
    let attribute = |key: &str, value: &str| json!({"key": key, "value": {"stringValue": value}});
    let nanos = |ms: u64| (u128::from(ms) * 1_000_000).to_string();
    let spans: Vec<Value> = ended
        .iter()
        .filter_map(|end| {
            let sandbox = end.sandbox_id.as_deref()?;
            let mine = |e: &&ClusterEvent| {
                e.sandbox_id.as_deref() == Some(sandbox) && e.team_id == end.team_id
            };
            let start = all
                .iter()
                .filter(mine)
                .filter(|e| {
                    matches!(e.kind.as_str(), "sandbox-created" | "sandbox-forked")
                        && e.at_ms <= end.at_ms
                })
                .max_by_key(|e| e.at_ms)?;
            let mut between: Vec<&ClusterEvent> = all
                .iter()
                .filter(mine)
                .filter(|e| {
                    matches!(e.kind.as_str(), "sandbox-paused" | "sandbox-resumed")
                        && e.at_ms >= start.at_ms
                        && e.at_ms <= end.at_ms
                })
                .collect();
            between.sort_by_key(|e| e.at_ms);
            let lost = end.kind.ends_with("-lost");
            let mut attributes = vec![
                attribute("hypermachine.sandbox", sandbox),
                attribute("hypermachine.node", &end.node_id),
                attribute("hypermachine.end", &end.kind),
            ];
            if let Some(template) = start.template_id.as_ref().or(end.template_id.as_ref()) {
                attributes.push(attribute("hypermachine.template", template));
            }
            Some(json!({
                "traceId": hash_hex(&format!("trace/{sandbox}"), 16),
                "spanId": hash_hex(&format!("span/{}", end.id), 8),
                "name": "sandbox",
                "kind": 1,
                "startTimeUnixNano": nanos(start.at_ms),
                "endTimeUnixNano": nanos(end.at_ms),
                "attributes": attributes,
                "events": between.iter().map(|e| json!({
                    "timeUnixNano": nanos(e.at_ms),
                    "name": e.kind,
                })).collect::<Vec<_>>(),
                "status": if lost {
                    json!({"code": 2, "message": "the sandbox was lost"})
                } else {
                    json!({"code": 1})
                },
            }))
        })
        .collect();
    if spans.is_empty() {
        return None;
    }
    Some(json!({"resourceSpans": [{
        "resource": {"attributes": resource.iter().map(|(k, v)| attribute(k, v)).collect::<Vec<_>>()},
        "scopeSpans": [{"scope": {"name": "hypermachine.sandbox-events"}, "spans": spans}],
    }]}))
}

/// The events of `team` that are new to `cursor`, oldest first, at most
/// [`MOST_LOG_RECORDS`].
fn new_events(
    events: &[ClusterEvent],
    team: Option<&TeamId>,
    cursor: &LogCursor,
) -> Vec<ClusterEvent> {
    let mut mine: Vec<ClusterEvent> = events
        .iter()
        .filter(|e| e.team_id.as_ref() == team && cursor.is_after(e))
        .cloned()
        .collect();
    mine.sort_by(|a, b| a.at_ms.cmp(&b.at_ms).then_with(|| a.id.cmp(&b.id)));
    mine.truncate(MOST_LOG_RECORDS);
    mine
}

/// `events` as an OTLP/HTTP JSON logs request.
fn otlp_logs(resource: &[(&str, &str)], events: &[ClusterEvent]) -> Value {
    let attribute = |key: &str, value: &str| json!({"key": key, "value": {"stringValue": value}});
    let records: Vec<Value> = events
        .iter()
        .map(|event| {
            let mut attributes = vec![
                attribute("event.name", &event.kind),
                attribute("event.id", &event.id),
                attribute("hypermachine.node", &event.node_id),
            ];
            if let Some(sandbox) = &event.sandbox_id {
                attributes.push(attribute("hypermachine.sandbox", sandbox));
            }
            if let Some(template) = &event.template_id {
                attributes.push(attribute("hypermachine.template", template));
            }
            if let Some(detail) = &event.detail {
                attributes.push(attribute("hypermachine.detail", detail));
            }
            let at = (u128::from(event.at_ms) * 1_000_000).to_string();
            let (number, text) =
                if event.kind.ends_with("-lost") || event.kind.ends_with("-expired") {
                    (13, "WARN")
                } else {
                    (9, "INFO")
                };
            json!({
                "timeUnixNano": at,
                "observedTimeUnixNano": at,
                "severityNumber": number,
                "severityText": text,
                "body": {"stringValue": event.kind},
                "attributes": attributes,
            })
        })
        .collect();
    json!({"resourceLogs": [{
        "resource": {"attributes": resource.iter().map(|(k, v)| attribute(k, v)).collect::<Vec<_>>()},
        "scopeLogs": [{"scope": {"name": "hypermachine.sandbox-events"}, "logRecords": records}],
    }]})
}

impl Shared {
    /// Allow destinations under `prefixes`.
    pub fn allow(&self, prefixes: Vec<String>) {
        *self.allowed.write() = prefixes;
    }

    /// The prefixes allowed.
    #[must_use]
    pub fn allowed(&self) -> Vec<String> {
        self.allowed.read().clone()
    }

    /// Whether any destination is allowed, and so whether anything is sent.
    #[must_use]
    pub fn sends(&self) -> bool {
        !self.allowed.read().is_empty()
    }

    fn last_of(&self, key: &str) -> Option<Push> {
        self.last.lock().get(key).cloned()
    }

    fn forget(&self, key: &str) {
        self.last.lock().remove(key);
        let mut logs = self.logs.lock();
        logs.remove(key);
        logs.remove(&format!("{key}#traces"));
    }

    /// Where `key`'s logs were sent up to, starting at `from_ms` when never.
    fn log_cursor(&self, key: &str, from_ms: u64) -> LogCursor {
        self.logs
            .lock()
            .entry(key.to_string())
            .or_insert_with(|| LogCursor::starting(from_ms))
            .clone()
    }

    fn set_log_cursor(&self, key: &str, cursor: LogCursor) {
        self.logs.lock().insert(key.to_string(), cursor);
    }

    /// Keep `push` as the last for `key`; whether the one before it was ok.
    fn record(&self, key: &str, push: Push) -> Option<bool> {
        self.last.lock().insert(key.to_string(), push).map(|p| p.ok)
    }
}

fn api_error(status: StatusCode, message: impl std::fmt::Display) -> Response {
    (
        status,
        Json(json!({ "code": status.as_u16(), "message": message.to_string() })),
    )
        .into_response()
}

/// What a team gives.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Given {
    endpoint: String,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    interval_seconds: Option<u64>,
}

/// A destination that passed.
#[derive(Debug)]
struct Checked {
    endpoint: String,
    headers: BTreeMap<String, String>,
    interval_secs: u64,
}

/// `given`, checked: a well-formed URL and headers, an interval in range,
/// and a destination under what `allowed` lists. The status is the reply's.
fn checked(given: Given, allowed: &[String]) -> Result<Checked, (StatusCode, String)> {
    let bad = |why: String| (StatusCode::BAD_REQUEST, why);
    let url = reqwest::Url::parse(&given.endpoint).map_err(|e| bad(format!("endpoint: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(bad("endpoint: OTLP/HTTP is http or https".into()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(bad("endpoint: no host".into()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(bad(
            "endpoint: put credentials in a header, not in the URL".into()
        ));
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(bad(
            "endpoint: no query or fragment; put a key in a header".into()
        ));
    }
    if given.headers.len() > MOST_HEADERS {
        return Err(bad(format!("at most {MOST_HEADERS} headers")));
    }
    let mut headers = BTreeMap::new();
    for (name, value) in given.headers {
        let lower = name.to_ascii_lowercase();
        if reqwest::header::HeaderName::from_bytes(lower.as_bytes()).is_err() {
            return Err(bad(format!("{name:?} is not a header name")));
        }
        if RESERVED.contains(&lower.as_str()) {
            return Err(bad(format!(
                "the {lower} header is the control plane's own"
            )));
        }
        if value.len() > MOST_HEADER_VALUE
            || reqwest::header::HeaderValue::from_str(&value).is_err()
        {
            return Err(bad(format!(
                "header {lower}: a value of at most {MOST_HEADER_VALUE} bytes that can be sent"
            )));
        }
        headers.insert(lower, value);
    }
    let interval_secs = given.interval_seconds.unwrap_or(DEFAULT_INTERVAL_SECS);
    if !(MIN_INTERVAL_SECS..=MAX_INTERVAL_SECS).contains(&interval_secs) {
        return Err(bad(format!(
            "intervalSeconds is between {MIN_INTERVAL_SECS} and {MAX_INTERVAL_SECS}"
        )));
    }
    let endpoint = url.to_string();
    if allowed.is_empty() {
        return Err((
            StatusCode::FORBIDDEN,
            "this control plane sends no team telemetry: its operator has allowed no destination \
             (--team-otlp-endpoint)"
                .into(),
        ));
    }
    if !crate::allowlist::allowed(&endpoint, allowed) {
        return Err((
            StatusCode::FORBIDDEN,
            format!("{endpoint}: not under a destination this control plane's operator allows"),
        ));
    }
    Ok(Checked {
        endpoint,
        headers,
        interval_secs,
    })
}

#[derive(Clone)]
struct Ctx {
    store: Arc<dyn ClusterStore>,
    shared: Arc<Shared>,
}

fn team_of(team: Option<Extension<RequestTeam>>) -> Option<TeamId> {
    team.and_then(|Extension(team)| team.0)
}

/// `GET`, `PUT` and `DELETE /telemetry`: the caller's team's destination.
pub fn router<S: Clone + Send + Sync + 'static>(
    store: Arc<dyn ClusterStore>,
    shared: Arc<Shared>,
) -> Router<S> {
    Router::new()
        .route("/telemetry", get(one).put(put).delete(delete))
        .with_state(Ctx { store, shared })
}

async fn find(ctx: &Ctx, team: Option<&TeamId>) -> Result<Option<TeamTelemetry>, String> {
    let all = ctx
        .store
        .team_telemetries()
        .await
        .map_err(|e| e.to_string())?;
    Ok(all.into_iter().find(|c| c.team.as_ref() == team))
}

async fn one(State(ctx): State<Ctx>, team: Option<Extension<RequestTeam>>) -> Response {
    let team = team_of(team);
    match find(&ctx, team.as_ref()).await {
        Ok(Some(config)) => {
            let last = ctx.shared.last_of(&TeamTelemetry::key(team.as_ref()));
            Json(config.describe(last.as_ref())).into_response()
        }
        Ok(None) => api_error(
            StatusCode::NOT_FOUND,
            "this team has no telemetry destination",
        ),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn put(
    State(ctx): State<Ctx>,
    team: Option<Extension<RequestTeam>>,
    Json(given): Json<Given>,
) -> Response {
    let team = team_of(team);
    let checked = match checked(given, &ctx.shared.allowed()) {
        Ok(checked) => checked,
        Err((status, why)) => return api_error(status, why),
    };
    let before = match find(&ctx, team.as_ref()).await {
        Ok(before) => before,
        Err(e) => return api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    };
    let now = now_ms();
    let config = TeamTelemetry {
        team: team.clone(),
        endpoint: checked.endpoint,
        headers: checked.headers,
        interval_secs: checked.interval_secs,
        created_ms: before.as_ref().map_or(now, |b| b.created_ms),
        updated_ms: now,
    };
    match ctx.store.put_team_telemetry(&config).await {
        Ok(()) => {
            // The last push was to what it replaces.
            ctx.shared.forget(&TeamTelemetry::key(team.as_ref()));
            let status = if before.is_some() {
                StatusCode::OK
            } else {
                StatusCode::CREATED
            };
            (status, Json(config.describe(None))).into_response()
        }
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

async fn delete(State(ctx): State<Ctx>, team: Option<Extension<RequestTeam>>) -> Response {
    let team = team_of(team);
    let key = TeamTelemetry::key(team.as_ref());
    match ctx.store.delete_team_telemetry(&key).await {
        Ok(true) => {
            ctx.shared.forget(&key);
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(false) => api_error(
            StatusCode::NOT_FOUND,
            "this team has no telemetry destination",
        ),
        Err(e) => api_error(StatusCode::SERVICE_UNAVAILABLE, e),
    }
}

/// Everything a push round reads, once, however many teams are due.
struct Snapshot {
    records: Vec<SandboxRecord>,
    events: Vec<ClusterEvent>,
    /// The tail of the event stream, for the destinations that take logs.
    log_events: Vec<ClusterEvent>,
    truncated: bool,
    ledger: Option<usage::Ledger>,
    from_ms: u64,
    now_ms: u64,
}

async fn snapshot(
    store: &dyn ClusterStore,
    now_ms: u64,
    want_events: bool,
) -> Result<Snapshot, String> {
    let from_ms = now_ms.saturating_sub(WINDOW_MS);
    let records = store.sandboxes().await.map_err(|e| e.to_string())?;
    let ledger = store
        .usage_ledger(from_ms)
        .await
        .map_err(|e| e.to_string())?;
    // The events matter only for the part of the window before the ledger
    // began; when it covers the window, they are not read.
    let needs_events = ledger
        .as_ref()
        .and_then(|l| l.since_ms)
        .is_none_or(|since| since > from_ms);
    let read = if needs_events || want_events {
        store.events(EVENT_TAIL).await.map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };
    let truncated = needs_events && read.len() >= EVENT_TAIL;
    // The usage report is given the events only when the ledger does not
    // cover its window; the logs are given them whenever a destination takes
    // logs.
    let log_events = if want_events {
        read.clone()
    } else {
        Vec::new()
    };
    let events = if needs_events { read } else { Vec::new() };
    Ok(Snapshot {
        records,
        events,
        log_events,
        truncated,
        ledger,
        from_ms,
        now_ms,
    })
}

impl Snapshot {
    /// `team`'s numbers, and nobody else's.
    fn exposition(&self, team: Option<&TeamId>) -> Exposition {
        let mine = self
            .records
            .iter()
            .filter(|record| record.team_id.as_ref() == team);
        let (running, paused): (Vec<_>, Vec<_>) = mine.partition(|record| !record.paused);
        let vcpus: u64 = running.iter().map(|r| u64::from(r.cpu_count)).sum();
        let mib: u64 = running.iter().map(|r| r.memory_mb).sum();
        let report = usage::report(
            &self.events,
            self.truncated,
            self.ledger.as_ref(),
            self.from_ms,
            self.now_ms,
            self.now_ms,
            By::Team,
            |owner| owner == team,
        );
        let seconds = |key: &str| report["total"][key].as_f64().unwrap_or(0.0);
        let mut e = Exposition::new();
        e.gauge(
            "hm_team_sandboxes_running",
            "Sandboxes of the team running now",
            running.len() as f64,
        );
        e.gauge(
            "hm_team_sandboxes_paused",
            "Sandboxes of the team paused",
            paused.len() as f64,
        );
        e.gauge(
            "hm_team_vcpus_running",
            "vCPUs the team's running sandboxes have",
            vcpus as f64,
        );
        e.gauge(
            "hm_team_memory_mib_running",
            "MiB of memory the team's running sandboxes have",
            mib as f64,
        );
        e.gauge(
            "hm_team_running_seconds_24h",
            "Seconds the team's sandboxes ran in the last 24 hours",
            seconds("runningSeconds"),
        );
        e.gauge(
            "hm_team_vcpu_seconds_24h",
            "vCPU-seconds the team used in the last 24 hours",
            seconds("vcpuSeconds"),
        );
        e.gauge(
            "hm_team_memory_gib_seconds_24h",
            "GiB-seconds of memory the team used in the last 24 hours",
            seconds("memoryGiBSeconds"),
        );
        e
    }
}

fn now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// POST `body` to `config`'s endpoint, if the operator still allows it.
async fn send(
    client: &reqwest::Client,
    config: &TeamTelemetry,
    url: &str,
    allowed: &[String],
    body: &Value,
) -> Result<(), String> {
    if !crate::allowlist::allowed(url, allowed) {
        return Err("not sent: the destination is no longer under one the operator allows".into());
    }
    let mut request = client.post(url).json(body);
    for (name, value) in &config.headers {
        request = request.header(name, value);
    }
    match request.send().await {
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
    }
}

/// When each destination was last sent to, and when it is next due: by key,
/// the `updated_ms` it was scheduled for and the time.
pub type Schedule = HashMap<String, (u64, Instant)>;

/// One look at the destinations: push to each that is due.
///
/// A destination is due when it has not been sent to, when a team changed it
/// since, or when its interval has passed. The sandboxes and usage are read
/// once for all that are due.
pub async fn round(
    store: &Arc<dyn ClusterStore>,
    shared: &Arc<Shared>,
    client: &reqwest::Client,
    schedule: &mut Schedule,
    now: Instant,
    instance: &str,
    start_ns: u64,
) {
    let configs = match store.team_telemetries().await {
        Ok(configs) => configs,
        Err(e) => {
            tracing::warn!("team telemetry: reading the destinations: {e}");
            return;
        }
    };
    let keys: HashSet<String> = configs
        .iter()
        .map(|c| TeamTelemetry::key(c.team.as_ref()))
        .collect();
    schedule.retain(|key, _| keys.contains(key));
    let due: Vec<TeamTelemetry> = configs
        .into_iter()
        .filter(
            |c| match schedule.get(&TeamTelemetry::key(c.team.as_ref())) {
                Some((updated, next)) => *updated != c.updated_ms || *next <= now,
                None => true,
            },
        )
        .collect();
    if due.is_empty() {
        return;
    }
    for config in &due {
        schedule.insert(
            TeamTelemetry::key(config.team.as_ref()),
            (
                config.updated_ms,
                now + Duration::from_secs(config.interval_secs),
            ),
        );
    }
    let at = now_ms();
    let want_events = due.iter().any(|c| logs_endpoint(&c.endpoint).is_some());
    let snapshot = match snapshot(store.as_ref(), at, want_events).await {
        Ok(snapshot) => snapshot,
        Err(why) => {
            for config in &due {
                shared.record(
                    &TeamTelemetry::key(config.team.as_ref()),
                    Push {
                        at_ms: at,
                        ok: false,
                        error: Some(format!("reading the team's numbers: {why}")),
                    },
                );
            }
            return;
        }
    };
    let allowed = shared.allowed();
    let permits = Arc::new(tokio::sync::Semaphore::new(AT_ONCE));
    let mut sends = tokio::task::JoinSet::new();
    for config in due {
        let key = TeamTelemetry::key(config.team.as_ref());
        let label = key.clone();
        let body = snapshot.exposition(config.team.as_ref()).otlp(
            &[
                ("service.name", "hypermachine"),
                ("hypermachine.team", &label),
                ("service.instance.id", instance),
            ],
            start_ns,
            now_ns(),
        );
        // The events not yet sent, from when the destination began or this
        // control plane did.
        let (cursor, batch) = if logs_endpoint(&config.endpoint).is_some() {
            let cursor = shared.log_cursor(&key, config.created_ms.max(start_ns / 1_000_000));
            let batch = new_events(&snapshot.log_events, config.team.as_ref(), &cursor);
            (cursor, batch)
        } else {
            (LogCursor::default(), Vec::new())
        };
        // And the sandboxes that have ended since, as spans.
        let trace_key = format!("{key}#traces");
        let (trace_cursor, ended, all_for_spans) = if traces_endpoint(&config.endpoint).is_some() {
            let cursor = shared.log_cursor(&trace_key, config.created_ms.max(start_ns / 1_000_000));
            let endings: Vec<ClusterEvent> = snapshot
                .log_events
                .iter()
                .filter(|e| ends_a_sandbox(&e.kind))
                .cloned()
                .collect();
            let ended = new_events(&endings, config.team.as_ref(), &cursor);
            let all = if ended.is_empty() {
                Vec::new()
            } else {
                snapshot.log_events.clone()
            };
            (cursor, ended, all)
        } else {
            (LogCursor::default(), Vec::new(), Vec::new())
        };
        let resource: Vec<(String, String)> = vec![
            ("service.name".into(), "hypermachine".into()),
            ("hypermachine.team".into(), label.clone()),
            ("service.instance.id".into(), instance.to_string()),
        ];
        let (client, allowed, permits) = (client.clone(), allowed.clone(), Arc::clone(&permits));
        sends.spawn(async move {
            let _permit = permits.acquire_owned().await;
            let mut outcome = send(&client, &config, &config.endpoint, &allowed, &body).await;
            let mut advanced = None;
            if let (Some(url), false) = (logs_endpoint(&config.endpoint), batch.is_empty()) {
                let pairs: Vec<(&str, &str)> = resource
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                let logs = otlp_logs(&pairs, &batch);
                match send(&client, &config, &url, &allowed, &logs).await {
                    Ok(()) => advanced = Some(cursor.advanced_over(&batch)),
                    Err(why) if outcome.is_ok() => outcome = Err(format!("logs: {why}")),
                    Err(_) => {}
                }
            }
            let mut advanced_traces = None;
            if let (Some(url), false) = (traces_endpoint(&config.endpoint), ended.is_empty()) {
                let pairs: Vec<(&str, &str)> = resource
                    .iter()
                    .map(|(k, v)| (k.as_str(), v.as_str()))
                    .collect();
                match otlp_spans(&pairs, &all_for_spans, &ended) {
                    // Ended, but with no start to be found: nothing to send
                    // now or ever, so it is passed over.
                    None => advanced_traces = Some(trace_cursor.advanced_over(&ended)),
                    Some(spans) => match send(&client, &config, &url, &allowed, &spans).await {
                        Ok(()) => advanced_traces = Some(trace_cursor.advanced_over(&ended)),
                        Err(why) if outcome.is_ok() => outcome = Err(format!("traces: {why}")),
                        Err(_) => {}
                    },
                }
            }
            (key, outcome, advanced, advanced_traces, trace_key)
        });
    }
    while let Some(done) = sends.join_next().await {
        let Ok((key, outcome, advanced, advanced_traces, trace_key)) = done else {
            continue;
        };
        if let Some(cursor) = advanced {
            shared.set_log_cursor(&key, cursor);
        }
        if let Some(cursor) = advanced_traces {
            shared.set_log_cursor(&trace_key, cursor);
        }
        let (ok, error) = match outcome {
            Ok(()) => (true, None),
            Err(why) => (false, Some(why)),
        };
        let before = shared.record(
            &key,
            Push {
                at_ms: now_ms(),
                ok,
                error: error.clone(),
            },
        );
        // Told when it starts failing and when it stops, not every time.
        match (before, ok) {
            (Some(true) | None, false) => {
                tracing::warn!(
                    "team telemetry: push for team {key} failed: {}",
                    error.unwrap_or_default()
                );
            }
            (Some(false), true) => tracing::info!("team telemetry: pushing for team {key} again"),
            _ => {}
        }
    }
}

/// Push every team's numbers to its destination, for as long as this runs.
pub async fn run(store: Arc<dyn ClusterStore>, shared: Arc<Shared>) {
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(SEND_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            tracing::error!("team telemetry: no HTTP client: {e}");
            return;
        }
    };
    let instance = format!("control-plane-{}", std::process::id());
    let start_ns = now_ns();
    let mut schedule = Schedule::new();
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tracing::info!(
        "pushing teams' own telemetry to destinations under {:?}",
        shared.allowed()
    );
    loop {
        ticker.tick().await;
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            Instant::now(),
            &instance,
            start_ns,
        )
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::MemoryStore;
    use axum::routing::post;

    type Sink = Arc<parking_lot::Mutex<Vec<(axum::http::HeaderMap, Value)>>>;

    /// A collector on loopback: what it was POSTed, and where it is.
    async fn collector() -> (String, Sink) {
        let sink: Sink = Arc::default();
        let take = |State(sink): State<Sink>,
                    headers: axum::http::HeaderMap,
                    body: axum::body::Bytes| async move {
            sink.lock().push((
                headers,
                serde_json::from_slice(&body).unwrap_or(Value::Null),
            ));
            StatusCode::OK
        };
        let app = Router::new()
            .route("/v1/metrics", post(take))
            .route("/v1/logs", post(take))
            .route("/v1/traces", post(take))
            .with_state(Arc::clone(&sink));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (base, sink)
    }

    /// A server that answers every push with a redirect to `target`.
    async fn redirector(target: String) -> String {
        let app = Router::new().route(
            "/v1/metrics",
            post(move || {
                let target = target.clone();
                async move { axum::response::Redirect::temporary(&target) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    /// The routes, each request's team taken from a header as a daemon's
    /// authentication would set it.
    async fn serve(store: Arc<dyn ClusterStore>, shared: Arc<Shared>) -> String {
        let app: Router = router(store, shared).layer(axum::middleware::from_fn(
            |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
                let team = request
                    .headers()
                    .get("x-test-team")
                    .and_then(|v| v.to_str().ok())
                    .map(|t| TeamId::parse(t).unwrap());
                request.extensions_mut().insert(RequestTeam(team));
                next.run(request).await
            },
        ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        base
    }

    async fn call(
        base: &str,
        method: &str,
        team: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = reqwest::Client::new()
            .request(
                reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                format!("{base}/telemetry"),
            )
            .header("x-test-team", team);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.unwrap();
        let status = StatusCode::from_u16(response.status().as_u16()).unwrap();
        let bytes = response.bytes().await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(5))
            .build()
            .unwrap()
    }

    fn record(id: &str, team: &str, paused: bool, cpus: u32, mib: u64) -> SandboxRecord {
        serde_json::from_value(json!({
            "sandbox_id": id, "owner_id": "key-a", "team_id": team, "node_id": "n",
            "template_id": "base", "started_at_ms": 0, "end_at_ms": 0, "cpu_count": cpus,
            "memory_mb": mib, "envd_version": "", "descriptor": {}, "paused": paused,
        }))
        .unwrap()
    }

    /// A start event for a sandbox, `ago_ms` before now.
    fn started(id: &str, team: &str, ago_ms: u64, cpus: u64, mib: u64) -> ClusterEvent {
        let mut event = ClusterEvent::new("sandbox-created", "n", Some(id));
        event.at_ms = now_ms() - ago_ms;
        event.team_id = Some(TeamId::parse(team).unwrap());
        event.template_id = Some("base".into());
        event.data = Some(json!({"hm_usage": {
            "vcpuCount": cpus, "memoryMB": mib, "running": true, "ownerId": "key-a"}}));
        event
    }

    fn gauge(body: &Value, name: &str) -> f64 {
        body["resourceMetrics"][0]["scopeMetrics"][0]["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["name"] == name)
            .unwrap_or_else(|| panic!("no {name} in {body}"))["gauge"]["dataPoints"][0]["asDouble"]
            .as_f64()
            .unwrap()
    }

    fn resource(body: &Value, key: &str) -> Option<String> {
        body["resourceMetrics"][0]["resource"]["attributes"]
            .as_array()?
            .iter()
            .find(|a| a["key"] == key)
            .and_then(|a| a["value"]["stringValue"].as_str())
            .map(str::to_string)
    }

    fn given(body: Value) -> Given {
        serde_json::from_value(body).unwrap()
    }

    fn list(prefix: &str) -> Vec<String> {
        vec![prefix.to_string()]
    }

    #[test]
    fn a_destination_is_checked_before_it_is_kept() {
        let allowed = list("https://collector.example/teams");
        let ok = checked(
            given(
                json!({"endpoint": "https://collector.example/teams/red/v1/metrics",
                         "headers": {"Authorization": "Bearer k"}, "intervalSeconds": 30}),
            ),
            &allowed,
        )
        .unwrap();
        assert_eq!(ok.interval_secs, 30);
        assert_eq!(ok.headers.get("authorization").unwrap(), "Bearer k");
        let default = checked(
            given(json!({"endpoint": "https://collector.example/teams/x"})),
            &allowed,
        )
        .unwrap();
        assert_eq!(default.interval_secs, DEFAULT_INTERVAL_SECS);

        let nine: BTreeMap<String, String> =
            (0..9).map(|i| (format!("h{i}"), "v".into())).collect();
        let cases = [
            (json!({"endpoint": "not a url"}), "endpoint"),
            (
                json!({"endpoint": "ftp://collector.example/teams"}),
                "http or https",
            ),
            (
                json!({"endpoint": "https://u:p@collector.example/teams"}),
                "credentials",
            ),
            (
                json!({"endpoint": "https://collector.example/teams?key=k"}),
                "query",
            ),
            (
                json!({"endpoint": "https://collector.example/teams#f"}),
                "query",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "headers": {"Host": "x"}}),
                "own",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "headers": {"bad name": "x"}}),
                "header name",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "headers": {"x": "a\nb"}}),
                "sent",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "headers": nine}),
                "at most",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "intervalSeconds": 5}),
                "between",
            ),
            (
                json!({"endpoint": "https://collector.example/teams", "intervalSeconds": 99999}),
                "between",
            ),
        ];
        for (body, expect) in cases {
            let (status, why) = checked(given(body.clone()), &allowed).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST, "{body}: {why}");
            assert!(why.contains(expect), "{body}: {why}");
        }
        // Well-formed, but not the operator's to allow.
        for endpoint in [
            "https://collector.example/other",
            "https://evil.example/teams",
            "http://169.254.169.254/teams",
            "https://collector.example/teams/../other",
        ] {
            let (status, why) =
                checked(given(json!({"endpoint": endpoint})), &allowed).unwrap_err();
            assert_eq!(status, StatusCode::FORBIDDEN, "{endpoint}: {why}");
        }
        let (status, why) = checked(
            given(json!({"endpoint": "https://collector.example/teams"})),
            &[],
        )
        .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert!(why.contains("no destination"), "{why}");
    }

    #[test]
    fn a_header_value_is_not_in_the_debug_output() {
        let config = TeamTelemetry {
            team: None,
            endpoint: "https://c.example/v1/metrics".into(),
            headers: BTreeMap::from([("authorization".into(), "Bearer s3cr3t".into())]),
            interval_secs: 60,
            created_ms: 0,
            updated_ms: 0,
        };
        let shown = format!("{config:?}");
        assert!(
            shown.contains("authorization") && !shown.contains("s3cr3t"),
            "{shown}"
        );
        assert!(!config.describe(None).to_string().contains("s3cr3t"));
    }

    #[tokio::test]
    async fn a_team_is_sent_its_own_numbers_and_only_when_due() {
        let (base, sink) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        for r in [
            record("a1", "team-a", false, 2, 512),
            record("a2", "team-a", false, 2, 512),
            record("a3", "team-a", true, 4, 1024),
            record("b1", "team-b", false, 8, 4096),
        ] {
            store.put_sandbox(&r).await.unwrap();
        }
        // team-a's a1 has run ten minutes; team-b's b1 an hour.
        store
            .publish(&started("a1", "team-a", 600_000, 2, 512))
            .await
            .unwrap();
        store
            .publish(&started("b1", "team-b", 3_600_000, 8, 4096))
            .await
            .unwrap();
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;

        let (status, made) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"),
                        "headers": {"Authorization": "Bearer k"}, "intervalSeconds": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        assert_eq!(made["headers"], json!(["authorization"]));
        assert!(!made.to_string().contains("Bearer k"));
        assert_eq!(made["lastPush"], Value::Null);

        let (client, mut schedule) = (client(), Schedule::new());
        let start = Instant::now();
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        let got = sink.lock().clone();
        assert_eq!(got.len(), 1, "one push, for the one destination");
        let (headers, body) = &got[0];
        assert_eq!(headers.get("authorization").unwrap(), "Bearer k");
        assert_eq!(
            resource(body, "hypermachine.team").as_deref(),
            Some("team-a")
        );
        assert_eq!(
            resource(body, "service.name").as_deref(),
            Some("hypermachine")
        );
        assert_eq!(gauge(body, "hm_team_sandboxes_running"), 2.0);
        assert_eq!(gauge(body, "hm_team_sandboxes_paused"), 1.0);
        assert_eq!(gauge(body, "hm_team_vcpus_running"), 4.0);
        assert_eq!(gauge(body, "hm_team_memory_mib_running"), 1024.0);
        // Ten minutes at 2 vCPUs and 512 MiB, and none of team-b's hour.
        let seconds = gauge(body, "hm_team_running_seconds_24h");
        assert!((599.0..=620.0).contains(&seconds), "{seconds}");
        let vcpu = gauge(body, "hm_team_vcpu_seconds_24h");
        assert!((1198.0..=1240.0).contains(&vcpu), "{vcpu}");
        assert!(!body.to_string().contains("team-b"));

        let (status, shown) = call(&app, "GET", "team-a", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(shown["lastPush"]["ok"], true, "{shown}");
        let (status, _) = call(&app, "GET", "team-b", None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "team-b has none");

        // Not due again until its interval has passed.
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(14),
            "test",
            0,
        )
        .await;
        assert_eq!(sink.lock().len(), 1);
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(16),
            "test",
            0,
        )
        .await;
        assert_eq!(sink.lock().len(), 2);

        // Changing it sends at once; deleting it stops.
        //
        // A change is noticed by its `updated_ms` differing, and that is a
        // millisecond clock: a second edit inside the millisecond of the first
        // is the same destination as far as the schedule can tell. This test
        // is fast enough to do exactly that (1 run in 30), so it waits one out.
        tokio::time::sleep(Duration::from_millis(3)).await;
        let (status, _) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"), "intervalSeconds": 3600})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(17),
            "test",
            0,
        )
        .await;
        assert_eq!(sink.lock().len(), 3);
        assert!(
            sink.lock()[2].0.get("authorization").is_none(),
            "its headers were replaced"
        );
        assert_eq!(
            call(&app, "DELETE", "team-a", None).await.0,
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            call(&app, "DELETE", "team-a", None).await.0,
            StatusCode::NOT_FOUND
        );
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(9000),
            "test",
            0,
        )
        .await;
        assert_eq!(sink.lock().len(), 3);
    }

    #[tokio::test]
    async fn two_teams_are_sent_to_their_own_destinations_alone() {
        let (base_a, sink_a) = collector().await;
        let (base_b, sink_b) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        store
            .put_sandbox(&record("a1", "team-a", false, 1, 256))
            .await
            .unwrap();
        store
            .put_sandbox(&record("b1", "team-b", false, 1, 256))
            .await
            .unwrap();
        let shared = Arc::new(Shared::default());
        shared.allow(vec![base_a.clone(), base_b.clone()]);
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        for (team, base) in [("team-a", &base_a), ("team-b", &base_b)] {
            let (status, made) = call(
                &app,
                "PUT",
                team,
                Some(json!({"endpoint": format!("{base}/v1/metrics")})),
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{made}");
        }
        let mut schedule = Schedule::new();
        round(
            &store,
            &shared,
            &client(),
            &mut schedule,
            Instant::now(),
            "test",
            0,
        )
        .await;
        for (sink, team, other) in [(&sink_a, "team-a", "team-b"), (&sink_b, "team-b", "team-a")] {
            let got = sink.lock().clone();
            assert_eq!(got.len(), 1);
            assert_eq!(
                resource(&got[0].1, "hypermachine.team").as_deref(),
                Some(team)
            );
            assert!(!got[0].1.to_string().contains(other));
        }
    }

    #[tokio::test]
    async fn nothing_is_stored_or_sent_without_an_operator_allowing_it() {
        let (base, sink) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let body = json!({"endpoint": format!("{base}/v1/metrics")});
        // No prefix listed: nothing is sent to.
        let (status, why) = call(&app, "PUT", "team-a", Some(body.clone())).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{why}");
        assert!(why["message"].as_str().unwrap().contains("no destination"));
        assert!(!shared.sends());
        // A prefix that is not this destination's.
        shared.allow(list("http://127.0.0.1:1/elsewhere"));
        assert_eq!(
            call(&app, "PUT", "team-a", Some(body)).await.0,
            StatusCode::FORBIDDEN
        );
        assert!(store.team_telemetries().await.unwrap().is_empty());
        assert!(sink.lock().is_empty());
    }

    #[tokio::test]
    async fn a_destination_the_operator_stops_allowing_is_not_sent_to() {
        let (base, sink) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let (status, _) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        // The operator now lists another place only.
        shared.allow(list("http://127.0.0.1:1/elsewhere"));
        let mut schedule = Schedule::new();
        round(
            &store,
            &shared,
            &client(),
            &mut schedule,
            Instant::now(),
            "test",
            0,
        )
        .await;
        assert!(sink.lock().is_empty());
        let (_, shown) = call(&app, "GET", "team-a", None).await;
        assert_eq!(shown["lastPush"]["ok"], false, "{shown}");
        assert!(shown["lastPush"]["error"]
            .as_str()
            .unwrap()
            .contains("no longer"));
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed() {
        let (target, behind) = collector().await;
        let redirecting = redirector(format!("{target}/v1/metrics")).await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        // Only the redirecting server is allowed; the target is not.
        shared.allow(list(&redirecting));
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let (status, _) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{redirecting}/v1/metrics")})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let mut schedule = Schedule::new();
        round(
            &store,
            &shared,
            &client(),
            &mut schedule,
            Instant::now(),
            "test",
            0,
        )
        .await;
        assert!(behind.lock().is_empty(), "the redirect was followed");
        let (_, shown) = call(&app, "GET", "team-a", None).await;
        assert_eq!(shown["lastPush"]["ok"], false, "{shown}");
        assert!(
            shown["lastPush"]["error"]
                .as_str()
                .unwrap()
                .starts_with("307"),
            "{shown}"
        );
    }

    /// The log records a collector was sent, as (event name, sandbox).
    fn log_records(sink: &Sink) -> Vec<(String, String)> {
        let attr = |record: &Value, key: &str| {
            record["attributes"]
                .as_array()
                .and_then(|all| all.iter().find(|a| a["key"] == key))
                .and_then(|a| a["value"]["stringValue"].as_str())
                .unwrap_or("")
                .to_string()
        };
        sink.lock()
            .iter()
            .flat_map(|(_, body)| {
                body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .map(|record| {
                (
                    attr(&record, "event.name"),
                    attr(&record, "hypermachine.sandbox"),
                )
            })
            .collect()
    }

    fn event_of(kind: &str, id: &str, team: &str, at_ms: u64) -> ClusterEvent {
        let mut event = ClusterEvent::new(kind, "n", Some(id));
        event.id = format!("{kind}/{id}/{at_ms}");
        event.at_ms = at_ms;
        event.team_id = Some(TeamId::parse(team).unwrap());
        event
    }

    #[test]
    fn logs_go_where_metrics_do_with_the_path_swapped() {
        assert_eq!(
            logs_endpoint("https://c.example/teams/a/v1/metrics").as_deref(),
            Some("https://c.example/teams/a/v1/logs")
        );
        assert_eq!(logs_endpoint("https://c.example/teams/a/metrics"), None);
        assert_eq!(logs_endpoint("https://c.example/v1/metrics/extra"), None);
    }

    #[test]
    fn a_cursor_neither_skips_nor_repeats_events_in_one_millisecond() {
        let (a, b) = (
            event_of("sandbox-created", "a", "t", 100),
            event_of("sandbox-paused", "b", "t", 100),
        );
        let cursor = LogCursor::starting(100);
        let first = new_events(
            &[a.clone(), b.clone()],
            TeamId::parse("t").ok().as_ref(),
            &cursor,
        );
        assert_eq!(first.len(), 2);
        let after_a = cursor.advanced_over(std::slice::from_ref(&a));
        let rest = new_events(
            &[a.clone(), b.clone()],
            TeamId::parse("t").ok().as_ref(),
            &after_a,
        );
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].id, b.id);
        let after_both = after_a.advanced_over(&rest);
        assert!(new_events(&[a, b], TeamId::parse("t").ok().as_ref(), &after_both).is_empty());
    }

    #[tokio::test]
    async fn a_team_is_sent_its_own_sandbox_events_as_logs_once() {
        let (base, sink) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        // Before the destination exists: not sent.
        store
            .publish(&event_of(
                "sandbox-created",
                "old",
                "team-a",
                now_ms() - 60_000,
            ))
            .await
            .unwrap();
        let (status, made) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"), "intervalSeconds": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        tokio::time::sleep(Duration::from_millis(5)).await;
        let at = now_ms();
        for event in [
            event_of("sandbox-created", "a1", "team-a", at),
            event_of("sandbox-paused", "a1", "team-a", at + 1),
            event_of("sandbox-created", "b1", "team-b", at + 1),
        ] {
            store.publish(&event).await.unwrap();
        }

        let (client, mut schedule) = (client(), Schedule::new());
        let start = Instant::now();
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        assert_eq!(
            log_records(&sink),
            vec![
                ("sandbox-created".to_string(), "a1".to_string()),
                ("sandbox-paused".to_string(), "a1".to_string()),
            ],
            "the team's two, oldest first; not the other team's, not the old one"
        );
        let logs_body = sink
            .lock()
            .iter()
            .find(|(_, body)| body.get("resourceLogs").is_some())
            .map(|(_, body)| body.clone())
            .unwrap();
        let resource_team = logs_body["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["key"] == "hypermachine.team")
            .map(|a| a["value"]["stringValue"].clone());
        assert_eq!(resource_team, Some(json!("team-a")));

        // Nothing new: the next push carries metrics and no logs.
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(16),
            "test",
            0,
        )
        .await;
        assert_eq!(log_records(&sink).len(), 2, "not sent twice");
        // One more.
        store
            .publish(&event_of("sandbox-deleted", "a1", "team-a", now_ms() + 1))
            .await
            .unwrap();
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(40),
            "test",
            0,
        )
        .await;
        let all = log_records(&sink);
        assert_eq!(all.len(), 3, "{all:?}");
        assert_eq!(all[2].0, "sandbox-deleted");
    }

    #[tokio::test]
    async fn logs_that_could_not_be_delivered_are_sent_again() {
        // A collector that refuses logs the first time it is asked.
        let hits: Sink = Arc::default();
        let refused = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let app = Router::new()
            .route("/v1/metrics", post(|| async { StatusCode::OK }))
            .route(
                "/v1/logs",
                post(
                    |State((hits, refused)): State<(Sink, Arc<std::sync::atomic::AtomicBool>)>,
                     headers: axum::http::HeaderMap,
                     body: axum::body::Bytes| async move {
                        hits.lock().push((
                            headers,
                            serde_json::from_slice(&body).unwrap_or(Value::Null),
                        ));
                        if refused.swap(false, std::sync::atomic::Ordering::SeqCst) {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::OK
                        }
                    },
                ),
            )
            .with_state((Arc::clone(&hits), refused));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let ui = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let (status, made) = call(
            &ui,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"), "intervalSeconds": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        tokio::time::sleep(Duration::from_millis(5)).await;
        store
            .publish(&event_of("sandbox-lost", "a1", "team-a", now_ms()))
            .await
            .unwrap();

        let (client, mut schedule) = (client(), Schedule::new());
        let start = Instant::now();
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        let (_, shown) = call(&ui, "GET", "team-a", None).await;
        assert_eq!(shown["lastPush"]["ok"], false, "{shown}");
        assert!(
            shown["lastPush"]["error"]
                .as_str()
                .unwrap()
                .starts_with("logs: 503"),
            "{shown}"
        );
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(16),
            "test",
            0,
        )
        .await;
        let sent = log_records(&hits);
        assert_eq!(sent.len(), 2, "the same record, offered again: {sent:?}");
        assert_eq!(sent[0], sent[1]);
        let (_, shown) = call(&ui, "GET", "team-a", None).await;
        assert_eq!(shown["lastPush"]["ok"], true, "{shown}");
        // And a WARN: it is a loss.
        let severity = hits.lock()[1].1["resourceLogs"][0]["scopeLogs"][0]["logRecords"][0]
            ["severityText"]
            .clone();
        assert_eq!(severity, json!("WARN"));
    }

    /// The spans a collector was sent.
    fn spans(sink: &Sink) -> Vec<Value> {
        sink.lock()
            .iter()
            .flat_map(|(_, body)| {
                body["resourceSpans"][0]["scopeSpans"][0]["spans"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .collect()
    }

    #[test]
    fn traces_go_where_metrics_do_with_the_path_swapped() {
        assert_eq!(
            traces_endpoint("https://c.example/teams/a/v1/metrics").as_deref(),
            Some("https://c.example/teams/a/v1/traces")
        );
        assert_eq!(traces_endpoint("https://c.example/teams/a/metrics"), None);
    }

    #[tokio::test]
    async fn a_sandbox_that_ended_is_sent_as_one_span_with_its_pauses() {
        let (base, sink) = collector().await;
        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let app = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let (status, made) = call(
            &app,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"), "intervalSeconds": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        tokio::time::sleep(Duration::from_millis(5)).await;
        let at = now_ms();
        for event in [
            event_of("sandbox-created", "a1", "team-a", at),
            event_of("sandbox-paused", "a1", "team-a", at + 10),
            event_of("sandbox-resumed", "a1", "team-a", at + 20),
            // Still running: no span yet.
            event_of("sandbox-created", "a2", "team-a", at + 1),
            // Another team's.
            event_of("sandbox-created", "b1", "team-b", at),
            event_of("sandbox-deleted", "b1", "team-b", at + 30),
        ] {
            store.publish(&event).await.unwrap();
        }
        let (client, mut schedule) = (client(), Schedule::new());
        let start = Instant::now();
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        assert!(spans(&sink).is_empty(), "nothing has ended");

        store
            .publish(&event_of("sandbox-deleted", "a1", "team-a", at + 40))
            .await
            .unwrap();
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(16),
            "test",
            0,
        )
        .await;
        let sent = spans(&sink);
        assert_eq!(sent.len(), 1, "{sent:?}");
        let span = &sent[0];
        assert_eq!(span["name"], "sandbox");
        assert_eq!(span["traceId"].as_str().unwrap().len(), 32);
        assert_eq!(span["spanId"].as_str().unwrap().len(), 16);
        let nanos = |ms: u64| (u128::from(ms) * 1_000_000).to_string();
        assert_eq!(span["startTimeUnixNano"], json!(nanos(at)));
        assert_eq!(span["endTimeUnixNano"], json!(nanos(at + 40)));
        assert_eq!(span["status"]["code"], 1);
        let names: Vec<&str> = span["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["sandbox-paused", "sandbox-resumed"]);

        // Not sent twice.
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(40),
            "test",
            0,
        )
        .await;
        assert_eq!(spans(&sink).len(), 1);
    }

    #[tokio::test]
    async fn a_lost_sandbox_is_an_error_span_and_a_refused_one_is_sent_again() {
        let hits: Sink = Arc::default();
        let refused = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let app = Router::new()
            .route("/v1/metrics", post(|| async { StatusCode::OK }))
            .route("/v1/logs", post(|| async { StatusCode::OK }))
            .route(
                "/v1/traces",
                post(
                    |State((hits, refused)): State<(Sink, Arc<std::sync::atomic::AtomicBool>)>,
                     headers: axum::http::HeaderMap,
                     body: axum::body::Bytes| async move {
                        hits.lock().push((
                            headers,
                            serde_json::from_slice(&body).unwrap_or(Value::Null),
                        ));
                        if refused.swap(false, std::sync::atomic::Ordering::SeqCst) {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::OK
                        }
                    },
                ),
            )
            .with_state((Arc::clone(&hits), refused));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let store: Arc<dyn ClusterStore> = Arc::new(MemoryStore::default());
        let shared = Arc::new(Shared::default());
        shared.allow(list(&base));
        let ui = serve(Arc::clone(&store), Arc::clone(&shared)).await;
        let (status, made) = call(
            &ui,
            "PUT",
            "team-a",
            Some(json!({"endpoint": format!("{base}/v1/metrics"), "intervalSeconds": 15})),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{made}");
        tokio::time::sleep(Duration::from_millis(5)).await;
        let at = now_ms();
        store
            .publish(&event_of("sandbox-created", "a1", "team-a", at))
            .await
            .unwrap();
        store
            .publish(&event_of("sandbox-lost", "a1", "team-a", at + 5))
            .await
            .unwrap();

        let (client, mut schedule) = (client(), Schedule::new());
        let start = Instant::now();
        round(&store, &shared, &client, &mut schedule, start, "test", 0).await;
        let (_, shown) = call(&ui, "GET", "team-a", None).await;
        assert_eq!(shown["lastPush"]["ok"], false, "{shown}");
        assert!(
            shown["lastPush"]["error"]
                .as_str()
                .unwrap()
                .starts_with("traces: 503"),
            "{shown}"
        );
        round(
            &store,
            &shared,
            &client,
            &mut schedule,
            start + Duration::from_secs(16),
            "test",
            0,
        )
        .await;
        let sent = spans(&hits);
        assert_eq!(sent.len(), 2, "offered again: {sent:?}");
        assert_eq!(sent[0]["spanId"], sent[1]["spanId"]);
        assert_eq!(sent[1]["status"]["code"], 2);
    }
}
