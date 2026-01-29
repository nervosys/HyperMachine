//! CPU emulation and execution

pub mod aarch64;
pub mod x86_64;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CpuError {
    #[error("CPU error: {0}")]
    Execution(String),

    #[error("Unsupported instruction: {0}")]
    UnsupportedInstruction(String),

    #[error("Invalid instruction encoding")]
    InvalidInstruction,

    #[error("Invalid memory access")]
    InvalidMemoryAccess,
}

pub type Result<T> = std::result::Result<T, CpuError>;
