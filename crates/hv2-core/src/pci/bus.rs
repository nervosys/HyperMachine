//! PCI Bus and Device Enumeration
//!
//! This module provides PCI bus topology, device enumeration,
//! and bridge forwarding logic.

use super::config::{BridgeConfigSpace, ConfigSpace};
use super::types::{ClassCode, DeviceId, HeaderType, PciAddress, VendorId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of PCI buses
pub const MAX_BUSES: usize = 256;
/// Maximum devices per bus
pub const MAX_DEVICES: usize = 32;
/// Maximum functions per device
pub const MAX_FUNCTIONS: usize = 8;

/// PCI device information
#[derive(Debug, Clone)]
pub struct PciDeviceInfo {
    /// Device address
    pub address: PciAddress,
    /// Vendor ID
    pub vendor_id: VendorId,
    /// Device ID
    pub device_id: DeviceId,
    /// Class code
    pub class_code: ClassCode,
    /// Revision ID
    pub revision: u8,
    /// Is multi-function
    pub multifunction: bool,
    /// Header type
    pub header_type: HeaderType,
}

impl PciDeviceInfo {
    /// Create from config space
    pub fn from_config(address: PciAddress, config: &ConfigSpace) -> Option<Self> {
        let vendor_id = config.vendor_id();
        if !vendor_id.is_valid() {
            return None;
        }

        Some(Self {
            address,
            vendor_id,
            device_id: config.device_id(),
            class_code: config.class_code(),
            revision: config.read_u8(0x08),
            multifunction: config.is_multifunction(),
            header_type: config.header_type().unwrap_or(HeaderType::Standard),
        })
    }

    /// Check if device is a bridge
    pub fn is_bridge(&self) -> bool {
        self.header_type == HeaderType::PciBridge
    }
}

/// PCI device slot
pub struct PciDeviceSlot {
    /// Device info
    pub info: PciDeviceInfo,
    /// Configuration space (Type 0)
    config: Option<ConfigSpace>,
    /// Bridge configuration space (Type 1)
    bridge_config: Option<BridgeConfigSpace>,
}

impl PciDeviceSlot {
    /// Create standard device slot
    pub fn new_device(address: PciAddress, config: ConfigSpace) -> Self {
        let info = PciDeviceInfo::from_config(address, &config).unwrap_or(PciDeviceInfo {
            address,
            vendor_id: config.vendor_id(),
            device_id: config.device_id(),
            class_code: config.class_code(),
            revision: 0,
            multifunction: false,
            header_type: HeaderType::Standard,
        });

        Self {
            info,
            config: Some(config),
            bridge_config: None,
        }
    }

    /// Create bridge device slot
    pub fn new_bridge(address: PciAddress, config: BridgeConfigSpace) -> Self {
        let info = PciDeviceInfo {
            address,
            vendor_id: config.base().vendor_id(),
            device_id: config.base().device_id(),
            class_code: config.base().class_code(),
            revision: config.base().read_u8(0x08),
            multifunction: config.base().is_multifunction(),
            header_type: HeaderType::PciBridge,
        };

        Self {
            info,
            config: None,
            bridge_config: Some(config),
        }
    }

    /// Read configuration space
    pub fn read_config(&self, offset: u16) -> u32 {
        if let Some(ref config) = self.config {
            config.read_u32(offset)
        } else if let Some(ref bridge) = self.bridge_config {
            bridge.read_u32(offset)
        } else {
            0xFFFFFFFF
        }
    }

    /// Write configuration space
    pub fn write_config(&mut self, offset: u16, value: u32) {
        if let Some(ref mut config) = self.config {
            config.write_u32(offset, value);
        } else if let Some(ref mut bridge) = self.bridge_config {
            bridge.write_u32(offset, value);
        }
    }

    /// Get config space reference
    pub fn config(&self) -> Option<&ConfigSpace> {
        self.config.as_ref()
    }

    /// Get mutable config space reference
    pub fn config_mut(&mut self) -> Option<&mut ConfigSpace> {
        self.config.as_mut()
    }

    /// Get bridge config space reference
    pub fn bridge_config(&self) -> Option<&BridgeConfigSpace> {
        self.bridge_config.as_ref()
    }

    /// Get mutable bridge config space reference
    pub fn bridge_config_mut(&mut self) -> Option<&mut BridgeConfigSpace> {
        self.bridge_config.as_mut()
    }

    /// Check if this is a bridge
    pub fn is_bridge(&self) -> bool {
        self.bridge_config.is_some()
    }
}

/// PCI bus
pub struct PciBus {
    /// Bus number
    pub number: u8,
    /// Parent bridge address (None for root bus)
    pub parent_bridge: Option<PciAddress>,
    /// Devices on this bus (indexed by device:function)
    devices: HashMap<(u8, u8), PciDeviceSlot>,
}

impl PciBus {
    /// Create new bus
    pub fn new(number: u8, parent_bridge: Option<PciAddress>) -> Self {
        Self {
            number,
            parent_bridge,
            devices: HashMap::new(),
        }
    }

    /// Add device to bus
    pub fn add_device(&mut self, device: u8, function: u8, slot: PciDeviceSlot) {
        self.devices.insert((device, function), slot);
    }

    /// Remove device from bus
    pub fn remove_device(&mut self, device: u8, function: u8) -> Option<PciDeviceSlot> {
        self.devices.remove(&(device, function))
    }

    /// Get device
    pub fn device(&self, device: u8, function: u8) -> Option<&PciDeviceSlot> {
        self.devices.get(&(device, function))
    }

    /// Get mutable device
    pub fn device_mut(&mut self, device: u8, function: u8) -> Option<&mut PciDeviceSlot> {
        self.devices.get_mut(&(device, function))
    }

    /// Check if device exists
    pub fn has_device(&self, device: u8, function: u8) -> bool {
        self.devices.contains_key(&(device, function))
    }

    /// Iterate over devices
    pub fn devices(&self) -> impl Iterator<Item = &PciDeviceSlot> {
        self.devices.values()
    }

    /// Iterate over device addresses
    pub fn device_addresses(&self) -> impl Iterator<Item = PciAddress> + '_ {
        self.devices
            .keys()
            .map(move |(d, f)| PciAddress::new(self.number, *d, *f))
    }

    /// Count devices
    pub fn device_count(&self) -> usize {
        self.devices.len()
    }
}

