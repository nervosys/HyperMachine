//! Cgroup Resource Controllers
//!
//! This module provides cgroup v1 and v2 resource controller support
//! for container resource isolation and limits.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Cgroup version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CgroupVersion {
    /// Cgroup v1 (legacy)
    V1,
    /// Cgroup v2 (unified)
    V2,
}

/// Cgroup controller type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Controller {
    /// CPU controller
    Cpu,
    /// CPU accounting
    Cpuacct,
    /// CPU set (pinning)
    Cpuset,
    /// Memory controller
    Memory,
    /// Block I/O controller
    Blkio,
    /// I/O controller (v2)
    Io,
    /// Process ID controller
    Pids,
    /// Devices controller
    Devices,
    /// Freezer controller
    Freezer,
    /// Network classifier
    NetCls,
    /// Network priority
    NetPrio,
    /// Huge pages
    Hugetlb,
    /// Perf events
    PerfEvent,
}

impl Controller {
    /// Get controller name for v1
    pub fn v1_name(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cpuacct => "cpuacct",
            Self::Cpuset => "cpuset",
            Self::Memory => "memory",
            Self::Blkio => "blkio",
            Self::Io => "io",
            Self::Pids => "pids",
            Self::Devices => "devices",
            Self::Freezer => "freezer",
            Self::NetCls => "net_cls",
            Self::NetPrio => "net_prio",
            Self::Hugetlb => "hugetlb",
            Self::PerfEvent => "perf_event",
        }
    }

    /// Get controller name for v2
    pub fn v2_name(&self) -> Option<&'static str> {
        match self {
            Self::Cpu => Some("cpu"),
            Self::Cpuset => Some("cpuset"),
            Self::Memory => Some("memory"),
            Self::Io => Some("io"),
            Self::Pids => Some("pids"),
            Self::Hugetlb => Some("hugetlb"),
            // These don't exist in v2
            Self::Cpuacct
            | Self::Blkio
            | Self::Devices
            | Self::Freezer
            | Self::NetCls
            | Self::NetPrio
            | Self::PerfEvent => None,
        }
    }

    /// Get all v1 controllers
    pub fn v1_all() -> &'static [Self] {
        &[
            Self::Cpu,
            Self::Cpuacct,
            Self::Cpuset,
            Self::Memory,
            Self::Blkio,
            Self::Pids,
            Self::Devices,
            Self::Freezer,
            Self::NetCls,
            Self::NetPrio,
            Self::Hugetlb,
            Self::PerfEvent,
        ]
    }

    /// Get all v2 controllers
    pub fn v2_all() -> &'static [Self] {
        &[
            Self::Cpu,
            Self::Cpuset,
            Self::Memory,
            Self::Io,
            Self::Pids,
            Self::Hugetlb,
        ]
    }
}

/// CPU controller configuration
#[derive(Debug, Clone)]
pub struct CpuController {
    /// CPU shares (relative weight, v1 cpu.shares, v2 cpu.weight)
    pub shares: u64,
    /// CPU quota in microseconds per period (-1 = unlimited)
    pub quota: i64,
    /// CPU period in microseconds
    pub period: u64,
    /// Burst quota (v2 only)
    pub burst: u64,
    /// Realtime runtime in microseconds
    pub rt_runtime: i64,
    /// Realtime period in microseconds
    pub rt_period: u64,
}

impl Default for CpuController {
    fn default() -> Self {
        Self {
            shares: 1024,
            quota: -1,
            period: 100_000,
            burst: 0,
            rt_runtime: 0,
            rt_period: 1_000_000,
        }
    }
}

impl CpuController {
    /// Calculate CPU weight from shares (for v2)
    pub fn weight(&self) -> u64 {
        // Convert shares (2-262144) to weight (1-10000)
        // Default 1024 shares = 100 weight
        ((self.shares.clamp(2, 262144) - 2) * 9999 / 262142 + 1).clamp(1, 10000)
    }

    /// Calculate max (quota/period) string for v2
    pub fn max_string(&self) -> String {
        if self.quota < 0 {
            "max".to_string()
        } else {
            format!("{}", self.quota)
        }
    }

    /// Check if CPU is limited
    pub fn is_limited(&self) -> bool {
        self.quota > 0
    }

