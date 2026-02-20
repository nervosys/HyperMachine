//! Error types for AetherVM

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[non_exhaustive]
#[derive(Error, Debug)]
pub enum Error {
    #[error("VM error: {0}")]
    VM(String),

    #[error("CPU error: {0}")]
    Cpu(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Device error: {0}")]
    Device(String),

    #[error("GPU error: {0}")]
    Gpu(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Hypervisor error: {0}")]
    Hypervisor(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] bincode::Error),

    #[error("Invalid state: {0}")]
    InvalidState(String),

    #[error("Not supported: {0}")]
    NotSupported(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Resource exhausted: {0}")]
    ResourceExhausted(String),

    #[error("Invalid memory access at address {address:#x}")]
    InvalidMemoryAccess { address: u64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_string_variants() {
        assert_eq!(Error::VM("boom".into()).to_string(), "VM error: boom");
        assert_eq!(Error::Cpu("halt".into()).to_string(), "CPU error: halt");
        assert_eq!(
            Error::Memory("oom".into()).to_string(),
            "Memory error: oom"
        );
        assert_eq!(
            Error::Device("lost".into()).to_string(),
            "Device error: lost"
        );
        assert_eq!(Error::Gpu("fail".into()).to_string(), "GPU error: fail");
        assert_eq!(
            Error::Network("down".into()).to_string(),
            "Network error: down"
        );
        assert_eq!(
            Error::Hypervisor("kvm".into()).to_string(),
            "Hypervisor error: kvm"
        );
        assert_eq!(
            Error::Config("bad".into()).to_string(),
            "Configuration error: bad"
        );
        assert_eq!(
            Error::InvalidState("wrong".into()).to_string(),
            "Invalid state: wrong"
        );
        assert_eq!(
            Error::NotSupported("nope".into()).to_string(),
            "Not supported: nope"
        );
        assert_eq!(
            Error::PermissionDenied("root".into()).to_string(),
            "Permission denied: root"
        );
        assert_eq!(
            Error::ResourceExhausted("full".into()).to_string(),
            "Resource exhausted: full"
        );
    }

    #[test]
    fn display_invalid_memory_access() {
        let err = Error::InvalidMemoryAccess { address: 0xDEAD };
        assert_eq!(err.to_string(), "Invalid memory access at address 0xdead");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
        assert!(err.to_string().contains("missing"));
    }

    #[test]
    fn error_is_debug() {
        let err = Error::VM("test".into());
        let debug = format!("{:?}", err);
        assert!(debug.contains("VM"));
    }

    #[test]
    fn implements_std_error() {
        let err = Error::Cpu("x".into());
        let std_err: &dyn std::error::Error = &err;
        assert!(!std_err.to_string().is_empty());
    }
}
