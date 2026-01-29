//! GPU virtualization for HV2

pub mod passthrough;
pub mod vgpu;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum GpuError {
    #[error("GPU not available: {0}")]
    NotAvailable(String),

    #[error("GPU initialization failed: {0}")]
    InitFailed(String),

    #[error("Unsupported operation: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, GpuError>;
