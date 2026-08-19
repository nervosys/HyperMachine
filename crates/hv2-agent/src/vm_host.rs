//! The seam between the MCP tool surface and a real hypervisor.
//!
//! [`McpServer`](crate::mcp::McpServer) dispatches its `vm.*` tools against a
//! [`VmHost`]. Without one installed the server keeps its own bookkeeping — a
//! faithful state machine over nothing, which is what makes tool schemas and
//! agent logic testable with no hypervisor present. Install a host and the same
//! tool calls create, boot, and destroy real VMs.
//!
//! [`LocalVmHost`] is the in-process implementation, backed by
//! [`AgentVM`] and therefore by `hv2-core`'s hypervisor
//! backends. A deployment that manages VMs elsewhere — a fleet service, a
//! remote node — implements [`VmHost`] itself.
//!
//! # Example
//!
//! ```no_run
//! use std::sync::Arc;
//! use hv2_agent::mcp::{AgentCapabilities, McpServer};
//! use hv2_agent::vm_host::LocalVmHost;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let server = McpServer::new();
//! server.set_vm_host(Arc::new(LocalVmHost::new()));
//!
//! // `vm.create` now allocates a real VM instead of a JSON record.
//! let session = server.create_session("agent-1", AgentCapabilities::full())?;
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hv2_core::BootSource;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::AgentVM;

/// What an agent asked for when it called `vm.create`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VmSpec {
    /// Human-readable VM name.
    pub name: String,
    /// Number of virtual CPUs.
    pub cpu_cores: u32,
    /// Guest memory in gigabytes.
    pub memory_gb: u64,
    /// Whether to expose a GPU.
    #[serde(default)]
    pub enable_gpu: bool,
    /// Whether to attach networking.
    #[serde(default)]
    pub enable_networking: bool,
    /// What the VM boots, if anything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot: Option<BootSource>,
}

impl VmSpec {
    /// A spec with the tool surface's defaults: 2 cores, 4 GB, no boot source.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cpu_cores: 2,
            memory_gb: 4,
            enable_gpu: false,
            enable_networking: false,
            boot: None,
        }
    }
}

/// A VM as the tool surface reports it back to an agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VmDescriptor {
    /// Host-assigned identifier, used by every subsequent tool call.
    pub vm_id: String,
    /// Human-readable name.
    pub name: String,
    /// Number of virtual CPUs.
    pub cpu_cores: u32,
    /// Guest memory in gigabytes.
    pub memory_gb: u64,
    /// Lifecycle status: `created`, `running`, `paused`, or `stopped`.
    pub status: String,
    /// Boot protocol in use (`linux`, `multiboot`, `raw`), if the VM has one.
    ///
    /// `None` means the VM has no guest code — it can be started, but nothing
    /// executes. Agents need to be able to tell the difference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_protocol: Option<String>,
}

/// What a host can actually measure about a running VM.
///
/// Every measured quantity is an [`Option`] on purpose. A host that cannot
/// observe a value reports `None`, never a plausible-looking number: an agent
/// deciding whether to scale a workload has to be able to tell "idle" from
/// "not instrumented". A host backed by [`AgentVM`] measures
/// `cpu_usage_percent` from the run loop’s own vCPU timings; one that only
/// tracks VMs reports `None`. `memory_used_bytes` stays `None` everywhere:
/// it needs cooperation from inside the guest (virtio-balloon or a guest
/// agent), which does not exist yet.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VmMetrics {
    /// The VM these metrics describe.
    pub vm_id: String,
    /// Lifecycle status, matching [`VmDescriptor::status`].
    pub status: String,
    /// Number of virtual CPUs configured.
    pub vcpu_count: u32,
    /// Guest memory allocated, in bytes.
    pub memory_total_bytes: u64,
    /// Seconds since the guest last started, or `None` if it has never run or
    /// the host does not track start times.
    pub uptime_seconds: Option<u64>,
    /// CPU utilization across all vCPUs, 0–100. `None` when unmeasured.
    pub cpu_usage_percent: Option<f64>,
    /// Guest memory actually in use. `None` without guest cooperation.
    pub memory_used_bytes: Option<u64>,
}

impl VmMetrics {
    /// The most any host can say from a descriptor alone: shape and status,
    /// nothing measured.
    pub fn from_descriptor(descriptor: &VmDescriptor) -> Self {
        Self {
            vm_id: descriptor.vm_id.clone(),
            status: descriptor.status.clone(),
            vcpu_count: descriptor.cpu_cores,
            memory_total_bytes: descriptor.memory_gb * 1024 * 1024 * 1024,
            uptime_seconds: None,
            cpu_usage_percent: None,
            memory_used_bytes: None,
        }
    }
}

