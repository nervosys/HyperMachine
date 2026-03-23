//! OCI Container Runtime Interface
//!
//! This module provides OCI-compatible container runtime support for
//! container-optimized VM execution.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Container state as defined by OCI runtime spec
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// Container is being created
    Creating,
    /// Container has been created but not started
    Created,
    /// Container is running
    Running,
    /// Container has stopped
    Stopped,
    /// Container is paused
    Paused,
}

impl ContainerState {
    /// Check if container can be started
    pub fn can_start(&self) -> bool {
        matches!(self, Self::Created)
    }

    /// Check if container can be stopped
    pub fn can_stop(&self) -> bool {
        matches!(self, Self::Running | Self::Paused)
    }

    /// Check if container can be paused
    pub fn can_pause(&self) -> bool {
        matches!(self, Self::Running)
    }

    /// Check if container can be resumed
    pub fn can_resume(&self) -> bool {
        matches!(self, Self::Paused)
    }

    /// Check if container can be deleted
    pub fn can_delete(&self) -> bool {
        matches!(self, Self::Stopped | Self::Created)
    }
}

/// Container process information
#[derive(Debug, Clone)]
pub struct ContainerProcess {
    /// Process ID (inside container)
    pub pid: u32,
    /// Process command
    pub command: Vec<String>,
    /// Working directory
    pub cwd: PathBuf,
    /// Environment variables
    pub env: HashMap<String, String>,
    /// User ID
    pub uid: u32,
    /// Group ID
    pub gid: u32,
    /// Terminal attached
    pub terminal: bool,
}

impl Default for ContainerProcess {
    fn default() -> Self {
        Self {
            pid: 1,
            command: vec!["/bin/sh".to_string()],
            cwd: PathBuf::from("/"),
            env: HashMap::new(),
            uid: 0,
            gid: 0,
            terminal: false,
        }
    }
}

/// Mount point specification
#[derive(Debug, Clone)]
pub struct Mount {
    /// Source path on host
    pub source: PathBuf,
    /// Destination path in container
    pub destination: PathBuf,
    /// Mount type (bind, tmpfs, etc.)
    pub mount_type: MountType,
    /// Mount options
    pub options: Vec<MountOption>,
}

/// Mount type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountType {
    /// Bind mount
    Bind,
    /// tmpfs mount
    Tmpfs,
    /// proc filesystem
    Proc,
    /// sysfs filesystem
    Sysfs,
    /// devpts filesystem
    Devpts,
    /// cgroup filesystem
    Cgroup,
    /// mqueue filesystem
    Mqueue,
}

/// Mount option
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountOption {
    /// Read-only
    ReadOnly,
    /// No setuid
    NoSuid,
    /// No device files
    NoDev,
    /// No execute
    NoExec,
    /// Recursive bind
    Rbind,
    /// Private mount
    Private,
    /// Slave mount
    Slave,
    /// Shared mount
    Shared,
}

/// Container root filesystem
#[derive(Debug, Clone)]
pub struct RootFs {
    /// Path to root filesystem
    pub path: PathBuf,
    /// Read-only flag
    pub readonly: bool,
}

/// OCI container specification
#[derive(Debug, Clone)]
pub struct ContainerSpec {
    /// OCI spec version
    pub oci_version: String,
    /// Container hostname
    pub hostname: String,
    /// Root filesystem
    pub root: RootFs,
    /// Process to run
    pub process: ContainerProcess,
    /// Mount points
    pub mounts: Vec<Mount>,
    /// Linux-specific configuration
    pub linux: Option<LinuxConfig>,
    /// Annotations
    pub annotations: HashMap<String, String>,
}

impl Default for ContainerSpec {
    fn default() -> Self {
        Self {
            oci_version: "1.0.2".to_string(),
            hostname: "container".to_string(),
            root: RootFs {
                path: PathBuf::from("/"),
                readonly: false,
            },
            process: ContainerProcess::default(),
            mounts: Vec::new(),
            linux: Some(LinuxConfig::default()),
            annotations: HashMap::new(),
        }
    }
}

