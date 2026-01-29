//! Container and isolation infrastructure
//!
//! This module provides container support for the hypervisor:
//! - OCI-compatible runtime interface
//! - Linux namespace isolation
//! - Cgroup resource controllers

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
