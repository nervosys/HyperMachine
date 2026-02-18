//! UEFI Runtime Services
//!
//! This module provides the UEFI Runtime Services implementation
//! for time, variable, and system control operations.

use super::types::{guids, Guid, Status, TableHeader, Time, TimeCapabilities};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// Virtual address map status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtualAddressMapState {
    /// Not yet converted to virtual addresses
    #[default]
    Physical,
    /// Converted to virtual addresses
    Virtual,
}

/// Reset type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u32)]
pub enum ResetType {
    /// Cold reset
    #[default]
    Cold = 0,
    /// Warm reset
    Warm = 1,
    /// Shutdown
    Shutdown = 2,
    /// Platform specific
    PlatformSpecific = 3,
}

/// Variable attributes
#[derive(Debug, Clone, Copy, Default)]
pub struct VariableAttributes(u32);

impl VariableAttributes {
    /// Non-volatile
    pub const NON_VOLATILE: u32 = 0x00000001;
    /// Boot services access
    pub const BOOTSERVICE_ACCESS: u32 = 0x00000002;
    /// Runtime access
    pub const RUNTIME_ACCESS: u32 = 0x00000004;
    /// Hardware error record
    pub const HARDWARE_ERROR_RECORD: u32 = 0x00000008;
    /// Authenticated write access
    pub const AUTHENTICATED_WRITE_ACCESS: u32 = 0x00000010;
    /// Time-based authenticated write access
    pub const TIME_BASED_AUTHENTICATED_WRITE_ACCESS: u32 = 0x00000020;
    /// Append write
    pub const APPEND_WRITE: u32 = 0x00000040;
    /// Enhanced authenticated access
    pub const ENHANCED_AUTHENTICATED_ACCESS: u32 = 0x00000080;

    /// Create new attributes
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    /// Get raw value
    pub fn bits(&self) -> u32 {
        self.0
    }

    /// Check if non-volatile
    pub fn is_non_volatile(&self) -> bool {
        self.0 & Self::NON_VOLATILE != 0
    }

    /// Check if boot services access
    pub fn is_bootservice_access(&self) -> bool {
        self.0 & Self::BOOTSERVICE_ACCESS != 0
    }

    /// Check if runtime access
    pub fn is_runtime_access(&self) -> bool {
        self.0 & Self::RUNTIME_ACCESS != 0
    }

    /// Default NV+BS+RT attributes
    pub fn nvbsrt() -> Self {
        Self(Self::NON_VOLATILE | Self::BOOTSERVICE_ACCESS | Self::RUNTIME_ACCESS)
    }
}

/// Capsule header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct CapsuleHeader {
    /// Capsule GUID
    pub capsule_guid: Guid,
    /// Header size
    pub header_size: u32,
    /// Flags
    pub flags: u32,
    /// Capsule image size
    pub capsule_image_size: u32,
}

/// Capsule flags
pub mod capsule_flags {
    /// Persist across reset
    pub const PERSIST_ACROSS_RESET: u32 = 0x00010000;
    /// Populate system table
    pub const POPULATE_SYSTEM_TABLE: u32 = 0x00020000;
    /// Initiate reset
    pub const INITIATE_RESET: u32 = 0x00040000;
}

/// Variable entry
#[derive(Debug, Clone)]
pub struct Variable {
    /// GUID
    pub guid: Guid,
    /// Name
    pub name: String,
    /// Attributes
    pub attributes: VariableAttributes,
    /// Data
    pub data: Vec<u8>,
}

impl Variable {
    /// Create new variable
    pub fn new(guid: Guid, name: &str, attributes: VariableAttributes, data: Vec<u8>) -> Self {
        Self {
            guid,
            name: name.to_string(),
            attributes,
            data,
        }
    }
}