/// Linux-specific container configuration
#[derive(Debug, Clone, Default)]
pub struct LinuxConfig {
    /// Namespaces to create/join
    pub namespaces: Vec<NamespaceConfig>,
    /// UID mappings
    pub uid_mappings: Vec<IdMapping>,
    /// GID mappings
    pub gid_mappings: Vec<IdMapping>,
    /// Resource limits (cgroups)
    pub resources: Option<ResourceConfig>,
    /// Seccomp configuration
    pub seccomp: Option<SeccompConfig>,
    /// Masked paths
    pub masked_paths: Vec<PathBuf>,
    /// Readonly paths
    pub readonly_paths: Vec<PathBuf>,
}

/// Namespace configuration
#[derive(Debug, Clone)]
pub struct NamespaceConfig {
    /// Namespace type
    pub ns_type: NamespaceType,
    /// Path to existing namespace (for joining)
    pub path: Option<PathBuf>,
}

/// Namespace types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamespaceType {
    /// PID namespace
    Pid,
    /// Network namespace
    Network,
    /// Mount namespace
    Mount,
    /// UTS namespace (hostname)
    Uts,
    /// IPC namespace
    Ipc,
    /// User namespace
    User,
    /// Cgroup namespace
    Cgroup,
    /// Time namespace
    Time,
}

impl NamespaceType {
    /// Get clone flag for this namespace
    pub fn clone_flag(&self) -> u32 {
        match self {
            Self::Pid => 0x20000000,     // CLONE_NEWPID
            Self::Network => 0x40000000, // CLONE_NEWNET
            Self::Mount => 0x00020000,   // CLONE_NEWNS
            Self::Uts => 0x04000000,     // CLONE_NEWUTS
            Self::Ipc => 0x08000000,     // CLONE_NEWIPC
            Self::User => 0x10000000,    // CLONE_NEWUSER
            Self::Cgroup => 0x02000000,  // CLONE_NEWCGROUP
            Self::Time => 0x00000080,    // CLONE_NEWTIME
        }
    }

    /// Get namespace file name in `/proc/<pid>/ns/`
    pub fn proc_name(&self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Network => "net",
            Self::Mount => "mnt",
            Self::Uts => "uts",
            Self::Ipc => "ipc",
            Self::User => "user",
            Self::Cgroup => "cgroup",
            Self::Time => "time",
        }
    }
}

/// ID mapping for user namespaces
#[derive(Debug, Clone, Copy)]
pub struct IdMapping {
    /// ID inside container
    pub container_id: u32,
    /// ID on host
    pub host_id: u32,
    /// Range size
    pub size: u32,
}

impl IdMapping {
    /// Create identity mapping
    pub fn identity(size: u32) -> Self {
        Self {
            container_id: 0,
            host_id: 0,
            size,
        }
    }

    /// Map container ID to host ID
    pub fn map_to_host(&self, container_id: u32) -> Option<u32> {
        if container_id >= self.container_id && container_id < self.container_id + self.size {
            Some(self.host_id + (container_id - self.container_id))
        } else {
            None
        }
    }

    /// Map host ID to container ID
    pub fn map_to_container(&self, host_id: u32) -> Option<u32> {
        if host_id >= self.host_id && host_id < self.host_id + self.size {
            Some(self.container_id + (host_id - self.host_id))
        } else {
            None
        }
    }
}

/// Resource configuration (cgroup limits)
#[derive(Debug, Clone, Default)]
pub struct ResourceConfig {
    /// CPU limits
    pub cpu: Option<CpuConfig>,
    /// Memory limits
    pub memory: Option<MemoryConfig>,
    /// Block I/O limits
    pub block_io: Option<BlockIoConfig>,
    /// Process limits
    pub pids: Option<PidsConfig>,
}

