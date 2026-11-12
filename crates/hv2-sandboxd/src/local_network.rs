//! Named networks joining sandboxes *on this one node* to each other, by a
//! DNS alias each member picks -- `docker network create/connect/disconnect`
//! for sandbox VMs, with no cluster, no mTLS and no control plane. That is
//! the gap `hv2-cluster`'s private networking leaves open: its `PrivateNetwork`
//! (`private_source::for_gateway`) requires `--cluster-token` and mTLS, which
//! a standalone node has neither of, and which two sandboxes on the one node
//! should not need just to reach each other.
//!
//! # Design
//!
//! [`Registry`] is one process-wide table of networks, each a name to
//! `(alias -> (sandbox_id, address))` map. [`LocalNetwork`] implements
//! [`hv2_net::gateway::PrivateNetwork`] for one sandbox (the gateway's own
//! guest, "the source"): it answers DNS and routes connections only within
//! networks the source itself currently belongs to, checked fresh on every
//! call. Membership is not snapshotted into the gateway at creation time --
//! the gateway holds only a `Registry` handle and the source's ID -- so a
//! `connect` after boot takes effect on the guest's very next lookup, and a
//! `disconnect` refuses the next one, with nothing to rebuild or restart.
//! That is what lets a test disconnect and reconnect a VM mid-run.
//!
//! Dialing a peer never touches its virtio-net device or IP stack at all:
//! [`hv2_agent::AgentVM::forward_port`] already carries a connection to a
//! guest's listening port over vsock, with no network device needed on
//! either end -- the same primitive a published machine port uses
//! (`machines::ports::carry`). `LocalNetwork::dial` calls it directly,
//! wrapping the vsock stream in a loopback TCP pair with
//! [`crate::forwards::splice`] (also the published-port path's own code) so
//! it satisfies [`hv2_net::gateway::Upstream`] (an async byte stream), rather
//! than inventing new network-stack code or a new async/blocking bridge.
//!
//! # Not verified
//!
//! Compiles and is unit-tested (the registry's own bookkeeping) on this
//! Windows host; nothing here has been run against a live guest. The one
//! thing not exercised even on KVM elsewhere in this repo is a guest's TCP
//! stack actually reaching another guest's alias end to end -- that needs a
//! real two-VM run, which belongs in `docs/BOAT_PARITY.md` once done.

use crate::AppState;
use hv2_net::gateway::{PrivateNetwork, Upstream};
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::{Arc, Weak},
    time::Duration,
};

/// How long a dial waits for the target guest to accept. Generous: this is
/// local traffic, so the usual cost is the target's own listener coming up,
/// not anything in between.
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);

/// The reserved `/16` local networks are allocated from, each network
/// getting one `/24` inside it in creation order
/// (`LOCAL_NETWORK_BASE.{index}.0/24`). Chosen clear of the gateway's own
/// `10.0.2.0/24` (`hv2_net::nat::NatConfig::default`'s external address) and
/// of the common `172.17.0.0/16`/`192.168.0.0/16` a guest's own tooling
/// might assume, so a guest that also runs Docker internally does not see
/// its ranges collide with this one.
const LOCAL_NETWORK_BASE: [u8; 2] = [10, 200];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NotFound,
    AlreadyExists,
    AliasTaken,
    AlreadyMember,
    NotAMember,
    /// `remove` on a network that still has a member.
    NotEmpty,
    /// More than 253 networks, or more than 253 members of one network.
    Exhausted,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "no such network",
            Self::AlreadyExists => "a network of that name already exists",
            Self::AliasTaken => "that alias is already used on this network",
            Self::AlreadyMember => "that sandbox is already connected to this network",
            Self::NotAMember => "that sandbox is not connected to this network",
            Self::NotEmpty => "the network still has a member; disconnect it first",
            Self::Exhausted => "no addresses or network slots are left",
        })
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub sandbox_id: String,
    pub alias: String,
    pub address: Ipv4Addr,
}

