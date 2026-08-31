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
use std::time::Duration;

use async_trait::async_trait;
use hv2_core::devices::virtio_vsock::VSOCK_GUEST_CID_MIN;
use hv2_core::BootSource;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::{AgentVM, Capability, CapabilitySet};

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
    /// Context ID for a guest channel, if this VM should have one.
    ///
    /// A VM with a CID gets a vsock device attached before its guest boots,
    /// which is what `vm.exec` runs commands over. Without one the VM is
    /// perfectly normal — it boots, prints to its console, and refuses
    /// `vm.exec` with an explanation instead of hanging.
    ///
    /// The channel cannot be added later: virtio-mmio has no enumeration, so
    /// the guest only finds the device because the kernel command line named
    /// its window before the kernel was loaded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guest_cid: Option<u64>,
}

/// The kernel argument that lets a Linux guest find its guest channel.
///
/// virtio-mmio has no enumeration: a guest probes the window only because the
/// command line names it. That command line is fixed inside a [`BootSource`]
/// before a VM exists to be asked where it put the device, so this renders the
/// argument from `hv2-core`'s constants instead — which is exactly why they are
/// constants. Whatever is built from this is cross-checked against the device's
/// own [`AgentVM::guest_kernel_args`] once it is attached.
pub fn guest_channel_kernel_arg() -> String {
    format!(
        "virtio_mmio.device=4K@{:#x}:{}",
        hv2_core::VM::VSOCK_MMIO_BASE,
        hv2_core::VM::VSOCK_IRQ
    )
}

/// Put [`guest_channel_kernel_arg`] on a Linux command line and report the
/// command line the guest will see.
///
/// Returns `None` for a boot source with no Linux command line to carry the
/// argument — a raw image is hand-written guest code that knows where to look,
/// and there is nowhere to write this for it.
///
/// An argument already present is left alone: a caller that spelled the window
/// out itself gets its own command line back, not a duplicated one.
fn merge_guest_channel_arg(boot: &mut BootSource) -> Option<String> {
    let BootSource::Linux { cmdline, .. } = boot else {
        return None;
    };
    let arg = guest_channel_kernel_arg();
    if !cmdline.split_whitespace().any(|token| token == arg) {
        if !cmdline.is_empty() {
            cmdline.push(' ');
        }
        cmdline.push_str(&arg);
    }
    Some(cmdline.clone())
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
            guest_cid: None,
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

/// What a guest has written to its console, as the tool surface reports it.
///
/// `attached` is the field that carries the information: an agent debugging a
/// guest that appears to be silent needs to know whether it is looking at a
/// quiet guest or at a VM with no console wired up at all. `output` is empty
/// in both cases, so on its own it cannot say which.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VmConsole {
    /// The VM this console belongs to.
    pub vm_id: String,
    /// Whether the host can observe a console for this VM at all.
    pub attached: bool,
    /// Console bytes so far, decoded lossily. Empty when nothing is attached.
    pub output: String,
}

/// A program to run inside a VM's guest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuestCommand {
    /// Program to execute. Run directly, not through a shell — `ls > out`
    /// redirects nothing. A caller wanting shell semantics names a shell.
    pub program: String,
    /// Arguments, already split. Nothing here parses a command line.
    #[serde(default)]
    pub args: Vec<String>,
    /// How long to wait before giving up.
    pub timeout_seconds: u64,
}

impl GuestCommand {
    /// A command with the default timeout.
    pub fn new(program: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            program: program.into(),
            args,
            timeout_seconds: 30,
        }
    }
}

