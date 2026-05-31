//! Permission delegation — controlled transfer of a permission subset.
//!
//! An agent can delegate only permissions it already holds, with:
//! - **Attenuation**: the delegated set is always ⊆ the delegator's set.
//! - **Depth limits**: each hop decrements the remaining depth counter.
//! - **Scope narrowing**: delegations can only narrow, never widen scope.
//! - **Expiry propagation**: a delegated grant cannot outlive its parent.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use super::graph::{GrantId, PermissionGraph, PermissionSet, PrincipalId};
use super::hierarchy::ResourceScope;
use super::{PermissionError, PermissionResult};

/// Constraints applied when creating a delegation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConstraint {
    /// Maximum permissions that may be delegated (must be ⊆ delegator's set).
    pub max_permissions: PermissionSet,
    /// Scope must be equal or narrower than the parent grant.
    pub max_scope: ResourceScope,
    /// Maximum further delegation hops (0 = terminal).
    pub max_depth: u32,
    /// Delegation cannot outlive this instant.
    pub expires_at: Option<SystemTime>,
}

/// Record of a single delegation hop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    /// The grant that was created as a result of this delegation.
    pub grant_id: GrantId,
    /// Who delegated.
    pub from: PrincipalId,
    /// Who received.
    pub to: PrincipalId,
    /// Permissions conveyed.
    pub permissions: PermissionSet,
    /// Scope of the delegation.
    pub scope: ResourceScope,
    /// Remaining depth after this hop (0 = cannot re-delegate).
    pub remaining_depth: u32,
    pub created_at: SystemTime,
}

/// Ordered chain of delegations from an original grant to a terminal grantee.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DelegationChain {
    pub hops: Vec<Delegation>,
}

impl DelegationChain {
    pub fn depth(&self) -> u32 {
        self.hops.len() as u32
    }

    pub fn origin(&self) -> Option<&PrincipalId> {
        self.hops.first().map(|h| &h.from)
    }

    pub fn terminal(&self) -> Option<&PrincipalId> {
        self.hops.last().map(|h| &h.to)
    }
}

// ── Delegation logic on the graph ──────────────────────────────────

impl PermissionGraph {
    /// Delegate a subset of permissions from `delegator` to `delegatee`.
    ///
    /// - The delegator must already hold every permission in `permissions`
    ///   at a scope that contains `scope`.
    /// - The resulting grant's delegation depth is `min(parent_remaining - 1, constraint.max_depth)`.
    pub fn delegate(
        &self,
        delegator: &PrincipalId,
        delegatee: &PrincipalId,
        permissions: PermissionSet,
        scope: ResourceScope,
        constraint: &DelegationConstraint,
    ) -> PermissionResult<GrantId> {
        // 1. Verify both principals exist.
        if self.get_principal(delegator).is_none() {
            return Err(PermissionError::PrincipalNotFound(delegator.to_string()));
        }
        if self.get_principal(delegatee).is_none() {
            return Err(PermissionError::PrincipalNotFound(delegatee.to_string()));
        }

        // 2. Scope must be within the constraint's max_scope.
        if !constraint.max_scope.contains(&scope) {
            return Err(PermissionError::InvalidScope(format!(
                "delegation scope {} is outside allowed scope {}",
                scope, constraint.max_scope
            )));
        }

        // 3. Requested permissions must be ⊆ constraint.max_permissions.
        if !constraint.max_permissions.allows_all(&permissions) {
            return Err(PermissionError::CannotDelegateUnheld(
                "requested permissions exceed delegation constraint".into(),
            ));
        }

        // 4. Find the delegator's best matching grant to derive depth from.
        let delegator_grants = self.direct_grants(delegator);
        let parent_grant = delegator_grants
            .iter()
            .filter(|g| g.covers_scope(&scope) && g.permissions.allows_all(&permissions))
            .max_by_key(|g| g.delegation_depth);

        let parent = parent_grant.ok_or_else(|| {
            PermissionError::CannotDelegateUnheld(
                "delegator does not hold the required permissions at this scope".into(),
            )
        })?;

        if parent.delegation_depth == 0 {
            return Err(PermissionError::DelegationDepthExceeded { max: 0 });
        }

        let new_depth = (parent.delegation_depth - 1).min(constraint.max_depth);

        // 5. Expiry: take the earliest of parent, constraint, or explicit.
        let expires_at = earliest_expiry(parent.expires_at, constraint.expires_at);

        // 6. Create the grant.
        self.grant(
            delegatee.clone(),
            permissions,
            scope,
            Some(delegator.clone()),
            new_depth,
            expires_at,
        )
    }
}