#[derive(Debug, Clone)]
pub struct NetworkInfo {
    pub name: String,
    pub members: Vec<Member>,
}

#[derive(Default)]
struct Network {
    /// This network's `/24` inside [`LOCAL_NETWORK_BASE`]; fixed at
    /// creation, from the registry's own counter, never reused within the
    /// registry's lifetime even after this network is removed, so an old
    /// member's address never silently becomes valid again on a new
    /// network of the same index.
    index: u8,
    by_alias: HashMap<String, Member>,
    by_sandbox: HashMap<String, String>,
}

impl Network {
    /// `.0` is the network address and `.1` the (unused) reserved gateway
    /// address; members start at `.2`. The lowest address no current member
    /// holds: counting members instead hands out one that is still in use
    /// once any member but the newest has left.
    fn next_address(&self) -> Option<Ipv4Addr> {
        (2..=254u8)
            .map(|host| {
                Ipv4Addr::new(
                    LOCAL_NETWORK_BASE[0],
                    LOCAL_NETWORK_BASE[1],
                    self.index,
                    host,
                )
            })
            .find(|address| self.by_alias.values().all(|m| m.address != *address))
    }
}

#[derive(Default)]
struct Inner {
    networks: HashMap<String, Network>,
    next_index: u16,
}

/// The node-wide table every sandbox's [`LocalNetwork`] reads and writes
/// through. One of these per `hv2-sandboxd` process, held by [`AppState`].
#[derive(Default)]
pub struct Registry {
    inner: Mutex<Inner>,
}

impl Registry {
    pub fn create(&self, name: &str) -> Result<(), Error> {
        let mut inner = self.inner.lock();
        if inner.networks.contains_key(name) {
            return Err(Error::AlreadyExists);
        }
        let index = u8::try_from(inner.next_index).map_err(|_| Error::Exhausted)?;
        inner.next_index = inner.next_index.checked_add(1).ok_or(Error::Exhausted)?;
        inner.networks.insert(
            name.to_string(),
            Network {
                index,
                ..Default::default()
            },
        );
        Ok(())
    }

    /// Refuses while any sandbox is still connected -- `docker network rm`'s
    /// own rule -- so a network is never torn out from under a gateway
    /// still holding a live `LocalNetwork` for it.
    pub fn remove(&self, name: &str) -> Result<(), Error> {
        let mut inner = self.inner.lock();
        match inner.networks.get(name) {
            None => Err(Error::NotFound),
            Some(net) if !net.by_alias.is_empty() => Err(Error::NotEmpty),
            Some(_) => {
                inner.networks.remove(name);
                Ok(())
            }
        }
    }

    pub fn list(&self) -> Vec<NetworkInfo> {
        let inner = self.inner.lock();
        let mut names: Vec<&String> = inner.networks.keys().collect();
        names.sort();
        names
            .into_iter()
            .map(|name| describe(name, &inner.networks[name]))
            .collect()
    }

    pub fn inspect(&self, name: &str) -> Option<NetworkInfo> {
        let inner = self.inner.lock();
        inner.networks.get(name).map(|net| describe(name, net))
    }

    /// Join `sandbox_id` to `name` under `alias`, allocating its address.
    /// A sandbox may be a member of several networks at once (with its own
    /// alias, possibly different, on each), but only once each.
    pub fn connect(&self, name: &str, sandbox_id: &str, alias: &str) -> Result<Ipv4Addr, Error> {
        let mut inner = self.inner.lock();
        let net = inner.networks.get_mut(name).ok_or(Error::NotFound)?;
        if net.by_sandbox.contains_key(sandbox_id) {
            return Err(Error::AlreadyMember);
        }
        if net.by_alias.contains_key(alias) {
            return Err(Error::AliasTaken);
        }
        let address = net.next_address().ok_or(Error::Exhausted)?;
        net.by_alias.insert(
            alias.to_string(),
            Member {
                sandbox_id: sandbox_id.to_string(),
                alias: alias.to_string(),
                address,
            },
        );
        net.by_sandbox
            .insert(sandbox_id.to_string(), alias.to_string());
        Ok(address)
    }

