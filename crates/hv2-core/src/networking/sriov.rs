//! SR-IOV and Passthrough Support
//!
//! This module provides support for Single Root I/O Virtualization (SR-IOV)
//! and PCI device passthrough for high-performance networking.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// PCI address
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PciAddress {
    /// Domain (usually 0)
    pub domain: u16,
    /// Bus number
    pub bus: u8,
    /// Device number
    pub device: u8,
    /// Function number
    pub function: u8,
}

impl PciAddress {
    /// Create new PCI address
    pub fn new(domain: u16, bus: u8, device: u8, function: u8) -> Self {
        Self {
            domain,
            bus,
            device,
            function,
        }
    }

    /// Parse from BDF string (e.g., "0000:00:1f.0")
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return None;
        }

        let domain = u16::from_str_radix(parts[0], 16).ok()?;
        let bus = u8::from_str_radix(parts[1], 16).ok()?;

        let df: Vec<&str> = parts[2].split('.').collect();
        if df.len() != 2 {
            return None;
        }

        let device = u8::from_str_radix(df[0], 16).ok()?;
        let function = u8::from_str_radix(df[1], 16).ok()?;

        Some(Self {
            domain,
            bus,
            device,
            function,
        })
    }
}

impl std::fmt::Display for PciAddress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:04x}:{:02x}:{:02x}.{:x}",
            self.domain, self.bus, self.device, self.function
        )
    }
}

/// PCI device class
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PciClass {
    /// Network controller
    Network,
    /// Storage controller
    Storage,
    /// Display controller
    Display,
    /// Other
    Other(u8, u8),
}

impl PciClass {
    /// Parse from class code
    pub fn from_code(class: u8, subclass: u8) -> Self {
        match (class, subclass) {
            (0x02, _) => Self::Network,
            (0x01, _) => Self::Storage,
            (0x03, _) => Self::Display,
            _ => Self::Other(class, subclass),
        }
    }
}

/// SR-IOV capability
#[derive(Debug, Clone)]
pub struct SriovCapability {
    /// Total VFs supported
    pub total_vfs: u16,
    /// Currently enabled VFs
    pub num_vfs: u16,
    /// VF offset
    pub vf_offset: u16,
    /// VF stride
    pub vf_stride: u16,
    /// VF device ID
    pub vf_device_id: u16,
    /// Supported page sizes
    pub supported_page_sizes: u32,
    /// System page size
    pub system_page_size: u32,
    /// VF migration capable
    pub vf_migration: bool,
    /// ARI capable
    pub ari_capable: bool,
}

impl SriovCapability {
    /// Get VF's PCI address
    pub fn vf_address(&self, pf: &PciAddress, vf_index: u16) -> Option<PciAddress> {
        if vf_index >= self.num_vfs {
            return None;
        }

        let routing_id = (pf.bus as u16) << 8 | (pf.device as u16) << 3 | (pf.function as u16);
        let vf_routing_id = routing_id + self.vf_offset + vf_index * self.vf_stride;

        Some(PciAddress {
            domain: pf.domain,
            bus: (vf_routing_id >> 8) as u8,
            device: ((vf_routing_id >> 3) & 0x1F) as u8,
            function: (vf_routing_id & 0x7) as u8,
        })
    }
}

/// VF state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfState {
    /// VF not enabled
    Disabled,
    /// VF enabled but not assigned
    Enabled,
    /// VF assigned to VM
    Assigned,
    /// VF in error state
    Error,
}

/// Virtual Function
#[derive(Debug, Clone)]
pub struct VirtualFunction {
    /// VF index
    pub index: u16,
    /// VF PCI address
    pub address: PciAddress,
    /// VF state
    pub state: VfState,
    /// Assigned VM ID
    pub vm_id: Option<u64>,
    /// MAC address (for network VFs)
    pub mac: Option<[u8; 6]>,
    /// VLAN ID (for network VFs)
    pub vlan: Option<u16>,
    /// Spoofcheck enabled
    pub spoofcheck: bool,
    /// Trust mode
    pub trust: bool,
    /// Min TX rate (Mbps)
    pub min_tx_rate: u32,
    /// Max TX rate (Mbps)
    pub max_tx_rate: u32,
    /// Link state
    pub link_state: VfLinkState,
}

/// VF link state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VfLinkState {
    /// Auto (follows PF)
    #[default]
    Auto,
    /// Always up
    Enable,
    /// Always down
    Disable,
}