    /// Get effective CPU limit as a fraction
    pub fn limit_fraction(&self) -> Option<f64> {
        if self.quota > 0 && self.period > 0 {
            Some(self.quota as f64 / self.period as f64)
        } else {
            None
        }
    }
}

/// Cpuset controller configuration
#[derive(Debug, Clone, Default)]
pub struct CpusetController {
    /// CPUs to use (e.g., "0-3" or "0,2,4")
    pub cpus: String,
    /// Memory nodes to use
    pub mems: String,
    /// CPU exclusive
    pub cpu_exclusive: bool,
    /// Memory exclusive
    pub mem_exclusive: bool,
}

impl CpusetController {
    /// Parse CPU list into individual CPUs
    pub fn parse_cpus(&self) -> Vec<u32> {
        parse_list_format(&self.cpus)
    }

    /// Parse memory node list
    pub fn parse_mems(&self) -> Vec<u32> {
        parse_list_format(&self.mems)
    }

    /// Set CPUs from a list
    pub fn set_cpus(&mut self, cpus: &[u32]) {
        self.cpus = format_list(cpus);
    }

    /// Set memory nodes from a list
    pub fn set_mems(&mut self, mems: &[u32]) {
        self.mems = format_list(mems);
    }
}

/// Parse list format (e.g., "0-3,5,7-9") into individual values
fn parse_list_format(s: &str) -> Vec<u32> {
    let mut result = Vec::new();
    if s.is_empty() {
        return result;
    }

    for part in s.split(',') {
        let part = part.trim();
        if let Some(dash_pos) = part.find('-') {
            if let (Ok(start), Ok(end)) = (
                part[..dash_pos].parse::<u32>(),
                part[dash_pos + 1..].parse::<u32>(),
            ) {
                for i in start..=end {
                    result.push(i);
                }
            }
        } else if let Ok(num) = part.parse::<u32>() {
            result.push(num);
        }
    }
    result.sort();
    result.dedup();
    result
}

/// Format a list of values into range format
fn format_list(values: &[u32]) -> String {
    if values.is_empty() {
        return String::new();
    }

    let mut sorted: Vec<u32> = values.to_vec();
    sorted.sort();
    sorted.dedup();

    let mut result = String::new();
    let mut start = sorted[0];
    let mut end = start;

    for &val in &sorted[1..] {
        if val == end + 1 {
            end = val;
        } else {
            if !result.is_empty() {
                result.push(',');
            }
            if start == end {
                result.push_str(&start.to_string());
            } else {
                result.push_str(&format!("{}-{}", start, end));
            }
            start = val;
            end = val;
        }
    }

    if !result.is_empty() {
        result.push(',');
    }
    if start == end {
        result.push_str(&start.to_string());
    } else {
        result.push_str(&format!("{}-{}", start, end));
    }

    result
}

/// Memory controller configuration
#[derive(Debug, Clone)]
pub struct MemoryController {
    /// Memory limit in bytes (-1 = unlimited)
    pub limit: i64,
    /// Memory soft limit (reservation)
    pub soft_limit: i64,
    /// Memory + swap limit (-1 = unlimited)
    pub swap_limit: i64,
    /// Kernel memory limit (v1 only)
    pub kernel_limit: i64,
    /// OOM score adjustment
    pub oom_score_adj: i32,
    /// Disable OOM killer
    pub oom_kill_disable: bool,
    /// Memory high threshold (v2 only)
    pub high: i64,
    /// Memory low protection (v2 only)
    pub low: i64,
    /// Memory min protection (v2 only)
    pub min: i64,
}

impl Default for MemoryController {
    fn default() -> Self {
        Self {
            limit: -1,
            soft_limit: -1,
            swap_limit: -1,
            kernel_limit: -1,
            oom_score_adj: 0,
            oom_kill_disable: false,
            high: -1,
            low: 0,
            min: 0,
        }
    }
}

impl MemoryController {
    /// Check if memory is limited
    pub fn is_limited(&self) -> bool {
        self.limit > 0
    }

    /// Get limit in human-readable format
    pub fn limit_string(&self) -> String {
        if self.limit < 0 {
            "unlimited".to_string()
        } else {
            format_bytes(self.limit as u64)
        }
    }

    /// Calculate swap limit for v2 (memory.swap.max = swap_limit - limit)
    pub fn swap_max(&self) -> i64 {
        if self.swap_limit < 0 {
            -1
        } else if self.limit > 0 {
            self.swap_limit - self.limit
        } else {
            self.swap_limit
        }
    }
}

