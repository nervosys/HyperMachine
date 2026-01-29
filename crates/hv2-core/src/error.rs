//! Error types for AetherVM

use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

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