/// What a program did inside a guest, as the tool surface reports it.
///
/// A non-zero `exit_code` is a result, not an error: the program ran, and its
/// output is what explains the failure. `exit_code` and `signal` stay apart
/// because a program killed by SIGKILL did not exit 0, and one field for both
/// would report a crash as a success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VmExec {
    /// The VM the program ran in.
    pub vm_id: String,
    /// Exit status, or `None` when a signal ended the program.
    pub exit_code: Option<i32>,
    /// Signal that ended the program, if one did.
    pub signal: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// Whether output was cut short at the agent's per-stream ceiling.
    pub truncated: bool,
    /// Whether the guest agent killed the program for overrunning.
    pub timed_out: bool,
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

    /// Console output for one VM, without consuming it.
    ///
    /// The default reports no console attached — correct for a host that
    /// tracks VMs without running them. It still resolves `vm_id` first, so an
    /// unknown VM is an error rather than an empty console.
    ///
    /// Implementations must not drain the buffer: an agent polling a boot log
    /// should see the whole log each time, not the slice that arrived since it
    /// last asked.
    async fn console(&self, vm_id: &str) -> Result<VmConsole, String> {
        let descriptor = self.status(vm_id).await?;
        Ok(VmConsole {
            vm_id: descriptor.vm_id,
            attached: false,
            output: String::new(),
        })
    }

    /// Run a program inside one VM's guest.
    ///
    /// The default refuses, because a host that tracks VMs without running
    /// them has no guest to run anything in. Returning a fabricated success
    /// here is the shape of defect `execute_plan` had — a result that reads
    /// like a measurement and is not one — so this says no instead. It still
    /// resolves `vm_id` first, so an unknown VM is an unknown VM.
    async fn exec(&self, vm_id: &str, command: GuestCommand) -> Result<VmExec, String> {
        let _ = self.status(vm_id).await?;
        Err(format!(
            "this host does not run guests, so it cannot run {} in one",
            command.program
        ))
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

    /// Build the VM a spec describes, with its guest channel already attached.
    ///
    /// The channel has to exist before the guest boots and the guest has to be
    /// told where it is, and those two facts are established at opposite ends
    /// of this function: the kernel command line is fixed inside the
    /// [`BootSource`] that `build()` consumes, while the device can only be
    /// attached once there is a VM to attach it to. So the argument is
    /// rendered from constants first, and checked against what the attached
    /// device reports afterwards — a guest pointed at the wrong address finds
    /// nothing and says nothing about it, so a disagreement fails here instead.
    async fn build_vm(spec: &VmSpec) -> Result<Arc<AgentVM>, String> {
        // Authorization for this host lives one layer up: the MCP
        // server checks that the caller holds GuestExec and owns this
        // VM before dispatching. The AgentVM-level check is for an
        // embedder holding one directly with no session behind it, so
        // a VM created here is given the capability its callers have
        // already been checked for -- otherwise the two gates
        // disagree and the outer one silently means nothing.
        let mut capabilities = CapabilitySet::default();
        capabilities.add(Capability::GuestExec);

        let mut boot = spec.boot.clone();
        let guest_cmdline = match (spec.guest_cid, boot.as_mut()) {
            (Some(_), Some(source)) => merge_guest_channel_arg(source),
            _ => None,
        };

        let mut builder = AgentVM::builder()
            .name(&spec.name)
            .cpu_cores(spec.cpu_cores)
            .memory_gb(spec.memory_gb)
            .enable_gpu(spec.enable_gpu)
            .enable_networking(spec.enable_networking)
            .capabilities(capabilities);
        if let Some(source) = boot {
            builder = builder.boot(source);
        }
        let vm = Arc::new(builder.build().await.map_err(|e| e.to_string())?);

        if let Some(cid) = spec.guest_cid {
            vm.attach_guest_channel(cid)
                .await
                .map_err(|e| e.to_string())?;
            let attached = vm.guest_kernel_args().ok_or_else(|| {
                "the guest channel reported no kernel arguments after attaching".to_string()
            })?;
            if let Some(cmdline) = &guest_cmdline {
                if !cmdline.split_whitespace().any(|token| token == attached) {
                    return Err(format!(
                        "the guest channel is at `{attached}` but the kernel command line says \
                         `{cmdline}`, so the guest would probe the wrong address and report \
                         nothing"
                    ));
                }
            }
        }

        Ok(vm)
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
        // Likewise a CID the device layer will refuse: the failure would
        // otherwise surface at start, long after the agent chose the number.
        if let Some(cid) = spec.guest_cid {
            if cid < VSOCK_GUEST_CID_MIN {
                return Err(format!(
                    "guest_cid {cid} is reserved; the first usable CID is {VSOCK_GUEST_CID_MIN}"
                ));
            }
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
            None => Self::build_vm(&spec).await?,
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

    async fn console(&self, vm_id: &str) -> Result<VmConsole, String> {
        let descriptor = self.status(vm_id).await?;

        // A VM that was created but never started has no `AgentVM` behind it,
        // and therefore no devices — not even an unattached console.
        let output = match self.live(vm_id) {
            Ok(vm) => vm.console_output().await,
            Err(_) => None,
        };

        Ok(VmConsole {
            vm_id: descriptor.vm_id,
            attached: output.is_some(),
            output: output.unwrap_or_default(),
        })
    }

    async fn exec(&self, vm_id: &str, command: GuestCommand) -> Result<VmExec, String> {
        let descriptor = self.status(vm_id).await?;
        let vm = self.live(vm_id).map_err(|_| {
            format!("VM {vm_id} has not been started, so there is no guest to run a command in")
        })?;

        let out = vm
            .exec_in_guest(
                &command.program,
                &command.args,
                Duration::from_secs(command.timeout_seconds),
            )
            .await
            .map_err(|e| e.to_string())?;

        Ok(VmExec {
            vm_id: descriptor.vm_id,
            exit_code: out.exit_code,
            signal: out.signal,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
            timed_out: out.timed_out,
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

    #[tokio::test]
    async fn console_for_a_vm_that_never_started_reports_nothing_attached() {
        let host = LocalVmHost::new();
        let vm = host.create(VmSpec::new("unstarted")).await.unwrap();

        let console = host.console(&vm.vm_id).await.unwrap();

        assert_eq!(console.vm_id, vm.vm_id);
        assert!(
            !console.attached,
            "there is no VM behind this yet, so there is no console"
        );
        assert!(console.output.is_empty());
    }

    #[tokio::test]
    async fn console_for_an_unknown_vm_says_so() {
        let host = LocalVmHost::new();
        // Not an empty console: an agent that mistyped a VM id must find out.
        assert!(host
            .console("nope")
            .await
            .unwrap_err()
            .contains("not found"));
    }

    /// A host that tracks VMs without running them, to exercise the trait's
    /// default `console`.
    struct BookkeepingHost;

    #[async_trait]
    impl VmHost for BookkeepingHost {
        async fn create(&self, _spec: VmSpec) -> Result<VmDescriptor, String> {
            unimplemented!()
        }
        async fn start(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            unimplemented!()
        }
        async fn stop(&self, _vm_id: &str, _force: bool) -> Result<VmDescriptor, String> {
            unimplemented!()
        }
        async fn pause(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            unimplemented!()
        }
        async fn resume(&self, _vm_id: &str) -> Result<VmDescriptor, String> {
            unimplemented!()
        }
        async fn delete(&self, _vm_id: &str) -> Result<(), String> {
            unimplemented!()
        }
        async fn status(&self, vm_id: &str) -> Result<VmDescriptor, String> {
            if vm_id != "known" {
                return Err(format!("VM not found: {vm_id}"));
            }
            Ok(VmDescriptor {
                vm_id: vm_id.to_string(),
                name: "known".to_string(),
                cpu_cores: 1,
                memory_gb: 1,
                status: "running".to_string(),
                boot_protocol: None,
            })
        }
        async fn list(&self) -> Result<Vec<VmDescriptor>, String> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn the_default_console_resolves_the_vm_before_answering() {
        let host = BookkeepingHost;

        let console = host.console("known").await.unwrap();
        assert!(!console.attached);
        assert!(console.output.is_empty());

        // The default must not report an empty console for a VM that does not
        // exist -- that would turn a typo into a silent guest.
        assert!(host
            .console("ghost")
            .await
            .unwrap_err()
            .contains("not found"));
    }

    // ── Guest channels ────────────────────────────────────────────────
    //
    // These build a real `AgentVM`, so they need a hypervisor backend. Where
    // there is none they report that and stop, the same as the `AgentVM` tests.

    /// A spec small enough to build twice on a laptop.
    fn small(name: &str) -> VmSpec {
        let mut spec = VmSpec::new(name);
        spec.cpu_cores = 1;
        spec.memory_gb = 1;
        spec
    }

    /// Whether a VM can be built here at all. Without a backend these tests
    /// have nothing to assert against, and a skip must not be mistaken for the
    /// guest-channel wiring failing.
    async fn hypervisor_available() -> bool {
        match LocalVmHost::build_vm(&small("backend-probe")).await {
            Ok(_) => true,
            Err(e) => {
                eprintln!("skipping: no hypervisor backend available ({e})");
                false
            }
        }
    }

    /// The command line a built VM will hand its guest.
    fn boot_cmdline(vm: &AgentVM) -> String {
        let core = vm.vm();
        match core.config().boot.as_ref() {
            Some(BootSource::Linux { cmdline, .. }) => cmdline.clone(),
            other => panic!("expected a Linux boot source, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_guest_cid_attaches_a_channel_and_tells_the_guest_where_it_is() {
        // Both halves or neither: a device with nothing on the command line is
        // a window no guest ever probes, and an argument with no device behind
        // it points the guest at empty address space. Either way `vm.exec`
        // times out with nothing to explain it.
        if !hypervisor_available().await {
            return;
        }
        let mut spec = small("with-channel");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz").with_cmdline("console=ttyS0"));
        spec.guest_cid = Some(3);

        let vm = LocalVmHost::build_vm(&spec).await.expect("build");

        assert!(
            vm.vm().vsock().is_some(),
            "the channel must exist before the guest boots"
        );
        let cmdline = boot_cmdline(&vm);
        assert!(
            cmdline.contains("console=ttyS0"),
            "the caller's own arguments must survive: {cmdline}"
        );
        assert!(
            cmdline.contains(&guest_channel_kernel_arg()),
            "virtio-mmio has no enumeration, so this argument is the whole of how the guest \
             learns the channel exists: {cmdline}"
        );
    }

    #[tokio::test]
    async fn the_argument_on_the_command_line_matches_the_device_that_was_attached() {
        // The argument is rendered before a VM exists and the device is placed
        // after one does. If those two ever drift apart the guest probes an
        // address with nothing at it, and reports nothing — so the check has to
        // happen here, where it can still be loud.
        if !hypervisor_available().await {
            return;
        }
        let mut spec = small("cross-checked");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz"));
        spec.guest_cid = Some(7);

        let vm = LocalVmHost::build_vm(&spec).await.expect("build");

        let reported = vm
            .guest_kernel_args()
            .expect("an attached channel reports its window");
        let cmdline = boot_cmdline(&vm);
        assert!(
            cmdline.split_whitespace().any(|token| token == reported),
            "the device is at `{reported}` but the command line says `{cmdline}`"
        );
    }

    #[tokio::test]
    async fn a_vm_with_no_guest_cid_boots_exactly_as_it_was_asked_to() {
        // A VM without a channel is the normal case, not a degraded one. It
        // must not acquire a device nobody asked for, and its command line must
        // come back untouched.
        if !hypervisor_available().await {
            return;
        }
        let mut spec = small("no-channel");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz").with_cmdline("console=ttyS0 quiet"));

        let vm = LocalVmHost::build_vm(&spec).await.expect("build");

        assert!(vm.vm().vsock().is_none());
        assert!(vm.guest_kernel_args().is_none());
        assert_eq!(boot_cmdline(&vm), "console=ttyS0 quiet");
    }

    #[tokio::test]
    async fn a_command_line_that_already_names_the_window_is_left_alone() {
        // A caller that spelled the argument out itself -- copied from the boot
        // probe, say -- must not get it twice. Two identical virtio_mmio.device
        // entries make the kernel probe the same window twice.
        if !hypervisor_available().await {
            return;
        }
        let existing = format!("console=ttyS0 {}", guest_channel_kernel_arg());
        let mut spec = small("already-named");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz").with_cmdline(&existing));
        spec.guest_cid = Some(3);

        let vm = LocalVmHost::build_vm(&spec).await.expect("build");

        assert_eq!(boot_cmdline(&vm), existing);
        assert!(vm.vm().vsock().is_some());
    }

    #[tokio::test]
    async fn create_rejects_a_reserved_guest_cid() {
        // 0, 1 and 2 belong to the hypervisor, to loopback and to the host.
        // Refusing at create keeps the mistake next to the number the agent
        // chose, instead of surfacing as a failed start much later.
        let host = LocalVmHost::new();
        let mut spec = VmSpec::new("host-cid");
        spec.guest_cid = Some(2);

        let err = host.create(spec).await.expect_err("2 is the host");
        assert!(err.contains("reserved"), "got: {err}");
        assert_eq!(host.vm_count(), 0);
    }

    #[test]
    fn a_spec_round_trips_through_json() {
        // Specs cross the tool boundary as JSON, boot source included.
        let mut spec = VmSpec::new("serialized");
        spec.boot = Some(BootSource::linux("/boot/vmlinuz").with_cmdline("quiet"));
        spec.guest_cid = Some(3);

        let json = serde_json::to_string(&spec).unwrap();
        let back: VmSpec = serde_json::from_str(&json).unwrap();

        assert_eq!(back, spec);
    }
}
