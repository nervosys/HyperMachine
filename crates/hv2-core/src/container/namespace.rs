//! Linux Namespace Isolation
//!
//! This module models the Linux namespace types the container spec names --
//! it does not call `unshare`, `setns`, or anything else that would create
//! one. See `container::runtime` for what the container module as a whole
//! does and does not do; for namespaces the operating system actually
//! creates, see `hv2-sandbox`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Namespace type identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NsType {
    /// PID namespace - process ID isolation
    Pid,
    /// Network namespace - network stack isolation
    Net,
    /// Mount namespace - filesystem mount isolation
    Mnt,
    /// UTS namespace - hostname/domainname isolation
    Uts,
    /// IPC namespace - System V IPC isolation
    Ipc,
    /// User namespace - UID/GID isolation
    User,
    /// Cgroup namespace - cgroup view isolation
    Cgroup,
    /// Time namespace - time offset isolation
    Time,
}

impl NsType {
    /// Get CLONE_NEW* flag for this namespace type
    pub fn clone_flag(&self) -> u64 {
        match self {
            Self::Pid => 0x20000000,    // CLONE_NEWPID
            Self::Net => 0x40000000,    // CLONE_NEWNET
            Self::Mnt => 0x00020000,    // CLONE_NEWNS
            Self::Uts => 0x04000000,    // CLONE_NEWUTS
            Self::Ipc => 0x08000000,    // CLONE_NEWIPC
            Self::User => 0x10000000,   // CLONE_NEWUSER
            Self::Cgroup => 0x02000000, // CLONE_NEWCGROUP
            Self::Time => 0x00000080,   // CLONE_NEWTIME
        }
    }

    /// Get proc filesystem name
    pub fn proc_name(&self) -> &'static str {
        match self {
            Self::Pid => "pid",
            Self::Net => "net",
            Self::Mnt => "mnt",
            Self::Uts => "uts",
            Self::Ipc => "ipc",
            Self::User => "user",
            Self::Cgroup => "cgroup",
            Self::Time => "time",
        }
    }

    /// Get all namespace types
    pub fn all() -> &'static [Self] {
        &[
            Self::Pid,
            Self::Net,
            Self::Mnt,
            Self::Uts,
            Self::Ipc,
            Self::User,
            Self::Cgroup,
            Self::Time,
        ]
    }

    /// Get default namespaces for container isolation
    pub fn container_defaults() -> &'static [Self] {
        &[Self::Pid, Self::Net, Self::Mnt, Self::Uts, Self::Ipc]
    }
}

/// Namespace identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NsId(pub u64);

impl NsId {
    /// Initial/host namespace ID
    pub const HOST: Self = Self(1);

    /// Create new namespace ID
    pub fn new(id: u64) -> Self {
        Self(id)
    }
}

/// Namespace handle representing a reference to a namespace
#[derive(Debug, Clone)]
pub struct NsHandle {
    /// Namespace type
    ns_type: NsType,
    /// Namespace ID
    ns_id: NsId,
    /// Path to namespace file (e.g., `/proc/<pid>/ns/net`)
    path: Option<PathBuf>,
}

impl NsHandle {
    /// Create new namespace handle
    pub fn new(ns_type: NsType, ns_id: NsId) -> Self {
        Self {
            ns_type,
            ns_id,
            path: None,
        }
    }

    /// Create handle with path
    pub fn with_path(ns_type: NsType, ns_id: NsId, path: PathBuf) -> Self {
        Self {
            ns_type,
            ns_id,
            path: Some(path),
        }
    }

    /// Get namespace type
    pub fn ns_type(&self) -> NsType {
        self.ns_type
    }

    /// Get namespace ID
    pub fn ns_id(&self) -> NsId {
        self.ns_id
    }

    /// Get path if available
    pub fn path(&self) -> Option<&PathBuf> {
        self.path.as_ref()
    }
}

/// PID namespace state
#[derive(Debug)]
pub struct PidNamespace {
    /// Namespace ID
    id: NsId,
    /// Parent namespace
    parent: Option<NsId>,
    /// Next PID to allocate
    next_pid: AtomicU64,
    /// PID limit
    pid_limit: u64,
    /// Active processes (virtual PID -> host PID)
    processes: RwLock<HashMap<u32, u32>>,
}

