//! Permission resolution — compute effective permissions for a principal.
//!
//! The engine walks:
//! 1. Direct grants on the principal.
//! 2. Grants inherited through role edges (transitive).
//! 3. Scope hierarchy — a grant at org level applies to all VMs within.
//!
//! The result is an [`EffectivePermissions`] snapshot.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::graph::{PermissionGraph, PermissionSet, PrincipalId};
use super::hierarchy::ResourceScope;
use super::{PermissionError, PermissionResult};

/// The resolved permission set for a principal at a given scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectivePermissions {
    pub principal: PrincipalId,
    pub scope: ResourceScope,
    pub permissions: PermissionSet,
    /// Which principal IDs contributed (for audit / explainability).
    pub contributors: Vec<PrincipalId>,
}

impl EffectivePermissions {
    /// Convenience: does this effective set allow `perm`?
    pub fn allows(&self, perm: &super::graph::Permission) -> bool {
        self.permissions.allows(perm)
    }
}

/// Stateless resolution engine — takes a reference to the graph.
pub struct ResolutionEngine;

impl ResolutionEngine {
    /// Compute the full effective permission set for `principal` at `scope`.
    ///
    /// Algorithm:
    /// 1. Collect the principal's own direct grants.
    /// 2. Walk role inheritance to collect inherited grants.
    /// 3. For each grant, check if its scope covers the target scope.
    /// 4. Union all matching, non-expired permission sets.
    pub fn resolve(
        graph: &PermissionGraph,
        principal: &PrincipalId,
        scope: &ResourceScope,
    ) -> PermissionResult<EffectivePermissions> {
        if graph.get_principal(principal).is_none() {
            return Err(PermissionError::PrincipalNotFound(principal.to_string()));
        }

        let mut effective = PermissionSet::new();
        let mut contributors = Vec::new();

        // Collect all principal IDs to check: self + inherited roles.
        let mut to_check = vec![principal.clone()];
        to_check.extend(graph.inherited_roles(principal));

        let mut seen = HashSet::new();
        for pid in &to_check {
            if !seen.insert(pid.clone()) {
                continue;
            }
            let grants = graph.direct_grants(pid);
            for grant in &grants {
                if grant.covers_scope(scope) {
                    effective = effective.union(&grant.permissions);
                    if !contributors.contains(pid) {
                        contributors.push(pid.clone());
                    }
                }
            }
        }

        Ok(EffectivePermissions {
            principal: principal.clone(),
            scope: scope.clone(),
            permissions: effective,
            contributors,
        })
    }

    /// Quick check: does `principal` hold `permission` at `scope`?
    pub fn check(
        graph: &PermissionGraph,
        principal: &PrincipalId,
        permission: &super::graph::Permission,
        scope: &ResourceScope,
    ) -> PermissionResult<bool> {
        let eff = Self::resolve(graph, principal, scope)?;
        Ok(eff.allows(permission))
    }