/// Something that can run VMs on an agent's behalf.
///
/// Every method takes a host-assigned `vm_id`. The MCP server checks that the
/// calling session owns that VM before dispatching, so an implementation may
/// assume authorization but must still validate that the VM exists.
#[async_trait]
pub trait VmHost: Send + Sync {
    /// Create a VM. The returned descriptor's `vm_id` is what the agent uses
    /// from then on.
    async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String>;

    /// Start a VM, booting its guest when it has a boot source.
    async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String>;

    /// Stop a VM. `force` skips a graceful guest shutdown.
    async fn stop(&self, vm_id: &str, force: bool) -> Result<VmDescriptor, String>;

    /// Pause a running VM.
    async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String>;

    /// Resume a paused VM.
    async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String>;

    /// Destroy a VM and release its resources.
    async fn delete(&self, vm_id: &str) -> Result<(), String>;

    /// Current state of one VM.
    async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String>;

    /// Every VM this host knows about.
    ///
    /// The MCP server filters the result to the calling session's own VMs, so
    /// an implementation may return the whole host inventory.
    async fn list(&self) -> Result<Vec<VmDescriptor>, String>;

    /// Current telemetry for one VM.
    ///
    /// The default reports the VM's configured shape and status with every
    /// measured field `None` — correct for a host that tracks VMs without
    /// observing them. Override it when the host can measure more.
    async fn metrics(&self, vm_id: &str) -> Result<VmMetrics, String> {
        Ok(VmMetrics::from_descriptor(&self.status(vm_id).await?))
    }
}

// ═══════════════════════════════════════════════════════════════════
//  In-process host
// ═══════════════════════════════════════════════════════════════════

/// A [`VmHost`] that runs VMs in this process via [`AgentVM`].
///
/// This is the implementation that connects the agent tool surface to the
/// hypervisor: `vm.create` allocates guest memory and vCPUs, and `vm.start`
/// provisions the backend partition, loads the boot images, and runs the guest.
#[derive(Default)]
pub struct LocalVmHost {
    vms: RwLock<HashMap<String, HostedVm>>,
}

/// A VM held by a [`LocalVmHost`].
struct HostedVm {
    /// The live VM. Absent until the VM is first started, because
    /// [`AgentVM`] allocates guest memory on construction — a created-but-never
    /// -started VM should not hold gigabytes of it.
    vm: Option<Arc<AgentVM>>,
    spec: VmSpec,
    status: String,
}

impl HostedVm {
    fn describe(&self, vm_id: &str) -> VmDescriptor {
        VmDescriptor {
            vm_id: vm_id.to_string(),
            name: self.spec.name.clone(),
            cpu_cores: self.spec.cpu_cores,
            memory_gb: self.spec.memory_gb,
            status: self.status.clone(),
            boot_protocol: self.spec.boot.as_ref().map(|b| b.protocol().to_string()),
        }
    }
}

impl LocalVmHost {
    /// A host with no VMs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of VMs this host is tracking.
    pub fn vm_count(&self) -> usize {
        self.vms.read().len()
    }

    /// Apply `f` to the named VM's record, or report that it does not exist.
    fn with_vm<T>(&self, vm_id: &str, f: impl FnOnce(&mut HostedVm) -> T) -> Result<T, String> {
        let mut vms = self.vms.write();
        let hosted = vms
            .get_mut(vm_id)
            .ok_or_else(|| format!("VM not found: {vm_id}"))?;
        Ok(f(hosted))
    }

    /// The live VM handle for `vm_id`, if it has been started.
    fn live(&self, vm_id: &str) -> Result<Arc<AgentVM>, String> {
        self.vms
            .read()
            .get(vm_id)
            .ok_or_else(|| format!("VM not found: {vm_id}"))?
            .vm
            .clone()
            .ok_or_else(|| format!("VM {vm_id} is not running"))
    }
}

#[async_trait]
impl VmHost for LocalVmHost {
    async fn create(&self, spec: VmSpec) -> Result<VmDescriptor, String> {
        if spec.cpu_cores == 0 {
            return Err("cpu_cores must be at least 1".to_string());
        }
        if spec.memory_gb == 0 {
            return Err("memory_gb must be at least 1".to_string());
        }
        // Fail on an unusable boot image now, while the agent can still
        // correct it, rather than at start time.
        if let Some(source) = &spec.boot {
            source.load().map_err(|e| e.to_string())?;
        }

        let vm_id = crate::mcp::uuid_v4();
        let hosted = HostedVm {
            vm: None,
            spec,
            status: "created".to_string(),
        };
        let descriptor = hosted.describe(&vm_id);
        self.vms.write().insert(vm_id, hosted);
        Ok(descriptor)
    }

