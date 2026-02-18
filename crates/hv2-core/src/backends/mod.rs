//! Hypervisor backend implementations
//!
//! This module contains platform-specific implementations of the
//! `HypervisorBackend` trait for different virtualization technologies.
//!
//! # Available Backends
//!
//! - **KVM** (Linux) - Kernel-based Virtual Machine, uses hardware virtualization
//! - **WHPX** (Windows) - Windows Hypervisor Platform, uses hardware virtualization
//! - **HVF** (macOS) - Hypervisor Framework (future implementation)
//!
//! # Backend Selection
//!
//! Backends are automatically selected based on the platform:
//! ```text
//! Linux   → KVM   (if /dev/kvm exists and is accessible)
//! Windows → WHPX  (if Hyper-V Platform is available)
//! macOS   → HVF   (if Hypervisor.framework is available)
//! Fallback→ TCG   (software emulation, in hypervisor.rs)
//! ```

// Linux KVM backend
#[cfg(target_os = "linux")]
pub mod kvm_ffi;

#[cfg(target_os = "linux")]
pub mod kvm;

#[cfg(target_os = "linux")]
pub use kvm::KvmBackend;

// Windows WHPX backend
#[cfg(target_os = "windows")]
pub mod whpx_ffi;

#[cfg(target_os = "windows")]
pub mod whpx;

#[cfg(target_os = "windows")]
pub use whpx::WhpxBackend;

// macOS HVF backend
#[cfg(target_os = "macos")]
pub mod hvf_ffi;

#[cfg(target_os = "macos")]
pub mod hvf;

#[cfg(target_os = "macos")]
pub use hvf::HvfBackend;
