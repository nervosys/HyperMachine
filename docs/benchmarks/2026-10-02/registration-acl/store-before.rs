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

use crate::domains::{DomainBinding, DomainName};
use crate::names::{NameReservation, SandboxName};

use crate::model::{ClusterEvent, Delivery, NodeInfo, SandboxRecord, Webhook};

/// A store error. Opaque on purpose: a caller's only decision is whether to
/// retry, and the message is for the log.
#[derive(Debug, thiserror::Error)]
#[error("cluster store: {0}")]
pub struct StoreError(pub String);

pub type Result<T> = std::result::Result<T, StoreError>;

/// Outcome of an atomic domain ownership claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainClaim {
    Claimed,
    Conflict,
    SandboxMissing,
}

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
    /// Publish a new sandbox and bind its pending name in one transaction.
    /// Exact same-owner/record replay succeeds without overwriting updates.
    /// False leaves both record and ownership unchanged. No ID or owner transfer.
    async fn register_named_sandbox(
        &self,
        record: &SandboxRecord,
        reservation: &NameReservation,
    ) -> Result<bool>;
    async fn sandbox(&self, id: &str) -> Result<Option<SandboxRecord>>;
    /// Remove a sandbox's record. Returns whether this call removed it, so
    /// that when two control planes reap the same sandbox exactly one of them
    /// reports it.
    async fn delete_sandbox(&self, id: &str) -> Result<bool>;
    async fn sandboxes(&self) -> Result<Vec<SandboxRecord>>;

    /// Atomically reserve pending ownership. True includes same-token replay.
    /// Bound reservations must be created through the atomic bind operation.
    async fn reserve_name(&self, reservation: &NameReservation) -> Result<bool>;
    async fn name_reservation(&self, name: &SandboxName) -> Result<Option<NameReservation>>;
    /// Bind matching pending ownership to an existing sandbox; same-target replay succeeds.
    async fn bind_name(&self, name: &SandboxName, token: &str, sandbox: &str) -> Result<bool>;
    /// Release pending ownership only with its matching operation token.
    async fn release_pending_name(&self, name: &SandboxName, token: &str) -> Result<bool>;

    /// Claim a hostname, or update its port for the same sandbox. No ownership transfer.
    async fn claim_domain(&self, binding: &DomainBinding) -> Result<DomainClaim>;
    async fn domain(&self, name: &DomainName) -> Result<Option<DomainBinding>>;
    async fn domains(&self, sandbox: &str) -> Result<Vec<DomainBinding>>;
    /// Remove only if the hostname still belongs to this sandbox.
    async fn delete_domain(&self, name: &DomainName, sandbox: &str) -> Result<bool>;

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
    sandboxes: Mutex<SandboxState>,
    events: Mutex<std::collections::VecDeque<ClusterEvent>>,
    webhooks: Mutex<Vec<Webhook>>,
    deliveries: Mutex<HashMap<String, std::collections::VecDeque<Delivery>>>,
}

