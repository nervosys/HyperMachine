//! Membership-backed private names and generation-pinned source dialing.
//! A gateway binds this router to its own trusted VM lease factory. No guest
//! header or DNS label selects the source identity.
use crate::{
    model::SandboxRecord,
    native_tcp::NativeTunnel,
    private_addresses::PrivateAddressBook,
    private_networks::{MembershipAccess, MembershipChange, NetworkTag, PrivateRouteSnapshot},
    private_node::{PrivateNodeConnector, PrivateSourceLease},
    store::ClusterStore,
};
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

pub trait PrivateSourceLeaseFactory: Send + Sync {
    fn acquire(&self) -> io::Result<Box<dyn PrivateSourceLease>>;
}
pub struct PrivateSourceRouter {
    source: SandboxRecord,
    store: Arc<dyn ClusterStore>,
    connector: Arc<PrivateNodeConnector>,
    leases: Arc<dyn PrivateSourceLeaseFactory>,
}
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "private route refused")
}
fn unavailable() -> io::Error {
    io::Error::other("private route state unavailable")
}
fn parse_name(name: &str) -> io::Result<(String, NetworkTag)> {
    if name.len() > 253 {
        return Err(refused());
    }
    let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
    let value = name.strip_suffix(".hv2.internal").ok_or_else(refused)?;
    let (destination, network) = value.split_once('.').ok_or_else(refused)?;
    // The explicit namespace uses lowercase DNS labels for generated VM IDs.
    NetworkTag::parse(destination).map_err(|_| refused())?;
    let network = NetworkTag::parse(network).map_err(|_| refused())?;
    Ok((destination.into(), network))
}
impl PrivateSourceRouter {
    pub fn new(
        connector: Arc<PrivateNodeConnector>,
        leases: Arc<dyn PrivateSourceLeaseFactory>,
    ) -> Self {
        Self {
            source: connector.source_record().clone(),
            store: connector.cluster_store(),
            connector,
            leases,
        }
    }
    pub fn owns_name(name: &str) -> bool {
        let name = name.trim_end_matches('.').to_ascii_lowercase();
        name == "hv2.internal" || name.ends_with(".hv2.internal")
    }
    pub fn owns_address(address: IpAddr) -> bool {
        matches!(address,IpAddr::V4(ip) if PrivateAddressBook::owns_address(ip))
    }
    async fn acquire_lease(&self) -> io::Result<Box<dyn PrivateSourceLease>> {
        loop {
            match self.leases.acquire() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                result => return result,
            }
        }
    }
    async fn ledger(&self) -> io::Result<(Option<Vec<u8>>, PrivateAddressBook)> {
        let bytes = match self
            .store
            .private_address_ledger(&self.source)
            .await
            .map_err(|_| unavailable())?
        {
            MembershipAccess::Granted(bytes) => bytes,
            _ => return Err(refused()),
        };
        let book = match &bytes {
            Some(bytes) => PrivateAddressBook::restore(bytes, &self.source),
            None => PrivateAddressBook::new(&self.source),
        }
        .map_err(|_| unavailable())?;
        Ok((bytes, book))
    }
    async fn view(&self, destination: &str) -> io::Result<PrivateRouteSnapshot> {
        let view = self
            .store
            .private_route_snapshot(&self.source.sandbox_id, destination)
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        if view.source_record.owner_id != self.source.owner_id
            || view.source_record.node_id != self.source.node_id
            || view.source_record.started_at_ms != self.source.started_at_ms
        {
            return Err(refused());
        }
        let (source, destination) = tokio::join!(
            self.store.node(&view.source_record.node_id),
            self.store.node(&view.destination_record.node_id)
        );
        let source = source.map_err(|_| unavailable())?.ok_or_else(refused)?;
        let destination = destination
            .map_err(|_| unavailable())?
            .ok_or_else(refused)?;
        if source.id != view.source_record.node_id
            || destination.id != view.destination_record.node_id
        {
            return Err(refused());
        }
        crate::private_node::node_origin(&destination.api)?;
        Ok(view)
    }
    /// Resolve `<sandbox-id>.<tag>.hv2.internal`. Commit the ledger before
    /// returning any address; a refusal/error/timeout publishes nothing.
    pub async fn resolve(&self, name: &str) -> io::Result<Vec<IpAddr>> {
        let (destination, network) = parse_name(name)?;
        let attempt = async {
            let lease = self.acquire_lease().await?;
            crate::private_node::validate_source(lease.as_ref(), &self.source).await?;
            for _ in 0..4 {
                let view = self.view(&destination).await?;
                let (expected, book) = self.ledger().await?;
                let ip = book
                    .allocate(&view, network.clone(), crate::model::now_ms(), false, false)
                    .map_err(|_| refused())?;
                crate::private_node::validate_source(lease.as_ref(), &self.source).await?;
                let next = book.snapshot().map_err(|_| unavailable())?;
                match self
                    .store
                    .compare_private_address_ledger(&self.source, expected.as_deref(), &next)
                    .await
                    .map_err(|_| unavailable())?
                {
                    MembershipChange::Applied => {
                        let fresh = self.view(&destination).await?;
                        book.authorize(ip, 1, &fresh, crate::model::now_ms(), false, false)
                            .map_err(|_| refused())?;
                        crate::private_node::validate_source(lease.as_ref(), &self.source).await?;
                        return Ok(vec![IpAddr::V4(ip)]);
                    }
                    MembershipChange::RevisionConflict => continue,
                    _ => return Err(refused()),
                }
            }
            Err(unavailable())
        };
        tokio::time::timeout(Duration::from_secs(5), attempt)
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
    /// Only committed address bindings can dial. Preserve their generations
    /// through the connector's fresh pre/post-upgrade authorization checks.
    pub async fn dial(&self, destination: SocketAddr) -> io::Result<Box<dyn NativeTunnel>> {
        self.dial_inner(destination, false).await
    }
    /// Open a framed UDP stream only from a committed current private binding.
    /// Datagram encoding/decoding belongs to the gateway session layer.
    pub async fn dial_udp(&self, destination: SocketAddr) -> io::Result<Box<dyn NativeTunnel>> {
        self.dial_inner(destination, true).await
    }
    async fn dial_inner(
        &self,
        destination: SocketAddr,
        udp: bool,
    ) -> io::Result<Box<dyn NativeTunnel>> {
        let IpAddr::V4(ip) = destination.ip() else {
            return Err(refused());
        };
        if !Self::owns_address(destination.ip()) || destination.port() == 0 {
            return Err(refused());
        }
        let attempt = async {
            let lease = self.acquire_lease().await?;
            crate::private_node::validate_source(lease.as_ref(), &self.source).await?;
            let (persisted, book) = self.ledger().await?;
            if persisted.is_none() {
                return Err(refused());
            }
            let binding = book
                .binding(ip, destination.port())
                .map_err(|_| refused())?;
            let view = self.view(&binding.destination_id).await?;
            book.authorize(
                ip,
                destination.port(),
                &view,
                crate::model::now_ms(),
                false,
                false,
            )
            .map_err(|_| refused())?;
            if udp {
                self.connector.open_bound_udp(binding, lease).await
            } else {
                self.connector.open_bound_tcp(binding, lease).await
            }
        };
        tokio::time::timeout(Duration::from_secs(20), attempt)
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ownership::OwnerId,
        private_networks::NetworkMembershipState,
        store::{MemoryStore, RedisStore},
    };
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    struct Factory {
        active: Arc<AtomicBool>,
        dropped: Arc<AtomicUsize>,
    }
    struct Lease {
        active: Arc<AtomicBool>,
        dropped: Arc<AtomicUsize>,
    }
    impl Drop for Lease {
        fn drop(&mut self) {
            self.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }
    impl PrivateSourceLease for Lease {
        fn validate(&self, _: &SandboxRecord) -> io::Result<()> {
            if self.active.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(refused())
            }
        }
    }
    impl PrivateSourceLeaseFactory for Factory {
        fn acquire(&self) -> io::Result<Box<dyn PrivateSourceLease>> {
            Ok(Box::new(Lease {
                active: self.active.clone(),
                dropped: self.dropped.clone(),
            }))
        }
    }
    #[test]
    fn namespace_claims_unknown_names_without_accepting_malformed_routes() {
        for name in [
            "hv2.internal",
            "UNKNOWN.TEAM.HV2.INTERNAL.",
            "bad.name.extra.hv2.internal",
        ] {
            assert!(PrivateSourceRouter::owns_name(name));
        }
        assert!(!PrivateSourceRouter::owns_name("foo.hv2.internal.example"));
        assert_eq!(
            parse_name("DESTINATION.TEAM.HV2.INTERNAL.").unwrap(),
            ("destination".into(), NetworkTag::parse("team").unwrap())
        );
        for name in [
            "hv2.internal",
            "source..hv2.internal",
            "a.b.c.hv2.internal",
            "-bad.team.hv2.internal",
        ] {
            assert!(parse_name(name).is_err());
        }
    }
    async fn contract(store: Arc<dyn ClusterStore>) {
        let identity = crate::native_node::tests::identity(crate::mtls::DEFAULT_NODE_NAME);
        let mut source = crate::store::tests::sandbox("router-source", "router-node");
        source.owner_id = Some(OwnerId::parse("router-owner").unwrap());
        let mut destination = crate::store::tests::sandbox("router-destination", "router-node");
        destination.owner_id = source.owner_id.clone();
        let mut other = crate::store::tests::sandbox("router-other", "router-node");
        other.owner_id = Some(OwnerId::parse("other-owner").unwrap());
        let tags = || {
            vec![
                NetworkTag::parse("team").unwrap(),
                NetworkTag::parse("dev").unwrap(),
            ]
        };
        let mut destination_revision = String::new();
        for record in [&source, &destination, &other] {
            store.put_sandbox(record).await.unwrap();
            let m = NetworkMembershipState::new(record, Some(tags())).unwrap();
            assert_eq!(
                store.compare_private_membership(None, &m).await.unwrap(),
                MembershipChange::Applied
            );
            if record.sandbox_id == destination.sandbox_id {
                destination_revision = m.revision().into();
            }
        }
        let mut node = crate::store::tests::node("router-node", 1, 4);
        node.api = "https://127.0.0.1:1".into();
        store
            .put_node(&node, Duration::from_secs(60))
            .await
            .unwrap();
        let factory = Arc::new(Factory {
            active: Arc::new(AtomicBool::new(true)),
            dropped: Arc::new(AtomicUsize::new(0)),
        });
        let connector = Arc::new(
            PrivateNodeConnector::new(
                store.clone(),
                &identity.gateway,
                "owned-token",
                source.clone(),
            )
            .unwrap(),
        );
        let router = Arc::new(PrivateSourceRouter::new(connector, factory.clone()));
        factory.active.store(false, Ordering::SeqCst);
        assert!(router
            .resolve("router-destination.team.hv2.internal")
            .await
            .is_err());
        assert_eq!(
            store.private_address_ledger(&source).await.unwrap(),
            MembershipAccess::Granted(None)
        );
        factory.active.store(true, Ordering::SeqCst);
        let (team, dev) = tokio::join!(
            router.resolve("router-destination.team.hv2.internal"),
            router.resolve("router-destination.dev.hv2.internal")
        );
        let team = team.unwrap()[0];
        let dev = dev.unwrap()[0];
        assert_ne!(team, dev);
        let persisted = match store.private_address_ledger(&source).await.unwrap() {
            MembershipAccess::Granted(Some(bytes)) => bytes,
            _ => panic!("DNS returned before ledger persisted"),
        };
        let restored = PrivateAddressBook::restore(&persisted, &source).unwrap();
        for ip in [team, dev] {
            let IpAddr::V4(ip) = ip else { panic!() };
            assert_eq!(
                restored.binding(ip, 8080).unwrap().destination_id,
                destination.sandbox_id
            );
        }
        assert_eq!(
            router
                .resolve("router-destination.team.hv2.internal")
                .await
                .unwrap(),
            vec![team]
        );
        for name in [
            "router-other.team.hv2.internal",
            "router-destination.isolated.hv2.internal",
            "missing.team.hv2.internal",
        ] {
            assert!(router.resolve(name).await.is_err());
        }
        let fresh = NetworkMembershipState::new(&destination, Some(tags())).unwrap();
        assert_eq!(
            store
                .compare_private_membership(Some(&destination_revision), &fresh)
                .await
                .unwrap(),
            MembershipChange::Applied
        );
        assert!(
            matches!(router.dial(SocketAddr::new(team,8080)).await,Err(ref e) if e.kind()==io::ErrorKind::PermissionDenied)
        );
        assert!(
            matches!(router.dial_udp(SocketAddr::new(team,8080)).await,Err(ref e) if e.kind()==io::ErrorKind::PermissionDenied)
        );
        for address in ["127.0.0.1:8080", "[::1]:8080", "198.18.0.1:0"] {
            assert!(
                matches!(router.dial_udp(address.parse().unwrap()).await,Err(ref e) if e.kind()==io::ErrorKind::PermissionDenied)
            );
        }
        let new_ip = router
            .resolve("router-destination.team.hv2.internal")
            .await
            .unwrap()[0];
        assert_ne!(team, new_ip);
        assert_ne!(dev, new_ip);
        store.remove_node("router-node").await.unwrap();
        assert!(router
            .resolve("router-destination.team.hv2.internal")
            .await
            .is_err());
        assert_eq!(factory.dropped.load(Ordering::SeqCst), 11);
        eprintln!(
            "private_source_router_contract completed dns_commit_before_return=true concurrent_rebase=true cross_owner=refused stale_address=refused node_missing=refused"
        );
    }
    #[tokio::test]
    async fn memory_router_publishes_only_committed_current_bindings() {
        contract(Arc::new(MemoryStore::new())).await;
    }
    #[tokio::test]
    async fn redis_router_publishes_only_committed_current_bindings() {
        let Ok(url) = std::env::var("HV2_TEST_REDIS") else {
            eprintln!("skipped: set HV2_TEST_REDIS for private router contract");
            return;
        };
        let store = RedisStore::connect(&url, &format!("private-router-{}", uuid::Uuid::new_v4()))
            .await
            .unwrap();
        contract(Arc::new(store)).await;
    }
}
