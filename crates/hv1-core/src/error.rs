//! Error types for HV1 hypervisor

use core::fmt;

/// HV1 Result type
pub type Result<T> = core::result::Result<T, Error>;

/// HV1 Error types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Hypervisor already initialized
    AlreadyInitialized,
    /// No hardware virtualization support
    NoHardwareSupport,
    /// Unsupported CPU vendor
    UnsupportedCpu,
    /// VMX initialization failed
    VmxInitFailed,
    /// SVM initialization failed  
    SvmInitFailed,
    /// VMXON failed
    VmxonFailed,
    /// VMCLEAR failed
    VmclearFailed,
    /// VMPTRLD failed
    VmptrldFailed,
    /// VMWRITE failed
    VmwriteFailed,
    /// VMREAD failed
    VmreadFailed,
    /// VMLAUNCH failed
    VmlaunchFailed,
    /// VMRESUME failed
    VmresumeFailed,
    /// Invalid VMCS field
    InvalidVmcsField,
    /// Invalid VMCB field
    InvalidVmcbField,
    /// Memory allocation failed
    AllocationFailed,
    /// Page table error
    PageTableError,
    /// EPT violation
    EptViolation,
    /// NPT violation
    NptViolation,
    /// Invalid guest state
    InvalidGuestState,
    /// VM exit error
    VmExitError,
    /// Interrupt injection failed
    InterruptInjectionFailed,
    /// Device not found
    DeviceNotFound,
    /// Device error
    DeviceError,
    /// I/O error
    IoError,
    /// Out of memory
    OutOfMemory,
    /// Invalid parameter
    InvalidParameter,
    /// Not supported
    NotSupported,
    /// Internal error
    Internal,
    /// VMRUN failed (AMD SVM)
    VmrunFailed,
    /// Invalid state transition
    InvalidState,
    /// Invalid configuration
    InvalidConfiguration,
    /// Unsupported operation
    UnsupportedOperation,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::AlreadyInitialized => write!(f, "Hypervisor already initialized"),
            Error::NoHardwareSupport => write!(f, "No hardware virtualization support"),
            Error::UnsupportedCpu => write!(f, "Unsupported CPU vendor"),
            Error::VmxInitFailed => write!(f, "VMX initialization failed"),
            Error::SvmInitFailed => write!(f, "SVM initialization failed"),
            Error::VmxonFailed => write!(f, "VMXON failed"),
            Error::VmclearFailed => write!(f, "VMCLEAR failed"),
            Error::VmptrldFailed => write!(f, "VMPTRLD failed"),
            Error::VmwriteFailed => write!(f, "VMWRITE failed"),
            Error::VmreadFailed => write!(f, "VMREAD failed"),
            Error::VmlaunchFailed => write!(f, "VMLAUNCH failed"),
            Error::VmresumeFailed => write!(f, "VMRESUME failed"),
            Error::InvalidVmcsField => write!(f, "Invalid VMCS field"),
            Error::InvalidVmcbField => write!(f, "Invalid VMCB field"),
            Error::AllocationFailed => write!(f, "Memory allocation failed"),
            Error::PageTableError => write!(f, "Page table error"),
            Error::EptViolation => write!(f, "EPT violation"),
            Error::NptViolation => write!(f, "NPT violation"),
            Error::InvalidGuestState => write!(f, "Invalid guest state"),
            Error::VmExitError => write!(f, "VM exit error"),
            Error::InterruptInjectionFailed => write!(f, "Interrupt injection failed"),
            Error::DeviceNotFound => write!(f, "Device not found"),
            Error::DeviceError => write!(f, "Device error"),
            Error::IoError => write!(f, "I/O error"),
            Error::OutOfMemory => write!(f, "Out of memory"),
            Error::InvalidParameter => write!(f, "Invalid parameter"),
            Error::NotSupported => write!(f, "Not supported"),
            Error::Internal => write!(f, "Internal error"),
            Error::VmrunFailed => write!(f, "VMRUN failed"),
            Error::InvalidState => write!(f, "Invalid state"),
            Error::InvalidConfiguration => write!(f, "Invalid configuration"),
            Error::UnsupportedOperation => write!(f, "Unsupported operation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;

    #[test]
    fn test_error_display() {
        assert_eq!(
            format!("{}", Error::NoHardwareSupport),
            "No hardware virtualization support"
        );
        assert_eq!(format!("{}", Error::VmxonFailed), "VMXON failed");
    }

    #[test]
    fn test_all_error_variants_display() {
        // Ensure every variant produces a non-empty Display string
        let variants: &[Error] = &[
            Error::AlreadyInitialized,
            Error::NoHardwareSupport,
            Error::UnsupportedCpu,
            Error::VmxInitFailed,
            Error::SvmInitFailed,
            Error::VmxonFailed,
            Error::VmclearFailed,
            Error::VmptrldFailed,
            Error::VmwriteFailed,
            Error::VmreadFailed,
            Error::VmlaunchFailed,
            Error::VmresumeFailed,
            Error::InvalidVmcsField,
            Error::InvalidVmcbField,
            Error::AllocationFailed,
            Error::PageTableError,
            Error::EptViolation,
            Error::NptViolation,
            Error::InvalidGuestState,
            Error::VmExitError,
            Error::InterruptInjectionFailed,
            Error::DeviceNotFound,
            Error::DeviceError,
            Error::IoError,
            Error::OutOfMemory,
            Error::InvalidParameter,
            Error::NotSupported,
            Error::Internal,
            Error::VmrunFailed,
            Error::InvalidState,
            Error::InvalidConfiguration,
            Error::UnsupportedOperation,
        ];
        for err in variants {
            let s = format!("{}", err);
            assert!(!s.is_empty(), "{:?} has empty Display", err);
        }
    }

    #[test]
    fn test_error_equality() {
        assert_eq!(Error::OutOfMemory, Error::OutOfMemory);
        assert_ne!(Error::OutOfMemory, Error::IoError);
    }

    #[test]
    fn test_error_debug() {
        let dbg = format!("{:?}", Error::VmxonFailed);
        assert!(dbg.contains("VmxonFailed"));
    }

    #[test]
    fn test_error_clone() {
        let e = Error::PageTableError;
        let e2 = e;
        assert_eq!(e, e2);
    }
}