impl VirtualFunction {
    /// Create new VF
    pub fn new(index: u16, address: PciAddress) -> Self {
        Self {
            index,
            address,
            state: VfState::Disabled,
            vm_id: None,
            mac: None,
            vlan: None,
            spoofcheck: true,
            trust: false,
            min_tx_rate: 0,
            max_tx_rate: 0,
            link_state: VfLinkState::Auto,
        }
    }

    /// Check if VF can be assigned
    pub fn can_assign(&self) -> bool {
        self.state == VfState::Enabled
    }

    /// Check if VF is assigned
    pub fn is_assigned(&self) -> bool {
        self.state == VfState::Assigned
    }
}

/// Physical Function (SR-IOV capable device)
#[derive(Debug)]
pub struct PhysicalFunction {
    /// PCI address
    pub address: PciAddress,
    /// Device name
    pub name: String,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Device class
    pub device_class: PciClass,
    /// SR-IOV capability
    pub sriov: Option<SriovCapability>,
    /// Virtual functions
    vfs: RwLock<HashMap<u16, VirtualFunction>>,
    /// Driver bound
    pub driver: Option<String>,
    /// IOMMU group
    pub iommu_group: Option<u32>,
    /// Passthrough enabled
    passthrough_enabled: AtomicBool,
}

impl PhysicalFunction {
    /// Create new physical function
    pub fn new(address: PciAddress, vendor_id: u16, device_id: u16) -> Self {
        Self {
            address,
            name: format!("{}", address),
            vendor_id,
            device_id,
            device_class: PciClass::Other(0, 0),
            sriov: None,
            vfs: RwLock::new(HashMap::new()),
            driver: None,
            iommu_group: None,
            passthrough_enabled: AtomicBool::new(false),
        }
    }

    /// Check if SR-IOV capable
    pub fn is_sriov_capable(&self) -> bool {
        self.sriov.is_some()
    }

    /// Get total VFs
    pub fn total_vfs(&self) -> u16 {
        self.sriov.as_ref().map(|s| s.total_vfs).unwrap_or(0)
    }

    /// Get current VF count
    pub fn num_vfs(&self) -> u16 {
        self.sriov.as_ref().map(|s| s.num_vfs).unwrap_or(0)
    }

    /// Enable VFs
    pub fn enable_vfs(&mut self, count: u16) -> Result<(), SriovError> {
        let sriov = self.sriov.as_mut().ok_or(SriovError::NotSupported)?;

        if count > sriov.total_vfs {
            return Err(SriovError::TooManyVfs(count, sriov.total_vfs));
        }

        // Update num_vfs first so vf_address works
        sriov.num_vfs = count;

        // Create VF entries
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        vfs.clear();

        for i in 0..count {
            if let Some(addr) = sriov.vf_address(&self.address, i) {
                let mut vf = VirtualFunction::new(i, addr);
                vf.state = VfState::Enabled;
                vfs.insert(i, vf);
            }
        }

        Ok(())
    }

    /// Disable all VFs
    pub fn disable_vfs(&mut self) -> Result<(), SriovError> {
        let sriov = self.sriov.as_mut().ok_or(SriovError::NotSupported)?;

        // Check if any VFs are assigned
        let vfs = self.vfs.read().unwrap_or_else(|e| e.into_inner());
        if vfs.values().any(|vf| vf.is_assigned()) {
            return Err(SriovError::VfInUse);
        }
        drop(vfs);

        self.vfs.write().unwrap_or_else(|e| e.into_inner()).clear();
        sriov.num_vfs = 0;
        Ok(())
    }

