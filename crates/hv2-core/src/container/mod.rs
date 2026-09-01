//! Container and isolation infrastructure
//!
//! This module models container types for the hypervisor -- it does not run
//! anything. See [`runtime`] for what that means in practice:
//! [`runtime::ContainerRuntime::start`] refuses rather than fabricate a PID.
//! What it models:
//! - An OCI-compatible runtime *interface* (types and a state machine, not an implementation)
//! - Linux namespace types
//! - Cgroup resource controller types
//!
//! For confinement the operating system actually enforces, use `hv2-sandbox`
//! (see `docs/SANDBOXES.md`).

pub mod cgroup;
pub mod namespace;
pub mod runtime;

pub use cgroup::{
    Cgroup, CgroupManager, CgroupManagerStats, CgroupVersion, Controller, CpuController,
    CpusetController, DeviceRule, DeviceThrottle, DeviceWeight, DevicesController, FreezerState,
    IoController, MemoryController, PidsController,
};

pub use namespace::{
    FirewallAction, FirewallChain, FirewallRule, IdMap, InterfaceType, IpAddress, IpcNamespace,
    MntNamespace, MountFlags, MountPoint, MsgQueue, NamespaceManager, NamespaceStats, NetInterface,
    NetNamespace, NsHandle, NsId, NsType, PidNamespace, Protocol, Route, SemaphoreSet, ShmSegment,
    UserNamespace, UtsNamespace,
};

pub use runtime::{
    BlockIoConfig, Container, ContainerProcess, ContainerRuntime, ContainerSpec, ContainerState,
    CpuConfig as ContainerCpuConfig, IdMapping, LinuxConfig, MemoryConfig as ContainerMemoryConfig,
    Mount, MountOption, MountType, NamespaceConfig, NamespaceType, PidsConfig, ResourceConfig,
    RootFs, RuntimeError, RuntimeResult, RuntimeStats, SeccompAction, SeccompArg, SeccompConfig,
    SeccompOp, SeccompSyscall, ThrottleDevice, WeightDevice,
};
