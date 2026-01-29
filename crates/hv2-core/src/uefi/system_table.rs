//! EFI System Table and Services
//!
//! This module provides the EFI System Table, Boot Services,
//! and Configuration Table implementations.

use super::types::{
    AllocateType, Guid, Handle, MemoryAttribute, MemoryDescriptor, MemoryType, Status,
    TableHeader, Time, TimeCapabilities, guids,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Boot services function indices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum BootServiceFunction {
    // Task Priority Services
    RaiseTPL = 0,
    RestoreTPL = 1,

    // Memory Services
    AllocatePages = 2,
    FreePages = 3,
    GetMemoryMap = 4,
    AllocatePool = 5,
    FreePool = 6,

    // Event & Timer Services
    CreateEvent = 7,
    SetTimer = 8,
    WaitForEvent = 9,
    SignalEvent = 10,
    CloseEvent = 11,
    CheckEvent = 12,

    // Protocol Handler Services
    InstallProtocolInterface = 13,
    ReinstallProtocolInterface = 14,
    UninstallProtocolInterface = 15,
    HandleProtocol = 16,
    RegisterProtocolNotify = 18,
    LocateHandle = 19,
    LocateDevicePath = 20,
    InstallConfigurationTable = 21,

    // Image Services
    LoadImage = 22,
    StartImage = 23,
    Exit = 24,
    UnloadImage = 25,
    ExitBootServices = 26,

    // Miscellaneous Services
    GetNextMonotonicCount = 27,
    Stall = 28,
    SetWatchdogTimer = 29,

    // DriverSupport Services
    ConnectController = 30,
    DisconnectController = 31,

    // Open/Close Protocol Services
    OpenProtocol = 32,
    CloseProtocol = 33,
    OpenProtocolInformation = 34,

    // Library Services
    ProtocolsPerHandle = 35,
    LocateHandleBuffer = 36,
    LocateProtocol = 37,
    InstallMultipleProtocolInterfaces = 38,
    UninstallMultipleProtocolInterfaces = 39,

    // CRC Services
    CalculateCrc32 = 40,

    // Miscellaneous Services
    CopyMem = 41,
    SetMem = 42,
    CreateEventEx = 43,
}

/// Task Priority Level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u64)]
pub enum Tpl {
    /// Application level
    Application = 4,
    /// Callback level
    Callback = 8,
    /// Notify level
    Notify = 16,
    /// High level
    HighLevel = 31,
}

/// Timer delay type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum TimerDelay {
    /// Cancel timer
    Cancel = 0,
    /// Periodic timer
    Periodic = 1,
    /// Relative timer
    Relative = 2,
}

/// Event type flags
#[derive(Debug, Clone, Copy)]
pub struct EventType(pub u32);

impl EventType {
    /// Timer event
    pub const TIMER: EventType = EventType(0x80000000);
    /// Runtime event
    pub const RUNTIME: EventType = EventType(0x40000000);
    /// Notify wait
    pub const NOTIFY_WAIT: EventType = EventType(0x00000100);
    /// Notify signal
    pub const NOTIFY_SIGNAL: EventType = EventType(0x00000200);
    /// Signal ExitBootServices
    pub const SIGNAL_EXIT_BOOT_SERVICES: EventType = EventType(0x00000201);
    /// Signal virtual address change
    pub const SIGNAL_VIRTUAL_ADDRESS_CHANGE: EventType = EventType(0x60000202);
}

/// Locate search type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum LocateSearchType {
    /// All handles
    AllHandles = 0,
    /// By register notify
    ByRegisterNotify = 1,
    /// By protocol
    ByProtocol = 2,
}

/// EFI Event
#[derive(Debug, Clone)]
pub struct Event {
    /// Event ID
    pub id: u64,
    /// Event type
    pub event_type: u32,
    /// Notify TPL
    pub notify_tpl: Tpl,
    /// Is signaled
    pub signaled: bool,
    /// Timer period (100ns units)
    pub timer_period: u64,
    /// Timer trigger time
    pub timer_trigger: u64,
}