    async fn start(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        let (spec, already_live) = {
            let vms = self.vms.read();
            let hosted = vms
                .get(vm_id)
                .ok_or_else(|| format!("VM not found: {vm_id}"))?;
            if hosted.status == "running" {
                return Err(format!("VM {vm_id} is already running"));
            }
            (hosted.spec.clone(), hosted.vm.clone())
        };

        // Build outside the lock: allocating guest memory and provisioning the
        // backend can take a while, and holding the map would stall every other
        // agent's tool calls.
        let vm = match already_live {
            Some(vm) => vm,
            None => {
                let mut builder = AgentVM::builder()
                    .name(&spec.name)
                    .cpu_cores(spec.cpu_cores)
                    .memory_gb(spec.memory_gb)
                    .enable_gpu(spec.enable_gpu)
                    .enable_networking(spec.enable_networking);
                if let Some(source) = spec.boot.clone() {
                    builder = builder.boot(source);
                }
                Arc::new(builder.build().await.map_err(|e| e.to_string())?)
            }
        };

        if spec.boot.is_some() {
            vm.launch().await.map_err(|e| e.to_string())?;
        } else {
            vm.start().await.map_err(|e| e.to_string())?;
        }

        self.with_vm(vm_id, |hosted| {
            hosted.vm = Some(vm);
            hosted.status = "running".to_string();
            hosted.describe(vm_id)
        })
    }

    async fn stop(&self, vm_id: &str, _force: bool) -> Result<VmDescriptor, String> {
        if let Ok(vm) = self.live(vm_id) {
            vm.stop().await.map_err(|e| e.to_string())?;
        }
        self.with_vm(vm_id, |hosted| {
            hosted.status = "stopped".to_string();
            hosted.describe(vm_id)
        })
    }

    async fn pause(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.live(vm_id)?.pause().await.map_err(|e| e.to_string())?;
        self.with_vm(vm_id, |hosted| {
            hosted.status = "paused".to_string();
            hosted.describe(vm_id)
        })
    }

    async fn resume(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.live(vm_id)?
            .resume()
            .await
            .map_err(|e| e.to_string())?;
        self.with_vm(vm_id, |hosted| {
            hosted.status = "running".to_string();
            hosted.describe(vm_id)
        })
    }

    async fn delete(&self, vm_id: &str) -> Result<(), String> {
        // Stop first: dropping a running VM would leave its backend partition
        // and execution loop behind.
        if let Ok(vm) = self.live(vm_id) {
            let _ = vm.stop().await;
        }
        self.vms
            .write()
            .remove(vm_id)
            .map(|_| ())
            .ok_or_else(|| format!("VM not found: {vm_id}"))
    }

