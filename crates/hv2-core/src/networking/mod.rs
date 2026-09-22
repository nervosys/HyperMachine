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
// `ConnTracker`, `FilterChain` and `NetworkFilter` are deprecated: they are
// not wired to any data path. Re-exported anyway so the deprecation reaches
// anyone already using them rather than vanishing from the API without a
// word, and `allow`ed here because re-exporting a deprecated item warns at
// the `pub use` and this crate builds with warnings denied. The warning is
// for callers, not for this line.
#[allow(deprecated)]
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
