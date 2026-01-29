//! UEFI Boot Support
//!
//! This module provides UEFI firmware interface emulation including
//! system table, boot services, runtime services, and GOP.

pub mod types;
pub mod system_table;
pub mod gop;
pub mod runtime_services;

pub use types::*;
pub use system_table::{BootServices, SystemTable, BootServicesStats};
pub use gop::{
    GraphicsOutputProtocol, GopPixelFormat, GopPixelBitmask, GopModeInfo, 
    GopMode, GopBltPixel, GopBltOperation, GopStats,
};
pub use runtime_services::{
    RuntimeServices, ResetType, VariableAttributes, Variable, 
    VariableInfo, CapsuleCapabilities, RuntimeServicesStats,
};
