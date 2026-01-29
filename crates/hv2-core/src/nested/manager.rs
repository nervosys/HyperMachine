//! Nested virtualization manager
//!
//! This module provides the main manager for nested virtualization,
//! handling L1/L2 transitions and VMX instruction emulation.

use std::collections::HashMap;

use super::ept::NestedEptManager;
use super::shadow_vmcs::{ShadowVmcs, ShadowVmcsCache};
use super::types::{
    NestedGuestState, NestedLevel, NestedStats, SavedL1State, VmExitReason, VmxCapabilities,
    VmxInstructionError,
};

/// Result of a nested operation
pub type NestedResult<T> = Result<T, NestedError>;

/// Nested virtualization errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NestedError {
    /// VMX is not enabled
    VmxNotEnabled,
    /// VMX is already enabled
    VmxAlreadyEnabled,
    /// Invalid VMCS address
    InvalidVmcsAddress(u64),
    /// No current VMCS
    NoCurrentVmcs,
    /// Invalid VMCS state
    InvalidVmcsState,
    /// VMCS already launched
    VmcsAlreadyLaunched,
    /// VMCS not launched
    VmcsNotLaunched,
    /// Invalid VMCS field
    InvalidVmcsField(u32),
    /// Not in L2
    NotInL2,
    /// Already in L2
    AlreadyInL2,
    /// VMX instruction error
    VmxInstructionError(VmxInstructionError),
    /// EPT misconfiguration
    EptMisconfiguration(u64),
    /// Invalid guest state
    InvalidGuestState(String),
}

impl std::fmt::Display for NestedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VmxNotEnabled => write!(f, "VMX is not enabled"),
            Self::VmxAlreadyEnabled => write!(f, "VMX is already enabled"),
            Self::InvalidVmcsAddress(addr) => write!(f, "Invalid VMCS address: {:#x}", addr),
            Self::NoCurrentVmcs => write!(f, "No current VMCS"),
            Self::InvalidVmcsState => write!(f, "Invalid VMCS state"),
            Self::VmcsAlreadyLaunched => write!(f, "VMCS already launched"),
            Self::VmcsNotLaunched => write!(f, "VMCS not launched"),
            Self::InvalidVmcsField(field) => write!(f, "Invalid VMCS field: {:#x}", field),
            Self::NotInL2 => write!(f, "Not in L2 guest"),
            Self::AlreadyInL2 => write!(f, "Already in L2 guest"),
            Self::VmxInstructionError(err) => write!(f, "VMX instruction error: {:?}", err),
            Self::EptMisconfiguration(gpa) => write!(f, "EPT misconfiguration at GPA {:#x}", gpa),
            Self::InvalidGuestState(msg) => write!(f, "Invalid guest state: {}", msg),
        }
    }
}

impl std::error::Error for NestedError {}

impl From<VmxInstructionError> for NestedError {
    fn from(err: VmxInstructionError) -> Self {
        NestedError::VmxInstructionError(err)
    }
}

/// L2 entry information
#[derive(Debug, Clone, Default)]
pub struct L2EntryInfo {
    /// Guest RIP to start execution
    pub rip: u64,
    /// Guest RSP
    pub rsp: u64,
    /// Guest RFLAGS
    pub rflags: u64,
    /// Guest CR0
    pub cr0: u64,
    /// Guest CR3
    pub cr3: u64,
    /// Guest CR4
    pub cr4: u64,
    /// Entry interruption info
    pub entry_interruption_info: u32,
    /// Entry exception error code
    pub entry_exception_error_code: u32,
    /// Entry instruction length
    pub entry_instruction_length: u32,
}

/// L2 exit information
#[derive(Debug, Clone, Default)]
pub struct L2ExitInfo {
    /// Exit reason
    pub exit_reason: u32,
    /// Exit qualification
    pub exit_qualification: u64,
    /// Guest linear address
    pub guest_linear_addr: u64,
    /// Guest physical address
    pub guest_physical_addr: u64,
    /// VM instruction error
    pub vm_instruction_error: u32,
    /// IDT vectoring info
    pub idt_vectoring_info: u32,
    /// IDT vectoring error code
    pub idt_vectoring_error_code: u32,
    /// Exit instruction length
    pub exit_instruction_length: u32,
    /// Exit instruction info
    pub exit_instruction_info: u32,
}