/// Format bytes into human-readable string
fn format_bytes(bytes: u64) -> String {
    const KI: u64 = 1024;
    const MI: u64 = KI * 1024;
    const GI: u64 = MI * 1024;
    const TI: u64 = GI * 1024;

    if bytes >= TI {
        format!("{}Ti", bytes / TI)
    } else if bytes >= GI {
        format!("{}Gi", bytes / GI)
    } else if bytes >= MI {
        format!("{}Mi", bytes / MI)
    } else if bytes >= KI {
        format!("{}Ki", bytes / KI)
    } else {
        format!("{}B", bytes)
    }
}

/// I/O controller configuration
#[derive(Debug, Clone, Default)]
pub struct IoController {
    /// I/O weight (10-1000)
    pub weight: u16,
    /// Per-device weights
    pub weight_device: Vec<DeviceWeight>,
    /// Read BPS limits per device
    pub read_bps_device: Vec<DeviceThrottle>,
    /// Write BPS limits per device
    pub write_bps_device: Vec<DeviceThrottle>,
    /// Read IOPS limits per device
    pub read_iops_device: Vec<DeviceThrottle>,
    /// Write IOPS limits per device
    pub write_iops_device: Vec<DeviceThrottle>,
}

/// Per-device weight
#[derive(Debug, Clone)]
pub struct DeviceWeight {
    /// Device major number
    pub major: u64,
    /// Device minor number
    pub minor: u64,
    /// Weight
    pub weight: u16,
}

/// Per-device throttle limit
#[derive(Debug, Clone)]
pub struct DeviceThrottle {
    /// Device major number
    pub major: u64,
    /// Device minor number
    pub minor: u64,
    /// Rate limit
    pub rate: u64,
}

impl IoController {
    /// Get v2 io.weight value (default 100)
    pub fn v2_weight(&self) -> u16 {
        // Convert v1 weight (10-1000) to v2 weight (1-10000)
        if self.weight == 0 {
            100
        } else {
            ((self.weight as u32 - 10) * 9999 / 990 + 1) as u16
        }
    }
}

/// PIDs controller configuration
#[derive(Debug, Clone)]
pub struct PidsController {
    /// Maximum number of PIDs (-1 = unlimited)
    pub limit: i64,
    /// Current PID count
    pub current: u64,
}

impl Default for PidsController {
    fn default() -> Self {
        Self {
            limit: -1,
            current: 0,
        }
    }
}

impl PidsController {
    /// Check if at limit
    pub fn at_limit(&self) -> bool {
        self.limit > 0 && self.current >= self.limit as u64
    }

    /// Remaining PIDs
    pub fn remaining(&self) -> Option<u64> {
        if self.limit > 0 {
            Some((self.limit as u64).saturating_sub(self.current))
        } else {
            None
        }
    }
}

/// Devices controller configuration (v1 only)
#[derive(Debug, Clone, Default)]
pub struct DevicesController {
    /// Device rules
    pub rules: Vec<DeviceRule>,
}

/// Device access rule
#[derive(Debug, Clone)]
pub struct DeviceRule {
    /// Allow or deny
    pub allow: bool,
    /// Device type (a=all, c=char, b=block)
    pub dev_type: char,
    /// Major number (* = all)
    pub major: Option<u64>,
    /// Minor number (* = all)
    pub minor: Option<u64>,
    /// Access permissions (r=read, w=write, m=mknod)
    pub access: String,
}

impl DeviceRule {
    /// Create allow rule
    pub fn allow(dev_type: char, major: Option<u64>, minor: Option<u64>, access: &str) -> Self {
        Self {
            allow: true,
            dev_type,
            major,
            minor,
            access: access.to_string(),
        }
    }

    /// Create deny rule
    pub fn deny(dev_type: char, major: Option<u64>, minor: Option<u64>, access: &str) -> Self {
        Self {
            allow: false,
            dev_type,
            major,
            minor,
            access: access.to_string(),
        }
    }

    /// Format as cgroup devices rule string
    pub fn to_string(&self) -> String {
        let major_str = self
            .major
            .map(|m| m.to_string())
            .unwrap_or_else(|| "*".to_string());
        let minor_str = self
            .minor
            .map(|m| m.to_string())
            .unwrap_or_else(|| "*".to_string());
        format!(
            "{} {}:{} {}",
            self.dev_type, major_str, minor_str, self.access
        )
    }
}