/// CPU resource configuration
#[derive(Debug, Clone)]
pub struct CpuConfig {
    /// CPU shares (relative weight)
    pub shares: u64,
    /// CPU quota (microseconds per period)
    pub quota: i64,
    /// CPU period (microseconds)
    pub period: u64,
    /// Realtime runtime (microseconds)
    pub realtime_runtime: i64,
    /// Realtime period (microseconds)
    pub realtime_period: u64,
    /// CPUs to use (e.g., "0-3" or "0,2")
    pub cpus: String,
    /// Memory nodes to use
    pub mems: String,
}

impl Default for CpuConfig {
    fn default() -> Self {
        Self {
            shares: 1024,
            quota: -1,
            period: 100_000,
            realtime_runtime: 0,
            realtime_period: 1_000_000,
            cpus: String::new(),
            mems: String::new(),
        }
    }
}

/// Memory resource configuration
#[derive(Debug, Clone)]
pub struct MemoryConfig {
    /// Memory limit in bytes
    pub limit: i64,
    /// Memory reservation (soft limit)
    pub reservation: i64,
    /// Memory + swap limit
    pub swap: i64,
    /// Kernel memory limit
    pub kernel: i64,
    /// Kernel TCP buffer limit
    pub kernel_tcp: i64,
    /// OOM score adjustment
    pub oom_score_adj: i32,
    /// Disable OOM killer
    pub disable_oom_killer: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            limit: -1,
            reservation: -1,
            swap: -1,
            kernel: -1,
            kernel_tcp: -1,
            oom_score_adj: 0,
            disable_oom_killer: false,
        }
    }
}

/// Block I/O configuration
#[derive(Debug, Clone, Default)]
pub struct BlockIoConfig {
    /// I/O weight (10-1000)
    pub weight: u16,
    /// Per-device weight
    pub weight_device: Vec<WeightDevice>,
    /// Read BPS limit per device
    pub throttle_read_bps_device: Vec<ThrottleDevice>,
    /// Write BPS limit per device
    pub throttle_write_bps_device: Vec<ThrottleDevice>,
    /// Read IOPS limit per device
    pub throttle_read_iops_device: Vec<ThrottleDevice>,
    /// Write IOPS limit per device
    pub throttle_write_iops_device: Vec<ThrottleDevice>,
}

/// Per-device weight
#[derive(Debug, Clone)]
pub struct WeightDevice {
    /// Device major number
    pub major: u64,
    /// Device minor number
    pub minor: u64,
    /// Weight
    pub weight: u16,
}

/// Per-device throttle
#[derive(Debug, Clone)]
pub struct ThrottleDevice {
    /// Device major number
    pub major: u64,
    /// Device minor number
    pub minor: u64,
    /// Rate limit
    pub rate: u64,
}

/// Process limits configuration
#[derive(Debug, Clone)]
pub struct PidsConfig {
    /// Maximum number of processes
    pub limit: i64,
}

impl Default for PidsConfig {
    fn default() -> Self {
        Self { limit: -1 }
    }
}

/// Seccomp configuration
#[derive(Debug, Clone)]
pub struct SeccompConfig {
    /// Default action
    pub default_action: SeccompAction,
    /// Architectures
    pub architectures: Vec<String>,
    /// Syscall rules
    pub syscalls: Vec<SeccompSyscall>,
}

impl Default for SeccompConfig {
    fn default() -> Self {
        Self {
            default_action: SeccompAction::Allow,
            architectures: vec!["SCMP_ARCH_X86_64".to_string()],
            syscalls: Vec::new(),
        }
    }
}

/// Seccomp action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompAction {
    /// Allow syscall
    Allow,
    /// Return error
    Errno(u32),
    /// Kill thread
    Kill,
    /// Kill process
    KillProcess,
    /// Log and allow
    Log,
    /// Trace
    Trace(u32),
    /// Trap
    Trap,
}

/// Seccomp syscall rule
#[derive(Debug, Clone)]
pub struct SeccompSyscall {
    /// Syscall names
    pub names: Vec<String>,
    /// Action to take
    pub action: SeccompAction,
    /// Argument conditions
    pub args: Vec<SeccompArg>,
}

