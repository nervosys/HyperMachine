//! Interrupt handling for Type-1 hypervisor
//!
//! This module handles:
//! - IDT setup and interrupt handling
//! - APIC initialization and management
//! - Interrupt virtualization
//! - IPI (Inter-Processor Interrupt) support

use crate::{Error, Result};
use core::sync::atomic::{AtomicBool, Ordering};

/// APIC base address
pub const APIC_BASE: u64 = 0xFEE0_0000;

/// APIC register offsets
pub mod apic_reg {
    pub const ID: u32 = 0x020;
    pub const VERSION: u32 = 0x030;
    pub const TPR: u32 = 0x080;
    pub const APR: u32 = 0x090;
    pub const PPR: u32 = 0x0A0;
    pub const EOI: u32 = 0x0B0;
    pub const RRD: u32 = 0x0C0;
    pub const LDR: u32 = 0x0D0;
    pub const DFR: u32 = 0x0E0;
    pub const SVR: u32 = 0x0F0;
    pub const ISR0: u32 = 0x100;
    pub const TMR0: u32 = 0x180;
    pub const IRR0: u32 = 0x200;
    pub const ESR: u32 = 0x280;
    pub const ICR_LOW: u32 = 0x300;
    pub const ICR_HIGH: u32 = 0x310;
    pub const LVT_TIMER: u32 = 0x320;
    pub const LVT_THERMAL: u32 = 0x330;
    pub const LVT_PERF: u32 = 0x340;
    pub const LVT_LINT0: u32 = 0x350;
    pub const LVT_LINT1: u32 = 0x360;
    pub const LVT_ERROR: u32 = 0x370;
    pub const TIMER_ICR: u32 = 0x380;
    pub const TIMER_CCR: u32 = 0x390;
    pub const TIMER_DCR: u32 = 0x3E0;
}

/// Interrupt vector numbers
pub mod vector {
    pub const DIVIDE_ERROR: u8 = 0;
    pub const DEBUG: u8 = 1;
    pub const NMI: u8 = 2;
    pub const BREAKPOINT: u8 = 3;
    pub const OVERFLOW: u8 = 4;
    pub const BOUND_RANGE: u8 = 5;
    pub const INVALID_OPCODE: u8 = 6;
    pub const DEVICE_NOT_AVAILABLE: u8 = 7;
    pub const DOUBLE_FAULT: u8 = 8;
    pub const INVALID_TSS: u8 = 10;
    pub const SEGMENT_NOT_PRESENT: u8 = 11;
    pub const STACK_FAULT: u8 = 12;
    pub const GENERAL_PROTECTION: u8 = 13;
    pub const PAGE_FAULT: u8 = 14;
    pub const X87_FPU_ERROR: u8 = 16;
    pub const ALIGNMENT_CHECK: u8 = 17;
    pub const MACHINE_CHECK: u8 = 18;
    pub const SIMD_FP_EXCEPTION: u8 = 19;
    pub const VIRTUALIZATION: u8 = 20;
    pub const SECURITY_EXCEPTION: u8 = 30;

    // PIC interrupts (remapped)
    pub const PIC_TIMER: u8 = 32;
    pub const PIC_KEYBOARD: u8 = 33;
    pub const PIC_CASCADE: u8 = 34;
    pub const PIC_COM2: u8 = 35;
    pub const PIC_COM1: u8 = 36;
    pub const PIC_LPT2: u8 = 37;
    pub const PIC_FLOPPY: u8 = 38;
    pub const PIC_LPT1: u8 = 39;
    pub const PIC_RTC: u8 = 40;
    pub const PIC_FREE1: u8 = 41;
    pub const PIC_FREE2: u8 = 42;
    pub const PIC_FREE3: u8 = 43;
    pub const PIC_MOUSE: u8 = 44;
    pub const PIC_FPU: u8 = 45;
    pub const PIC_ATA_PRIMARY: u8 = 46;
    pub const PIC_ATA_SECONDARY: u8 = 47;