    /// Enforce: like `check`, but returns `PermissionError::Denied` on failure.
    pub fn enforce(
        graph: &PermissionGraph,
        principal: &PrincipalId,
        permission: &super::graph::Permission,
        scope: &ResourceScope,
    ) -> PermissionResult<()> {
        if Self::check(graph, principal, permission, scope)? {
            Ok(())
        } else {
            Err(PermissionError::Denied {
                action: permission.to_string(),
                resource: scope.to_string(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::graph::{Permission, PrincipalKind};

    fn pid(s: &str) -> PrincipalId {
        PrincipalId(s.to_string())
    }

    fn build_graph() -> PermissionGraph {
        let g = PermissionGraph::new();

        // Roles
        g.add_principal(pid("admin"), PrincipalKind::Role, "Admin")
            .unwrap();
        g.add_principal(pid("operator"), PrincipalKind::Role, "Operator")
            .unwrap();

        // Admin gets All at root
        g.grant(
            pid("admin"),
            PermissionSet::new().with(Permission::All),
            ResourceScope::Root,
            None,
            3,
            None,
        )
        .unwrap();

        // Operator gets VM ops at org level
        g.grant(
            pid("operator"),
            PermissionSet::new()
                .with(Permission::VmStart)
                .with(Permission::VmStop)
                .with(Permission::VmRead)
                .with(Permission::VmList),
            ResourceScope::Org("acme".into()),
            None,
            1,
            None,
        )
        .unwrap();

        // operator inherits from admin? No — admin is higher.
        // Instead, operator is standalone.
        // agent-1 gets operator role
        g.add_principal(pid("agent-1"), PrincipalKind::Agent, "Agent 1")
            .unwrap();
        g.assign_role(&pid("agent-1"), &pid("operator")).unwrap();

        // agent-2 gets admin role
        g.add_principal(pid("agent-2"), PrincipalKind::Agent, "Agent 2")
            .unwrap();
        g.assign_role(&pid("agent-2"), &pid("admin")).unwrap();

        // agent-3 has a direct grant at a narrow scope
        g.add_principal(pid("agent-3"), PrincipalKind::Agent, "Agent 3")
            .unwrap();
        g.grant(
            pid("agent-3"),
            PermissionSet::new().with(Permission::MetricsRead),
            ResourceScope::Vm {
                org: "acme".into(),
                tenant: "prod".into(),
                project: "ml".into(),
                vm: "gpu-001".into(),
            },
            None,
            0,
            None,
        )
        .unwrap();

        g
    }

    #[test]
    fn resolve_inherited_permissions() {
        let g = build_graph();

        // agent-1 inherits operator's VmStart at org:acme
        let eff =
            ResolutionEngine::resolve(&g, &pid("agent-1"), &ResourceScope::Org("acme".into()))
                .unwrap();
        assert!(eff.allows(&Permission::VmStart));
        assert!(eff.allows(&Permission::VmRead));
        assert!(!eff.allows(&Permission::GpuAttach));
    }

    #[test]
    fn scope_inheritance_applies_to_children() {
        let g = build_graph();

        // operator grant is at org:acme — should apply to a VM under acme
        let eff = ResolutionEngine::resolve(
            &g,
            &pid("agent-1"),
            &ResourceScope::Vm {
                org: "acme".into(),
                tenant: "prod".into(),
                project: "ml".into(),
                vm: "test-vm".into(),
            },
        )
        .unwrap();
        assert!(eff.allows(&Permission::VmStart));
    }

    #[test]
    fn scope_does_not_leak_across_orgs() {
        let g = build_graph();

        // operator grant is at org:acme — should NOT apply to org:other
        let eff =
            ResolutionEngine::resolve(&g, &pid("agent-1"), &ResourceScope::Org("other".into()))
                .unwrap();
        assert!(!eff.allows(&Permission::VmStart));
    }

    #[test]
    fn admin_all_covers_everything() {
        let g = build_graph();

        let eff = ResolutionEngine::resolve(
            &g,
            &pid("agent-2"),
            &ResourceScope::Vm {
                org: "x".into(),
                tenant: "y".into(),
                project: "z".into(),
                vm: "any".into(),
            },
        )
        .unwrap();
        assert!(eff.allows(&Permission::GpuAttach));
        assert!(eff.allows(&Permission::AdminConfig));
        assert!(eff.allows(&Permission::VmDelete));
    }

    #[test]
    fn narrow_scope_grant() {
        let g = build_graph();
        let target_vm = ResourceScope::Vm {
            org: "acme".into(),
            tenant: "prod".into(),
            project: "ml".into(),
            vm: "gpu-001".into(),
        };

        // agent-3 can read metrics on the specific VM
        assert!(
            ResolutionEngine::check(&g, &pid("agent-3"), &Permission::MetricsRead, &target_vm)
                .unwrap()
        );

        // but not on a sibling VM
        let other_vm = ResourceScope::Vm {
            org: "acme".into(),
            tenant: "prod".into(),
            project: "ml".into(),
            vm: "gpu-002".into(),
        };
        assert!(
            !ResolutionEngine::check(&g, &pid("agent-3"), &Permission::MetricsRead, &other_vm)
                .unwrap()
        );
    }

    #[test]
    fn enforce_produces_denied_error() {
        let g = build_graph();
        let err = ResolutionEngine::enforce(
            &g,
            &pid("agent-3"),
            &Permission::VmDelete,
            &ResourceScope::Root,
        )
        .unwrap_err();
        assert!(matches!(err, PermissionError::Denied { .. }));
    }

    #[test]
    fn contributors_are_tracked() {
        let g = build_graph();
        let eff =
            ResolutionEngine::resolve(&g, &pid("agent-1"), &ResourceScope::Org("acme".into()))
                .unwrap();
        // agent-1 has no direct grants, permissions come from "operator" role
        assert!(eff.contributors.contains(&pid("operator")));
    }
}
