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
use crate::private_networks::{NetworkMembershipState, MembershipAccess, MembershipChange, PrivateRouteSnapshot};
use crate::ports::{PortAllocation, PortClaim, PortProtocol, PublicPortRange, OwnedPortAccess, MAX_PORTS_PER_SANDBOX};

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

fn private_address_source_matches(source: &SandboxRecord, current: &SandboxRecord) -> bool {
    source.sandbox_id == current.sandbox_id && source.owner_id == current.owner_id
        && source.started_at_ms == current.started_at_ms && source.node_id == current.node_id
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



    /// Read both endpoint records and memberships in one atomic view. Missing,
    /// removed, owner-mismatched or migrated memberships cannot yield a route.
    /// The caller must still authenticate its source and enforce node liveness,
    /// local pending registration, generation, network and lifecycle state.
    async fn private_route_snapshot(&self, _source: &str, _destination: &str)
        -> Result<Option<PrivateRouteSnapshot>> {
        Err(StoreError("atomic private routes unsupported by this store".into()))
    }

    /// Fresh route records/memberships plus both requested live nodes. The
    /// caller still checks placement, local VM identity and authorization.
    /// Stores may combine these reads atomically; the fallback preserves all
    /// checks without caching or removing either authorization barrier.
    async fn private_route_snapshot_with_live_nodes(&self, source: &str, destination: &str,
        source_node: &str, destination_node: &str) -> Result<Option<PrivateRouteSnapshot>> {
        let (snapshot, source_live, destination_live)=tokio::join!(
            self.private_route_snapshot(source,destination),self.node(source_node),self.node(destination_node));
        let Some(snapshot)=snapshot? else { return Ok(None); };
        if source_live?.is_none() || destination_live?.is_none() { return Ok(None); }
        Ok(Some(snapshot))
    }

    /// Owner-authorized membership snapshot; includes a removal tombstone.
    async fn private_membership(&self, _sandbox: &str, _owner: &crate::ownership::OwnerId)
        -> Result<MembershipAccess<Option<NetworkMembershipState>>> {
        Err(StoreError("private membership unsupported by this store".into()))
    }
    /// Atomic revision replacement against the current trusted VM incarnation.
    /// None expects no prior row. Exact replay succeeds; removed rows retain a
    /// revision and cannot be recreated by replaying an earlier None request.
    async fn compare_private_membership(&self, _expected: Option<&str>, _next: &NetworkMembershipState)
        -> Result<MembershipChange> {
        Err(StoreError("private membership unsupported by this store".into()))
    }

    /// Source-incarnation-scoped append-only address ledger. Returned bytes are
    /// canonical snapshots; retain them as the expected CAS version.
    async fn private_address_ledger(&self, _source: &SandboxRecord) -> Result<MembershipAccess<Option<Vec<u8>>>> {
        Err(StoreError("private address ledgers unsupported by this store".into()))
    }
    /// Exact replay succeeds; a stale expected snapshot or changed VM refuses.
    /// Stored bindings can only be appended, never changed or removed. Publish
    /// an address to a guest only after Applied (including exact retry recovery).
    async fn compare_private_address_ledger(&self, _source: &SandboxRecord, _expected: Option<&[u8]>, _next: &[u8]) -> Result<MembershipChange> {
        Err(StoreError("private address ledgers unsupported by this store".into()))
    }

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()>;
    /// Assign an owner only to an existing ownerless VM without port reservations.
    /// Caller must authorize an administrator and serialize node lifecycle updates.
    async fn adopt_sandbox_owner(&self, _sandbox: &str, _owner: &crate::ownership::OwnerId)
        -> Result<crate::ownership::OwnerAdoption> {
        Err(StoreError("owner adoption unsupported by this store".into()))
    }

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

    /// Reserve or update a VM destination atomically. Replay/protocol updates keep its public port.
    /// The caller must authorize the VM owner before its first reservation; owner IDs are not secrets.
    async fn claim_port(&self, _sandbox: &str, _machine_port: u16, _owner: &str,
        _protocol: PortProtocol, _range: PublicPortRange) -> Result<PortClaim> {
        Err(StoreError("managed ports unsupported by this store".into()))
    }
    async fn port_allocations(&self, _sandbox: Option<&str>) -> Result<Vec<PortAllocation>> {
        Err(StoreError("managed ports unsupported by this store".into()))
    }
    /// Release only the matching principal's current destination reservation.
    async fn delete_port(&self, _sandbox: &str, _machine_port: u16, _owner: &str) -> Result<bool> {
        Err(StoreError("managed ports unsupported by this store".into()))
    }

    /// Require the stored VM creator and reserve/update in the same transaction.
    async fn claim_owned_port(&self, _sandbox: &str, _machine_port: u16, _owner: &str,
        _protocol: PortProtocol, _range: PublicPortRange) -> Result<PortClaim> {
        Err(StoreError("owner port management unsupported by this store".into()))
    }
    async fn owned_ports(&self, _sandbox: &str, _owner: &str) -> Result<OwnedPortAccess<Vec<PortAllocation>>> {
        Err(StoreError("owner port management unsupported by this store".into()))
    }
    async fn delete_owned_port(&self, _sandbox: &str, _machine_port: u16, _owner: &str) -> Result<OwnedPortAccess<bool>> {
        Err(StoreError("owner port management unsupported by this store".into()))
    }

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
    private_memberships: HashMap<String, NetworkMembershipState>,
    private_addresses: HashMap<(String, u64), Vec<u8>>,
    domains: HashMap<DomainName, DomainBinding>,
    names: HashMap<SandboxName, NameReservation>,
    ports: HashMap<(String, u16), PortAllocation>,
    public_ports: HashMap<u16, (String, u16)>,
}

impl MemoryStore {
    fn delete_port_inner(&self, sandbox: &str, machine_port: u16, owner: &str, owned: bool) -> Result<OwnedPortAccess<bool>> {
        crate::ports::validate_request(sandbox, machine_port, owner)?;
        let mut state = self.sandboxes.lock();
        if owned {
            let Some(record) = state.records.get(sandbox) else { return Ok(OwnedPortAccess::SandboxMissing); };
            if record.owner_id.as_ref().map(|id| id.as_str()) != Some(owner) { return Ok(OwnedPortAccess::OwnerConflict); }
        }
        let key = (sandbox.to_owned(), machine_port);
        let Some(row) = state.ports.get(&key) else { return Ok(OwnedPortAccess::Granted(false)) };
        if row.owner_id != owner { return Ok(if owned { OwnedPortAccess::OwnerConflict } else { OwnedPortAccess::Granted(false) }); }
        let public_port = row.public_port;
        state.ports.remove(&key);
        state.public_ports.remove(&public_port);
        Ok(OwnedPortAccess::Granted(true))
    }


    fn claim_port_inner(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange, owned: bool) -> Result<PortClaim> {
        crate::ports::validate_request(sandbox, machine_port, owner)?;
        let mut state = self.sandboxes.lock();
        if !state.records.contains_key(sandbox) { return Ok(PortClaim::SandboxMissing); }
        if owned && state.records.get(sandbox).and_then(|row| row.owner_id.as_ref()).map(|id| id.as_str()) != Some(owner) {
            return Ok(PortClaim::OwnerConflict);
        }
        let key = (sandbox.to_owned(), machine_port);
        if let Some(existing) = state.ports.get_mut(&key) {
            if existing.owner_id != owner { return Ok(PortClaim::OwnerConflict); }
            existing.protocol = protocol;
            return Ok(PortClaim::Allocated(existing.clone()));
        }
        if state.ports.keys().filter(|(id, _)| id == sandbox).count() >= MAX_PORTS_PER_SANDBOX {
            return Ok(PortClaim::LimitReached);
        }
        let Some(public_port) = (range.first()..=range.last()).find(|p| !state.public_ports.contains_key(p)) else {
            return Ok(PortClaim::PoolExhausted);
        };
        let allocation = PortAllocation { sandbox_id: sandbox.into(), machine_port,
            public_port, owner_id: owner.into(), protocol };
        state.public_ports.insert(public_port, key.clone());
        state.ports.insert(key, allocation.clone());
        Ok(PortClaim::Allocated(allocation))
    }


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

    async fn adopt_sandbox_owner(&self, sandbox: &str, owner: &crate::ownership::OwnerId)
        -> Result<crate::ownership::OwnerAdoption> {
        use crate::ownership::OwnerAdoption;
        crate::ports::validate_request(sandbox, 1, owner.as_str())?;
        let mut state = self.sandboxes.lock();
        let Some(record) = state.records.get(sandbox) else { return Ok(OwnerAdoption::SandboxMissing); };
        if let Some(current) = &record.owner_id {
            return Ok(if current == owner { OwnerAdoption::AlreadyOwned } else { OwnerAdoption::OwnerConflict });
        }
        if state.ports.keys().any(|(id, _)| id == sandbox) { return Ok(OwnerAdoption::PortsPresent); }
        state.records.get_mut(sandbox).unwrap().owner_id = Some(owner.clone());
        Ok(OwnerAdoption::Adopted)
    }



    async fn private_address_ledger(&self, source: &SandboxRecord) -> Result<MembershipAccess<Option<Vec<u8>>>> {
        crate::private_addresses::PrivateAddressBook::new(source).map_err(|e|StoreError(e.into()))?;
        let state = self.sandboxes.lock();
        let Some(record) = state.records.get(&source.sandbox_id) else { return Ok(MembershipAccess::SandboxMissing); };
        if record.owner_id != source.owner_id { return Ok(MembershipAccess::OwnerConflict); }
        if !private_address_source_matches(source, record) { return Err(StoreError("private address source changed".into())); }
        let row = state.private_addresses.get(&(source.sandbox_id.clone(),source.started_at_ms)).cloned();
        if let Some(bytes) = &row { crate::private_addresses::PrivateAddressBook::restore(bytes, source).map_err(|e|StoreError(e.into()))?; }
        Ok(MembershipAccess::Granted(row))
    }
    async fn compare_private_address_ledger(&self, source: &SandboxRecord, expected: Option<&[u8]>, next: &[u8]) -> Result<MembershipChange> {
        // Validate input before taking the registry mutex.
        let canonical = crate::private_addresses::PrivateAddressBook::canonical_append(None,next,source).map_err(|e|StoreError(e.into()))?;
        let mut state = self.sandboxes.lock();
        let Some(record) = state.records.get(&source.sandbox_id) else { return Ok(MembershipChange::SandboxMissing); };
        if record.owner_id != source.owner_id { return Ok(MembershipChange::OwnerConflict); }
        if !private_address_source_matches(source,record) { return Ok(MembershipChange::RecordChanged); }
        let key = (source.sandbox_id.clone(),source.started_at_ms);
        let current = state.private_addresses.get(&key);
        if current.is_some_and(|bytes|bytes==&canonical) { return Ok(MembershipChange::Applied); }
        if current.map(Vec::as_slice) != expected { return Ok(MembershipChange::RevisionConflict); }
        crate::private_addresses::PrivateAddressBook::canonical_append(current.map(Vec::as_slice),&canonical,source).map_err(|e|StoreError(e.into()))?;
        state.private_addresses.insert(key,canonical);
        Ok(MembershipChange::Applied)
    }

    async fn private_route_snapshot(&self, source: &str, destination: &str)
        -> Result<Option<PrivateRouteSnapshot>> {
        crate::ports::validate_request(source,1,"private-route")?;
        crate::ports::validate_request(destination,1,"private-route")?;
        let state=self.sandboxes.lock();
        let (Some(sr),Some(dr),Some(sm),Some(dm))=(state.records.get(source),state.records.get(destination),
            state.private_memberships.get(source),state.private_memberships.get(destination)) else { return Ok(None); };
        Ok(PrivateRouteSnapshot::checked(source,destination,sr.clone(),dr.clone(),sm.clone(),dm.clone()))
    }