    async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
        self.vms
            .read()
            .get(vm_id)
            .map(|hosted| hosted.describe(vm_id))
            .ok_or_else(|| format!("VM not found: {vm_id}"))
    }

    async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
        Ok(self
            .vms
            .read()
            .iter()
            .map(|(id, hosted)| hosted.describe(id))
            .collect())
    }

    async fn metrics(&self, vm_id: &str) -> Result<VmMetrics, String> {
        let descriptor = self.status(vm_id).await?;

        // A VM that was created but never started has no `AgentVM` behind it,
        // so there is nothing to measure — report its shape and say so through
        // the absent uptime rather than inventing a zero.
        let Ok(vm) = self.live(vm_id) else {
            return Ok(VmMetrics::from_descriptor(&descriptor));
        };

        let measured = vm.get_metrics().await.map_err(|e| e.to_string())?;
        Ok(VmMetrics {
            vm_id: descriptor.vm_id,
            status: descriptor.status,
            vcpu_count: measured.vcpu_count,
            memory_total_bytes: measured.memory_size,
            uptime_seconds: Some(measured.uptime_seconds),
            cpu_usage_percent: measured.cpu_usage_percent,
            memory_used_bytes: measured.memory_used_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_registers_a_vm_and_returns_its_id() {
        let host = LocalVmHost::new();
        let descriptor = host.create(VmSpec::new("agent-vm")).await.unwrap();

        assert_eq!(descriptor.name, "agent-vm");
        assert_eq!(descriptor.status, "created");
        assert!(!descriptor.vm_id.is_empty());
        assert_eq!(host.vm_count(), 1);
    }

    #[tokio::test]
    async fn create_reports_no_boot_protocol_when_there_is_no_guest() {
        let host = LocalVmHost::new();
        let descriptor = host.create(VmSpec::new("empty")).await.unwrap();

        assert!(
            descriptor.boot_protocol.is_none(),
            "an agent must be able to tell that this VM has no guest code"
        );
    }

    #[tokio::test]
    async fn create_rejects_a_degenerate_shape() {
        let host = LocalVmHost::new();

        let mut spec = VmSpec::new("no-cpu");
        spec.cpu_cores = 0;
        assert!(host.create(spec).await.is_err());

        let mut spec = VmSpec::new("no-memory");
        spec.memory_gb = 0;
        assert!(host.create(spec).await.is_err());

        assert_eq!(host.vm_count(), 0, "rejected specs must not be registered");
    }

    #[tokio::test]
    async fn create_rejects_an_unusable_boot_image() {
        let host = LocalVmHost::new();
        let mut spec = VmSpec::new("bad-boot");
        spec.boot = Some(BootSource::linux("/nonexistent/vmlinuz"));

        let err = host.create(spec).await.expect_err("should reject");
        assert!(err.contains("vmlinuz"), "got: {err}");
        assert_eq!(host.vm_count(), 0);
    }

    #[tokio::test]
    async fn operations_on_an_unknown_vm_say_so() {
        let host = LocalVmHost::new();

        assert!(host.status("nope").await.unwrap_err().contains("not found"));
        assert!(host.start("nope").await.unwrap_err().contains("not found"));
        assert!(host.delete("nope").await.unwrap_err().contains("not found"));
        assert!(host
            .stop("nope", false)
            .await
            .unwrap_err()
            .contains("not found"));
    }

    #[tokio::test]
    async fn pausing_a_vm_that_was_never_started_is_an_error() {
        let host = LocalVmHost::new();
        let vm = host.create(VmSpec::new("idle")).await.unwrap();

        let err = host.pause(&vm.vm_id).await.expect_err("nothing to pause");
        assert!(err.contains("not running"), "got: {err}");
    }

    #[tokio::test]
    async fn delete_removes_the_vm() {
        let host = LocalVmHost::new();
        let vm = host.create(VmSpec::new("temp")).await.unwrap();

        host.delete(&vm.vm_id).await.unwrap();

        assert_eq!(host.vm_count(), 0);
        assert!(host.status(&vm.vm_id).await.is_err());
    }

    #[tokio::test]
    async fn list_reports_every_vm() {
        let host = LocalVmHost::new();
        host.create(VmSpec::new("a")).await.unwrap();
        host.create(VmSpec::new("b")).await.unwrap();

        let mut names: Vec<String> = host
            .list()
            .await
            .unwrap()
            .into_iter()
            .map(|v| v.name)
            .collect();
        names.sort();

        assert_eq!(names, vec!["a", "b"]);
    }

    #[tokio::test]
    async fn metrics_report_shape_without_inventing_measurements() {
        let host = LocalVmHost::new();
        let mut spec = VmSpec::new("unstarted");
        spec.cpu_cores = 4;
        spec.memory_gb = 8;
        let vm = host.create(spec).await.unwrap();

        let metrics = host.metrics(&vm.vm_id).await.unwrap();

        assert_eq!(metrics.vcpu_count, 4);
        assert_eq!(metrics.memory_total_bytes, 8 * 1024 * 1024 * 1024);
        assert_eq!(metrics.status, "created");
        // A VM that never ran has no uptime and no utilization. Reporting 0%
        // would be indistinguishable from a genuinely idle guest.
        assert_eq!(metrics.uptime_seconds, None);
        assert_eq!(metrics.cpu_usage_percent, None);
        assert_eq!(metrics.memory_used_bytes, None);
    }

    #[tokio::test]
    async fn metrics_for_an_unknown_vm_say_so() {
        let host = LocalVmHost::new();
        assert!(host
            .metrics("nope")
            .await
            .unwrap_err()
            .contains("not found"));
    }

    #[test]
    fn a_spec_round_trips_through_json() {
        // Specs cross the tool boundary as JSON, boot source included.
        let mut spec = VmSpec::new("serialized");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz").with_cmdline("quiet"));

        let json = serde_json::to_string(&spec).unwrap();
        let back: VmSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(back, spec);
    }
}