    /// Get VF by index
    pub fn get_vf(&self, index: u16) -> Option<VirtualFunction> {
        self.vfs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&index)
            .cloned()
    }

    /// Get all VFs
    pub fn list_vfs(&self) -> Vec<VirtualFunction> {
        self.vfs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Assign VF to VM
    pub fn assign_vf(&self, vf_index: u16, vm_id: u64) -> Result<PciAddress, SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;

        if !vf.can_assign() {
            return Err(SriovError::VfNotAvailable(vf_index));
        }

        vf.state = VfState::Assigned;
        vf.vm_id = Some(vm_id);

        Ok(vf.address)
    }

    /// Release VF from VM
    pub fn release_vf(&self, vf_index: u16) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;

        vf.state = VfState::Enabled;
        vf.vm_id = None;

        Ok(())
    }

    /// Set VF MAC address
    pub fn set_vf_mac(&self, vf_index: u16, mac: [u8; 6]) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.mac = Some(mac);
        Ok(())
    }

    /// Set VF VLAN
    pub fn set_vf_vlan(&self, vf_index: u16, vlan: Option<u16>) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.vlan = vlan;
        Ok(())
    }

    /// Set VF rate limit
    pub fn set_vf_rate(
        &self,
        vf_index: u16,
        min_rate: u32,
        max_rate: u32,
    ) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.min_tx_rate = min_rate;
        vf.max_tx_rate = max_rate;
        Ok(())
    }

    /// Set VF spoofcheck
    pub fn set_vf_spoofcheck(&self, vf_index: u16, enabled: bool) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.spoofcheck = enabled;
        Ok(())
    }

    /// Set VF trust mode
    pub fn set_vf_trust(&self, vf_index: u16, trusted: bool) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.trust = trusted;
        Ok(())
    }

    /// Set VF link state
    pub fn set_vf_link_state(&self, vf_index: u16, state: VfLinkState) -> Result<(), SriovError> {
        let mut vfs = self.vfs.write().unwrap_or_else(|e| e.into_inner());
        let vf = vfs
            .get_mut(&vf_index)
            .ok_or(SriovError::VfNotFound(vf_index))?;
        vf.link_state = state;
        Ok(())
    }

    /// Enable passthrough mode
    pub fn enable_passthrough(&self) {
        self.passthrough_enabled.store(true, Ordering::Release);
    }

    /// Disable passthrough mode
    pub fn disable_passthrough(&self) {
        self.passthrough_enabled.store(false, Ordering::Release);
    }

    /// Check if passthrough enabled
    pub fn is_passthrough_enabled(&self) -> bool {
        self.passthrough_enabled.load(Ordering::Acquire)
    }
}

/// SR-IOV error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SriovError {
    /// Device doesn't support SR-IOV
    #[error("Device does not support SR-IOV")]
    NotSupported,
    /// Too many VFs requested
    #[error("Requested {0} VFs but maximum is {1}")]
    TooManyVfs(u16, u16),
    /// VF not found
    #[error("VF {0} not found")]
    VfNotFound(u16),
    /// VF not available for assignment
    #[error("VF {0} not available for assignment")]
    VfNotAvailable(u16),
    /// VF is in use
    #[error("Cannot disable VFs while in use")]
    VfInUse,
    /// IOMMU not available
    #[error("IOMMU not available")]
    NoIommu,
    /// Device busy
    #[error("Device is busy")]
    DeviceBusy,
    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
    /// Driver error
    #[error("Driver error: {0}")]
    DriverError(String),
}

/// Passthrough device assignment
#[derive(Debug, Clone)]
pub struct DeviceAssignment {
    /// Device PCI address
    pub device: PciAddress,
    /// Guest PCI address
    pub guest_address: Option<PciAddress>,
    /// VM ID
    pub vm_id: u64,
    /// Is VF
    pub is_vf: bool,
    /// PF address (if VF)
    pub pf_address: Option<PciAddress>,
    /// IOMMU domain
    pub iommu_domain: Option<u64>,
}

/// SR-IOV manager
#[derive(Debug, Default)]
pub struct SriovManager {
    /// Physical functions
    pfs: RwLock<HashMap<PciAddress, Arc<PhysicalFunction>>>,
    /// Device assignments
    assignments: RwLock<HashMap<PciAddress, DeviceAssignment>>,
    /// Statistics
    stats: SriovStats,
}

/// SR-IOV statistics
#[derive(Debug, Default)]
pub struct SriovStats {
    /// Total PFs
    pub total_pfs: AtomicU64,
    /// Total VFs
    pub total_vfs: AtomicU64,
    /// Assigned VFs
    pub assigned_vfs: AtomicU64,
    /// Assignment operations
    pub assignments: AtomicU64,
    /// Release operations
    pub releases: AtomicU64,
}

impl SriovManager {
    /// Create new SR-IOV manager
    pub fn new() -> Self {
        Self {
            pfs: RwLock::new(HashMap::new()),
            assignments: RwLock::new(HashMap::new()),
            stats: SriovStats::default(),
        }
    }

    /// Register physical function
    pub fn register_pf(&self, pf: PhysicalFunction) -> PciAddress {
        let address = pf.address;
        let arc_pf = Arc::new(pf);
        self.pfs
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(address, arc_pf);
        self.stats.total_pfs.fetch_add(1, Ordering::Relaxed);
        address
    }

