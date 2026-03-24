//! Distributed Graph-Based Hierarchical Permissions
//!
//! This module implements a DAG-structured permission system for agentic AI
//! systems, supporting:
//!
//! - **Resource hierarchy**: Org → Tenant → Project → VM scoping with inheritance
//! - **Permission graph**: Principals (agents, roles, services) connected by
//!   typed edges that carry scoped permission grants
//! - **Delegation**: Agent A can grant a subset of its own permissions to Agent B,
//!   with configurable depth limits and automatic attenuation
//! - **Resolution**: Walk the graph to compute the effective permission set for
//!   any principal at any resource scope
//! - **Audit trail**: Every grant, revocation, and delegation is logged
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────┐
//! │                  PermissionGraph                     │
//! │                                                     │
//! │  Principals (nodes)          Grants (edges)         │
//! │  ┌──────────┐               ┌──────────────┐       │
//! │  │ Agent A  │──has_role────▶│ admin@org:acme│       │
//! │  │ (Agent)  │               └──────────────┘       │
//! │  └──────────┘                                      │
//! │  ┌──────────┐  delegates   ┌──────────────┐        │
//! │  │ Agent B  │◀─(subset)───│ Agent A      │        │
//! │  │ (Agent)  │              └──────────────┘        │
//! │  └──────────┘                                      │
//! │  ┌──────────┐  inherits    ┌──────────────┐        │
//! │  │ operator │◀────────────│ admin        │        │
//! │  │ (Role)   │              │ (Role)       │        │
//! │  └──────────┘              └──────────────┘        │
//! │                                                     │
//! │  ResourceScope (hierarchy)                          │
//! │  root ─▶ org:acme ─▶ tenant:prod ─▶ project:ml     │
//! │                                    ─▶ vm:gpu-001    │
//! └─────────────────────────────────────────────────────┘
//! ```

mod audit;
mod delegation;
mod graph;
mod hierarchy;
mod resolution;

pub use audit::{AuditLog, PermissionAuditEntry, PermissionChange};
pub use delegation::{Delegation, DelegationChain, DelegationConstraint};
pub use graph::{
    GrantEdge, GrantId, Permission, PermissionGraph, PermissionSet, Principal, PrincipalId,
    PrincipalKind,
};
pub use hierarchy::ResourceScope;
pub use resolution::{EffectivePermissions, ResolutionEngine};

use thiserror::Error;

/// Errors from the permission system.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PermissionError {
    #[error("principal not found: {0}")]
    PrincipalNotFound(String),

    #[error("permission denied: {action} on {resource}")]
    Denied { action: String, resource: String },

    #[error("delegation depth exceeded (max {max})")]
    DelegationDepthExceeded { max: u32 },

    #[error("cannot delegate permission not held: {0}")]
    CannotDelegateUnheld(String),

    #[error("cycle detected in permission graph")]
    CycleDetected,

    #[error("grant not found: {0}")]
    GrantNotFound(String),

    #[error("invalid scope: {0}")]
    InvalidScope(String),
}

pub type PermissionResult<T> = Result<T, PermissionError>;
