//! UEFI Boot Support
//!
//! This module provides UEFI firmware interface emulation including
//! system table, boot services, runtime services, and GOP.

pub mod gop;
pub mod runtime_services;
pub mod system_table;
pub mod types;

pub use gop::{
    GopBltOperation, GopBltPixel, GopMode, GopModeInfo, GopPixelBitmask, GopPixelFormat, GopStats,
    GraphicsOutputProtocol,
};
pub use runtime_services::{
    CapsuleCapabilities, ResetType, RuntimeServices, RuntimeServicesStats, Variable,
    VariableAttributes, VariableInfo,
};
pub use system_table::{BootServices, BootServicesStats, SystemTable};
pub use types::*;
