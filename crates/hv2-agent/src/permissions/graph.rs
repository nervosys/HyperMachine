//! Core permission graph — principals, permissions, and grant edges.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use super::audit::{AuditLog, PermissionChange};
use super::hierarchy::ResourceScope;
use super::{PermissionError, PermissionResult};

// ── Identifiers ────────────────────────────────────────────────────

/// Opaque principal identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PrincipalId(pub String);

impl std::fmt::Display for PrincipalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque grant identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GrantId(pub u64);

impl std::fmt::Display for GrantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "grant-{}", self.0)
    }
}

// ── Principals ─────────────────────────────────────────────────────

/// What kind of entity holds permissions.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrincipalKind {
    /// An AI agent (e.g. Claude, GPT, custom).
    Agent,
    /// A named role (e.g. "admin", "operator").
    Role,
    /// A service account for automated pipelines.
    Service,
    /// An organization-level identity.
    OrgIdentity,
}

/// A node in the permission graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub display_name: String,
    /// Roles this principal inherits from (role → role edges).
    pub roles: HashSet<PrincipalId>,
    pub created_at: SystemTime,
}

// ── Permissions ────────────────────────────────────────────────────

/// An individual permission token.
///
/// These are intentionally fine-grained — combinable via [`PermissionSet`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // VM lifecycle
    VmCreate,
    VmDelete,
    VmStart,
    VmStop,
    VmPause,
    VmResume,
    VmReboot,

    // VM inspection
    VmRead,
    VmList,

    // Configuration
    VmConfigure,
    VmMigrate,

    // Execution
    GuestExec,
    GuestFileRead,
    GuestFileWrite,

    // Snapshots
    SnapshotCreate,
    SnapshotRestore,
    SnapshotDelete,

    // Network
    NetworkAttach,
    NetworkDetach,
    NetworkConfigure,

    // Storage
    StorageAttach,
    StorageDetach,
    StorageResize,

    // GPU
    GpuAttach,
    GpuDetach,
    GpuConfigure,

    // Memory
    MemoryRead,
    MemoryWrite,

    // Metrics / observability
    MetricsRead,
    MetricsExport,

    // Admin
    AdminConfig,
    AdminAudit,
    AdminDelegate,

    // Agent management
    AgentRegister,
    AgentDeregister,
    AgentCoordinate,

    // Wildcard — superuser
    All,
}

impl Permission {
    /// True if `self` subsumes `other` (e.g. `All` subsumes everything).
    pub fn subsumes(&self, other: &Permission) -> bool {
        self == &Permission::All || self == other
    }
}

impl std::fmt::Display for Permission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// A set of permissions — the unit of granting/checking.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PermissionSet {
    perms: HashSet<Permission>,
}

impl PermissionSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, p: Permission) -> Self {
        self.perms.insert(p);
        self
    }

    pub fn insert(&mut self, p: Permission) {
        self.perms.insert(p);
    }

    pub fn remove(&mut self, p: &Permission) {
        self.perms.remove(p);
    }

    /// True if this set grants `p` (directly or via [`Permission::All`]).
    pub fn allows(&self, p: &Permission) -> bool {
        self.perms.iter().any(|held| held.subsumes(p))
    }

    /// True if this set grants every permission in `required`.
    pub fn allows_all(&self, required: &PermissionSet) -> bool {
        required.perms.iter().all(|p| self.allows(p))
    }

    /// Intersection — only permissions present in both sets.
    pub fn intersect(&self, other: &PermissionSet) -> PermissionSet {
        PermissionSet {
            perms: self.perms.intersection(&other.perms).cloned().collect(),
        }
    }

    /// Union of two sets.
    pub fn union(&self, other: &PermissionSet) -> PermissionSet {
        PermissionSet {
            perms: self.perms.union(&other.perms).cloned().collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.perms.is_empty()
    }

    pub fn len(&self) -> usize {
        self.perms.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Permission> {
        self.perms.iter()
    }
}

// ── Grant edges ────────────────────────────────────────────────────

/// A directed edge in the graph: "principal X is granted these permissions
/// at this scope, optionally delegated from another principal."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrantEdge {
    pub id: GrantId,
    /// The principal receiving the grant.
    pub grantee: PrincipalId,
    /// Permissions conveyed by this edge.
    pub permissions: PermissionSet,
    /// Scope at which the grant applies.
    pub scope: ResourceScope,
    /// Who made this grant (`None` = system/bootstrap).
    pub granted_by: Option<PrincipalId>,
    /// How many further delegation hops are allowed (0 = cannot delegate).
    pub delegation_depth: u32,
    /// Optional expiry.
    pub expires_at: Option<SystemTime>,
    pub created_at: SystemTime,
}