/// Runtime services function indices
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RuntimeServiceFunction {
    /// Get time
    GetTime = 0,
    /// Set time
    SetTime = 1,
    /// Get wakeup time
    GetWakeupTime = 2,
    /// Set wakeup time
    SetWakeupTime = 3,
    /// Set virtual address map
    SetVirtualAddressMap = 4,
    /// Convert pointer
    ConvertPointer = 5,
    /// Get variable
    GetVariable = 6,
    /// Get next variable name
    GetNextVariableName = 7,
    /// Set variable
    SetVariable = 8,
    /// Get next high monotonic count
    GetNextHighMonotonicCount = 9,
    /// Reset system
    ResetSystem = 10,
    /// Update capsule
    UpdateCapsule = 11,
    /// Query capsule capabilities
    QueryCapsuleCapabilities = 12,
    /// Query variable info
    QueryVariableInfo = 13,
}

/// Runtime services statistics
#[derive(Debug, Default)]
pub struct RuntimeServicesStats {
    /// Get time calls
    get_time_calls: AtomicU64,
    /// Set time calls
    set_time_calls: AtomicU64,
    /// Get variable calls
    get_variable_calls: AtomicU64,
    /// Set variable calls
    set_variable_calls: AtomicU64,
    /// Reset calls
    reset_calls: AtomicU64,
}

impl RuntimeServicesStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Get snapshot
    pub fn snapshot(&self) -> RuntimeServicesStatsSnapshot {
        RuntimeServicesStatsSnapshot {
            get_time_calls: self.get_time_calls.load(Ordering::Relaxed),
            set_time_calls: self.set_time_calls.load(Ordering::Relaxed),
            get_variable_calls: self.get_variable_calls.load(Ordering::Relaxed),
            set_variable_calls: self.set_variable_calls.load(Ordering::Relaxed),
            reset_calls: self.reset_calls.load(Ordering::Relaxed),
        }
    }
}

/// Stats snapshot
#[derive(Debug, Clone, Default)]
pub struct RuntimeServicesStatsSnapshot {
    /// Get time calls
    pub get_time_calls: u64,
    /// Set time calls
    pub set_time_calls: u64,
    /// Get variable calls
    pub get_variable_calls: u64,
    /// Set variable calls
    pub set_variable_calls: u64,
    /// Reset calls
    pub reset_calls: u64,
}

/// Runtime Services
pub struct RuntimeServices {
    /// Header
    pub header: TableHeader,
    /// Current time
    time: Time,
    /// Time capabilities
    time_capabilities: TimeCapabilities,
    /// Wakeup time
    wakeup_time: Option<Time>,
    /// Wakeup enabled
    wakeup_enabled: bool,
    /// Variables storage
    variables: HashMap<(Guid, String), Variable>,
    /// High monotonic count
    high_monotonic_count: AtomicU32,
    /// Virtual address map state
    virtual_address_state: VirtualAddressMapState,
    /// Reset request (for testing)
    reset_requested: Option<ResetType>,
    /// Statistics
    stats: RuntimeServicesStats,
}

