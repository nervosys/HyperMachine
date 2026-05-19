//! GPU virtualization for HyperMachine

#![allow(dead_code)]

pub mod passthrough;

#[cfg(feature = "wgpu-backend")]
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

    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("I/O error: {0}")]
    IoError(String),
}

pub type Result<T> = std::result::Result<T, GpuError>;