    /// Unregister physical function
    pub fn unregister_pf(&self, address: PciAddress) -> Option<Arc<PhysicalFunction>> {
        let pf = self
            .pfs
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&address);
        if pf.is_some() {
            self.stats.total_pfs.fetch_sub(1, Ordering::Relaxed);
        }
        pf
    }

    /// Get physical function
    pub fn get_pf(&self, address: PciAddress) -> Option<Arc<PhysicalFunction>> {
        self.pfs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&address)
            .cloned()
    }

    /// List all PFs
    pub fn list_pfs(&self) -> Vec<PciAddress> {
        self.pfs
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Assign device to VM
    pub fn assign_device(
        &self,
        device: PciAddress,
        vm_id: u64,
        guest_address: Option<PciAddress>,
    ) -> Result<DeviceAssignment, SriovError> {
        // Check if already assigned
        if self
            .assignments
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(&device)
        {
            return Err(SriovError::DeviceBusy);
        }

        let assignment = DeviceAssignment {
            device,
            guest_address,
            vm_id,
            is_vf: false,
            pf_address: None,
            iommu_domain: None,
        };

        self.assignments
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(device, assignment.clone());
        self.stats.assignments.fetch_add(1, Ordering::Relaxed);

        Ok(assignment)
    }

    /// Release device from VM
    pub fn release_device(&self, device: PciAddress) -> Result<(), SriovError> {
        if self
            .assignments
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&device)
            .is_some()
        {
            self.stats.releases.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            Err(SriovError::VfNotFound(0))
        }
    }

    /// Get device assignment
    pub fn get_assignment(&self, device: PciAddress) -> Option<DeviceAssignment> {
        self.assignments
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&device)
            .cloned()
    }

    /// List assignments for VM
    pub fn list_vm_assignments(&self, vm_id: u64) -> Vec<DeviceAssignment> {
        self.assignments
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|a| a.vm_id == vm_id)
            .cloned()
            .collect()
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.stats.total_pfs.load(Ordering::Relaxed),
            self.stats.total_vfs.load(Ordering::Relaxed),
            self.stats.assigned_vfs.load(Ordering::Relaxed),
            self.stats.assignments.load(Ordering::Relaxed),
            self.stats.releases.load(Ordering::Relaxed),
        )
    }
}

/// IOMMU group
#[derive(Debug, Clone)]
pub struct IommuGroup {
    /// Group ID
    pub id: u32,
    /// Devices in group
    pub devices: Vec<PciAddress>,
    /// Viable for passthrough
    pub viable: bool,
}

impl IommuGroup {
    /// Create new IOMMU group
    pub fn new(id: u32) -> Self {
        Self {
            id,
            devices: Vec::new(),
            viable: true,
        }
    }

    /// Add device to group
    pub fn add_device(&mut self, device: PciAddress) {
        if !self.devices.contains(&device) {
            self.devices.push(device);
        }
    }

    /// Check if group contains only one device
    pub fn is_isolated(&self) -> bool {
        self.devices.len() == 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pci_address_new() {
        let addr = PciAddress::new(0, 1, 2, 3);
        assert_eq!(addr.domain, 0);
        assert_eq!(addr.bus, 1);
        assert_eq!(addr.device, 2);
        assert_eq!(addr.function, 3);
    }

    #[test]
    fn test_pci_address_parse() {
        let addr = PciAddress::parse("0000:00:1f.0").unwrap();
        assert_eq!(addr.domain, 0);
        assert_eq!(addr.bus, 0);
        assert_eq!(addr.device, 0x1f);
        assert_eq!(addr.function, 0);
    }

    #[test]
    fn test_pci_address_display() {
        let addr = PciAddress::new(0, 0, 0x1f, 0);
        assert_eq!(addr.to_string(), "0000:00:1f.0");
    }

    #[test]
    fn test_pci_class() {
        assert_eq!(PciClass::from_code(0x02, 0x00), PciClass::Network);
        assert_eq!(PciClass::from_code(0x01, 0x00), PciClass::Storage);
        assert_eq!(PciClass::from_code(0x03, 0x00), PciClass::Display);
    }

    #[test]
    fn test_sriov_capability_vf_address() {
        let cap = SriovCapability {
            total_vfs: 64,
            num_vfs: 4,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x1234,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        };

        let pf = PciAddress::new(0, 3, 0, 0);
        let vf0 = cap.vf_address(&pf, 0).unwrap();
        let vf1 = cap.vf_address(&pf, 1).unwrap();

        assert_ne!(vf0, vf1);
    }

    #[test]
    fn test_virtual_function() {
        let vf = VirtualFunction::new(0, PciAddress::new(0, 3, 0, 1));
        assert_eq!(vf.state, VfState::Disabled);
        assert!(!vf.can_assign());
    }

    #[test]
    fn test_physical_function() {
        let pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);
        assert!(!pf.is_sriov_capable());
        assert_eq!(pf.total_vfs(), 0);
    }