    async fn private_membership(&self, sandbox: &str, owner: &crate::ownership::OwnerId)
        -> Result<MembershipAccess<Option<NetworkMembershipState>>> {
        crate::ports::validate_request(sandbox, 1, owner.as_str())?;
        let state = self.sandboxes.lock();
        let Some(record) = state.records.get(sandbox) else { return Ok(MembershipAccess::SandboxMissing); };
        if record.owner_id.as_ref() != Some(owner) { return Ok(MembershipAccess::OwnerConflict); }
        let row = state.private_memberships.get(sandbox).cloned();
        if row.as_ref().is_some_and(|row| row.owner_id() != owner) {
            return Ok(MembershipAccess::OwnerConflict);
        }
        Ok(MembershipAccess::Granted(row))
    }
    async fn compare_private_membership(&self, expected: Option<&str>, next: &NetworkMembershipState)
        -> Result<MembershipChange> {
        let mut state = self.sandboxes.lock();
        let Some(record) = state.records.get(next.sandbox_id()) else { return Ok(MembershipChange::SandboxMissing); };
        if record.owner_id.as_ref() != Some(next.owner_id()) { return Ok(MembershipChange::OwnerConflict); }
        if !next.matches_record(record) { return Ok(MembershipChange::RecordChanged); }
        let current = state.private_memberships.get(next.sandbox_id());
        if current.is_some_and(|row| row.owner_id() != next.owner_id()) { return Ok(MembershipChange::OwnerConflict); }
        if current == Some(next) { return Ok(MembershipChange::Applied); }
        if current.map(|row| row.revision()) != expected
            || current.is_some_and(|row| row.revision() == next.revision()) {
            return Ok(MembershipChange::RevisionConflict);
        }
        state.private_memberships.insert(next.sandbox_id().into(), next.clone());
        Ok(MembershipChange::Applied)
    }

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()> {
        let mut state = self.sandboxes.lock();
        let mut updated = record.clone();
        if updated.owner_id.is_none() {
            updated.owner_id = state.records.get(&record.sandbox_id).and_then(|current| current.owner_id.clone());
        }
        state.records.insert(record.sandbox_id.clone(), updated);
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
        state.ports.retain(|(sandbox, _), _| sandbox != id);
        state.public_ports.retain(|_, (sandbox, _)| sandbox != id);
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

    async fn claim_port(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange) -> Result<PortClaim> {
        self.claim_port_inner(sandbox, machine_port, owner, protocol, range, false)
    }
    async fn claim_owned_port(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange) -> Result<PortClaim> {
        self.claim_port_inner(sandbox, machine_port, owner, protocol, range, true)
    }

    async fn port_allocations(&self, sandbox: Option<&str>) -> Result<Vec<PortAllocation>> {
        let mut rows: Vec<_> = self.sandboxes.lock().ports.values()
            .filter(|row| sandbox.is_none_or(|id| row.sandbox_id == id)).cloned().collect();
        rows.sort_by_key(|row| row.public_port);
        Ok(rows)
    }

    async fn delete_port(&self, sandbox: &str, machine_port: u16, owner: &str) -> Result<bool> {
        match self.delete_port_inner(sandbox, machine_port, owner, false)? {
            OwnedPortAccess::Granted(value) => Ok(value), _ => Ok(false),
        }
    }
    async fn delete_owned_port(&self, sandbox: &str, machine_port: u16, owner: &str) -> Result<OwnedPortAccess<bool>> {
        self.delete_port_inner(sandbox, machine_port, owner, true)
    }

    async fn owned_ports(&self, sandbox: &str, owner: &str) -> Result<OwnedPortAccess<Vec<PortAllocation>>> {
        crate::ports::validate_request(sandbox, 1, owner)?;
        let state = self.sandboxes.lock();
        let Some(record) = state.records.get(sandbox) else { return Ok(OwnedPortAccess::SandboxMissing); };
        if record.owner_id.as_ref().map(|id| id.as_str()) != Some(owner) { return Ok(OwnedPortAccess::OwnerConflict); }
        let mut rows: Vec<_> = state.ports.values().filter(|row| row.sandbox_id == sandbox).cloned().collect();
        if rows.len()>MAX_PORTS_PER_SANDBOX || rows.iter().any(|row| row.owner_id!=owner) { return Err(StoreError("invalid owned-port snapshot".into())); }
        rows.sort_by_key(|row| row.public_port);
        Ok(OwnedPortAccess::Granted(rows))
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
    async fn delete_port_inner(&self, sandbox: &str, machine_port: u16, owner: &str, owned: bool) -> Result<OwnedPortAccess<bool>> {
        crate::ports::validate_request(sandbox, machine_port, owner)?;
        let mut c = self.connection.clone();
        let removed: i64 = redis::cmd("EVAL").arg(r#"
if ARGV[4]=='1' then
 local encoded=redis.call('GET',KEYS[3]); if not encoded then return 3 end
 local vm=cjson.decode(encoded)
 if vm.sandbox_id~=ARGV[1] then return redis.error_reply('invalid sandbox ownership record') end
 if vm.owner_id~=ARGV[3] then return 2 end
end
local value=redis.call('HGET',KEYS[1],ARGV[2]); if not value then return 0 end
local row=cjson.decode(value)
if type(row)~='table' then return redis.error_reply('invalid managed-port record') end
local fields={sandbox_id=true,machine_port=true,public_port=true,owner_id=true,protocol=true}
for field,_ in pairs(row) do if not fields[field] then return redis.error_reply('invalid managed-port record') end end
if row.protocol~='tcp' and row.protocol~='udp' and row.protocol~='both' then return redis.error_reply('invalid managed-port record') end
if type(row.owner_id)~='string' or #row.owner_id<1 or #row.owner_id>128 or row.owner_id:find('[^%w_.%-]') then return redis.error_reply('invalid managed-port record') end
if row.sandbox_id~=ARGV[1] or row.machine_port~=tonumber(ARGV[2]) or type(row.public_port)~='number' or row.public_port<1 or row.public_port>65535 or row.public_port%1~=0 then
 return redis.error_reply('invalid managed-port record')
end
if row.owner_id~=ARGV[3] then if ARGV[4]=='1' then return 2 else return 0 end end
local field=tostring(row.public_port)
if redis.call('HGET',KEYS[2],field)~=value then return redis.error_reply('managed-port indexes disagree') end
if type(redis.acl_check_cmd)~='function' or not redis.acl_check_cmd('HDEL',KEYS[1],ARGV[2]) or
 not redis.acl_check_cmd('HDEL',KEYS[2],field) then return redis.error_reply('managed-port delete preflight denied') end
redis.call('HDEL',KEYS[1],ARGV[2]); redis.call('HDEL',KEYS[2],field); return 1
"#).arg(3).arg(self.key(&format!("ports:{sandbox}"))).arg(self.key("public-ports")).arg(self.key(&format!("sandbox:{sandbox}")))
            .arg(sandbox).arg(machine_port).arg(owner).arg(u8::from(owned)).query_async(&mut c).await.map_err(redis_error)?;
        match removed {
            0 | 1 => Ok(OwnedPortAccess::Granted(removed == 1)),
            2 => Ok(OwnedPortAccess::OwnerConflict), 3 => Ok(OwnedPortAccess::SandboxMissing),
            _ => Err(StoreError("unexpected owned-port deletion result".into())),
        }
    }


    async fn claim_port_inner(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange, owned: bool) -> Result<PortClaim> {
        crate::ports::validate_request(sandbox, machine_port, owner)?;
        let mut c = self.connection.clone();
        let (status, json): (i64, String) = redis::cmd("EVAL").arg(r#"
local function kind(key) local t=redis.call('TYPE',key); return type(t)=='table' and t.ok or t end
for _,key in ipairs({KEYS[2],KEYS[3]}) do
 local t=kind(key); if t~='none' and t~='hash' then return redis.error_reply('invalid managed-port index type') end
end
if redis.call('EXISTS',KEYS[1])==0 then return {0,''} end
if ARGV[8]=='1' then
 local vm=cjson.decode(redis.call('GET',KEYS[1]))
 if vm.sandbox_id~=ARGV[1] then return redis.error_reply('invalid sandbox ownership record') end
 if vm.owner_id~=ARGV[3] then return {2,''} end
end
local value=redis.call('HGET',KEYS[3],ARGV[2])
local row=nil
if value then
 row=cjson.decode(value)
 if type(row)~='table' then return redis.error_reply('invalid managed-port record') end
 local fields={sandbox_id=true,machine_port=true,public_port=true,owner_id=true,protocol=true}
 for field,_ in pairs(row) do if not fields[field] then return redis.error_reply('invalid managed-port record') end end
 if row.protocol~='tcp' and row.protocol~='udp' and row.protocol~='both' then return redis.error_reply('invalid managed-port record') end
 if type(row.owner_id)~='string' or #row.owner_id<1 or #row.owner_id>128 or row.owner_id:find('[^%w_.%-]') then return redis.error_reply('invalid managed-port record') end
 if row.sandbox_id~=ARGV[1] or row.machine_port~=tonumber(ARGV[2]) or type(row.public_port)~='number' or row.public_port<1 or row.public_port>65535 or row.public_port%1~=0 then
  return redis.error_reply('invalid managed-port record')
 end
 if row.owner_id~=ARGV[3] then return {2,''} end
 if redis.call('HGET',KEYS[2],tostring(row.public_port))~=value then return redis.error_reply('managed-port indexes disagree') end
 row.protocol=ARGV[4]
else
 if redis.call('HLEN',KEYS[3])>=tonumber(ARGV[7]) then return {3,''} end
 local selected=nil
 for port=tonumber(ARGV[5]),tonumber(ARGV[6]) do
  if redis.call('HEXISTS',KEYS[2],tostring(port))==0 then selected=port; break end
 end
 if not selected then return {4,''} end
 row={sandbox_id=ARGV[1],machine_port=tonumber(ARGV[2]),public_port=selected,owner_id=ARGV[3],protocol=ARGV[4]}
end
local encoded=cjson.encode(row)
if type(redis.acl_check_cmd)~='function' or
 not redis.acl_check_cmd('HSET',KEYS[2],tostring(row.public_port),encoded) or
 not redis.acl_check_cmd('HSET',KEYS[3],ARGV[2],encoded) then
 return redis.error_reply('managed-port write preflight denied')
end
redis.call('HSET',KEYS[2],tostring(row.public_port),encoded)
redis.call('HSET',KEYS[3],ARGV[2],encoded)
return {1,encoded}
"#).arg(3).arg(self.key(&format!("sandbox:{sandbox}")))
            .arg(self.key("public-ports")).arg(self.key(&format!("ports:{sandbox}")))
            .arg(sandbox).arg(machine_port).arg(owner).arg(protocol.as_str())
            .arg(range.first()).arg(range.last()).arg(MAX_PORTS_PER_SANDBOX).arg(u8::from(owned))
            .query_async(&mut c).await.map_err(redis_error)?;
        match status {
            0 => Ok(PortClaim::SandboxMissing),
            1 => { let row: PortAllocation = serde_json::from_str(&json).map_err(json_error)?;
                row.validate()?; Ok(PortClaim::Allocated(row)) },
            2 => Ok(PortClaim::OwnerConflict), 3 => Ok(PortClaim::LimitReached),
            4 => Ok(PortClaim::PoolExhausted), _ => Err(StoreError("unexpected managed-port claim result".into())),
        }
    }


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

    async fn adopt_sandbox_owner(&self, sandbox: &str, owner: &crate::ownership::OwnerId)
        -> Result<crate::ownership::OwnerAdoption> {
        use crate::ownership::OwnerAdoption;
        crate::ports::validate_request(sandbox, 1, owner.as_str())?;
        let mut c = self.connection.clone();
        let key = self.key(&format!("sandbox:{sandbox}"));
        // Decode/validate outside Lua; compare exact bytes before the single write.
        // Preserve unknown JSON fields instead of reconstructing the typed record.
        for _ in 0..16 {
            let original: Option<String> = redis::cmd("GET").arg(&key).query_async(&mut c).await.map_err(redis_error)?;
            let Some(original) = original else { return Ok(OwnerAdoption::SandboxMissing); };
            let record: SandboxRecord = serde_json::from_str(&original).map_err(json_error)?;
            if record.sandbox_id != sandbox { return Err(StoreError("invalid sandbox adoption record".into())); }
            if let Some(current) = record.owner_id {
                return Ok(if current == *owner { OwnerAdoption::AlreadyOwned } else { OwnerAdoption::OwnerConflict });
            }
            let mut value: serde_json::Value = serde_json::from_str(&original).map_err(json_error)?;
            value["owner_id"] = serde_json::json!(owner);
            let replacement = serde_json::to_string(&value).map_err(json_error)?;
            let result: i64 = redis::cmd("EVAL").arg(r#"
local current=redis.call('GET',KEYS[1])
if not current then return 0 end
if current~=ARGV[1] then return 1 end
if redis.call('HLEN',KEYS[2])~=0 then return 2 end
if redis.call('PTTL',KEYS[1])~=-1 then return redis.error_reply('invalid expiring sandbox record') end
redis.call('SET',KEYS[1],ARGV[2]); return 3
"#).arg(2).arg(&key).arg(self.key(&format!("ports:{sandbox}")))
                .arg(&original).arg(replacement).query_async(&mut c).await.map_err(redis_error)?;
            match result {
                0 => return Ok(OwnerAdoption::SandboxMissing),
                1 => continue,
                2 => return Ok(OwnerAdoption::PortsPresent),
                3 => return Ok(OwnerAdoption::Adopted),
                _ => return Err(StoreError("invalid owner adoption result".into())),
            }
        }
        Err(StoreError("sandbox owner adoption contention; retry".into()))
    }



    async fn private_address_ledger(&self, source: &SandboxRecord) -> Result<MembershipAccess<Option<Vec<u8>>>> {
        crate::private_addresses::PrivateAddressBook::new(source).map_err(|e|StoreError(e.into()))?;
        let mut c = self.connection.clone();
        let (vm,row):(Option<String>,Option<Vec<u8>>) = redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2])}").arg(2)
            .arg(self.key(&format!("sandbox:{}",source.sandbox_id)))
            .arg(self.key(&format!("private-addresses:{}:{}",source.sandbox_id,source.started_at_ms)))
            .query_async(&mut c).await.map_err(redis_error)?;
        let Some(vm)=vm else { return Ok(MembershipAccess::SandboxMissing); };
        let record:SandboxRecord=serde_json::from_str(&vm).map_err(json_error)?;
        if record.owner_id!=source.owner_id { return Ok(MembershipAccess::OwnerConflict); }
        if !private_address_source_matches(source,&record) { return Err(StoreError("private address source changed".into())); }
        if let Some(bytes)=&row { crate::private_addresses::PrivateAddressBook::restore(bytes,source).map_err(|e|StoreError(e.into()))?; }
        Ok(MembershipAccess::Granted(row))
    }
    async fn compare_private_address_ledger(&self, source: &SandboxRecord, expected: Option<&[u8]>, next: &[u8]) -> Result<MembershipChange> {
        let canonical=crate::private_addresses::PrivateAddressBook::canonical_append(None,next,source).map_err(|e|StoreError(e.into()))?;
        let vm_key=self.key(&format!("sandbox:{}",source.sandbox_id));
        let row_key=self.key(&format!("private-addresses:{}:{}",source.sandbox_id,source.started_at_ms));
        let mut c=self.connection.clone();
        let (vm,row):(Option<String>,Option<Vec<u8>>)=redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2])}").arg(2).arg(&vm_key).arg(&row_key)
            .query_async(&mut c).await.map_err(redis_error)?;
        let Some(vm)=vm else { return Ok(MembershipChange::SandboxMissing); };
        let record:SandboxRecord=serde_json::from_str(&vm).map_err(json_error)?;
        if record.owner_id!=source.owner_id { return Ok(MembershipChange::OwnerConflict); }
        if !private_address_source_matches(source,&record) { return Ok(MembershipChange::RecordChanged); }
        let replay=row.as_ref()==Some(&canonical);
        if !replay && row.as_deref()!=expected { return Ok(MembershipChange::RevisionConflict); }
        crate::private_addresses::PrivateAddressBook::canonical_append(row.as_deref(),&canonical,source).map_err(|e|StoreError(e.into()))?;
        let result:i64=redis::cmd("EVAL").arg(r#"
local vm=redis.call('GET',KEYS[1])
if not vm then return 3 end
if vm~=ARGV[1] then return 4 end
local row=redis.call('GET',KEYS[2])
if (row or '')~=ARGV[2] then return 1 end
if redis.call('PTTL',KEYS[1])~=-1 then return redis.error_reply('expiring source record refused') end
if row and redis.call('PTTL',KEYS[2])~=-1 then return redis.error_reply('expiring address ledger refused') end
if ARGV[4]=='0' then redis.call('SET',KEYS[2],ARGV[3]) end
return 0
"#).arg(2).arg(vm_key).arg(row_key).arg(vm).arg(row.unwrap_or_default()).arg(canonical).arg(u8::from(replay))
            .query_async(&mut c).await.map_err(redis_error)?;
        match result { 0=>Ok(MembershipChange::Applied),1=>Ok(MembershipChange::RevisionConflict),3=>Ok(MembershipChange::SandboxMissing),4=>Ok(MembershipChange::RecordChanged),_=>Err(StoreError("invalid address ledger transaction result".into())) }
    }