impl GrantEdge {
    /// True if this grant has expired.
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| SystemTime::now() >= exp)
            .unwrap_or(false)
    }

    /// True if this grant covers the requested scope.
    pub fn covers_scope(&self, target: &ResourceScope) -> bool {
        self.scope.contains(target)
    }
}

// ── The graph itself ───────────────────────────────────────────────

/// Thread-safe permission graph.
///
/// Nodes = principals, edges = grant edges.  Role inheritance is modeled
/// by following `principal.roles` links.
pub struct PermissionGraph {
    principals: RwLock<HashMap<PrincipalId, Principal>>,
    /// Grants indexed by grantee.
    grants_by_grantee: RwLock<HashMap<PrincipalId, Vec<GrantEdge>>>,
    next_grant_id: AtomicU64,
    audit: RwLock<AuditLog>,
}

impl PermissionGraph {
    pub fn new() -> Self {
        Self {
            principals: RwLock::new(HashMap::new()),
            grants_by_grantee: RwLock::new(HashMap::new()),
            next_grant_id: AtomicU64::new(1),
            audit: RwLock::new(AuditLog::new()),
        }
    }

    // ── Principal management ───────────────────────────────────────

    /// Register a new principal.
    pub fn add_principal(
        &self,
        id: PrincipalId,
        kind: PrincipalKind,
        display_name: impl Into<String>,
    ) -> PermissionResult<()> {
        let principal = Principal {
            id: id.clone(),
            kind,
            display_name: display_name.into(),
            roles: HashSet::new(),
            created_at: SystemTime::now(),
        };
        self.principals.write().insert(id, principal);
        Ok(())
    }

    /// Assign a role to a principal (creates an inheritance edge).
    pub fn assign_role(
        &self,
        principal_id: &PrincipalId,
        role_id: &PrincipalId,
    ) -> PermissionResult<()> {
        let mut principals = self.principals.write();

        // Verify both exist upfront with immutable borrows.
        if !principals.contains_key(principal_id) {
            return Err(PermissionError::PrincipalNotFound(principal_id.to_string()));
        }
        let role_kind = principals
            .get(role_id)
            .map(|r| r.kind.clone())
            .ok_or_else(|| PermissionError::PrincipalNotFound(role_id.to_string()))?;
        if role_kind != PrincipalKind::Role {
            return Err(PermissionError::InvalidScope(format!(
                "{role_id} is not a role"
            )));
        }

        // Cycle check: the role must not (transitively) already inherit from principal_id.
        if self.would_create_cycle_inner(&principals, role_id, principal_id) {
            return Err(PermissionError::CycleDetected);
        }

        // Now mutate.
        principals
            .get_mut(principal_id)
            .unwrap()
            .roles
            .insert(role_id.clone());

        self.audit.write().record(PermissionChange::RoleAssigned {
            principal: principal_id.clone(),
            role: role_id.clone(),
        });

        Ok(())
    }

    /// Check whether making `start` inherit from `target` would create a cycle.
    fn would_create_cycle_inner(
        &self,
        principals: &HashMap<PrincipalId, Principal>,
        start: &PrincipalId,
        target: &PrincipalId,
    ) -> bool {
        if start == target {
            return true;
        }
        let Some(p) = principals.get(start) else {
            return false;
        };
        for r in &p.roles {
            if self.would_create_cycle_inner(principals, r, target) {
                return true;
            }
        }
        false
    }

    // ── Grant management ───────────────────────────────────────────

    /// Create a direct permission grant.
    pub fn grant(
        &self,
        grantee: PrincipalId,
        permissions: PermissionSet,
        scope: ResourceScope,
        granted_by: Option<PrincipalId>,
        delegation_depth: u32,
        expires_at: Option<SystemTime>,
    ) -> PermissionResult<GrantId> {
        // Verify grantee exists.
        if !self.principals.read().contains_key(&grantee) {
            return Err(PermissionError::PrincipalNotFound(grantee.to_string()));
        }

        let id = GrantId(self.next_grant_id.fetch_add(1, Ordering::Relaxed));
        let edge = GrantEdge {
            id: id.clone(),
            grantee: grantee.clone(),
            permissions: permissions.clone(),
            scope: scope.clone(),
            granted_by: granted_by.clone(),
            delegation_depth,
            expires_at,
            created_at: SystemTime::now(),
        };

        self.grants_by_grantee
            .write()
            .entry(grantee.clone())
            .or_default()
            .push(edge);

        self.audit.write().record(PermissionChange::Granted {
            grant_id: id.clone(),
            grantee,
            scope,
            granted_by,
        });

        Ok(id)
    }

