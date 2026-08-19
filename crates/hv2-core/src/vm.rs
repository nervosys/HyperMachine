//! Virtual Machine implementation
//!
//! This module provides the core VM abstraction including multi-vCPU
//! parallel execution support using tokio tasks.

use crate::{
    DeviceManager, Error, EventBus, GuestMemory, HypervisorBackend, IoDirection, Pic8259, Result,
    VCpu, VmEvent, VmExit,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;

/// VM state
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VMState {
    Created,
    Running,
    Paused,
    Stopped,
    Error,
}

impl std::fmt::Display for VMState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Created => write!(f, "Created"),
            Self::Running => write!(f, "Running"),
            Self::Paused => write!(f, "Paused"),
            Self::Stopped => write!(f, "Stopped"),
            Self::Error => write!(f, "Error"),
        }
    }
}

/// VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMConfig {
    /// Human-readable name for this VM instance.
    pub name: String,
    /// Number of virtual CPUs to allocate.
    pub vcpu_count: u32,
    /// Total guest memory in bytes.
    pub memory_size: u64,
    /// Enable GPU virtualization for this VM.
    pub enable_gpu: bool,
    /// Enable network virtualization for this VM.
    pub enable_networking: bool,
    /// Enable tracing instrumentation for this VM.
    pub enable_tracing: bool,
    /// Enable parallel vCPU execution using multiple tokio tasks
    #[serde(default = "default_parallel_vcpu")]
    pub parallel_vcpu: bool,
    /// vCPU affinity: map vCPU ID to host CPU core (optional)
    #[serde(default)]
    pub vcpu_affinity: Vec<(u32, usize)>,
    /// Bind this VM's guest memory to a host NUMA node (optional). Pair with
    /// `vcpu_affinity` cores on the same node for NUMA-local execution.
    #[serde(default)]
    pub memory_numa_node: Option<u32>,

    /// What this VM boots — a Linux kernel, a Multiboot kernel, or a raw image.
    ///
    /// `None` creates a VM with vCPUs and empty guest memory. That is the right
    /// shape for a caller that writes guest code itself (tests, unikernel
    /// harnesses), but such a VM has nothing to execute until something does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot: Option<crate::boot::source::BootSource>,
}

fn default_parallel_vcpu() -> bool {
    true
}

impl VMConfig {
    /// Validate the optional [`Self::vcpu_affinity`] map.
    ///
    /// Every entry must reference a vCPU that exists in this config and a host
    /// core that exists on this machine, and no vCPU or host core may appear
    /// twice. An empty map (the default) is always valid and leaves vCPU
    /// scheduling to the host.
    ///
    /// This makes `vcpu_affinity` a *checked* input rather than silently-ignored
    /// configuration. Enforcing the mapping at run time — pinning each vCPU
    /// thread to its core — additionally requires running pinned vCPUs on
    /// dedicated OS threads (see [`Self::affinity_for`]).
    pub fn validate_affinity(&self) -> Result<()> {
        if self.vcpu_affinity.is_empty() {
            return Ok(());
        }
        let core_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mut seen_vcpus = std::collections::HashSet::new();
        let mut seen_cores = std::collections::HashSet::new();
        for &(vcpu, core) in &self.vcpu_affinity {
            if vcpu >= self.vcpu_count {
                return Err(Error::Config(format!(
                    "vcpu_affinity references vCPU {vcpu}, but the VM has {} vCPU(s)",
                    self.vcpu_count
                )));
            }
            if core >= core_count {
                return Err(Error::Config(format!(
                    "vcpu_affinity pins vCPU {vcpu} to host core {core}, but only \
                     {core_count} core(s) are available"
                )));
            }
            if !seen_vcpus.insert(vcpu) {
                return Err(Error::Config(format!(
                    "vcpu_affinity maps vCPU {vcpu} more than once"
                )));
            }
            if !seen_cores.insert(core) {
                return Err(Error::Config(format!(
                    "vcpu_affinity pins more than one vCPU to host core {core}"
                )));
            }
        }
        Ok(())
    }

    /// The host core a given vCPU is pinned to, if any.
    pub fn affinity_for(&self, vcpu_id: u32) -> Option<usize> {
        self.vcpu_affinity
            .iter()
            .find(|(id, _)| *id == vcpu_id)
            .map(|(_, core)| *core)
    }

    /// Resolve the host NUMA node this VM's guest memory should be bound to.
    ///
    /// An explicit [`Self::memory_numa_node`] wins. Otherwise, if every pinned
    /// core in [`Self::vcpu_affinity`] resolves to the *same* host NUMA node,
    /// that node is used — so setting `vcpu_affinity` alone already yields
    /// NUMA-local memory. Returns `None` (host-default placement) when there is
    /// no affinity, the pinned cores span multiple nodes, or the host topology
    /// cannot be queried.
    pub fn resolve_memory_node(&self) -> Option<u32> {
        if let Some(node) = self.memory_numa_node {
            return Some(node);
        }
        if self.vcpu_affinity.is_empty() {
            return None;
        }
        let mut resolved = None;
        for &(_, core) in &self.vcpu_affinity {
            // An unknown topology means we cannot promise NUMA locality, so
            // fall back to host-default placement rather than guessing.
            let node = crate::cpu_affinity::numa_node_for_core(core)?;
            match resolved {
                None => resolved = Some(node),
                Some(existing) if existing == node => {}
                Some(_) => return None, // cores span multiple nodes
            }
        }
        resolved
    }
}

impl Default for VMConfig {
    fn default() -> Self {
        Self {
            name: "default".to_string(),
            vcpu_count: 1,
            memory_size: 1024 * 1024 * 1024, // 1GB
            enable_gpu: false,
            enable_networking: false,
            enable_tracing: false,
            parallel_vcpu: true,
            vcpu_affinity: Vec::new(),
            memory_numa_node: None,
            boot: None,
        }
    }
}

/// How long [`VM::stop`] waits for a launched execution loop to unwind before
/// giving up on it. The loop checks the running flag once per VM exit, so this
/// only elapses for a guest that has stopped exiting altogether.
const RUN_LOOP_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Message type for vCPU coordination
#[derive(Debug)]
enum VCpuMessage {
    /// Stop the vCPU
    Stop,
    /// Pause the vCPU
    Pause,
    /// Resume the vCPU
    Resume,
    /// Inject an interrupt
    Interrupt { vector: u8 },
}

/// vCPU execution statistics
#[derive(Debug, Default)]
pub struct VCpuStats {
    /// Number of VM exits
    pub exits: AtomicU64,
    /// Time spent running (nanoseconds)
    pub run_time_ns: AtomicU64,
    /// Number of interrupts injected
    pub interrupts: AtomicU64,
    /// Number of MMIO exits
    pub mmio_exits: AtomicU64,
    /// Number of I/O exits
    pub io_exits: AtomicU64,
}