/// Bus enumeration statistics
#[derive(Debug, Default)]
pub struct BusStats {
    /// Config reads
    config_reads: AtomicU64,
    /// Config writes
    config_writes: AtomicU64,
    /// Devices enumerated
    devices_found: AtomicU64,
    /// Bridges found
    bridges_found: AtomicU64,
}

impl BusStats {
    /// Record config read
    pub fn record_read(&self) {
        self.config_reads.fetch_add(1, Ordering::Relaxed);
    }

    /// Record config write
    pub fn record_write(&self) {
        self.config_writes.fetch_add(1, Ordering::Relaxed);
    }

    /// Record device found
    pub fn record_device(&self) {
        self.devices_found.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bridge found
    pub fn record_bridge(&self) {
        self.bridges_found.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> BusStatsSnapshot {
        BusStatsSnapshot {
            config_reads: self.config_reads.load(Ordering::Relaxed),
            config_writes: self.config_writes.load(Ordering::Relaxed),
            devices_found: self.devices_found.load(Ordering::Relaxed),
            bridges_found: self.bridges_found.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct BusStatsSnapshot {
    /// Config reads
    pub config_reads: u64,
    /// Config writes
    pub config_writes: u64,
    /// Devices found
    pub devices_found: u64,
    /// Bridges found
    pub bridges_found: u64,
}

/// PCI root complex / host bridge
pub struct PciRootComplex {
    /// Segment group
    pub segment: u16,
    /// Buses
    buses: HashMap<u8, PciBus>,
    /// Next available bus number
    next_bus: u8,
    /// Statistics
    stats: BusStats,
    /// ECAM base address
    ecam_base: u64,
}

impl Default for PciRootComplex {
    fn default() -> Self {
        Self::new()
    }
}

impl PciRootComplex {
    /// Create new root complex
    pub fn new() -> Self {
        let mut rc = Self {
            segment: 0,
            buses: HashMap::new(),
            next_bus: 1, // Bus 0 is root bus
            stats: BusStats::default(),
            ecam_base: 0xE0000000, // Default ECAM base
        };

        // Create root bus
        rc.buses.insert(0, PciBus::new(0, None));

        rc
    }

    /// Create with segment
    pub fn with_segment(segment: u16) -> Self {
        let mut rc = Self::new();
        rc.segment = segment;
        rc
    }

    /// Set ECAM base address
    pub fn set_ecam_base(&mut self, base: u64) {
        self.ecam_base = base;
    }

    /// Get ECAM base address
    pub fn ecam_base(&self) -> u64 {
        self.ecam_base
    }

    /// Get statistics
    pub fn stats(&self) -> &BusStats {
        &self.stats
    }

    /// Add device to root bus
    pub fn add_device(&mut self, device: u8, function: u8, config: ConfigSpace) {
        if let Some(bus) = self.buses.get_mut(&0) {
            let address = PciAddress::new(0, device, function);
            self.stats.record_device();
            bus.add_device(device, function, PciDeviceSlot::new_device(address, config));
        }
    }

    /// Add bridge and create secondary bus
    pub fn add_bridge(
        &mut self,
        bus: u8,
        device: u8,
        function: u8,
        mut config: BridgeConfigSpace,
    ) -> Option<u8> {
        // Allocate secondary bus number
        let secondary_bus = self.next_bus;
        if secondary_bus == 255 {
            return None;
        }
        self.next_bus += 1;

        // Configure bridge
        config.set_bus_numbers(bus, secondary_bus, secondary_bus);

        let address = PciAddress::new(bus, device, function);

        // Add bridge to parent bus
        if let Some(parent_bus) = self.buses.get_mut(&bus) {
            self.stats.record_device();
            self.stats.record_bridge();
            parent_bus.add_device(device, function, PciDeviceSlot::new_bridge(address, config));
        }

        // Create secondary bus
        self.buses
            .insert(secondary_bus, PciBus::new(secondary_bus, Some(address)));

        Some(secondary_bus)
    }

    /// Add device to specific bus
    pub fn add_device_to_bus(
        &mut self,
        bus: u8,
        device: u8,
        function: u8,
        config: ConfigSpace,
    ) -> bool {
        if let Some(pci_bus) = self.buses.get_mut(&bus) {
            let address = PciAddress::new(bus, device, function);
            self.stats.record_device();
            pci_bus.add_device(device, function, PciDeviceSlot::new_device(address, config));
            true
        } else {
            false
        }
    }

    /// Get bus
    pub fn bus(&self, number: u8) -> Option<&PciBus> {
        self.buses.get(&number)
    }

    /// Get mutable bus
    pub fn bus_mut(&mut self, number: u8) -> Option<&mut PciBus> {
        self.buses.get_mut(&number)
    }

    /// Get device by address
    pub fn device(&self, address: &PciAddress) -> Option<&PciDeviceSlot> {
        self.buses
            .get(&address.bus)?
            .device(address.device, address.function)
    }

    /// Get mutable device by address
    pub fn device_mut(&mut self, address: &PciAddress) -> Option<&mut PciDeviceSlot> {
        self.buses
            .get_mut(&address.bus)?
            .device_mut(address.device, address.function)
    }

    /// Read configuration space
    pub fn read_config(&self, address: &PciAddress, offset: u16) -> u32 {
        self.stats.record_read();

        if let Some(device) = self.device(address) {
            device.read_config(offset)
        } else {
            0xFFFFFFFF // No device present
        }
    }

    /// Write configuration space
    pub fn write_config(&mut self, address: &PciAddress, offset: u16, value: u32) {
        self.stats.record_write();

        if let Some(device) = self.device_mut(address) {
            device.write_config(offset, value);
        }
    }

    /// Read configuration via ECAM offset
    pub fn read_ecam(&self, ecam_offset: u64) -> u32 {
        let offset = ecam_offset - self.ecam_base;
        let bus = ((offset >> 20) & 0xFF) as u8;
        let device = ((offset >> 15) & 0x1F) as u8;
        let function = ((offset >> 12) & 0x07) as u8;
        let register = (offset & 0xFFF) as u16;

        let address = PciAddress::new(bus, device, function);
        self.read_config(&address, register)
    }

    /// Write configuration via ECAM offset
    pub fn write_ecam(&mut self, ecam_offset: u64, value: u32) {
        let offset = ecam_offset - self.ecam_base;
        let bus = ((offset >> 20) & 0xFF) as u8;
        let device = ((offset >> 15) & 0x1F) as u8;
        let function = ((offset >> 12) & 0x07) as u8;
        let register = (offset & 0xFFF) as u16;

        let address = PciAddress::new(bus, device, function);
        self.write_config(&address, register, value);
    }

    /// Enumerate all devices
    pub fn enumerate(&self) -> Vec<PciDeviceInfo> {
        let mut devices = Vec::new();

        for bus in self.buses.values() {
            for slot in bus.devices() {
                devices.push(slot.info.clone());
            }
        }

        devices
    }

    /// Find device by vendor/device ID
    pub fn find_device(&self, vendor: VendorId, device: DeviceId) -> Option<PciAddress> {
        for (bus_num, bus) in &self.buses {
            for ((dev, func), slot) in &bus.devices {
                if slot.info.vendor_id == vendor && slot.info.device_id == device {
                    return Some(PciAddress::new(*bus_num, *dev, *func));
                }
            }
        }
        None
    }

    /// Find devices by class code
    pub fn find_by_class(&self, class: ClassCode) -> Vec<PciAddress> {
        let mut found = Vec::new();

        for (bus_num, bus) in &self.buses {
            for ((dev, func), slot) in &bus.devices {
                if slot.info.class_code == class {
                    found.push(PciAddress::new(*bus_num, *dev, *func));
                }
            }
        }

        found
    }

    /// Get total device count
    pub fn device_count(&self) -> usize {
        self.buses.values().map(|b| b.device_count()).sum()
    }

    /// Get bus count
    pub fn bus_count(&self) -> usize {
        self.buses.len()
    }

    /// Update subordinate bus numbers after enumeration
    pub fn update_subordinate_numbers(&mut self) {
        // Collect bridge addresses and their secondary bus numbers
        let bridge_info: Vec<(PciAddress, u8)> = self
            .enumerate()
            .into_iter()
            .filter(|d| d.is_bridge())
            .filter_map(|d| {
                self.device(&d.address)
                    .and_then(|slot| slot.bridge_config())
                    .map(|bridge| (d.address, bridge.secondary_bus()))
            })
            .collect();

        // Calculate max subordinates first
        let updates: Vec<(PciAddress, u8, u8)> = bridge_info
            .iter()
            .map(|(addr, secondary)| {
                let max_sub = self.find_max_subordinate(*secondary);
                (*addr, *secondary, max_sub)
            })
            .collect();

        // Apply updates
        for (bridge_addr, secondary, max_sub) in updates {
            if let Some(slot) = self.device_mut(&bridge_addr) {
                if let Some(bridge) = slot.bridge_config_mut() {
                    bridge.set_bus_numbers(bridge_addr.bus, secondary, max_sub);
                }
            }
        }
    }

    /// Find maximum subordinate bus number reachable from a bus
    fn find_max_subordinate(&self, bus: u8) -> u8 {
        let mut max = bus;

        if let Some(pci_bus) = self.buses.get(&bus) {
            for slot in pci_bus.devices() {
                if let Some(bridge) = slot.bridge_config() {
                    let sub = bridge.subordinate_bus();
                    max = max.max(sub);
                    max = max.max(self.find_max_subordinate(bridge.secondary_bus()));
                }
            }
        }

        max
    }

    /// Check if address is in ECAM range
    pub fn is_ecam_address(&self, address: u64) -> bool {
        let ecam_end = self.ecam_base + (256 * 32 * 8 * 4096) as u64; // Full ECAM space
        address >= self.ecam_base && address < ecam_end
    }
}

/// Bridge forwarding helper
pub struct BridgeForwarder;

impl BridgeForwarder {
    /// Check if address should be forwarded through bridge
    pub fn should_forward_memory(
        bridge: &BridgeConfigSpace,
        address: u64,
        is_prefetchable: bool,
    ) -> bool {
        if is_prefetchable {
            // Check prefetchable memory window
            let base = ((bridge.base().read_u16(0x24) as u64 & 0xFFF0) << 16)
                | ((bridge.base().read_u32(0x28) as u64) << 32);
            let limit = ((bridge.base().read_u16(0x26) as u64 & 0xFFF0) << 16)
                | ((bridge.base().read_u32(0x2C) as u64) << 32)
                | 0xFFFFF;

            address >= base && address <= limit
        } else {
            // Check non-prefetchable memory window
            let (base, limit) = bridge.memory_window();
            address >= base as u64 && address <= limit as u64
        }
    }

    /// Check if I/O address should be forwarded
    pub fn should_forward_io(bridge: &BridgeConfigSpace, address: u32) -> bool {
        let io_base = ((bridge.base().read_u8(0x1C) as u32 & 0xF0) << 8)
            | ((bridge.base().read_u16(0x30) as u32) << 16);
        let io_limit = ((bridge.base().read_u8(0x1D) as u32 & 0xF0) << 8)
            | ((bridge.base().read_u16(0x32) as u32) << 16)
            | 0xFFF;

        address >= io_base && address <= io_limit
    }

    /// Check if config access should be forwarded
    pub fn should_forward_config(bridge: &BridgeConfigSpace, target_bus: u8) -> bool {
        let secondary = bridge.secondary_bus();
        let subordinate = bridge.subordinate_bus();

        target_bus >= secondary && target_bus <= subordinate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_bus_creation() {
        let bus = PciBus::new(0, None);
        assert_eq!(bus.number, 0);
        assert!(bus.parent_bridge.is_none());
        assert_eq!(bus.device_count(), 0);
    }

    #[test]
    fn test_pci_bus_add_device() {
        let mut bus = PciBus::new(0, None);
        let config =
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01);
        let address = PciAddress::new(0, 1, 0);
        bus.add_device(1, 0, PciDeviceSlot::new_device(address, config));

        assert_eq!(bus.device_count(), 1);
        assert!(bus.has_device(1, 0));
    }

    #[test]
    fn test_pci_root_complex_creation() {
        let rc = PciRootComplex::new();
        assert_eq!(rc.segment, 0);
        assert_eq!(rc.bus_count(), 1); // Root bus
    }

    #[test]
    fn test_pci_root_complex_add_device() {
        let mut rc = PciRootComplex::new();
        let config =
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01);
        rc.add_device(1, 0, config);

        assert_eq!(rc.device_count(), 1);

        let addr = PciAddress::new(0, 1, 0);
        assert!(rc.device(&addr).is_some());
    }

    #[test]
    fn test_pci_root_complex_add_bridge() {
        let mut rc = PciRootComplex::new();
        let config = BridgeConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), 0x01);

        let secondary = rc.add_bridge(0, 2, 0, config).unwrap();
        assert_eq!(secondary, 1);
        assert_eq!(rc.bus_count(), 2);
    }

    #[test]
    fn test_pci_root_complex_config_access() {
        let mut rc = PciRootComplex::new();
        let config =
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01);
        rc.add_device(1, 0, config);

        let addr = PciAddress::new(0, 1, 0);

        // Read vendor ID
        let vendor = rc.read_config(&addr, 0x00) as u16;
        assert_eq!(vendor, VendorId::INTEL.0);

        // Write command register
        rc.write_config(&addr, 0x04, 0x07);
        let cmd = rc.read_config(&addr, 0x04) as u16;
        assert_eq!(cmd & 0x07, 0x07);
    }

    #[test]
    fn test_pci_root_complex_find_device() {
        let mut rc = PciRootComplex::new();
        let config =
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01);
        rc.add_device(3, 0, config);

        let found = rc.find_device(VendorId::INTEL, DeviceId(0x1234));
        assert!(found.is_some());
        assert_eq!(found.unwrap().device, 3);
    }

