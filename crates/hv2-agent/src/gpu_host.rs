//! The seam between the MCP `gpu.*` tools and a real GPU fabric.
//!
//! This is the same shape as [`VmHost`](crate::vm_host::VmHost), for the same
//! reason: [`McpServer`](crate::mcp::McpServer) dispatches `gpu.*` against a
//! [`GpuHost`] when one is installed, and keeps its own session bookkeeping
//! when one is not.
//!
//! The trait lives here rather than in `hv2-runtime` deliberately.
//! `hv2-runtime` depends on `hv2-agent`, so the agent crate cannot import the
//! runtime's `GpuTopologyMap` without a dependency cycle. Inverting it — the
//! interface in the crate that *calls* it, the implementation in the crate that
//! *owns* the fabric — lets the real topology back these tools with no cycle at
//! all. `hv2-runtime`'s `AgentGpuHost` is that implementation.

use async_trait::async_trait;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A GPU as the tool surface reports it to an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuDescriptor {
    /// Unique device identifier.
    pub device_id: String,
    /// Model name, e.g. `H100`.
    pub model: String,
    /// VRAM in gigabytes.
    pub vram_gb: u64,
    /// Compute capability, e.g. `90` for sm_90.
    pub compute_capability: u32,
    /// The VM this device is attached to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocated_to: Option<String>,
}

impl GpuDescriptor {
    /// Whether this device is free to attach.
    pub fn is_available(&self) -> bool {
        self.allocated_to.is_none()
    }
}

/// A GPU inventory an agent can enumerate and attach devices from.
///
/// Implementations own allocation exclusivity: [`Self::attach`] must reject a
/// device that is already attached elsewhere, because that is the invariant an
/// agent cannot check for itself without racing.
#[async_trait]
pub trait GpuHost: Send + Sync {
    /// Add a device to the inventory.
    async fn register(&self, device: GpuDescriptor) -> Result<GpuDescriptor, String>;

    /// Every device the host knows about.
    async fn list(&self) -> Result<Vec<GpuDescriptor>, String>;

    /// Attach a device to a VM, failing if it is already attached.
    async fn attach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String>;

    /// Detach a device from a VM, failing if it is not attached to that VM.
    async fn detach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String>;
}

// ═══════════════════════════════════════════════════════════════════
//  In-memory host
// ═══════════════════════════════════════════════════════════════════

/// A [`GpuHost`] holding its inventory in memory.
///
/// Useful on its own for tests and for a single-node deployment with no fabric
/// manager. It enforces the same allocation exclusivity a real fabric does, so
/// code written against it behaves the same way against
/// `hv2-runtime`'s topology-backed host.
#[derive(Default)]
pub struct InMemoryGpuHost {
    devices: RwLock<HashMap<String, GpuDescriptor>>,
}

impl InMemoryGpuHost {
    /// A host with no devices.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of devices in the inventory.
    pub fn device_count(&self) -> usize {
        self.devices.read().len()
    }
}

#[async_trait]
impl GpuHost for InMemoryGpuHost {
    async fn register(&self, device: GpuDescriptor) -> Result<GpuDescriptor, String> {
        let mut devices = self.devices.write();
        if devices.contains_key(&device.device_id) {
            return Err(format!(
                "GPU device already registered: {}",
                device.device_id
            ));
        }
        devices.insert(device.device_id.clone(), device.clone());
        Ok(device)
    }

    async fn list(&self) -> Result<Vec<GpuDescriptor>, String> {
        Ok(self.devices.read().values().cloned().collect())
    }

    async fn attach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String> {
        let mut devices = self.devices.write();
        let device = devices
            .get_mut(device_id)
            .ok_or_else(|| format!("GPU device not found: {device_id}"))?;

        if let Some(owner) = &device.allocated_to {
            // Idempotent re-attach to the same VM is fine; stealing is not.
            if owner != vm_id {
                return Err(format!("GPU {device_id} already allocated to {owner}"));
            }
        }

        device.allocated_to = Some(vm_id.to_string());
        Ok(device.clone())
    }

    async fn detach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String> {
        let mut devices = self.devices.write();
        let device = devices
            .get_mut(device_id)
            .ok_or_else(|| format!("GPU device not found: {device_id}"))?;

        if device.allocated_to.as_deref() != Some(vm_id) {
            return Err(format!("GPU {device_id} is not attached to VM {vm_id}"));
        }

        device.allocated_to = None;
        Ok(device.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str) -> GpuDescriptor {
        GpuDescriptor {
            device_id: id.to_string(),
            model: "H100".to_string(),
            vram_gb: 80,
            compute_capability: 90,
            allocated_to: None,
        }
    }

    #[tokio::test]
    async fn a_registered_device_is_listed_and_available() {
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();

        let listed = host.list().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].is_available());
    }

    #[tokio::test]
    async fn registering_the_same_device_twice_is_rejected() {
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();

        let err = host.register(device("gpu-0")).await.unwrap_err();
        assert!(err.contains("already registered"), "got: {err}");
        assert_eq!(host.device_count(), 1);
    }

    #[tokio::test]
    async fn a_device_cannot_be_attached_to_two_vms() {
        // This is the invariant an agent cannot enforce for itself.
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();
        host.attach("vm-a", "gpu-0").await.unwrap();

        let err = host.attach("vm-b", "gpu-0").await.unwrap_err();
        assert!(err.contains("already allocated"), "got: {err}");
        assert_eq!(
            host.list().await.unwrap()[0].allocated_to.as_deref(),
            Some("vm-a"),
            "the original owner must keep the device"
        );
    }

    #[tokio::test]
    async fn reattaching_to_the_same_vm_is_idempotent() {
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();
        host.attach("vm-a", "gpu-0").await.unwrap();

        assert!(host.attach("vm-a", "gpu-0").await.is_ok());
    }

    #[tokio::test]
    async fn detaching_frees_the_device_for_another_vm() {
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();
        host.attach("vm-a", "gpu-0").await.unwrap();
        host.detach("vm-a", "gpu-0").await.unwrap();

        assert!(host.list().await.unwrap()[0].is_available());
        assert!(host.attach("vm-b", "gpu-0").await.is_ok());
    }

    #[tokio::test]
    async fn a_vm_cannot_detach_a_device_it_does_not_hold() {
        let host = InMemoryGpuHost::new();
        host.register(device("gpu-0")).await.unwrap();
        host.attach("vm-a", "gpu-0").await.unwrap();

        let err = host.detach("vm-b", "gpu-0").await.unwrap_err();
        assert!(err.contains("not attached"), "got: {err}");
        assert_eq!(
            host.list().await.unwrap()[0].allocated_to.as_deref(),
            Some("vm-a")
        );
    }

    #[tokio::test]
    async fn operations_on_an_unknown_device_say_so() {
        let host = InMemoryGpuHost::new();
        assert!(host
            .attach("vm-a", "nope")
            .await
            .unwrap_err()
            .contains("not found"));
        assert!(host
            .detach("vm-a", "nope")
            .await
            .unwrap_err()
            .contains("not found"));
    }
}