impl VCpuStats {
    #[inline]
    pub fn exits(&self) -> u64 {
        self.exits.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn run_time_ns(&self) -> u64 {
        self.run_time_ns.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn interrupts(&self) -> u64 {
        self.interrupts.load(Ordering::Relaxed)
    }
}

/// State for a running vCPU task
struct VCpuTaskState {
    /// Channel to send commands to the vCPU
    tx: mpsc::Sender<VCpuMessage>,
    /// Join handle for the task
    handle: JoinHandle<Result<()>>,
}

/// Virtual Machine
pub struct VM {
    config: VMConfig,
    state: Arc<RwLock<VMState>>,
    vcpus: Vec<Arc<VCpu>>,
    memory: Arc<GuestMemory>,
    devices: Arc<DeviceManager>,
    pic: Arc<Pic8259>,
    backend: Arc<dyn HypervisorBackend>,
    exit_notify: Arc<Notify>,
    event_bus: EventBus,
    /// Running state flag shared with vCPU tasks
    running: Arc<AtomicBool>,
    /// Per-vCPU statistics
    vcpu_stats: Vec<Arc<VCpuStats>>,
    /// vCPU task handles (only populated when running in parallel mode)
    vcpu_tasks: RwLock<Vec<VCpuTaskState>>,
    /// Backend VM handle (partition / VM fd), created by [`VM::provision`].
    ///
    /// Held for the VM's lifetime: dropping it would tear down the backend's
    /// partition out from under the running vCPUs.
    hv_vm: RwLock<Option<crate::hypervisor::HypervisorVm>>,
    /// The background execution loop spawned by [`VM::launch`], if any.
    run_task: RwLock<Option<JoinHandle<Result<()>>>>,
    /// Image allowlist consulted by [`VM::provision`], if installed.
    ///
    /// `None` — the default — admits any readable boot image, which is what
    /// every caller before this expected. Install one with
    /// [`VM::set_image_registry`] to make a denied or revoked image fail to
    /// provision rather than merely be queryable.
    image_registry: RwLock<Option<Arc<crate::security::image_registry::ImageRegistry>>>,
}

impl VM {
    /// Create a new VM with the given configuration
    pub fn new(config: VMConfig) -> Result<Self> {
        // Validate configuration
        if config.vcpu_count == 0 {
            return Err(Error::Config("vCPU count must be > 0".to_string()));
        }

        if config.memory_size == 0 {
            return Err(Error::Config("Memory size must be > 0".to_string()));
        }

        // Create hypervisor backend
        let backend = Arc::from(crate::hypervisor::create_backend()?);

        Self::new_with_backend(config, backend)
    }

    /// Create a new VM with a custom hypervisor backend
    ///
    /// This is primarily used for testing with mock backends.
    pub fn new_with_backend(config: VMConfig, backend: Arc<dyn HypervisorBackend>) -> Result<Self> {
        // Validate configuration
        if config.vcpu_count == 0 {
            return Err(Error::Config("vCPU count must be > 0".to_string()));
        }

        if config.memory_size == 0 {
            return Err(Error::Config("Memory size must be > 0".to_string()));
        }

        // Reject a malformed vCPU affinity map instead of silently ignoring it.
        config.validate_affinity()?;

        // Reject a memory NUMA node that does not exist on this host.
        if let Some(node) = config.memory_numa_node {
            let node_count = crate::cpu_affinity::numa_node_count();
            if node >= node_count {
                return Err(Error::Config(format!(
                    "memory_numa_node {node} does not exist (host has {node_count} NUMA node(s))"
                )));
            }
        }

        // Create vCPUs
        let vcpus: Vec<Arc<VCpu>> = (0..config.vcpu_count)
            .map(|id| Arc::new(VCpu::new(id)))
            .collect();

        // Create guest memory, bound to the resolved host NUMA node — explicit
        // when set, otherwise derived from the pinned cores' node.
        let memory = Arc::new(GuestMemory::new_on_node(
            config.memory_size,
            config.resolve_memory_node(),
        )?);

        // Initialize main memory region
        memory.allocate_region(config.memory_size, false)?;

        // Create device manager
        let devices = Arc::new(DeviceManager::new());

        // Create PIC (Intel 8259)
        let pic = Arc::new(Pic8259::new());

        let event_bus = EventBus::default();

        // Create per-vCPU statistics
        let vcpu_stats = (0..config.vcpu_count)
            .map(|_| Arc::new(VCpuStats::default()))
            .collect();

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(VMState::Created)),
            vcpus,
            memory,
            devices,
            pic,
            backend,
            exit_notify: Arc::new(Notify::new()),
            event_bus,
            running: Arc::new(AtomicBool::new(false)),
            vcpu_stats,
            vcpu_tasks: RwLock::new(Vec::new()),
            hv_vm: RwLock::new(None),
            run_task: RwLock::new(None),
            image_registry: RwLock::new(None),
        })
    }

    /// Get VM configuration
    pub fn config(&self) -> &VMConfig {
        &self.config
    }

    /// Get VM state
    pub fn state(&self) -> VMState {
        *self.state.read()
    }

    /// Provision this VM on its hypervisor backend and load its boot source.
    ///
    /// This is the step that turns a configured VM into one the hardware knows
    /// about: it creates the backend's partition (WHPX) or VM file descriptor
    /// (KVM) along with the backing vCPUs, then — if [`VMConfig::boot`] names a
    /// boot source — reads the images, writes them into guest physical memory,
    /// and leaves vCPU 0 at the entry point with the architectural state the
    /// boot protocol requires.
    ///
    /// Calling it more than once is a no-op, so [`VM::start`] and [`VM::launch`]
    /// can both call it without coordinating.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend cannot create the VM, if a boot image is
    /// missing or malformed, if the images do not fit in guest memory, or if
    /// the backend does not support the configured boot protocol.
    pub async fn provision(&self) -> Result<()> {
        if self.hv_vm.read().is_some() {
            return Ok(());
        }

        // Resolve, admit, and size-check the boot image *before* the backend is
        // asked for anything. Refusing a disallowed image after a partition
        // exists would allocate hypervisor resources for a VM that was never
        // going to run — and would hide the refusal behind whatever the backend
        // happened to say first.
        let boot = match &self.config.boot {
            Some(source) => {
                let loaded = source.load()?;

                self.admit_boot_image(&loaded)?;

                let needed = loaded.highest_address()?;
                if needed > self.config.memory_size {
                    return Err(Error::Config(format!(
                        "boot images need guest memory up to {:#x} but VM '{}' has only {:#x}",
                        needed, self.config.name, self.config.memory_size
                    )));
                }
                Some(loaded)
            }
            None => None,
        };

        // A boot source on a backend that cannot execute is the one case where
        // everything below succeeds and the guest still never runs. Say so here
        // rather than leaving it to be inferred from a Running VM that produces
        // no output.
        if boot.is_some() && !self.backend.executes_guest_code() {
            tracing::warn!(
                "VM {} on the {} backend: guest code will not execute, so the loaded image will not run. Use KVM, WHPX or HVF to boot it.",
                self.config.name,
                self.backend.platform(),
            );
        }

        let hv_vm = self
            .backend
            .create_vm(self.config.vcpu_count, self.config.memory_size)
            .await?;

        tracing::info!(
            "Provisioned VM '{}' on the {} backend ({} vCPUs, {} MiB)",
            self.config.name,
            self.backend.platform(),
            self.config.vcpu_count,
            self.config.memory_size / (1024 * 1024),
        );

        if let Some(loaded) = boot {
            let boot_vcpu = &self.vcpus[0];
            self.backend.load_boot(boot_vcpu, &loaded).await?;

            // Backends that keep no vCPU state of their own (TCG, mocks) read
            // the shared `VCpu`, so the entry point has to land there too.
            let mut regs = boot_vcpu.registers();
            regs.rip = loaded.entry_point();
            boot_vcpu.set_registers(regs);

            tracing::info!(
                "VM '{}': loaded {} boot image ({} bytes), entry {:#x}",
                self.config.name,
                loaded.protocol(),
                loaded.image_bytes(),
                loaded.entry_point(),
            );
        }

        *self.hv_vm.write() = Some(hv_vm);
        Ok(())
    }

    /// Refuse a boot image the installed registry does not admit.
    ///
    /// With no registry installed this is a no-op, which is the default and
    /// preserves the behaviour every existing caller relies on.
    fn admit_boot_image(&self, loaded: &crate::boot::source::LoadedBoot) -> Result<()> {
        use crate::security::image_registry::AdmissionDecision;

        let Some(registry) = self.image_registry.read().clone() else {
            return Ok(());
        };

        // Identify the bytes about to be loaded, not the path they came from.
        // A digest we cannot compute is a denial, never a pass: an enforcement
        // point that fails open is not one.
        let digest = loaded.primary_image_digest()?;

        match registry.check_admission_by_digest(&digest) {
            AdmissionDecision::Allowed => Ok(()),
            AdmissionDecision::AllowedWithWarning(warning) => {
                tracing::warn!("VM '{}': {}", self.config.name, warning);
                Ok(())
            }
            AdmissionDecision::Denied(reason) => Err(Error::PermissionDenied(format!(
                "VM '{}': boot image rejected by the image registry: {reason}",
                self.config.name
            ))),
        }
    }

    /// Gate this VM's boot images on an image allowlist.
    ///
    /// Without one — the default — any readable image boots. With one, a VM
    /// refuses to provision unless the registry admits the digest of the kernel
    /// (or raw image) it is about to load.
    pub fn set_image_registry(
        &self,
        registry: Arc<crate::security::image_registry::ImageRegistry>,
    ) {
        *self.image_registry.write() = Some(registry);
    }

    /// The installed image registry, if any.
    pub fn image_registry(&self) -> Option<Arc<crate::security::image_registry::ImageRegistry>> {
        self.image_registry.read().clone()
    }

    /// Whether this VM backend actually executes guest instructions.
    ///
    /// `false` on the TCG fallback. A VM can still be created, provisioned
    /// and started there; it simply will not run a guest, which is worth
    /// being able to report rather than discover.
    pub fn executes_guest_code(&self) -> bool {
        self.backend.executes_guest_code()
    }

    /// Whether this VM has been provisioned on its backend.
    pub fn is_provisioned(&self) -> bool {
        self.hv_vm.read().is_some()
    }

    /// Provision, start, and begin executing this VM in the background.
    ///
    /// This is the whole-VM entry point a CLI or API handler wants: on return
    /// the guest is running, and the execution loop continues on a spawned task
    /// until [`VM::stop`] is called or the guest shuts down.
    ///
    /// Use [`VM::run`] instead when you want to own the execution loop and
    /// await it yourself.
    ///
    /// # Errors
    ///
    /// Returns an error if provisioning fails or the VM is not in a startable
    /// state. Failures *inside* the execution loop surface through
    /// [`VM::stop`], which awaits the loop and propagates its result.
    pub async fn launch(self: &Arc<Self>) -> Result<()> {
        self.provision().await?;
        self.start().await?;

        let vm = Arc::clone(self);
        let handle = tokio::spawn(async move {
            match vm.run().await {
                // A `stop()` that lands before the spawned loop is scheduled
                // leaves it starting against a VM that is already stopped.
                // That is the stop working, not a failure to report.
                Err(Error::InvalidState(reason)) if vm.state() != VMState::Running => {
                    tracing::debug!(
                        "VM '{}' stopped before its loop ran: {reason}",
                        vm.config.name
                    );
                    Ok(())
                }
                other => other,
            }
        });
        *self.run_task.write() = Some(handle);

        Ok(())
    }

    /// Start the VM
    pub async fn start(&self) -> Result<()> {
        let old_state = {
            let mut state = self.state.write();

            if *state != VMState::Created && *state != VMState::Stopped {
                return Err(Error::InvalidState(format!(
                    "Cannot start VM in state {:?}",
                    *state
                )));
            }

            let old = *state;
            *state = VMState::Running;
            old
        };

        // Set running flag
        self.running.store(true, Ordering::SeqCst);

        tracing::info!(
            "Starting VM '{}' with {} vCPUs and {} GB memory (parallel={})",
            self.config.name,
            self.config.vcpu_count,
            self.config.memory_size / (1024 * 1024 * 1024),
            self.config.parallel_vcpu
        );

        // Emit state change event
        self.event_bus.publish(VmEvent::state_changed(
            self.config.name.clone(),
            old_state,
            VMState::Running,
        ));

        Ok(())
    }

    /// Pause the VM
    pub async fn pause(&self) -> Result<()> {
        let mut state = self.state.write();

        if *state != VMState::Running {
            return Err(Error::InvalidState(format!(
                "Cannot pause VM in state {:?}",
                *state
            )));
        }

        // Pause all vCPUs
        for vcpu in &self.vcpus {
            vcpu.pause()?;
        }

        *state = VMState::Paused;
        tracing::info!("VM '{}' paused", self.config.name);

        Ok(())
    }

    /// Resume the VM
    pub async fn resume(&self) -> Result<()> {
        let mut state = self.state.write();

        if *state != VMState::Paused {
            return Err(Error::InvalidState(format!(
                "Cannot resume VM in state {:?}",
                *state
            )));
        }

        *state = VMState::Running;
        tracing::info!("VM '{}' resumed", self.config.name);

        Ok(())
    }

    /// Stop the VM
    pub async fn stop(&self) -> Result<()> {
        // Check if already stopped (don't hold guard across await)
        {
            let state = self.state.read();
            if *state == VMState::Stopped {
                return Ok(());
            }
        }

        // Clear running flag first to signal vCPU tasks to stop
        self.running.store(false, Ordering::SeqCst);

        // Send stop messages to all vCPU tasks
        {
            let tasks = self.vcpu_tasks.read();
            for task in tasks.iter() {
                let _ = task.tx.try_send(VCpuMessage::Stop);
            }
        }

        // Collect task handles (drop the lock before awaiting)
        let handles: Vec<_> = {
            let mut tasks = self.vcpu_tasks.write();
            tasks.drain(..).map(|t| t.handle).collect()
        };

        // Wait for all vCPU tasks to complete (no lock held)
        for handle in handles {
            if let Err(e) = handle.await {
                tracing::warn!("vCPU task join error: {:?}", e);
            }
        }

        // Stop all vCPUs
        for vcpu in &self.vcpus {
            vcpu.stop()?;
        }

        // Update state (brief lock acquisition)
        {
            let mut state = self.state.write();
            *state = VMState::Stopped;
        }
        self.exit_notify.notify_waiters();

        // Reap the background execution loop, if this VM was launched. This
        // must come *after* `notify_waiters`: in parallel mode the loop is
        // parked on that notification, so awaiting it any earlier would
        // deadlock against the notification stop() has not yet sent.
        //
        // A guest that never takes a VM exit would park the loop indefinitely,
        // so the wait is bounded — stop() stays responsive and the task is
        // abandoned rather than allowed to hang its caller.
        let run_task = self.run_task.write().take();
        if let Some(handle) = run_task {
            match tokio::time::timeout(RUN_LOOP_REAP_TIMEOUT, handle).await {
                Ok(Ok(Ok(()))) => {}
                Ok(Ok(Err(e))) => {
                    tracing::error!("VM '{}' execution loop failed: {}", self.config.name, e);
                    *self.state.write() = VMState::Error;
                    return Err(e);
                }
                Ok(Err(e)) => tracing::warn!("VM execution task join error: {:?}", e),
                Err(_) => tracing::warn!(
                    "VM '{}' execution loop did not exit within {:?}; abandoning it",
                    self.config.name,
                    RUN_LOOP_REAP_TIMEOUT
                ),
            }
        }

        tracing::info!("VM '{}' stopped", self.config.name);

        Ok(())
    }

    /// Get vCPU by ID
    pub fn vcpu(&self, id: u32) -> Option<Arc<VCpu>> {
        self.vcpus.get(id as usize).cloned()
    }

    /// Get all vCPUs
    pub fn vcpus(&self) -> &[Arc<VCpu>] {
        &self.vcpus
    }

    /// Get guest memory
    pub fn memory(&self) -> Arc<GuestMemory> {
        Arc::clone(&self.memory)
    }

    /// Get device manager
    pub fn devices(&self) -> Arc<DeviceManager> {
        Arc::clone(&self.devices)
    }

    /// Get PIC (interrupt controller)
    pub fn pic(&self) -> Arc<Pic8259> {
        Arc::clone(&self.pic)
    }

    /// Wait for VM exit
    pub async fn wait_for_exit(&self) {
        self.exit_notify.notified().await;
    }

    /// Get the event bus
    pub fn event_bus(&self) -> &EventBus {
        &self.event_bus
    }

    /// Subscribe to VM events
    pub fn subscribe_events(&self) -> tokio::sync::broadcast::Receiver<VmEvent> {
        self.event_bus.subscribe()
    }

    /// Get vCPU statistics
    pub fn vcpu_stats(&self, id: u32) -> Option<Arc<VCpuStats>> {
        self.vcpu_stats.get(id as usize).cloned()
    }

    /// Get all vCPU statistics
    pub fn all_vcpu_stats(&self) -> &[Arc<VCpuStats>] {
        &self.vcpu_stats
    }

    /// Run the VM execution loop
    ///
    /// This is the main execution loop that runs vCPUs and handles VM exits.
    /// It will continue until the VM is stopped or encounters a shutdown exit.
    ///
    /// With `parallel_vcpu` enabled, each vCPU runs in its own tokio task.
    /// Otherwise, a single-threaded round-robin approach is used.
    pub async fn run(&self) -> Result<()> {
        // Ensure VM is in running state
        if self.state() != VMState::Running {
            return Err(Error::InvalidState(format!(
                "Cannot run VM in state {:?}",
                self.state()
            )));
        }

        tracing::info!(
            "Starting VM execution for '{}' with {} vCPUs",
            self.config.name,
            self.config.vcpu_count
        );

        // Pinned vCPUs always use the parallel path, which gives each vCPU its
        // own (optionally core-pinned) thread — even for a single vCPU.
        if (self.config.parallel_vcpu && self.config.vcpu_count > 1)
            || !self.config.vcpu_affinity.is_empty()
        {
            self.run_parallel().await
        } else {
            self.run_single().await
        }
    }

    /// Run VM with single-threaded vCPU execution (round-robin)
    async fn run_single(&self) -> Result<()> {
        let vcpu = self.vcpus[0].clone();
        let stats = self.vcpu_stats[0].clone();

        tracing::info!(
            "Starting single-threaded execution loop for vCPU {}",
            vcpu.id()
        );

        loop {
            // Check if VM should stop
            if !self.running.load(Ordering::SeqCst) {
                tracing::info!("VM stopped, exiting execution loop");
                break;
            }

            // Run vCPU until exit
            let start = std::time::Instant::now();
            let exit = self.backend.run_vcpu(&vcpu).await?;
            stats
                .run_time_ns
                .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
            stats.exits.fetch_add(1, Ordering::Relaxed);

            tracing::debug!("VM exit: {}", exit);

            // Handle the exit
            match self.handle_exit(&vcpu, &stats, exit).await {
                Ok(should_continue) => {
                    if !should_continue {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Error handling VM exit: {}", e);
                    *self.state.write() = VMState::Error;
                    return Err(e);
                }
            }
        }

        tracing::info!("VM execution loop exited for '{}'", self.config.name);
        Ok(())
    }

    /// Run VM with parallel vCPU execution
    async fn run_parallel(&self) -> Result<()> {
        tracing::info!(
            "Starting parallel execution with {} vCPU tasks",
            self.vcpus.len()
        );

        // Create channels and spawn tasks for each vCPU
        let mut task_handles = Vec::with_capacity(self.vcpus.len());

        for (idx, vcpu) in self.vcpus.iter().enumerate() {
            let (tx, rx) = mpsc::channel::<VCpuMessage>(32);
            let handle = self.spawn_vcpu_task(vcpu.clone(), self.vcpu_stats[idx].clone(), rx);

            task_handles.push(VCpuTaskState { tx, handle });
        }

        // Store task handles
        {
            let mut tasks = self.vcpu_tasks.write();
            *tasks = task_handles;
        }

        // Wait for the exit notification
        self.exit_notify.notified().await;

        tracing::info!("VM parallel execution completed for '{}'", self.config.name);

        Ok(())
    }

    /// Spawn a vCPU execution task
    fn spawn_vcpu_task(
        &self,
        vcpu: Arc<VCpu>,
        stats: Arc<VCpuStats>,
        rx: mpsc::Receiver<VCpuMessage>,
    ) -> JoinHandle<Result<()>> {
        let backend = self.backend.clone();
        let running = self.running.clone();
        let state = self.state.clone();
        let exit_notify = self.exit_notify.clone();
        let devices = self.devices.clone();
        let pic = self.pic.clone();
        let memory = self.memory.clone();
        let event_bus = self.event_bus.clone();
        let vm_name = self.config.name.clone();
        let vcpu_id = vcpu.id();
        let core = self.config.affinity_for(vcpu_id);

        match core {
            // Pinned vCPU: run the loop on a dedicated OS thread bound to `core`
            // with its own current-thread runtime, so the vCPU never migrates
            // across cores. A thin tokio task awaits the thread's result, keeping
            // the returned handle type unchanged.
            Some(core) => tokio::spawn(async move {
                let (done_tx, done_rx) = tokio::sync::oneshot::channel();
                let spawned = std::thread::Builder::new()
                    .name(format!("vcpu-{vcpu_id}-core{core}"))
                    .spawn(move || {
                        match crate::cpu_affinity::pin_current_thread(core) {
                            Ok(()) => {
                                tracing::info!("vCPU {vcpu_id} pinned to host core {core}");
                            }
                            Err(e) => {
                                tracing::warn!("vCPU {vcpu_id}: failed to pin to core {core}: {e}");
                            }
                        }
                        let rt = match tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                        {
                            Ok(rt) => rt,
                            Err(e) => {
                                let _ = done_tx.send(Err(Error::Config(format!(
                                    "failed to build vCPU {vcpu_id} runtime: {e}"
                                ))));
                                return;
                            }
                        };
                        let res = rt.block_on(Self::run_vcpu_loop(
                            vcpu,
                            stats,
                            rx,
                            backend,
                            running,
                            state,
                            exit_notify,
                            devices,
                            pic,
                            memory,
                            event_bus,
                            vm_name,
                        ));
                        let _ = done_tx.send(res);
                    });
                if let Err(e) = spawned {
                    return Err(Error::Config(format!(
                        "failed to spawn pinned vCPU {vcpu_id} thread: {e}"
                    )));
                }
                done_rx.await.unwrap_or(Ok(()))
            }),
            // Unpinned vCPU: existing behavior on the shared tokio runtime.
            None => tokio::spawn(Self::run_vcpu_loop(
                vcpu,
                stats,
                rx,
                backend,
                running,
                state,
                exit_notify,
                devices,
                pic,
                memory,
                event_bus,
                vm_name,
            )),
        }
    }

    /// The vCPU execution loop, factored out so it can run either as a tokio
    /// task (unpinned) or via `block_on` on a dedicated, core-pinned thread.
    #[allow(clippy::too_many_arguments)]
    async fn run_vcpu_loop(
        vcpu: Arc<VCpu>,
        stats: Arc<VCpuStats>,
        mut rx: mpsc::Receiver<VCpuMessage>,
        backend: Arc<dyn HypervisorBackend>,
        running: Arc<AtomicBool>,
        state: Arc<RwLock<VMState>>,
        exit_notify: Arc<Notify>,
        devices: Arc<DeviceManager>,
        pic: Arc<Pic8259>,
        memory: Arc<GuestMemory>,
        event_bus: EventBus,
        vm_name: String,
    ) -> Result<()> {
        tracing::info!("vCPU {} task started", vcpu.id());
        let mut paused = false;

        loop {
            // Check for control messages (non-blocking)
            match rx.try_recv() {
                Ok(VCpuMessage::Stop) => {
                    tracing::debug!("vCPU {} received stop", vcpu.id());
                    break;
                }
                Ok(VCpuMessage::Pause) => {
                    tracing::debug!("vCPU {} paused", vcpu.id());
                    paused = true;
                    continue;
                }
                Ok(VCpuMessage::Resume) => {
                    tracing::debug!("vCPU {} resumed", vcpu.id());
                    paused = false;
                }
                Ok(VCpuMessage::Interrupt { vector }) => {
                    tracing::debug!("vCPU {} injecting interrupt {}", vcpu.id(), vector);
                    if let Err(e) = backend.inject_interrupt(&vcpu, vector).await {
                        tracing::warn!("Failed to inject interrupt: {}", e);
                    }
                    stats.interrupts.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    tracing::debug!("vCPU {} channel disconnected", vcpu.id());
                    break;
                }
            }

            // If paused, wait for resume message
            if paused {
                match rx.recv().await {
                    Some(VCpuMessage::Resume) => paused = false,
                    Some(VCpuMessage::Stop) | None => break,
                    _ => continue,
                }
            }

            // Check if VM should stop
            if !running.load(Ordering::SeqCst) {
                break;
            }

            // Run vCPU until exit
            let start = std::time::Instant::now();
            let exit = match backend.run_vcpu(&vcpu).await {
                Ok(exit) => exit,
                Err(e) => {
                    tracing::error!("vCPU {} run error: {}", vcpu.id(), e);
                    *state.write() = VMState::Error;
                    exit_notify.notify_waiters();
                    return Err(e);
                }
            };
            stats
                .run_time_ns
                .fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
            stats.exits.fetch_add(1, Ordering::Relaxed);

            // Handle the exit
            let should_continue = Self::handle_exit_static(
                &vcpu,
                &stats,
                exit,
                &vm_name,
                &devices,
                &pic,
                &memory,
                backend.as_ref(),
                &event_bus,
                &state,
                &exit_notify,
            )
            .await?;

            if !should_continue {
                running.store(false, Ordering::SeqCst);
                exit_notify.notify_waiters();
                break;
            }
        }

        tracing::info!("vCPU {} task exited (exits={})", vcpu.id(), stats.exits());
        Ok(())
    }

    /// Static version of handle_exit for use in spawned tasks
    async fn handle_exit_static(
        vcpu: &VCpu,
        stats: &VCpuStats,
        exit: VmExit,
        vm_name: &str,
        devices: &DeviceManager,
        pic: &Pic8259,
        _memory: &GuestMemory,
        backend: &dyn HypervisorBackend,
        event_bus: &EventBus,
        state: &RwLock<VMState>,
        exit_notify: &Notify,
    ) -> Result<bool> {
        match exit {
            VmExit::Mmio {
                phys_addr,
                mut data,
                len,
                is_write,
            } => {
                stats.mmio_exits.fetch_add(1, Ordering::Relaxed);
                Self::handle_mmio_static(
                    phys_addr, &mut data, len, is_write, vm_name, devices, event_bus,
                )
                .await?;
                Ok(true)
            }

            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                stats.io_exits.fetch_add(1, Ordering::Relaxed);
                let io_result = Self::handle_io_static(
                    port, direction, size, data, vm_name, devices, pic, event_bus,
                )
                .await?;

                // Write IO IN data back to guest RAX
                if let Some((in_data, in_size)) = io_result {
                    backend.set_io_result(vcpu, in_data, in_size).await?;
                }

                Ok(true)
            }

            VmExit::Hlt => {
                Self::handle_hlt_static(vcpu, stats, vm_name, pic, backend, event_bus).await?;
                Ok(true)
            }

            VmExit::Shutdown => {
                tracing::info!("Guest initiated shutdown");
                *state.write() = VMState::Stopped;
                exit_notify.notify_waiters();
                Ok(false)
            }

            VmExit::InterruptWindow => {
                Self::handle_interrupt_window_static(vcpu, stats, vm_name, pic, backend, event_bus)
                    .await?;
                Ok(true)
            }

            VmExit::Exception { vector, error_code } => {
                tracing::warn!(
                    "Guest exception: vector={} error_code={:?}",
                    vector,
                    error_code
                );

                // Fatal exceptions (double fault, triple fault) should stop the VM
                if vector == 8 {
                    tracing::error!("Double fault — stopping VM");
                    *state.write() = VMState::Stopped;
                    exit_notify.notify_waiters();
                    return Ok(false);
                }

                // Re-inject the exception into the guest so its IDT handler runs
                backend.inject_exception(vcpu, vector, error_code).await?;
                Ok(true)
            }

            VmExit::Debug { info } => {
                tracing::debug!("Debug exit: {}", info);
                Ok(true)
            }

            VmExit::Unknown { reason } => {
                tracing::warn!("Unknown VM exit reason: {}", reason);
                Ok(true)
            }

            VmExit::Hypercall { nr, .. } => {
                tracing::debug!("Hypercall nr={:#x}", nr);
                Ok(true)
            }

            VmExit::SystemEvent { type_, flags } => {
                tracing::info!("System event: type={} flags={:#x}", type_, flags);
                *state.write() = VMState::Stopped;
                exit_notify.notify_waiters();
                Ok(false)
            }

            VmExit::Nmi => {
                tracing::debug!("NMI received");
                Ok(true)
            }

            VmExit::Rdmsr { index } => {
                tracing::debug!("RDMSR index={:#x}", index);
                Ok(true)
            }

            VmExit::Wrmsr { index, data } => {
                tracing::debug!("WRMSR index={:#x} data={:#x}", index, data);
                Ok(true)
            }

            VmExit::IoapicEoi { vector } => {
                tracing::debug!("IOAPIC EOI vector={}", vector);
                Ok(true)
            }
        }
    }

    /// Static MMIO handler
    async fn handle_mmio_static(
        phys_addr: u64,
        data: &mut [u8; 8],
        len: u32,
        is_write: bool,
        vm_name: &str,
        devices: &DeviceManager,
        event_bus: &EventBus,
    ) -> Result<()> {
        if is_write {
            tracing::debug!(
                "MMIO write: addr={:#x} data={:?} len={}",
                phys_addr,
                &data[..len as usize],
                len
            );

            if let Some(device) = devices.find_mmio_device(phys_addr).await {
                let offset = phys_addr - device.base_address();
                let value = match len {
                    1 => data[0] as u32,
                    2 => u16::from_le_bytes([data[0], data[1]]) as u32,
                    4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                    8 => {
                        let low = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        device.write_register(offset, low).await?;
                        let high = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                        device.write_register(offset + 4, high).await?;
                        return Ok(());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                };

                device.write_register(offset, value).await?;

                event_bus.publish(VmEvent::memory_access(
                    vm_name.to_string(),
                    phys_addr,
                    len as u64,
                    true,
                ));
            } else {
                tracing::warn!("MMIO write to unmapped address: {:#x}", phys_addr);
            }
        } else {
            tracing::debug!("MMIO read: addr={:#x} len={}", phys_addr, len);

            if let Some(device) = devices.find_mmio_device(phys_addr).await {
                let offset = phys_addr - device.base_address();
                let value = device.read_register(offset).await?;

                match len {
                    1 => data[0] = value as u8,
                    2 => {
                        let bytes = (value as u16).to_le_bytes();
                        data[..2].copy_from_slice(&bytes);
                    }
                    4 => {
                        let bytes = value.to_le_bytes();
                        data[..4].copy_from_slice(&bytes);
                    }
                    8 => {
                        let low = device.read_register(offset).await?;
                        let high = device.read_register(offset + 4).await?;
                        data[..4].copy_from_slice(&low.to_le_bytes());
                        data[4..8].copy_from_slice(&high.to_le_bytes());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                }

                event_bus.publish(VmEvent::memory_access(
                    vm_name.to_string(),
                    phys_addr,
                    len as u64,
                    false,
                ));
            } else {
                tracing::warn!("MMIO read from unmapped address: {:#x}", phys_addr);
                data[..len as usize].fill(0xFF);
            }
        }

        Ok(())
    }

    /// Static I/O handler
    ///
    /// Returns `Some((data, size))` for IO IN operations so the caller can
    /// write the result back to guest RAX via `set_io_result()`.
    async fn handle_io_static(
        port: u16,
        direction: IoDirection,
        size: u8,
        mut data: u32,
        vm_name: &str,
        devices: &DeviceManager,
        pic: &Pic8259,
        event_bus: &EventBus,
    ) -> Result<Option<(u32, u8)>> {
        match direction {
            IoDirection::Out => {
                tracing::debug!("IO OUT: port={:#x} data={:#x}", port, data);

                if pic.handles_port(port) {
                    pic.write_port(port, data as u8).await?;
                } else if let Some(device) = devices.find_io_device(port).await {
                    let offset = (port - device.base_port()) as u64;
                    device.write_register(offset, data).await?;
                } else {
                    tracing::debug!("IO OUT to unhandled port: {:#x}", port);
                }

                event_bus.publish(VmEvent::io_operation(vm_name.to_string(), port, true));
                Ok(None)
            }

            IoDirection::In => {
                tracing::debug!("IO IN: port={:#x}", port);

                if pic.handles_port(port) {
                    data = pic.read_port(port).await? as u32;
                } else if let Some(device) = devices.find_io_device(port).await {
                    let offset = (port - device.base_port()) as u64;
                    data = device.read_register(offset).await?;
                } else {
                    tracing::debug!("IO IN from unhandled port: {:#x}", port);
                    data = 0xFF;
                }

                event_bus.publish(VmEvent::io_operation(vm_name.to_string(), port, false));
                tracing::debug!("IO IN result: {:#x}", data);
                Ok(Some((data, size)))
            }
        }
    }

    /// Static HLT handler
    async fn handle_hlt_static(
        vcpu: &VCpu,
        stats: &VCpuStats,
        vm_name: &str,
        pic: &Pic8259,
        backend: &dyn HypervisorBackend,
        event_bus: &EventBus,
    ) -> Result<()> {
        tracing::debug!("HLT: vCPU {} halted, checking for interrupts", vcpu.id());

        if let Some(vector) = pic.get_pending_interrupt() {
            tracing::debug!("Injecting pending interrupt: vector {:#x}", vector);
            backend.inject_interrupt(vcpu, vector).await?;
            pic.acknowledge_interrupt(vector)?;
            stats.interrupts.fetch_add(1, Ordering::Relaxed);

            event_bus.publish(VmEvent::device_interrupt(
                vm_name.to_string(),
                vector as u32,
            ));
        } else {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        Ok(())
    }

    /// Static interrupt window handler
    async fn handle_interrupt_window_static(
        vcpu: &VCpu,
        stats: &VCpuStats,
        vm_name: &str,
        pic: &Pic8259,
        backend: &dyn HypervisorBackend,
        event_bus: &EventBus,
    ) -> Result<()> {
        tracing::debug!("Interrupt window opened for vCPU {}", vcpu.id());

        if let Some(vector) = pic.get_pending_interrupt() {
            tracing::debug!("Injecting interrupt: vector {:#x}", vector);
            backend.inject_interrupt(vcpu, vector).await?;
            pic.acknowledge_interrupt(vector)?;
            stats.interrupts.fetch_add(1, Ordering::Relaxed);

            event_bus.publish(VmEvent::device_interrupt(
                vm_name.to_string(),
                vector as u32,
            ));
        }

        Ok(())
    }

    /// Handle a VM exit (instance method for single-threaded mode)
    ///
    /// Returns Ok(true) if execution should continue, Ok(false) if VM should stop.
    async fn handle_exit(&self, vcpu: &VCpu, stats: &VCpuStats, exit: VmExit) -> Result<bool> {
        match exit {
            VmExit::Mmio {
                phys_addr,
                mut data,
                len,
                is_write,
            } => {
                stats.mmio_exits.fetch_add(1, Ordering::Relaxed);
                self.handle_mmio_exit(vcpu, phys_addr, &mut data, len, is_write)
                    .await?;
                Ok(true)
            }

            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                stats.io_exits.fetch_add(1, Ordering::Relaxed);
                self.handle_io_exit(vcpu, port, direction, size, data)
                    .await?;
                Ok(true)
            }

            VmExit::Hlt => {
                self.handle_hlt_exit(vcpu, stats).await?;
                Ok(true)
            }

            VmExit::Shutdown => {
                tracing::info!("Guest initiated shutdown");
                self.stop().await?;
                Ok(false)
            }

            VmExit::InterruptWindow => {
                self.handle_interrupt_window(vcpu, stats).await?;
                Ok(true)
            }

            VmExit::Exception { vector, error_code } => {
                tracing::warn!(
                    "Guest exception: vector={} error_code={:?}",
                    vector,
                    error_code
                );

                // Fatal exceptions (double fault) should stop the VM
                if vector == 8 {
                    tracing::error!("Double fault — stopping VM");
                    return Ok(false);
                }

                // Re-inject the exception into the guest so its IDT handler runs
                self.backend
                    .inject_exception(vcpu, vector, error_code)
                    .await?;
                Ok(true)
            }

            VmExit::Debug { info } => {
                tracing::debug!("Debug exit: {}", info);
                Ok(true)
            }

            VmExit::Unknown { reason } => {
                tracing::warn!("Unknown VM exit reason: {}", reason);
                Ok(true)
            }

            VmExit::Hypercall { nr, .. } => {
                tracing::debug!("Hypercall nr={:#x}", nr);
                Ok(true)
            }

            VmExit::SystemEvent { type_, flags } => {
                tracing::info!("System event: type={} flags={:#x}", type_, flags);
                self.stop().await?;
                Ok(false)
            }

            VmExit::Nmi => {
                tracing::debug!("NMI received");
                Ok(true)
            }

            VmExit::Rdmsr { index } => {
                tracing::debug!("RDMSR index={:#x}", index);
                Ok(true)
            }

            VmExit::Wrmsr { index, data } => {
                tracing::debug!("WRMSR index={:#x} data={:#x}", index, data);
                Ok(true)
            }

            VmExit::IoapicEoi { vector } => {
                tracing::debug!("IOAPIC EOI vector={}", vector);
                Ok(true)
            }
        }
    }

    /// Handle MMIO exit
    async fn handle_mmio_exit(
        &self,
        _vcpu: &VCpu,
        phys_addr: u64,
        data: &mut [u8; 8],
        len: u32,
        is_write: bool,
    ) -> Result<()> {
        if is_write {
            // MMIO write
            tracing::debug!(
                "MMIO write: addr={:#x} data={:?} len={}",
                phys_addr,
                &data[..len as usize],
                len
            );

            // Try to find device handler
            if let Some(device) = self.devices.find_mmio_device(phys_addr).await {
                let offset = phys_addr - device.base_address();
                let value = match len {
                    1 => data[0] as u32,
                    2 => u16::from_le_bytes([data[0], data[1]]) as u32,
                    4 => u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
                    8 => {
                        // For 8-byte writes, we'll do two 4-byte writes
                        let low = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                        device.write_register(offset, low).await?;
                        let high = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
                        device.write_register(offset + 4, high).await?;
                        return Ok(());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                };

                device.write_register(offset, value).await?;

                // Publish event
                self.event_bus.publish(VmEvent::memory_access(
                    self.config.name.clone(),
                    phys_addr,
                    len as u64,
                    true,
                ));
            } else {
                tracing::warn!("MMIO write to unmapped address: {:#x}", phys_addr);
            }
        } else {
            // MMIO read
            tracing::debug!("MMIO read: addr={:#x} len={}", phys_addr, len);

            if let Some(device) = self.devices.find_mmio_device(phys_addr).await {
                let offset = phys_addr - device.base_address();
                let value = device.read_register(offset).await?;

                // Write value into data buffer
                match len {
                    1 => data[0] = value as u8,
                    2 => {
                        let bytes = (value as u16).to_le_bytes();
                        data[..2].copy_from_slice(&bytes);
                    }
                    4 => {
                        let bytes = value.to_le_bytes();
                        data[..4].copy_from_slice(&bytes);
                    }
                    8 => {
                        // For 8-byte reads, we'll do two 4-byte reads
                        let low = device.read_register(offset).await?;
                        let high = device.read_register(offset + 4).await?;
                        data[..4].copy_from_slice(&low.to_le_bytes());
                        data[4..8].copy_from_slice(&high.to_le_bytes());
                    }
                    _ => return Err(Error::InvalidMemoryAccess { address: phys_addr }),
                }

                // Publish event
                self.event_bus.publish(VmEvent::memory_access(
                    self.config.name.clone(),
                    phys_addr,
                    len as u64,
                    false,
                ));
            } else {
                tracing::warn!("MMIO read from unmapped address: {:#x}", phys_addr);
                // Return 0xFF for unmapped reads
                data[..len as usize].fill(0xFF);
            }
        }

        Ok(())
    }

    /// Handle I/O port exit
    async fn handle_io_exit(
        &self,
        _vcpu: &VCpu,
        port: u16,
        direction: IoDirection,
        size: u8,
        mut data: u32,
    ) -> Result<()> {
        match direction {
            IoDirection::Out => {
                tracing::debug!("IO OUT: port={:#x} data={:#x} size={}", port, data, size);

                // Check if this is a PIC port
                if self.pic.handles_port(port) {
                    self.pic.write_port(port, data as u8).await?;
                } else if let Some(device) = self.devices.find_io_device(port).await {
                    // Device I/O write
                    let offset = (port - device.base_port()) as u64;
                    device.write_register(offset, data).await?;
                } else {
                    tracing::debug!("IO OUT to unhandled port: {:#x}", port);
                }

                // Publish event
                self.event_bus
                    .publish(VmEvent::io_operation(self.config.name.clone(), port, true));
            }

            IoDirection::In => {
                tracing::debug!("IO IN: port={:#x} size={}", port, size);

                // Check if this is a PIC port
                if self.pic.handles_port(port) {
                    data = self.pic.read_port(port).await? as u32;
                } else if let Some(device) = self.devices.find_io_device(port).await {
                    // Device I/O read
                    let offset = (port - device.base_port()) as u64;
                    data = device.read_register(offset).await?;
                } else {
                    tracing::debug!("IO IN from unhandled port: {:#x}", port);
                    data = 0xFF; // Return 0xFF for unmapped ports
                }

                // Publish event
                self.event_bus.publish(VmEvent::io_operation(
                    self.config.name.clone(),
                    port,
                    false,
                ));

                // Write IO IN data back to guest RAX
                self.backend.set_io_result(_vcpu, data, size).await?;
                tracing::debug!("IO IN result: {:#x}", data);
            }
        }

        Ok(())
    }

    /// Handle HLT exit
    async fn handle_hlt_exit(&self, vcpu: &VCpu, stats: &VCpuStats) -> Result<()> {
        tracing::debug!("HLT: vCPU {} halted, checking for interrupts", vcpu.id());

        // Check if there are pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            tracing::debug!("Injecting pending interrupt: vector {:#x}", vector);

            // Inject the interrupt
            self.backend.inject_interrupt(vcpu, vector).await?;

            // Acknowledge the interrupt in PIC
            self.pic.acknowledge_interrupt(vector)?;

            // Update stats
            stats.interrupts.fetch_add(1, Ordering::Relaxed);

            // Publish interrupt event
            self.event_bus.publish(VmEvent::device_interrupt(
                self.config.name.clone(),
                vector as u32,
            ));
        } else {
            // No interrupts pending, sleep briefly
            tokio::time::sleep(Duration::from_millis(1)).await;
        }

        Ok(())
    }

    /// Handle interrupt window exit
    async fn handle_interrupt_window(&self, vcpu: &VCpu, stats: &VCpuStats) -> Result<()> {
        tracing::debug!(
            "Interrupt window opened for vCPU {}, injecting pending interrupts",
            vcpu.id()
        );

        // Check if there are pending interrupts
        if let Some(vector) = self.pic.get_pending_interrupt() {
            tracing::debug!("Injecting interrupt: vector {:#x}", vector);

            // Inject the interrupt
            self.backend.inject_interrupt(vcpu, vector).await?;

            // Acknowledge the interrupt in PIC
            self.pic.acknowledge_interrupt(vector)?;

            // Update stats
            stats.interrupts.fetch_add(1, Ordering::Relaxed);

            // Publish interrupt event
            self.event_bus.publish(VmEvent::device_interrupt(
                self.config.name.clone(),
                vector as u32,
            ));
        }

        Ok(())
    }

    /// Get the hypervisor backend
    pub fn backend(&self) -> Arc<dyn HypervisorBackend> {
        Arc::clone(&self.backend)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a VM for tests that need a real hypervisor backend, returning
    /// `None` when none is available in the environment (e.g. CI or WSL2 where
    /// `/dev/kvm` is not accessible). Callers return early on `None`, matching
    /// the "skip when the backend is unavailable" convention the WHPX and KVM
    /// backend tests already use.
    fn vm_or_skip(config: VMConfig) -> Option<VM> {
        match VM::new(config) {
            Ok(vm) => Some(vm),
            Err(e) => {
                eprintln!("skipping: no hypervisor backend available ({e})");
                None
            }
        }
    }

    #[tokio::test]
    async fn test_vm_creation() {
        let config = VMConfig {
            name: "test-vm".to_string(),
            vcpu_count: 2,
            memory_size: 1024 * 1024 * 1024,
            ..Default::default()
        };

        let Some(vm) = vm_or_skip(config) else {
            return;
        };
        assert_eq!(vm.state(), VMState::Created);
        assert_eq!(vm.vcpus().len(), 2);
        assert_eq!(vm.all_vcpu_stats().len(), 2);
    }

    #[tokio::test]
    async fn test_vm_lifecycle() {
        let config = VMConfig::default();
        let Some(vm) = vm_or_skip(config) else {
            return;
        };

        vm.start().await.unwrap();
        assert_eq!(vm.state(), VMState::Running);

        vm.stop().await.unwrap();
        assert_eq!(vm.state(), VMState::Stopped);
    }

    #[tokio::test]
    async fn test_vcpu_stats() {
        let config = VMConfig {
            vcpu_count: 4,
            ..Default::default()
        };
        let Some(vm) = vm_or_skip(config) else {
            return;
        };

        // Each vCPU should have its own stats
        for i in 0..4 {
            let stats = vm.vcpu_stats(i).expect("Stats should exist");
            assert_eq!(stats.exits(), 0);
            assert_eq!(stats.run_time_ns(), 0);
            assert_eq!(stats.interrupts(), 0);
        }

        // Non-existent vCPU
        assert!(vm.vcpu_stats(10).is_none());
    }

    #[tokio::test]
    async fn test_parallel_config() {
        let config = VMConfig {
            name: "parallel-test".to_string(),
            vcpu_count: 4,
            parallel_vcpu: true,
            ..Default::default()
        };
        assert!(config.parallel_vcpu);
        assert_eq!(config.vcpu_count, 4);

        let Some(vm) = vm_or_skip(config) else {
            return;
        };
        assert_eq!(vm.vcpus().len(), 4);
    }

    #[test]
    fn affinity_empty_is_valid() {
        let config = VMConfig::default();
        assert!(config.validate_affinity().is_ok());
        assert_eq!(config.affinity_for(0), None);
    }

    #[test]
    fn affinity_valid_mapping_passes() {
        // Pin vCPU 0 to host core 0 (always exists); leave vCPU 1 unpinned.
        let config = VMConfig {
            vcpu_count: 2,
            vcpu_affinity: vec![(0, 0)],
            ..Default::default()
        };
        assert!(config.validate_affinity().is_ok());
        assert_eq!(config.affinity_for(0), Some(0));
        assert_eq!(config.affinity_for(1), None);
    }

    #[test]
    fn affinity_rejects_unknown_vcpu() {
        let config = VMConfig {
            vcpu_count: 2,
            vcpu_affinity: vec![(5, 0)],
            ..Default::default()
        };
        assert!(config.validate_affinity().is_err());
    }

    #[test]
    fn affinity_rejects_out_of_range_core() {
        let config = VMConfig {
            vcpu_count: 1,
            vcpu_affinity: vec![(0, 1_000_000)],
            ..Default::default()
        };
        assert!(config.validate_affinity().is_err());
    }

    #[test]
    fn affinity_rejects_duplicate_core() {
        let config = VMConfig {
            vcpu_count: 2,
            vcpu_affinity: vec![(0, 0), (1, 0)],
            ..Default::default()
        };
        assert!(config.validate_affinity().is_err());
    }

    #[test]
    fn affinity_rejects_duplicate_vcpu() {
        let config = VMConfig {
            vcpu_count: 2,
            vcpu_affinity: vec![(0, 0), (0, 1)],
            ..Default::default()
        };
        assert!(config.validate_affinity().is_err());
    }

    #[tokio::test]
    async fn vm_new_rejects_invalid_affinity() {
        let config = VMConfig {
            vcpu_count: 1,
            vcpu_affinity: vec![(7, 0)], // vCPU 7 does not exist
            ..Default::default()
        };
        assert!(VM::new(config).is_err());
    }

    #[tokio::test]
    async fn vm_new_rejects_invalid_memory_numa_node() {
        let config = VMConfig {
            vcpu_count: 1,
            memory_numa_node: Some(9_999), // no such NUMA node
            ..Default::default()
        };
        assert!(VM::new(config).is_err());
    }

    #[test]
    fn resolve_memory_node_prefers_explicit() {
        let config = VMConfig {
            vcpu_count: 1,
            vcpu_affinity: vec![(0, 0)],
            memory_numa_node: Some(2),
            ..Default::default()
        };
        assert_eq!(config.resolve_memory_node(), Some(2));
    }

    #[test]
    fn resolve_memory_node_none_without_affinity_or_explicit() {
        assert_eq!(VMConfig::default().resolve_memory_node(), None);
    }

    #[test]
    fn resolve_memory_node_derives_from_pinned_core() {
        let config = VMConfig {
            vcpu_count: 1,
            vcpu_affinity: vec![(0, 0)],
            ..Default::default()
        };
        // With no explicit node, derivation follows the pinned core's host node.
        assert_eq!(
            config.resolve_memory_node(),
            crate::cpu_affinity::numa_node_for_core(0)
        );
    }
}