    async fn private_route_snapshot_with_live_nodes(&self, source: &str, destination: &str,
        source_node: &str, destination_node: &str) -> Result<Option<PrivateRouteSnapshot>> {
        crate::ports::validate_request(source,1,"private-route")?;
        crate::ports::validate_request(destination,1,"private-route")?;
        let mut c=self.connection.clone();
        let (sr,dr,sm,dm,sn,dn):(Option<String>,Option<String>,Option<String>,Option<String>,Option<String>,Option<String>)=redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2]),redis.call('GET',KEYS[3]),redis.call('GET',KEYS[4]),redis.call('GET',KEYS[5]),redis.call('GET',KEYS[6])}")
            .arg(6).arg(self.key(&format!("sandbox:{source}"))).arg(self.key(&format!("sandbox:{destination}")))
            .arg(self.key(&format!("private-membership:{source}"))).arg(self.key(&format!("private-membership:{destination}")))
            .arg(self.key(&format!("node:{source_node}"))).arg(self.key(&format!("node:{destination_node}")))
            .query_async(&mut c).await.map_err(redis_error)?;
        let (Some(sr),Some(dr),Some(sm),Some(dm))=(sr,dr,sm,dm) else { return Ok(None); };
        let Some(snapshot)=PrivateRouteSnapshot::checked(source,destination,
            serde_json::from_str(&sr).map_err(json_error)?,serde_json::from_str(&dr).map_err(json_error)?,
            serde_json::from_str(&sm).map_err(json_error)?,serde_json::from_str(&dm).map_err(json_error)?) else { return Ok(None); };
        // Decode node JSON exactly as node(); malformed live records fail closed.
        let source_live:Option<NodeInfo>=sn.map(|value|serde_json::from_str(&value).map_err(json_error)).transpose()?;
        if source_live.is_none() { return Ok(None); }
        let destination_live:Option<NodeInfo>=dn.map(|value|serde_json::from_str(&value).map_err(json_error)).transpose()?;
        if destination_live.is_none() { return Ok(None); }
        Ok(Some(snapshot))
    }

    async fn private_route_snapshot(&self, source: &str, destination: &str)
        -> Result<Option<PrivateRouteSnapshot>> {
        crate::ports::validate_request(source,1,"private-route")?;
        crate::ports::validate_request(destination,1,"private-route")?;
        let mut c=self.connection.clone();
        let (sr,dr,sm,dm):(Option<String>,Option<String>,Option<String>,Option<String>)=redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2]),redis.call('GET',KEYS[3]),redis.call('GET',KEYS[4])}")
            .arg(4).arg(self.key(&format!("sandbox:{source}")))
            .arg(self.key(&format!("sandbox:{destination}")))
            .arg(self.key(&format!("private-membership:{source}")))
            .arg(self.key(&format!("private-membership:{destination}")))
            .query_async(&mut c).await.map_err(redis_error)?;
        let (Some(sr),Some(dr),Some(sm),Some(dm))=(sr,dr,sm,dm) else { return Ok(None); };
        Ok(PrivateRouteSnapshot::checked(source,destination,
            serde_json::from_str(&sr).map_err(json_error)?,serde_json::from_str(&dr).map_err(json_error)?,
            serde_json::from_str(&sm).map_err(json_error)?,serde_json::from_str(&dm).map_err(json_error)?))
    }

    async fn private_membership(&self, sandbox: &str, owner: &crate::ownership::OwnerId)
        -> Result<MembershipAccess<Option<NetworkMembershipState>>> {
        crate::ports::validate_request(sandbox, 1, owner.as_str())?;
        let mut c = self.connection.clone();
        let (vm, row): (Option<String>, Option<String>) = redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2])}")
            .arg(2).arg(self.key(&format!("sandbox:{sandbox}")))
            .arg(self.key(&format!("private-membership:{sandbox}")))
            .query_async(&mut c).await.map_err(redis_error)?;
        let Some(vm) = vm else { return Ok(MembershipAccess::SandboxMissing); };
        let record: SandboxRecord = serde_json::from_str(&vm).map_err(json_error)?;
        if record.sandbox_id != sandbox { return Err(StoreError("invalid private membership sandbox record".into())); }
        if record.owner_id.as_ref() != Some(owner) { return Ok(MembershipAccess::OwnerConflict); }
        let row: Option<NetworkMembershipState> = row.map(|row| serde_json::from_str(&row).map_err(json_error)).transpose()?;
        if row.as_ref().is_some_and(|row| row.sandbox_id() != sandbox) {
            return Err(StoreError("invalid private membership identity".into()));
        }
        if row.as_ref().is_some_and(|row| row.owner_id() != owner) { return Ok(MembershipAccess::OwnerConflict); }
        Ok(MembershipAccess::Granted(row))
    }
    async fn compare_private_membership(&self, expected: Option<&str>, next: &NetworkMembershipState)
        -> Result<MembershipChange> {
        let mut c = self.connection.clone();
        let vm_key = self.key(&format!("sandbox:{}", next.sandbox_id()));
        let row_key = self.key(&format!("private-membership:{}", next.sandbox_id()));
        // Read/decode before the script; byte comparisons fence both snapshots
        // against concurrent writers. JSON numbers are never compared in Lua.
        let (vm, row): (Option<String>, Option<String>) = redis::cmd("EVAL")
            .arg("return {redis.call('GET',KEYS[1]),redis.call('GET',KEYS[2])}")
            .arg(2).arg(&vm_key).arg(&row_key).query_async(&mut c).await.map_err(redis_error)?;
        let Some(vm) = vm else { return Ok(MembershipChange::SandboxMissing); };
        let record: SandboxRecord = serde_json::from_str(&vm).map_err(json_error)?;
        if record.owner_id.as_ref() != Some(next.owner_id()) { return Ok(MembershipChange::OwnerConflict); }
        if !next.matches_record(&record) { return Ok(MembershipChange::RecordChanged); }
        let current: Option<NetworkMembershipState> = row.as_ref()
            .map(|row| serde_json::from_str(row).map_err(json_error)).transpose()?;
        if current.as_ref().is_some_and(|row| row.sandbox_id() != next.sandbox_id()) {
            return Err(StoreError("invalid private membership identity".into()));
        }
        if current.as_ref().is_some_and(|row| row.owner_id() != next.owner_id()) { return Ok(MembershipChange::OwnerConflict); }
        let replay = current.as_ref() == Some(next);
        if !replay && (current.as_ref().map(|row| row.revision()) != expected
            || current.as_ref().is_some_and(|row| row.revision() == next.revision())) {
            return Ok(MembershipChange::RevisionConflict);
        }
        let encoded = serde_json::to_string(next).map_err(json_error)?;
        let result: i64 = redis::cmd("EVAL").arg(r#"
local vm=redis.call('GET',KEYS[1])
if not vm then return 3 end
if vm~=ARGV[1] then return 4 end
local row=redis.call('GET',KEYS[2])
if (row or '')~=ARGV[2] then return 1 end
if ARGV[4]=='1' then return 0 end
redis.call('SET',KEYS[2],ARGV[3])
return 0
"#).arg(2).arg(&vm_key).arg(&row_key)
            .arg(vm).arg(row.unwrap_or_default()).arg(encoded).arg(u8::from(replay))
            .query_async(&mut c).await.map_err(redis_error)?;
        match result {
            0 => Ok(MembershipChange::Applied), 1 => Ok(MembershipChange::RevisionConflict),
            3 => Ok(MembershipChange::SandboxMissing), 4 => Ok(MembershipChange::RecordChanged),
            _ => Err(StoreError("invalid private membership transaction result".into())),
        }
    }

    async fn put_sandbox(&self, record: &SandboxRecord) -> Result<()> {
        let json = serde_json::to_string(record).map_err(json_error)?;
        let mut c = self.connection.clone();
        redis::cmd("EVAL").arg(r#"
local updated=cjson.decode(ARGV[2])
local encoded=ARGV[2]
local index_type=redis.call('TYPE',KEYS[2]);if type(index_type)=='table' then index_type=index_type.ok end
if index_type~='none' and index_type~='set' then return redis.error_reply('invalid sandbox inventory index type') end
local current=redis.call('GET',KEYS[1])
if current and updated.owner_id==nil then
 local previous=cjson.decode(current)
 if previous.sandbox_id~=ARGV[1] then return redis.error_reply('invalid sandbox record identity') end
 if previous.owner_id~=nil and previous.owner_id~=cjson.null then
  local owner=previous.owner_id
  if type(owner)~='string' or #owner<1 or #owner>128 or owner:find('[^%w_.%-]') then return redis.error_reply('invalid sandbox owner') end
  -- Rust omits a None owner and serializes an object. Append only that field;
  -- re-encoding the whole record in Lua would change empty JSON arrays.
  encoded=string.sub(encoded,1,-2)..',"owner_id":'..cjson.encode(owner)..'}'
 end
end
if type(redis.acl_check_cmd)~='function' or not redis.acl_check_cmd('SET',KEYS[1],encoded) or
 not redis.acl_check_cmd('SADD',KEYS[2],ARGV[1]) then return redis.error_reply('sandbox write preflight denied') end
redis.call('SET',KEYS[1],encoded);redis.call('SADD',KEYS[2],ARGV[1]);return 1
"#).arg(2).arg(self.key(&format!("sandbox:{}", record.sandbox_id))).arg(self.key("sandboxes"))
            .arg(&record.sandbox_id).arg(json).query_async::<i64>(&mut c).await.map(|_|()).map_err(redis_error)
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
-- A script error does not undo earlier writes, including ACL denials.
-- Fail closed on servers without the Redis 7 ACL preflight capability.
if not redis.acl_check_cmd then
    return redis.error_reply('named registration requires ACL preflight support')
end
local writes = {
    {'SET', KEYS[2], ARGV[4]},
    {'SADD', KEYS[3], ARGV[3]},
    {'HSET', KEYS[1], ARGV[1], ARGV[5]},
    {'SADD', KEYS[4], ARGV[1]}
}
for _, command in ipairs(writes) do
    if not redis.acl_check_cmd(unpack(command)) then
        return redis.error_reply('named registration write permission refused')
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
local port_type=redis.call('TYPE',KEYS[7]); port_type=type(port_type)=='table' and port_type.ok or port_type
local public_type=redis.call('TYPE',KEYS[8]); public_type=type(public_type)=='table' and public_type.ok or public_type
if (port_type~='none' and port_type~='hash') or (public_type~='none' and public_type~='hash') then
 return redis.error_reply('invalid managed-port cleanup index type')
end
local owned_ports={}
local port_values=redis.call('HVALS',KEYS[7])
for _,value in ipairs(port_values) do
 local row=cjson.decode(value)
 if row.sandbox_id~=ARGV[1] or type(row.public_port)~='number' or row.public_port<1 or row.public_port>65535 or row.public_port%1~=0 then return redis.error_reply('invalid managed-port cleanup record') end
 local field=tostring(row.public_port)
 if redis.call('HGET',KEYS[8],field)==value then table.insert(owned_ports,field) end
end
if #port_values>0 then
 if type(redis.acl_check_cmd)~='function' or not redis.acl_check_cmd('DEL',KEYS[7]) then
  return redis.error_reply('managed-port cleanup preflight denied')
 end
 for _,field in ipairs(owned_ports) do
  if not redis.acl_check_cmd('HDEL',KEYS[8],field) then return redis.error_reply('managed-port cleanup preflight denied') end
 end
end
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
for _,field in ipairs(owned_ports) do redis.call('HDEL',KEYS[8],field) end
if #port_values>0 then redis.call('DEL',KEYS[7]) end
local deleted = redis.call('DEL', KEYS[1])
redis.call('SREM', KEYS[2], ARGV[1])
return deleted
"#,
            )
            .arg(8)
            .arg(self.key(&format!("sandbox:{id}")))
            .arg(self.key("sandboxes"))
            .arg(self.key(&format!("domains:{id}")))
            .arg(self.key("domains"))
            .arg(self.key(&format!("reserved-names:{id}")))
            .arg(self.key("name-reservations"))
            .arg(self.key(&format!("ports:{id}")))
            .arg(self.key("public-ports"))
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

    async fn claim_port(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange) -> Result<PortClaim> {
        self.claim_port_inner(sandbox, machine_port, owner, protocol, range, false).await
    }
    async fn claim_owned_port(&self, sandbox: &str, machine_port: u16, owner: &str,
        protocol: PortProtocol, range: PublicPortRange) -> Result<PortClaim> {
        self.claim_port_inner(sandbox, machine_port, owner, protocol, range, true).await
    }

    async fn port_allocations(&self, sandbox: Option<&str>) -> Result<Vec<PortAllocation>> {
        let mut c = self.connection.clone();
        let key = sandbox.map_or_else(|| self.key("public-ports"), |id| self.key(&format!("ports:{id}")));
        let values: HashMap<String, String> = redis::cmd("HGETALL").arg(key).query_async(&mut c).await.map_err(redis_error)?;
        let mut rows = Vec::with_capacity(values.len());
        for (field, value) in values {
            let row: PortAllocation = serde_json::from_str(&value).map_err(json_error)?;
            row.validate()?;
            let expected = if sandbox.is_some() { row.machine_port } else { row.public_port };
            if field != expected.to_string() { return Err(StoreError("managed-port field index disagrees".into())); }
            if sandbox.is_some_and(|id| row.sandbox_id != id) { return Err(StoreError("managed-port owner index disagrees".into())); }
            rows.push(row);
        }
        rows.sort_by_key(|row| row.public_port);
        Ok(rows)
    }

    async fn delete_port(&self, sandbox: &str, machine_port: u16, owner: &str) -> Result<bool> {
        match self.delete_port_inner(sandbox, machine_port, owner, false).await? {
            OwnedPortAccess::Granted(value) => Ok(value), _ => Ok(false),
        }
    }
    async fn delete_owned_port(&self, sandbox: &str, machine_port: u16, owner: &str) -> Result<OwnedPortAccess<bool>> {
        self.delete_port_inner(sandbox, machine_port, owner, true).await
    }

    async fn owned_ports(&self, sandbox: &str, owner: &str) -> Result<OwnedPortAccess<Vec<PortAllocation>>> {
        crate::ports::validate_request(sandbox, 1, owner)?;
        let mut connection = self.connection.clone();
        let (status, values): (i64, Vec<String>) = redis::cmd("EVAL").arg(r#"
local encoded=redis.call('GET',KEYS[1]); if not encoded then return {0,{}} end
local vm=cjson.decode(encoded)
if vm.sandbox_id~=ARGV[1] then return redis.error_reply('invalid sandbox ownership record') end
if vm.owner_id~=ARGV[2] then return {2,{}} end
if redis.call('HLEN',KEYS[2])>tonumber(ARGV[3]) then return redis.error_reply('owned-port snapshot over capacity') end
local values=redis.call('HGETALL',KEYS[2])
for i=1,#values,2 do
 local row=cjson.decode(values[i+1])
 if row.owner_id~=ARGV[2] or redis.call('HGET',KEYS[3],tostring(row.public_port))~=values[i+1] then
  return redis.error_reply('owned-port indexes disagree')
 end
end
return {1,values}
"#).arg(3).arg(self.key(&format!("sandbox:{sandbox}"))).arg(self.key(&format!("ports:{sandbox}")))
            .arg(self.key("public-ports")).arg(sandbox).arg(owner).arg(MAX_PORTS_PER_SANDBOX)
            .query_async(&mut connection).await.map_err(redis_error)?;
        match status {
            0 => Ok(OwnedPortAccess::SandboxMissing), 2 => Ok(OwnedPortAccess::OwnerConflict),
            1 => {
                if values.len()%2!=0 { return Err(StoreError("invalid owned-port snapshot".into())); }
                let mut rows=Vec::with_capacity(values.len()/2);
                for pair in values.chunks_exact(2) {
                    let row: PortAllocation=serde_json::from_str(&pair[1]).map_err(json_error)?;
                    row.validate()?;
                    if row.sandbox_id!=sandbox || row.owner_id!=owner || pair[0]!=row.machine_port.to_string() { return Err(StoreError("invalid owned-port snapshot".into())); }
                    rows.push(row);
                }
                rows.sort_by_key(|row|row.public_port);
                Ok(OwnedPortAccess::Granted(rows))
            }
            _ => Err(StoreError("unexpected owned-port listing result".into())),
        }
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
            owner_id: None,
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


    async fn private_address_ledger_contract(store: &dyn ClusterStore) {
        use crate::{private_addresses::PrivateAddressBook, private_networks::NetworkTag, ownership::OwnerId};
        let mut source=sandbox("address-source","address-node"); source.owner_id=Some(OwnerId::parse("address-owner").unwrap());
        let mut destination=sandbox("address-destination","address-node"); destination.owner_id=source.owner_id.clone();
        let book=PrivateAddressBook::new(&source).unwrap(); let empty=book.snapshot().unwrap();
        assert_eq!(store.private_address_ledger(&source).await.unwrap(),MembershipAccess::SandboxMissing);
        assert_eq!(store.compare_private_address_ledger(&source,None,&empty).await.unwrap(),MembershipChange::SandboxMissing);
        for record in [&source,&destination] { store.put_sandbox(record).await.unwrap(); }
        let tags=||vec![NetworkTag::parse("team").unwrap(),NetworkTag::parse("dev").unwrap()];
        for record in [&source,&destination] {
            let m=NetworkMembershipState::new(record,Some(tags())).unwrap();
            assert_eq!(store.compare_private_membership(None,&m).await.unwrap(),MembershipChange::Applied);
        }
        assert_eq!(store.private_address_ledger(&source).await.unwrap(),MembershipAccess::Granted(None));
        assert_eq!(store.compare_private_address_ledger(&source,None,&empty).await.unwrap(),MembershipChange::Applied);
        assert_eq!(store.compare_private_address_ledger(&source,None,&empty).await.unwrap(),MembershipChange::Applied);
        let mut forged=source.clone(); forged.owner_id=Some(OwnerId::parse("other-owner").unwrap());
        let forged_bytes=PrivateAddressBook::new(&forged).unwrap().snapshot().unwrap();
        assert_eq!(store.private_address_ledger(&forged).await.unwrap(),MembershipAccess::OwnerConflict);
        assert_eq!(store.compare_private_address_ledger(&forged,None,&forged_bytes).await.unwrap(),MembershipChange::OwnerConflict);
        let view=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        let left=PrivateAddressBook::restore(&empty,&source).unwrap(); let right=PrivateAddressBook::restore(&empty,&source).unwrap();
        let ip=left.allocate(&view,NetworkTag::parse("team").unwrap(),crate::model::now_ms(),false,false).unwrap();
        right.allocate(&view,NetworkTag::parse("dev").unwrap(),crate::model::now_ms(),false,false).unwrap();
        let l=left.snapshot().unwrap(); let r=right.snapshot().unwrap();
        let (a,b)=tokio::join!(store.compare_private_address_ledger(&source,Some(&empty),&l),store.compare_private_address_ledger(&source,Some(&empty),&r));
        let (a,b)=(a.unwrap(),b.unwrap());
        assert!(matches!((a,b),(MembershipChange::Applied,MembershipChange::RevisionConflict)|(MembershipChange::RevisionConflict,MembershipChange::Applied)));
        let winner=if a==MembershipChange::Applied { l } else { r };
        assert_eq!(store.private_address_ledger(&source).await.unwrap(),MembershipAccess::Granted(Some(winner.clone())));
        assert_eq!(store.compare_private_address_ledger(&source,Some(&empty),&winner).await.unwrap(),MembershipChange::Applied);
        assert_eq!(store.compare_private_address_ledger(&source,None,&empty).await.unwrap(),MembershipChange::RevisionConflict);
        assert!(store.compare_private_address_ledger(&source,Some(&winner),&empty).await.is_err());
        let restored=PrivateAddressBook::restore(&winner,&source).unwrap();
        let original=restored.binding(ip,8080).unwrap();
        let next_tag=if original.network.as_str()=="team" { "dev" } else { "team" };
        let second=restored.allocate(&view,NetworkTag::parse(next_tag).unwrap(),crate::model::now_ms(),false,false).unwrap();
        assert_ne!(ip,second);
        let next=restored.snapshot().unwrap();
        assert_eq!(store.compare_private_address_ledger(&source,Some(&winner),&next).await.unwrap(),MembershipChange::Applied);
        assert_eq!(store.compare_private_address_ledger(&source,Some(&empty),&winner).await.unwrap(),MembershipChange::RevisionConflict);
        let mut corrupt:serde_json::Value=serde_json::from_slice(&next).unwrap();
        corrupt["entries"][0]["destination_generation"]=uuid::Uuid::new_v4().to_string().into();
        assert!(store.compare_private_address_ledger(&source,Some(&next),&serde_json::to_vec(&corrupt).unwrap()).await.is_err());
        // Placement change refuses old-node writers while preserving the ledger.
        let mut moved=source.clone();moved.node_id="moved-address-node".into();store.put_sandbox(&moved).await.unwrap();
        assert_eq!(store.compare_private_address_ledger(&source,Some(&next),&next).await.unwrap(),MembershipChange::RecordChanged);
        assert_eq!(store.private_address_ledger(&moved).await.unwrap(),MembershipAccess::Granted(Some(next.clone())));
        store.delete_sandbox(&source.sandbox_id).await.unwrap();
        assert_eq!(store.private_address_ledger(&moved).await.unwrap(),MembershipAccess::SandboxMissing);
        assert_eq!(store.compare_private_address_ledger(&moved,Some(&next),&next).await.unwrap(),MembershipChange::SandboxMissing);
        store.put_sandbox(&moved).await.unwrap();
        assert_eq!(store.private_address_ledger(&moved).await.unwrap(),MembershipAccess::Granted(Some(next.clone())));
        let mut replacement=moved.clone();replacement.started_at_ms+=1;store.put_sandbox(&replacement).await.unwrap();
        assert_eq!(store.private_address_ledger(&replacement).await.unwrap(),MembershipAccess::Granted(None));
        assert_eq!(store.compare_private_address_ledger(&moved,Some(&next),&next).await.unwrap(),MembershipChange::RecordChanged);
        let fresh=PrivateAddressBook::new(&replacement).unwrap().snapshot().unwrap();
        assert_eq!(store.compare_private_address_ledger(&replacement,None,&fresh).await.unwrap(),MembershipChange::Applied);
        store.delete_sandbox(&source.sandbox_id).await.unwrap();
        store.delete_sandbox(&destination.sandbox_id).await.unwrap();
        eprintln!("private_address_ledger_contract completed concurrent_single_winner=true rollback=refused replay=exact migration=preserved deleted=refused replacement=separate");
    }

    async fn private_membership_contract(store: &dyn ClusterStore) {
        use crate::private_networks::{NetworkMembershipState as State, NetworkTag};
        use crate::ownership::OwnerId;
        let owner = OwnerId::parse("network-owner").unwrap();
        let other = OwnerId::parse("network-other").unwrap();
        let mut r = sandbox("private-member", "network-node");
        let tags = || Some(vec![NetworkTag::parse("team").unwrap()]);
        assert_eq!(store.private_membership(&r.sandbox_id, &owner).await.unwrap(), MembershipAccess::SandboxMissing);
        store.put_sandbox(&r).await.unwrap();
        assert!(State::new(&r, tags()).is_err());
        assert_eq!(store.private_membership(&r.sandbox_id, &owner).await.unwrap(), MembershipAccess::OwnerConflict);
        r.owner_id = Some(owner.clone()); store.put_sandbox(&r).await.unwrap();
        let initial = State::new(&r, tags()).unwrap();
        assert_eq!(store.compare_private_membership(None, &initial).await.unwrap(), MembershipChange::Applied);
        assert_eq!(store.compare_private_membership(None, &initial).await.unwrap(), MembershipChange::Applied);
        assert_eq!(store.private_membership(&r.sandbox_id, &other).await.unwrap(), MembershipAccess::OwnerConflict);
        assert_eq!(store.private_membership(&r.sandbox_id, &owner).await.unwrap(), MembershipAccess::Granted(Some(initial.clone())));
        let mut forged = r.clone(); forged.owner_id = Some(other);
        assert_eq!(store.compare_private_membership(Some(initial.revision()), &State::new(&forged, tags()).unwrap()).await.unwrap(), MembershipChange::OwnerConflict);
        let left = State::new(&r, tags()).unwrap(); let right = State::new(&r, tags()).unwrap();
        let (a,b) = tokio::join!(store.compare_private_membership(Some(initial.revision()), &left),
            store.compare_private_membership(Some(initial.revision()), &right));
        let (a,b) = (a.unwrap(), b.unwrap());
        assert!(matches!((a,b), (MembershipChange::Applied, MembershipChange::RevisionConflict)
            | (MembershipChange::RevisionConflict, MembershipChange::Applied)));
        let winner = if a == MembershipChange::Applied { &left } else { &right };
        let removed = State::new(&r, None).unwrap();
        assert_eq!(store.compare_private_membership(Some(winner.revision()), &removed).await.unwrap(), MembershipChange::Applied);
        assert!(removed.membership().is_none());
        assert_eq!(store.compare_private_membership(None, &initial).await.unwrap(), MembershipChange::RevisionConflict);
        assert_eq!(store.compare_private_membership(Some(initial.revision()), &left).await.unwrap(), MembershipChange::RevisionConflict);
        assert_eq!(store.private_membership(&r.sandbox_id, &owner).await.unwrap(), MembershipAccess::Granted(Some(removed.clone())));
        let mut migrated = r.clone(); migrated.node_id = "network-node-2".into();
        store.put_sandbox(&migrated).await.unwrap();
        assert_eq!(store.compare_private_membership(Some(removed.revision()), &left).await.unwrap(), MembershipChange::RecordChanged);
        let joined = State::new(&migrated, tags()).unwrap();
        assert_eq!(store.compare_private_membership(Some(removed.revision()), &joined).await.unwrap(), MembershipChange::Applied);
        assert!(store.delete_sandbox(&r.sandbox_id).await.unwrap());
        assert_eq!(store.private_membership(&r.sandbox_id, &owner).await.unwrap(), MembershipAccess::SandboxMissing);
        assert_eq!(store.compare_private_membership(Some(joined.revision()), &removed).await.unwrap(), MembershipChange::SandboxMissing);
        // Even deliberate ID reuse cannot replay the old VM incarnation.
        migrated.started_at_ms += 1; store.put_sandbox(&migrated).await.unwrap();
        assert_eq!(store.compare_private_membership(Some(joined.revision()), &joined).await.unwrap(), MembershipChange::RecordChanged);
        let fresh = State::new(&migrated, tags()).unwrap();
        assert_eq!(store.compare_private_membership(None, &fresh).await.unwrap(), MembershipChange::RevisionConflict);
        assert_eq!(store.compare_private_membership(Some(joined.revision()), &fresh).await.unwrap(), MembershipChange::Applied);
        assert!(store.delete_sandbox(&r.sandbox_id).await.unwrap());
    }


    async fn private_route_live_snapshot_contract(store: &dyn ClusterStore) {
        use crate::private_networks::{NetworkMembershipState as State, NetworkTag};
        use crate::ownership::OwnerId;
        let mut source=sandbox("live-route-source","live-route-node-a");
        let mut destination=sandbox("live-route-destination","live-route-node-b");
        source.owner_id=Some(OwnerId::parse("live-route-owner").unwrap());destination.owner_id=source.owner_id.clone();
        for record in [&source,&destination] {
            store.put_sandbox(record).await.unwrap();
            let member=State::new(record,Some(vec![NetworkTag::parse("team").unwrap()])).unwrap();
            assert_eq!(store.compare_private_membership(None,&member).await.unwrap(),MembershipChange::Applied);
        }
        let view=||store.private_route_snapshot_with_live_nodes(&source.sandbox_id,&destination.sandbox_id,&source.node_id,&destination.node_id);
        assert!(view().await.unwrap().is_none());
        let sn=node(&source.node_id,1,10);let dn=node(&destination.node_id,1,10);
        store.put_node(&sn,Duration::from_secs(60)).await.unwrap();assert!(view().await.unwrap().is_none());
        store.put_node(&dn,Duration::from_secs(60)).await.unwrap();
        let snapshot=view().await.unwrap().unwrap();
        assert!(snapshot.claim(NetworkTag::parse("team").unwrap(),8080,now_ms(),false,false).is_ok());
        store.remove_node(&source.node_id).await.unwrap();assert!(view().await.unwrap().is_none());
        store.put_node(&sn,Duration::from_secs(1)).await.unwrap();tokio::time::sleep(Duration::from_millis(1100)).await;
        assert!(view().await.unwrap().is_none());
        store.put_node(&sn,Duration::from_secs(60)).await.unwrap();store.remove_node(&destination.node_id).await.unwrap();
        assert!(view().await.unwrap().is_none());
        store.put_node(&dn,Duration::from_secs(60)).await.unwrap();assert!(view().await.unwrap().is_some());
        assert!(store.private_route_snapshot_with_live_nodes("missing-live-source",&destination.sandbox_id,&source.node_id,&destination.node_id).await.unwrap().is_none());
        store.remove_node(&source.node_id).await.unwrap();store.remove_node(&destination.node_id).await.unwrap();
        store.delete_sandbox(&source.sandbox_id).await.unwrap();store.delete_sandbox(&destination.sandbox_id).await.unwrap();
        println!("private_route_live_snapshot_contract completed");
    }

    async fn private_route_snapshot_contract(store: &dyn ClusterStore) {
        use crate::private_networks::{NetworkMembershipState as State, NetworkTag};
        use crate::ownership::OwnerId;
        let now=now_ms();
        let mut source=sandbox("route-source","route-node-a");
        let mut destination=sandbox("route-destination","route-node-b");
        source.owner_id=Some(OwnerId::parse("route-alice").unwrap());
        destination.owner_id=source.owner_id.clone();
        let mut other=sandbox("route-other-owner","route-node-b");
        other.owner_id=Some(OwnerId::parse("route-bob").unwrap());
        for r in [&source,&destination,&other] { store.put_sandbox(r).await.unwrap(); }
        let tag=|| NetworkTag::parse("team").unwrap();
        let member=|r:&SandboxRecord| State::new(r,Some(vec![tag()])).unwrap();
        assert!(store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().is_none());
        let sm=member(&source);let dm=member(&destination);let om=member(&other);
        for m in [&sm,&dm,&om] { assert_eq!(store.compare_private_membership(None,m).await.unwrap(),MembershipChange::Applied); }
        let view=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        let claim=view.claim(tag(),8080,now_ms(),false,false).unwrap();
        assert!(view.authorize(&source.sandbox_id,&claim,now_ms(),false,false).is_ok());
        assert!(view.authorize("forged-source",&claim,now_ms(),false,false).is_err());
        assert!(view.authorize(&source.sandbox_id,&claim,now_ms(),true,false).is_err());
        assert!(view.authorize(&source.sandbox_id,&claim,now_ms(),false,true).is_err());
        assert!(view.claim(tag(),0,now_ms(),false,false).is_err());
        assert!(view.claim(NetworkTag::parse("other").unwrap(),8080,now_ms(),false,false).is_err());
        assert!(store.private_route_snapshot(&source.sandbox_id,&other.sandbox_id).await.unwrap().is_none());
        assert!(store.private_route_snapshot("missing-source",&destination.sandbox_id).await.unwrap().is_none());
        assert!(store.private_route_snapshot("bad/source",&destination.sandbox_id).await.is_err());
        // Even rejoining the same tag invalidates the previous route claim.
        let rejoined=member(&source);
        assert_eq!(store.compare_private_membership(Some(sm.revision()),&rejoined).await.unwrap(),MembershipChange::Applied);
        let fresh=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        assert!(fresh.authorize(&source.sandbox_id,&claim,now_ms(),false,false).is_err());
        let claim=fresh.claim(tag(),8080,now_ms(),false,false).unwrap();
        source.paused=true;store.put_sandbox(&source).await.unwrap();
        let paused=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        assert!(paused.authorize(&source.sandbox_id,&claim,now_ms(),false,false).is_err());
        source.paused=false;store.put_sandbox(&source).await.unwrap();
        destination.end_at_ms=now;store.put_sandbox(&destination).await.unwrap();
        let expired=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        assert!(expired.authorize(&source.sandbox_id,&claim,now_ms(),false,false).is_err());
        destination.end_at_ms=now_ms()+60_000;destination.node_id="route-node-c".into();store.put_sandbox(&destination).await.unwrap();
        assert!(store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().is_none());
        let moved=member(&destination);
        assert_eq!(store.compare_private_membership(Some(dm.revision()),&moved).await.unwrap(),MembershipChange::Applied);
        let migrated=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        assert!(migrated.authorize(&source.sandbox_id,&claim,now_ms(),false,false).is_err());
        assert!(migrated.claim(tag(),8080,now_ms(),false,false).is_ok());
        let isolated=State::new(&destination,Some(vec![NetworkTag::parse("other").unwrap()])).unwrap();
        assert_eq!(store.compare_private_membership(Some(moved.revision()),&isolated).await.unwrap(),MembershipChange::Applied);
        let separated=store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().unwrap();
        assert!(separated.claim(tag(),8080,now_ms(),false,false).is_err());
        assert!(separated.claim(NetworkTag::parse("other").unwrap(),8080,now_ms(),false,false).is_err());
        let removed=State::new(&destination,None).unwrap();
        assert_eq!(store.compare_private_membership(Some(isolated.revision()),&removed).await.unwrap(),MembershipChange::Applied);
        assert!(store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().is_none());
        for r in [&source,&destination,&other] { assert!(store.delete_sandbox(&r.sandbox_id).await.unwrap()); }
        assert!(store.private_route_snapshot(&source.sandbox_id,&destination.sandbox_id).await.unwrap().is_none());
        eprintln!("private_route_contract completed source_rejoin=refused cross_owner=refused cross_network=refused pause=refused expiration=refused stale_migration=refused removal=absent deletion=absent");
    }

    /// The contract, run against any store.
    pub(crate) async fn contract(store: &dyn ClusterStore) {
        private_address_ledger_contract(store).await;
        private_membership_contract(store).await;
        private_route_snapshot_contract(store).await;
        private_route_live_snapshot_contract(store).await;
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
        let restricted_url =
            url.replacen("redis://", &format!("redis://{username}:{password}@"), 1);
        let restricted = RedisStore::connect(&restricted_url, &namespace)
            .await
            .unwrap();
        let mut outcomes = Vec::new();
        for denied in ["set", "sadd", "hset"] {
            let name = SandboxName::parse(&format!("acl-{denied}")).unwrap();
            let owner = NameReservation::pending(name.clone());
            let mut record = sandbox(&format!("acl-{denied}-vm"), "a");
            record
                .metadata
                .insert("hm.name".into(), name.as_str().into());
            assert!(store.reserve_name(&owner).await.unwrap());
            redis::cmd("ACL")
                .arg("SETUSER")
                .arg(&username)
                .arg(format!("-{denied}"))
                .query_async::<()>(&mut admin)
                .await
                .unwrap();
            let refused = restricted
                .register_named_sandbox(&record, &owner)
                .await
                .is_err();
            let absent = store.sandbox(&record.sandbox_id).await.unwrap().is_none();
            let pending = store.name_reservation(&name).await.unwrap().as_ref() == Some(&owner);
            let indexed: bool = redis::cmd("SISMEMBER")
                .arg(store.key("sandboxes"))
                .arg(&record.sandbox_id)
                .query_async(&mut admin)
                .await
                .unwrap();
            let names: u64 = redis::cmd("SCARD")
                .arg(store.key(&format!("reserved-names:{}", record.sandbox_id)))
                .query_async(&mut admin)
                .await
                .unwrap();
            outcomes.push((
                denied,
                refused && absent && pending && !indexed && names == 0,
            ));
            redis::cmd("ACL")
                .arg("SETUSER")
                .arg(&username)
                .arg(format!("+{denied}"))
                .query_async::<()>(&mut admin)
                .await
                .unwrap();
            // Clean even the old script's partial writes before asserting.
            store.delete_sandbox(&record.sandbox_id).await.unwrap();
            store
                .release_pending_name(&name, owner.operation_token())
                .await
                .unwrap();
        }
        redis::cmd("ACL")
            .arg("DELUSER")
            .arg(&username)
            .query_async::<u64>(&mut admin)
            .await
            .unwrap();
        for (denied, intact) in outcomes {
            assert!(
                intact,
                "registration partially wrote after denying {denied}"
            );
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
    fn allocated(claim: PortClaim) -> PortAllocation {
        match claim { PortClaim::Allocated(row) => row, other => panic!("unexpected claim: {other:?}") }
    }

    async fn managed_port_contract(store: std::sync::Arc<dyn ClusterStore>) {
        let range = PublicPortRange::new(30000, 30003).unwrap();
        assert!(PublicPortRange::new(0, 10).is_err());
        assert!(PublicPortRange::new(10, 9).is_err());
        assert!(PublicPortRange::new(1, 4097).is_err());
        assert_eq!(store.claim_port("absent", 5353, "owner-a", PortProtocol::Udp, range).await.unwrap(), PortClaim::SandboxMissing);
        store.put_sandbox(&sandbox("ports-a", "node")).await.unwrap();
        store.put_sandbox(&sandbox("ports-b", "node")).await.unwrap();
        let first = allocated(store.claim_port("ports-a", 5353, "owner-a", PortProtocol::Tcp, range).await.unwrap());
        let both = allocated(store.claim_port("ports-a", 5353, "owner-a", PortProtocol::Both, range).await.unwrap());
        assert_eq!(first.public_port(), both.public_port());
        assert_eq!(both.protocol(), PortProtocol::Both);
        assert_eq!(store.claim_port("ports-a", 5353, "owner-b", PortProtocol::Udp, range).await.unwrap(), PortClaim::OwnerConflict);
        assert!(!store.delete_port("ports-a", 5353, "owner-b").await.unwrap());
        assert_eq!(store.port_allocations(Some("ports-a")).await.unwrap(), vec![both.clone()]);
        let second = allocated(store.claim_port("ports-b", 5353, "owner-b", PortProtocol::Udp, range).await.unwrap());
        assert_ne!(first.public_port(), second.public_port());
        let mut updated = sandbox("ports-a", "new-node"); updated.paused = true;
        store.put_sandbox(&updated).await.unwrap();
        let retained = allocated(store.claim_port("ports-a", 5353, "owner-a", PortProtocol::Udp, PublicPortRange::new(40000, 40001).unwrap()).await.unwrap());
        assert_eq!(first.public_port(), retained.public_port(), "record/config updates must not remap reservations");
        assert!(store.delete_port("ports-a", 5353, "owner-a").await.unwrap());
        assert!(!store.delete_port("ports-a", 5353, "owner-a").await.unwrap());
        let recycled = allocated(store.claim_port("ports-a", 5354, "owner-a", PortProtocol::Udp, range).await.unwrap());
        assert_eq!(recycled.public_port(), first.public_port());
        assert!(store.delete_sandbox("ports-a").await.unwrap());
        assert!(store.port_allocations(Some("ports-a")).await.unwrap().is_empty());
        assert_eq!(store.port_allocations(None).await.unwrap(), vec![second]);
        assert!(store.claim_port("ports-b", 0, "owner-b", PortProtocol::Udp, range).await.is_err());
        assert!(store.claim_port("ports-b", 1, "bad/owner", PortProtocol::Udp, range).await.is_err());
        store.delete_sandbox("ports-b").await.unwrap();

        store.put_sandbox(&sandbox("ports-concurrent", "node")).await.unwrap();
        let mut jobs = tokio::task::JoinSet::new();
        for _ in 0..32 {
            let store = store.clone();
            jobs.spawn(async move { allocated(store.claim_port("ports-concurrent", 9000, "owner-a", PortProtocol::Both, range).await.unwrap()) });
        }
        while let Some(result) = jobs.join_next().await { assert_eq!(result.unwrap().public_port(), range.first()); }
        assert_eq!(store.port_allocations(None).await.unwrap().len(), 1);
        store.delete_sandbox("ports-concurrent").await.unwrap();

        let wide = PublicPortRange::new(35000, 35063).unwrap();
        store.put_sandbox(&sandbox("ports-limit", "node")).await.unwrap();
        for n in 0..16u16 {
            let store = store.clone();
            jobs.spawn(async move { allocated(store.claim_port("ports-limit", 10000+n, "owner-a", PortProtocol::Tcp, wide).await.unwrap()) });
        }
        let mut ports = std::collections::HashSet::new();
        while let Some(result) = jobs.join_next().await { assert!(ports.insert(result.unwrap().public_port()), "concurrent destinations collided"); }
        assert_eq!(ports.len(), 16);
        assert_eq!(store.claim_port("ports-limit", 20000, "owner-a", PortProtocol::Udp, wide).await.unwrap(), PortClaim::LimitReached);
        let existing = store.port_allocations(Some("ports-limit")).await.unwrap()[0].clone();
        assert_eq!(allocated(store.claim_port("ports-limit", existing.machine_port(), "owner-a", PortProtocol::Both, wide).await.unwrap()).public_port(), existing.public_port());
        store.delete_sandbox("ports-limit").await.unwrap();
        assert!(store.port_allocations(None).await.unwrap().is_empty());

        store.put_sandbox(&sandbox("ports-pool", "node")).await.unwrap();
        let small = PublicPortRange::new(37000, 37001).unwrap();
        for port in [1, 2] { allocated(store.claim_port("ports-pool", port, "owner-a", PortProtocol::Udp, small).await.unwrap()); }
        assert_eq!(store.claim_port("ports-pool", 3, "owner-a", PortProtocol::Udp, small).await.unwrap(), PortClaim::PoolExhausted);
        store.delete_sandbox("ports-pool").await.unwrap();
    }

    #[tokio::test]
    async fn managed_ports_memory_contract() {
        managed_port_contract(std::sync::Arc::new(MemoryStore::new())).await;
    }

    struct OwnedPortRedis {
        child: std::process::Child,
        directory: tempfile::TempDir,
    }
    impl OwnedPortRedis {
        fn launch(directory: &std::path::Path) -> std::process::Child {
            let log = std::fs::OpenOptions::new().create(true).append(true).open(directory.join("redis.log")).unwrap();
            std::process::Command::new("redis-server")
                .args(["--port", "0", "--protected-mode", "yes", "--appendonly", "yes", "--appendfsync", "always", "--save", ""])
                .arg("--dir").arg(directory).arg("--unixsocket").arg(directory.join("redis.sock"))
                .args(["--unixsocketperm", "700"])
                .stdout(std::process::Stdio::from(log.try_clone().unwrap())).stderr(std::process::Stdio::from(log)).spawn().unwrap()
        }
        fn new() -> Self {
            let directory = tempfile::tempdir().unwrap();
            let child = Self::launch(directory.path());
            Self { child, directory }
        }
        async fn store(&mut self, namespace: &str) -> RedisStore {
            let url = format!("redis+unix://{}", self.directory.path().join("redis.sock").display());
            tokio::time::timeout(Duration::from_secs(15), async {
                loop {
                    assert!(self.child.try_wait().unwrap().is_none(), "owned Redis exited");
                    if let Ok(Ok(store)) = tokio::time::timeout(Duration::from_secs(1), RedisStore::connect(&url, namespace)).await { return store; }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }).await.expect("owned Redis readiness")
        }
        fn restart(&mut self) {
            self.child.kill().unwrap(); self.child.wait().unwrap();
            self.child = Self::launch(self.directory.path());
        }
    }
    impl Drop for OwnedPortRedis {
        fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
    }

    async fn owned_port_contract(store: std::sync::Arc<dyn ClusterStore>) {
        let range=PublicPortRange::new(48000,48031).unwrap();
        let mut record=sandbox("owner-port-vm","node");
        store.put_sandbox(&record).await.unwrap();
        assert_eq!(store.claim_owned_port(&record.sandbox_id,18082,"principal-a",PortProtocol::Both,range).await.unwrap(),PortClaim::OwnerConflict);
        assert_eq!(store.owned_ports(&record.sandbox_id,"principal-a").await.unwrap(),OwnedPortAccess::OwnerConflict);
        assert_eq!(store.delete_owned_port(&record.sandbox_id,18082,"principal-a").await.unwrap(),OwnedPortAccess::OwnerConflict);
        record.owner_id=Some(crate::ownership::OwnerId::parse("principal-a").unwrap());
        store.put_sandbox(&record).await.unwrap();
        assert_eq!(store.claim_owned_port(&record.sandbox_id,18082,"principal-b",PortProtocol::Both,range).await.unwrap(),PortClaim::OwnerConflict);
        assert_eq!(store.owned_ports(&record.sandbox_id,"principal-b").await.unwrap(),OwnedPortAccess::OwnerConflict);
        let mut jobs=tokio::task::JoinSet::new();
        for _ in 0..32 {
            let store=store.clone();
            jobs.spawn(async move { allocated(store.claim_owned_port("owner-port-vm",18082,"principal-a",PortProtocol::Both,range).await.unwrap()) });
        }
        let mut first=None;
        while let Some(result)=jobs.join_next().await {
            let row=result.unwrap();
            if let Some(first)=&first { assert_eq!(first,&row); } else { first=Some(row); }
        }
        let first=first.unwrap();
        let updated=allocated(store.claim_owned_port(&record.sandbox_id,18082,"principal-a",PortProtocol::Udp,range).await.unwrap());
        assert_eq!(updated.public_port(),first.public_port());
        for port in 1..16 { allocated(store.claim_owned_port(&record.sandbox_id,port,"principal-a",PortProtocol::Tcp,range).await.unwrap()); }
        assert_eq!(store.claim_owned_port(&record.sandbox_id,18083,"principal-a",PortProtocol::Both,range).await.unwrap(),PortClaim::LimitReached);
        let OwnedPortAccess::Granted(rows)=store.owned_ports(&record.sandbox_id,"principal-a").await.unwrap() else { panic!("owner listing refused") };
        assert_eq!(rows.len(),16);
        assert_eq!(store.delete_owned_port(&record.sandbox_id,18082,"principal-b").await.unwrap(),OwnedPortAccess::OwnerConflict);
        assert_eq!(store.delete_owned_port(&record.sandbox_id,18082,"principal-a").await.unwrap(),OwnedPortAccess::Granted(true));
        assert_eq!(store.delete_owned_port(&record.sandbox_id,18082,"principal-a").await.unwrap(),OwnedPortAccess::Granted(false));
        // A trusted record change must be seen by each operation, not cached by the caller.
        record.owner_id=Some(crate::ownership::OwnerId::parse("principal-b").unwrap());
        store.put_sandbox(&record).await.unwrap();
        assert_eq!(store.claim_owned_port(&record.sandbox_id,18083,"principal-a",PortProtocol::Both,range).await.unwrap(),PortClaim::OwnerConflict);
        assert_eq!(store.delete_owned_port(&record.sandbox_id,1,"principal-a").await.unwrap(),OwnedPortAccess::OwnerConflict);
        assert_eq!(store.owned_ports(&record.sandbox_id,"principal-a").await.unwrap(),OwnedPortAccess::OwnerConflict);
        assert!(store.owned_ports(&record.sandbox_id,"principal-b").await.is_err());
        store.delete_sandbox(&record.sandbox_id).await.unwrap();
        assert_eq!(store.claim_owned_port(&record.sandbox_id,1,"principal-b",PortProtocol::Tcp,range).await.unwrap(),PortClaim::SandboxMissing);
        assert_eq!(store.owned_ports(&record.sandbox_id,"principal-b").await.unwrap(),OwnedPortAccess::SandboxMissing);
        assert_eq!(store.delete_owned_port(&record.sandbox_id,1,"principal-b").await.unwrap(),OwnedPortAccess::SandboxMissing);
    }

    async fn owner_adoption_contract(store: std::sync::Arc<dyn ClusterStore>) {
        use crate::ownership::{OwnerAdoption, OwnerId};
        let first = OwnerId::parse("adopter-a").unwrap();
        assert_eq!(store.adopt_sandbox_owner("absent-adoption", &first).await.unwrap(), OwnerAdoption::SandboxMissing);
        let record = sandbox("legacy-adoption", "node");
        store.put_sandbox(&record).await.unwrap();
        let mut jobs = tokio::task::JoinSet::new();
        for index in 0..32 {
            let store = store.clone();
            jobs.spawn(async move {
                let owner = OwnerId::parse(&format!("adopter-{index}")).unwrap();
                let result = store.adopt_sandbox_owner("legacy-adoption", &owner).await.unwrap();
                (owner, result)
            });
        }
        let mut winner = None;
        while let Some(result) = jobs.join_next().await {
            let (owner, outcome) = result.unwrap();
            if outcome == OwnerAdoption::Adopted { assert!(winner.replace(owner).is_none()); }
            else { assert_eq!(outcome, OwnerAdoption::OwnerConflict); }
        }
        let winner = winner.unwrap();
        let mut expected = record;
        expected.owner_id = Some(winner.clone());
        assert_eq!(store.sandbox("legacy-adoption").await.unwrap(), Some(expected));
        assert_eq!(store.adopt_sandbox_owner("legacy-adoption", &winner).await.unwrap(), OwnerAdoption::AlreadyOwned);
        let mut stale = store.sandbox("legacy-adoption").await.unwrap().unwrap();
        stale.owner_id = None;
        stale.metadata.insert("lifecycle-update".into(),"kept".into());
        store.put_sandbox(&stale).await.unwrap();
        let refreshed = store.sandbox("legacy-adoption").await.unwrap().unwrap();
        assert_eq!(refreshed.owner_id,Some(winner.clone()));
        assert_eq!(refreshed.metadata.get("lifecycle-update").map(String::as_str),Some("kept"));

        assert_eq!(store.adopt_sandbox_owner("legacy-adoption", &first).await.unwrap(), OwnerAdoption::OwnerConflict);
        store.put_sandbox(&sandbox("legacy-adoption-ports", "node")).await.unwrap();
        let range = PublicPortRange::new(46000, 46000).unwrap();
        store.claim_port("legacy-adoption-ports", 8080, "legacy-owner", PortProtocol::Tcp, range).await.unwrap();
        assert_eq!(store.adopt_sandbox_owner("legacy-adoption-ports", &first).await.unwrap(), OwnerAdoption::PortsPresent);
        assert!(store.sandbox("legacy-adoption-ports").await.unwrap().unwrap().owner_id.is_none());
        store.delete_sandbox("legacy-adoption-ports").await.unwrap();
        store.delete_sandbox("legacy-adoption").await.unwrap();
    }

    #[tokio::test]
    async fn memory_legacy_owner_adoption_is_atomic() {
        owner_adoption_contract(std::sync::Arc::new(MemoryStore::new())).await;
    }

    #[tokio::test]
    async fn memory_owned_port_operations_authorize_atomically() {
        owned_port_contract(std::sync::Arc::new(MemoryStore::new())).await;
    }

    #[tokio::test]
    #[ignore = "launches an owned Redis server; invoke explicitly on Linux"]
    async fn managed_ports_redis_owned_restart() {
        let mut server = OwnedPortRedis::new();
        let namespace = format!("port-test-{}", uuid::Uuid::new_v4());
        let store = std::sync::Arc::new(server.store(&namespace).await);
        managed_port_contract(store.clone()).await;
        owned_port_contract(store.clone()).await;
        owner_adoption_contract(store.clone()).await;
        let mut persistent=sandbox("ports-persistent","node");
        persistent.owner_id=Some(crate::ownership::OwnerId::parse("owner-a").unwrap());
        store.put_sandbox(&persistent).await.unwrap();
        let range = PublicPortRange::new(45000, 45002).unwrap();
        let before = allocated(store.claim_port("ports-persistent", 5353, "owner-a", PortProtocol::Both, range).await.unwrap());
        // A bad second index must be detected before any global reservation is written.
        store.put_sandbox(&sandbox("ports-type-fault", "node")).await.unwrap();
        let mut admin = store.connection.clone();
        // Adoption preserves the complete raw record and survives owned AOF restart.
        store.put_sandbox(&sandbox("adoption-persistent", "node")).await.unwrap();
        let adoption_key = store.key("sandbox:adoption-persistent");
        let original: String = redis::cmd("GET").arg(&adoption_key).query_async(&mut admin).await.unwrap();
        let mut adoption_record: serde_json::Value = serde_json::from_str(&original).unwrap();
        adoption_record["future_field"] = serde_json::json!({"keep": [1, "value", null]});
        let encoded = serde_json::to_string(&adoption_record).unwrap();
        redis::cmd("SET").arg(&adoption_key).arg(&encoded).query_async::<()>(&mut admin).await.unwrap();
        // A malformed inventory index must not commit the first SET before SADD fails.
        let inventory_key=store.key("sandboxes");
        let inventory: Vec<String>=redis::cmd("SMEMBERS").arg(&inventory_key).query_async(&mut admin).await.unwrap();
        redis::cmd("DEL").arg(&inventory_key).query_async::<u64>(&mut admin).await.unwrap();
        redis::cmd("SET").arg(&inventory_key).arg("wrong-type").query_async::<()>(&mut admin).await.unwrap();
        let mut stale_record=store.sandbox("adoption-persistent").await.unwrap().unwrap();
        stale_record.metadata.insert("must-not-commit".into(),"value".into());
        assert!(store.put_sandbox(&stale_record).await.is_err());
        let unchanged: String=redis::cmd("GET").arg(&adoption_key).query_async(&mut admin).await.unwrap();
        assert_eq!(unchanged,encoded);
        redis::cmd("DEL").arg(&inventory_key).query_async::<u64>(&mut admin).await.unwrap();
        redis::cmd("SADD").arg(&inventory_key).arg(&inventory).query_async::<u64>(&mut admin).await.unwrap();
        let bad_adoption_index = store.key("ports:adoption-persistent");
        redis::cmd("SET").arg(&bad_adoption_index).arg("wrong-type").query_async::<()>(&mut admin).await.unwrap();
        let adopter = crate::ownership::OwnerId::parse("adopter-persistent").unwrap();
        assert!(store.adopt_sandbox_owner("adoption-persistent", &adopter).await.is_err());
        let after: String = redis::cmd("GET").arg(&adoption_key).query_async(&mut admin).await.unwrap();
        assert_eq!(after, encoded);
        redis::cmd("DEL").arg(&bad_adoption_index).query_async::<u64>(&mut admin).await.unwrap();
        assert_eq!(store.adopt_sandbox_owner("adoption-persistent", &adopter).await.unwrap(), crate::ownership::OwnerAdoption::Adopted);
        adoption_record["owner_id"] = serde_json::json!(adopter);
        let after: String = redis::cmd("GET").arg(&adoption_key).query_async(&mut admin).await.unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&after).unwrap(), adoption_record);
        let bad_index = store.key("ports:ports-type-fault");
        redis::cmd("SET").arg(&bad_index).arg("wrong-type").query_async::<()>(&mut admin).await.unwrap();
        let baseline = store.port_allocations(None).await.unwrap();
        assert!(store.claim_port("ports-type-fault", 1234, "owner-a", PortProtocol::Udp, range).await.is_err());
        assert!(store.delete_sandbox("ports-type-fault").await.is_err());
        assert!(store.sandbox("ports-type-fault").await.unwrap().is_some());
        assert_eq!(store.port_allocations(None).await.unwrap(), baseline);
        redis::cmd("DEL").arg(&bad_index).query_async::<u64>(&mut admin).await.unwrap();
        store.delete_sandbox("ports-type-fault").await.unwrap();

        // Mutually agreeing malformed records must not be updated or removed.
        let local_key = store.key("ports:ports-persistent");
        let global_key = store.key("public-ports");
        let original: String = redis::cmd("HGET").arg(&local_key).arg(5353).query_async(&mut admin).await.unwrap();
        for fault in ["extra", "protocol", "owner", "array"] {
            let mut row: serde_json::Value = serde_json::from_str(&original).unwrap();
            match fault {
                "extra" => row["unexpected"] = serde_json::json!(true),
                "protocol" => row["protocol"] = serde_json::json!("invalid"),
                "owner" => row["owner_id"] = serde_json::json!("invalid owner"),
                "array" => row = serde_json::json!([]),
                _ => unreachable!(),
            }
            let corrupted = serde_json::to_string(&row).unwrap();
            for (key, field) in [(&local_key, 5353), (&global_key, before.public_port())] {
                redis::cmd("HSET").arg(key).arg(field).arg(&corrupted).query_async::<u64>(&mut admin).await.unwrap();
            }
            assert!(store.claim_owned_port("ports-persistent", 5353, "owner-a", PortProtocol::Udp, range).await.is_err(), "{fault}");
            assert!(store.claim_port("ports-persistent", 5353, "owner-a", PortProtocol::Udp, range).await.is_err(), "{fault}");
            assert!(store.delete_owned_port("ports-persistent", 5353, "owner-a").await.is_err(), "{fault}");
            assert!(store.delete_port("ports-persistent", 5353, "owner-a").await.is_err(), "{fault}");
            for (key, field) in [(&local_key, 5353), (&global_key, before.public_port())] {
                let after: String = redis::cmd("HGET").arg(key).arg(field).query_async(&mut admin).await.unwrap();
                assert_eq!(after, corrupted, "{fault}");
            }
        }
        for (key, field) in [(&local_key, 5353), (&global_key, before.public_port())] {
            redis::cmd("HSET").arg(key).arg(field).arg(&original).query_async::<u64>(&mut admin).await.unwrap();
        }

        // A dedicated authenticated client checks denied writes before mutating either index.
        let username = format!("port-writer-{}", uuid::Uuid::new_v4().simple());
        let password = uuid::Uuid::new_v4().simple().to_string();
        redis::cmd("ACL").arg("SETUSER").arg(&username).arg("on").arg(format!(">{password}"))
            .arg(format!("~{}*", store.prefix)).arg("+@all").arg("-hset")
            .query_async::<()>(&mut admin).await.unwrap();
        let url = format!("redis+unix://{}", server.directory.path().join("redis.sock").display());
        let info = redis::Client::open(url).unwrap().get_connection_info().clone();
        let settings = info.redis_settings().clone().set_username(&username).set_password(password);
        let info = info.set_redis_settings(settings);
        let client = redis::Client::open(info).unwrap();
        let restricted = RedisStore { connection: redis::aio::ConnectionManager::new(client).await.unwrap(), prefix: store.prefix.clone() };
        let error = restricted.claim_port("ports-persistent", 7777, "owner-a", PortProtocol::Udp, range).await.unwrap_err();
        assert!(error.to_string().contains("write preflight denied"));
        assert_eq!(store.port_allocations(None).await.unwrap(), baseline);
        redis::cmd("ACL").arg("SETUSER").arg(&username).arg("+hset").arg("-hdel")
            .query_async::<()>(&mut admin).await.unwrap();
        let error = restricted.delete_port("ports-persistent", 5353, "owner-a").await.unwrap_err();
        assert!(error.to_string().contains("delete preflight denied"));
        let error = restricted.delete_sandbox("ports-persistent").await.unwrap_err();
        assert!(error.to_string().contains("cleanup preflight denied"));
        assert!(store.sandbox("ports-persistent").await.unwrap().is_some());
        assert_eq!(store.port_allocations(None).await.unwrap(), baseline);
        redis::cmd("ACL").arg("DELUSER").arg(&username).query_async::<u64>(&mut admin).await.unwrap();
        drop(restricted);
        drop(store);
        server.restart();
        let recovered = server.store(&namespace).await;
        assert_eq!(recovered.sandbox("adoption-persistent").await.unwrap().unwrap().owner_id, Some(adopter.clone()));
        assert_eq!(recovered.adopt_sandbox_owner("adoption-persistent", &adopter).await.unwrap(), crate::ownership::OwnerAdoption::AlreadyOwned);
        let mut admin = recovered.connection.clone();
        let recovered_raw: String = redis::cmd("GET").arg(&adoption_key).query_async(&mut admin).await.unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&recovered_raw).unwrap(), adoption_record);
        recovered.delete_sandbox("adoption-persistent").await.unwrap();
        assert_eq!(recovered.sandbox("ports-persistent").await.unwrap().unwrap().owner_id,persistent.owner_id);
        assert!(matches!(recovered.owned_ports("ports-persistent","owner-a").await.unwrap(),OwnedPortAccess::Granted(_)));
        assert_eq!(recovered.owned_ports("ports-persistent","owner-b").await.unwrap(),OwnedPortAccess::OwnerConflict);
        assert_eq!(recovered.port_allocations(Some("ports-persistent")).await.unwrap(), vec![before.clone()]);
        let after = allocated(recovered.claim_port("ports-persistent", 5353, "owner-a", PortProtocol::Udp, range).await.unwrap());
        assert_eq!(after.public_port(), before.public_port());
        assert_eq!(after.protocol(), PortProtocol::Udp);
        assert_eq!(recovered.claim_port("ports-persistent", 5353, "owner-b", PortProtocol::Udp, range).await.unwrap(), PortClaim::OwnerConflict);
        assert!(recovered.delete_sandbox("ports-persistent").await.unwrap());
        assert!(recovered.port_allocations(None).await.unwrap().is_empty());

        // Recover actual native listeners from this owned durable store, then
        // close them on corrupt snapshots and on reservation deletion.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = probe.local_addr().unwrap(); drop(probe);
        recovered.put_sandbox(&sandbox("gateway-recovery", "node")).await.unwrap();
        let gateway_range = PublicPortRange::new(address.port(), address.port()).unwrap();
        let gateway_row = allocated(recovered.claim_port("gateway-recovery", 8080, "owner-a",
            PortProtocol::Both, gateway_range).await.unwrap());
        let mut gateway = crate::native_ports::NativePortRegistry::new(address.ip(), 1).unwrap();
        assert!(gateway.refresh(&recovered).await.unwrap().is_empty());
        assert_eq!(gateway.binding(address.port()).unwrap().allocation(), &gateway_row);
        drop(gateway); drop(recovered);
        server.restart();
        let recovered = server.store(&namespace).await;
        let mut gateway = crate::native_ports::NativePortRegistry::new(address.ip(), 1).unwrap();
        assert!(gateway.refresh(&recovered).await.unwrap().is_empty());
        assert_eq!(gateway.binding(address.port()).unwrap().allocation(), &gateway_row);
        assert!(tokio::net::TcpListener::bind(address).await.is_err());
        assert!(tokio::net::UdpSocket::bind(address).await.is_err());
        let mut admin = recovered.connection.clone();
        let global = recovered.key("public-ports");
        let encoded: String = redis::cmd("HGET").arg(&global).arg(address.port()).query_async(&mut admin).await.unwrap();
        let invalid_field = "0";
        redis::cmd("HSET").arg(&global).arg(invalid_field).arg(&encoded).query_async::<u64>(&mut admin).await.unwrap();
        assert!(gateway.refresh(&recovered).await.is_err());
        assert!(gateway.is_empty());
        redis::cmd("HDEL").arg(&global).arg(invalid_field).query_async::<u64>(&mut admin).await.unwrap();
        assert!(gateway.refresh(&recovered).await.unwrap().is_empty());
        let per_vm = recovered.key("ports:gateway-recovery");
        redis::cmd("HSET").arg(&per_vm).arg(invalid_field).arg(&encoded).query_async::<u64>(&mut admin).await.unwrap();
        assert!(recovered.port_allocations(Some("gateway-recovery")).await.is_err());
        redis::cmd("HDEL").arg(&per_vm).arg(invalid_field).query_async::<u64>(&mut admin).await.unwrap();
        redis::cmd("DEL").arg(&global).query_async::<u64>(&mut admin).await.unwrap();
        redis::cmd("SET").arg(&global).arg("owned-type-fault").query_async::<()>(&mut admin).await.unwrap();
        assert!(gateway.refresh(&recovered).await.is_err());
        assert!(gateway.is_empty());
        let tcp = tokio::net::TcpListener::bind(address).await.unwrap();
        let udp = tokio::net::UdpSocket::bind(address).await.unwrap();
        drop(tcp); drop(udp);
        redis::cmd("DEL").arg(&global).query_async::<u64>(&mut admin).await.unwrap();
        redis::cmd("HSET").arg(&global).arg(address.port()).arg(encoded).query_async::<u64>(&mut admin).await.unwrap();
        assert!(gateway.refresh(&recovered).await.unwrap().is_empty());
        assert!(recovered.delete_sandbox("gateway-recovery").await.unwrap());
        assert!(gateway.refresh(&recovered).await.unwrap().is_empty());
        assert!(gateway.is_empty());
        let _tcp = tokio::net::TcpListener::bind(address).await.unwrap();
        let _udp = tokio::net::UdpSocket::bind(address).await.unwrap();
    }

}
