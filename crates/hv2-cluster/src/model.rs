//! What the cluster stores: nodes, sandboxes, and what happened to them.
//!
//! One shape, written by nodes and read by control planes, and the E2B wire
//! shapes built from it in one place so a node answering on its own and a
//! control plane answering for the cluster cannot describe a sandbox two
//! different ways.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Milliseconds since the Unix epoch, now.
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// RFC 3339 with milliseconds, UTC -- what E2B's `date-time` fields carry.
#[must_use]
pub fn rfc3339(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// A node, as it last reported itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    /// Base URL of the node's E2B API, e.g. `http://10.0.0.5:3980`.
    pub api: String,
    /// The node's envd proxy.
    pub proxy: SocketAddr,
    /// Sandboxes it will run at once.
    pub capacity: u32,
    /// Sandboxes it was running at its last heartbeat.
    pub running: u32,
    pub heartbeat_ms: u64,
    pub version: String,
}

impl NodeInfo {
    /// Room for one more, as of its last heartbeat. The node itself is the
    /// authority -- it refuses a create it has no room for -- so this is a
    /// scheduling hint and a stale one is safe.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.running < self.capacity
    }
}

/// A sandbox, as the node running it recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxRecord {
    pub sandbox_id: String,
    pub node_id: String,
    pub template_id: String,
    pub started_at_ms: u64,
    pub end_at_ms: u64,
    pub cpu_count: u32,
    pub memory_mb: u64,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub envd_version: String,
    /// What `POST /sandboxes` answered with, verbatim, so `connect` and
    /// detail can repeat it rather than rebuild it.
    pub descriptor: Value,
}

impl SandboxRecord {
    /// E2B's `ListedSandbox`.
    #[must_use]
    pub fn listed(&self) -> Value {
        json!({
            "templateID": self.template_id,
            "sandboxID": self.sandbox_id,
            "clientID": self.sandbox_id,
            "startedAt": rfc3339(self.started_at_ms),
            "endAt": rfc3339(self.end_at_ms),
            "cpuCount": self.cpu_count,
            "memoryMB": self.memory_mb,
            // No disk of its own: the root filesystem is an initramfs in RAM.
            "diskSizeMB": 0,
            "metadata": self.metadata,
            "state": "running",
            "envdVersion": self.envd_version,
            // Not E2B's; which node runs it, for an operator.
            "nodeID": self.node_id,
        })
    }

    /// E2B's `SandboxDetail`: `ListedSandbox` plus the access token and
    /// domain a client needs to reach it.
    #[must_use]
    pub fn detail(&self) -> Value {
        let mut detail = self.listed();
        if let (Some(detail), Some(descriptor)) =
            (detail.as_object_mut(), self.descriptor.as_object())
        {
            for key in ["envdAccessToken", "domain"] {
                if let Some(value) = descriptor.get(key) {
                    detail.insert(key.to_string(), value.clone());
                }
            }
        }
        detail
    }
}

/// Something that happened, for the event stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterEvent {
    pub at_ms: u64,
    /// `node-joined`, `sandbox-created`, `sandbox-deleted`, `sandbox-expired`,
    /// `sandbox-lost`.
    pub kind: String,
    pub node_id: String,
    #[serde(default)]
    pub sandbox_id: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

impl ClusterEvent {
    #[must_use]
    pub fn new(kind: &str, node_id: &str, sandbox_id: Option<&str>) -> Self {
        Self {
            at_ms: now_ms(),
            kind: kind.to_string(),
            node_id: node_id.to_string(),
            sandbox_id: sandbox_id.map(str::to_string),
            detail: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// E2B's `metadata` query parameter: a URL-encoded `key=value&key=value`
/// string. A sandbox matches if it carries every pair.
#[must_use]
pub fn parse_metadata_query(query: &str) -> BTreeMap<String, String> {
    form_urlencoded::parse(query.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

/// Does `record` carry every pair in `wanted`?
#[must_use]
pub fn metadata_matches(record: &SandboxRecord, wanted: &BTreeMap<String, String>) -> bool {
    wanted
        .iter()
        .all(|(k, v)| record.metadata.get(k).is_some_and(|have| have == v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> SandboxRecord {
        SandboxRecord {
            sandbox_id: "sbx-1".into(),
            node_id: "node-a".into(),
            template_id: "base".into(),
            started_at_ms: 1_700_000_000_000,
            end_at_ms: 1_700_000_300_000,
            cpu_count: 2,
            memory_mb: 1024,
            metadata: [("team".to_string(), "red".to_string())].into(),
            envd_version: "0.6.3".into(),
            descriptor: json!({"envdAccessToken": "tok", "sandboxID": "sbx-1"}),
        }
    }

    /// Every field E2B's `ListedSandbox` requires, because the SDK's
    /// generated parser raises on a missing one.
    #[test]
    fn listed_carries_every_required_field() {
        let listed = record().listed();
        for field in [
            "templateID",
            "sandboxID",
            "clientID",
            "startedAt",
            "cpuCount",
            "memoryMB",
            "diskSizeMB",
            "endAt",
            "state",
            "envdVersion",
        ] {
            assert!(listed.get(field).is_some(), "{field}");
        }
        assert_eq!(listed["startedAt"], "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn detail_adds_the_token_and_where_it_runs() {
        let detail = record().detail();
        assert_eq!(detail["envdAccessToken"], "tok");
        assert_eq!(detail["nodeID"], "node-a");
    }

    #[test]
    fn metadata_filters_need_every_pair() {
        let r = record();
        assert!(metadata_matches(&r, &parse_metadata_query("team=red")));
        assert!(!metadata_matches(
            &r,
            &parse_metadata_query("team=red&env=prod")
        ));
        assert!(!metadata_matches(&r, &parse_metadata_query("team=blue")));
        assert!(metadata_matches(&r, &parse_metadata_query("")));
    }
}
