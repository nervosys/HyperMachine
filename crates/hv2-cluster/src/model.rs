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
    /// The public key this node signs sandboxes' workload tokens with, as
    /// a JWK -- what a control plane's JWKS publishes, so a token from any
    /// node verifies against the cluster's issuer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwk: Option<Value>,
    /// The templates this node can start sandboxes from. Empty from a node
    /// older than templates, which offered `base` alone.
    #[serde(default)]
    pub templates: Vec<String>,
}

impl NodeInfo {
    /// Whether this node can start a sandbox from `template`.
    #[must_use]
    pub fn offers(&self, template: &str) -> bool {
        let template = untagged(template);
        if self.templates.is_empty() {
            template == "base"
        } else {
            self.templates.iter().any(|t| t == template)
        }
    }

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
    /// Suspended to its node's disk rather than running: it holds no VM and
    /// counts against no capacity until something resumes it.
    #[serde(default)]
    pub paused: bool,
    /// Paused into a snapshot store every node shares, so any node can
    /// resume it: it outlives the node that paused it, and a request for it
    /// goes to whichever node has room.
    #[serde(default)]
    pub portable: bool,
    /// Volumes mounted in it, remounted wherever it is resumed or forked.
    #[serde(
        default,
        rename = "volumeMounts",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub volume_mounts: Vec<VolumeMount>,
}

/// The ID of the volume named `name`: derived, not drawn, so a control
/// plane knows which node holds a volume from its name as from its ID.
#[must_use]
pub fn volume_id(name: &str) -> String {
    let fnv = |seed: u64| {
        name.bytes().fold(seed, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    format!(
        "vol-{:016x}",
        fnv(0xcbf2_9ce4_8422_2325) ^ fnv(0x8422_2325_cbf2_9ce4).rotate_left(29)
    )
}

/// A volume, by name, mounted at a path in a sandbox: E2B's
/// `SandboxVolumeMount`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VolumeMount {
    pub name: String,
    pub path: String,
}

impl SandboxRecord {
    /// Whether this sandbox survives its node: paused, into shared storage.
    #[must_use]
    pub fn survives_its_node(&self) -> bool {
        self.paused && self.portable
    }

    /// E2B's `SandboxState`: `running` or `paused`.
    #[must_use]
    pub fn state(&self) -> &'static str {
        if self.paused {
            "paused"
        } else {
            "running"
        }
    }

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
            "state": self.state(),
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
    /// Unique, so a webhook delivery names the event it carried.
    #[serde(default)]
    pub id: String,
    pub at_ms: u64,
    /// `node-joined`, `sandbox-created`, `sandbox-deleted`, `sandbox-expired`,
    /// `sandbox-lost`.
    pub kind: String,
    pub node_id: String,
    #[serde(default)]
    pub sandbox_id: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
    /// The template of the sandbox it concerns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
}

impl ClusterEvent {
    #[must_use]
    pub fn new(kind: &str, node_id: &str, sandbox_id: Option<&str>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            at_ms: now_ms(),
            kind: kind.to_string(),
            node_id: node_id.to_string(),
            sandbox_id: sandbox_id.map(str::to_string),
            detail: None,
            template_id: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    #[must_use]
    pub fn with_template(mut self, template_id: impl Into<String>) -> Self {
        self.template_id = Some(template_id.into());
        self
    }
}

/// A webhook: where a cluster sends the sandbox events it subscribes to,
/// signed with its secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub name: String,
    pub url: String,
    /// E2B event types (`sandbox.lifecycle.created`, ...); empty is all.
    pub events: Vec<String>,
    pub enabled: bool,
    /// Signs each payload; never returned by the API.
    pub secret: String,
    pub created_ms: u64,
}

/// One attempt to deliver an event to a webhook: E2B's `WebhookDelivery`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Delivery {
    pub id: String,
    pub webhook_id: String,
    pub event_id: String,
    pub sandbox_id: String,
    pub event_type: String,
    /// `success` or `failed`.
    pub status: String,
    pub duration_ms: u64,
    pub request_body: String,
    pub request_url: String,
    pub response_status: Option<u16>,
    pub response_body: Option<String>,
    /// E2B's `errorClass`: `http_error`, `dns_error`, `timeout`,
    /// `transport_error`, `request_error`.
    pub error_class: Option<String>,
    pub error_message: Option<String>,
    pub at_ms: u64,
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

/// A template name as E2B's clients may spell it -- `team/name:tag` -- as
/// a node names it. Template names hold neither `/` nor `:`.
#[must_use]
pub fn untagged(template: &str) -> &str {
    let name = template.rsplit_once('/').map_or(template, |(_, name)| name);
    name.split_once(':').map_or(name, |(name, _)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_names_as_e2b_spells_them() {
        assert_eq!(untagged("my-snap"), "my-snap");
        assert_eq!(untagged("my-snap:default"), "my-snap");
        assert_eq!(untagged("team/my-snap:v2"), "my-snap");
        assert_eq!(untagged("team/my-snap"), "my-snap");
    }

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
            paused: false,
            portable: false,
            volume_mounts: Vec::new(),
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

    /// A paused sandbox lists as `paused`, which the SDK filters on; and a
    /// record written before the field existed reads as running.
    #[test]
    fn a_paused_record_lists_as_paused() {
        let mut r = record();
        assert_eq!(r.listed()["state"], "running");
        r.paused = true;
        assert_eq!(r.listed()["state"], "paused");
        let mut old = serde_json::to_value(record()).expect("encodes");
        old.as_object_mut().expect("an object").remove("paused");
        let old: SandboxRecord = serde_json::from_value(old).expect("still decodes");
        assert!(!old.paused);
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