/// Seccomp argument condition
#[derive(Debug, Clone)]
pub struct SeccompArg {
    /// Argument index
    pub index: u32,
    /// Value to compare
    pub value: u64,
    /// Second value (for masked equal)
    pub value_two: u64,
    /// Comparison operator
    pub op: SeccompOp,
}

/// Seccomp comparison operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompOp {
    /// Not equal
    NotEqual,
    /// Less than
    LessThan,
    /// Less than or equal
    LessEqual,
    /// Equal
    Equal,
    /// Greater than or equal
    GreaterEqual,
    /// Greater than
    GreaterThan,
    /// Masked equal
    MaskedEqual,
}

/// Container runtime error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    /// Container not found
    NotFound(String),
    /// Container already exists
    AlreadyExists(String),
    /// Invalid state transition
    InvalidState(ContainerState, &'static str),
    /// Spec validation failed
    InvalidSpec(String),
    /// Namespace error
    NamespaceError(String),
    /// Resource limit error
    ResourceError(String),
    /// I/O error
    IoError(String),
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "Container not found: {}", id),
            Self::AlreadyExists(id) => write!(f, "Container already exists: {}", id),
            Self::InvalidState(state, op) => {
                write!(f, "Cannot {} container in {:?} state", op, state)
            }
            Self::InvalidSpec(msg) => write!(f, "Invalid spec: {}", msg),
            Self::NamespaceError(msg) => write!(f, "Namespace error: {}", msg),
            Self::ResourceError(msg) => write!(f, "Resource error: {}", msg),
            Self::IoError(msg) => write!(f, "I/O error: {}", msg),
        }
    }
}

impl std::error::Error for RuntimeError {}

/// Result type for runtime operations
pub type RuntimeResult<T> = Result<T, RuntimeError>;

/// Container instance
#[derive(Debug)]
pub struct Container {
    /// Container ID
    id: String,
    /// Bundle path
    bundle: PathBuf,
    /// Container specification
    spec: ContainerSpec,
    /// Current state
    state: RwLock<ContainerState>,
    /// Process ID on host
    pid: RwLock<Option<u32>>,
    /// Creation time
    created: SystemTime,
    /// Start time
    started: RwLock<Option<SystemTime>>,
    /// Exit code
    exit_code: RwLock<Option<i32>>,
}

impl Container {
    /// Create new container
    pub fn new(id: impl Into<String>, bundle: PathBuf, spec: ContainerSpec) -> Self {
        Self {
            id: id.into(),
            bundle,
            spec,
            state: RwLock::new(ContainerState::Creating),
            pid: RwLock::new(None),
            created: SystemTime::now(),
            started: RwLock::new(None),
            exit_code: RwLock::new(None),
        }
    }

    /// Get container ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get bundle path
    pub fn bundle(&self) -> &PathBuf {
        &self.bundle
    }

    /// Get specification
    pub fn spec(&self) -> &ContainerSpec {
        &self.spec
    }

    /// Get current state
    pub fn state(&self) -> ContainerState {
        *self
            .state
            .read()
            .expect("container state read lock poisoned")
    }

    /// Get host PID
    pub fn pid(&self) -> Option<u32> {
        *self.pid.read().expect("container pid read lock poisoned")
    }

    /// Get creation time
    pub fn created(&self) -> SystemTime {
        self.created
    }

    /// Mark as created
    pub fn mark_created(&self) -> RuntimeResult<()> {
        let mut state = self
            .state
            .write()
            .expect("container state write lock poisoned");
        if *state != ContainerState::Creating {
            return Err(RuntimeError::InvalidState(*state, "create"));
        }
        *state = ContainerState::Created;
        Ok(())
    }

    /// Start container
    pub fn start(&self, pid: u32) -> RuntimeResult<()> {
        let mut state = self
            .state
            .write()
            .expect("container state write lock poisoned");
        if !state.can_start() {
            return Err(RuntimeError::InvalidState(*state, "start"));
        }
        *self.pid.write().expect("container pid write lock poisoned") = Some(pid);
        *self
            .started
            .write()
            .expect("container started write lock poisoned") = Some(SystemTime::now());
        *state = ContainerState::Running;
        Ok(())
    }

