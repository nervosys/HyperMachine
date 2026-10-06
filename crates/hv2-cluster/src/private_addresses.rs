//! Bounded, source-incarnation-bound synthetic private address ledger.
//! Persist the complete append-only snapshot atomically with source lifecycle
//! state. Restoring an older ledger is forbidden; this module cannot enforce
//! crash durability or prevent an external store from rolling state back.
use crate::{
    model::SandboxRecord,
    ownership::OwnerId,
    private_networks::{
        NetworkMembershipState, NetworkTag, PrivateRouteClaim, PrivateRouteSnapshot,
    },
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

pub const MAX_PRIVATE_ADDRESSES: usize = 4096;
const MAX_SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: String,
    owner: OwnerId,
    started_at_ms: u64,
}
impl Source {
    fn trusted(record: &SandboxRecord) -> Result<Self, &'static str> {
        NetworkMembershipState::new(record, None)?;
        Ok(Self {
            id: record.sandbox_id.clone(),
            owner: record.owner_id.clone().ok_or("owner required")?,
            started_at_ms: record.started_at_ms,
        })
    }
    fn matches(&self, record: &SandboxRecord) -> bool {
        self.id == record.sandbox_id
            && Some(&self.owner) == record.owner_id.as_ref()
            && self.started_at_ms == record.started_at_ms
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ledger {
    source: Source,
    // Index is the address: entries are retained and never reassigned.
    // Port 1 is a canonical sentinel; actual TCP ports are checked on route.
    entries: Vec<PrivateRouteClaim>,
}
pub struct PrivateAddressBook(Mutex<Ledger>);
fn address(index: usize) -> Ipv4Addr {
    Ipv4Addr::from(u32::from(Ipv4Addr::new(198, 18, 0, 0)) + index as u32 + 1)
}
impl PrivateAddressBook {
    pub fn new(source: &SandboxRecord) -> Result<Self, &'static str> {
        Ok(Self(Mutex::new(Ledger {
            source: Source::trusted(source)?,
            entries: vec![],
        })))
    }
    /// Reserve the whole synthetic pool, even for unknown/unallocated IPs.
    /// Those destinations must never fall through to host or Internet routes.
    pub fn owns_address(ip: Ipv4Addr) -> bool {
        let b = ip.octets();
        b[0] == 198 && (b[1] == 18 || b[1] == 19)
    }
    /// Allocate atomically for an authorized paired snapshot. Local pending
    /// flags must come from trusted node state, never from guest input.
    pub fn allocate(
        &self,
        snapshot: &PrivateRouteSnapshot,
        network: NetworkTag,
        now: u64,
        source_pending: bool,
        destination_pending: bool,
    ) -> Result<Ipv4Addr, &'static str> {
        let claim = snapshot.claim(network, 1, now, source_pending, destination_pending)?;
        let mut ledger = self.0.lock();
        if !ledger.source.matches(&snapshot.source_record) {
            return Err("source incarnation changed");
        }
        if let Some(index) = ledger.entries.iter().position(|entry| entry == &claim) {
            return Ok(address(index));
        }
        if ledger.entries.len() >= MAX_PRIVATE_ADDRESSES {
            return Err("private address capacity exhausted");
        }
        let ip = address(ledger.entries.len());
        ledger.entries.push(claim);
        Ok(ip)
    }
    /// Address lookup is only route context. Every dial must obtain a fresh
    /// paired snapshot and independently authenticate both source and node.
    pub fn binding(&self, ip: Ipv4Addr, port: u16) -> Result<PrivateRouteClaim, &'static str> {
        if !Self::owns_address(ip) || port == 0 {
            return Err("private address refused");
        }
        let offset = u32::from(ip)
            .checked_sub(u32::from(address(0)))
            .ok_or("private address refused")? as usize;
        let mut claim = self
            .0
            .lock()
            .entries
            .get(offset)
            .cloned()
            .ok_or("private address unallocated")?;
        claim.port = port;
        Ok(claim)
    }
    pub fn authorize(
        &self,
        ip: Ipv4Addr,
        port: u16,
        snapshot: &PrivateRouteSnapshot,
        now: u64,
        source_pending: bool,
        destination_pending: bool,
    ) -> Result<PrivateRouteClaim, &'static str> {
        let claim = self.binding(ip, port)?;
        if !self.0.lock().source.matches(&snapshot.source_record) {
            return Err("source incarnation changed");
        }
        snapshot.authorize(
            &claim.source_id,
            &claim,
            now,
            source_pending,
            destination_pending,
        )?;
        Ok(claim)
    }
    /// Snapshot under the same lock as allocation. The caller must commit it
    /// durably before exposing a newly allocated address to a guest.
    pub fn snapshot(&self) -> Result<Vec<u8>, &'static str> {
        serde_json::to_vec(&*self.0.lock()).map_err(|_| "address snapshot failed")
    }
    /// Store-side validation: existing indexes can never change or disappear.
    pub(crate) fn canonical_append(
        current: Option<&[u8]>,
        next: &[u8],
        source: &SandboxRecord,
    ) -> Result<Vec<u8>, &'static str> {
        let proposed = Self::restore(next, source)?;
        if let Some(bytes) = current {
            let previous = Self::restore(bytes, source)?;
            if !proposed
                .0
                .lock()
                .entries
                .starts_with(&previous.0.lock().entries)
            {
                return Err("private address ledger cannot replace or remove bindings");
            }
        }
        proposed.snapshot()
    }
    pub fn restore(bytes: &[u8], source: &SandboxRecord) -> Result<Self, &'static str> {
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err("address snapshot too large");
        }
        let ledger: Ledger =
            serde_json::from_slice(bytes).map_err(|_| "invalid address snapshot")?;
        if ledger.source != Source::trusted(source)? || ledger.entries.len() > MAX_PRIVATE_ADDRESSES
        {
            return Err("address snapshot identity or capacity refused");
        }
        for (index, claim) in ledger.entries.iter().enumerate() {
            claim.validate()?;
            if claim.source_id != ledger.source.id
                || claim.port != 1
                || ledger.entries[..index].contains(claim)
            {
                return Err("invalid address binding");
            }
        }
        Ok(Self(Mutex::new(ledger)))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    fn record(id: &str) -> SandboxRecord {
        SandboxRecord {
            sandbox_id: id.into(),
            owner_id: Some(OwnerId::parse("owner").unwrap()),
            node_id: "node".into(),
            template_id: "base".into(),
            started_at_ms: 1,
            end_at_ms: 100,
            cpu_count: 1,
            memory_mb: 1,
            metadata: Default::default(),
            envd_version: "test".into(),
            descriptor: serde_json::Value::Null,
            paused: false,
            portable: false,
            volume_mounts: vec![],
        }
    }
    fn snapshot() -> PrivateRouteSnapshot {
        let s = record("source");
        let d = record("destination");
        let tag = NetworkTag::parse("team").unwrap();
        PrivateRouteSnapshot::checked(
            "source",
            "destination",
            s.clone(),
            d.clone(),
            NetworkMembershipState::new(&s, Some(vec![tag.clone()])).unwrap(),
            NetworkMembershipState::new(&d, Some(vec![tag])).unwrap(),
        )
        .unwrap()
    }
    fn tag() -> NetworkTag {
        NetworkTag::parse("team").unwrap()
    }
    #[test]
    fn concurrent_allocations_are_idempotent_and_restore_preserves_bindings() {
        let view = Arc::new(snapshot());
        let book = Arc::new(PrivateAddressBook::new(&view.source_record).unwrap());
        let tasks: Vec<_> = (0..32)
            .map(|_| {
                let view = view.clone();
                let book = book.clone();
                std::thread::spawn(move || book.allocate(&view, tag(), 50, false, false).unwrap())
            })
            .collect();
        let addresses: Vec<_> = tasks.into_iter().map(|t| t.join().unwrap()).collect();
        assert!(addresses.iter().all(|ip| *ip == addresses[0]));
        let restored =
            PrivateAddressBook::restore(&book.snapshot().unwrap(), &view.source_record).unwrap();
        assert_eq!(
            restored.allocate(&view, tag(), 50, false, false).unwrap(),
            addresses[0]
        );
        assert_eq!(
            restored
                .authorize(addresses[0], 8080, &view, 50, false, false)
                .unwrap()
                .port,
            8080
        );
        assert!(restored
            .authorize(addresses[0], 0, &view, 50, false, false)
            .is_err());
    }
    #[test]
    fn rejoin_and_source_replacement_never_retarget_old_addresses() {
        let old = snapshot();
        let fresh = snapshot();
        let book = PrivateAddressBook::new(&old.source_record).unwrap();
        let first = book.allocate(&old, tag(), 50, false, false).unwrap();
        let second = book.allocate(&fresh, tag(), 50, false, false).unwrap();
        assert_ne!(first, second);
        assert!(book
            .authorize(first, 8080, &fresh, 50, false, false)
            .is_err());
        assert!(book
            .authorize(second, 8080, &fresh, 50, false, false)
            .is_ok());
        assert!(book.allocate(&fresh, tag(), 50, true, false).is_err());
        assert!(book
            .authorize(second, 8080, &fresh, 100, false, false)
            .is_err());
        let mut other = old.source_record.clone();
        other.started_at_ms += 1;
        assert!(PrivateAddressBook::restore(&book.snapshot().unwrap(), &other).is_err());
        other = old.source_record.clone();
        other.owner_id = Some(OwnerId::parse("other").unwrap());
        assert!(PrivateAddressBook::restore(&book.snapshot().unwrap(), &other).is_err());
        assert!(PrivateAddressBook::owns_address(Ipv4Addr::new(
            198, 19, 255, 255
        )));
        assert!(book.binding(Ipv4Addr::new(198, 19, 255, 255), 80).is_err());
    }
    #[test]
    fn snapshot_validation_and_capacity_fail_closed() {
        let view = snapshot();
        let book = PrivateAddressBook::new(&view.source_record).unwrap();
        book.allocate(&view, tag(), 50, false, false).unwrap();
        let bytes = book.snapshot().unwrap();
        let mut raw: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let original = raw.clone();
        raw["entries"][0]["port"] = 8080.into();
        assert!(PrivateAddressBook::restore(
            &serde_json::to_vec(&raw).unwrap(),
            &view.source_record
        )
        .is_err());
        raw = original.clone();
        raw["entries"]
            .as_array_mut()
            .unwrap()
            .push(original["entries"][0].clone());
        assert!(PrivateAddressBook::restore(
            &serde_json::to_vec(&raw).unwrap(),
            &view.source_record
        )
        .is_err());
        assert!(PrivateAddressBook::restore(
            &vec![b' '; MAX_SNAPSHOT_BYTES + 1],
            &view.source_record
        )
        .is_err());
        for _ in 1..MAX_PRIVATE_ADDRESSES {
            let mut claim = book.binding(address(0), 1).unwrap();
            claim.destination_generation = uuid::Uuid::new_v4().to_string();
            book.0.lock().entries.push(claim);
        }
        assert!(book.allocate(&snapshot(), tag(), 50, false, false).is_err());
        // Exhaustion preserves existing mappings, including after restore.
        assert!(
            PrivateAddressBook::restore(&book.snapshot().unwrap(), &view.source_record)
                .unwrap()
                .authorize(address(0), 80, &view, 50, false, false)
                .is_ok()
        );
    }
}
