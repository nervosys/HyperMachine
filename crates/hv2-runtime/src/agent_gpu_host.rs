//! The real GPU fabric, behind the agent tool surface.
//!
//! [`AgentGpuHost`] implements `hv2-agent`'s [`GpuHost`] over this crate's
//! [`GpuTopologyMap`], so an agent's `gpu.attach` claims an actual device out
//! of the fleet inventory rather than a JSON record in its own session.
//!
//! This is the half of the seam that could not live in `hv2-agent`:
//! `hv2-runtime` depends on `hv2-agent`, so the topology types are not visible
//! there. Putting the trait in the calling crate and the implementation here
//! resolves that without a dependency cycle.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use hv2_agent::mcp::McpServer;
//! use hv2_runtime::{AgentGpuHost, GpuDevice, GpuTopologyMap};
//!
//! let mut topology = GpuTopologyMap::new();
//! topology.add_device(GpuDevice::new("gpu-0", "node-1", "H100"));
//!
//! let server = McpServer::new();
//! server.set_gpu_host(Arc::new(AgentGpuHost::new(topology)));
//! // `gpu.list` now enumerates the real fleet.
//! ```

use std::collections::HashMap;

use async_trait::async_trait;
use hv2_agent::gpu_host::{GpuDescriptor, GpuHost};
use parking_lot::RwLock;

use crate::topology::{GpuDevice, GpuTopologyMap};

/// Bytes per gigabyte, for translating between the fabric's byte-precise VRAM
/// and the gigabytes the tool surface speaks.
const BYTES_PER_GB: u64 = 1024 * 1024 * 1024;

/// A [`GpuHost`] backed by a live [`GpuTopologyMap`].
pub struct AgentGpuHost {
    topology: RwLock<GpuTopologyMap>,
    /// Which VM holds each allocated device.
    ///
    /// The topology tracks *that* a device is allocated, which is what
    /// placement needs; the tool surface additionally has to answer *to whom*,
    /// so an agent can see its own attachments and a detach can be checked
    /// against the actual holder.
    owners: RwLock<HashMap<String, String>>,
}

impl AgentGpuHost {
    /// Serve agent tool calls from `topology`.
    pub fn new(topology: GpuTopologyMap) -> Self {
        Self {
            topology: RwLock::new(topology),
            owners: RwLock::new(HashMap::new()),
        }
    }

    /// A host with an empty inventory.
    pub fn empty() -> Self {
        Self::new(GpuTopologyMap::new())
    }

    /// Read-only access to the underlying topology, for placement and metrics.
    pub fn with_topology<T>(&self, f: impl FnOnce(&GpuTopologyMap) -> T) -> T {
        f(&self.topology.read())
    }

    fn describe(device: &GpuDevice, owners: &HashMap<String, String>) -> GpuDescriptor {
        GpuDescriptor {
            device_id: device.id.clone(),
            model: device.model.clone(),
            vram_gb: device.vram_bytes / BYTES_PER_GB,
            compute_capability: device.compute_capability,
            allocated_to: owners.get(&device.id).cloned(),
        }
    }
}

impl Default for AgentGpuHost {
    fn default() -> Self {
        Self::empty()
    }
}

#[async_trait]
impl GpuHost for AgentGpuHost {
    async fn register(&self, device: GpuDescriptor) -> Result<GpuDescriptor, String> {
        let mut topology = self.topology.write();

        if topology.contains_device(&device.device_id) {
            return Err(format!(
                "GPU device already registered: {}",
                device.device_id
            ));
        }

        // An agent registering a device does not know its host or NUMA
        // placement; those come from fleet discovery. Record what it does know.
        topology.add_device(
            GpuDevice::new(&device.device_id, "agent-registered", &device.model)
                .vram(device.vram_gb * BYTES_PER_GB)
                .capability(device.compute_capability),
        );

        Ok(GpuDescriptor {
            allocated_to: None,
            ..device
        })
    }

    async fn list(&self) -> Result<Vec<GpuDescriptor>, String> {
        let topology = self.topology.read();
        let owners = self.owners.read();

        Ok(topology
            .devices()
            .into_iter()
            .map(|device| Self::describe(device, &owners))
            .collect())
    }

    async fn attach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String> {
        // Both locks, in a fixed order, for the whole check-then-claim: an
        // exclusivity check that releases before claiming is not exclusive.
        let mut topology = self.topology.write();
        let mut owners = self.owners.write();

        if !topology.contains_device(device_id) {
            return Err(format!("GPU device not found: {device_id}"));
        }

        // Re-attaching to the same VM is idempotent; stealing is not.
        if let Some(owner) = owners.get(device_id) {
            if owner != vm_id {
                return Err(format!("GPU {device_id} already allocated to {owner}"));
            }
        }

        // Mark it allocated in the fabric too, so placement for other workloads
        // stops considering it free.
        topology.allocate(&[device_id.to_string()]);
        owners.insert(device_id.to_string(), vm_id.to_string());

        let device = topology
            .device(device_id)
            .expect("device presence checked above");
        Ok(Self::describe(device, &owners))
    }