    pub fn disconnect(&self, name: &str, sandbox_id: &str) -> Result<(), Error> {
        let mut inner = self.inner.lock();
        let net = inner.networks.get_mut(name).ok_or(Error::NotFound)?;
        let alias = net.by_sandbox.remove(sandbox_id).ok_or(Error::NotAMember)?;
        net.by_alias.remove(&alias);
        Ok(())
    }

    /// Leave every network, for a sandbox that no longer exists. Harmless
    /// to call for a sandbox in none (deletion always calls it): a stale
    /// membership is otherwise self-healing anyway, since `dial` refuses
    /// once its target sandbox is gone, but this frees its alias and
    /// address immediately rather than leaving them held until someone
    /// else's `connect` happens to notice.
    pub fn disconnect_all(&self, sandbox_id: &str) {
        let mut inner = self.inner.lock();
        for net in inner.networks.values_mut() {
            if let Some(alias) = net.by_sandbox.remove(sandbox_id) {
                net.by_alias.remove(&alias);
            }
        }
    }

    /// `name`'s address, in a network `source` itself belongs to -- the
    /// same per-network scoping Docker's own embedded DNS gives a
    /// container, and what makes a `disconnect` take hold on the very next
    /// lookup rather than something cached from before it.
    fn resolve_for(&self, source: &str, name: &str) -> Option<Ipv4Addr> {
        let inner = self.inner.lock();
        inner
            .networks
            .values()
            .filter(|net| net.by_sandbox.contains_key(source))
            .find_map(|net| net.by_alias.get(name).map(|m| m.address))
    }

    /// The sandbox `address` belongs to, in a network `source` is also in.
    fn owner_of(&self, source: &str, address: Ipv4Addr) -> Option<String> {
        let inner = self.inner.lock();
        inner
            .networks
            .values()
            .filter(|net| net.by_sandbox.contains_key(source))
            .find_map(|net| {
                net.by_alias
                    .values()
                    .find(|m| m.address == address)
                    .map(|m| m.sandbox_id.clone())
            })
    }
}

fn describe(name: &str, net: &Network) -> NetworkInfo {
    let mut members: Vec<Member> = net.by_alias.values().cloned().collect();
    members.sort_by(|a, b| a.alias.cmp(&b.alias));
    NetworkInfo {
        name: name.to_string(),
        members,
    }
}

fn refused() -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, "not on this network")
}

/// One sandbox's view of [`Registry`]: the [`PrivateNetwork`] its gateway
/// is built with. Cheap to construct (no lookup happens until a guest
/// actually asks), so sandbox creation makes one unconditionally whenever
/// the sandbox has a network at all -- whether it has joined a local
/// network yet or ever does is immaterial to building it.
pub struct LocalNetwork {
    registry: Arc<Registry>,
    state: Weak<AppState>,
    source: String,
}

impl LocalNetwork {
    pub fn new(registry: Arc<Registry>, state: &Arc<AppState>, source: impl Into<String>) -> Self {
        Self {
            registry,
            state: Arc::downgrade(state),
            source: source.into(),
        }
    }
}

#[async_trait::async_trait]
impl PrivateNetwork for LocalNetwork {
    fn owns_name(&self, name: &str) -> bool {
        self.registry.resolve_for(&self.source, name).is_some()
    }

    fn owns_address(&self, address: IpAddr) -> bool {
        match address {
            IpAddr::V4(v4) => self.registry.owner_of(&self.source, v4).is_some(),
            IpAddr::V6(_) => false,
        }
    }

