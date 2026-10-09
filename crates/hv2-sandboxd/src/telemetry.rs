//! What a sandbox used and what happened to it: E2B's
//! `GET /sandboxes/{id}/metrics`, `GET /sandboxes/metrics`, and
//! `GET /sandboxes/{id}/logs` (v1 and v2).
//!
//! Metrics are the guest's own account -- CPU from `/proc/stat`, memory
//! from `/proc/meminfo`, its root filesystem from `statvfs` -- read in one
//! agent round trip every [`INTERVAL`], as E2B samples, and kept for the
//! last hour. Logs are the node's lifecycle events for the sandbox.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use serde_json::{json, Value};

use hv2_cluster::model::{now_ms, rfc3339};

use super::{
    api_error, AppState, Arc, Deserialize, IntoResponse, Json, Path, Query, Response, State,
    StatusCode,
};

/// How often every running sandbox is sampled.
pub(crate) const INTERVAL: Duration = Duration::from_secs(5);
/// An hour of samples.
const MAX_SAMPLES: usize = 720;
const MAX_LOGS: usize = 1000;
/// How long a guest has to answer one sample.
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(crate) struct Record {
    samples: VecDeque<Value>,
    /// The CPU counters of the last sample, for the next one's rate.
    last_ticks: Option<(u64, u64)>,
    logs: VecDeque<(u64, &'static str, String)>,
}

pub(crate) type Telemetry = parking_lot::Mutex<HashMap<String, Record>>;

/// The highest CPU use `sandbox_id`'s guest reported in any sample taken at
/// or after `since_ms`; `None` when no sample falls in that window.
pub(crate) fn busiest_since(state: &AppState, sandbox_id: &str, since_ms: u64) -> Option<f64> {
    let all = state.telemetry.lock();
    all.get(sandbox_id)?
        .samples
        .iter()
        .filter(|s| {
            s["timestampUnix"]
                .as_u64()
                .is_some_and(|t| t * 1000 >= since_ms)
        })
        .filter_map(|s| s["cpuUsedPct"].as_f64())
        .fold(None, |max: Option<f64>, pct| {
            Some(max.map_or(pct, |m| m.max(pct)))
        })
}

/// Note an event in `sandbox_id`'s log.
pub(crate) fn log(
    state: &AppState,
    sandbox_id: &str,
    level: &'static str,
    message: impl Into<String>,
) {
    let mut all = state.telemetry.lock();
    let record = all.entry(sandbox_id.to_string()).or_default();
    if record.logs.len() == MAX_LOGS {
        record.logs.pop_front();
    }
    record.logs.push_back((now_ms(), level, message.into()));
}

/// Forget a sandbox that ended.
pub(crate) fn forget(state: &AppState, sandbox_id: &str) {
    state.telemetry.lock().remove(sandbox_id);
}

/// Sample every running sandbox, every [`INTERVAL`], for as long as the
/// node runs. Paused ones have nothing to report and are not woken; nor are
/// ones in standby, which a sample would resume.
pub(crate) async fn sample(state: Arc<AppState>) {
    loop {
        tokio::time::sleep(INTERVAL).await;
        let running: Vec<(String, Arc<hv2_agent::AgentVM>, u32)> = state
            .sandboxes
            .lock()
            .iter()
            .filter(|(_, live)| !live.vm.in_standby())
            .map(|(id, live)| (id.clone(), Arc::clone(&live.vm), live.record.cpu_count))
            .collect();
        let asks = running.into_iter().map(|(id, vm, cpus)| async move {
            (id, cpus, vm.stats_in_guest(SAMPLE_TIMEOUT).await)
        });
        let answers = futures_join_all(asks).await;
        let at = now_ms();
        let mut all = state.telemetry.lock();
        for (id, cpus, answer) in answers {
            let Ok(stats) = answer else { continue };
            let record = all.entry(id).or_default();
            let pct = match record.last_ticks {
                Some((busy, total)) if stats.cpu_total_ticks > total => {
                    100.0 * (stats.cpu_busy_ticks.saturating_sub(busy)) as f64
                        / (stats.cpu_total_ticks - total) as f64
                }
                _ => 0.0,
            };
            record.last_ticks = Some((stats.cpu_busy_ticks, stats.cpu_total_ticks));
            if record.samples.len() == MAX_SAMPLES {
                record.samples.pop_front();
            }
            record.samples.push_back(json!({
                "timestamp": rfc3339(at),
                "timestampUnix": at / 1000,
                "cpuCount": if stats.cpus > 0 { stats.cpus } else { cpus },
                "cpuUsedPct": (pct * 100.0).round() / 100.0,
                "memUsed": stats.mem_total.saturating_sub(stats.mem_available),
                "memTotal": stats.mem_total,
                "memCache": stats.mem_cached,
                "diskUsed": stats.disk_used,
                "diskTotal": stats.disk_total,
            }));
        }
    }
}

/// Concurrently, without a crate for it: each future on its own task.
async fn futures_join_all<F, T>(futures: impl Iterator<Item = F>) -> Vec<T>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let handles: Vec<_> = futures.map(tokio::spawn).collect();
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        if let Ok(value) = handle.await {
            out.push(value);
        }
    }
    out
}