    async fn detach(&self, vm_id: &str, device_id: &str) -> Result<GpuDescriptor, String> {
        let mut topology = self.topology.write();
        let mut owners = self.owners.write();

        if !topology.contains_device(device_id) {
            return Err(format!("GPU device not found: {device_id}"));
        }
        if owners.get(device_id).map(String::as_str) != Some(vm_id) {
            return Err(format!("GPU {device_id} is not attached to VM {vm_id}"));
        }

        topology.release(&[device_id.to_string()]);
        owners.remove(device_id);

        let device = topology
            .device(device_id)
            .expect("device presence checked above");
        Ok(Self::describe(device, &owners))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host_with_two_gpus() -> AgentGpuHost {
        let mut topology = GpuTopologyMap::new();
        topology.add_device(GpuDevice::new("gpu-0", "node-1", "H100").vram(80 * BYTES_PER_GB));
        topology.add_device(GpuDevice::new("gpu-1", "node-1", "H100").vram(80 * BYTES_PER_GB));
        AgentGpuHost::new(topology)
    }

    #[tokio::test]
    async fn list_reports_the_real_fleet_inventory() {
        let host = host_with_two_gpus();

        let mut devices = host.list().await.unwrap();
        devices.sort_by(|a, b| a.device_id.cmp(&b.device_id));

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].device_id, "gpu-0");
        assert_eq!(devices[0].model, "H100");
        assert_eq!(devices[0].vram_gb, 80, "bytes should convert to gigabytes");
        assert!(devices.iter().all(|d| d.is_available()));
    }

    #[tokio::test]
    async fn attaching_marks_the_device_allocated_in_the_fabric() {
        // The point of backing these tools with the real topology: an agent's
        // attach has to take the device out of the placement pool, or the
        // scheduler will hand the same GPU to another workload.
        let host = host_with_two_gpus();
        assert_eq!(host.with_topology(|t| t.available_count()), 2);

        host.attach("vm-a", "gpu-0").await.unwrap();

        assert_eq!(
            host.with_topology(|t| t.available_count()),
            1,
            "the fabric must see the device as allocated"
        );
    }

    #[tokio::test]
    async fn detaching_returns_the_device_to_the_placement_pool() {
        let host = host_with_two_gpus();
        host.attach("vm-a", "gpu-0").await.unwrap();
        host.detach("vm-a", "gpu-0").await.unwrap();

        assert_eq!(host.with_topology(|t| t.available_count()), 2);
        assert!(host.list().await.unwrap().iter().all(|d| d.is_available()));
    }

    #[tokio::test]
    async fn a_device_cannot_be_attached_to_two_vms() {
        let host = host_with_two_gpus();
        host.attach("vm-a", "gpu-0").await.unwrap();

        let err = host.attach("vm-b", "gpu-0").await.unwrap_err();
        assert!(err.contains("already allocated"), "got: {err}");
    }

    #[tokio::test]
    async fn the_owner_of_a_device_is_reported() {
        let host = host_with_two_gpus();
        host.attach("vm-a", "gpu-0").await.unwrap();

        let devices = host.list().await.unwrap();
        let gpu0 = devices.iter().find(|d| d.device_id == "gpu-0").unwrap();
        assert_eq!(gpu0.allocated_to.as_deref(), Some("vm-a"));
    }

    #[tokio::test]
    async fn a_vm_cannot_detach_a_device_it_does_not_hold() {
        let host = host_with_two_gpus();
        host.attach("vm-a", "gpu-0").await.unwrap();

        let err = host.detach("vm-b", "gpu-0").await.unwrap_err();
        assert!(err.contains("not attached"), "got: {err}");
        assert_eq!(
            host.with_topology(|t| t.available_count()),
            1,
            "a rejected detach must not free the device"
        );
    }

    #[tokio::test]
    async fn an_unknown_device_is_reported_as_missing() {
        let host = host_with_two_gpus();
        assert!(host
            .attach("vm-a", "gpu-99")
            .await
            .unwrap_err()
            .contains("not found"));
        assert!(host
            .detach("vm-a", "gpu-99")
            .await
            .unwrap_err()
            .contains("not found"));
    }

    #[tokio::test]
    async fn a_registered_device_joins_the_inventory() {
        let host = AgentGpuHost::empty();

        host.register(GpuDescriptor {
            device_id: "gpu-new".to_string(),
            model: "A100".to_string(),
            vram_gb: 40,
            compute_capability: 80,
            allocated_to: None,
        })
        .await
        .unwrap();

        let devices = host.list().await.unwrap();
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].model, "A100");
        assert_eq!(devices[0].vram_gb, 40);
        assert_eq!(devices[0].compute_capability, 80);
    }

    #[tokio::test]
    async fn registering_a_known_device_twice_is_rejected() {
        let host = host_with_two_gpus();

        let err = host
            .register(GpuDescriptor {
                device_id: "gpu-0".to_string(),
                model: "H100".to_string(),
                vram_gb: 80,
                compute_capability: 90,
                allocated_to: None,
            })
            .await
            .unwrap_err();

        assert!(err.contains("already registered"), "got: {err}");
        assert_eq!(host.list().await.unwrap().len(), 2);
    }
}
