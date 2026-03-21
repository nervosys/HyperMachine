//! Error types for HV1 ARM64 EL2 hypervisor backend

use core::fmt;

/// ARM64 EL2 Result type
pub type Result<T> = core::result::Result<T, Error>;

/// ARM64 EL2 Error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Hypervisor already initialized
    AlreadyInitialized,
    /// EL2 not available (running at wrong exception level)
    El2NotAvailable,
    /// Virtualization extensions not supported
    NoVirtualizationSupport,
    /// Stage-2 translation fault
    Stage2Fault,
    /// Stage-2 page table allocation failed
    Stage2AllocationFailed,
    /// Invalid stage-2 mapping
    InvalidStage2Mapping,
    /// Overlapping stage-2 mapping
    OverlappingMapping,
    /// vGIC initialization failed
    VgicInitFailed,
    /// vGIC distribution error
    VgicDistributorError,
    /// vGIC redistribution error
    VgicRedistributorError,
    /// Invalid interrupt ID
    InvalidInterruptId,
    /// vCPU creation failed
    VcpuCreateFailed,
    /// vCPU already running
    VcpuAlreadyRunning,
    /// Invalid vCPU state
    InvalidVcpuState,
    /// System register access fault
    SysregAccessFault,
    /// Unknown system register
    UnknownSysreg,
    /// HVC (Hypervisor Call) error
    HvcError,
    /// SMC (Secure Monitor Call) trapped
    SmcTrapped,
    /// Memory allocation failed
    AllocationFailed,
    /// Invalid guest state
    InvalidGuestState,
    /// VM exit error
    VmExitError,
    /// Invalid parameter
    InvalidParameter,
    /// Out of memory
    OutOfMemory,
    /// Not supported on this hardware
    NotSupported,
    /// Internal error
    Internal,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlreadyInitialized => write!(f, "Hypervisor already initialized"),
            Error::El2NotAvailable => write!(f, "EL2 not available"),
            Error::NoVirtualizationSupport => write!(f, "Virtualization extensions not supported"),
            Error::Stage2Fault => write!(f, "Stage-2 translation fault"),
            Error::Stage2AllocationFailed => write!(f, "Stage-2 page table allocation failed"),
            Error::InvalidStage2Mapping => write!(f, "Invalid stage-2 mapping"),
            Error::OverlappingMapping => write!(f, "Overlapping stage-2 mapping"),
            Error::VgicInitFailed => write!(f, "vGIC initialization failed"),
            Error::VgicDistributorError => write!(f, "vGIC distributor error"),
            Error::VgicRedistributorError => write!(f, "vGIC redistributor error"),
            Error::InvalidInterruptId => write!(f, "Invalid interrupt ID"),
            Error::VcpuCreateFailed => write!(f, "vCPU creation failed"),
            Error::VcpuAlreadyRunning => write!(f, "vCPU already running"),
            Error::InvalidVcpuState => write!(f, "Invalid vCPU state"),
            Error::SysregAccessFault => write!(f, "System register access fault"),
            Error::UnknownSysreg => write!(f, "Unknown system register"),
            Error::HvcError => write!(f, "HVC error"),
            Error::SmcTrapped => write!(f, "SMC trapped"),
            Error::AllocationFailed => write!(f, "Memory allocation failed"),
            Error::InvalidGuestState => write!(f, "Invalid guest state"),
            Error::VmExitError => write!(f, "VM exit error"),
            Error::InvalidParameter => write!(f, "Invalid parameter"),
            Error::OutOfMemory => write!(f, "Out of memory"),
            Error::NotSupported => write!(f, "Not supported on this hardware"),
            Error::Internal => write!(f, "Internal error"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_error_display() {
        assert_eq!(format!("{}", Error::El2NotAvailable), "EL2 not available");
        assert_eq!(
            format!("{}", Error::Stage2Fault),
            "Stage-2 translation fault"
        );
    }

    #[test]
    fn test_all_error_variants_display() {
        let variants: &[Error] = &[
            Error::AlreadyInitialized,
            Error::El2NotAvailable,
            Error::NoVirtualizationSupport,
            Error::Stage2Fault,
            Error::Stage2AllocationFailed,
            Error::InvalidStage2Mapping,
            Error::OverlappingMapping,
            Error::VgicInitFailed,
            Error::VgicDistributorError,
            Error::VgicRedistributorError,
            Error::InvalidInterruptId,
            Error::VcpuCreateFailed,
            Error::VcpuAlreadyRunning,
            Error::InvalidVcpuState,
            Error::SysregAccessFault,
            Error::UnknownSysreg,
            Error::HvcError,
            Error::SmcTrapped,
            Error::AllocationFailed,
            Error::InvalidGuestState,
            Error::VmExitError,
            Error::InvalidParameter,
            Error::OutOfMemory,
            Error::NotSupported,
            Error::Internal,
        ];
        for err in variants {
            let s = format!("{}", err);
            assert!(!s.is_empty(), "{:?} has empty Display", err);
        }
    }

    #[test]
    fn test_error_equality() {
        assert_eq!(Error::El2NotAvailable, Error::El2NotAvailable);
        assert_ne!(Error::El2NotAvailable, Error::Stage2Fault);
    }

    #[test]
    fn test_error_clone() {
        let e = Error::Stage2Fault;
        let e2 = e;
        assert_eq!(e, e2);
    }
}