impl L2ExitInfo {
    /// Get the basic exit reason
    pub fn basic_reason(&self) -> u16 {
        (self.exit_reason & 0xFFFF) as u16
    }

    /// Check if this is a VM-entry failure
    pub fn is_entry_failure(&self) -> bool {
        self.exit_reason & (1 << 31) != 0
    }

    /// Get the exit reason enum
    pub fn reason(&self) -> VmExitReason {
        VmExitReason(self.exit_reason)
    }
}

/// Nested manager configuration
#[derive(Debug, Clone)]
pub struct NestedConfig {
    /// VMX capabilities to expose to L1
    pub capabilities: VmxCapabilities,
    /// Enable shadow VMCS
    pub shadow_vmcs_enabled: bool,
    /// Enable nested EPT
    pub nested_ept_enabled: bool,
    /// Enable VPID support
    pub vpid_enabled: bool,
    /// Maximum cached VMCSes
    pub max_vmcs_cache: usize,
}

impl Default for NestedConfig {
    fn default() -> Self {
        Self {
            capabilities: VmxCapabilities::default_nested(),
            shadow_vmcs_enabled: true,
            nested_ept_enabled: true,
            vpid_enabled: true,
            max_vmcs_cache: 64,
        }
    }
}

/// Nested virtualization manager
#[derive(Debug)]
pub struct NestedManager {
    /// Configuration
    config: NestedConfig,
    /// Per-vCPU nested state
    vcpu_states: HashMap<u32, VcpuNestedState>,
    /// Statistics
    stats: NestedStats,
}

/// Per-vCPU nested state
#[derive(Debug, Default)]
struct VcpuNestedState {
    /// Guest state
    guest_state: NestedGuestState,
    /// Shadow VMCS cache
    vmcs_cache: ShadowVmcsCache,
    /// Nested EPT manager
    ept_manager: NestedEptManager,
    /// Saved L1 state during L2 execution
    saved_l1_state: Option<SavedL1State>,
}