impl Event {
    /// Create new event
    pub fn new(id: u64, event_type: u32, notify_tpl: Tpl) -> Self {
        Self {
            id,
            event_type,
            notify_tpl,
            signaled: false,
            timer_period: 0,
            timer_trigger: 0,
        }
    }

    /// Check if timer event
    pub fn is_timer(&self) -> bool {
        (self.event_type & EventType::TIMER.0) != 0
    }
}

/// Configuration table entry
#[derive(Debug, Clone)]
pub struct ConfigurationTable {
    /// Vendor GUID
    pub vendor_guid: Guid,
    /// Vendor table address
    pub vendor_table: u64,
}

impl ConfigurationTable {
    /// Create new configuration table
    pub fn new(vendor_guid: Guid, vendor_table: u64) -> Self {
        Self {
            vendor_guid,
            vendor_table,
        }
    }
}

/// Memory allocation entry
#[derive(Debug, Clone)]
struct MemoryAllocation {
    /// Physical address
    address: u64,
    /// Number of pages
    pages: u64,
    /// Memory type
    memory_type: MemoryType,
}

/// Protocol interface
#[derive(Debug, Clone)]
pub struct ProtocolInterface {
    /// Protocol GUID
    pub guid: Guid,
    /// Interface address
    pub interface: u64,
}

/// Handle database entry
#[derive(Debug, Clone, Default)]
pub struct HandleEntry {
    /// Installed protocols
    pub protocols: Vec<ProtocolInterface>,
}

/// Boot Services statistics
#[derive(Debug, Default)]
pub struct BootServicesStats {
    /// Memory allocations
    allocations: AtomicU64,
    /// Memory frees
    frees: AtomicU64,
    /// Events created
    events_created: AtomicU64,
    /// Protocol installs
    protocol_installs: AtomicU64,
    /// Images loaded
    images_loaded: AtomicU64,
}