    async fn resolve(&self, name: &str) -> std::io::Result<Vec<IpAddr>> {
        Ok(self
            .registry
            .resolve_for(&self.source, name)
            .map(|ip| vec![IpAddr::V4(ip)])
            .unwrap_or_default())
    }

    async fn dial(&self, destination: SocketAddr) -> std::io::Result<Box<dyn Upstream>> {
        let IpAddr::V4(address) = destination.ip() else {
            return Err(std::io::ErrorKind::Unsupported.into());
        };
        let target_id = self
            .registry
            .owner_of(&self.source, address)
            .ok_or_else(refused)?;
        let state = self.state.upgrade().ok_or_else(refused)?;
        let vm = {
            let sandboxes = state.sandboxes.lock();
            sandboxes.get(&target_id).map(|live| Arc::clone(&live.vm))
        }
        .ok_or_else(refused)?;

        let (stream, early) = vm
            .forward_port(destination.port(), OPEN_TIMEOUT)
            .await
            .map_err(std::io::Error::other)?;

        // `stream` is a blocking vsock handle (`forward_port`'s own
        // contract), and `Upstream` needs an async one. Rather than writing
        // a new blocking/async bridge, open a loopback TCP pair, splice the
        // vsock side onto one end on a blocking thread with the exact
        // function the port-publish path already uses, and hand the
        // gateway the other end -- a plain `tokio::net::TcpStream`, already
        // `Upstream`.
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let (accepted, our_end) =
            tokio::try_join!(listener.accept(), tokio::net::TcpStream::connect(addr))?;
        let (accepted, _) = accepted;
        let accepted = accepted.into_std()?;
        accepted.set_nonblocking(false)?;
        tokio::task::spawn_blocking(move || crate::forwards::splice(&stream, accepted, &early));
        Ok(Box::new(our_end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connect_allocates_distinct_addresses_and_resolves_only_within_membership() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry.create("wan").unwrap();

        let box_ip = registry.connect("site", "box", "box").unwrap();
        let siem_ip = registry
            .connect("site", "siem", "siem.example.internal")
            .unwrap();
        let relay_site_ip = registry
            .connect("site", "relay", "relay.aircom.test")
            .unwrap();
        let relay_wan_ip = registry
            .connect("wan", "relay", "relay.aircom.test")
            .unwrap();
        let phone_ip = registry.connect("wan", "phone", "phone").unwrap();

        assert_ne!(box_ip, siem_ip);
        assert_ne!(
            relay_site_ip, relay_wan_ip,
            "the same alias on two networks gets two addresses"
        );

        // box (site only) can resolve siem (site) but not phone (wan only).
        assert_eq!(
            registry.resolve_for("box", "siem.example.internal"),
            Some(siem_ip)
        );
        assert_eq!(registry.resolve_for("box", "phone"), None);
        // phone (wan only) can resolve its own alias (it shares "wan" with
        // itself, same as any other member) but not box, which is site-only.
        assert_eq!(registry.resolve_for("phone", "phone"), Some(phone_ip));
        assert_eq!(registry.resolve_for("phone", "box"), None);

        // relay is on both and can see both.
        assert_eq!(registry.resolve_for("relay", "box"), Some(box_ip));
        assert_eq!(registry.resolve_for("relay", "phone"), Some(phone_ip));

        // owner_of agrees with resolve, and is likewise scoped by membership.
        assert_eq!(registry.owner_of("box", siem_ip), Some("siem".to_string()));
        assert_eq!(registry.owner_of("box", phone_ip), None);
    }

    #[test]
    fn disconnect_frees_the_alias_and_refuses_further_resolution() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry.connect("site", "box", "box").unwrap();
        registry
            .connect("site", "siem", "siem.example.internal")
            .unwrap();
        assert!(registry
            .resolve_for("box", "siem.example.internal")
            .is_some());

        registry.disconnect("site", "siem").unwrap();
        assert_eq!(registry.resolve_for("box", "siem.example.internal"), None);

        // The alias is free again, for the same or another sandbox.
        registry
            .connect("site", "siem2", "siem.example.internal")
            .unwrap();
    }

    #[test]
    fn a_member_rejoining_is_never_given_an_address_still_in_use() {
        let registry = Registry::default();
        registry.create("wan").unwrap();
        let a = registry.connect("wan", "a", "a.test").unwrap();
        registry.connect("wan", "b", "b.test").unwrap();
        let c = registry.connect("wan", "c", "c.test").unwrap();

        registry.disconnect("wan", "b").unwrap();
        let b = registry.connect("wan", "b", "b.test").unwrap();

        assert_ne!(b, a);
        assert_ne!(b, c);
        assert_eq!(registry.owner_of("a", b), Some("b".to_string()));
        assert_eq!(registry.owner_of("a", c), Some("c".to_string()));
    }

    #[test]
    fn reconnect_after_disconnect_restores_resolution_with_a_fresh_address() {
        let registry = Registry::default();
        registry.create("wan").unwrap();
        let first = registry
            .connect("wan", "relay", "relay.aircom.test")
            .unwrap();
        registry.disconnect("wan", "relay").unwrap();
        assert_eq!(registry.resolve_for("relay", "relay.aircom.test"), None);
        let second = registry
            .connect("wan", "relay", "relay.aircom.test")
            .unwrap();
        assert_eq!(
            registry.resolve_for("relay", "relay.aircom.test"),
            Some(second)
        );
        let _ = first; // addresses may or may not be reused; only resolution is asserted
    }

    #[test]
    fn duplicate_alias_and_duplicate_membership_are_refused() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry.connect("site", "box", "box").unwrap();
        assert_eq!(
            registry.connect("site", "other", "box"),
            Err(Error::AliasTaken)
        );
        assert_eq!(
            registry.connect("site", "box", "box2"),
            Err(Error::AlreadyMember)
        );
    }

