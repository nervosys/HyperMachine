//! HyperMachine Type-1 ARM64 EL2 Hypervisor Backend
//!
//! This crate implements an ARM64 EL2 (Exception Level 2) hypervisor backend
//! for the HyperMachine Type-1 bare-metal hypervisor. It runs directly on
//! ARMv8-A hardware using the Virtualization Host Extension (VHE) and
//! hardware-assisted stage-2 address translation.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                      Guest VMs (EL1/EL0)                           │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │                    HyperMachine HV1-ARM (EL2)                      │
//! │           vCPU  │  Stage-2 MMU  │  vGIC  │  System Regs           │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │               ARMv8-A Hardware (EL3/Secure)                        │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Maturity
//!
//! **Trap handling is a skeleton, and this crate has never run on ARM
//! hardware.** The feature list below describes the intended shape; what
//! `VmContext::handle_exit` does today is narrower, and a reader taking the
//! bullets at face value would over-estimate it:
//!
//! * A **data abort** or **instruction abort** returns "resume" without
//!   emulating anything and without advancing the PC. There is no MMIO
//!   dispatch in this crate at all. A guest that faults would re-execute the
//!   faulting instruction and fault again, indefinitely.
//! * An unhandled system-register access is commented "inject undefined
//!   exception into guest"; no injection is performed.
//! * `HVC` and `SMC` advance the PC and are otherwise ignored, so a guest's
//!   PSCI call (`CPU_ON`, `SYSTEM_OFF`) returns with its result registers
//!   untouched while the guest believes the call completed.
//!
//! Nothing outside this crate's own tests calls `handle_exit`. The stage-2
//! translation tables, vGIC state and system-register decoding below are
//! real and unit-tested; it is the exit path that is not finished.
//!
//! # Features
//!
//! - **EL2 Exception Handling**: Trap and emulate guest EL1 operations
//! - **Stage-2 Translation**: Hardware-enforced guest physical-to-host physical mapping
//! - **vGIC**: Virtual Generic Interrupt Controller (GICv2/GICv3)
//! - **System Register Trapping**: Intercept and emulate EL1 system register access
//! - **vCPU Management**: AArch64 guest state save/restore
//!
//! # Exception Levels
//!
//! | Level | Purpose                          |
//! |-------|----------------------------------|
//! | EL0   | Guest userspace                  |
//! | EL1   | Guest kernel                     |
//! | EL2   | Hypervisor (this crate)          |
//! | EL3   | Secure monitor (firmware)        |

#![no_std]
#![allow(dead_code, unused_variables, unused_imports)]

extern crate alloc;

pub mod el2;
pub mod error;
pub mod stage2;
pub mod sysreg;
pub mod vcpu;
pub mod vgic;
pub mod vm;

pub use error::{Error, Result};