impl BootServicesStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Get snapshot
    pub fn snapshot(&self) -> BootServicesStatsSnapshot {
        BootServicesStatsSnapshot {
            allocations: self.allocations.load(Ordering::Relaxed),
            frees: self.frees.load(Ordering::Relaxed),
            events_created: self.events_created.load(Ordering::Relaxed),
            protocol_installs: self.protocol_installs.load(Ordering::Relaxed),
            images_loaded: self.images_loaded.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct BootServicesStatsSnapshot {
    /// Memory allocations
    pub allocations: u64,
    /// Memory frees
    pub frees: u64,
    /// Events created
    pub events_created: u64,
    /// Protocol installs
    pub protocol_installs: u64,
    /// Images loaded
    pub images_loaded: u64,
}

/// EFI Boot Services
pub struct BootServices {
    /// Header
    header: TableHeader,
    /// Current TPL
    current_tpl: Tpl,
    /// Memory map
    memory_map: Vec<MemoryDescriptor>,
    /// Memory allocations
    allocations: Vec<MemoryAllocation>,
    /// Memory map key (changes on modification)
    memory_map_key: u64,
    /// Next handle ID
    next_handle: u64,
    /// Handle database
    handles: HashMap<u64, HandleEntry>,
    /// Events
    events: HashMap<u64, Event>,
    /// Next event ID
    next_event: u64,
    /// Monotonic count
    monotonic_count: u64,
    /// Boot services exited
    exited: bool,
    /// Statistics
    stats: BootServicesStats,
}

impl Default for BootServices {
    fn default() -> Self {
        Self::new()
    }
}

impl BootServices {
    /// Create new boot services
    pub fn new() -> Self {
        Self {
            header: TableHeader::new(
                TableHeader::BOOT_SERVICES_SIGNATURE,
                TableHeader::UEFI_2_10_REVISION,
                std::mem::size_of::<BootServices>() as u32,
            ),
            current_tpl: Tpl::Application,
            memory_map: Vec::new(),
            allocations: Vec::new(),
            memory_map_key: 1,
            next_handle: 1,
            handles: HashMap::new(),
            events: HashMap::new(),
            next_event: 1,
            monotonic_count: 0,
            exited: false,
            stats: BootServicesStats::new(),
        }
    }

    /// Check if boot services have exited
    pub fn is_exited(&self) -> bool {
        self.exited
    }

    /// Get current TPL
    pub fn current_tpl(&self) -> Tpl {
        self.current_tpl
    }

    /// Get statistics
    pub fn stats(&self) -> &BootServicesStats {
        &self.stats
    }

    /// Add memory region to map
    pub fn add_memory_region(&mut self, descriptor: MemoryDescriptor) {
        self.memory_map.push(descriptor);
        self.memory_map_key += 1;
    }

    /// Initialize default memory map
    pub fn init_default_memory_map(&mut self, memory_size: u64) {
        // Low memory (0 - 1MB) - reserved for legacy
        self.add_memory_region(MemoryDescriptor::new(
            MemoryType::ReservedMemoryType,
            0,
            256, // 1MB
            MemoryAttribute::WB,
        ));

        // Conventional memory (1MB - memory_size - 16MB)
        let conventional_pages = (memory_size - 0x100000 - 0x1000000) / 4096;
        self.add_memory_region(MemoryDescriptor::new(
            MemoryType::ConventionalMemory,
            0x100000,
            conventional_pages,
            MemoryAttribute::WB,
        ));

        // Runtime services data (top 16MB)
        self.add_memory_region(MemoryDescriptor::new(
            MemoryType::RuntimeServicesData,
            memory_size - 0x1000000,
            4096, // 16MB
            MemoryAttribute::WB.or(MemoryAttribute::RUNTIME),
        ));
    }

    /// Raise TPL
    pub fn raise_tpl(&mut self, new_tpl: Tpl) -> Tpl {
        let old_tpl = self.current_tpl;
        if new_tpl > old_tpl {
            self.current_tpl = new_tpl;
        }
        old_tpl
    }

    /// Restore TPL
    pub fn restore_tpl(&mut self, old_tpl: Tpl) {
        self.current_tpl = old_tpl;
    }

    /// Allocate pages
    pub fn allocate_pages(
        &mut self,
        allocate_type: AllocateType,
        memory_type: MemoryType,
        pages: u64,
        address: u64,
    ) -> Result<u64, Status> {
        if self.exited {
            return Err(Status::UNSUPPORTED);
        }

        // Find suitable region
        let alloc_address = match allocate_type {
            AllocateType::AllocateAddress => {
                // Check if address is available
                if self.is_address_allocated(address, pages) {
                    return Err(Status::NOT_FOUND);
                }
                address
            }
            AllocateType::AllocateAnyPages => {
                // Find first available region
                self.find_free_pages(pages, 0)?
            }
            AllocateType::AllocateMaxAddress => {
                // Find highest available region below address
                self.find_free_pages_below(pages, address)?
            }
        };

        // Record allocation
        self.allocations.push(MemoryAllocation {
            address: alloc_address,
            pages,
            memory_type,
        });

        self.memory_map_key += 1;
        self.stats.allocations.fetch_add(1, Ordering::Relaxed);

        Ok(alloc_address)
    }

    /// Free pages
    pub fn free_pages(&mut self, address: u64, pages: u64) -> Status {
        if self.exited {
            return Status::UNSUPPORTED;
        }

        // Find and remove allocation
        if let Some(pos) = self
            .allocations
            .iter()
            .position(|a| a.address == address && a.pages == pages)
        {
            self.allocations.remove(pos);
            self.memory_map_key += 1;
            self.stats.frees.fetch_add(1, Ordering::Relaxed);
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }

    /// Get memory map
    pub fn get_memory_map(&self) -> (&[MemoryDescriptor], u64, usize) {
        (
            &self.memory_map,
            self.memory_map_key,
            std::mem::size_of::<MemoryDescriptor>(),
        )
    }

    /// Get memory map key
    pub fn memory_map_key(&self) -> u64 {
        self.memory_map_key
    }

    /// Allocate pool
    pub fn allocate_pool(&mut self, memory_type: MemoryType, size: u64) -> Result<u64, Status> {
        // Round up to page size and allocate pages
        let pages = (size + 4095) / 4096;
        self.allocate_pages(AllocateType::AllocateAnyPages, memory_type, pages, 0)
    }

    /// Free pool
    pub fn free_pool(&mut self, buffer: u64) -> Status {
        // Find allocation by address
        if let Some(pos) = self.allocations.iter().position(|a| a.address == buffer) {
            let pages = self.allocations[pos].pages;
            self.allocations.remove(pos);
            self.memory_map_key += 1;
            self.stats.frees.fetch_add(1, Ordering::Relaxed);
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }

    /// Create event
    pub fn create_event(&mut self, event_type: u32, notify_tpl: Tpl) -> Result<u64, Status> {
        if self.exited {
            return Err(Status::UNSUPPORTED);
        }

        let id = self.next_event;
        self.next_event += 1;

        let event = Event::new(id, event_type, notify_tpl);
        self.events.insert(id, event);
        self.stats.events_created.fetch_add(1, Ordering::Relaxed);

        Ok(id)
    }

    /// Set timer
    pub fn set_timer(&mut self, event_id: u64, delay_type: TimerDelay, trigger_time: u64) -> Status {
        if let Some(event) = self.events.get_mut(&event_id) {
            if !event.is_timer() {
                return Status::INVALID_PARAMETER;
            }

            match delay_type {
                TimerDelay::Cancel => {
                    event.timer_period = 0;
                    event.timer_trigger = 0;
                }
                TimerDelay::Periodic => {
                    event.timer_period = trigger_time;
                    event.timer_trigger = trigger_time;
                }
                TimerDelay::Relative => {
                    event.timer_period = 0;
                    event.timer_trigger = trigger_time;
                }
            }
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }

    /// Signal event
    pub fn signal_event(&mut self, event_id: u64) -> Status {
        if let Some(event) = self.events.get_mut(&event_id) {
            event.signaled = true;
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }

    /// Check event
    pub fn check_event(&self, event_id: u64) -> Status {
        if let Some(event) = self.events.get(&event_id) {
            if event.signaled {
                Status::SUCCESS
            } else {
                Status::NOT_READY
            }
        } else {
            Status::NOT_FOUND
        }
    }

    /// Close event
    pub fn close_event(&mut self, event_id: u64) -> Status {
        if self.events.remove(&event_id).is_some() {
            Status::SUCCESS
        } else {
            Status::NOT_FOUND
        }
    }

    /// Create handle
    pub fn create_handle(&mut self) -> Handle {
        let id = self.next_handle;
        self.next_handle += 1;
        self.handles.insert(id, HandleEntry::default());
        Handle::new(id)
    }

    /// Install protocol interface
    pub fn install_protocol_interface(
        &mut self,
        handle: &mut Handle,
        protocol: Guid,
        interface: u64,
    ) -> Status {
        if self.exited {
            return Status::UNSUPPORTED;
        }

        // Create handle if null
        if handle.is_null() {
            *handle = self.create_handle();
        }

        // Get or create handle entry
        let entry = self.handles.entry(handle.0).or_default();

        // Check if protocol already installed
        if entry.protocols.iter().any(|p| p.guid == protocol) {
            return Status::INVALID_PARAMETER;
        }

        entry.protocols.push(ProtocolInterface {
            guid: protocol,
            interface,
        });

        self.stats.protocol_installs.fetch_add(1, Ordering::Relaxed);
        Status::SUCCESS
    }

    /// Handle protocol
    pub fn handle_protocol(&self, handle: Handle, protocol: &Guid) -> Result<u64, Status> {
        if let Some(entry) = self.handles.get(&handle.0) {
            if let Some(proto) = entry.protocols.iter().find(|p| &p.guid == protocol) {
                Ok(proto.interface)
            } else {
                Err(Status::UNSUPPORTED)
            }
        } else {
            Err(Status::INVALID_PARAMETER)
        }
    }

    /// Locate handle
    pub fn locate_handle(
        &self,
        search_type: LocateSearchType,
        protocol: Option<&Guid>,
    ) -> Vec<Handle> {
        match search_type {
            LocateSearchType::AllHandles => {
                self.handles.keys().map(|&id| Handle::new(id)).collect()
            }
            LocateSearchType::ByProtocol => {
                if let Some(guid) = protocol {
                    self.handles
                        .iter()
                        .filter(|(_, entry)| entry.protocols.iter().any(|p| &p.guid == guid))
                        .map(|(&id, _)| Handle::new(id))
                        .collect()
                } else {
                    Vec::new()
                }
            }
            LocateSearchType::ByRegisterNotify => Vec::new(),
        }
    }

    /// Locate protocol
    pub fn locate_protocol(&self, protocol: &Guid) -> Result<u64, Status> {
        for entry in self.handles.values() {
            if let Some(proto) = entry.protocols.iter().find(|p| &p.guid == protocol) {
                return Ok(proto.interface);
            }
        }
        Err(Status::NOT_FOUND)
    }

    /// Get next monotonic count
    pub fn get_next_monotonic_count(&mut self) -> u64 {
        self.monotonic_count += 1;
        self.monotonic_count
    }

    /// Stall (microseconds)
    pub fn stall(&self, _microseconds: u64) -> Status {
        // In emulation, we just return success
        Status::SUCCESS
    }

    /// Exit boot services
    pub fn exit_boot_services(&mut self, map_key: u64) -> Status {
        if self.exited {
            return Status::UNSUPPORTED;
        }

        if map_key != self.memory_map_key {
            return Status::INVALID_PARAMETER;
        }

        self.exited = true;
        Status::SUCCESS
    }

    // Helper methods

    fn is_address_allocated(&self, address: u64, pages: u64) -> bool {
        let end = address + pages * 4096;
        self.allocations
            .iter()
            .any(|a| !(end <= a.address || address >= a.address + a.pages * 4096))
    }

    fn find_free_pages(&self, pages: u64, min_address: u64) -> Result<u64, Status> {
        // Find in conventional memory regions
        for desc in &self.memory_map {
            if desc.get_memory_type() != Some(MemoryType::ConventionalMemory) {
                continue;
            }

            let start = desc.physical_start.max(min_address);
            let end = desc.physical_end();

            if end - start >= pages * 4096 {
                // Check if not already allocated
                let aligned_start = (start + 4095) & !4095;
                if !self.is_address_allocated(aligned_start, pages) {
                    return Ok(aligned_start);
                }
            }
        }
        Err(Status::OUT_OF_RESOURCES)
    }

    fn find_free_pages_below(&self, pages: u64, max_address: u64) -> Result<u64, Status> {
        let mut best_address = None;

        for desc in &self.memory_map {
            if desc.get_memory_type() != Some(MemoryType::ConventionalMemory) {
                continue;
            }

            let start = desc.physical_start;
            let end = desc.physical_end().min(max_address);

            if end > start && end - start >= pages * 4096 {
                let candidate = end - pages * 4096;
                if !self.is_address_allocated(candidate, pages) {
                    best_address = Some(best_address.map_or(candidate, |b: u64| b.max(candidate)));
                }
            }
        }

        best_address.ok_or(Status::OUT_OF_RESOURCES)
    }
}

/// EFI System Table
pub struct SystemTable {
    /// Header
    header: TableHeader,
    /// Firmware vendor string address
    pub firmware_vendor: u64,
    /// Firmware revision
    pub firmware_revision: u32,
    /// Console in handle
    pub console_in_handle: Handle,
    /// Console in protocol
    pub con_in: u64,
    /// Console out handle
    pub console_out_handle: Handle,
    /// Console out protocol
    pub con_out: u64,
    /// Standard error handle
    pub standard_error_handle: Handle,
    /// Standard error protocol
    pub std_err: u64,
    /// Runtime services pointer
    pub runtime_services: u64,
    /// Boot services pointer
    pub boot_services: u64,
    /// Configuration tables
    configuration_tables: Vec<ConfigurationTable>,
}

impl Default for SystemTable {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemTable {
    /// Create new system table
    pub fn new() -> Self {
        Self {
            header: TableHeader::new(
                TableHeader::SYSTEM_TABLE_SIGNATURE,
                TableHeader::UEFI_2_10_REVISION,
                std::mem::size_of::<SystemTable>() as u32,
            ),
            firmware_vendor: 0,
            firmware_revision: 0x00010000, // 1.0
            console_in_handle: Handle::NULL,
            con_in: 0,
            console_out_handle: Handle::NULL,
            con_out: 0,
            standard_error_handle: Handle::NULL,
            std_err: 0,
            runtime_services: 0,
            boot_services: 0,
            configuration_tables: Vec::new(),
        }
    }

    /// Get header
    pub fn header(&self) -> &TableHeader {
        &self.header
    }

    /// Get UEFI revision
    pub fn revision(&self) -> u32 {
        self.header.revision
    }

    /// Set firmware vendor
    pub fn set_firmware_vendor(&mut self, vendor_address: u64) {
        self.firmware_vendor = vendor_address;
    }

    /// Set firmware revision
    pub fn set_firmware_revision(&mut self, revision: u32) {
        self.firmware_revision = revision;
    }

    /// Add configuration table
    pub fn add_configuration_table(&mut self, guid: Guid, table_address: u64) {
        // Replace if exists
        if let Some(entry) = self.configuration_tables.iter_mut().find(|t| t.vendor_guid == guid) {
            entry.vendor_table = table_address;
        } else {
            self.configuration_tables
                .push(ConfigurationTable::new(guid, table_address));
        }
    }

    /// Get configuration table
    pub fn get_configuration_table(&self, guid: &Guid) -> Option<u64> {
        self.configuration_tables
            .iter()
            .find(|t| &t.vendor_guid == guid)
            .map(|t| t.vendor_table)
    }

    /// Get configuration tables
    pub fn configuration_tables(&self) -> &[ConfigurationTable] {
        &self.configuration_tables
    }

    /// Get number of configuration tables
    pub fn number_of_table_entries(&self) -> usize {
        self.configuration_tables.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boot_services_creation() {
        let bs = BootServices::new();
        assert!(!bs.is_exited());
        assert_eq!(bs.current_tpl(), Tpl::Application);
    }

    #[test]
    fn test_boot_services_tpl() {
        let mut bs = BootServices::new();

        let old = bs.raise_tpl(Tpl::Notify);
        assert_eq!(old, Tpl::Application);
        assert_eq!(bs.current_tpl(), Tpl::Notify);

        bs.restore_tpl(Tpl::Application);
        assert_eq!(bs.current_tpl(), Tpl::Application);
    }

    #[test]
    fn test_boot_services_memory_map() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000); // 256MB

        let (map, key, _size) = bs.get_memory_map();
        assert!(!map.is_empty());
        assert!(key > 0);
    }

    #[test]
    fn test_boot_services_allocate_pages() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000);

        let result = bs.allocate_pages(
            AllocateType::AllocateAnyPages,
            MemoryType::LoaderData,
            10,
            0,
        );
        assert!(result.is_ok());

        let addr = result.unwrap();
        assert!(addr >= 0x100000); // Above 1MB
    }

    #[test]
    fn test_boot_services_free_pages() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000);

        let addr = bs
            .allocate_pages(AllocateType::AllocateAnyPages, MemoryType::LoaderData, 10, 0)
            .unwrap();

        let status = bs.free_pages(addr, 10);
        assert!(status.is_success());

        // Free again should fail
        let status = bs.free_pages(addr, 10);
        assert!(status.is_error());
    }

    #[test]
    fn test_boot_services_allocate_pool() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000);

        let result = bs.allocate_pool(MemoryType::LoaderData, 1024);
        assert!(result.is_ok());
    }

    #[test]
    fn test_boot_services_create_event() {
        let mut bs = BootServices::new();

        let result = bs.create_event(EventType::TIMER.0, Tpl::Callback);
        assert!(result.is_ok());

        let event_id = result.unwrap();
        assert!(event_id > 0);
    }

    #[test]
    fn test_boot_services_signal_event() {
        let mut bs = BootServices::new();
        let event_id = bs.create_event(0, Tpl::Callback).unwrap();

        // Initially not signaled
        assert_eq!(bs.check_event(event_id), Status::NOT_READY);

        // Signal
        bs.signal_event(event_id);
        assert_eq!(bs.check_event(event_id), Status::SUCCESS);
    }

    #[test]
    fn test_boot_services_close_event() {
        let mut bs = BootServices::new();
        let event_id = bs.create_event(0, Tpl::Callback).unwrap();

        let status = bs.close_event(event_id);
        assert!(status.is_success());

        // Close again should fail
        let status = bs.close_event(event_id);
        assert!(status.is_error());
    }

    #[test]
    fn test_boot_services_create_handle() {
        let mut bs = BootServices::new();

        let h1 = bs.create_handle();
        let h2 = bs.create_handle();

        assert!(!h1.is_null());
        assert!(!h2.is_null());
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_boot_services_install_protocol() {
        let mut bs = BootServices::new();
        let mut handle = Handle::NULL;

        let status = bs.install_protocol_interface(
            &mut handle,
            guids::EFI_LOADED_IMAGE_PROTOCOL,
            0x1000,
        );
        assert!(status.is_success());
        assert!(!handle.is_null());

        // Installing same protocol again should fail
        let status = bs.install_protocol_interface(
            &mut handle,
            guids::EFI_LOADED_IMAGE_PROTOCOL,
            0x2000,
        );
        assert!(status.is_error());
    }

    #[test]
    fn test_boot_services_handle_protocol() {
        let mut bs = BootServices::new();
        let mut handle = Handle::NULL;

        bs.install_protocol_interface(&mut handle, guids::EFI_BLOCK_IO_PROTOCOL, 0x3000);

        let result = bs.handle_protocol(handle, &guids::EFI_BLOCK_IO_PROTOCOL);
        assert_eq!(result, Ok(0x3000));

        let result = bs.handle_protocol(handle, &guids::EFI_DISK_IO_PROTOCOL);
        assert!(result.is_err());
    }

    #[test]
    fn test_boot_services_locate_handle() {
        let mut bs = BootServices::new();
        let mut h1 = Handle::NULL;
        let mut h2 = Handle::NULL;

        bs.install_protocol_interface(&mut h1, guids::EFI_BLOCK_IO_PROTOCOL, 0x1000);
        bs.install_protocol_interface(&mut h2, guids::EFI_BLOCK_IO_PROTOCOL, 0x2000);

        let handles = bs.locate_handle(LocateSearchType::ByProtocol, Some(&guids::EFI_BLOCK_IO_PROTOCOL));
        assert_eq!(handles.len(), 2);

        let handles = bs.locate_handle(LocateSearchType::AllHandles, None);
        assert_eq!(handles.len(), 2);
    }

    #[test]
    fn test_boot_services_locate_protocol() {
        let mut bs = BootServices::new();
        let mut handle = Handle::NULL;

        bs.install_protocol_interface(&mut handle, guids::EFI_GRAPHICS_OUTPUT_PROTOCOL, 0x5000);

        let result = bs.locate_protocol(&guids::EFI_GRAPHICS_OUTPUT_PROTOCOL);
        assert_eq!(result, Ok(0x5000));

        let result = bs.locate_protocol(&guids::EFI_SIMPLE_FILE_SYSTEM_PROTOCOL);
        assert!(result.is_err());
    }

    #[test]
    fn test_boot_services_monotonic_count() {
        let mut bs = BootServices::new();

        let c1 = bs.get_next_monotonic_count();
        let c2 = bs.get_next_monotonic_count();

        assert_eq!(c2, c1 + 1);
    }

    #[test]
    fn test_boot_services_exit() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000);

        let key = bs.memory_map_key();
        let status = bs.exit_boot_services(key);
        assert!(status.is_success());
        assert!(bs.is_exited());

        // Operations after exit should fail
        let result = bs.allocate_pages(AllocateType::AllocateAnyPages, MemoryType::LoaderData, 1, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_boot_services_stats() {
        let mut bs = BootServices::new();
        bs.init_default_memory_map(0x10000000);

        bs.allocate_pages(AllocateType::AllocateAnyPages, MemoryType::LoaderData, 1, 0).unwrap();
        bs.create_event(0, Tpl::Callback).unwrap();

        let stats = bs.stats().snapshot();
        assert!(stats.allocations > 0);
        assert!(stats.events_created > 0);
    }

    #[test]
    fn test_system_table_creation() {
        let st = SystemTable::new();
        assert_eq!(st.header().signature, TableHeader::SYSTEM_TABLE_SIGNATURE);
    }

    #[test]
    fn test_system_table_config_tables() {
        let mut st = SystemTable::new();

        st.add_configuration_table(guids::EFI_ACPI_TABLE, 0xE0000);
        st.add_configuration_table(guids::EFI_SMBIOS_TABLE, 0xF0000);

        assert_eq!(st.number_of_table_entries(), 2);
        assert_eq!(st.get_configuration_table(&guids::EFI_ACPI_TABLE), Some(0xE0000));
        assert_eq!(st.get_configuration_table(&guids::EFI_SMBIOS_TABLE), Some(0xF0000));
    }

    #[test]
    fn test_system_table_replace_config_table() {
        let mut st = SystemTable::new();

        st.add_configuration_table(guids::EFI_ACPI_TABLE, 0xE0000);
        st.add_configuration_table(guids::EFI_ACPI_TABLE, 0xF0000);

        assert_eq!(st.number_of_table_entries(), 1);
        assert_eq!(st.get_configuration_table(&guids::EFI_ACPI_TABLE), Some(0xF0000));
    }

    #[test]
    fn test_system_table_firmware() {
        let mut st = SystemTable::new();

        st.set_firmware_vendor(0x1000);
        st.set_firmware_revision(0x00020000);

        assert_eq!(st.firmware_vendor, 0x1000);
        assert_eq!(st.firmware_revision, 0x00020000);
    }

    #[test]
    fn test_event_timer() {
        let event = Event::new(1, EventType::TIMER.0, Tpl::Callback);
        assert!(event.is_timer());

        let event = Event::new(2, 0, Tpl::Callback);
        assert!(!event.is_timer());
    }

    #[test]
    fn test_set_timer() {
        let mut bs = BootServices::new();
        let event_id = bs.create_event(EventType::TIMER.0, Tpl::Callback).unwrap();

        let status = bs.set_timer(event_id, TimerDelay::Periodic, 10000000);
        assert!(status.is_success());

        // Non-timer event
        let event_id2 = bs.create_event(0, Tpl::Callback).unwrap();
        let status = bs.set_timer(event_id2, TimerDelay::Periodic, 10000000);
        assert!(status.is_error());
    }
}