    #[test]
    fn test_physical_function_with_sriov() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 64,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        assert!(pf.is_sriov_capable());
        assert_eq!(pf.total_vfs(), 64);
    }

    #[test]
    fn test_enable_vfs() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 64,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        pf.enable_vfs(4).unwrap();
        assert_eq!(pf.num_vfs(), 4);
        assert_eq!(pf.list_vfs().len(), 4);
    }

    #[test]
    fn test_enable_too_many_vfs() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 4,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        let result = pf.enable_vfs(8);
        assert!(matches!(result, Err(SriovError::TooManyVfs(8, 4))));
    }

    #[test]
    fn test_assign_vf() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 64,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        pf.enable_vfs(4).unwrap();
        let addr = pf.assign_vf(0, 1).unwrap();
        assert!(addr.function > 0 || addr.device > 0);

        let vf = pf.get_vf(0).unwrap();
        assert_eq!(vf.state, VfState::Assigned);
        assert_eq!(vf.vm_id, Some(1));
    }

    #[test]
    fn test_release_vf() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 64,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        pf.enable_vfs(4).unwrap();
        pf.assign_vf(0, 1).unwrap();
        pf.release_vf(0).unwrap();

        let vf = pf.get_vf(0).unwrap();
        assert_eq!(vf.state, VfState::Enabled);
        assert_eq!(vf.vm_id, None);
    }

    #[test]
    fn test_vf_mac() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 4,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        pf.enable_vfs(1).unwrap();
        let mac = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55];
        pf.set_vf_mac(0, mac).unwrap();

        let vf = pf.get_vf(0).unwrap();
        assert_eq!(vf.mac, Some(mac));
    }

    #[test]
    fn test_sriov_manager() {
        let manager = SriovManager::new();

        let pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        let addr = manager.register_pf(pf);
        assert!(manager.get_pf(addr).is_some());
        assert_eq!(manager.list_pfs().len(), 1);
    }

    #[test]
    fn test_device_assignment() {
        let manager = SriovManager::new();
        let device = PciAddress::new(0, 3, 0, 0);

        let assignment = manager.assign_device(device, 1, None).unwrap();
        assert_eq!(assignment.vm_id, 1);

        // Cannot assign again
        assert!(matches!(
            manager.assign_device(device, 2, None),
            Err(SriovError::DeviceBusy)
        ));
    }

    #[test]
    fn test_release_device() {
        let manager = SriovManager::new();
        let device = PciAddress::new(0, 3, 0, 0);

        manager.assign_device(device, 1, None).unwrap();
        manager.release_device(device).unwrap();

        // Can assign again
        manager.assign_device(device, 2, None).unwrap();
    }

    #[test]
    fn test_iommu_group() {
        let mut group = IommuGroup::new(1);
        assert!(group.devices.is_empty());

        group.add_device(PciAddress::new(0, 3, 0, 0));
        assert!(group.is_isolated());

        group.add_device(PciAddress::new(0, 3, 0, 1));
        assert!(!group.is_isolated());
    }

    #[test]
    fn test_vf_link_state() {
        let mut pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        pf.sriov = Some(SriovCapability {
            total_vfs: 4,
            num_vfs: 0,
            vf_offset: 1,
            vf_stride: 1,
            vf_device_id: 0x10ed,
            supported_page_sizes: 0x553,
            system_page_size: 0x1,
            vf_migration: false,
            ari_capable: true,
        });

        pf.enable_vfs(1).unwrap();
        pf.set_vf_link_state(0, VfLinkState::Enable).unwrap();

        let vf = pf.get_vf(0).unwrap();
        assert_eq!(vf.link_state, VfLinkState::Enable);
    }

    #[test]
    fn test_passthrough_mode() {
        let pf = PhysicalFunction::new(PciAddress::new(0, 3, 0, 0), 0x8086, 0x10fb);

        assert!(!pf.is_passthrough_enabled());
        pf.enable_passthrough();
        assert!(pf.is_passthrough_enabled());
        pf.disable_passthrough();
        assert!(!pf.is_passthrough_enabled());
    }
}