    /// Stop container
    pub fn stop(&self, exit_code: i32) -> RuntimeResult<()> {
        let mut state = self
            .state
            .write()
            .expect("container state write lock poisoned");
        if !state.can_stop() {
            return Err(RuntimeError::InvalidState(*state, "stop"));
        }
        *self
            .exit_code
            .write()
            .expect("container exit_code write lock poisoned") = Some(exit_code);
        *state = ContainerState::Stopped;
        Ok(())
    }

    /// Pause container
    pub fn pause(&self) -> RuntimeResult<()> {
        let mut state = self
            .state
            .write()
            .expect("container state write lock poisoned");
        if !state.can_pause() {
            return Err(RuntimeError::InvalidState(*state, "pause"));
        }
        *state = ContainerState::Paused;
        Ok(())
    }

    /// Resume container
    pub fn resume(&self) -> RuntimeResult<()> {
        let mut state = self
            .state
            .write()
            .expect("container state write lock poisoned");
        if !state.can_resume() {
            return Err(RuntimeError::InvalidState(*state, "resume"));
        }
        *state = ContainerState::Running;
        Ok(())
    }

    /// Get exit code
    pub fn exit_code(&self) -> Option<i32> {
        *self
            .exit_code
            .read()
            .expect("container exit_code read lock poisoned")
    }

    /// Get uptime
    pub fn uptime(&self) -> Option<Duration> {
        let started = self
            .started
            .read()
            .expect("container started read lock poisoned");
        started.map(|s| SystemTime::now().duration_since(s).unwrap_or_default())
    }
}

/// Container runtime manager
#[derive(Debug)]
pub struct ContainerRuntime {
    /// Containers by ID
    containers: RwLock<HashMap<String, Arc<Container>>>,
    /// Container counter
    container_count: AtomicU64,
    /// Runtime name
    name: String,
    /// Runtime version
    version: String,
}

impl ContainerRuntime {
    /// Create new runtime
    pub fn new() -> Self {
        Self {
            containers: RwLock::new(HashMap::new()),
            container_count: AtomicU64::new(0),
            name: "aether-runtime".to_string(),
            version: "1.0.0".to_string(),
        }
    }

    /// Get runtime name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get runtime version
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Create a container
    pub fn create(
        &self,
        id: impl Into<String>,
        bundle: PathBuf,
        spec: ContainerSpec,
    ) -> RuntimeResult<Arc<Container>> {
        let id = id.into();

        // Check if container already exists
        {
            let containers = self
                .containers
                .read()
                .expect("containers read lock poisoned");
            if containers.contains_key(&id) {
                return Err(RuntimeError::AlreadyExists(id));
            }
        }

        // Validate spec
        self.validate_spec(&spec)?;

        // Create container
        let container = Arc::new(Container::new(id.clone(), bundle, spec));
        container.mark_created()?;

        // Store container
        {
            let mut containers = self
                .containers
                .write()
                .expect("containers write lock poisoned");
            containers.insert(id, container.clone());
        }

        self.container_count.fetch_add(1, Ordering::Relaxed);
        Ok(container)
    }

    /// Get container by ID
    pub fn get(&self, id: &str) -> RuntimeResult<Arc<Container>> {
        let containers = self
            .containers
            .read()
            .expect("containers read lock poisoned");
        containers
            .get(id)
            .cloned()
            .ok_or_else(|| RuntimeError::NotFound(id.to_string()))
    }

    /// Start a container
    pub fn start(&self, id: &str) -> RuntimeResult<()> {
        let container = self.get(id)?;
        // In real implementation, would fork/exec and setup namespaces
        // For now, simulate with a PID
        container.start(1000 + self.container_count.load(Ordering::Relaxed) as u32)
    }

    /// Stop a container
    pub fn stop(&self, id: &str, timeout: Duration) -> RuntimeResult<()> {
        let container = self.get(id)?;
        // In real implementation, would send SIGTERM, wait, then SIGKILL
        let _ = timeout;
        container.stop(0)
    }