    // APIC interrupts
    pub const APIC_TIMER: u8 = 48;
    pub const APIC_ERROR: u8 = 49;
    pub const APIC_SPURIOUS: u8 = 255;

    // IPI vectors
    pub const IPI_RESCHEDULE: u8 = 50;
    pub const IPI_TLB_SHOOTDOWN: u8 = 51;
    pub const IPI_CALL_FUNCTION: u8 = 52;
}

/// Interrupt frame pushed by CPU on interrupt/exception
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Extended interrupt frame with error code
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrameWithError {
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// APIC state
static APIC_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Local APIC interface
pub struct LocalApic {
    base_addr: u64,
}

impl LocalApic {
    /// Create a new Local APIC interface
    pub fn new() -> Self {
        Self {
            base_addr: APIC_BASE,
        }
    }

    /// Create with custom base address
    pub fn with_base(base_addr: u64) -> Self {
        Self { base_addr }
    }

    /// Read an APIC register
    ///
    /// # Safety
    /// Requires that the APIC base address is correctly mapped.
    pub unsafe fn read(&self, offset: u32) -> u32 {
        let addr = (self.base_addr + offset as u64) as *const u32;
        core::ptr::read_volatile(addr)
    }

    /// Write to an APIC register
    ///
    /// # Safety
    /// Requires that the APIC base address is correctly mapped.
    pub unsafe fn write(&self, offset: u32, value: u32) {
        let addr = (self.base_addr + offset as u64) as *mut u32;
        core::ptr::write_volatile(addr, value);
    }

    /// Get the APIC ID
    pub unsafe fn id(&self) -> u8 {
        ((self.read(apic_reg::ID) >> 24) & 0xFF) as u8
    }

    /// Get the APIC version
    pub unsafe fn version(&self) -> u32 {
        self.read(apic_reg::VERSION)
    }

    /// Send End of Interrupt
    pub unsafe fn eoi(&self) {
        self.write(apic_reg::EOI, 0);
    }

    /// Enable the APIC
    pub unsafe fn enable(&self) {
        let svr = self.read(apic_reg::SVR);
        // Set bit 8 (APIC enable) and set spurious vector
        self.write(
            apic_reg::SVR,
            svr | (1 << 8) | (vector::APIC_SPURIOUS as u32),
        );
    }

    /// Disable the APIC
    pub unsafe fn disable(&self) {
        let svr = self.read(apic_reg::SVR);
        self.write(apic_reg::SVR, svr & !(1 << 8));
    }

    /// Send an IPI to another CPU
    pub unsafe fn send_ipi(&self, target_apic_id: u8, vector: u8) {
        // Set target APIC ID in ICR high
        self.write(apic_reg::ICR_HIGH, (target_apic_id as u32) << 24);

        // Send IPI: fixed delivery mode, physical destination
        self.write(apic_reg::ICR_LOW, vector as u32);
    }

    /// Send an IPI to all CPUs (including self)
    pub unsafe fn send_ipi_all(&self, vector: u8) {
        // All including self, fixed delivery mode
        self.write(apic_reg::ICR_HIGH, 0);
        self.write(apic_reg::ICR_LOW, (vector as u32) | (2 << 18));
    }

    /// Send an IPI to all CPUs except self
    pub unsafe fn send_ipi_all_except_self(&self, vector: u8) {
        // All excluding self, fixed delivery mode
        self.write(apic_reg::ICR_HIGH, 0);
        self.write(apic_reg::ICR_LOW, (vector as u32) | (3 << 18));
    }

    /// Configure the APIC timer
    pub unsafe fn configure_timer(&self, vector: u8, divide: u8, periodic: bool) {
        // Set divide configuration
        let dcr = match divide {
            1 => 0xB,
            2 => 0x0,
            4 => 0x1,
            8 => 0x2,
            16 => 0x3,
            32 => 0x8,
            64 => 0x9,
            128 => 0xA,
            _ => 0x0, // Default to divide by 2
        };
        self.write(apic_reg::TIMER_DCR, dcr);

        // Set LVT timer
        let lvt = vector as u32 | if periodic { 1 << 17 } else { 0 };
        self.write(apic_reg::LVT_TIMER, lvt);
    }

    /// Start the APIC timer
    pub unsafe fn start_timer(&self, initial_count: u32) {
        self.write(apic_reg::TIMER_ICR, initial_count);
    }

    /// Stop the APIC timer
    pub unsafe fn stop_timer(&self) {
        self.write(apic_reg::TIMER_ICR, 0);
    }

    /// Get the current timer count
    pub unsafe fn timer_current(&self) -> u32 {
        self.read(apic_reg::TIMER_CCR)
    }
}

impl Default for LocalApic {
    fn default() -> Self {
        Self::new()
    }
}

/// Initialize APIC on the current CPU
pub fn initialize_apic() -> Result<()> {
    // Check if APIC is available
    let cpuid = raw_cpuid::CpuId::new();
    let has_apic = cpuid
        .get_feature_info()
        .map(|f| f.has_apic())
        .unwrap_or(false);

    if !has_apic {
        return Err(Error::NoHardwareSupport);
    }

    // Enable APIC via MSR
    unsafe {
        let apic_base = x86::msr::rdmsr(0x1B);
        // Set bit 11 (global enable)
        x86::msr::wrmsr(0x1B, apic_base | (1 << 11));

        // Enable local APIC
        let apic = LocalApic::new();
        apic.enable();
    }

    APIC_INITIALIZED.store(true, Ordering::SeqCst);
    Ok(())
}

/// Check if APIC is initialized
pub fn is_apic_initialized() -> bool {
    APIC_INITIALIZED.load(Ordering::SeqCst)
}

/// Virtual APIC for guest
#[derive(Default)]
pub struct VirtualApic {
    /// APIC ID
    pub id: u8,
    /// TPR (Task Priority Register)
    pub tpr: u8,
    /// PPR (Processor Priority Register)
    pub ppr: u8,
    /// ISR (In-Service Register)
    pub isr: [u32; 8],
    /// IRR (Interrupt Request Register)
    pub irr: [u32; 8],
    /// Timer initial count
    pub timer_icr: u32,
    /// Timer current count
    pub timer_ccr: u32,
    /// Timer divider
    pub timer_divider: u32,
    /// LVT Timer
    pub lvt_timer: u32,
}

impl VirtualApic {
    /// Create a new virtual APIC
    pub fn new(id: u8) -> Self {
        Self {
            id,
            ..Default::default()
        }
    }

    /// Check if there's a pending interrupt
    pub fn has_pending_interrupt(&self) -> bool {
        for i in 0..8 {
            if self.irr[i] != 0 {
                return true;
            }
        }
        false
    }

    /// Get the highest priority pending interrupt
    pub fn get_pending_interrupt(&self) -> Option<u8> {
        for i in (0..8).rev() {
            if self.irr[i] != 0 {
                let bit = 31 - self.irr[i].leading_zeros();
                return Some((i as u8 * 32) + bit as u8);
            }
        }
        None
    }

    /// Set a pending interrupt
    pub fn set_irr(&mut self, vector: u8) {
        let idx = (vector / 32) as usize;
        let bit = vector % 32;
        self.irr[idx] |= 1 << bit;
    }

    /// Clear a pending interrupt
    pub fn clear_irr(&mut self, vector: u8) {
        let idx = (vector / 32) as usize;
        let bit = vector % 32;
        self.irr[idx] &= !(1 << bit);
    }

    /// Mark interrupt as in-service
    pub fn set_isr(&mut self, vector: u8) {
        let idx = (vector / 32) as usize;
        let bit = vector % 32;
        self.isr[idx] |= 1 << bit;
        self.clear_irr(vector);
    }

    /// Clear in-service interrupt (EOI)
    pub fn eoi(&mut self) {
        // Find highest priority in-service interrupt and clear it
        for i in (0..8).rev() {
            if self.isr[i] != 0 {
                let bit = 31 - self.isr[i].leading_zeros();
                self.isr[i] &= !(1 << bit);
                break;
            }
        }
    }
}
