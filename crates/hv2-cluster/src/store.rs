//! Where cluster state lives, so that no control plane has to hold any.
//!
//! CubeMaster's shape: every control-plane instance reads and writes one
//! shared store, and any instance can serve any request. [`RedisStore`] is
//! that store (Redis or Valkey -- the protocol is the same); [`MemoryStore`]
//! is the same contract in one process, for tests and for a single host that
//! wants the control plane without running a database.
//!
//! Liveness is the store's job, not the reader's: a node's record carries a
//! time-to-live that each heartbeat renews, and [`ClusterStore::nodes`]
//! returns only nodes whose record has not expired. A control plane never
//! has to decide how stale is too stale.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

use crate::model::{ClusterEvent, Delivery, NodeInfo, SandboxRecord, Webhook};

/// A store error. Opaque on purpose: a caller's only decision is whether to
/// retry, and the message is for the log.
#[derive(Debug, thiserror::Error)]
#[error("cluster store: {0}")]
pub struct StoreError(pub String);

pub type Result<T> = std::result::Result<T, StoreError>;

/// The shared state of a cluster.
#[async_trait::async_trait]
pub trait ClusterStore: Send + Sync {
    /// Record `node`, alive for `ttl` unless renewed.
    async fn put_node(&self, node: &NodeInfo, ttl: Duration) -> Result<()>;
    /// Every node whose record has not expired.
    async fn nodes(&self) -> Result<Vec<NodeInfo>>;
    /// One node, if it is alive.
    async fn node(&self, id: &str) -> Result<Option<NodeInfo>>;
    /// Remove a node's record now, as it shuts down cleanly.
    async fn remove_node(&self, id: &str) -> Result<()>;

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()>;
    async fn sandbox(&self, id: &str) -> Result<Option<SandboxRecord>>;
    /// Remove a sandbox's record. Returns whether this call removed it, so
    /// that when two control planes reap the same sandbox exactly one of them
    /// reports it.
    async fn delete_sandbox(&self, id: &str) -> Result<bool>;
    async fn sandboxes(&self) -> Result<Vec<SandboxRecord>>;

    /// Append to the event stream, which keeps a bounded tail.
    async fn publish(&self, event: &ClusterEvent) -> Result<()>;
    /// The most recent `count` events, newest first.
    async fn events(&self, count: usize) -> Result<Vec<ClusterEvent>>;

    /// Register or replace a webhook.
    async fn put_webhook(&self, hook: &Webhook) -> Result<()>;
    /// Every webhook, oldest first.
    async fn webhooks(&self) -> Result<Vec<Webhook>>;
    /// Remove a webhook and its deliveries; whether there was one.
    async fn delete_webhook(&self, id: &str) -> Result<bool>;
    /// Keep a delivery attempt, of a bounded tail per webhook.
    async fn record_delivery(&self, delivery: &Delivery) -> Result<()>;
    /// A webhook's most recent `count` deliveries, newest first.
    async fn deliveries(&self, webhook_id: &str, count: usize) -> Result<Vec<Delivery>>;
}

/// Deliveries kept per webhook, at most.
pub const DELIVERY_TAIL: usize = 1_000;

/// Events kept, at most.
pub const EVENT_TAIL: usize = 10_000;

// ── In memory ───────────────────────────────────────────────────────────────

/// [`ClusterStore`] in one process.
#[derive(Default)]
pub struct MemoryStore {
    nodes: Mutex<HashMap<String, (NodeInfo, Instant)>>,
    sandboxes: Mutex<HashMap<String, SandboxRecord>>,
    events: Mutex<std::collections::VecDeque<ClusterEvent>>,
    webhooks: Mutex<Vec<Webhook>>,
    deliveries: Mutex<HashMap<String, std::collections::VecDeque<Delivery>>>,
}

