//! Audit trail for permission changes.
//!
//! Every mutation to the permission graph (grants, revocations, role
//! assignments, delegations) is recorded as a [`PermissionChange`] entry.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

use super::graph::{GrantId, PrincipalId};
use super::hierarchy::ResourceScope;

/// A single audit entry recording *what* changed and *when*.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAuditEntry {
    pub change: PermissionChange,
    pub timestamp: SystemTime,
}

/// The kinds of permission mutations we track.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionChange {
    /// A role was assigned to a principal.
    RoleAssigned {
        principal: PrincipalId,
        role: PrincipalId,
    },
    /// A permission grant was created.
    Granted {
        grant_id: GrantId,
        grantee: PrincipalId,
        scope: ResourceScope,
        granted_by: Option<PrincipalId>,
    },
    /// A permission grant was revoked.
    Revoked {
        grant_id: GrantId,
        grantee: PrincipalId,
    },
    /// A delegation was created.
    Delegated {
        grant_id: GrantId,
        from: PrincipalId,
        to: PrincipalId,
        scope: ResourceScope,
    },
}

/// Append-only log of permission changes.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditLog {
    entries: Vec<PermissionAuditEntry>,
}

impl AuditLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a change.
    pub fn record(&mut self, change: PermissionChange) {
        self.entries.push(PermissionAuditEntry {
            change,
            timestamp: SystemTime::now(),
        });
    }

    /// Total number of recorded entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True if no entries have been recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over all entries (oldest first).
    pub fn entries(&self) -> &[PermissionAuditEntry] {
        &self.entries
    }

    /// Filter entries for a specific principal.
    pub fn entries_for(&self, principal: &PrincipalId) -> Vec<&PermissionAuditEntry> {
        self.entries
            .iter()
            .filter(|e| match &e.change {
                PermissionChange::RoleAssigned { principal: p, .. } => p == principal,
                PermissionChange::Granted { grantee, .. } => grantee == principal,
                PermissionChange::Revoked { grantee, .. } => grantee == principal,
                PermissionChange::Delegated { from, to, .. } => {
                    from == principal || to == principal
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pid(s: &str) -> PrincipalId {
        PrincipalId(s.to_string())
    }

    #[test]
    fn record_and_query() {
        let mut log = AuditLog::new();
        assert!(log.is_empty());

        log.record(PermissionChange::RoleAssigned {
            principal: pid("agent-1"),
            role: pid("admin"),
        });
        log.record(PermissionChange::Granted {
            grant_id: GrantId(1),
            grantee: pid("agent-2"),
            scope: ResourceScope::Root,
            granted_by: None,
        });

        assert_eq!(log.len(), 2);
        assert_eq!(log.entries_for(&pid("agent-1")).len(), 1);
        assert_eq!(log.entries_for(&pid("agent-2")).len(), 1);
        assert_eq!(log.entries_for(&pid("nobody")).len(), 0);
    }

    #[test]
    fn entries_for_delegation() {
        let mut log = AuditLog::new();
        log.record(PermissionChange::Delegated {
            grant_id: GrantId(5),
            from: pid("alice"),
            to: pid("bob"),
            scope: ResourceScope::Org("acme".into()),
        });

        // Both the delegator and delegate see the entry
        assert_eq!(log.entries_for(&pid("alice")).len(), 1);
        assert_eq!(log.entries_for(&pid("bob")).len(), 1);
    }
}
