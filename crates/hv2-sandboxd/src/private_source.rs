//! Trusted local source leases for private node dialing.
//! The caller supplies its own VM handle; no guest request selects a source.
use super::{ActivityGuard, AppState};
use hv2_agent::AgentVM;
use hv2_cluster::{model::SandboxRecord, private_node::PrivateSourceLease};
use std::{
    io,
    sync::{Arc, Weak},
};

fn refused() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "local private source refused",
    )
}
fn record_matches(
    expected: &SandboxRecord,
    current: &SandboxRecord,
    pending: bool,
    now: u64,
) -> bool {
    !pending
        && expected.owner_id.is_some()
        && current.owner_id == expected.owner_id
        && current.sandbox_id == expected.sandbox_id
        && current.node_id == expected.node_id
        && current.started_at_ms == expected.started_at_ms
        && !current.paused
        && current.started_at_ms <= now
        && current.end_at_ms > now
}

/// Weak references avoid retaining a deleted VM or the whole daemon. Activity
/// is retained until the connector drops this lease on refusal or stream close.
struct LocalSourceLease {
    state: Weak<AppState>,
    vm: Weak<AgentVM>,
    _activity: ActivityGuard,
}
impl PrivateSourceLease for LocalSourceLease {
    fn validate(&self, expected: &SandboxRecord) -> io::Result<()> {
        let state = self.state.upgrade().ok_or_else(refused)?;
        let vm = self.vm.upgrade().ok_or_else(refused)?;
        // Polling never blocks on the registry. WouldBlock carries no bytes;
        // connector setup/polling retries under its bounded deadline.
        let local = state
            .sandboxes
            .try_lock()
            .ok_or_else(|| io::Error::from(io::ErrorKind::WouldBlock))?;
        let live = local.get(&expected.sandbox_id).ok_or_else(refused)?;
        if !Arc::ptr_eq(&live.vm, &vm)
            || vm.state() != hv2_core::VMState::Running
            || !record_matches(
                expected,
                &live.record,
                live.pending_registration.is_some(),
                super::now_ms(),
            )
        {
            return Err(refused());
        }
        Ok(())
    }
}

/// Acquire from the actual committed local VM. A same-ID replacement cannot
/// inherit this lease. Registry identity check and activity entry are atomic.
/// Guest gateway integration must call this with its own fixed VM handle.
pub(crate) fn acquire(
    state: &Arc<AppState>,
    source_id: &str,
    source_vm: &Arc<AgentVM>,
) -> io::Result<(SandboxRecord, Box<dyn PrivateSourceLease>)> {
    let local = state
        .sandboxes
        .try_lock()
        .ok_or_else(|| io::Error::from(io::ErrorKind::WouldBlock))?;
    let live = local.get(source_id).ok_or_else(refused)?;
    if !Arc::ptr_eq(&live.vm, source_vm)
        || source_vm.state() != hv2_core::VMState::Running
        || !record_matches(
            &live.record,
            &live.record,
            live.pending_registration.is_some(),
            super::now_ms(),
        )
    {
        return Err(refused());
    }
    let record = live.record.clone();
    let lease = LocalSourceLease {
        state: Arc::downgrade(state),
        vm: Arc::downgrade(source_vm),
        _activity: ActivityGuard::enter(&live.activity),
    };
    Ok((record, Box::new(lease)))
}