impl MemoryStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl ClusterStore for MemoryStore {
    async fn put_node(&self, node: &NodeInfo, ttl: Duration) -> Result<()> {
        self.nodes
            .lock()
            .insert(node.id.clone(), (node.clone(), Instant::now() + ttl));
        Ok(())
    }

    async fn nodes(&self) -> Result<Vec<NodeInfo>> {
        let now = Instant::now();
        let mut nodes = self.nodes.lock();
        nodes.retain(|_, (_, expires)| *expires > now);
        let mut live: Vec<_> = nodes.values().map(|(n, _)| n.clone()).collect();
        live.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(live)
    }

    async fn node(&self, id: &str) -> Result<Option<NodeInfo>> {
        let now = Instant::now();
        Ok(self
            .nodes
            .lock()
            .get(id)
            .filter(|(_, expires)| *expires > now)
            .map(|(n, _)| n.clone()))
    }

    async fn remove_node(&self, id: &str) -> Result<()> {
        self.nodes.lock().remove(id);
        Ok(())
    }

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()> {
        self.sandboxes
            .lock()
            .insert(record.sandbox_id.clone(), record.clone());
        Ok(())
    }

    async fn sandbox(&self, id: &str) -> Result<Option<SandboxRecord>> {
        Ok(self.sandboxes.lock().get(id).cloned())
    }

    async fn delete_sandbox(&self, id: &str) -> Result<bool> {
        Ok(self.sandboxes.lock().remove(id).is_some())
    }

    async fn sandboxes(&self) -> Result<Vec<SandboxRecord>> {
        let mut all: Vec<_> = self.sandboxes.lock().values().cloned().collect();
        all.sort_by(|a, b| (a.started_at_ms, &a.sandbox_id).cmp(&(b.started_at_ms, &b.sandbox_id)));
        Ok(all)
    }

    async fn publish(&self, event: &ClusterEvent) -> Result<()> {
        let mut events = self.events.lock();
        if events.len() == EVENT_TAIL {
            events.pop_front();
        }
        events.push_back(event.clone());
        Ok(())
    }

    async fn events(&self, count: usize) -> Result<Vec<ClusterEvent>> {
        Ok(self
            .events
            .lock()
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect())
    }

    async fn put_webhook(&self, hook: &Webhook) -> Result<()> {
        let mut hooks = self.webhooks.lock();
        hooks.retain(|h| h.id != hook.id);
        hooks.push(hook.clone());
        hooks.sort_by_key(|h| h.created_ms);
        Ok(())
    }

    async fn webhooks(&self) -> Result<Vec<Webhook>> {
        Ok(self.webhooks.lock().clone())
    }

    async fn delete_webhook(&self, id: &str) -> Result<bool> {
        let mut hooks = self.webhooks.lock();
        let before = hooks.len();
        hooks.retain(|h| h.id != id);
        self.deliveries.lock().remove(id);
        Ok(hooks.len() != before)
    }

    async fn record_delivery(&self, delivery: &Delivery) -> Result<()> {
        let mut all = self.deliveries.lock();
        let tail = all.entry(delivery.webhook_id.clone()).or_default();
        if tail.len() == DELIVERY_TAIL {
            tail.pop_front();
        }
        tail.push_back(delivery.clone());
        Ok(())
    }

    async fn deliveries(&self, webhook_id: &str, count: usize) -> Result<Vec<Delivery>> {
        Ok(self
            .deliveries
            .lock()
            .get(webhook_id)
            .map(|t| t.iter().rev().take(count).cloned().collect())
            .unwrap_or_default())
    }
}

// ── Redis / Valkey ──────────────────────────────────────────────────────────

