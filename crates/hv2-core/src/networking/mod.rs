//! Advanced Networking
//!
//! This module provides advanced networking capabilities for the hypervisor:
//!
//! - **Virtual Switch**: Software L2 switch with MAC learning, VLAN support, and STP
//! - **Network Filter**: Stateful packet filtering with connection tracking
//! - **SR-IOV**: Single Root I/O Virtualization for high-performance networking

pub mod filter;
pub mod sriov;
pub mod vswitch;

// Re-export key types
pub use filter::{
    ConnState, ConnTrackEntry, ConnTracker, FilterAction, FilterChain, FilterRule, IpMatch,
    IpProtocol, NetworkFilter, PortMatch, ProtocolMatch, StateMatch,
};

pub use sriov::{
    DeviceAssignment, IommuGroup, PciAddress, PciClass, PhysicalFunction, SriovCapability,
    SriovError, SriovManager, VfLinkState, VfState, VirtualFunction,
};

pub use vswitch::{
    EthernetFrame, MacAddress, MacEntry, MacTable, Port, PortState, PortStats, PortType, StpState,
    SwitchStats, VirtualSwitch, VlanId, VlanMode, VlanSet,
};
