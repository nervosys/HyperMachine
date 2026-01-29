//! PCI Express Support
//!
//! This module provides comprehensive PCI/PCIe support including:
//! - PCI configuration space emulation
//! - PCI capability structures (MSI, MSI-X, PM, PCIe)
//! - PCI bus topology and device enumeration
//! - PCI Express link management

mod bus;
mod capabilities;
mod config;
mod types;

pub use bus::{
    BridgeForwarder, BusStats, BusStatsSnapshot, PciBus, PciDeviceInfo, PciDeviceSlot,
    PciRootComplex, MAX_BUSES, MAX_DEVICES, MAX_FUNCTIONS,
};

pub use capabilities::{
    CapabilityHeader, CapabilityId, CapabilityStats, ExtendedCapabilityId, MsiCapability,
    MsiControl, MsixCapability, MsixControl, MsixTableEntry, PcieCapability, PcieDeviceType,
    PcieLinkSpeed, PcieLinkStatus, PcieLinkWidth, PmCapability, PmControl, PowerState,
};

pub use config::{
    bridge_control, registers, BarConfig, BridgeConfigSpace, ConfigSpace, ConfigStats,
    PCIE_CONFIG_SIZE, PCI_CONFIG_SIZE,
};

pub use types::{
    BarType, ClassCode, CommandRegister, DeviceId, HeaderType, InterruptPin, PciAddress,
    StatusRegister, VendorId,
};