    #[test]
    fn unknown_network_is_not_found() {
        let registry = Registry::default();
        assert_eq!(registry.connect("none", "box", "box"), Err(Error::NotFound));
        assert_eq!(registry.disconnect("none", "box"), Err(Error::NotFound));
    }

    #[test]
    fn remove_refuses_while_a_member_remains() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry.connect("site", "box", "box").unwrap();
        assert_eq!(registry.remove("site"), Err(Error::NotEmpty));
        registry.disconnect("site", "box").unwrap();
        registry.remove("site").unwrap();
    }

    #[test]
    fn creating_the_same_network_twice_is_refused() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        assert_eq!(registry.create("site"), Err(Error::AlreadyExists));
    }

    #[test]
    fn disconnect_all_leaves_every_network_a_sandbox_was_in() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry.create("wan").unwrap();
        registry
            .connect("site", "relay", "relay.aircom.test")
            .unwrap();
        registry
            .connect("wan", "relay", "relay.aircom.test")
            .unwrap();

        registry.disconnect_all("relay");

        assert_eq!(registry.disconnect("site", "relay"), Err(Error::NotAMember));
        assert_eq!(registry.disconnect("wan", "relay"), Err(Error::NotAMember));
        // The alias is free on both again.
        registry
            .connect("site", "other", "relay.aircom.test")
            .unwrap();
        registry
            .connect("wan", "other", "relay.aircom.test")
            .unwrap();
    }

    #[test]
    fn list_and_inspect_report_every_member_sorted_by_alias() {
        let registry = Registry::default();
        registry.create("site").unwrap();
        registry
            .connect("site", "siem", "siem.example.internal")
            .unwrap();
        registry.connect("site", "box", "box").unwrap();

        let info = registry.inspect("site").unwrap();
        let aliases: Vec<&str> = info.members.iter().map(|m| m.alias.as_str()).collect();
        assert_eq!(aliases, vec!["box", "siem.example.internal"]);

        let listed = registry.list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "site");

        assert!(registry.inspect("none").is_none());
    }
}