    /// Revoke a grant by id.
    pub fn revoke(&self, grant_id: &GrantId) -> PermissionResult<()> {
        let mut grants = self.grants_by_grantee.write();
        for edges in grants.values_mut() {
            if let Some(pos) = edges.iter().position(|e| &e.id == grant_id) {
                let removed = edges.remove(pos);
                self.audit.write().record(PermissionChange::Revoked {
                    grant_id: grant_id.clone(),
                    grantee: removed.grantee,
                });
                return Ok(());
            }
        }
        Err(PermissionError::GrantNotFound(grant_id.to_string()))
    }

    // ── Queries ────────────────────────────────────────────────────

    /// Get all non-expired grants for a single principal (no inheritance walk).
    pub fn direct_grants(&self, principal: &PrincipalId) -> Vec<GrantEdge> {
        self.grants_by_grantee
            .read()
            .get(principal)
            .map(|edges| edges.iter().filter(|e| !e.is_expired()).cloned().collect())
            .unwrap_or_default()
    }

    /// Collect all principal IDs reachable via role inheritance from `start`.
    pub fn inherited_roles(&self, start: &PrincipalId) -> Vec<PrincipalId> {
        let principals = self.principals.read();
        let mut visited = HashSet::new();
        let mut stack = vec![start.clone()];
        let mut result = Vec::new();

        while let Some(current) = stack.pop() {
            if !visited.insert(current.clone()) {
                continue;
            }
            if let Some(p) = principals.get(&current) {
                for role in &p.roles {
                    result.push(role.clone());
                    stack.push(role.clone());
                }
            }
        }
        result
    }

    /// Get a principal by id.
    pub fn get_principal(&self, id: &PrincipalId) -> Option<Principal> {
        self.principals.read().get(id).cloned()
    }

    /// All registered principal ids.
    pub fn principal_ids(&self) -> Vec<PrincipalId> {
        self.principals.read().keys().cloned().collect()
    }

    /// Read-only view of the audit log.
    pub fn audit_log(&self) -> AuditLog {
        self.audit.read().clone()
    }
}

impl Default for PermissionGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(s: &str) -> PrincipalId {
        PrincipalId(s.to_string())
    }

    #[test]
    fn add_principal_and_grant() {
        let g = PermissionGraph::new();
        g.add_principal(pid("a1"), PrincipalKind::Agent, "Agent 1")
            .unwrap();
        let perms = PermissionSet::new()
            .with(Permission::VmRead)
            .with(Permission::VmList);
        let grant_id = g
            .grant(pid("a1"), perms, ResourceScope::Root, None, 0, None)
            .unwrap();
        let grants = g.direct_grants(&pid("a1"));
        assert_eq!(grants.len(), 1);
        assert!(grants[0].permissions.allows(&Permission::VmRead));

        g.revoke(&grant_id).unwrap();
        assert!(g.direct_grants(&pid("a1")).is_empty());
    }

    #[test]
    fn role_inheritance() {
        let g = PermissionGraph::new();
        g.add_principal(pid("admin"), PrincipalKind::Role, "Admin")
            .unwrap();
        g.add_principal(pid("operator"), PrincipalKind::Role, "Operator")
            .unwrap();
        g.add_principal(pid("a1"), PrincipalKind::Agent, "Agent 1")
            .unwrap();

        g.assign_role(&pid("operator"), &pid("admin")).unwrap(); // operator inherits admin
        g.assign_role(&pid("a1"), &pid("operator")).unwrap(); // agent inherits operator

        let roles = g.inherited_roles(&pid("a1"));
        assert!(roles.contains(&pid("operator")));
        assert!(roles.contains(&pid("admin"))); // transitive
    }

    #[test]
    fn cycle_detection() {
        let g = PermissionGraph::new();
        g.add_principal(pid("r1"), PrincipalKind::Role, "R1")
            .unwrap();
        g.add_principal(pid("r2"), PrincipalKind::Role, "R2")
            .unwrap();
        g.assign_role(&pid("r1"), &pid("r2")).unwrap();
        let err = g.assign_role(&pid("r2"), &pid("r1")).unwrap_err();
        assert_eq!(err, PermissionError::CycleDetected);
    }

    #[test]
    fn grant_to_missing_principal_fails() {
        let g = PermissionGraph::new();
        let err = g
            .grant(
                pid("ghost"),
                PermissionSet::new(),
                ResourceScope::Root,
                None,
                0,
                None,
            )
            .unwrap_err();
        assert!(matches!(err, PermissionError::PrincipalNotFound(_)));
    }
}