/// [`ClusterStore`] in Redis or Valkey.
///
/// ```text
///   hv2:{ns}:node:{id}      JSON NodeInfo, with a TTL each heartbeat renews
///   hv2:{ns}:nodes          set of node ids ever seen (pruned on read)
///   hv2:{ns}:sandbox:{id}   JSON SandboxRecord
///   hv2:{ns}:sandboxes      set of sandbox ids
///   hv2:{ns}:events         stream, trimmed to about EVENT_TAIL
/// ```
///
/// `ns` lets two clusters share one database without seeing each other.
#[derive(Clone)]
pub struct RedisStore {
    connection: redis::aio::ConnectionManager,
    prefix: String,
}

fn redis_error(e: redis::RedisError) -> StoreError {
    StoreError(e.to_string())
}

fn json_error(e: serde_json::Error) -> StoreError {
    StoreError(format!("a record did not decode: {e}"))
}

impl RedisStore {
    /// Connect to `url` (`redis://host:6379/0`), keeping keys under
    /// `namespace`.
    ///
    /// A `rediss://` URL is TLS. The server is verified against the system's
    /// roots, or against the CA in the PEM file `HV2_STORE_CA` names -- a
    /// private store's usual case; `HV2_STORE_CERT` and `HV2_STORE_KEY`, both
    /// or neither, add a client certificate for a server that asks for one.
    ///
    /// # Errors
    ///
    /// The URL does not parse, a named file cannot be read, or the first
    /// connection fails.
    pub async fn connect(url: &str, namespace: &str) -> Result<Self> {
        let client = if url.starts_with("rediss://") {
            // The workspace's one provider, for a crate that asks for the
            // process default. Already installed is fine.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let read = |var: &str| -> Result<Option<Vec<u8>>> {
                match std::env::var_os(var) {
                    None => Ok(None),
                    Some(path) => std::fs::read(&path)
                        .map(Some)
                        .map_err(|e| StoreError(format!("{var}={}: {e}", path.to_string_lossy()))),
                }
            };
            let client_tls = match (read("HV2_STORE_CERT")?, read("HV2_STORE_KEY")?) {
                (Some(client_cert), Some(client_key)) => Some(redis::ClientTlsConfig {
                    client_cert,
                    client_key,
                }),
                (None, None) => None,
                _ => {
                    return Err(StoreError(
                        "HV2_STORE_CERT and HV2_STORE_KEY go together".into(),
                    ))
                }
            };
            redis::Client::build_with_tls(
                url,
                redis::TlsCertificates {
                    client_tls,
                    root_cert: read("HV2_STORE_CA")?,
                },
            )
            .map_err(redis_error)?
        } else {
            redis::Client::open(url).map_err(redis_error)?
        };
        let connection = client.get_connection_manager().await.map_err(redis_error)?;
        Ok(Self {
            connection,
            prefix: format!("hv2:{namespace}"),
        })
    }

    fn key(&self, rest: &str) -> String {
        format!("{}:{rest}", self.prefix)
    }
}

#[async_trait::async_trait]
impl ClusterStore for RedisStore {
    async fn put_node(&self, node: &NodeInfo, ttl: Duration) -> Result<()> {
        let json = serde_json::to_string(node).map_err(json_error)?;
        let mut c = self.connection.clone();
        redis::pipe()
            .atomic()
            .set_ex(
                self.key(&format!("node:{}", node.id)),
                json,
                ttl.as_secs().max(1),
            )
            .sadd(self.key("nodes"), &node.id)
            .query_async::<()>(&mut c)
            .await
            .map_err(redis_error)
    }

    async fn nodes(&self) -> Result<Vec<NodeInfo>> {
        let mut c = self.connection.clone();
        let ids: Vec<String> = redis::cmd("SMEMBERS")
            .arg(self.key("nodes"))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<String> = ids
            .iter()
            .map(|id| self.key(&format!("node:{id}")))
            .collect();
        let values: Vec<Option<String>> = redis::cmd("MGET")
            .arg(&keys)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;

        let mut live = Vec::new();
        let mut expired = Vec::new();
        for (id, value) in ids.into_iter().zip(values) {
            match value {
                Some(json) => {
                    live.push(serde_json::from_str::<NodeInfo>(&json).map_err(json_error)?);
                }
                None => expired.push(id),
            }
        }
        if !expired.is_empty() {
            // A node whose record expired is gone; forget it so the set does
            // not grow with every node that ever joined.
            let _: std::result::Result<(), _> = redis::cmd("SREM")
                .arg(self.key("nodes"))
                .arg(&expired)
                .query_async(&mut c)
                .await;
        }
        live.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(live)
    }