impl PidNamespace {
    /// Create new PID namespace
    pub fn new(id: NsId, parent: Option<NsId>) -> Self {
        Self {
            id,
            parent,
            next_pid: AtomicU64::new(1),
            pid_limit: 32768,
            processes: RwLock::new(HashMap::new()),
        }
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Get parent namespace
    pub fn parent(&self) -> Option<NsId> {
        self.parent
    }

    /// Allocate a new PID
    pub fn alloc_pid(&self) -> Option<u32> {
        let pid = self.next_pid.fetch_add(1, Ordering::SeqCst);
        if pid > self.pid_limit {
            self.next_pid.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(pid as u32)
    }

    /// Register a process
    pub fn register(&self, virtual_pid: u32, host_pid: u32) {
        self.processes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(virtual_pid, host_pid);
    }

    /// Unregister a process
    pub fn unregister(&self, virtual_pid: u32) -> Option<u32> {
        self.processes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&virtual_pid)
    }

    /// Translate virtual PID to host PID
    pub fn translate_to_host(&self, virtual_pid: u32) -> Option<u32> {
        self.processes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&virtual_pid)
            .copied()
    }

    /// Get process count
    pub fn process_count(&self) -> usize {
        self.processes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Set PID limit
    pub fn set_pid_limit(&mut self, limit: u64) {
        self.pid_limit = limit;
    }
}

/// Network namespace state
#[derive(Debug)]
pub struct NetNamespace {
    /// Namespace ID
    id: NsId,
    /// Network interfaces
    interfaces: RwLock<Vec<NetInterface>>,
    /// Routing table
    routes: RwLock<Vec<Route>>,
    /// Firewall rules
    rules: RwLock<Vec<FirewallRule>>,
}

/// Network interface
#[derive(Debug, Clone)]
pub struct NetInterface {
    /// Interface name
    pub name: String,
    /// Interface type
    pub if_type: InterfaceType,
    /// MAC address
    pub mac: [u8; 6],
    /// IP addresses
    pub addresses: Vec<IpAddress>,
    /// MTU
    pub mtu: u32,
    /// Interface is up
    pub up: bool,
}

/// Interface type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceType {
    /// Loopback
    Loopback,
    /// Virtual Ethernet pair
    Veth,
    /// Bridge
    Bridge,
    /// VLAN
    Vlan,
    /// Macvlan
    Macvlan,
    /// IPvlan
    Ipvlan,
}

/// IP address
#[derive(Debug, Clone)]
pub struct IpAddress {
    /// Address bytes (4 for IPv4, 16 for IPv6)
    pub addr: Vec<u8>,
    /// Prefix length
    pub prefix_len: u8,
}

impl IpAddress {
    /// Create IPv4 address
    pub fn ipv4(a: u8, b: u8, c: u8, d: u8, prefix: u8) -> Self {
        Self {
            addr: vec![a, b, c, d],
            prefix_len: prefix,
        }
    }

    /// Check if IPv4
    pub fn is_ipv4(&self) -> bool {
        self.addr.len() == 4
    }

    /// Check if IPv6
    pub fn is_ipv6(&self) -> bool {
        self.addr.len() == 16
    }
}

/// Network route
#[derive(Debug, Clone)]
pub struct Route {
    /// Destination network
    pub destination: IpAddress,
    /// Gateway (if any)
    pub gateway: Option<Vec<u8>>,
    /// Output interface
    pub interface: String,
    /// Route metric
    pub metric: u32,
}

/// Firewall rule
#[derive(Debug, Clone)]
pub struct FirewallRule {
    /// Rule chain
    pub chain: FirewallChain,
    /// Source address (CIDR)
    pub source: Option<IpAddress>,
    /// Destination address (CIDR)
    pub destination: Option<IpAddress>,
    /// Protocol
    pub protocol: Option<Protocol>,
    /// Destination port
    pub dport: Option<u16>,
    /// Source port
    pub sport: Option<u16>,
    /// Action
    pub action: FirewallAction,
}

/// Firewall chain
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallChain {
    /// Input chain
    Input,
    /// Output chain
    Output,
    /// Forward chain
    Forward,
}

/// Network protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// TCP
    Tcp,
    /// UDP
    Udp,
    /// ICMP
    Icmp,
    /// All protocols
    All,
}