#[derive(Debug, Deserialize)]
pub(crate) struct Range {
    start: Option<u64>,
    end: Option<u64>,
}

fn known(state: &AppState, id: &str) -> bool {
    state.sandboxes.lock().contains_key(id) || state.paused.lock().contains_key(id)
}

/// `GET /sandboxes/{id}/metrics?start&end`: samples in `[start, end]`,
/// Unix seconds.
pub(crate) async fn metrics(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(range): Query<Range>,
) -> Response {
    if !known(&state, &id) {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}"));
    }
    let all = state.telemetry.lock();
    let samples: Vec<Value> = all
        .get(&id)
        .map(|r| {
            r.samples
                .iter()
                .filter(|s| {
                    let t = s["timestampUnix"].as_u64().unwrap_or(0);
                    range.start.is_none_or(|start| t >= start)
                        && range.end.is_none_or(|end| t <= end)
                })
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    Json(samples).into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct Ids {
    sandbox_ids: Option<String>,
}

/// `GET /sandboxes/metrics?sandbox_ids=a,b`: each one's latest sample.
pub(crate) async fn latest(State(state): State<Arc<AppState>>, Query(ids): Query<Ids>) -> Response {
    let all = state.telemetry.lock();
    let wanted: Option<Vec<&str>> = ids.sandbox_ids.as_deref().map(|s| s.split(',').collect());
    let mut out = serde_json::Map::new();
    for (id, record) in all.iter() {
        if wanted.as_ref().is_some_and(|w| !w.contains(&id.as_str())) {
            continue;
        }
        if let Some(sample) = record.samples.back() {
            out.insert(id.clone(), sample.clone());
        }
    }
    Json(json!({ "sandboxes": out })).into_response()
}

#[derive(Debug, Deserialize)]
pub(crate) struct LogQuery {
    start: Option<u64>,
    cursor: Option<u64>,
    limit: Option<usize>,
    direction: Option<String>,
    level: Option<String>,
    search: Option<String>,
}

fn entries(state: &AppState, id: &str, q: &LogQuery) -> Vec<(u64, &'static str, String)> {
    let all = state.telemetry.lock();
    let Some(record) = all.get(id) else {
        return Vec::new();
    };
    let from = q.start.or(q.cursor).unwrap_or(0);
    let mut out: Vec<_> = record
        .logs
        .iter()
        .filter(|(t, level, message)| {
            *t >= from
                && q.level.as_deref().is_none_or(|l| l == *level)
                && q.search.as_deref().is_none_or(|s| message.contains(s))
        })
        .cloned()
        .collect();
    if q.direction.as_deref() == Some("backward") {
        out.reverse();
    }
    out.truncate(q.limit.unwrap_or(1000).min(MAX_LOGS));
    out
}

fn entry_json((t, level, message): &(u64, &'static str, String)) -> Value {
    json!({ "timestamp": rfc3339(*t), "level": level, "message": message, "fields": {} })
}

/// `GET /sandboxes/{id}/logs` (v1): lines and structured entries.
pub(crate) async fn logs_v1(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Response {
    if !known(&state, &id) {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}"));
    }
    let found = entries(&state, &id, &q);
    let lines: Vec<Value> = found
        .iter()
        .map(|(t, _, m)| json!({ "timestamp": rfc3339(*t), "line": m }))
        .collect();
    let structured: Vec<Value> = found.iter().map(entry_json).collect();
    Json(json!({ "logs": lines, "logEntries": structured })).into_response()
}

/// `GET /v2/sandboxes/{id}/logs`.
pub(crate) async fn logs_v2(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<LogQuery>,
) -> Response {
    if !known(&state, &id) {
        return api_error(StatusCode::NOT_FOUND, format!("no sandbox {id}"));
    }
    let found: Vec<Value> = entries(&state, &id, &q).iter().map(entry_json).collect();
    Json(json!({ "logs": found })).into_response()
}