    /// Kill a container
    pub fn kill(&self, id: &str, signal: i32) -> RuntimeResult<()> {
        let container = self.get(id)?;
        // In real implementation, would send signal to container process
        let _ = signal;
        if container.state() == ContainerState::Running {
            container.stop(128 + signal)?;
        }
        Ok(())
    }

    /// Delete a container
    pub fn delete(&self, id: &str) -> RuntimeResult<()> {
        let container = self.get(id)?;
        let state = container.state();
        if !state.can_delete() {
            return Err(RuntimeError::InvalidState(state, "delete"));
        }

        let mut containers = self
            .containers
            .write()
            .expect("containers write lock poisoned");
        containers.remove(id);
        Ok(())
    }

    /// Pause a container
    pub fn pause(&self, id: &str) -> RuntimeResult<()> {
        let container = self.get(id)?;
        container.pause()
    }

    /// Resume a container
    pub fn resume(&self, id: &str) -> RuntimeResult<()> {
        let container = self.get(id)?;
        container.resume()
    }

    /// List containers
    pub fn list(&self) -> Vec<Arc<Container>> {
        let containers = self
            .containers
            .read()
            .expect("containers read lock poisoned");
        containers.values().cloned().collect()
    }

    /// Get container count
    pub fn count(&self) -> usize {
        self.containers
            .read()
            .expect("containers read lock poisoned")
            .len()
    }

    /// Validate container spec
    fn validate_spec(&self, spec: &ContainerSpec) -> RuntimeResult<()> {
        // Validate OCI version
        if !spec.oci_version.starts_with("1.") {
            return Err(RuntimeError::InvalidSpec(format!(
                "Unsupported OCI version: {}",
                spec.oci_version
            )));
        }

        // Validate process command
        if spec.process.command.is_empty() {
            return Err(RuntimeError::InvalidSpec(
                "Process command cannot be empty".to_string(),
            ));
        }

        Ok(())
    }

    /// Get runtime statistics
    pub fn stats(&self) -> RuntimeStats {
        let containers = self
            .containers
            .read()
            .expect("containers read lock poisoned");
        let mut running = 0;
        let mut stopped = 0;
        let mut paused = 0;
        let mut created = 0;

        for container in containers.values() {
            match container.state() {
                ContainerState::Running => running += 1,
                ContainerState::Stopped => stopped += 1,
                ContainerState::Paused => paused += 1,
                ContainerState::Created | ContainerState::Creating => created += 1,
            }
        }

        RuntimeStats {
            total: containers.len(),
            running,
            stopped,
            paused,
            created,
        }
    }
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime statistics
#[derive(Debug, Clone)]
pub struct RuntimeStats {
    /// Total containers
    pub total: usize,
    /// Running containers
    pub running: usize,
    /// Stopped containers
    pub stopped: usize,
    /// Paused containers
    pub paused: usize,
    /// Created (not started) containers
    pub created: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_state_transitions() {
        assert!(ContainerState::Created.can_start());
        assert!(!ContainerState::Running.can_start());
        assert!(ContainerState::Running.can_stop());
        assert!(ContainerState::Running.can_pause());
        assert!(ContainerState::Paused.can_resume());
        assert!(ContainerState::Stopped.can_delete());
    }

    #[test]
    fn test_namespace_type_flags() {
        assert_eq!(NamespaceType::Pid.clone_flag(), 0x20000000);
        assert_eq!(NamespaceType::Network.clone_flag(), 0x40000000);
        assert_eq!(NamespaceType::Mount.clone_flag(), 0x00020000);
    }

    #[test]
    fn test_namespace_proc_names() {
        assert_eq!(NamespaceType::Pid.proc_name(), "pid");
        assert_eq!(NamespaceType::Network.proc_name(), "net");
        assert_eq!(NamespaceType::Mount.proc_name(), "mnt");
    }