/// Firewall action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallAction {
    /// Accept packet
    Accept,
    /// Drop packet
    Drop,
    /// Reject packet (send error)
    Reject,
    /// Log packet
    Log,
}

impl NetNamespace {
    /// Create new network namespace
    pub fn new(id: NsId) -> Self {
        let mut ns = Self {
            id,
            interfaces: RwLock::new(Vec::new()),
            routes: RwLock::new(Vec::new()),
            rules: RwLock::new(Vec::new()),
        };
        ns.setup_loopback();
        ns
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Setup loopback interface
    fn setup_loopback(&mut self) {
        let lo = NetInterface {
            name: "lo".to_string(),
            if_type: InterfaceType::Loopback,
            mac: [0; 6],
            addresses: vec![IpAddress::ipv4(127, 0, 0, 1, 8)],
            mtu: 65536,
            up: true,
        };
        self.interfaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(lo);
    }

    /// Add interface
    pub fn add_interface(&self, iface: NetInterface) {
        self.interfaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(iface);
    }

    /// Remove interface
    pub fn remove_interface(&self, name: &str) -> Option<NetInterface> {
        let mut interfaces = self.interfaces.write().unwrap_or_else(|e| e.into_inner());
        interfaces
            .iter()
            .position(|i| i.name == name)
            .map(|pos| interfaces.remove(pos))
    }

    /// Get interface by name
    pub fn get_interface(&self, name: &str) -> Option<NetInterface> {
        self.interfaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .find(|i| i.name == name)
            .cloned()
    }

    /// List interfaces
    pub fn list_interfaces(&self) -> Vec<NetInterface> {
        self.interfaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Add route
    pub fn add_route(&self, route: Route) {
        self.routes
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(route);
    }

    /// List routes
    pub fn list_routes(&self) -> Vec<Route> {
        self.routes
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Add firewall rule
    pub fn add_rule(&self, rule: FirewallRule) {
        self.rules
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(rule);
    }

    /// List rules
    pub fn list_rules(&self) -> Vec<FirewallRule> {
        self.rules.read().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// Mount namespace state
#[derive(Debug)]
pub struct MntNamespace {
    /// Namespace ID
    id: NsId,
    /// Mount points
    mounts: RwLock<Vec<MountPoint>>,
    /// Root path
    root: PathBuf,
}

/// Mount point
#[derive(Debug, Clone)]
pub struct MountPoint {
    /// Source (device or path)
    pub source: String,
    /// Target mount point
    pub target: PathBuf,
    /// Filesystem type
    pub fstype: String,
    /// Mount flags
    pub flags: MountFlags,
    /// Mount options
    pub options: String,
}

bitflags::bitflags! {
    /// Mount flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct MountFlags: u32 {
        /// Read-only mount
        const RDONLY = 0x0001;
        /// No setuid
        const NOSUID = 0x0002;
        /// No device nodes
        const NODEV = 0x0004;
        /// No exec
        const NOEXEC = 0x0008;
        /// Synchronous I/O
        const SYNC = 0x0010;
        /// Remount
        const REMOUNT = 0x0020;
        /// Mandatory locking
        const MANDLOCK = 0x0040;
        /// Directory timestamps
        const DIRSYNC = 0x0080;
        /// No access time
        const NOATIME = 0x0400;
        /// No directory access time
        const NODIRATIME = 0x0800;
        /// Bind mount
        const BIND = 0x1000;
        /// Move mount
        const MOVE = 0x2000;
        /// Recursive mount
        const REC = 0x4000;
        /// Silent mount
        const SILENT = 0x8000;
        /// Private mount
        const PRIVATE = 0x40000;
        /// Slave mount
        const SLAVE = 0x80000;
        /// Shared mount
        const SHARED = 0x100000;
    }
}

impl MntNamespace {
    /// Create new mount namespace
    pub fn new(id: NsId, root: PathBuf) -> Self {
        Self {
            id,
            mounts: RwLock::new(Vec::new()),
            root,
        }
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Get root path
    pub fn root(&self) -> &PathBuf {
        &self.root
    }

    /// Add mount
    pub fn mount(&self, mount: MountPoint) {
        self.mounts
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(mount);
    }

    /// Remove mount
    pub fn umount(&self, target: &PathBuf) -> Option<MountPoint> {
        let mut mounts = self.mounts.write().unwrap_or_else(|e| e.into_inner());
        mounts
            .iter()
            .position(|m| &m.target == target)
            .map(|pos| mounts.remove(pos))
    }

    /// List mounts
    pub fn list_mounts(&self) -> Vec<MountPoint> {
        self.mounts
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Find mount for path
    pub fn find_mount(&self, path: &PathBuf) -> Option<MountPoint> {
        let mounts = self.mounts.read().unwrap_or_else(|e| e.into_inner());
        // Find longest matching prefix
        mounts
            .iter()
            .filter(|m| path.starts_with(&m.target))
            .max_by_key(|m| m.target.as_os_str().len())
            .cloned()
    }
}

/// UTS namespace state
#[derive(Debug)]
pub struct UtsNamespace {
    /// Namespace ID
    id: NsId,
    /// Hostname
    hostname: RwLock<String>,
    /// Domain name
    domainname: RwLock<String>,
}

impl UtsNamespace {
    /// Create new UTS namespace
    pub fn new(id: NsId, hostname: impl Into<String>) -> Self {
        Self {
            id,
            hostname: RwLock::new(hostname.into()),
            domainname: RwLock::new(String::new()),
        }
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Get hostname
    pub fn hostname(&self) -> String {
        self.hostname
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set hostname
    pub fn set_hostname(&self, hostname: impl Into<String>) {
        *self.hostname.write().unwrap_or_else(|e| e.into_inner()) = hostname.into();
    }

    /// Get domain name
    pub fn domainname(&self) -> String {
        self.domainname
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Set domain name
    pub fn set_domainname(&self, domainname: impl Into<String>) {
        *self.domainname.write().unwrap_or_else(|e| e.into_inner()) = domainname.into();
    }
}

/// IPC namespace state
#[derive(Debug)]
pub struct IpcNamespace {
    /// Namespace ID
    id: NsId,
    /// Message queues
    msg_queues: RwLock<HashMap<i32, MsgQueue>>,
    /// Semaphore sets
    semaphores: RwLock<HashMap<i32, SemaphoreSet>>,
    /// Shared memory segments
    shm_segments: RwLock<HashMap<i32, ShmSegment>>,
    /// Next key ID
    next_key: AtomicU64,
}

/// Message queue
#[derive(Debug, Clone)]
pub struct MsgQueue {
    /// Queue ID
    pub id: i32,
    /// Key
    pub key: i32,
    /// Messages
    pub messages: Vec<Vec<u8>>,
    /// Maximum bytes
    pub max_bytes: u64,
    /// Current bytes
    pub current_bytes: u64,
}

/// Semaphore set
#[derive(Debug, Clone)]
pub struct SemaphoreSet {
    /// Set ID
    pub id: i32,
    /// Key
    pub key: i32,
    /// Semaphore values
    pub values: Vec<i32>,
}

/// Shared memory segment
#[derive(Debug, Clone)]
pub struct ShmSegment {
    /// Segment ID
    pub id: i32,
    /// Key
    pub key: i32,
    /// Size
    pub size: usize,
    /// Attached count
    pub attached: u32,
}

impl IpcNamespace {
    /// Create new IPC namespace
    pub fn new(id: NsId) -> Self {
        Self {
            id,
            msg_queues: RwLock::new(HashMap::new()),
            semaphores: RwLock::new(HashMap::new()),
            shm_segments: RwLock::new(HashMap::new()),
            next_key: AtomicU64::new(1),
        }
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Create message queue
    pub fn msgget(&self, key: i32) -> i32 {
        let id = self.next_key.fetch_add(1, Ordering::SeqCst) as i32;
        let queue = MsgQueue {
            id,
            key,
            messages: Vec::new(),
            max_bytes: 16384,
            current_bytes: 0,
        };
        self.msg_queues
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, queue);
        id
    }

    /// Create semaphore set
    pub fn semget(&self, key: i32, nsems: usize) -> i32 {
        let id = self.next_key.fetch_add(1, Ordering::SeqCst) as i32;
        let sem = SemaphoreSet {
            id,
            key,
            values: vec![0; nsems],
        };
        self.semaphores
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, sem);
        id
    }

    /// Create shared memory segment
    pub fn shmget(&self, key: i32, size: usize) -> i32 {
        let id = self.next_key.fetch_add(1, Ordering::SeqCst) as i32;
        let shm = ShmSegment {
            id,
            key,
            size,
            attached: 0,
        };
        self.shm_segments
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, shm);
        id
    }

    /// Get message queue count
    pub fn msg_count(&self) -> usize {
        self.msg_queues
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Get semaphore count
    pub fn sem_count(&self) -> usize {
        self.semaphores
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }

    /// Get shared memory count
    pub fn shm_count(&self) -> usize {
        self.shm_segments
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

/// User namespace state
#[derive(Debug)]
pub struct UserNamespace {
    /// Namespace ID
    id: NsId,
    /// Parent namespace
    parent: Option<NsId>,
    /// UID mappings
    uid_map: RwLock<Vec<IdMap>>,
    /// GID mappings
    gid_map: RwLock<Vec<IdMap>>,
}

/// ID mapping entry
#[derive(Debug, Clone, Copy)]
pub struct IdMap {
    /// ID inside namespace
    pub inside_id: u32,
    /// ID outside namespace
    pub outside_id: u32,
    /// Count
    pub count: u32,
}

impl IdMap {
    /// Map inside ID to outside
    pub fn to_outside(&self, inside: u32) -> Option<u32> {
        if inside >= self.inside_id && inside < self.inside_id + self.count {
            Some(self.outside_id + (inside - self.inside_id))
        } else {
            None
        }
    }

    /// Map outside ID to inside
    pub fn to_inside(&self, outside: u32) -> Option<u32> {
        if outside >= self.outside_id && outside < self.outside_id + self.count {
            Some(self.inside_id + (outside - self.outside_id))
        } else {
            None
        }
    }
}

impl UserNamespace {
    /// Create new user namespace
    pub fn new(id: NsId, parent: Option<NsId>) -> Self {
        Self {
            id,
            parent,
            uid_map: RwLock::new(Vec::new()),
            gid_map: RwLock::new(Vec::new()),
        }
    }

    /// Get namespace ID
    pub fn id(&self) -> NsId {
        self.id
    }

    /// Get parent namespace
    pub fn parent(&self) -> Option<NsId> {
        self.parent
    }

    /// Add UID mapping
    pub fn add_uid_map(&self, map: IdMap) {
        self.uid_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(map);
    }

    /// Add GID mapping
    pub fn add_gid_map(&self, map: IdMap) {
        self.gid_map
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .push(map);
    }

    /// Map UID to outside
    pub fn map_uid_out(&self, uid: u32) -> Option<u32> {
        for map in self
            .uid_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            if let Some(outside) = map.to_outside(uid) {
                return Some(outside);
            }
        }
        None
    }

    /// Map UID to inside
    pub fn map_uid_in(&self, uid: u32) -> Option<u32> {
        for map in self
            .uid_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            if let Some(inside) = map.to_inside(uid) {
                return Some(inside);
            }
        }
        None
    }

    /// Map GID to outside
    pub fn map_gid_out(&self, gid: u32) -> Option<u32> {
        for map in self
            .gid_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            if let Some(outside) = map.to_outside(gid) {
                return Some(outside);
            }
        }
        None
    }

    /// Map GID to inside
    pub fn map_gid_in(&self, gid: u32) -> Option<u32> {
        for map in self
            .gid_map
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            if let Some(inside) = map.to_inside(gid) {
                return Some(inside);
            }
        }
        None
    }
}

/// Namespace manager
#[derive(Debug)]
pub struct NamespaceManager {
    /// PID namespaces
    pid_namespaces: RwLock<HashMap<NsId, Arc<PidNamespace>>>,
    /// Network namespaces
    net_namespaces: RwLock<HashMap<NsId, Arc<NetNamespace>>>,
    /// Mount namespaces
    mnt_namespaces: RwLock<HashMap<NsId, Arc<MntNamespace>>>,
    /// UTS namespaces
    uts_namespaces: RwLock<HashMap<NsId, Arc<UtsNamespace>>>,
    /// IPC namespaces
    ipc_namespaces: RwLock<HashMap<NsId, Arc<IpcNamespace>>>,
    /// User namespaces
    user_namespaces: RwLock<HashMap<NsId, Arc<UserNamespace>>>,
    /// Next namespace ID
    next_id: AtomicU64,
}

impl NamespaceManager {
    /// Create new namespace manager
    pub fn new() -> Self {
        Self {
            pid_namespaces: RwLock::new(HashMap::new()),
            net_namespaces: RwLock::new(HashMap::new()),
            mnt_namespaces: RwLock::new(HashMap::new()),
            uts_namespaces: RwLock::new(HashMap::new()),
            ipc_namespaces: RwLock::new(HashMap::new()),
            user_namespaces: RwLock::new(HashMap::new()),
            next_id: AtomicU64::new(2), // 1 is reserved for HOST
        }
    }

    /// Allocate new namespace ID
    fn alloc_id(&self) -> NsId {
        NsId(self.next_id.fetch_add(1, Ordering::SeqCst))
    }

    /// Create PID namespace
    pub fn create_pid_ns(&self, parent: Option<NsId>) -> Arc<PidNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(PidNamespace::new(id, parent));
        self.pid_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Create network namespace
    pub fn create_net_ns(&self) -> Arc<NetNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(NetNamespace::new(id));
        self.net_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Create mount namespace
    pub fn create_mnt_ns(&self, root: PathBuf) -> Arc<MntNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(MntNamespace::new(id, root));
        self.mnt_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Create UTS namespace
    pub fn create_uts_ns(&self, hostname: impl Into<String>) -> Arc<UtsNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(UtsNamespace::new(id, hostname));
        self.uts_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Create IPC namespace
    pub fn create_ipc_ns(&self) -> Arc<IpcNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(IpcNamespace::new(id));
        self.ipc_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Create user namespace
    pub fn create_user_ns(&self, parent: Option<NsId>) -> Arc<UserNamespace> {
        let id = self.alloc_id();
        let ns = Arc::new(UserNamespace::new(id, parent));
        self.user_namespaces
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, ns.clone());
        ns
    }

    /// Get PID namespace
    pub fn get_pid_ns(&self, id: NsId) -> Option<Arc<PidNamespace>> {
        self.pid_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get network namespace
    pub fn get_net_ns(&self, id: NsId) -> Option<Arc<NetNamespace>> {
        self.net_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get mount namespace
    pub fn get_mnt_ns(&self, id: NsId) -> Option<Arc<MntNamespace>> {
        self.mnt_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get UTS namespace
    pub fn get_uts_ns(&self, id: NsId) -> Option<Arc<UtsNamespace>> {
        self.uts_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get IPC namespace
    pub fn get_ipc_ns(&self, id: NsId) -> Option<Arc<IpcNamespace>> {
        self.ipc_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get user namespace
    pub fn get_user_ns(&self, id: NsId) -> Option<Arc<UserNamespace>> {
        self.user_namespaces
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }

    /// Get namespace statistics
    pub fn stats(&self) -> NamespaceStats {
        NamespaceStats {
            pid_count: self
                .pid_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            net_count: self
                .net_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            mnt_count: self
                .mnt_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            uts_count: self
                .uts_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            ipc_count: self
                .ipc_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
            user_count: self
                .user_namespaces
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .len(),
        }
    }
}

impl Default for NamespaceManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Namespace statistics
#[derive(Debug, Clone)]
pub struct NamespaceStats {
    /// PID namespace count
    pub pid_count: usize,
    /// Network namespace count
    pub net_count: usize,
    /// Mount namespace count
    pub mnt_count: usize,
    /// UTS namespace count
    pub uts_count: usize,
    /// IPC namespace count
    pub ipc_count: usize,
    /// User namespace count
    pub user_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ns_type_clone_flags() {
        assert_eq!(NsType::Pid.clone_flag(), 0x20000000);
        assert_eq!(NsType::Net.clone_flag(), 0x40000000);
        assert_eq!(NsType::Mnt.clone_flag(), 0x00020000);
        assert_eq!(NsType::User.clone_flag(), 0x10000000);
    }

    #[test]
    fn test_ns_type_proc_names() {
        assert_eq!(NsType::Pid.proc_name(), "pid");
        assert_eq!(NsType::Net.proc_name(), "net");
        assert_eq!(NsType::Mnt.proc_name(), "mnt");
    }

    #[test]
    fn test_ns_type_defaults() {
        let defaults = NsType::container_defaults();
        assert!(defaults.contains(&NsType::Pid));
        assert!(defaults.contains(&NsType::Net));
        assert!(defaults.contains(&NsType::Mnt));
    }

    #[test]
    fn test_pid_namespace_alloc() {
        let ns = PidNamespace::new(NsId(1), None);
        assert_eq!(ns.alloc_pid(), Some(1));
        assert_eq!(ns.alloc_pid(), Some(2));
        assert_eq!(ns.alloc_pid(), Some(3));
    }

    #[test]
    fn test_pid_namespace_register() {
        let ns = PidNamespace::new(NsId(1), None);
        ns.register(1, 1000);
        ns.register(2, 1001);

        assert_eq!(ns.translate_to_host(1), Some(1000));
        assert_eq!(ns.translate_to_host(2), Some(1001));
        assert_eq!(ns.translate_to_host(3), None);
        assert_eq!(ns.process_count(), 2);
    }

    #[test]
    fn test_pid_namespace_unregister() {
        let ns = PidNamespace::new(NsId(1), None);
        ns.register(1, 1000);

        assert_eq!(ns.unregister(1), Some(1000));
        assert_eq!(ns.translate_to_host(1), None);
    }

    #[test]
    fn test_net_namespace_loopback() {
        let ns = NetNamespace::new(NsId(1));
        let lo = ns.get_interface("lo").unwrap();

        assert_eq!(lo.name, "lo");
        assert!(lo.up);
        assert_eq!(lo.if_type, InterfaceType::Loopback);
    }

    #[test]
    fn test_net_namespace_interfaces() {
        let ns = NetNamespace::new(NsId(1));
        ns.add_interface(NetInterface {
            name: "eth0".to_string(),
            if_type: InterfaceType::Veth,
            mac: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF],
            addresses: vec![IpAddress::ipv4(192, 168, 1, 100, 24)],
            mtu: 1500,
            up: true,
        });

        let interfaces = ns.list_interfaces();
        assert_eq!(interfaces.len(), 2); // lo + eth0

        let eth0 = ns.get_interface("eth0").unwrap();
        assert_eq!(eth0.mtu, 1500);
    }

    #[test]
    fn test_net_namespace_routes() {
        let ns = NetNamespace::new(NsId(1));
        ns.add_route(Route {
            destination: IpAddress::ipv4(0, 0, 0, 0, 0),
            gateway: Some(vec![192, 168, 1, 1]),
            interface: "eth0".to_string(),
            metric: 100,
        });

        let routes = ns.list_routes();
        assert_eq!(routes.len(), 1);
    }

    #[test]
    fn test_mnt_namespace_mounts() {
        let ns = MntNamespace::new(NsId(1), PathBuf::from("/"));
        ns.mount(MountPoint {
            source: "tmpfs".to_string(),
            target: PathBuf::from("/tmp"),
            fstype: "tmpfs".to_string(),
            flags: MountFlags::NOSUID | MountFlags::NODEV,
            options: "size=100m".to_string(),
        });

        let mounts = ns.list_mounts();
        assert_eq!(mounts.len(), 1);
    }

    #[test]
    fn test_mnt_namespace_find_mount() {
        let ns = MntNamespace::new(NsId(1), PathBuf::from("/"));
        ns.mount(MountPoint {
            source: "/dev/sda1".to_string(),
            target: PathBuf::from("/mnt/data"),
            fstype: "ext4".to_string(),
            flags: MountFlags::empty(),
            options: String::new(),
        });

        let mount = ns.find_mount(&PathBuf::from("/mnt/data/file.txt"));
        assert!(mount.is_some());
        assert_eq!(mount.unwrap().target, PathBuf::from("/mnt/data"));
    }

    #[test]
    fn test_uts_namespace() {
        let ns = UtsNamespace::new(NsId(1), "container1");
        assert_eq!(ns.hostname(), "container1");

        ns.set_hostname("newhost");
        assert_eq!(ns.hostname(), "newhost");

        ns.set_domainname("example.com");
        assert_eq!(ns.domainname(), "example.com");
    }

    #[test]
    fn test_ipc_namespace_msgget() {
        let ns = IpcNamespace::new(NsId(1));
        let id = ns.msgget(0x1234);
        assert!(id > 0);
        assert_eq!(ns.msg_count(), 1);
    }

    #[test]
    fn test_ipc_namespace_semget() {
        let ns = IpcNamespace::new(NsId(1));
        let id = ns.semget(0x5678, 4);
        assert!(id > 0);
        assert_eq!(ns.sem_count(), 1);
    }

    #[test]
    fn test_ipc_namespace_shmget() {
        let ns = IpcNamespace::new(NsId(1));
        let id = ns.shmget(0xABCD, 4096);
        assert!(id > 0);
        assert_eq!(ns.shm_count(), 1);
    }

    #[test]
    fn test_user_namespace_uid_map() {
        let ns = UserNamespace::new(NsId(1), None);
        ns.add_uid_map(IdMap {
            inside_id: 0,
            outside_id: 1000,
            count: 65536,
        });

        assert_eq!(ns.map_uid_out(0), Some(1000));
        assert_eq!(ns.map_uid_out(100), Some(1100));
        assert_eq!(ns.map_uid_in(1000), Some(0));
        assert_eq!(ns.map_uid_in(1100), Some(100));
    }

    #[test]
    fn test_user_namespace_gid_map() {
        let ns = UserNamespace::new(NsId(1), None);
        ns.add_gid_map(IdMap {
            inside_id: 0,
            outside_id: 1000,
            count: 65536,
        });

        assert_eq!(ns.map_gid_out(0), Some(1000));
        assert_eq!(ns.map_gid_in(1000), Some(0));
    }

    #[test]
    fn test_id_map_boundaries() {
        let map = IdMap {
            inside_id: 0,
            outside_id: 1000,
            count: 100,
        };

        assert_eq!(map.to_outside(0), Some(1000));
        assert_eq!(map.to_outside(99), Some(1099));
        assert_eq!(map.to_outside(100), None);
    }

    #[test]
    fn test_namespace_manager_create() {
        let mgr = NamespaceManager::new();

        let pid_ns = mgr.create_pid_ns(None);
        let net_ns = mgr.create_net_ns();
        let mnt_ns = mgr.create_mnt_ns(PathBuf::from("/"));
        let uts_ns = mgr.create_uts_ns("test");
        let ipc_ns = mgr.create_ipc_ns();
        let user_ns = mgr.create_user_ns(None);

        // All should have different IDs
        assert_ne!(pid_ns.id(), net_ns.id());
        assert_ne!(net_ns.id(), mnt_ns.id());
        assert_ne!(mnt_ns.id(), uts_ns.id());
        assert_ne!(uts_ns.id(), ipc_ns.id());
        assert_ne!(ipc_ns.id(), user_ns.id());
    }

    #[test]
    fn test_namespace_manager_get() {
        let mgr = NamespaceManager::new();

        let pid_ns = mgr.create_pid_ns(None);
        let id = pid_ns.id();

        let retrieved = mgr.get_pid_ns(id).unwrap();
        assert_eq!(retrieved.id(), id);
    }

    #[test]
    fn test_namespace_manager_stats() {
        let mgr = NamespaceManager::new();

        mgr.create_pid_ns(None);
        mgr.create_pid_ns(None);
        mgr.create_net_ns();

        let stats = mgr.stats();
        assert_eq!(stats.pid_count, 2);
        assert_eq!(stats.net_count, 1);
    }

    #[test]
    fn test_ip_address_ipv4() {
        let addr = IpAddress::ipv4(192, 168, 1, 1, 24);
        assert!(addr.is_ipv4());
        assert!(!addr.is_ipv6());
        assert_eq!(addr.prefix_len, 24);
    }

    #[test]
    fn test_mount_flags() {
        let flags = MountFlags::RDONLY | MountFlags::NOSUID | MountFlags::NODEV;
        assert!(flags.contains(MountFlags::RDONLY));
        assert!(flags.contains(MountFlags::NOSUID));
        assert!(!flags.contains(MountFlags::NOEXEC));
    }

    #[test]
    fn test_ns_handle() {
        let handle = NsHandle::new(NsType::Pid, NsId(42));
        assert_eq!(handle.ns_type(), NsType::Pid);
        assert_eq!(handle.ns_id(), NsId(42));
        assert!(handle.path().is_none());

        let handle_with_path =
            NsHandle::with_path(NsType::Net, NsId(100), PathBuf::from("/proc/1/ns/net"));
        assert!(handle_with_path.path().is_some());
    }
}
