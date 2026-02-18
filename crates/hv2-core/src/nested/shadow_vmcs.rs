//! Shadow VMCS management for nested virtualization
//!
//! This module provides shadow VMCS functionality for tracking and caching
//! the L1 guest's VMCS state (VMCS12) during nested virtualization.

use std::collections::HashMap;

use super::types::{VmcsField, VmxInstructionError, VmxResult};

/// Shadow VMCS state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowVmcsState {
    /// VMCS is clear (not loaded)
    #[default]
    Clear,
    /// VMCS is loaded but not launched
    Loaded,
    /// VMCS has been launched
    Launched,
}

/// Shadow VMCS for tracking L1's VMCS12
#[derive(Debug, Clone)]
pub struct ShadowVmcs {
    /// Physical address of the VMCS12 in guest memory
    guest_vmcs_addr: u64,
    /// Cached VMCS fields
    fields: HashMap<u32, u64>,
    /// VMCS state
    state: ShadowVmcsState,
    /// VMCS revision identifier
    revision_id: u32,
    /// Shadow VMCS indicator
    is_shadow: bool,
    /// Dirty fields that need writeback
    dirty_fields: Vec<u32>,
    /// Launch state for VMLAUNCH/VMRESUME
    launch_state: bool,
}

impl ShadowVmcs {
    /// Create a new shadow VMCS
    pub fn new(guest_vmcs_addr: u64) -> Self {
        Self {
            guest_vmcs_addr,
            fields: HashMap::new(),
            state: ShadowVmcsState::Clear,
            revision_id: 1,
            is_shadow: false,
            dirty_fields: Vec::new(),
            launch_state: false,
        }
    }

    /// Create a shadow VMCS copy
    pub fn new_shadow(guest_vmcs_addr: u64, revision_id: u32) -> Self {
        Self {
            guest_vmcs_addr,
            fields: HashMap::new(),
            state: ShadowVmcsState::Clear,
            revision_id,
            is_shadow: true,
            dirty_fields: Vec::new(),
            launch_state: false,
        }
    }

    /// Get the guest VMCS address
    pub fn guest_addr(&self) -> u64 {
        self.guest_vmcs_addr
    }

    /// Get the VMCS state
    pub fn state(&self) -> ShadowVmcsState {
        self.state
    }

    /// Check if VMCS is launched
    pub fn is_launched(&self) -> bool {
        self.launch_state
    }

    /// Check if this is a shadow VMCS
    pub fn is_shadow(&self) -> bool {
        self.is_shadow
    }

    /// Get revision ID
    pub fn revision_id(&self) -> u32 {
        self.revision_id
    }

    /// Load the VMCS (VMPTRLD)
    pub fn load(&mut self) -> VmxResult<()> {
        self.state = ShadowVmcsState::Loaded;
        Ok(())
    }

    /// Clear the VMCS (VMCLEAR)
    pub fn clear(&mut self) -> VmxResult<()> {
        self.state = ShadowVmcsState::Clear;
        self.launch_state = false;
        self.dirty_fields.clear();
        Ok(())
    }

    /// Launch the VMCS (VMLAUNCH)
    pub fn launch(&mut self) -> VmxResult<()> {
        if self.launch_state {
            return Err(VmxInstructionError::VmresumeNonlaunchedVmcs);
        }
        self.state = ShadowVmcsState::Launched;
        self.launch_state = true;
        Ok(())
    }

    /// Resume the VMCS (VMRESUME)
    pub fn resume(&mut self) -> VmxResult<()> {
        if !self.launch_state {
            return Err(VmxInstructionError::VmlaunchNonclearVmcs);
        }
        Ok(())
    }