    async fn node(&self, id: &str) -> Result<Option<NodeInfo>> {
        let mut c = self.connection.clone();
        let value: Option<String> = redis::cmd("GET")
            .arg(self.key(&format!("node:{id}")))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        value
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    async fn remove_node(&self, id: &str) -> Result<()> {
        let mut c = self.connection.clone();
        redis::pipe()
            .atomic()
            .del(self.key(&format!("node:{id}")))
            .srem(self.key("nodes"), id)
            .query_async::<()>(&mut c)
            .await
            .map_err(redis_error)
    }

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()> {
        let json = serde_json::to_string(record).map_err(json_error)?;
        let mut c = self.connection.clone();
        redis::pipe()
            .atomic()
            .set(self.key(&format!("sandbox:{}", record.sandbox_id)), json)
            .sadd(self.key("sandboxes"), &record.sandbox_id)
            .query_async::<()>(&mut c)
            .await
            .map_err(redis_error)
    }

    async fn sandbox(&self, id: &str) -> Result<Option<SandboxRecord>> {
        let mut c = self.connection.clone();
        let value: Option<String> = redis::cmd("GET")
            .arg(self.key(&format!("sandbox:{id}")))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        value
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    async fn delete_sandbox(&self, id: &str) -> Result<bool> {
        let mut c = self.connection.clone();
        let (deleted, _): (u32, u32) = redis::pipe()
            .atomic()
            .del(self.key(&format!("sandbox:{id}")))
            .srem(self.key("sandboxes"), id)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(deleted > 0)
    }

    async fn sandboxes(&self) -> Result<Vec<SandboxRecord>> {
        let mut c = self.connection.clone();
        let ids: Vec<String> = redis::cmd("SMEMBERS")
            .arg(self.key("sandboxes"))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let keys: Vec<String> = ids
            .iter()
            .map(|id| self.key(&format!("sandbox:{id}")))
            .collect();
        let values: Vec<Option<String>> = redis::cmd("MGET")
            .arg(&keys)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        let mut all = Vec::new();
        for json in values.into_iter().flatten() {
            all.push(serde_json::from_str::<SandboxRecord>(&json).map_err(json_error)?);
        }
        all.sort_by(|a, b| (a.started_at_ms, &a.sandbox_id).cmp(&(b.started_at_ms, &b.sandbox_id)));
        Ok(all)
    }

    async fn publish(&self, event: &ClusterEvent) -> Result<()> {
        let json = serde_json::to_string(event).map_err(json_error)?;
        let mut c = self.connection.clone();
        redis::cmd("XADD")
            .arg(self.key("events"))
            .arg("MAXLEN")
            .arg("~")
            .arg(EVENT_TAIL)
            .arg("*")
            .arg("event")
            .arg(json)
            .query_async::<String>(&mut c)
            .await
            .map(|_| ())
            .map_err(redis_error)
    }

    async fn events(&self, count: usize) -> Result<Vec<ClusterEvent>> {
        let mut c = self.connection.clone();
        let reply: redis::streams::StreamRangeReply = redis::cmd("XREVRANGE")
            .arg(self.key("events"))
            .arg("+")
            .arg("-")
            .arg("COUNT")
            .arg(count)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        let mut events = Vec::new();
        for entry in reply.ids {
            if let Some(json) = entry.get::<String>("event") {
                events.push(serde_json::from_str(&json).map_err(json_error)?);
            }
        }
        Ok(events)
    }

    async fn put_webhook(&self, hook: &Webhook) -> Result<()> {
        let json = serde_json::to_string(hook).map_err(json_error)?;
        let mut c = self.connection.clone();
        redis::cmd("HSET")
            .arg(self.key("webhooks"))
            .arg(&hook.id)
            .arg(json)
            .query_async::<()>(&mut c)
            .await
            .map_err(redis_error)
    }

    async fn webhooks(&self) -> Result<Vec<Webhook>> {
        let mut c = self.connection.clone();
        let all: HashMap<String, String> = redis::cmd("HGETALL")
            .arg(self.key("webhooks"))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        let mut hooks: Vec<Webhook> = all
            .values()
            .filter_map(|json| serde_json::from_str(json).ok())
            .collect();
        hooks.sort_by_key(|h| h.created_ms);
        Ok(hooks)
    }

    async fn delete_webhook(&self, id: &str) -> Result<bool> {
        let mut c = self.connection.clone();
        let removed: u64 = redis::cmd("HDEL")
            .arg(self.key("webhooks"))
            .arg(id)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        let _: u64 = redis::cmd("DEL")
            .arg(self.key(&format!("deliveries:{id}")))
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(removed > 0)
    }

    async fn record_delivery(&self, delivery: &Delivery) -> Result<()> {
        let json = serde_json::to_string(delivery).map_err(json_error)?;
        let key = self.key(&format!("deliveries:{}", delivery.webhook_id));
        let mut c = self.connection.clone();
        redis::pipe()
            .cmd("LPUSH")
            .arg(&key)
            .arg(json)
            .ignore()
            .cmd("LTRIM")
            .arg(&key)
            .arg(0)
            .arg(DELIVERY_TAIL - 1)
            .ignore()
            .query_async::<()>(&mut c)
            .await
            .map_err(redis_error)
    }

    async fn deliveries(&self, webhook_id: &str, count: usize) -> Result<Vec<Delivery>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let mut c = self.connection.clone();
        let all: Vec<String> = redis::cmd("LRANGE")
            .arg(self.key(&format!("deliveries:{webhook_id}")))
            .arg(0)
            .arg(count - 1)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(all
            .iter()
            .filter_map(|json| serde_json::from_str(json).ok())
            .collect())
    }
}

/// Open the store `url` names: `memory:` for [`MemoryStore`], `redis://` or
/// `rediss://` for [`RedisStore`].
///
/// # Errors
///
/// An unknown scheme, or a Redis store that cannot be reached.
pub async fn open(url: &str, namespace: &str) -> Result<std::sync::Arc<dyn ClusterStore>> {
    if url == "memory:" {
        return Ok(std::sync::Arc::new(MemoryStore::new()));
    }
    if url.starts_with("redis://") || url.starts_with("rediss://") {
        return Ok(std::sync::Arc::new(
            RedisStore::connect(url, namespace).await?,
        ));
    }
    Err(StoreError(format!(
        "unknown store '{url}': use memory: or redis://host:port"
    )))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::now_ms;
    use serde_json::json;

    pub(crate) fn node(id: &str, running: u32, capacity: u32) -> NodeInfo {
        NodeInfo {
            id: id.into(),
            api: format!("http://{id}:3980"),
            proxy: "127.0.0.1:3981".parse().unwrap(),
            capacity,
            running,
            heartbeat_ms: now_ms(),
            version: "test".into(),
            jwk: None,
            templates: Vec::new(),
        }
    }

    pub(crate) fn sandbox(id: &str, node: &str) -> SandboxRecord {
        SandboxRecord {
            sandbox_id: id.into(),
            node_id: node.into(),
            template_id: "base".into(),
            started_at_ms: now_ms(),
            end_at_ms: now_ms() + 60_000,
            cpu_count: 1,
            memory_mb: 1024,
            metadata: Default::default(),
            envd_version: "0.6.3".into(),
            descriptor: json!({"sandboxID": id}),
            paused: false,
            portable: false,
            volume_mounts: Vec::new(),
        }
    }

    /// The contract, run against any store.
    pub(crate) async fn contract(store: &dyn ClusterStore) {
        // Nodes expire unless renewed.
        store
            .put_node(&node("a", 0, 4), Duration::from_secs(60))
            .await
            .unwrap();
        store
            .put_node(&node("b", 1, 4), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(store.nodes().await.unwrap().len(), 2);
        tokio::time::sleep(Duration::from_millis(2100)).await;
        let live: Vec<_> = store
            .nodes()
            .await
            .unwrap()
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(live, vec!["a".to_string()], "b's record expired");
        assert!(store.node("b").await.unwrap().is_none());
        store.remove_node("a").await.unwrap();
        assert!(store.nodes().await.unwrap().is_empty());

        // Sandboxes round-trip, and exactly one delete wins.
        store.put_sandbox(&sandbox("s1", "a")).await.unwrap();
        store.put_sandbox(&sandbox("s2", "a")).await.unwrap();
        assert_eq!(store.sandbox("s1").await.unwrap().unwrap().node_id, "a");
        assert_eq!(store.sandboxes().await.unwrap().len(), 2);
        assert!(store.delete_sandbox("s1").await.unwrap());
        assert!(!store.delete_sandbox("s1").await.unwrap(), "already gone");
        assert!(store.sandbox("s1").await.unwrap().is_none());

        // Events come back newest first.
        for kind in ["one", "two", "three"] {
            store
                .publish(&ClusterEvent::new(kind, "a", None))
                .await
                .unwrap();
        }
        let kinds: Vec<_> = store
            .events(2)
            .await
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, vec!["three".to_string(), "two".to_string()]);

        // Webhooks, and their deliveries newest first.
        let hook = crate::model::Webhook {
            id: format!("wh-{}", uuid::Uuid::new_v4()),
            name: "n".into(),
            url: "https://example.com/hook".into(),
            events: vec![],
            enabled: true,
            secret: "s".into(),
            created_ms: 1,
        };
        store.put_webhook(&hook).await.unwrap();
        assert!(store.webhooks().await.unwrap().contains(&hook));
        for n in 0..3u64 {
            let delivery = crate::model::Delivery {
                id: n.to_string(),
                webhook_id: hook.id.clone(),
                event_id: "e".into(),
                sandbox_id: "s".into(),
                event_type: "sandbox.lifecycle.created".into(),
                status: "success".into(),
                duration_ms: n,
                request_body: String::new(),
                request_url: hook.url.clone(),
                response_status: Some(200),
                response_body: None,
                error_class: None,
                error_message: None,
                at_ms: n,
            };
            store.record_delivery(&delivery).await.unwrap();
        }
        let ids: Vec<_> = store
            .deliveries(&hook.id, 2)
            .await
            .unwrap()
            .into_iter()
            .map(|d| d.id)
            .collect();
        assert_eq!(ids, ["2", "1"]);
        assert!(store.delete_webhook(&hook.id).await.unwrap());
        assert!(!store.webhooks().await.unwrap().contains(&hook));
        assert!(store.deliveries(&hook.id, 10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn memory_store_keeps_the_contract() {
        contract(&MemoryStore::new()).await;
    }

    /// Against a real Redis or Valkey, when one is named. Each run uses its
    /// own namespace so a shared database is not disturbed.
    #[tokio::test]
    async fn redis_store_keeps_the_contract() {
        let Ok(url) = std::env::var("HV2_TEST_REDIS") else {
            eprintln!("skipped: set HV2_TEST_REDIS=redis://127.0.0.1:6379 to run");
            return;
        };
        let namespace = format!("test-{}", now_ms());
        let store = RedisStore::connect(&url, &namespace).await.unwrap();
        contract(&store).await;
    }
}