    #[test]
    fn test_pci_root_complex_find_by_class() {
        let mut rc = PciRootComplex::new();

        rc.add_device(
            1,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01),
        );
        rc.add_device(
            2,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x5678), ClassCode::ETHERNET, 0x01),
        );
        rc.add_device(
            3,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x9ABC), ClassCode::VGA, 0x01),
        );

        let eth_devices = rc.find_by_class(ClassCode::ETHERNET);
        assert_eq!(eth_devices.len(), 2);
    }

    #[test]
    fn test_pci_root_complex_enumerate() {
        let mut rc = PciRootComplex::new();
        rc.add_device(
            1,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01),
        );
        rc.add_device(
            2,
            0,
            ConfigSpace::with_device(VendorId::AMD, DeviceId(0x5678), ClassCode::VGA, 0x01),
        );

        let devices = rc.enumerate();
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn test_pci_root_complex_ecam() {
        let mut rc = PciRootComplex::new();
        rc.set_ecam_base(0xE0000000);

        rc.add_device(
            1,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01),
        );

        // ECAM offset for bus 0, device 1, function 0, register 0
        let ecam_addr = 0xE0000000 + (0 << 20) + (1 << 15) + (0 << 12) + 0;

        let vendor = rc.read_ecam(ecam_addr) as u16;
        assert_eq!(vendor, VendorId::INTEL.0);
    }

    #[test]
    fn test_pci_root_complex_stats() {
        let mut rc = PciRootComplex::new();
        rc.add_device(
            1,
            0,
            ConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), ClassCode::ETHERNET, 0x01),
        );

        let addr = PciAddress::new(0, 1, 0);
        rc.read_config(&addr, 0);
        rc.read_config(&addr, 0);
        rc.write_config(&addr, 4, 0);

        let stats = rc.stats().snapshot();
        assert_eq!(stats.config_reads, 2);
        assert_eq!(stats.config_writes, 1);
        assert_eq!(stats.devices_found, 1);
    }

    #[test]
    fn test_pci_device_info() {
        let config = ConfigSpace::with_device(
            VendorId::NVIDIA,
            DeviceId(0x1234),
            ClassCode::DISPLAY_3D,
            0x01,
        );
        let address = PciAddress::new(0, 1, 0);

        let info = PciDeviceInfo::from_config(address, &config).unwrap();
        assert_eq!(info.vendor_id, VendorId::NVIDIA);
        assert!(info.class_code.is_display());
    }

    #[test]
    fn test_bridge_forwarding_memory() {
        let mut bridge = BridgeConfigSpace::new();
        bridge.set_memory_window(0xE0000000, 0xEFFFFFFF);

        assert!(BridgeForwarder::should_forward_memory(
            &bridge, 0xE5000000, false
        ));
        assert!(!BridgeForwarder::should_forward_memory(
            &bridge, 0xF0000000, false
        ));
    }

    #[test]
    fn test_bridge_forwarding_config() {
        let mut bridge = BridgeConfigSpace::new();
        bridge.set_bus_numbers(0, 1, 5);

        assert!(BridgeForwarder::should_forward_config(&bridge, 1));
        assert!(BridgeForwarder::should_forward_config(&bridge, 3));
        assert!(BridgeForwarder::should_forward_config(&bridge, 5));
        assert!(!BridgeForwarder::should_forward_config(&bridge, 6));
    }

    #[test]
    fn test_pci_device_slot_bridge() {
        let config = BridgeConfigSpace::with_device(VendorId::INTEL, DeviceId(0x1234), 0x01);
        let address = PciAddress::new(0, 1, 0);
        let slot = PciDeviceSlot::new_bridge(address, config);

        assert!(slot.is_bridge());
        assert!(slot.bridge_config().is_some());
        assert!(slot.config().is_none());
    }

    #[test]
    fn test_ecam_address_check() {
        let mut rc = PciRootComplex::new();
        rc.set_ecam_base(0xE0000000);

        assert!(rc.is_ecam_address(0xE0000000));
        assert!(rc.is_ecam_address(0xEFFFFFFF));
        assert!(!rc.is_ecam_address(0xD0000000));
    }
}