    /// Read a VMCS field (VMREAD)
    pub fn read(&self, field: VmcsField) -> VmxResult<u64> {
        self.fields
            .get(&field.0)
            .copied()
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)
    }

    /// Write a VMCS field (VMWRITE)
    pub fn write(&mut self, field: VmcsField, value: u64) -> VmxResult<()> {
        if field.is_read_only() {
            return Err(VmxInstructionError::VmwriteReadonlyField);
        }

        // Mask value based on field width
        let masked_value = match field.width() {
            16 => value & 0xFFFF,
            32 => value & 0xFFFF_FFFF,
            64 => value,
            _ => value,
        };

        self.fields.insert(field.0, masked_value);
        self.dirty_fields.push(field.0);
        Ok(())
    }

    /// Read a field with default value if not set
    pub fn read_or_default(&self, field: VmcsField, default: u64) -> u64 {
        self.fields.get(&field.0).copied().unwrap_or(default)
    }

    /// Check if a field is set
    pub fn has_field(&self, field: VmcsField) -> bool {
        self.fields.contains_key(&field.0)
    }

    /// Get all dirty fields
    pub fn dirty_fields(&self) -> &[u32] {
        &self.dirty_fields
    }

    /// Clear dirty fields after writeback
    pub fn clear_dirty(&mut self) {
        self.dirty_fields.clear();
    }

    /// Initialize with default VMCS field values
    pub fn initialize_defaults(&mut self) {
        // 16-bit guest state
        self.fields.insert(VmcsField::GUEST_CS_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_SS_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_DS_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_ES_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_FS_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_GS_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_TR_SELECTOR.0, 0);
        self.fields.insert(VmcsField::GUEST_LDTR_SELECTOR.0, 0);

        // 32-bit guest state
        self.fields.insert(VmcsField::GUEST_CS_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_SS_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_DS_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_ES_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_FS_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_GS_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_GDTR_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_IDTR_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_TR_LIMIT.0, 0xFFFF);
        self.fields.insert(VmcsField::GUEST_LDTR_LIMIT.0, 0xFFFF);

        // 32-bit control fields
        self.fields
            .insert(VmcsField::PIN_BASED_VM_EXEC_CONTROL.0, 0);
        self.fields
            .insert(VmcsField::CPU_BASED_VM_EXEC_CONTROL.0, 0);
        self.fields
            .insert(VmcsField::SECONDARY_VM_EXEC_CONTROL.0, 0);
        self.fields.insert(VmcsField::VM_EXIT_CONTROLS.0, 0);
        self.fields.insert(VmcsField::VM_ENTRY_CONTROLS.0, 0);
        self.fields.insert(VmcsField::EXCEPTION_BITMAP.0, 0);

        // Natural-width guest state
        self.fields.insert(VmcsField::GUEST_CR0.0, 0x6000_0010);
        self.fields.insert(VmcsField::GUEST_CR3.0, 0);
        self.fields.insert(VmcsField::GUEST_CR4.0, 0);
        self.fields.insert(VmcsField::GUEST_DR7.0, 0x400);
        self.fields.insert(VmcsField::GUEST_RSP.0, 0);
        self.fields.insert(VmcsField::GUEST_RIP.0, 0);
        self.fields.insert(VmcsField::GUEST_RFLAGS.0, 0x2);

        // 64-bit fields
        self.fields
            .insert(VmcsField::VMCS_LINK_POINTER.0, 0xFFFF_FFFF_FFFF_FFFF);
        self.fields.insert(VmcsField::TSC_OFFSET.0, 0);
        self.fields.insert(VmcsField::GUEST_IA32_EFER.0, 0);
    }

    /// Copy fields from another shadow VMCS
    pub fn copy_from(&mut self, other: &ShadowVmcs) {
        for (field, value) in &other.fields {
            self.fields.insert(*field, *value);
        }
    }

    /// Get number of fields
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

impl Default for ShadowVmcs {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Shadow VMCS cache for managing multiple VMCSes
#[derive(Debug, Default)]
pub struct ShadowVmcsCache {
    /// Cached VMCSes by guest physical address
    vmcs_map: HashMap<u64, ShadowVmcs>,
    /// Current VMCS pointer
    current_vmcs: Option<u64>,
    /// Statistics
    stats: ShadowVmcsCacheStats,
}

impl ShadowVmcsCache {
    /// Create a new cache
    pub fn new() -> Self {
        Self::default()
    }