#[derive(Default)]
struct SandboxState {
    records: HashMap<String, SandboxRecord>,
    domains: HashMap<DomainName, DomainBinding>,
    names: HashMap<SandboxName, NameReservation>,
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
            .records
            .insert(record.sandbox_id.clone(), record.clone());
        Ok(())
    }

    async fn register_named_sandbox(
        &self,
        record: &SandboxRecord,
        reservation: &NameReservation,
    ) -> Result<bool> {
        if reservation.sandbox_id().is_some()
            || record.metadata.get("hm.name").map(String::as_str)
                != Some(reservation.name().as_str())
        {
            return Ok(false);
        }
        let mut state = self.sandboxes.lock();
        let Some(existing) = state.names.get(reservation.name()) else {
            return Ok(false);
        };
        let mut bound = existing.clone();
        if bound
            .bind(reservation.operation_token(), &record.sandbox_id)
            .is_err()
        {
            return Ok(false);
        }
        if let Some(old) = state.records.get(&record.sandbox_id) {
            return Ok(existing.sandbox_id() == Some(record.sandbox_id.as_str()) && old == record);
        }
        if existing.sandbox_id().is_some() {
            return Ok(false);
        }
        state
            .records
            .insert(record.sandbox_id.clone(), record.clone());
        state.names.insert(reservation.name().clone(), bound);
        Ok(true)
    }

    async fn sandbox(&self, id: &str) -> Result<Option<SandboxRecord>> {
        Ok(self.sandboxes.lock().records.get(id).cloned())
    }

    async fn delete_sandbox(&self, id: &str) -> Result<bool> {
        let mut state = self.sandboxes.lock();
        state
            .domains
            .retain(|_, binding| binding.sandbox_id() != id);
        state
            .names
            .retain(|_, reservation| reservation.sandbox_id() != Some(id));
        Ok(state.records.remove(id).is_some())
    }

    async fn sandboxes(&self) -> Result<Vec<SandboxRecord>> {
        let mut all: Vec<_> = self.sandboxes.lock().records.values().cloned().collect();
        all.sort_by(|a, b| (a.started_at_ms, &a.sandbox_id).cmp(&(b.started_at_ms, &b.sandbox_id)));
        Ok(all)
    }

    async fn reserve_name(&self, reservation: &NameReservation) -> Result<bool> {
        if reservation.sandbox_id().is_some() {
            return Err(StoreError(
                "only pending name reservations can be inserted".into(),
            ));
        }
        let mut state = self.sandboxes.lock();
        if let Some(existing) = state.names.get(reservation.name()) {
            return Ok(existing == reservation);
        }
        state
            .names
            .insert(reservation.name().clone(), reservation.clone());
        Ok(true)
    }
    async fn bind_name(&self, name: &SandboxName, token: &str, sandbox: &str) -> Result<bool> {
        let mut state = self.sandboxes.lock();
        if !state.records.contains_key(sandbox) {
            return Ok(false);
        }
        let Some(reservation) = state.names.get_mut(name) else {
            return Ok(false);
        };
        Ok(reservation.bind(token, sandbox).is_ok())
    }

    async fn name_reservation(&self, name: &SandboxName) -> Result<Option<NameReservation>> {
        Ok(self.sandboxes.lock().names.get(name).cloned())
    }
    async fn release_pending_name(&self, name: &SandboxName, token: &str) -> Result<bool> {
        let mut state = self.sandboxes.lock();
        if state
            .names
            .get(name)
            .is_some_and(|r| r.sandbox_id().is_none() && r.operation_token() == token)
        {
            state.names.remove(name);
            return Ok(true);
        }
        Ok(false)
    }

    async fn claim_domain(&self, binding: &DomainBinding) -> Result<DomainClaim> {
        let mut state = self.sandboxes.lock();
        if !state.records.contains_key(binding.sandbox_id()) {
            return Ok(DomainClaim::SandboxMissing);
        }
        if state
            .domains
            .get(binding.domain())
            .is_some_and(|old| old.sandbox_id() != binding.sandbox_id())
        {
            return Ok(DomainClaim::Conflict);
        }
        state
            .domains
            .insert(binding.domain().clone(), binding.clone());
        Ok(DomainClaim::Claimed)
    }

    async fn domain(&self, name: &DomainName) -> Result<Option<DomainBinding>> {
        Ok(self.sandboxes.lock().domains.get(name).cloned())
    }

    async fn domains(&self, sandbox: &str) -> Result<Vec<DomainBinding>> {
        let mut bindings: Vec<_> = self
            .sandboxes
            .lock()
            .domains
            .values()
            .filter(|binding| binding.sandbox_id() == sandbox)
            .cloned()
            .collect();
        bindings.sort_by(|a, b| a.domain().cmp(b.domain()));
        Ok(bindings)
    }

    async fn delete_domain(&self, name: &DomainName, sandbox: &str) -> Result<bool> {
        let mut state = self.sandboxes.lock();
        if state
            .domains
            .get(name)
            .is_some_and(|binding| binding.sandbox_id() == sandbox)
        {
            state.domains.remove(name);
            Ok(true)
        } else {
            Ok(false)
        }
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

    async fn register_named_sandbox(
        &self,
        record: &SandboxRecord,
        reservation: &NameReservation,
    ) -> Result<bool> {
        if reservation.sandbox_id().is_some()
            || record.metadata.get("hm.name").map(String::as_str)
                != Some(reservation.name().as_str())
        {
            return Ok(false);
        }
        let mut bound = reservation.clone();
        if bound
            .bind(reservation.operation_token(), &record.sandbox_id)
            .is_err()
        {
            return Ok(false);
        }
        let mut c = self.connection.clone();
        let result: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local existing = redis.call('HGET', KEYS[1], ARGV[1])
if not existing then return 0 end
local value = cjson.decode(existing)
if value.name ~= ARGV[1] or value.token ~= ARGV[2] then return 0 end
if value.sandbox_id ~= cjson.null and value.sandbox_id ~= ARGV[3] then return 0 end
local record = redis.call('GET', KEYS[2])
if record then
    if value.sandbox_id == ARGV[3] and record == ARGV[4] then return 1 end
    return 0
end
if value.sandbox_id ~= cjson.null then return 0 end
-- Check index types before writes: Lua runtime errors do not roll back writes.
for i = 3, 4 do
    local kind = redis.call('TYPE', KEYS[i]).ok
    if kind ~= 'none' and kind ~= 'set' then
        return redis.error_reply('named registration index type mismatch')
    end
end
redis.call('SET', KEYS[2], ARGV[4])
redis.call('SADD', KEYS[3], ARGV[3])
redis.call('HSET', KEYS[1], ARGV[1], ARGV[5])
redis.call('SADD', KEYS[4], ARGV[1])
return 1
"#,
            )
            .arg(4)
            .arg(self.key("name-reservations"))
            .arg(self.key(&format!("sandbox:{}", record.sandbox_id)))
            .arg(self.key("sandboxes"))
            .arg(self.key(&format!("reserved-names:{}", record.sandbox_id)))
            .arg(reservation.name().as_str())
            .arg(reservation.operation_token())
            .arg(&record.sandbox_id)
            .arg(serde_json::to_string(record).map_err(json_error)?)
            .arg(serde_json::to_string(&bound).map_err(json_error)?)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(result == 1)
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
        let deleted: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local names = redis.call('SMEMBERS', KEYS[3])
for _, name in ipairs(names) do
    local value = redis.call('HGET', KEYS[4], name)
    if value and cjson.decode(value).sandbox_id == ARGV[1] then
        redis.call('HDEL', KEYS[4], name)
    end
end
redis.call('DEL', KEYS[3])
local reserved = redis.call('SMEMBERS', KEYS[5])
for _, name in ipairs(reserved) do
    local value = redis.call('HGET', KEYS[6], name)
    if value and cjson.decode(value).sandbox_id == ARGV[1] then
        redis.call('HDEL', KEYS[6], name)
    end
end
redis.call('DEL', KEYS[5])
local deleted = redis.call('DEL', KEYS[1])
redis.call('SREM', KEYS[2], ARGV[1])
return deleted
"#,
            )
            .arg(6)
            .arg(self.key(&format!("sandbox:{id}")))
            .arg(self.key("sandboxes"))
            .arg(self.key(&format!("domains:{id}")))
            .arg(self.key("domains"))
            .arg(self.key(&format!("reserved-names:{id}")))
            .arg(self.key("name-reservations"))
            .arg(id)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(deleted > 0)
    }

    async fn reserve_name(&self, reservation: &NameReservation) -> Result<bool> {
        if reservation.sandbox_id().is_some() {
            return Err(StoreError(
                "only pending name reservations can be inserted".into(),
            ));
        }
        let mut c = self.connection.clone();
        let result: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local existing = redis.call('HGET', KEYS[1], ARGV[1])
if existing then
    local value = cjson.decode(existing)
    if value.token == ARGV[2] and value.sandbox_id == cjson.null then return 1 end
    return 0
end
redis.call('HSET', KEYS[1], ARGV[1], ARGV[3])
return 1
"#,
            )
            .arg(1)
            .arg(self.key("name-reservations"))
            .arg(reservation.name().as_str())
            .arg(reservation.operation_token())
            .arg(serde_json::to_string(reservation).map_err(json_error)?)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(result == 1)
    }
    async fn bind_name(&self, name: &SandboxName, token: &str, sandbox: &str) -> Result<bool> {
        let Some(mut reservation) = self.name_reservation(name).await? else {
            return Ok(false);
        };
        if reservation.bind(token, sandbox).is_err() {
            return Ok(false);
        }
        let mut c = self.connection.clone();
        let result: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local existing = redis.call('HGET', KEYS[1], ARGV[1])
if not existing or redis.call('EXISTS', KEYS[2]) == 0 then return 0 end
local value = cjson.decode(existing)
if value.token ~= ARGV[2] then return 0 end
if value.sandbox_id ~= cjson.null and value.sandbox_id ~= ARGV[3] then return 0 end
redis.call('HSET', KEYS[1], ARGV[1], ARGV[4])
redis.call('SADD', KEYS[3], ARGV[1])
return 1
"#,
            )
            .arg(3)
            .arg(self.key("name-reservations"))
            .arg(self.key(&format!("sandbox:{sandbox}")))
            .arg(self.key(&format!("reserved-names:{sandbox}")))
            .arg(name.as_str())
            .arg(token)
            .arg(sandbox)
            .arg(serde_json::to_string(&reservation).map_err(json_error)?)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(result == 1)
    }

    async fn name_reservation(&self, name: &SandboxName) -> Result<Option<NameReservation>> {
        let mut c = self.connection.clone();
        let value: Option<String> = redis::cmd("HGET")
            .arg(self.key("name-reservations"))
            .arg(name.as_str())
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        value
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }
    async fn release_pending_name(&self, name: &SandboxName, token: &str) -> Result<bool> {
        let mut c = self.connection.clone();
        let result: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local existing = redis.call('HGET', KEYS[1], ARGV[1])
if not existing then return 0 end
local value = cjson.decode(existing)
if value.token ~= ARGV[2] or value.sandbox_id ~= cjson.null then return 0 end
return redis.call('HDEL', KEYS[1], ARGV[1])
"#,
            )
            .arg(1)
            .arg(self.key("name-reservations"))
            .arg(name.as_str())
            .arg(token)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(result == 1)
    }

    async fn claim_domain(&self, binding: &DomainBinding) -> Result<DomainClaim> {
        let mut c = self.connection.clone();
        let result: u32 = redis::cmd("EVAL")
            .arg(
                r#"
if redis.call('EXISTS', KEYS[1]) == 0 then return 0 end
local value = redis.call('HGET', KEYS[2], ARGV[2])
if value and cjson.decode(value).sandbox_id ~= ARGV[1] then return 2 end
redis.call('HSET', KEYS[2], ARGV[2], ARGV[3])
redis.call('SADD', KEYS[3], ARGV[2])
return 1
"#,
            )
            .arg(3)
            .arg(self.key(&format!("sandbox:{}", binding.sandbox_id())))
            .arg(self.key("domains"))
            .arg(self.key(&format!("domains:{}", binding.sandbox_id())))
            .arg(binding.sandbox_id())
            .arg(binding.domain().as_str())
            .arg(serde_json::to_string(binding).map_err(json_error)?)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        match result {
            0 => Ok(DomainClaim::SandboxMissing),
            1 => Ok(DomainClaim::Claimed),
            2 => Ok(DomainClaim::Conflict),
            _ => Err(StoreError(format!(
                "unexpected domain claim result: {result}"
            ))),
        }
    }

    async fn domain(&self, name: &DomainName) -> Result<Option<DomainBinding>> {
        let mut c = self.connection.clone();
        let value: Option<String> = redis::cmd("HGET")
            .arg(self.key("domains"))
            .arg(name.as_str())
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        value
            .map(|json| serde_json::from_str(&json).map_err(json_error))
            .transpose()
    }

    async fn domains(&self, sandbox: &str) -> Result<Vec<DomainBinding>> {
        let mut c = self.connection.clone();
        let values: Vec<String> = redis::cmd("EVAL")
            .arg(
                r#"
local result = {}
for _, name in ipairs(redis.call('SMEMBERS', KEYS[2])) do
    local value = redis.call('HGET', KEYS[1], name)
    if value and cjson.decode(value).sandbox_id == ARGV[1] then
        table.insert(result, value)
    end
end
return result
"#,
            )
            .arg(2)
            .arg(self.key("domains"))
            .arg(self.key(&format!("domains:{sandbox}")))
            .arg(sandbox)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        let mut bindings: Vec<DomainBinding> = values
            .iter()
            .map(|json| serde_json::from_str(json).map_err(json_error))
            .collect::<Result<_>>()?;
        bindings.sort_by(|a, b| a.domain().cmp(b.domain()));
        Ok(bindings)
    }

    async fn delete_domain(&self, name: &DomainName, sandbox: &str) -> Result<bool> {
        let mut c = self.connection.clone();
        let removed: u32 = redis::cmd("EVAL")
            .arg(
                r#"
local value = redis.call('HGET', KEYS[1], ARGV[1])
if not value or cjson.decode(value).sandbox_id ~= ARGV[2] then return 0 end
redis.call('HDEL', KEYS[1], ARGV[1])
redis.call('SREM', KEYS[2], ARGV[1])
return 1
"#,
            )
            .arg(2)
            .arg(self.key("domains"))
            .arg(self.key(&format!("domains:{sandbox}")))
            .arg(name.as_str())
            .arg(sandbox)
            .query_async(&mut c)
            .await
            .map_err(redis_error)?;
        Ok(removed > 0)
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
        "unknown store '{}': use memory: or redis://host:port",
        redacted(url)
    )))
}

