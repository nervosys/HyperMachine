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
        // Polling a stream must never block on the registry. Contention fails
        // closed; a caller may establish a new connection after it clears.
        let local = state.sandboxes.try_lock().ok_or_else(refused)?;
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
    let local = state.sandboxes.try_lock().ok_or_else(refused)?;
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
}