/// A gateway's fixed source VM. Weak references make late callbacks refuse
/// after deletion, replacement or daemon teardown rather than retain a VM.
struct LocalSourceFactory {
    state: Weak<AppState>,
    vm: Weak<AgentVM>,
    source_id: String,
}
impl LocalSourceFactory {
    fn source(&self) -> io::Result<(SandboxRecord, Box<dyn PrivateSourceLease>)> {
        let state = self.state.upgrade().ok_or_else(refused)?;
        let vm = self.vm.upgrade().ok_or_else(refused)?;
        acquire(&state, &self.source_id, &vm)
    }
}
impl hv2_cluster::private_router::PrivateSourceLeaseFactory for LocalSourceFactory {
    fn acquire(&self) -> io::Result<Box<dyn PrivateSourceLease>> {
        self.source().map(|(_, lease)| lease)
    }
}
struct GatewayPrivateRouter {
    factory: Arc<LocalSourceFactory>,
    store: Arc<dyn hv2_cluster::store::ClusterStore>,
    tls: hv2_cluster::mtls::Mtls,
    token: String,
    router: tokio::sync::OnceCell<Arc<hv2_cluster::private_router::PrivateSourceRouter>>,
}
impl GatewayPrivateRouter {
    async fn router(&self) -> io::Result<&Arc<hv2_cluster::private_router::PrivateSourceRouter>> {
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.router.get_or_try_init(|| async {
                // Boot builds the NIC before local registration. Initialize only
                // once the actual VM is present and its publication is committed.
                let (source, lease) = loop {
                    match self.factory.source() {
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                        }
                        result => break result?,
                    }
                };
                let connector = hv2_cluster::private_node::PrivateNodeConnector::new(
                    self.store.clone(),
                    &self.tls,
                    &self.token,
                    source.clone(),
                )?;
                loop {
                    match lease.validate(&source) {
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
                        }
                        result => {
                            result?;
                            break;
                        }
                    }
                }
                Ok(Arc::new(
                    hv2_cluster::private_router::PrivateSourceRouter::new(
                        Arc::new(connector),
                        self.factory.clone(),
                    ),
                ))
            }),
        )
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
    }
}
#[async_trait::async_trait]
impl hv2_net::gateway::PrivateNetwork for GatewayPrivateRouter {
    fn owns_name(&self, name: &str) -> bool {
        hv2_cluster::private_router::PrivateSourceRouter::owns_name(name)
    }
    fn owns_address(&self, address: std::net::IpAddr) -> bool {
        hv2_cluster::private_router::PrivateSourceRouter::owns_address(address)
    }
    async fn resolve(&self, name: &str) -> io::Result<Vec<std::net::IpAddr>> {
        self.router().await?.resolve(name).await
    }
    async fn dial(
        &self,
        destination: std::net::SocketAddr,
    ) -> io::Result<Box<dyn hv2_net::gateway::Upstream>> {
        Ok(Box::new(self.router().await?.dial(destination).await?))
    }
    async fn dial_udp(
        &self,
        destination: std::net::SocketAddr,
    ) -> io::Result<Box<dyn hv2_net::gateway::Upstream>> {
        Ok(Box::new(self.router().await?.dial_udp(destination).await?))
    }
}
/// Install private routing for authenticated mTLS cluster guests. Source state
/// comes from the fixed gateway VM, never its packets, DNS or caller headers.
pub(crate) fn for_gateway(
    state: &Arc<AppState>,
    source_id: &str,
    vm: &Arc<AgentVM>,
) -> io::Result<Option<Arc<dyn hv2_net::gateway::PrivateNetwork>>> {
    if !super::forwards::private_transport_enabled(
        state.node.is_some(),
        state.opts.cluster_token.as_deref(),
        [
            state.opts.mtls_ca.is_some(),
            state.opts.mtls_cert.is_some(),
            state.opts.mtls_key.is_some(),
        ],
    ) {
        return Ok(None);
    }
    let node = state.node.as_ref().ok_or_else(refused)?;
    let tls = hv2_cluster::mtls::Mtls::load(
        std::path::Path::new(state.opts.mtls_ca.as_deref().ok_or_else(refused)?),
        std::path::Path::new(state.opts.mtls_cert.as_deref().ok_or_else(refused)?),
        std::path::Path::new(state.opts.mtls_key.as_deref().ok_or_else(refused)?),
        hv2_cluster::mtls::DEFAULT_NODE_NAME,
    )
    .map_err(|_| io::Error::other("private gateway TLS configuration failed"))?;
    Ok(Some(Arc::new(GatewayPrivateRouter {
        factory: Arc::new(LocalSourceFactory {
            state: Arc::downgrade(state),
            vm: Arc::downgrade(vm),
            source_id: source_id.into(),
        }),
        store: node.store().clone(),
        tls,
        token: state.opts.cluster_token.clone().ok_or_else(refused)?,
        router: tokio::sync::OnceCell::new(),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> SandboxRecord {
        SandboxRecord {
            sandbox_id: "source".into(),
            owner_id: Some(hv2_cluster::ownership::OwnerId::parse("owner-a").unwrap()),
            node_id: "node".into(),
            template_id: "base".into(),
            started_at_ms: 10,
            end_at_ms: 100,
            cpu_count: 1,
            memory_mb: 1024,
            metadata: Default::default(),
            envd_version: "test".into(),
            descriptor: serde_json::Value::Null,
            paused: false,
            portable: false,
            volume_mounts: vec![],
        }
    }
    #[test]
    fn local_source_refuses_pending_expired_paused_or_changed_identity() {
        let expected = record();
        assert!(record_matches(&expected, &expected, false, 50));
        assert!(!record_matches(&expected, &expected, true, 50));
        assert!(!record_matches(&expected, &expected, false, 100));
        assert!(!record_matches(&expected, &expected, false, 9));
        for change in 0..5 {
            let mut current = expected.clone();
            match change {
                0 => current.owner_id = None,
                1 => current.sandbox_id = "other".into(),
                2 => current.node_id = "moved".into(),
                3 => current.started_at_ms = 11,
                _ => current.paused = true,
            }
            assert!(!record_matches(&expected, &current, false, 50));
        }
        let mut ownerless = expected;
        ownerless.owner_id = None;
        assert!(!record_matches(&ownerless, &ownerless, false, 50));
    }
    #[test]
    fn lost_local_state_refuses_and_lease_drop_releases_activity() {
        let activity = super::super::Activity::new();
        let lease = LocalSourceLease {
            state: Weak::new(),
            vm: Weak::new(),
            _activity: ActivityGuard::enter(&activity),
        };
        assert!(activity.busy());
        assert_eq!(
            lease.validate(&record()).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        drop(lease);
        assert!(!activity.busy());
    }
    #[test]
    fn gateway_factory_refuses_after_its_daemon_is_gone() {
        use hv2_cluster::private_router::PrivateSourceLeaseFactory;
        let factory = LocalSourceFactory {
            state: Weak::new(),
            vm: Weak::new(),
            source_id: "source".into(),
        };
        assert!(
            matches!(factory.acquire(), Err(ref error) if error.kind() == io::ErrorKind::PermissionDenied)
        );
        assert!(hv2_cluster::private_router::PrivateSourceRouter::owns_name(
            "unknown.team.hv2.internal"
        ));
    }
}