/// `url` fit for a log: any credentials (`redis://:password@host`) replaced.
#[must_use]
pub fn redacted(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_string();
    };
    let authority_end = rest.find('/').unwrap_or(rest.len());
    match rest[..authority_end].rfind('@') {
        Some(at) => format!("{scheme}://***@{}", &rest[at + 1..]),
        None => url.to_string(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::now_ms;
    use serde_json::json;

    #[test]
    fn a_logged_store_url_carries_no_password() {
        assert_eq!(
            redacted("redis://:hunter2@store:6379/0"),
            "redis://***@store:6379/0"
        );
        assert_eq!(
            redacted("rediss://user:p@ss@store:6380"),
            "rediss://***@store:6380"
        );
        assert_eq!(redacted("redis://store:6379"), "redis://store:6379");
        assert_eq!(redacted("memory:"), "memory:");
    }

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
            template_metadata: std::collections::BTreeMap::new(),
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
        // Named registration publishes the record and ownership together.
        let name = SandboxName::parse("atomic-create").unwrap();
        let owner = NameReservation::pending(name.clone());
        let wrong = NameReservation::pending(name.clone());
        let mut record = sandbox("atomic-created", "a");
        record
            .metadata
            .insert("hm.name".into(), name.as_str().into());
        assert!(!store.register_named_sandbox(&record, &owner).await.unwrap());
        assert!(store.sandbox(&record.sandbox_id).await.unwrap().is_none());
        assert!(store.reserve_name(&owner).await.unwrap());
        assert!(!store.register_named_sandbox(&record, &wrong).await.unwrap());
        let mut bad = record.clone();
        bad.metadata.clear();
        assert!(!store.register_named_sandbox(&bad, &owner).await.unwrap());
        bad = record.clone();
        bad.sandbox_id = "bad/id".into();
        assert!(!store.register_named_sandbox(&bad, &owner).await.unwrap());
        assert_eq!(
            store.name_reservation(&name).await.unwrap().as_ref(),
            Some(&owner)
        );
        assert!(store.sandbox(&record.sandbox_id).await.unwrap().is_none());
        assert!(store.register_named_sandbox(&record, &owner).await.unwrap());
        assert_eq!(
            store.sandbox(&record.sandbox_id).await.unwrap().as_ref(),
            Some(&record)
        );
        let bound = store.name_reservation(&name).await.unwrap().unwrap();
        assert_eq!(bound.sandbox_id(), Some(record.sandbox_id.as_str()));
        assert!(store.register_named_sandbox(&record, &owner).await.unwrap());
        assert!(!store.register_named_sandbox(&record, &bound).await.unwrap());
        let mut changed = record.clone();
        changed.end_at_ms += 1;
        store.put_sandbox(&changed).await.unwrap();
        assert!(!store.register_named_sandbox(&record, &owner).await.unwrap());
        assert_eq!(
            store.sandbox(&record.sandbox_id).await.unwrap().as_ref(),
            Some(&changed)
        );
        let mut transferred = record.clone();
        transferred.sandbox_id = "atomic-transferred".into();
        assert!(!store
            .register_named_sandbox(&transferred, &owner)
            .await
            .unwrap());
        assert!(store
            .sandbox(&transferred.sandbox_id)
            .await
            .unwrap()
            .is_none());
        assert!(store.delete_sandbox(&record.sandbox_id).await.unwrap());
        let replacement = NameReservation::pending(name.clone());
        assert!(store.reserve_name(&replacement).await.unwrap());
        assert!(!store.register_named_sandbox(&record, &owner).await.unwrap());
        assert!(store
            .register_named_sandbox(&transferred, &replacement)
            .await
            .unwrap());
        assert!(!store.delete_sandbox(&record.sandbox_id).await.unwrap());
        assert_eq!(
            store
                .name_reservation(&name)
                .await
                .unwrap()
                .unwrap()
                .sandbox_id(),
            Some(transferred.sandbox_id.as_str())
        );
        store.delete_sandbox(&transferred.sandbox_id).await.unwrap();

        let collision_name = SandboxName::parse("atomic-existing-id").unwrap();
        let collision = NameReservation::pending(collision_name.clone());
        let original = sandbox("atomic-id-collision", "original-node");
        store.put_sandbox(&original).await.unwrap();
        let mut intruder = original.clone();
        intruder.node_id = "other-node".into();
        intruder
            .metadata
            .insert("hm.name".into(), collision_name.as_str().into());
        assert!(store.reserve_name(&collision).await.unwrap());
        assert!(!store
            .register_named_sandbox(&intruder, &collision)
            .await
            .unwrap());
        assert_eq!(
            store.sandbox(&original.sandbox_id).await.unwrap().as_ref(),
            Some(&original)
        );
        assert_eq!(
            store
                .name_reservation(&collision_name)
                .await
                .unwrap()
                .as_ref(),
            Some(&collision)
        );
        store.delete_sandbox(&original.sandbox_id).await.unwrap();
        store
            .release_pending_name(&collision_name, collision.operation_token())
            .await
            .unwrap();

        for round in 0..16 {
            let name = SandboxName::parse(&format!("atomic-register-race-{round}")).unwrap();
            let owner = NameReservation::pending(name.clone());
            assert!(store.reserve_name(&owner).await.unwrap());
            let mut a = sandbox(&format!("atomic-register-a-{round}"), "a");
            a.metadata.insert("hm.name".into(), name.as_str().into());
            let mut b = a.clone();
            b.sandbox_id = format!("atomic-register-b-{round}");
            b.node_id = "b".into();
            let (ra, rb) = tokio::join!(
                store.register_named_sandbox(&a, &owner),
                store.register_named_sandbox(&b, &owner)
            );
            let (ra, rb) = (ra.unwrap(), rb.unwrap());
            assert_ne!(ra, rb);
            let (winner, loser) = if ra { (&a, &b) } else { (&b, &a) };
            assert_eq!(
                store.sandbox(&winner.sandbox_id).await.unwrap().as_ref(),
                Some(winner)
            );
            assert!(store.sandbox(&loser.sandbox_id).await.unwrap().is_none());
            assert_eq!(
                store
                    .name_reservation(&name)
                    .await
                    .unwrap()
                    .unwrap()
                    .sandbox_id(),
                Some(winner.sandbox_id.as_str())
            );
            store.delete_sandbox(&winner.sandbox_id).await.unwrap();
            assert!(store.name_reservation(&name).await.unwrap().is_none());
        }

        for round in 0..16 {
            let name = SandboxName::parse(&format!("atomic-register-delete-{round}")).unwrap();
            let owner = NameReservation::pending(name.clone());
            let mut record = sandbox(&format!("atomic-register-delete-vm-{round}"), "a");
            record
                .metadata
                .insert("hm.name".into(), name.as_str().into());
            assert!(store.reserve_name(&owner).await.unwrap());
            let (registered, deleted) = if round % 2 == 0 {
                tokio::join!(
                    store.register_named_sandbox(&record, &owner),
                    store.delete_sandbox(&record.sandbox_id)
                )
            } else {
                let (deleted, registered) = tokio::join!(
                    store.delete_sandbox(&record.sandbox_id),
                    store.register_named_sandbox(&record, &owner)
                );
                (registered, deleted)
            };
            assert!(registered.unwrap());
            if deleted.unwrap() {
                assert!(store.sandbox(&record.sandbox_id).await.unwrap().is_none());
                assert!(store.name_reservation(&name).await.unwrap().is_none());
            } else {
                assert_eq!(
                    store.sandbox(&record.sandbox_id).await.unwrap().as_ref(),
                    Some(&record)
                );
                assert_eq!(
                    store
                        .name_reservation(&name)
                        .await
                        .unwrap()
                        .unwrap()
                        .sandbox_id(),
                    Some(record.sandbox_id.as_str())
                );
                store.delete_sandbox(&record.sandbox_id).await.unwrap();
            }
        }

        // Whichever transaction wins, deletion cannot leave bound ownership
        // pointing at a missing sandbox. Uncertain pending ownership survives.
        for round in 0..16 {
            let name = SandboxName::parse(&format!("bind-delete-{round}")).unwrap();
            let id = format!("bind-delete-vm-{round}");
            let reservation = NameReservation::pending(name.clone());
            assert!(store.reserve_name(&reservation).await.unwrap());
            store.put_sandbox(&sandbox(&id, "a")).await.unwrap();
            let (bound, deleted) = tokio::join!(
                store.bind_name(&name, reservation.operation_token(), &id),
                store.delete_sandbox(&id)
            );
            let bound = bound.unwrap();
            assert!(deleted.unwrap());
            assert!(store.sandbox(&id).await.unwrap().is_none());
            let remaining = store.name_reservation(&name).await.unwrap();
            if bound {
                assert!(remaining.is_none(), "delete must remove winning binding");
            } else {
                assert_eq!(remaining.as_ref(), Some(&reservation));
                assert!(store
                    .release_pending_name(&name, reservation.operation_token())
                    .await
                    .unwrap());
            }
        }
        let name = SandboxName::parse("competing-bind-targets").unwrap();
        let reservation = NameReservation::pending(name.clone());
        assert!(store.reserve_name(&reservation).await.unwrap());
        for id in ["bind-target-a", "bind-target-b"] {
            store.put_sandbox(&sandbox(id, "a")).await.unwrap();
        }
        let (a, b) = tokio::join!(
            store.bind_name(&name, reservation.operation_token(), "bind-target-a"),
            store.bind_name(&name, reservation.operation_token(), "bind-target-b")
        );
        let a = a.unwrap();
        let b = b.unwrap();
        assert_ne!(a, b, "one token cannot bind ownership to two targets");
        let winner = if a { "bind-target-a" } else { "bind-target-b" };
        let loser = if a { "bind-target-b" } else { "bind-target-a" };
        assert!(store.delete_sandbox(loser).await.unwrap());
        assert_eq!(
            store
                .name_reservation(&name)
                .await
                .unwrap()
                .unwrap()
                .sandbox_id(),
            Some(winner)
        );
        assert!(store.delete_sandbox(winner).await.unwrap());
        assert!(store.name_reservation(&name).await.unwrap().is_none());

        let bound_name = SandboxName::parse("binding-fixture").unwrap();
        let reservation = NameReservation::pending(bound_name.clone());
        let token = reservation.operation_token();
        assert!(store.reserve_name(&reservation).await.unwrap());
        assert!(!store
            .bind_name(&bound_name, token, "binding-old")
            .await
            .unwrap());
        store
            .put_sandbox(&sandbox("binding-old", "a"))
            .await
            .unwrap();
        assert!(!store
            .bind_name(&bound_name, "wrong", "binding-old")
            .await
            .unwrap());
        assert!(store
            .bind_name(&bound_name, token, "binding-old")
            .await
            .unwrap());
        assert!(store
            .bind_name(&bound_name, token, "binding-old")
            .await
            .unwrap());
        assert!(!store
            .release_pending_name(&bound_name, token)
            .await
            .unwrap());
        assert!(store.delete_sandbox("binding-old").await.unwrap());
        assert!(store.name_reservation(&bound_name).await.unwrap().is_none());
        let replacement = NameReservation::pending(bound_name.clone());
        assert!(store.reserve_name(&replacement).await.unwrap());
        store
            .put_sandbox(&sandbox("binding-new", "a"))
            .await
            .unwrap();
        assert!(store
            .bind_name(&bound_name, replacement.operation_token(), "binding-new")
            .await
            .unwrap());
        assert!(!store.delete_sandbox("binding-old").await.unwrap());
        assert_eq!(
            store
                .name_reservation(&bound_name)
                .await
                .unwrap()
                .unwrap()
                .sandbox_id(),
            Some("binding-new")
        );
        assert!(store.delete_sandbox("binding-new").await.unwrap());

        let name = SandboxName::parse("reservation-fixture").unwrap();
        let a = NameReservation::pending(name.clone());
        let b = NameReservation::pending(name.clone());
        let (ra, rb) = tokio::join!(store.reserve_name(&a), store.reserve_name(&b));
        let ra = ra.unwrap();
        let rb = rb.unwrap();
        assert_ne!(ra, rb, "exactly one competing owner wins");
        let (winner, loser) = if ra { (&a, &b) } else { (&b, &a) };
        assert!(store.reserve_name(winner).await.unwrap());
        assert!(!store
            .release_pending_name(&name, loser.operation_token())
            .await
            .unwrap());
        assert_eq!(
            store.name_reservation(&name).await.unwrap().as_ref(),
            Some(winner)
        );
        assert!(store
            .release_pending_name(&name, winner.operation_token())
            .await
            .unwrap());
        assert!(store.reserve_name(loser).await.unwrap());
        assert!(!store
            .release_pending_name(&name, winner.operation_token())
            .await
            .unwrap());
        let mut bound = NameReservation::pending(SandboxName::parse("bound-refused").unwrap());
        let token = bound.operation_token().to_owned();
        bound.bind(&token, "sandbox-fixture").unwrap();
        assert!(store.reserve_name(&bound).await.is_err());
        assert!(store
            .release_pending_name(&name, loser.operation_token())
            .await
            .unwrap());

        let first = DomainBinding::new("App.Example.com.", "domain-a", 8080).unwrap();
        let second = DomainBinding::new("app.example.com", "domain-b", 9090).unwrap();
        assert_eq!(
            store.claim_domain(&first).await.unwrap(),
            DomainClaim::SandboxMissing
        );
        store.put_sandbox(&sandbox("domain-a", "a")).await.unwrap();
        store.put_sandbox(&sandbox("domain-b", "a")).await.unwrap();
        let (a, b) = tokio::join!(store.claim_domain(&first), store.claim_domain(&second));
        let outcomes = [a.unwrap(), b.unwrap()];
        assert_eq!(
            outcomes
                .iter()
                .filter(|x| **x == DomainClaim::Claimed)
                .count(),
            1
        );
        assert_eq!(
            outcomes
                .iter()
                .filter(|x| **x == DomainClaim::Conflict)
                .count(),
            1
        );
        let winner = store.domain(first.domain()).await.unwrap().unwrap();
        let loser = if winner.sandbox_id() == "domain-a" {
            "domain-b"
        } else {
            "domain-a"
        };
        assert!(!store.delete_domain(first.domain(), loser).await.unwrap());
        let updated = DomainBinding::new("app.example.com", winner.sandbox_id(), 3000).unwrap();
        assert_eq!(
            store.claim_domain(&updated).await.unwrap(),
            DomainClaim::Claimed
        );
        assert_eq!(
            store.domain(first.domain()).await.unwrap(),
            Some(updated.clone())
        );
        assert_eq!(
            store.domains(winner.sandbox_id()).await.unwrap(),
            vec![updated]
        );
        assert!(store.domains(loser).await.unwrap().is_empty());
        assert!(store.delete_sandbox(winner.sandbox_id()).await.unwrap());
        assert!(store.domain(first.domain()).await.unwrap().is_none());
        let replacement = DomainBinding::new("app.example.com", loser, 4000).unwrap();
        assert_eq!(
            store.claim_domain(&replacement).await.unwrap(),
            DomainClaim::Claimed
        );
        assert!(!store
            .delete_domain(first.domain(), winner.sandbox_id())
            .await
            .unwrap());
        assert!(!store.delete_sandbox(winner.sandbox_id()).await.unwrap());
        assert_eq!(
            store.domain(first.domain()).await.unwrap(),
            Some(replacement)
        );
        assert!(store.delete_domain(first.domain(), loser).await.unwrap());
        assert!(!store.delete_domain(first.domain(), loser).await.unwrap());
        store.delete_sandbox(loser).await.unwrap();
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
    #[tokio::test]
    async fn redis_named_registration_checks_write_permissions_before_writing() {
        // This opt-in endpoint must be an owned test server: the test creates
        // and removes a uniquely named ACL user, never changes the default user.
        let Ok(url) = std::env::var("HV2_TEST_REDIS_ACL") else {
            eprintln!("skipped: HV2_TEST_REDIS_ACL is unset (requires an owned Redis server)");
            return;
        };
        let identity = uuid::Uuid::new_v4().simple().to_string();
        let namespace = format!("acl-test-{identity}");
        let store = RedisStore::connect(&url, &namespace).await.unwrap();
        let username = format!("registration-{identity}");
        let password = uuid::Uuid::new_v4().simple().to_string();
        let mut admin = store.connection.clone();
        redis::cmd("ACL")
            .arg("SETUSER")
            .arg(&username)
            .arg("on")
            .arg(format!(">{password}"))
            .arg(format!("~{}*", store.prefix))
            .arg("+@all")
            .query_async::<()>(&mut admin)
            .await
            .unwrap();
        let restricted_url = url.replacen("redis://", &format!("redis://{username}:{password}@"), 1);
        let restricted = RedisStore::connect(&restricted_url, &namespace).await.unwrap();
        let mut outcomes = Vec::new();
        for denied in ["set", "sadd", "hset"] {
            let name = SandboxName::parse(&format!("acl-{denied}")).unwrap();
            let owner = NameReservation::pending(name.clone());
            let mut record = sandbox(&format!("acl-{denied}-vm"), "a");
            record.metadata.insert("hm.name".into(), name.as_str().into());
            assert!(store.reserve_name(&owner).await.unwrap());
            redis::cmd("ACL").arg("SETUSER").arg(&username).arg(format!("-{denied}"))
                .query_async::<()>(&mut admin).await.unwrap();
            let refused = restricted.register_named_sandbox(&record, &owner).await.is_err();
            let absent = store.sandbox(&record.sandbox_id).await.unwrap().is_none();
            let pending = store.name_reservation(&name).await.unwrap().as_ref() == Some(&owner);
            let indexed: bool = redis::cmd("SISMEMBER").arg(store.key("sandboxes"))
                .arg(&record.sandbox_id).query_async(&mut admin).await.unwrap();
            let names: u64 = redis::cmd("SCARD").arg(store.key(&format!("reserved-names:{}", record.sandbox_id)))
                .query_async(&mut admin).await.unwrap();
            outcomes.push((denied, refused && absent && pending && !indexed && names == 0));
            redis::cmd("ACL").arg("SETUSER").arg(&username).arg(format!("+{denied}"))
                .query_async::<()>(&mut admin).await.unwrap();
            // Clean even the old script's partial writes before asserting.
            store.delete_sandbox(&record.sandbox_id).await.unwrap();
            store.release_pending_name(&name, owner.operation_token()).await.unwrap();
        }
        redis::cmd("ACL").arg("DELUSER").arg(&username)
            .query_async::<u64>(&mut admin).await.unwrap();
        for (denied, intact) in outcomes {
            assert!(intact, "registration partially wrote after denying {denied}");
        }
    }

    #[tokio::test]
    async fn redis_named_registration_checks_index_types_before_writing() {
        let Ok(url) = std::env::var("HV2_TEST_REDIS") else {
            eprintln!("skipped: HV2_TEST_REDIS is unset");
            return;
        };
        let store = RedisStore::connect(&url, &format!("type-test-{}", uuid::Uuid::new_v4()))
            .await
            .unwrap();
        let name = SandboxName::parse("atomic-type-fault").unwrap();
        let owner = NameReservation::pending(name.clone());
        let mut record = sandbox("atomic-type-fault-vm", "a");
        record
            .metadata
            .insert("hm.name".into(), name.as_str().into());
        assert!(store.reserve_name(&owner).await.unwrap());
        let mut connection = store.connection.clone();
        for index in [
            store.key("sandboxes"),
            store.key(&format!("reserved-names:{}", record.sandbox_id)),
        ] {
            redis::cmd("SET")
                .arg(&index)
                .arg("wrong-type")
                .query_async::<()>(&mut connection)
                .await
                .unwrap();
            assert!(store.register_named_sandbox(&record, &owner).await.is_err());
            assert!(store.sandbox(&record.sandbox_id).await.unwrap().is_none());
            assert_eq!(
                store.name_reservation(&name).await.unwrap().as_ref(),
                Some(&owner)
            );
            redis::cmd("DEL")
                .arg(&index)
                .query_async::<u64>(&mut connection)
                .await
                .unwrap();
        }
        assert!(store.register_named_sandbox(&record, &owner).await.unwrap());
        assert!(store.delete_sandbox(&record.sandbox_id).await.unwrap());
        assert!(store.name_reservation(&name).await.unwrap().is_none());
    }
}