fn earliest_expiry(a: Option<SystemTime>, b: Option<SystemTime>) -> Option<SystemTime> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (Some(x), None) | (None, Some(x)) => Some(x),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::graph::{Permission, PrincipalKind};

    fn pid(s: &str) -> PrincipalId {
        PrincipalId(s.to_string())
    }

    fn setup_graph() -> PermissionGraph {
        let g = PermissionGraph::new();
        g.add_principal(pid("admin"), PrincipalKind::Agent, "Admin Agent")
            .unwrap();
        g.add_principal(pid("worker"), PrincipalKind::Agent, "Worker Agent")
            .unwrap();
        g.add_principal(pid("sub"), PrincipalKind::Agent, "Sub Agent")
            .unwrap();

        // admin gets broad permissions with delegation depth 2
        let perms = PermissionSet::new()
            .with(Permission::VmCreate)
            .with(Permission::VmStart)
            .with(Permission::VmStop)
            .with(Permission::VmRead);
        g.grant(
            pid("admin"),
            perms,
            ResourceScope::Org("acme".into()),
            None,
            2,
            None,
        )
        .unwrap();
        g
    }

    #[test]
    fn delegation_narrows_permissions() {
        let g = setup_graph();
        let constraint = DelegationConstraint {
            max_permissions: PermissionSet::new()
                .with(Permission::VmStart)
                .with(Permission::VmRead),
            max_scope: ResourceScope::Org("acme".into()),
            max_depth: 1,
            expires_at: None,
        };

        let subset = PermissionSet::new().with(Permission::VmRead);
        g.delegate(
            &pid("admin"),
            &pid("worker"),
            subset,
            ResourceScope::Org("acme".into()),
            &constraint,
        )
        .unwrap();

        let worker_grants = g.direct_grants(&pid("worker"));
        assert_eq!(worker_grants.len(), 1);
        assert!(worker_grants[0].permissions.allows(&Permission::VmRead));
        assert!(!worker_grants[0].permissions.allows(&Permission::VmCreate));
        assert_eq!(worker_grants[0].delegation_depth, 1); // 2-1 = 1
    }

    #[test]
    fn delegation_rejects_unheld_permissions() {
        let g = setup_graph();
        let constraint = DelegationConstraint {
            max_permissions: PermissionSet::new().with(Permission::GpuAttach),
            max_scope: ResourceScope::Root,
            max_depth: 1,
            expires_at: None,
        };

        let err = g
            .delegate(
                &pid("admin"),
                &pid("worker"),
                PermissionSet::new().with(Permission::GpuAttach),
                ResourceScope::Root,
                &constraint,
            )
            .unwrap_err();
        assert!(matches!(err, PermissionError::CannotDelegateUnheld(_)));
    }

    #[test]
    fn delegation_depth_exhausted() {
        let g = PermissionGraph::new();
        g.add_principal(pid("a"), PrincipalKind::Agent, "A")
            .unwrap();
        g.add_principal(pid("b"), PrincipalKind::Agent, "B")
            .unwrap();

        // depth=0 — cannot delegate
        g.grant(
            pid("a"),
            PermissionSet::new().with(Permission::VmRead),
            ResourceScope::Root,
            None,
            0,
            None,
        )
        .unwrap();

        let constraint = DelegationConstraint {
            max_permissions: PermissionSet::new().with(Permission::VmRead),
            max_scope: ResourceScope::Root,
            max_depth: 0,
            expires_at: None,
        };
        let err = g
            .delegate(
                &pid("a"),
                &pid("b"),
                PermissionSet::new().with(Permission::VmRead),
                ResourceScope::Root,
                &constraint,
            )
            .unwrap_err();
        assert!(matches!(
            err,
            PermissionError::DelegationDepthExceeded { .. }
        ));
    }

    #[test]
    fn delegation_scope_must_be_within_constraint() {
        let g = setup_graph();
        let constraint = DelegationConstraint {
            max_permissions: PermissionSet::new().with(Permission::VmRead),
            max_scope: ResourceScope::Tenant {
                org: "acme".into(),
                tenant: "prod".into(),
            },
            max_depth: 1,
            expires_at: None,
        };

        // Try to delegate at org level — wider than constraint allows.
        let err = g
            .delegate(
                &pid("admin"),
                &pid("worker"),
                PermissionSet::new().with(Permission::VmRead),
                ResourceScope::Org("acme".into()),
                &constraint,
            )
            .unwrap_err();
        assert!(matches!(err, PermissionError::InvalidScope(_)));
    }

    #[test]
    fn chained_delegation() {
        let g = setup_graph();

        // admin → worker (depth 2 → 1)
        let c1 = DelegationConstraint {
            max_permissions: PermissionSet::new()
                .with(Permission::VmStart)
                .with(Permission::VmRead),
            max_scope: ResourceScope::Org("acme".into()),
            max_depth: 1,
            expires_at: None,
        };
        g.delegate(
            &pid("admin"),
            &pid("worker"),
            PermissionSet::new()
                .with(Permission::VmStart)
                .with(Permission::VmRead),
            ResourceScope::Org("acme".into()),
            &c1,
        )
        .unwrap();

        // worker → sub (depth 1 → 0)
        let c2 = DelegationConstraint {
            max_permissions: PermissionSet::new().with(Permission::VmRead),
            max_scope: ResourceScope::Org("acme".into()),
            max_depth: 0,
            expires_at: None,
        };
        g.delegate(
            &pid("worker"),
            &pid("sub"),
            PermissionSet::new().with(Permission::VmRead),
            ResourceScope::Org("acme".into()),
            &c2,
        )
        .unwrap();

        let sub_grants = g.direct_grants(&pid("sub"));
        assert_eq!(sub_grants.len(), 1);
        assert!(sub_grants[0].permissions.allows(&Permission::VmRead));
        assert!(!sub_grants[0].permissions.allows(&Permission::VmStart)); // attenuated
        assert_eq!(sub_grants[0].delegation_depth, 0); // terminal
    }
}