    /// Load a VMCS (VMPTRLD)
    pub fn vmptrld(&mut self, guest_vmcs_addr: u64) -> VmxResult<()> {
        // Check if we already have this VMCS
        if !self.vmcs_map.contains_key(&guest_vmcs_addr) {
            let vmcs = ShadowVmcs::new(guest_vmcs_addr);
            self.vmcs_map.insert(guest_vmcs_addr, vmcs);
        }

        // Load the VMCS
        if let Some(vmcs) = self.vmcs_map.get_mut(&guest_vmcs_addr) {
            vmcs.load()?;
        }

        self.current_vmcs = Some(guest_vmcs_addr);
        self.stats.vmptrld_count += 1;
        Ok(())
    }

    /// Store VMCS pointer (VMPTRST)
    pub fn vmptrst(&self) -> VmxResult<u64> {
        self.current_vmcs
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)
    }

    /// Clear a VMCS (VMCLEAR)
    pub fn vmclear(&mut self, guest_vmcs_addr: u64) -> VmxResult<()> {
        if let Some(vmcs) = self.vmcs_map.get_mut(&guest_vmcs_addr) {
            vmcs.clear()?;
        }

        // If clearing current VMCS, unset it
        if self.current_vmcs == Some(guest_vmcs_addr) {
            self.current_vmcs = None;
        }

        self.stats.vmclear_count += 1;
        Ok(())
    }

    /// Launch current VMCS (VMLAUNCH)
    pub fn vmlaunch(&mut self) -> VmxResult<()> {
        let vmcs_addr = self
            .current_vmcs
            .ok_or(VmxInstructionError::VmlaunchNonclearVmcs)?;

        if let Some(vmcs) = self.vmcs_map.get_mut(&vmcs_addr) {
            vmcs.launch()?;
        }

        self.stats.vmlaunch_count += 1;
        Ok(())
    }

    /// Resume current VMCS (VMRESUME)
    pub fn vmresume(&mut self) -> VmxResult<()> {
        let vmcs_addr = self
            .current_vmcs
            .ok_or(VmxInstructionError::VmresumeNonlaunchedVmcs)?;

        if let Some(vmcs) = self.vmcs_map.get_mut(&vmcs_addr) {
            vmcs.resume()?;
        }

        self.stats.vmresume_count += 1;
        Ok(())
    }

    /// Read from current VMCS (VMREAD)
    pub fn vmread(&self, field: u32) -> VmxResult<u64> {
        let vmcs_addr = self
            .current_vmcs
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)?;