    #[test]
    fn test_id_mapping_to_host() {
        let mapping = IdMapping {
            container_id: 0,
            host_id: 1000,
            size: 65536,
        };

        assert_eq!(mapping.map_to_host(0), Some(1000));
        assert_eq!(mapping.map_to_host(100), Some(1100));
        assert_eq!(mapping.map_to_host(65536), None);
    }

    #[test]
    fn test_id_mapping_to_container() {
        let mapping = IdMapping {
            container_id: 0,
            host_id: 1000,
            size: 65536,
        };

        assert_eq!(mapping.map_to_container(1000), Some(0));
        assert_eq!(mapping.map_to_container(1100), Some(100));
        assert_eq!(mapping.map_to_container(999), None);
    }

    #[test]
    fn test_id_mapping_identity() {
        let mapping = IdMapping::identity(65536);
        assert_eq!(mapping.container_id, 0);
        assert_eq!(mapping.host_id, 0);
        assert_eq!(mapping.map_to_host(100), Some(100));
    }

    #[test]
    fn test_container_spec_default() {
        let spec = ContainerSpec::default();
        assert_eq!(spec.oci_version, "1.0.2");
        assert_eq!(spec.hostname, "container");
        assert!(spec.linux.is_some());
    }

    #[test]
    fn test_cpu_config_default() {
        let cpu = CpuConfig::default();
        assert_eq!(cpu.shares, 1024);
        assert_eq!(cpu.quota, -1);
        assert_eq!(cpu.period, 100_000);
    }

    #[test]
    fn test_memory_config_default() {
        let mem = MemoryConfig::default();
        assert_eq!(mem.limit, -1);
        assert!(!mem.disable_oom_killer);
    }

    #[test]
    fn test_container_process_default() {
        let proc = ContainerProcess::default();
        assert_eq!(proc.pid, 1);
        assert_eq!(proc.command, vec!["/bin/sh".to_string()]);
        assert_eq!(proc.uid, 0);
    }

    #[test]
    fn test_container_creation() {
        let spec = ContainerSpec::default();
        let container = Container::new("test1", PathBuf::from("/bundle"), spec);

        assert_eq!(container.id(), "test1");
        assert_eq!(container.state(), ContainerState::Creating);
    }

    #[test]
    fn test_container_lifecycle() {
        let spec = ContainerSpec::default();
        let container = Container::new("test1", PathBuf::from("/bundle"), spec);

        container.mark_created().unwrap();
        assert_eq!(container.state(), ContainerState::Created);

        container.start(1000).unwrap();
        assert_eq!(container.state(), ContainerState::Running);
        assert_eq!(container.pid(), Some(1000));

        container.pause().unwrap();
        assert_eq!(container.state(), ContainerState::Paused);

        container.resume().unwrap();
        assert_eq!(container.state(), ContainerState::Running);

        container.stop(0).unwrap();
        assert_eq!(container.state(), ContainerState::Stopped);
        assert_eq!(container.exit_code(), Some(0));
    }

    #[test]
    fn test_container_invalid_transitions() {
        let spec = ContainerSpec::default();
        let container = Container::new("test1", PathBuf::from("/bundle"), spec);
        container.mark_created().unwrap();

        // Can't pause created container
        assert!(container.pause().is_err());

        // Can't stop created container
        assert!(container.stop(0).is_err());

        container.start(1000).unwrap();

        // Can't start running container
        let result = container.start(2000);
        assert!(result.is_err());
    }

    #[test]
    fn test_runtime_creation() {
        let runtime = ContainerRuntime::new();
        assert_eq!(runtime.name(), "aether-runtime");
        assert_eq!(runtime.version(), "1.0.0");
        assert_eq!(runtime.count(), 0);
    }

    #[test]
    fn test_runtime_create_container() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        let container = runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();
        assert_eq!(container.id(), "test1");
        assert_eq!(runtime.count(), 1);
    }

