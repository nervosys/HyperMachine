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

// macOS HVF backend.
//
// x86_64 only. This backend is built on Hypervisor.framework's VMX surface --
// hv_vmx_vcpu_read_vmcs / hv_vmx_vcpu_write_vmcs / hv_vcpu_read_register /
// hv_vcpu_write_register / hv_vcpu_interrupt -- none of which exist on Apple
// Silicon, where the framework exposes a different ARM API entirely. Gated only
// on target_os, it compiled on aarch64-apple-darwin (cargo check does not link)
// and then failed at link time with "symbol(s) not found for architecture
// arm64". Selecting a backend on Apple Silicon now falls through to TCG.
#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub mod hvf_ffi;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub mod hvf;

#[cfg(all(target_os = "macos", target_arch = "x86_64"))]
pub use hvf::HvfBackend;