        self.vmcs_map
            .get(&vmcs_addr)
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)?
            .read(VmcsField(field))
    }

    /// Write to current VMCS (VMWRITE)
    pub fn vmwrite(&mut self, field: u32, value: u64) -> VmxResult<()> {
        let vmcs_addr = self
            .current_vmcs
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)?;

        self.vmcs_map
            .get_mut(&vmcs_addr)
            .ok_or(VmxInstructionError::VmreadVmwriteUnsupportedField)?
            .write(VmcsField(field), value)
    }

    /// Get current VMCS
    pub fn current(&self) -> Option<&ShadowVmcs> {
        self.current_vmcs.and_then(|addr| self.vmcs_map.get(&addr))
    }

    /// Get current VMCS mutably
    pub fn current_mut(&mut self) -> Option<&mut ShadowVmcs> {
        self.current_vmcs
            .and_then(|addr| self.vmcs_map.get_mut(&addr))
    }

    /// Get VMCS by address
    pub fn get(&self, addr: u64) -> Option<&ShadowVmcs> {
        self.vmcs_map.get(&addr)
    }

    /// Get VMCS by address mutably
    pub fn get_mut(&mut self, addr: u64) -> Option<&mut ShadowVmcs> {
        self.vmcs_map.get_mut(&addr)
    }

    /// Check if a VMCS is cached
    pub fn contains(&self, addr: u64) -> bool {
        self.vmcs_map.contains_key(&addr)
    }

    /// Remove a VMCS from cache
    pub fn remove(&mut self, addr: u64) {
        self.vmcs_map.remove(&addr);
        if self.current_vmcs == Some(addr) {
            self.current_vmcs = None;
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &ShadowVmcsCacheStats {
        &self.stats
    }

    /// Get number of cached VMCSes
    pub fn count(&self) -> usize {
        self.vmcs_map.len()
    }
}

/// Shadow VMCS cache statistics
#[derive(Debug, Clone, Default)]
pub struct ShadowVmcsCacheStats {
    /// VMPTRLD count
    pub vmptrld_count: u64,
    /// VMCLEAR count
    pub vmclear_count: u64,
    /// VMLAUNCH count
    pub vmlaunch_count: u64,
    /// VMRESUME count
    pub vmresume_count: u64,
    /// VMREAD count
    pub vmread_count: u64,
    /// VMWRITE count
    pub vmwrite_count: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_vmcs_creation() {
        let vmcs = ShadowVmcs::new(0x1000);
        assert_eq!(vmcs.guest_addr(), 0x1000);
        assert_eq!(vmcs.revision_id(), 1);
        assert_eq!(vmcs.state(), ShadowVmcsState::Clear);
        assert!(!vmcs.is_shadow());
    }

    #[test]
    fn test_shadow_vmcs_shadow_creation() {
        let vmcs = ShadowVmcs::new_shadow(0x2000, 2);
        assert!(vmcs.is_shadow());
    }

    #[test]
    fn test_shadow_vmcs_load() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.load().unwrap();
        assert_eq!(vmcs.state(), ShadowVmcsState::Loaded);
    }

    #[test]
    fn test_shadow_vmcs_clear() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.load().unwrap();
        vmcs.launch().unwrap();
        vmcs.clear().unwrap();

        assert_eq!(vmcs.state(), ShadowVmcsState::Clear);
        assert!(!vmcs.is_launched());
    }

    #[test]
    fn test_shadow_vmcs_launch() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.load().unwrap();
        vmcs.launch().unwrap();

        assert_eq!(vmcs.state(), ShadowVmcsState::Launched);
        assert!(vmcs.is_launched());
    }

    #[test]
    fn test_shadow_vmcs_resume_without_launch() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.load().unwrap();

        let result = vmcs.resume();
        assert!(matches!(
            result,
            Err(VmxInstructionError::VmlaunchNonclearVmcs)
        ));
    }

    #[test]
    fn test_shadow_vmcs_double_launch() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.load().unwrap();
        vmcs.launch().unwrap();

        let result = vmcs.launch();
        assert!(matches!(
            result,
            Err(VmxInstructionError::VmresumeNonlaunchedVmcs)
        ));
    }

    #[test]
    fn test_shadow_vmcs_read_write() {
        let mut vmcs = ShadowVmcs::new(0x1000);

        vmcs.write(VmcsField::GUEST_CR0, 0x8000_0011).unwrap();
        assert_eq!(vmcs.read(VmcsField::GUEST_CR0).unwrap(), 0x8000_0011);
    }

    #[test]
    fn test_shadow_vmcs_write_readonly() {
        let mut vmcs = ShadowVmcs::new(0x1000);

        let result = vmcs.write(VmcsField::VM_EXIT_REASON, 0);
        assert!(matches!(
            result,
            Err(VmxInstructionError::VmwriteReadonlyField)
        ));
    }

    #[test]
    fn test_shadow_vmcs_read_or_default() {
        let vmcs = ShadowVmcs::new(0x1000);
        assert_eq!(vmcs.read_or_default(VmcsField::GUEST_CR0, 0x1234), 0x1234);
    }

    #[test]
    fn test_shadow_vmcs_has_field() {
        let mut vmcs = ShadowVmcs::new(0x1000);

        assert!(!vmcs.has_field(VmcsField::GUEST_CR0));
        vmcs.write(VmcsField::GUEST_CR0, 0).unwrap();
        assert!(vmcs.has_field(VmcsField::GUEST_CR0));
    }

    #[test]
    fn test_shadow_vmcs_dirty_fields() {
        let mut vmcs = ShadowVmcs::new(0x1000);

        vmcs.write(VmcsField::GUEST_CR0, 0).unwrap();
        vmcs.write(VmcsField::GUEST_CR3, 0).unwrap();

        assert_eq!(vmcs.dirty_fields().len(), 2);

        vmcs.clear_dirty();
        assert!(vmcs.dirty_fields().is_empty());
    }

    #[test]
    fn test_shadow_vmcs_initialize_defaults() {
        let mut vmcs = ShadowVmcs::new(0x1000);
        vmcs.initialize_defaults();

        assert!(vmcs.has_field(VmcsField::GUEST_CR0));
        assert!(vmcs.has_field(VmcsField::GUEST_RFLAGS));
        assert!(vmcs.has_field(VmcsField::VMCS_LINK_POINTER));
    }

    #[test]
    fn test_shadow_vmcs_copy_from() {
        let mut vmcs1 = ShadowVmcs::new(0x1000);
        vmcs1.write(VmcsField::GUEST_CR0, 0x1234).unwrap();
        vmcs1.write(VmcsField::GUEST_CR3, 0x5678).unwrap();

        let mut vmcs2 = ShadowVmcs::new(0x2000);
        vmcs2.copy_from(&vmcs1);

        assert_eq!(vmcs2.read(VmcsField::GUEST_CR0).unwrap(), 0x1234);
        assert_eq!(vmcs2.read(VmcsField::GUEST_CR3).unwrap(), 0x5678);
    }

    #[test]
    fn test_shadow_vmcs_cache_creation() {
        let cache = ShadowVmcsCache::new();
        assert_eq!(cache.count(), 0);
        assert!(cache.current().is_none());
    }

    #[test]
    fn test_shadow_vmcs_cache_vmptrld() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();

        assert_eq!(cache.count(), 1);
        assert!(cache.current().is_some());
        assert!(cache.contains(0x1000));
    }

    #[test]
    fn test_shadow_vmcs_cache_vmptrst() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();

        assert_eq!(cache.vmptrst().unwrap(), 0x1000);
    }

    #[test]
    fn test_shadow_vmcs_cache_vmclear() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();
        cache.vmclear(0x1000).unwrap();

        assert!(cache.current().is_none());
    }

    #[test]
    fn test_shadow_vmcs_cache_vmlaunch() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();
        cache.vmlaunch().unwrap();

        assert!(cache.current().unwrap().is_launched());
    }

    #[test]
    fn test_shadow_vmcs_cache_vmresume() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();
        cache.vmlaunch().unwrap();
        cache.vmresume().unwrap();

        assert_eq!(cache.stats().vmresume_count, 1);
    }

    #[test]
    fn test_shadow_vmcs_cache_vmread_vmwrite() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();

        cache.vmwrite(VmcsField::GUEST_CR0.0, 0xABCD).unwrap();
        assert_eq!(cache.vmread(VmcsField::GUEST_CR0.0).unwrap(), 0xABCD);
    }

    #[test]
    fn test_shadow_vmcs_cache_remove() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();

        cache.remove(0x1000);
        assert_eq!(cache.count(), 0);
        assert!(cache.current().is_none());
    }

    #[test]
    fn test_shadow_vmcs_cache_stats() {
        let mut cache = ShadowVmcsCache::new();
        cache.vmptrld(0x1000).unwrap();
        cache.vmlaunch().unwrap();
        cache.vmclear(0x1000).unwrap();

        let stats = cache.stats();
        assert_eq!(stats.vmptrld_count, 1);
        assert_eq!(stats.vmlaunch_count, 1);
        assert_eq!(stats.vmclear_count, 1);
    }
}