impl DevicesController {
    /// Add default container rules
    pub fn add_default_rules(&mut self) {
        // Deny all by default
        self.rules.push(DeviceRule::deny('a', None, None, "rwm"));

        // Allow common devices
        // /dev/null, /dev/zero, /dev/full, /dev/random, /dev/urandom
        self.rules
            .push(DeviceRule::allow('c', Some(1), Some(3), "rwm")); // null
        self.rules
            .push(DeviceRule::allow('c', Some(1), Some(5), "rwm")); // zero
        self.rules
            .push(DeviceRule::allow('c', Some(1), Some(7), "rwm")); // full
        self.rules
            .push(DeviceRule::allow('c', Some(1), Some(8), "rwm")); // random
        self.rules
            .push(DeviceRule::allow('c', Some(1), Some(9), "rwm")); // urandom

        // /dev/tty
        self.rules
            .push(DeviceRule::allow('c', Some(5), Some(0), "rwm")); // tty
        self.rules
            .push(DeviceRule::allow('c', Some(5), Some(1), "rwm")); // console
        self.rules
            .push(DeviceRule::allow('c', Some(5), Some(2), "rwm")); // ptmx

        // PTY devices
        self.rules
            .push(DeviceRule::allow('c', Some(136), None, "rwm")); // pts
    }
}

/// Freezer state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezerState {
    /// Processes are running
    Thawed,
    /// Processes are being frozen
    Freezing,
    /// Processes are frozen
    Frozen,
}

/// Cgroup hierarchy
#[derive(Debug)]
pub struct Cgroup {
    /// Cgroup path
    path: PathBuf,
    /// Parent cgroup
    parent: Option<Arc<Cgroup>>,
    /// Cgroup version
    version: CgroupVersion,
    /// CPU controller
    cpu: RwLock<CpuController>,
    /// Cpuset controller
    cpuset: RwLock<CpusetController>,
    /// Memory controller
    memory: RwLock<MemoryController>,
    /// I/O controller
    io: RwLock<IoController>,
    /// PIDs controller
    pids: RwLock<PidsController>,
    /// Devices controller
    devices: RwLock<DevicesController>,
    /// Freezer state
    freezer: RwLock<FreezerState>,
    /// Process IDs in this cgroup
    procs: RwLock<Vec<u32>>,
}

impl Cgroup {
    /// Create new cgroup
    pub fn new(path: PathBuf, version: CgroupVersion) -> Self {
        Self {
            path,
            parent: None,
            version,
            cpu: RwLock::new(CpuController::default()),
            cpuset: RwLock::new(CpusetController::default()),
            memory: RwLock::new(MemoryController::default()),
            io: RwLock::new(IoController::default()),
            pids: RwLock::new(PidsController::default()),
            devices: RwLock::new(DevicesController::default()),
            freezer: RwLock::new(FreezerState::Thawed),
            procs: RwLock::new(Vec::new()),
        }
    }

    /// Create child cgroup
    pub fn new_child(parent: Arc<Cgroup>, name: &str) -> Self {
        let path = parent.path.join(name);
        Self {
            path,
            parent: Some(parent.clone()),
            version: parent.version,
            cpu: RwLock::new(CpuController::default()),
            cpuset: RwLock::new(CpusetController::default()),
            memory: RwLock::new(MemoryController::default()),
            io: RwLock::new(IoController::default()),
            pids: RwLock::new(PidsController::default()),
            devices: RwLock::new(DevicesController::default()),
            freezer: RwLock::new(FreezerState::Thawed),
            procs: RwLock::new(Vec::new()),
        }
    }

    /// Get cgroup path
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get cgroup version
    pub fn version(&self) -> CgroupVersion {
        self.version
    }