impl Default for RuntimeServices {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeServices {
    /// Create new runtime services
    pub fn new() -> Self {
        Self {
            header: TableHeader {
                signature: TableHeader::RUNTIME_SERVICES_SIGNATURE,
                revision: TableHeader::UEFI_2_10_REVISION,
                header_size: std::mem::size_of::<RuntimeServices>() as u32,
                crc32: 0,
                reserved: 0,
            },
            time: Time::default(),
            time_capabilities: TimeCapabilities {
                resolution: 1,
                accuracy: 50_000_000, // 50ms
                sets_to_zero: false,
            },
            wakeup_time: None,
            wakeup_enabled: false,
            variables: HashMap::new(),
            high_monotonic_count: AtomicU32::new(0),
            virtual_address_state: VirtualAddressMapState::Physical,
            reset_requested: None,
            stats: RuntimeServicesStats::new(),
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &RuntimeServicesStats {
        &self.stats
    }

    // ========== Time Services ==========

    /// Get time
    pub fn get_time(&self) -> (Time, TimeCapabilities) {
        self.stats.get_time_calls.fetch_add(1, Ordering::Relaxed);
        (self.time, self.time_capabilities)
    }

    /// Set time
    pub fn set_time(&mut self, time: Time) -> Status {
        self.stats.set_time_calls.fetch_add(1, Ordering::Relaxed);

        // Validate time
        if time.month < 1 || time.month > 12 {
            return Status::INVALID_PARAMETER;
        }
        if time.day < 1 || time.day > 31 {
            return Status::INVALID_PARAMETER;
        }
        if time.hour > 23 {
            return Status::INVALID_PARAMETER;
        }
        if time.minute > 59 {
            return Status::INVALID_PARAMETER;
        }
        if time.second > 59 {
            return Status::INVALID_PARAMETER;
        }

        self.time = time;
        Status::SUCCESS
    }

    /// Get wakeup time
    pub fn get_wakeup_time(&self) -> (bool, bool, Option<Time>) {
        (
            self.wakeup_enabled,
            self.wakeup_time.is_some(),
            self.wakeup_time,
        )
    }

    /// Set wakeup time
    pub fn set_wakeup_time(&mut self, enable: bool, time: Option<Time>) -> Status {
        self.wakeup_enabled = enable;
        self.wakeup_time = if enable { time } else { None };
        Status::SUCCESS
    }

    // ========== Variable Services ==========

    /// Get variable
    pub fn get_variable(&self, name: &str, guid: &Guid) -> Result<&Variable, Status> {
        self.stats
            .get_variable_calls
            .fetch_add(1, Ordering::Relaxed);

        self.variables
            .get(&(*guid, name.to_string()))
            .ok_or(Status::NOT_FOUND)
    }

    /// Set variable
    pub fn set_variable(
        &mut self,
        name: &str,
        guid: &Guid,
        attributes: VariableAttributes,
        data: Vec<u8>,
    ) -> Status {
        self.stats
            .set_variable_calls
            .fetch_add(1, Ordering::Relaxed);

        if name.is_empty() {
            return Status::INVALID_PARAMETER;
        }

        let key = (*guid, name.to_string());

        if data.is_empty() {
            // Delete variable
            self.variables.remove(&key);
        } else {
            // Set variable
            self.variables
                .insert(key, Variable::new(*guid, name, attributes, data));
        }

        Status::SUCCESS
    }

    /// Get next variable name
    pub fn get_next_variable_name(
        &self,
        current_name: Option<&str>,
        current_guid: Option<&Guid>,
    ) -> Result<(&str, &Guid), Status> {
        let keys: Vec<_> = self.variables.keys().collect();

        if keys.is_empty() {
            return Err(Status::NOT_FOUND);
        }

        match (current_name, current_guid) {
            (None, None) | (Some(""), _) => {
                // Return first variable
                let (guid, name) = keys
                    .first()
                    .expect("keys guaranteed non-empty after is_empty check");
                let var = self
                    .variables
                    .get(&(*guid, name.clone()))
                    .ok_or(Status::NOT_FOUND)?;
                Ok((&var.name, &var.guid))
            }
            (Some(name), Some(guid)) => {
                // Find next variable after current
                let current_key = (*guid, name.to_string());
                let mut found = false;

                for key in &keys {
                    if found {
                        let var = self.variables.get(*key).ok_or(Status::NOT_FOUND)?;
                        return Ok((&var.name, &var.guid));
                    }
                    if **key == current_key {
                        found = true;
                    }
                }

                Err(Status::NOT_FOUND)
            }
            _ => Err(Status::INVALID_PARAMETER),
        }
    }

    /// Query variable info
    pub fn query_variable_info(
        &self,
        attributes: VariableAttributes,
    ) -> Result<VariableInfo, Status> {
        // Simulated variable storage info
        let max_storage: usize = 64 * 1024; // 64KB
        let remaining = max_storage
            .saturating_sub(self.variables.values().map(|v| v.data.len()).sum::<usize>());
        let max_variable_size: u64 = 32 * 1024; // 32KB

        let _ = attributes; // Attributes might affect limits in real implementation

        Ok(VariableInfo {
            maximum_variable_storage_size: max_storage as u64,
            remaining_variable_storage_size: remaining as u64,
            maximum_variable_size: max_variable_size,
        })
    }

    // ========== Virtual Memory Services ==========

    /// Set virtual address map
    pub fn set_virtual_address_map(
        &mut self,
        _memory_map_size: u64,
        _descriptor_size: u64,
        _descriptor_version: u32,
    ) -> Status {
        if self.virtual_address_state == VirtualAddressMapState::Virtual {
            return Status::UNSUPPORTED;
        }

        self.virtual_address_state = VirtualAddressMapState::Virtual;
        Status::SUCCESS
    }

    /// Convert pointer
    pub fn convert_pointer(&self, _debug_disposition: u32, _address: &mut u64) -> Status {
        if self.virtual_address_state != VirtualAddressMapState::Virtual {
            return Status::NOT_FOUND;
        }
        // In a real implementation, would convert physical to virtual address
        Status::SUCCESS
    }

    /// Get virtual address state
    pub fn virtual_address_state(&self) -> VirtualAddressMapState {
        self.virtual_address_state
    }

    // ========== Miscellaneous Services ==========

    /// Get next high monotonic count
    pub fn get_next_high_monotonic_count(&self) -> u32 {
        self.high_monotonic_count.fetch_add(1, Ordering::Relaxed)
    }

    /// Reset system
    pub fn reset_system(
        &mut self,
        reset_type: ResetType,
        _status: Status,
        _data_size: u64,
        _reset_data: Option<&[u8]>,
    ) -> ! {
        self.stats.reset_calls.fetch_add(1, Ordering::Relaxed);
        self.reset_requested = Some(reset_type);

        // In a real implementation, this would trigger a VM shutdown.
        // Using abort() instead of panic!() to avoid stack unwinding
        // in a context where the VM should simply halt.
        eprintln!("System reset requested: {:?}", reset_type);
        std::process::abort();
    }

    /// Check if reset was requested (for testing)
    pub fn reset_requested(&self) -> Option<ResetType> {
        self.reset_requested
    }

    // ========== Capsule Services ==========

    /// Update capsule
    pub fn update_capsule(
        &self,
        _capsule_header_array: &[CapsuleHeader],
        _capsule_count: usize,
    ) -> Status {
        // Capsule updates typically require platform-specific handling
        Status::UNSUPPORTED
    }

    /// Query capsule capabilities
    pub fn query_capsule_capabilities(
        &self,
        _capsule_header_array: &[CapsuleHeader],
        _capsule_count: usize,
    ) -> Result<CapsuleCapabilities, Status> {
        Ok(CapsuleCapabilities {
            maximum_capsule_size: 16 * 1024 * 1024, // 16MB
            reset_type: ResetType::Cold,
        })
    }

    // ========== Standard Variables ==========

    /// Initialize standard UEFI variables
    pub fn init_standard_variables(&mut self) {
        // SecureBoot variable
        self.set_variable(
            "SecureBoot",
            &guids::EFI_GLOBAL_VARIABLE,
            VariableAttributes::new(
                VariableAttributes::BOOTSERVICE_ACCESS | VariableAttributes::RUNTIME_ACCESS,
            ),
            vec![0], // Disabled
        );

        // SetupMode variable
        self.set_variable(
            "SetupMode",
            &guids::EFI_GLOBAL_VARIABLE,
            VariableAttributes::new(
                VariableAttributes::BOOTSERVICE_ACCESS | VariableAttributes::RUNTIME_ACCESS,
            ),
            vec![1], // Setup mode enabled
        );

        // BootOrder variable (empty)
        self.set_variable(
            "BootOrder",
            &guids::EFI_GLOBAL_VARIABLE,
            VariableAttributes::nvbsrt(),
            vec![],
        );
    }

    /// Get variable count
    pub fn variable_count(&self) -> usize {
        self.variables.len()
    }
}

/// Variable info
#[derive(Debug, Clone, Copy)]
pub struct VariableInfo {
    /// Maximum variable storage size
    pub maximum_variable_storage_size: u64,
    /// Remaining variable storage size
    pub remaining_variable_storage_size: u64,
    /// Maximum size of individual variable
    pub maximum_variable_size: u64,
}

/// Capsule capabilities
#[derive(Debug, Clone, Copy)]
pub struct CapsuleCapabilities {
    /// Maximum capsule size
    pub maximum_capsule_size: u64,
    /// Reset type required
    pub reset_type: ResetType,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_attributes() {
        let attrs = VariableAttributes::nvbsrt();
        assert!(attrs.is_non_volatile());
        assert!(attrs.is_bootservice_access());
        assert!(attrs.is_runtime_access());
    }

    #[test]
    fn test_runtime_services_creation() {
        let rs = RuntimeServices::new();
        assert_eq!(rs.header.signature, TableHeader::RUNTIME_SERVICES_SIGNATURE);
        assert_eq!(rs.variable_count(), 0);
    }

    #[test]
    fn test_get_set_time() {
        let mut rs = RuntimeServices::new();

        let (time, caps) = rs.get_time();
        assert!(caps.resolution > 0);

        let new_time = Time {
            year: 2024,
            month: 6,
            day: 15,
            hour: 12,
            minute: 30,
            second: 45,
            nanosecond: 0,
            timezone: 0,
            daylight: 0,
            pad1: 0,
            pad2: 0,
        };

        let status = rs.set_time(new_time);
        assert!(status.is_success());

        let (read_time, _) = rs.get_time();
        assert_eq!(read_time.year, 2024);
        assert_eq!(read_time.month, 6);
    }

    #[test]
    fn test_set_time_invalid() {
        let mut rs = RuntimeServices::new();

        let invalid_time = Time {
            month: 13, // Invalid
            ..Default::default()
        };
        let status = rs.set_time(invalid_time);
        assert!(status.is_error());

        let invalid_time = Time {
            month: 1,
            day: 32, // Invalid
            ..Default::default()
        };
        let status = rs.set_time(invalid_time);
        assert!(status.is_error());
    }

    #[test]
    fn test_wakeup_time() {
        let mut rs = RuntimeServices::new();

        let (enabled, pending, time) = rs.get_wakeup_time();
        assert!(!enabled);
        assert!(!pending);
        assert!(time.is_none());

        let wakeup = Time {
            year: 2024,
            month: 6,
            day: 16,
            hour: 8,
            minute: 0,
            second: 0,
            ..Default::default()
        };

        rs.set_wakeup_time(true, Some(wakeup));
        let (enabled, pending, time) = rs.get_wakeup_time();
        assert!(enabled);
        assert!(pending);
        assert!(time.is_some());
    }

    #[test]
    fn test_variable_operations() {
        let mut rs = RuntimeServices::new();
        let guid = guids::EFI_GLOBAL_VARIABLE;

        // Set variable
        let status = rs.set_variable(
            "TestVar",
            &guid,
            VariableAttributes::nvbsrt(),
            vec![1, 2, 3, 4],
        );
        assert!(status.is_success());

        // Get variable
        let var = rs.get_variable("TestVar", &guid).unwrap();
        assert_eq!(var.data, vec![1, 2, 3, 4]);

        // Delete variable
        rs.set_variable("TestVar", &guid, VariableAttributes::default(), vec![]);
        assert!(rs.get_variable("TestVar", &guid).is_err());
    }

    #[test]
    fn test_variable_not_found() {
        let rs = RuntimeServices::new();
        let result = rs.get_variable("NonExistent", &guids::EFI_GLOBAL_VARIABLE);
        assert!(result.is_err());
    }

    #[test]
    fn test_variable_invalid_name() {
        let mut rs = RuntimeServices::new();
        let status = rs.set_variable(
            "",
            &guids::EFI_GLOBAL_VARIABLE,
            VariableAttributes::default(),
            vec![1],
        );
        assert!(status.is_error());
    }

    #[test]
    fn test_query_variable_info() {
        let rs = RuntimeServices::new();
        let info = rs
            .query_variable_info(VariableAttributes::nvbsrt())
            .unwrap();

        assert!(info.maximum_variable_storage_size > 0);
        assert!(info.remaining_variable_storage_size > 0);
        assert!(info.maximum_variable_size > 0);
    }

    #[test]
    fn test_high_monotonic_count() {
        let rs = RuntimeServices::new();

        let count1 = rs.get_next_high_monotonic_count();
        let count2 = rs.get_next_high_monotonic_count();
        let count3 = rs.get_next_high_monotonic_count();

        assert_eq!(count2, count1 + 1);
        assert_eq!(count3, count2 + 1);
    }

    #[test]
    fn test_virtual_address_map() {
        let mut rs = RuntimeServices::new();

        assert_eq!(rs.virtual_address_state(), VirtualAddressMapState::Physical);

        let status = rs.set_virtual_address_map(0, 0, 0);
        assert!(status.is_success());
        assert_eq!(rs.virtual_address_state(), VirtualAddressMapState::Virtual);

        // Cannot set again
        let status = rs.set_virtual_address_map(0, 0, 0);
        assert!(status.is_error());
    }

    #[test]
    fn test_convert_pointer_before_va_map() {
        let rs = RuntimeServices::new();
        let mut addr = 0x1000u64;
        let status = rs.convert_pointer(0, &mut addr);
        assert!(status.is_error());
    }

    #[test]
    fn test_convert_pointer_after_va_map() {
        let mut rs = RuntimeServices::new();
        rs.set_virtual_address_map(0, 0, 0);

        let mut addr = 0x1000u64;
        let status = rs.convert_pointer(0, &mut addr);
        assert!(status.is_success());
    }

    #[test]
    fn test_capsule_capabilities() {
        let rs = RuntimeServices::new();
        let caps = rs.query_capsule_capabilities(&[], 0).unwrap();

        assert!(caps.maximum_capsule_size > 0);
    }

    #[test]
    fn test_update_capsule() {
        let rs = RuntimeServices::new();
        let status = rs.update_capsule(&[], 0);
        // Expected to be unsupported in this implementation
        assert_eq!(status, Status::UNSUPPORTED);
    }

    #[test]
    fn test_init_standard_variables() {
        let mut rs = RuntimeServices::new();
        rs.init_standard_variables();

        let secure_boot = rs.get_variable("SecureBoot", &guids::EFI_GLOBAL_VARIABLE);
        assert!(secure_boot.is_ok());
        assert_eq!(secure_boot.unwrap().data, vec![0]);

        let setup_mode = rs.get_variable("SetupMode", &guids::EFI_GLOBAL_VARIABLE);
        assert!(setup_mode.is_ok());
        assert_eq!(setup_mode.unwrap().data, vec![1]);
    }

    #[test]
    fn test_runtime_services_stats() {
        let mut rs = RuntimeServices::new();

        rs.get_time();
        rs.get_time();
        rs.set_time(Time::default());
        rs.set_variable(
            "Test",
            &guids::EFI_GLOBAL_VARIABLE,
            VariableAttributes::default(),
            vec![1],
        );
        rs.get_variable("Test", &guids::EFI_GLOBAL_VARIABLE).ok();

        let stats = rs.stats().snapshot();
        assert_eq!(stats.get_time_calls, 2);
        assert_eq!(stats.set_time_calls, 1);
        assert_eq!(stats.set_variable_calls, 1);
        assert_eq!(stats.get_variable_calls, 1);
    }

    #[test]
    fn test_reset_type() {
        assert_eq!(ResetType::Cold as u32, 0);
        assert_eq!(ResetType::Warm as u32, 1);
        assert_eq!(ResetType::Shutdown as u32, 2);
    }

    #[test]
    fn test_variable_struct() {
        let var = Variable::new(
            guids::EFI_GLOBAL_VARIABLE,
            "TestVar",
            VariableAttributes::nvbsrt(),
            vec![1, 2, 3],
        );

        assert_eq!(var.name, "TestVar");
        assert_eq!(var.data, vec![1, 2, 3]);
        assert!(var.attributes.is_non_volatile());
    }
}
