//! Nested virtualization support
//!
//! This module provides nested virtualization capabilities for running
//! hypervisors inside guests (L1 guests running L2 guests).
//!
//! # Architecture
//!
//! The nested virtualization support follows a three-level model:
//! - **L0**: The physical hypervisor (AetherVM)
//! - **L1**: The guest hypervisor running inside our VM
//! - **L2**: The nested guest running inside the L1 hypervisor
//!
//! # Components
//!
//! - [`types`]: Core types for nested VMX operations including VMCS fields,
//!   exit reasons, and guest state tracking.
//! - [`shadow_vmcs`]: Shadow VMCS management for tracking L1's VMCS state.
//! - [`ept`]: Nested EPT (Extended Page Tables) support for L2 address translation.
//! - [`manager`]: The main nested manager handling L1/L2 transitions.
//!
//! # VMX Instruction Emulation
//!
//! The nested manager emulates the following VMX instructions:
//! - `VMXON`/`VMXOFF`: Enable/disable VMX operation
//! - `VMPTRLD`/`VMPTRST`/`VMCLEAR`: VMCS management
//! - `VMREAD`/`VMWRITE`: VMCS field access
//! - `VMLAUNCH`/`VMRESUME`: Enter L2 guest
//! - `INVEPT`/`INVVPID`: TLB invalidation
//!
//! # Example
//!
//! ```rust,no_run
//! use hv2_core::nested::{NestedManager, NestedConfig, SavedL1State};
//!
//! // Create the nested manager
//! let mut manager = NestedManager::with_defaults();
//!
//! // Initialize nested state for a vCPU
//! manager.init_vcpu(0);
//!
//! // Handle VMXON from L1
//! manager.handle_vmxon(0, 0x1000).unwrap();
//!
//! // Handle VMPTRLD to set current VMCS
//! manager.handle_vmptrld(0, 0x2000).unwrap();
//!
//! // Handle VMLAUNCH to enter L2
//! let l1_state = SavedL1State::default();
//! let l2_entry = manager.handle_vmlaunch(0, l1_state).unwrap();
//! ```

mod ept;
mod manager;
mod shadow_vmcs;
mod types;

// Re-export primary types
pub use ept::{
    ept_flags, EptEntry, EptMemoryType, EptPointer, EptTranslationResult,
    EptViolationQualification, InvEptType, InvVpidType, NestedEptManager, NestedEptStats, Vpid,
    PAGE_SIZE_1G, PAGE_SIZE_2M, PAGE_SIZE_4K,
};
pub use manager::{
    ExitDisposition, L2EntryInfo, L2ExitInfo, NestedConfig, NestedError, NestedManager,
    NestedResult,
};
pub use shadow_vmcs::{ShadowVmcs, ShadowVmcsCache, ShadowVmcsCacheStats, ShadowVmcsState};
pub use types::{
    interruptibility, GuestActivityState, NestedGuestState, NestedLevel, NestedStats, SavedL1State,
    VmExitReason, VmcsAccessType, VmcsField, VmxCapabilities, VmxInstructionError,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify all primary types are accessible
        let _ = NestedManager::with_defaults();
        let _ = NestedConfig::default();
        let _ = NestedLevel::L0;
        let _ = VmExitReason::CPUID;
        let _ = EptPointer::with_defaults(0x1000);
        let _ = ShadowVmcs::new(0x1000);
        let _ = VmxCapabilities::default_nested();
    }

    #[test]
    fn test_nested_workflow() {
        // Test a complete nested workflow
        let mut manager = NestedManager::with_defaults();

        // Initialize vCPU
        manager.init_vcpu(0);
        assert!(manager.is_vcpu_initialized(0));

        // Enable VMX
        manager.handle_vmxon(0, 0x1000).unwrap();
        assert!(manager.is_vmx_enabled(0));

        // Load VMCS
        manager.handle_vmptrld(0, 0x2000).unwrap();

        // Read/Write VMCS
        manager
            .handle_vmwrite(0, VmcsField::GUEST_RIP.0, 0x12345)
            .unwrap();
        assert_eq!(
            manager.handle_vmread(0, VmcsField::GUEST_RIP.0).unwrap(),
            0x12345
        );

        // Launch L2
        let l1_state = SavedL1State::default();
        let entry_info = manager.handle_vmlaunch(0, l1_state).unwrap();
        assert_eq!(entry_info.rip, 0x12345);
        assert_eq!(manager.current_level(0), NestedLevel::L2);
    }

    #[test]
    fn test_ept_types() {
        let eptp = EptPointer::new(0x1000, EptMemoryType::WriteBack, 4);
        assert_eq!(eptp.pml4_addr(), 0x1000);

        let entry = EptEntry::page_entry(0x2000, EptMemoryType::WriteBack, ept_flags::RWX, false);
        assert!(entry.is_readable());
        assert!(entry.is_writable());
        assert!(entry.is_executable());
    }

    #[test]
    fn test_vmcs_fields() {
        assert_eq!(VmcsField::GUEST_RIP.0, 0x681E);
        assert_eq!(VmcsField::GUEST_RSP.0, 0x681C);
        assert_eq!(VmcsField::GUEST_RFLAGS.0, 0x6820);
    }

    #[test]
    fn test_interruptibility_flags() {
        assert_eq!(interruptibility::BLOCKING_BY_STI, 1 << 0);
        assert_eq!(interruptibility::BLOCKING_BY_MOV_SS, 1 << 1);
        assert_eq!(interruptibility::BLOCKING_BY_SMI, 1 << 2);
        assert_eq!(interruptibility::BLOCKING_BY_NMI, 1 << 3);
    }
}