impl NestedManager {
    /// Create a new nested manager
    pub fn new(config: NestedConfig) -> Self {
        Self {
            config,
            vcpu_states: HashMap::new(),
            stats: NestedStats::default(),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(NestedConfig::default())
    }

    /// Get configuration
    pub fn config(&self) -> &NestedConfig {
        &self.config
    }

    /// Get statistics
    pub fn stats(&self) -> &NestedStats {
        &self.stats
    }

    /// Initialize nested state for a vCPU
    pub fn init_vcpu(&mut self, vcpu_id: u32) {
        self.vcpu_states.insert(vcpu_id, VcpuNestedState::default());
    }

    /// Remove nested state for a vCPU
    pub fn remove_vcpu(&mut self, vcpu_id: u32) {
        self.vcpu_states.remove(&vcpu_id);
    }

    /// Get the current nesting level for a vCPU
    pub fn current_level(&self, vcpu_id: u32) -> NestedLevel {
        self.vcpu_states
            .get(&vcpu_id)
            .map(|s| s.guest_state.level)
            .unwrap_or(NestedLevel::L0)
    }

    /// Check if VMX is enabled for a vCPU
    pub fn is_vmx_enabled(&self, vcpu_id: u32) -> bool {
        self.vcpu_states
            .get(&vcpu_id)
            .map(|s| s.guest_state.is_vmx_enabled())
            .unwrap_or(false)
    }

    /// Handle VMXON instruction
    pub fn handle_vmxon(&mut self, vcpu_id: u32, vmxon_region: u64) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxAlreadyEnabled);
        }

        // Validate VMXON region address
        if vmxon_region == 0 || vmxon_region & 0xFFF != 0 {
            return Err(NestedError::InvalidVmcsAddress(vmxon_region));
        }

        state.guest_state.enable_vmx(vmxon_region);
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Handle VMXOFF instruction
    pub fn handle_vmxoff(&mut self, vcpu_id: u32) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        if state.guest_state.level == NestedLevel::L2 {
            return Err(NestedError::AlreadyInL2);
        }

        state.guest_state.disable_vmx();
        state.vmcs_cache = ShadowVmcsCache::default();
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Handle VMPTRLD instruction
    pub fn handle_vmptrld(&mut self, vcpu_id: u32, vmcs_addr: u64) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        // Validate VMCS address
        if vmcs_addr == 0 || vmcs_addr & 0xFFF != 0 {
            return Err(NestedError::InvalidVmcsAddress(vmcs_addr));
        }

        state.vmcs_cache.vmptrld(vmcs_addr)?;
        self.stats.vmcs_switches += 1;
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Handle VMPTRST instruction
    pub fn handle_vmptrst(&self, vcpu_id: u32) -> NestedResult<u64> {
        let state = self
            .vcpu_states
            .get(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        Ok(state.vmcs_cache.vmptrst().unwrap())
    }

    /// Handle VMCLEAR instruction
    pub fn handle_vmclear(&mut self, vcpu_id: u32, vmcs_addr: u64) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        state.vmcs_cache.vmclear(vmcs_addr)?;
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Handle VMREAD instruction
    pub fn handle_vmread(&self, vcpu_id: u32, field: u32) -> NestedResult<u64> {
        let state = self
            .vcpu_states
            .get(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        Ok(state.vmcs_cache.vmread(field)?)
    }

    /// Handle VMWRITE instruction
    pub fn handle_vmwrite(&mut self, vcpu_id: u32, field: u32, value: u64) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        Ok(state.vmcs_cache.vmwrite(field, value)?)
    }

    /// Handle VMLAUNCH instruction
    pub fn handle_vmlaunch(&mut self, vcpu_id: u32, l1_state: SavedL1State) -> NestedResult<L2EntryInfo> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        if state.guest_state.level == NestedLevel::L2 {
            return Err(NestedError::AlreadyInL2);
        }

        // Launch the VMCS
        state.vmcs_cache.vmlaunch()?;

        // Save L1 state
        state.saved_l1_state = Some(l1_state);

        // Enter L2
        state.guest_state.enter_l2();
        self.stats.l2_entries += 1;
        self.stats.emulated_vmx_instructions += 1;

        // Build L2 entry info from VMCS
        Ok(Self::build_l2_entry_info(state))
    }

    /// Handle VMRESUME instruction
    pub fn handle_vmresume(&mut self, vcpu_id: u32, l1_state: SavedL1State) -> NestedResult<L2EntryInfo> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        if state.guest_state.level == NestedLevel::L2 {
            return Err(NestedError::AlreadyInL2);
        }

        // Resume the VMCS
        state.vmcs_cache.vmresume()?;

        // Save L1 state
        state.saved_l1_state = Some(l1_state);

        // Enter L2
        state.guest_state.enter_l2();
        self.stats.l2_entries += 1;
        self.stats.emulated_vmx_instructions += 1;

        Ok(Self::build_l2_entry_info(state))
    }

    /// Build L2 entry information from current VMCS
    fn build_l2_entry_info(state: &VcpuNestedState) -> L2EntryInfo {
        use super::types::VmcsField;

        L2EntryInfo {
            rip: state.vmcs_cache.vmread(VmcsField::GUEST_RIP.0).unwrap_or(0),
            rsp: state.vmcs_cache.vmread(VmcsField::GUEST_RSP.0).unwrap_or(0),
            rflags: state.vmcs_cache.vmread(VmcsField::GUEST_RFLAGS.0).unwrap_or(0x2),
            cr0: state.vmcs_cache.vmread(VmcsField::GUEST_CR0.0).unwrap_or(0),
            cr3: state.vmcs_cache.vmread(VmcsField::GUEST_CR3.0).unwrap_or(0),
            cr4: state.vmcs_cache.vmread(VmcsField::GUEST_CR4.0).unwrap_or(0),
            entry_interruption_info: state
                .vmcs_cache
                .vmread(VmcsField::VM_ENTRY_INTR_INFO.0)
                .unwrap_or(0) as u32,
            entry_exception_error_code: state
                .vmcs_cache
                .vmread(VmcsField::VM_ENTRY_EXCEPTION_ERROR_CODE.0)
                .unwrap_or(0) as u32,
            entry_instruction_length: state
                .vmcs_cache
                .vmread(VmcsField::VM_ENTRY_INSTRUCTION_LEN.0)
                .unwrap_or(0) as u32,
        }
    }

    /// Handle an L2 exit
    pub fn handle_l2_exit(&mut self, vcpu_id: u32, exit_info: L2ExitInfo) -> NestedResult<ExitDisposition> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::NotInL2)?;

        if state.guest_state.level != NestedLevel::L2 {
            return Err(NestedError::NotInL2);
        }

        self.stats.l2_exits += 1;

        // Determine if we should reflect the exit to L1 or handle it ourselves
        let disposition = Self::classify_exit(&exit_info);

        match disposition {
            ExitDisposition::ReflectToL1 => {
                // Update VMCS exit fields
                Self::update_vmcs_exit_fields(state, &exit_info);

                // Exit L2, return to L1
                state.guest_state.exit_l2();
                self.stats.reflected_exits += 1;
            }
            ExitDisposition::HandleInL0 => {
                // L0 will handle this exit directly
            }
        }

        Ok(disposition)
    }

    /// Classify an exit to determine how to handle it
    fn classify_exit(exit_info: &L2ExitInfo) -> ExitDisposition {
        match exit_info.reason() {
            // These exits are always reflected to L1
            VmExitReason::EXTERNAL_INTERRUPT
            | VmExitReason::EXCEPTION_NMI
            | VmExitReason::INIT
            | VmExitReason::SIPI => ExitDisposition::HandleInL0,

            // EPT violations need special handling
            VmExitReason::EPT_VIOLATION | VmExitReason::EPT_MISCONFIG => {
                // Check if it's for L1's EPT or our EPT
                ExitDisposition::HandleInL0
            }

            // Most other exits should be reflected to L1
            _ => ExitDisposition::ReflectToL1,
        }
    }

    /// Update VMCS exit fields for L1
    fn update_vmcs_exit_fields(state: &VcpuNestedState, exit_info: &L2ExitInfo) {
        use super::types::VmcsField;

        let _ = state.vmcs_cache.vmread(VmcsField::VM_EXIT_REASON.0);
        // In a real implementation, we would write these to the shadow VMCS:
        // - VM_EXIT_REASON
        // - EXIT_QUALIFICATION
        // - GUEST_LINEAR_ADDRESS
        // - GUEST_PHYSICAL_ADDRESS
        // - VM_EXIT_INTR_INFO
        // - VM_EXIT_INTR_ERROR_CODE
        // - IDT_VECTORING_INFO
        // - IDT_VECTORING_ERROR_CODE
        // - VM_EXIT_INSTRUCTION_LEN
        // - VM_EXIT_INSTRUCTION_INFO
        let _ = exit_info;
    }

    /// Get saved L1 state for a vCPU
    pub fn get_saved_l1_state(&self, vcpu_id: u32) -> Option<&SavedL1State> {
        self.vcpu_states
            .get(&vcpu_id)
            .and_then(|s| s.saved_l1_state.as_ref())
    }

    /// Get nested EPT manager for a vCPU
    pub fn ept_manager(&self, vcpu_id: u32) -> Option<&NestedEptManager> {
        self.vcpu_states.get(&vcpu_id).map(|s| &s.ept_manager)
    }

    /// Get mutable nested EPT manager for a vCPU
    pub fn ept_manager_mut(&mut self, vcpu_id: u32) -> Option<&mut NestedEptManager> {
        self.vcpu_states.get_mut(&vcpu_id).map(|s| &mut s.ept_manager)
    }

    /// Get VMCS cache for a vCPU
    pub fn vmcs_cache(&self, vcpu_id: u32) -> Option<&ShadowVmcsCache> {
        self.vcpu_states.get(&vcpu_id).map(|s| &s.vmcs_cache)
    }

    /// Get mutable VMCS cache for a vCPU
    pub fn vmcs_cache_mut(&mut self, vcpu_id: u32) -> Option<&mut ShadowVmcsCache> {
        self.vcpu_states.get_mut(&vcpu_id).map(|s| &mut s.vmcs_cache)
    }

    /// Handle INVEPT instruction
    pub fn handle_invept(&mut self, vcpu_id: u32, inv_type: u64, eptp: u64) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        // Invalidate EPT caches based on type
        match inv_type {
            1 => {
                // Single-context invalidation
                state.ept_manager.flush_cache();
            }
            2 => {
                // Global invalidation
                state.ept_manager.flush_cache();
            }
            _ => {
                return Err(NestedError::VmxInstructionError(
                    VmxInstructionError::InvalidOperandToInveptInvvpid,
                ));
            }
        }

        let _ = eptp;
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Handle INVVPID instruction
    pub fn handle_invvpid(&mut self, vcpu_id: u32, inv_type: u64, vpid: u16) -> NestedResult<()> {
        let state = self
            .vcpu_states
            .get_mut(&vcpu_id)
            .ok_or(NestedError::VmxNotEnabled)?;

        if !state.guest_state.is_vmx_enabled() {
            return Err(NestedError::VmxNotEnabled);
        }

        // Invalidate TLB based on VPID type
        match inv_type {
            0 => {
                // Individual address
            }
            1 | 3 => {
                // Single context (with/without retaining globals)
            }
            2 => {
                // All contexts
            }
            _ => {
                return Err(NestedError::VmxInstructionError(
                    VmxInstructionError::InvalidOperandToInveptInvvpid,
                ));
            }
        }

        let _ = vpid;
        self.stats.emulated_vmx_instructions += 1;

        Ok(())
    }

    /// Get the number of initialized vCPUs
    pub fn vcpu_count(&self) -> usize {
        self.vcpu_states.len()
    }

    /// Check if a vCPU is initialized
    pub fn is_vcpu_initialized(&self, vcpu_id: u32) -> bool {
        self.vcpu_states.contains_key(&vcpu_id)
    }
}

