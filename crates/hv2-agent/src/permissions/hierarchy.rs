//! Resource scope hierarchy
//!
//! Models a tree of scopes: `Root → Org → Tenant → Project → Vm`.
//! Permissions granted at a higher scope automatically apply to all
//! descendant scopes unless an explicit deny overrides them.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A position in the resource hierarchy.
///
/// Scopes form a tree — a grant at `Org("acme")` covers every tenant,
/// project, and VM beneath that org.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResourceScope {
    /// Global root — covers everything.
    Root,
    /// Organization level.
    Org(String),
    /// Tenant within an org.
    Tenant { org: String, tenant: String },
    /// Project within a tenant.
    Project {
        org: String,
        tenant: String,
        project: String,
    },
    /// Individual VM.
    Vm {
        org: String,
        tenant: String,
        project: String,
        vm: String,
    },
}

impl ResourceScope {
    /// Returns the immediate parent scope, or `None` for `Root`.
    pub fn parent(&self) -> Option<ResourceScope> {
        match self {
            ResourceScope::Root => None,
            ResourceScope::Org(_) => Some(ResourceScope::Root),
            ResourceScope::Tenant { org, .. } => Some(ResourceScope::Org(org.clone())),
            ResourceScope::Project { org, tenant, .. } => Some(ResourceScope::Tenant {
                org: org.clone(),
                tenant: tenant.clone(),
            }),
            ResourceScope::Vm {
                org,
                tenant,
                project,
                ..
            } => Some(ResourceScope::Project {
                org: org.clone(),
                tenant: tenant.clone(),
                project: project.clone(),
            }),
        }
    }

    /// Depth in the hierarchy (Root = 0, Org = 1, … Vm = 4).
    pub fn depth(&self) -> u32 {
        match self {
            ResourceScope::Root => 0,
            ResourceScope::Org(_) => 1,
            ResourceScope::Tenant { .. } => 2,
            ResourceScope::Project { .. } => 3,
            ResourceScope::Vm { .. } => 4,
        }
    }

    /// True when `self` is an ancestor-or-equal of `other`.
    pub fn contains(&self, other: &ResourceScope) -> bool {
        if self == other {
            return true;
        }
        let mut cursor = other.clone();
        while let Some(p) = cursor.parent() {
            if &p == self {
                return true;
            }
            cursor = p;
        }
        false
    }

    /// Walk up the ancestry chain (self included) from leaf to root.
    pub fn ancestors(&self) -> Vec<ResourceScope> {
        let mut chain = vec![self.clone()];
        let mut cursor = self.clone();
        while let Some(p) = cursor.parent() {
            chain.push(p.clone());
            cursor = p;
        }
        chain
    }

    /// Canonical string representation (`/org/tenant/project/vm`).
    pub fn path(&self) -> String {
        match self {
            ResourceScope::Root => "/".to_string(),
            ResourceScope::Org(o) => format!("/{o}"),
            ResourceScope::Tenant { org, tenant } => format!("/{org}/{tenant}"),
            ResourceScope::Project {
                org,
                tenant,
                project,
            } => format!("/{org}/{tenant}/{project}"),
            ResourceScope::Vm {
                org,
                tenant,
                project,
                vm,
            } => format!("/{org}/{tenant}/{project}/{vm}"),
        }
    }
}

impl fmt::Display for ResourceScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_contains_everything() {
        let root = ResourceScope::Root;
        let org = ResourceScope::Org("acme".into());
        let vm = ResourceScope::Vm {
            org: "acme".into(),
            tenant: "prod".into(),
            project: "ml".into(),
            vm: "gpu-001".into(),
        };
        assert!(root.contains(&org));
        assert!(root.contains(&vm));
        assert!(!org.contains(&root));
    }

    #[test]
    fn parent_chain() {
        let vm = ResourceScope::Vm {
            org: "acme".into(),
            tenant: "prod".into(),
            project: "ml".into(),
            vm: "gpu-001".into(),
        };
        let ancestors = vm.ancestors();
        assert_eq!(ancestors.len(), 5);
        assert_eq!(ancestors[0], vm);
        assert_eq!(ancestors[4], ResourceScope::Root);
    }

    #[test]
    fn scope_path_format() {
        let scope = ResourceScope::Project {
            org: "acme".into(),
            tenant: "prod".into(),
            project: "ml".into(),
        };
        assert_eq!(scope.path(), "/acme/prod/ml");
    }

    #[test]
    fn sibling_scopes_do_not_contain() {
        let t1 = ResourceScope::Tenant {
            org: "acme".into(),
            tenant: "prod".into(),
        };
        let t2 = ResourceScope::Tenant {
            org: "acme".into(),
            tenant: "dev".into(),
        };
        assert!(!t1.contains(&t2));
        assert!(!t2.contains(&t1));
    }
}