    #[test]
    fn test_runtime_duplicate_container() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec.clone())
            .unwrap();
        let result = runtime.create("test1", PathBuf::from("/bundle"), spec);
        assert!(matches!(result, Err(RuntimeError::AlreadyExists(_))));
    }

    #[test]
    fn test_runtime_get_container() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();

        let container = runtime.get("test1").unwrap();
        assert_eq!(container.id(), "test1");

        let result = runtime.get("nonexistent");
        assert!(matches!(result, Err(RuntimeError::NotFound(_))));
    }

    #[test]
    fn test_runtime_start_stop() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();
        runtime.start("test1").unwrap();

        let container = runtime.get("test1").unwrap();
        assert_eq!(container.state(), ContainerState::Running);

        runtime.stop("test1", Duration::from_secs(10)).unwrap();
        assert_eq!(container.state(), ContainerState::Stopped);
    }

    #[test]
    fn test_runtime_pause_resume() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();
        runtime.start("test1").unwrap();
        runtime.pause("test1").unwrap();

        let container = runtime.get("test1").unwrap();
        assert_eq!(container.state(), ContainerState::Paused);

        runtime.resume("test1").unwrap();
        assert_eq!(container.state(), ContainerState::Running);
    }

    #[test]
    fn test_runtime_delete() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();
        runtime.start("test1").unwrap();

        // Can't delete running container
        let result = runtime.delete("test1");
        assert!(result.is_err());

        runtime.stop("test1", Duration::from_secs(10)).unwrap();
        runtime.delete("test1").unwrap();
        assert_eq!(runtime.count(), 0);
    }

    #[test]
    fn test_runtime_kill() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec)
            .unwrap();
        runtime.start("test1").unwrap();
        runtime.kill("test1", 9).unwrap();

        let container = runtime.get("test1").unwrap();
        assert_eq!(container.state(), ContainerState::Stopped);
        assert_eq!(container.exit_code(), Some(137)); // 128 + 9
    }

    #[test]
    fn test_runtime_list() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec.clone())
            .unwrap();
        runtime
            .create("test2", PathBuf::from("/bundle"), spec)
            .unwrap();

        let list = runtime.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_runtime_stats() {
        let runtime = ContainerRuntime::new();
        let spec = ContainerSpec::default();

        runtime
            .create("test1", PathBuf::from("/bundle"), spec.clone())
            .unwrap();
        runtime
            .create("test2", PathBuf::from("/bundle"), spec.clone())
            .unwrap();
        runtime
            .create("test3", PathBuf::from("/bundle"), spec)
            .unwrap();

        runtime.start("test1").unwrap();
        runtime.start("test2").unwrap();
        runtime.stop("test2", Duration::from_secs(10)).unwrap();

        let stats = runtime.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.stopped, 1);
        assert_eq!(stats.created, 1);
    }

    #[test]
    fn test_invalid_spec_version() {
        let runtime = ContainerRuntime::new();
        let mut spec = ContainerSpec::default();
        spec.oci_version = "0.9.0".to_string();

        let result = runtime.create("test1", PathBuf::from("/bundle"), spec);
        assert!(matches!(result, Err(RuntimeError::InvalidSpec(_))));
    }

    #[test]
    fn test_invalid_spec_empty_command() {
        let runtime = ContainerRuntime::new();
        let mut spec = ContainerSpec::default();
        spec.process.command = Vec::new();

        let result = runtime.create("test1", PathBuf::from("/bundle"), spec);
        assert!(matches!(result, Err(RuntimeError::InvalidSpec(_))));
    }

    #[test]
    fn test_seccomp_config_default() {
        let seccomp = SeccompConfig::default();
        assert_eq!(seccomp.default_action, SeccompAction::Allow);
        assert!(seccomp.syscalls.is_empty());
    }

    #[test]
    fn test_mount_types() {
        let mount = Mount {
            source: PathBuf::from("/host/data"),
            destination: PathBuf::from("/data"),
            mount_type: MountType::Bind,
            options: vec![MountOption::ReadOnly, MountOption::NoDev],
        };

        assert_eq!(mount.mount_type, MountType::Bind);
        assert!(mount.options.contains(&MountOption::ReadOnly));
    }
}