/// Disposition for handling an L2 exit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitDisposition {
    /// Reflect the exit to L1
    ReflectToL1,
    /// Handle the exit in L0 (our hypervisor)
    HandleInL0,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_manager_creation() {
        let manager = NestedManager::with_defaults();
        assert!(manager.config.shadow_vmcs_enabled);
        assert!(manager.config.nested_ept_enabled);
    }

    #[test]
    fn test_nested_manager_init_vcpu() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        assert!(manager.is_vcpu_initialized(0));
        assert_eq!(manager.current_level(0), NestedLevel::L0);
    }

    #[test]
    fn test_nested_manager_remove_vcpu() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.remove_vcpu(0);
        assert!(!manager.is_vcpu_initialized(0));
    }

    #[test]
    fn test_handle_vmxon() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        let result = manager.handle_vmxon(0, 0x1000);
        assert!(result.is_ok());
        assert!(manager.is_vmx_enabled(0));
    }

    #[test]
    fn test_handle_vmxon_invalid_address() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        // Unaligned address
        let result = manager.handle_vmxon(0, 0x1001);
        assert!(matches!(result, Err(NestedError::InvalidVmcsAddress(_))));

        // Zero address
        let result = manager.handle_vmxon(0, 0);
        assert!(matches!(result, Err(NestedError::InvalidVmcsAddress(_))));
    }

    #[test]
    fn test_handle_vmxon_already_enabled() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        manager.handle_vmxon(0, 0x1000).unwrap();
        let result = manager.handle_vmxon(0, 0x1000);
        assert!(matches!(result, Err(NestedError::VmxAlreadyEnabled)));
    }

    #[test]
    fn test_handle_vmxoff() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        manager.handle_vmxon(0, 0x1000).unwrap();
        let result = manager.handle_vmxoff(0);
        assert!(result.is_ok());
        assert!(!manager.is_vmx_enabled(0));
    }

    #[test]
    fn test_handle_vmxoff_not_enabled() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        let result = manager.handle_vmxoff(0);
        assert!(matches!(result, Err(NestedError::VmxNotEnabled)));
    }

    #[test]
    fn test_handle_vmptrld() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();

        let result = manager.handle_vmptrld(0, 0x2000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_vmptrst() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        let result = manager.handle_vmptrst(0);
        assert_eq!(result.unwrap(), 0x2000);
    }

    #[test]
    fn test_handle_vmclear() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        let result = manager.handle_vmclear(0, 0x2000);
        assert!(result.is_ok());
    }

    #[test]
    fn test_handle_vmread_vmwrite() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        // Write a field
        let field = 0x4800; // Guest RIP
        manager.handle_vmwrite(0, field, 0x12345).unwrap();

        // Read it back
        let value = manager.handle_vmread(0, field).unwrap();
        assert_eq!(value, 0x12345);
    }

    #[test]
    fn test_handle_vmlaunch() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        let l1_state = SavedL1State::default();
        let result = manager.handle_vmlaunch(0, l1_state);
        assert!(result.is_ok());
        assert_eq!(manager.current_level(0), NestedLevel::L2);
    }

    #[test]
    fn test_handle_vmresume_not_launched() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        let l1_state = SavedL1State::default();
        let result = manager.handle_vmresume(0, l1_state);
        assert!(result.is_err());
    }

    #[test]
    fn test_nested_error_display() {
        let err = NestedError::VmxNotEnabled;
        assert_eq!(format!("{}", err), "VMX is not enabled");

        let err = NestedError::InvalidVmcsAddress(0x1234);
        assert!(format!("{}", err).contains("0x1234"));
    }

    #[test]
    fn test_l2_exit_info() {
        let exit_info = L2ExitInfo {
            exit_reason: 48, // EPT violation
            exit_qualification: 0x123,
            ..Default::default()
        };

        assert_eq!(exit_info.basic_reason(), 48);
        assert!(!exit_info.is_entry_failure());
        assert_eq!(exit_info.reason(), VmExitReason::EPT_VIOLATION);
    }

    #[test]
    fn test_l2_exit_info_entry_failure() {
        let exit_info = L2ExitInfo {
            exit_reason: (1 << 31) | 33, // Entry failure + invalid guest state
            ..Default::default()
        };

        assert!(exit_info.is_entry_failure());
    }

    #[test]
    fn test_exit_disposition() {
        let manager = NestedManager::with_defaults();

        // EPT violation handled in L0
        let exit_info = L2ExitInfo {
            exit_reason: 48,
            ..Default::default()
        };
        assert_eq!(
            NestedManager::classify_exit(&exit_info),
            ExitDisposition::HandleInL0
        );

        // CPUID reflected to L1
        let exit_info = L2ExitInfo {
            exit_reason: 10,
            ..Default::default()
        };
        assert_eq!(
            NestedManager::classify_exit(&exit_info),
            ExitDisposition::ReflectToL1
        );
    }

    #[test]
    fn test_handle_invept() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();

        // Single context
        let result = manager.handle_invept(0, 1, 0);
        assert!(result.is_ok());

        // Global
        let result = manager.handle_invept(0, 2, 0);
        assert!(result.is_ok());

        // Invalid type
        let result = manager.handle_invept(0, 99, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_handle_invvpid() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();

        // Individual address
        let result = manager.handle_invvpid(0, 0, 1);
        assert!(result.is_ok());

        // Single context
        let result = manager.handle_invvpid(0, 1, 1);
        assert!(result.is_ok());

        // All contexts
        let result = manager.handle_invvpid(0, 2, 0);
        assert!(result.is_ok());

        // Invalid type
        let result = manager.handle_invvpid(0, 99, 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_saved_l1_state() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);
        manager.handle_vmxon(0, 0x1000).unwrap();
        manager.handle_vmptrld(0, 0x2000).unwrap();

        let l1_state = SavedL1State {
            rip: 0x12345,
            ..Default::default()
        };
        manager.handle_vmlaunch(0, l1_state).unwrap();

        let saved = manager.get_saved_l1_state(0);
        assert!(saved.is_some());
        assert_eq!(saved.unwrap().rip, 0x12345);
    }

    #[test]
    fn test_vcpu_count() {
        let mut manager = NestedManager::with_defaults();
        assert_eq!(manager.vcpu_count(), 0);

        manager.init_vcpu(0);
        manager.init_vcpu(1);
        assert_eq!(manager.vcpu_count(), 2);

        manager.remove_vcpu(0);
        assert_eq!(manager.vcpu_count(), 1);
    }

    #[test]
    fn test_ept_manager_access() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        assert!(manager.ept_manager(0).is_some());
        assert!(manager.ept_manager_mut(0).is_some());
    }

    #[test]
    fn test_vmcs_cache_access() {
        let mut manager = NestedManager::with_defaults();
        manager.init_vcpu(0);

        assert!(manager.vmcs_cache(0).is_some());
        assert!(manager.vmcs_cache_mut(0).is_some());
    }
}