    /// Get CPU controller
    pub fn cpu(&self) -> CpuController {
        self.cpu.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Set CPU controller
    pub fn set_cpu(&self, cpu: CpuController) {
        *self.cpu.write().unwrap_or_else(|e| e.into_inner()) = cpu;
    }

    /// Get cpuset controller
    pub fn cpuset(&self) -> CpusetController {
        self.cpuset
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set cpuset controller
    pub fn set_cpuset(&self, cpuset: CpusetController) {
        *self.cpuset.write().unwrap_or_else(|e| e.into_inner()) = cpuset;
    }

    /// Get memory controller
    pub fn memory(&self) -> MemoryController {
        self.memory
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set memory controller
    pub fn set_memory(&self, memory: MemoryController) {
        *self.memory.write().unwrap_or_else(|e| e.into_inner()) = memory;
    }

    /// Get I/O controller
    pub fn io(&self) -> IoController {
        self.io.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Set I/O controller
    pub fn set_io(&self, io: IoController) {
        *self.io.write().unwrap_or_else(|e| e.into_inner()) = io;
    }

    /// Get PIDs controller
    pub fn pids(&self) -> PidsController {
        self.pids.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Set PIDs controller
    pub fn set_pids(&self, pids: PidsController) {
        *self.pids.write().unwrap_or_else(|e| e.into_inner()) = pids;
    }

    /// Get devices controller
    pub fn devices(&self) -> DevicesController {
        self.devices
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set devices controller
    pub fn set_devices(&self, devices: DevicesController) {
        *self.devices.write().unwrap_or_else(|e| e.into_inner()) = devices;
    }

    /// Get freezer state
    pub fn freezer_state(&self) -> FreezerState {
        *self.freezer.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Freeze processes
    pub fn freeze(&self) {
        *self.freezer.write().unwrap_or_else(|e| e.into_inner()) = FreezerState::Frozen;
    }

    /// Thaw processes
    pub fn thaw(&self) {
        *self.freezer.write().unwrap_or_else(|e| e.into_inner()) = FreezerState::Thawed;
    }

    /// Add process
    pub fn add_proc(&self, pid: u32) {
        let mut procs = self.procs.write().unwrap_or_else(|e| e.into_inner());
        if !procs.contains(&pid) {
            procs.push(pid);
        }
        // Update PID counter
        self.pids.write().unwrap_or_else(|e| e.into_inner()).current = procs.len() as u64;
    }

    /// Remove process
    pub fn remove_proc(&self, pid: u32) {
        let mut procs = self.procs.write().unwrap_or_else(|e| e.into_inner());
        procs.retain(|&p| p != pid);
        self.pids.write().unwrap_or_else(|e| e.into_inner()).current = procs.len() as u64;
    }

    /// List processes
    pub fn list_procs(&self) -> Vec<u32> {
        self.procs.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Check if process can be added (PID limit)
    pub fn can_add_proc(&self) -> bool {
        !self
            .pids
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .at_limit()
    }
}

/// Cgroup manager
#[derive(Debug)]
pub struct CgroupManager {
    /// Root path for cgroups
    root: PathBuf,
    /// Cgroup version
    version: CgroupVersion,
    /// Cgroups by path
    cgroups: RwLock<HashMap<PathBuf, Arc<Cgroup>>>,
    /// Cgroup counter
    cgroup_count: AtomicU64,
}

impl CgroupManager {
    /// Create new cgroup manager
    pub fn new(root: PathBuf, version: CgroupVersion) -> Self {
        Self {
            root,
            version,
            cgroups: RwLock::new(HashMap::new()),
            cgroup_count: AtomicU64::new(0),
        }
    }

    /// Get root path
    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// Get cgroup version
    pub fn version(&self) -> CgroupVersion {
        self.version
    }

    /// Create a new cgroup
    pub fn create(&self, relative_path: &str) -> Arc<Cgroup> {
        let path = self.root.join(relative_path);

        let mut cgroups = self.cgroups.write().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = cgroups.get(&path) {
            return existing.clone();
        }

        let cgroup = Arc::new(Cgroup::new(path.clone(), self.version));
        cgroups.insert(path, cgroup.clone());
        self.cgroup_count.fetch_add(1, Ordering::Relaxed);

        cgroup
    }

    /// Get cgroup by path
    pub fn get(&self, relative_path: &str) -> Option<Arc<Cgroup>> {
        let path = self.root.join(relative_path);
        self.cgroups
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path)
            .cloned()
    }

    /// Delete cgroup
    pub fn delete(&self, relative_path: &str) -> bool {
        let path = self.root.join(relative_path);
        let mut cgroups = self.cgroups.write().unwrap_or_else(|e| e.into_inner());
        cgroups.remove(&path).is_some()
    }

    /// List all cgroups
    pub fn list(&self) -> Vec<Arc<Cgroup>> {
        self.cgroups
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Get cgroup count
    pub fn count(&self) -> usize {
        self.cgroups.read().unwrap_or_else(|e| e.into_inner()).len()
    }

    /// Get manager statistics
    pub fn stats(&self) -> CgroupManagerStats {
        CgroupManagerStats {
            version: self.version,
            cgroup_count: self.count(),
        }
    }
}

/// Cgroup manager statistics
#[derive(Debug, Clone)]
pub struct CgroupManagerStats {
    /// Cgroup version
    pub version: CgroupVersion,
    /// Number of cgroups
    pub cgroup_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_controller_names() {
        assert_eq!(Controller::Cpu.v1_name(), "cpu");
        assert_eq!(Controller::Memory.v1_name(), "memory");
        assert_eq!(Controller::Blkio.v1_name(), "blkio");
    }

    #[test]
    fn test_controller_v2_names() {
        assert_eq!(Controller::Cpu.v2_name(), Some("cpu"));
        assert_eq!(Controller::Memory.v2_name(), Some("memory"));
        assert_eq!(Controller::Io.v2_name(), Some("io"));
        assert_eq!(Controller::Blkio.v2_name(), None); // No v2 equivalent
    }

    #[test]
    fn test_cpu_controller_default() {
        let cpu = CpuController::default();
        assert_eq!(cpu.shares, 1024);
        assert_eq!(cpu.quota, -1);
        assert_eq!(cpu.period, 100_000);
    }

    #[test]
    fn test_cpu_controller_weight() {
        let cpu = CpuController::default();
        assert!(cpu.weight() >= 1 && cpu.weight() <= 10000);
    }

    #[test]
    fn test_cpu_controller_limit_fraction() {
        let mut cpu = CpuController::default();
        assert!(cpu.limit_fraction().is_none());

        cpu.quota = 50_000;
        cpu.period = 100_000;
        assert_eq!(cpu.limit_fraction(), Some(0.5));
    }

    #[test]
    fn test_cpuset_parse_list() {
        let mut cpuset = CpusetController::default();
        cpuset.cpus = "0-3,5,7-9".to_string();

        let cpus = cpuset.parse_cpus();
        assert_eq!(cpus, vec![0, 1, 2, 3, 5, 7, 8, 9]);
    }

    #[test]
    fn test_cpuset_format_list() {
        let mut cpuset = CpusetController::default();
        cpuset.set_cpus(&[0, 1, 2, 3, 5, 7, 8, 9]);

        assert_eq!(cpuset.cpus, "0-3,5,7-9");
    }

    #[test]
    fn test_memory_controller_default() {
        let mem = MemoryController::default();
        assert_eq!(mem.limit, -1);
        assert!(!mem.is_limited());
    }

    #[test]
    fn test_memory_controller_limit_string() {
        let mut mem = MemoryController::default();
        assert_eq!(mem.limit_string(), "unlimited");

        mem.limit = 1024 * 1024 * 1024; // 1 GiB
        assert_eq!(mem.limit_string(), "1Gi");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512B");
        assert_eq!(format_bytes(1024), "1Ki");
        assert_eq!(format_bytes(1024 * 1024), "1Mi");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1Gi");
    }

    #[test]
    fn test_pids_controller() {
        let mut pids = PidsController::default();
        assert!(!pids.at_limit());

        pids.limit = 100;
        pids.current = 50;
        assert!(!pids.at_limit());
        assert_eq!(pids.remaining(), Some(50));

        pids.current = 100;
        assert!(pids.at_limit());
        assert_eq!(pids.remaining(), Some(0));
    }

    #[test]
    fn test_device_rule() {
        let rule = DeviceRule::allow('c', Some(1), Some(3), "rwm");
        assert!(rule.allow);
        assert_eq!(rule.to_string(), "c 1:3 rwm");

        let rule_wildcard = DeviceRule::deny('b', None, None, "rwm");
        assert!(!rule_wildcard.allow);
        assert_eq!(rule_wildcard.to_string(), "b *:* rwm");
    }

    #[test]
    fn test_devices_controller_defaults() {
        let mut devices = DevicesController::default();
        devices.add_default_rules();

        assert!(!devices.rules.is_empty());
        // First rule should deny all
        assert!(!devices.rules[0].allow);
        assert_eq!(devices.rules[0].dev_type, 'a');
    }

    #[test]
    fn test_freezer_state() {
        let cgroup = Cgroup::new(PathBuf::from("/sys/fs/cgroup/test"), CgroupVersion::V2);
        assert_eq!(cgroup.freezer_state(), FreezerState::Thawed);

        cgroup.freeze();
        assert_eq!(cgroup.freezer_state(), FreezerState::Frozen);

        cgroup.thaw();
        assert_eq!(cgroup.freezer_state(), FreezerState::Thawed);
    }

    #[test]
    fn test_cgroup_procs() {
        let cgroup = Cgroup::new(PathBuf::from("/sys/fs/cgroup/test"), CgroupVersion::V2);

        cgroup.add_proc(1000);
        cgroup.add_proc(1001);
        assert_eq!(cgroup.list_procs().len(), 2);
        assert_eq!(cgroup.pids().current, 2);

        cgroup.remove_proc(1000);
        assert_eq!(cgroup.list_procs().len(), 1);
        assert_eq!(cgroup.pids().current, 1);
    }

    #[test]
    fn test_cgroup_pid_limit() {
        let cgroup = Cgroup::new(PathBuf::from("/sys/fs/cgroup/test"), CgroupVersion::V2);
        cgroup.set_pids(PidsController {
            limit: 2,
            current: 0,
        });

        assert!(cgroup.can_add_proc());
        cgroup.add_proc(1000);
        assert!(cgroup.can_add_proc());
        cgroup.add_proc(1001);
        assert!(!cgroup.can_add_proc());
    }

    #[test]
    fn test_cgroup_manager_create() {
        let mgr = CgroupManager::new(PathBuf::from("/sys/fs/cgroup"), CgroupVersion::V2);

        let cg1 = mgr.create("container1");
        let cg2 = mgr.create("container2");

        assert_ne!(cg1.path(), cg2.path());
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn test_cgroup_manager_get() {
        let mgr = CgroupManager::new(PathBuf::from("/sys/fs/cgroup"), CgroupVersion::V2);

        mgr.create("container1");

        let cg = mgr.get("container1");
        assert!(cg.is_some());

        let missing = mgr.get("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_cgroup_manager_delete() {
        let mgr = CgroupManager::new(PathBuf::from("/sys/fs/cgroup"), CgroupVersion::V2);

        mgr.create("container1");
        assert_eq!(mgr.count(), 1);

        mgr.delete("container1");
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn test_cgroup_manager_duplicate_create() {
        let mgr = CgroupManager::new(PathBuf::from("/sys/fs/cgroup"), CgroupVersion::V2);

        let cg1 = mgr.create("container1");
        let cg2 = mgr.create("container1");

        // Should return same cgroup
        assert!(Arc::ptr_eq(&cg1, &cg2));
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn test_io_controller_v2_weight() {
        let mut io = IoController::default();
        io.weight = 500;

        let v2_weight = io.v2_weight();
        assert!((1..=10000).contains(&v2_weight));
    }

    #[test]
    fn test_parse_list_empty() {
        let result = parse_list_format("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_list_single() {
        let result = parse_list_format("5");
        assert_eq!(result, vec![5]);
    }

    #[test]
    fn test_parse_list_range() {
        let result = parse_list_format("1-5");
        assert_eq!(result, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_parse_list_complex() {
        let result = parse_list_format("0,2-4,6");
        assert_eq!(result, vec![0, 2, 3, 4, 6]);
    }

    #[test]
    fn test_memory_swap_max() {
        let mut mem = MemoryController::default();
        mem.limit = 1024 * 1024 * 1024; // 1 GiB
        mem.swap_limit = 2 * 1024 * 1024 * 1024; // 2 GiB

        // v2 swap.max should be swap_limit - limit
        assert_eq!(mem.swap_max(), 1024 * 1024 * 1024);
    }

    #[test]
    fn test_cgroup_version() {
        let mgr = CgroupManager::new(PathBuf::from("/sys/fs/cgroup"), CgroupVersion::V2);
        assert_eq!(mgr.version(), CgroupVersion::V2);

        let stats = mgr.stats();
        assert_eq!(stats.version, CgroupVersion::V2);
    }
}
