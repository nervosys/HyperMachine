//! Capability-based security model

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Individual capability
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Capability {
    /// Start/stop VM
    VmControl,

    /// Read VM state
    VmRead,

    /// Modify VM configuration
    VmModify,

    /// Access guest memory
    MemoryAccess,

    /// Network operations
    Network,

    /// GPU operations
    Gpu,

    /// Device management
    DeviceControl,

    /// Execute commands in guest
    GuestExec,

    /// Snapshot/restore
    Snapshot,

    /// Metrics and monitoring
    Metrics,
}

/// Set of capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    capabilities: HashSet<Capability>,
}

impl CapabilitySet {
    /// Create an empty capability set
    pub fn new() -> Self {
        Self {
            capabilities: HashSet::new(),
        }
    }

    /// Create a capability set with all capabilities
    pub fn all() -> Self {
        let mut set = Self::new();
        set.add(Capability::VmControl);
        set.add(Capability::VmRead);
        set.add(Capability::VmModify);
        set.add(Capability::MemoryAccess);
        set.add(Capability::Network);
        set.add(Capability::Gpu);
        set.add(Capability::DeviceControl);
        set.add(Capability::GuestExec);
        set.add(Capability::Snapshot);
        set.add(Capability::Metrics);
        set
    }

    /// Create a read-only capability set
    pub fn readonly() -> Self {
        let mut set = Self::new();
        set.add(Capability::VmRead);
        set.add(Capability::Metrics);
        set
    }

    /// Add a capability
    pub fn add(&mut self, cap: Capability) {
        self.capabilities.insert(cap);
    }

    /// Remove a capability
    pub fn remove(&mut self, cap: Capability) {
        self.capabilities.remove(&cap);
    }

    /// Check if a capability is present
    pub fn has(&self, cap: Capability) -> bool {
        self.capabilities.contains(&cap)
    }

    /// Check if all capabilities are present
    pub fn has_all(&self, caps: &[Capability]) -> bool {
        caps.iter().all(|c| self.has(*c))
    }

    /// Check if any capability is present
    pub fn has_any(&self, caps: &[Capability]) -> bool {
        caps.iter().any(|c| self.has(*c))
    }
}

impl Default for CapabilitySet {
    fn default() -> Self {
        // Default: safe capabilities only
        let mut set = Self::new();
        set.add(Capability::VmRead);
        set.add(Capability::Metrics);
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_set() {
        let mut caps = CapabilitySet::new();
        caps.add(Capability::VmRead);
        caps.add(Capability::VmControl);

        assert!(caps.has(Capability::VmRead));
        assert!(caps.has(Capability::VmControl));
        assert!(!caps.has(Capability::Network));
    }

    #[test]
    fn test_all_capabilities() {
        let caps = CapabilitySet::all();
        assert!(caps.has(Capability::VmControl));
        assert!(caps.has(Capability::Network));
        assert!(caps.has(Capability::Gpu));
    }

    #[test]
    fn test_readonly_capabilities() {
        let caps = CapabilitySet::readonly();
        assert!(caps.has(Capability::VmRead));
        assert!(caps.has(Capability::Metrics));
        assert!(!caps.has(Capability::VmControl));
    }
}
