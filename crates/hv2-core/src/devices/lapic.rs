//! Local Advanced Programmable Interrupt Controller (LAPIC)
//!
//! This module implements the Local APIC found in each x86 processor.
//! The LAPIC handles interrupt delivery to the CPU and inter-processor interrupts.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         CPU Core                                 │
//! │  ┌────────────────────────────────────────────────────────────┐ │
//! │  │                      Local APIC                             │ │
//! │  │  ┌──────────────────┐  ┌──────────────────────────────┐   │ │
//! │  │  │  Timer           │  │  Interrupt Vectors            │   │ │
//! │  │  │  - One-shot      │  │  IRR (256 bits)              │   │ │
//! │  │  │  - Periodic      │  │  ISR (256 bits)              │   │ │
//! │  │  │  - TSC-deadline  │  │  TMR (256 bits)              │   │ │
//! │  │  └──────────────────┘  └──────────────────────────────┘   │ │
//! │  │  ┌──────────────────┐  ┌──────────────────────────────┐   │ │
//! │  │  │  LVT Entries     │  │  Error Status                │   │ │
//! │  │  │  - Timer         │  │  - Send Checksum Error       │   │ │
//! │  │  │  - LINT0/LINT1   │  │  - Receive Checksum Error    │   │ │
//! │  │  │  - Error         │  │  - Send Accept Error         │   │ │
//! │  │  │  - CMCI          │  │  - Receive Accept Error      │   │ │
//! │  │  │  - Perf/Thermal  │  │  - Redirectable IPI          │   │ │
//! │  │  └──────────────────┘  └──────────────────────────────┘   │ │
//! │  │  ┌──────────────────┐  ┌──────────────────────────────┐   │ │
//! │  │  │  ICR (64-bit)    │  │  TPR, PPR, EOI               │   │ │
//! │  │  │  IPI sending     │  │  Priority management         │   │ │
//! │  │  └──────────────────┘  └──────────────────────────────┘   │ │
//! │  └────────────────────────────────────────────────────────────┘ │
//! │                            │                                     │
//! │                            ▼                                     │
//! │                    System Bus (IPI)                              │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # LAPIC Registers (MMIO at 0xFEE00000)
//!
//! | Offset | Register           | Description                      |
//! |--------|--------------------|----------------------------------|
//! | 0x020  | ID                 | Local APIC ID                    |
//! | 0x030  | Version            | Local APIC Version               |
//! | 0x080  | TPR                | Task Priority Register           |
//! | 0x090  | APR                | Arbitration Priority Register    |
//! | 0x0A0  | PPR                | Processor Priority Register      |
//! | 0x0B0  | EOI                | End of Interrupt                 |
//! | 0x0D0  | LDR                | Logical Destination Register     |
//! | 0x0E0  | DFR                | Destination Format Register      |
//! | 0x0F0  | SVR                | Spurious Interrupt Vector        |
//! | 0x100  | ISR 0-7            | In-Service Register              |
//! | 0x180  | TMR 0-7            | Trigger Mode Register            |
//! | 0x200  | IRR 0-7            | Interrupt Request Register       |
//! | 0x280  | ESR                | Error Status Register            |
//! | 0x2F0  | LVT CMCI           | Corrected Machine Check          |
//! | 0x300  | ICR Low            | Interrupt Command (low)          |
//! | 0x310  | ICR High           | Interrupt Command (high)         |
//! | 0x320  | LVT Timer          | Timer Local Vector Table         |
//! | 0x330  | LVT Thermal        | Thermal Local Vector Table       |
//! | 0x340  | LVT PMC            | Performance Counter LVT          |
//! | 0x350  | LVT LINT0          | Local Interrupt 0                |
//! | 0x360  | LVT LINT1          | Local Interrupt 1                |
//! | 0x370  | LVT Error          | Error Local Vector Table         |
//! | 0x380  | Timer ICR          | Timer Initial Count              |
//! | 0x390  | Timer CCR          | Timer Current Count              |
//! | 0x3E0  | Timer DCR          | Timer Divide Configuration       |

use crate::Result;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parking_lot::RwLock;
#[cfg(test)]
use std::sync::Arc;

/// LAPIC base address (standard location)
pub const LAPIC_BASE: u64 = 0xFEE0_0000;

/// LAPIC MMIO region size
pub const LAPIC_SIZE: u64 = 0x400;

/// LAPIC register offsets
pub mod regs {
    /// Local APIC ID
    pub const ID: u64 = 0x020;
    /// Local APIC Version
    pub const VERSION: u64 = 0x030;
    /// Task Priority Register
    pub const TPR: u64 = 0x080;
    /// Arbitration Priority Register (read-only)
    pub const APR: u64 = 0x090;
    /// Processor Priority Register (read-only)
    pub const PPR: u64 = 0x0A0;
    /// End of Interrupt
    pub const EOI: u64 = 0x0B0;
    /// Remote Read Register
    pub const RRD: u64 = 0x0C0;
    /// Logical Destination Register
    pub const LDR: u64 = 0x0D0;
    /// Destination Format Register
    pub const DFR: u64 = 0x0E0;
    /// Spurious Interrupt Vector Register
    pub const SVR: u64 = 0x0F0;
    /// In-Service Register (8 x 32-bit)
    pub const ISR_BASE: u64 = 0x100;
    /// Trigger Mode Register (8 x 32-bit)
    pub const TMR_BASE: u64 = 0x180;
    /// Interrupt Request Register (8 x 32-bit)
    pub const IRR_BASE: u64 = 0x200;
    /// Error Status Register
    pub const ESR: u64 = 0x280;
    /// LVT Corrected Machine Check
    pub const LVT_CMCI: u64 = 0x2F0;
    /// Interrupt Command Register (low)
    pub const ICR_LOW: u64 = 0x300;
    /// Interrupt Command Register (high)
    pub const ICR_HIGH: u64 = 0x310;
    /// LVT Timer
    pub const LVT_TIMER: u64 = 0x320;
    /// LVT Thermal Sensor
    pub const LVT_THERMAL: u64 = 0x330;
    /// LVT Performance Monitoring
    pub const LVT_PMC: u64 = 0x340;
    /// LVT LINT0
    pub const LVT_LINT0: u64 = 0x350;
    /// LVT LINT1
    pub const LVT_LINT1: u64 = 0x360;
    /// LVT Error
    pub const LVT_ERROR: u64 = 0x370;
    /// Timer Initial Count
    pub const TIMER_ICR: u64 = 0x380;
    /// Timer Current Count
    pub const TIMER_CCR: u64 = 0x390;
    /// Timer Divide Configuration
    pub const TIMER_DCR: u64 = 0x3E0;
}

/// SVR register bits
pub mod svr {
    /// Spurious vector (bits 0-7)
    pub const VECTOR_MASK: u32 = 0xFF;
    /// APIC Software Enable (bit 8)
    pub const APIC_ENABLE: u32 = 1 << 8;
    /// Focus Processor Checking (bit 9)
    pub const FOCUS_CHECK: u32 = 1 << 9;
    /// EOI Broadcast Suppression (bit 12)
    pub const EOI_BROADCAST_SUPPRESS: u32 = 1 << 12;
}

/// LVT entry bits
pub mod lvt {
    /// Vector (bits 0-7)
    pub const VECTOR_MASK: u32 = 0xFF;
    /// Delivery Mode (bits 8-10)
    pub const DELMODE_MASK: u32 = 0x700;
    pub const DELMODE_SHIFT: u32 = 8;
    /// Delivery Status (bit 12) - read only
    pub const DELIVS: u32 = 1 << 12;
    /// Interrupt Polarity (bit 13) - LINT only
    pub const INTPOL: u32 = 1 << 13;
    /// Remote IRR (bit 14) - read only, LINT only
    pub const REMOTE_IRR: u32 = 1 << 14;
    /// Trigger Mode (bit 15) - LINT only
    pub const TRIGGER: u32 = 1 << 15;
    /// Mask (bit 16)
    pub const MASKED: u32 = 1 << 16;
    /// Timer Mode (bits 17-18) - Timer only
    pub const TIMER_MODE_MASK: u32 = 0x3 << 17;
    pub const TIMER_MODE_SHIFT: u32 = 17;
}

/// Timer modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TimerMode {
    /// One-shot: counts down once
    OneShot = 0,
    /// Periodic: reloads after reaching zero
    Periodic = 1,
    /// TSC-deadline mode
    TscDeadline = 2,
    /// Reserved
    Reserved = 3,
}

impl TimerMode {
    /// Create from bits
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x3 {
            0 => Self::OneShot,
            1 => Self::Periodic,
            2 => Self::TscDeadline,
            _ => Self::Reserved,
        }
    }
}

/// ICR delivery modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IcrDeliveryMode {
    /// Fixed: deliver interrupt
    Fixed = 0,
    /// Lowest Priority
    LowestPriority = 1,
    /// SMI
    Smi = 2,
    /// Reserved
    Reserved = 3,
    /// NMI
    Nmi = 4,
    /// INIT
    Init = 5,
    /// SIPI (Startup IPI)
    Sipi = 6,
    /// ExtINT
    ExtInt = 7,
}

impl IcrDeliveryMode {
    /// Create from bits
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x7 {
            0 => Self::Fixed,
            1 => Self::LowestPriority,
            2 => Self::Smi,
            3 => Self::Reserved,
            4 => Self::Nmi,
            5 => Self::Init,
            6 => Self::Sipi,
            _ => Self::ExtInt,
        }
    }
}

/// ICR destination shorthand
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IcrDestShorthand {
    /// No shorthand - use destination field
    None = 0,
    /// Self only
    SelfOnly = 1,
    /// All including self
    AllIncludingSelf = 2,
    /// All excluding self
    AllExcludingSelf = 3,
}

impl IcrDestShorthand {
    /// Create from bits
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0x3 {
            0 => Self::None,
            1 => Self::SelfOnly,
            2 => Self::AllIncludingSelf,
            _ => Self::AllExcludingSelf,
        }
    }
}

/// ICR register bits
pub mod icr {
    /// Vector (bits 0-7)
    pub const VECTOR_MASK: u64 = 0xFF;
    /// Delivery Mode (bits 8-10)
    pub const DELMODE_MASK: u64 = 0x700;
    pub const DELMODE_SHIFT: u64 = 8;
    /// Destination Mode (bit 11): 0=physical, 1=logical
    pub const DESTMODE: u64 = 1 << 11;
    /// Delivery Status (bit 12) - read only
    pub const DELIVS: u64 = 1 << 12;
    /// Level (bit 14): 0=de-assert, 1=assert
    pub const LEVEL: u64 = 1 << 14;
    /// Trigger Mode (bit 15): 0=edge, 1=level
    pub const TRIGGER: u64 = 1 << 15;
    /// Destination Shorthand (bits 18-19)
    pub const DESTSHORT_MASK: u64 = 0x3 << 18;
    pub const DESTSHORT_SHIFT: u64 = 18;
    /// Destination (bits 56-63)
    pub const DEST_MASK: u64 = 0xFF << 56;
    pub const DEST_SHIFT: u64 = 56;
}

/// Error status register bits
pub mod esr {
    /// Send Checksum Error
    pub const SEND_CHECKSUM: u32 = 1 << 0;
    /// Receive Checksum Error
    pub const RECV_CHECKSUM: u32 = 1 << 1;
    /// Send Accept Error
    pub const SEND_ACCEPT: u32 = 1 << 2;
    /// Receive Accept Error
    pub const RECV_ACCEPT: u32 = 1 << 3;
    /// Redirectable IPI
    pub const REDIRECT_IPI: u32 = 1 << 4;
    /// Send Illegal Vector
    pub const SEND_ILLEGAL: u32 = 1 << 5;
    /// Receive Illegal Vector
    pub const RECV_ILLEGAL: u32 = 1 << 6;
    /// Illegal Register Address
    pub const ILLEGAL_ADDR: u32 = 1 << 7;
}

/// Timer divide configuration values
pub mod timer_div {
    pub const DIV_1: u32 = 0b1011;
    pub const DIV_2: u32 = 0b0000;
    pub const DIV_4: u32 = 0b0001;
    pub const DIV_8: u32 = 0b0010;
    pub const DIV_16: u32 = 0b0011;
    pub const DIV_32: u32 = 0b1000;
    pub const DIV_64: u32 = 0b1001;
    pub const DIV_128: u32 = 0b1010;

    /// Get divisor from DCR value
    pub fn get_divisor(dcr: u32) -> u32 {
        let bits = ((dcr & 0x8) >> 1) | (dcr & 0x3);
        match bits {
            0b000 => 2,
            0b001 => 4,
            0b010 => 8,
            0b011 => 16,
            0b100 => 32,
            0b101 => 64,
            0b110 => 128,
            0b111 => 1,
            _ => 1,
        }
    }
}

/// LAPIC interrupt callback for IPI delivery
pub type LapicIpiCallback = Box<dyn Fn(u8, u8, IcrDeliveryMode) + Send + Sync>;

/// LAPIC EOI callback (for IOAPIC)
pub type LapicEoiCallback = Box<dyn Fn(u8) + Send + Sync>;

/// Local APIC state
pub struct LocalApic {
    /// APIC ID
    id: u8,
    /// Enabled via SVR
    enabled: AtomicBool,
    /// Task Priority Register
    tpr: AtomicU32,
    /// Logical Destination Register
    ldr: AtomicU32,
    /// Destination Format Register
    dfr: AtomicU32,
    /// Spurious Vector Register
    svr: AtomicU32,
    /// Error Status Register
    esr: AtomicU32,
    /// Interrupt Command Register
    icr: AtomicU64,
    /// LVT Timer
    lvt_timer: AtomicU32,
    /// LVT Thermal
    lvt_thermal: AtomicU32,
    /// LVT Performance
    lvt_pmc: AtomicU32,
    /// LVT LINT0
    lvt_lint0: AtomicU32,
    /// LVT LINT1
    lvt_lint1: AtomicU32,
    /// LVT Error
    lvt_error: AtomicU32,
    /// LVT CMCI
    lvt_cmci: AtomicU32,
    /// Timer Initial Count
    timer_icr: AtomicU32,
    /// Timer Current Count
    timer_ccr: AtomicU32,
    /// Timer Divide Configuration
    timer_dcr: AtomicU32,
    /// In-Service Register (256 bits)
    isr: RwLock<[u32; 8]>,
    /// Trigger Mode Register (256 bits)
    tmr: RwLock<[u32; 8]>,
    /// Interrupt Request Register (256 bits)
    irr: RwLock<[u32; 8]>,
    /// IPI callback
    ipi_callback: RwLock<Option<LapicIpiCallback>>,
    /// EOI callback (for IOAPIC)
    eoi_callback: RwLock<Option<LapicEoiCallback>>,
}

impl LocalApic {
    /// Create a new Local APIC
    pub fn new(id: u8) -> Self {
        Self {
            id,
            enabled: AtomicBool::new(false),
            tpr: AtomicU32::new(0),
            ldr: AtomicU32::new(0),
            dfr: AtomicU32::new(0xFFFF_FFFF), // Flat model by default
            svr: AtomicU32::new(0xFF),        // Vector 0xFF, disabled
            esr: AtomicU32::new(0),
            icr: AtomicU64::new(0),
            lvt_timer: AtomicU32::new(lvt::MASKED),
            lvt_thermal: AtomicU32::new(lvt::MASKED),
            lvt_pmc: AtomicU32::new(lvt::MASKED),
            lvt_lint0: AtomicU32::new(lvt::MASKED),
            lvt_lint1: AtomicU32::new(lvt::MASKED),
            lvt_error: AtomicU32::new(lvt::MASKED),
            lvt_cmci: AtomicU32::new(lvt::MASKED),
            timer_icr: AtomicU32::new(0),
            timer_ccr: AtomicU32::new(0),
            timer_dcr: AtomicU32::new(0),
            isr: RwLock::new([0; 8]),
            tmr: RwLock::new([0; 8]),
            irr: RwLock::new([0; 8]),
            ipi_callback: RwLock::new(None),
            eoi_callback: RwLock::new(None),
        }
    }

    /// Set IPI callback
    pub fn set_ipi_callback<F>(&self, callback: F)
    where
        F: Fn(u8, u8, IcrDeliveryMode) + Send + Sync + 'static,
    {
        *self.ipi_callback.write() = Some(Box::new(callback));
    }

    /// Set EOI callback
    pub fn set_eoi_callback<F>(&self, callback: F)
    where
        F: Fn(u8) + Send + Sync + 'static,
    {
        *self.eoi_callback.write() = Some(Box::new(callback));
    }

    /// Get the APIC ID
    pub fn id(&self) -> u8 {
        self.id
    }

    /// Check if APIC is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Read from MMIO address
    pub fn read(&self, offset: u64) -> Result<u32> {
        match offset {
            regs::ID => Ok((self.id as u32) << 24),
            regs::VERSION => {
                // Version 0x14 (P6 family), max LVT entry 5
                Ok(0x14 | (5 << 16))
            }
            regs::TPR => Ok(self.tpr.load(Ordering::Relaxed)),
            regs::APR => {
                // Arbitration Priority = max(TPR[7:4], highest ISR priority)
                Ok(self.tpr.load(Ordering::Relaxed) & 0xF0)
            }
            regs::PPR => {
                // Processor Priority = max(TPR[7:4], ISR priority)
                let tpr = self.tpr.load(Ordering::Relaxed);
                let isr_prio = self.get_highest_isr_priority() as u32;
                Ok(std::cmp::max(tpr & 0xF0, isr_prio << 4))
            }
            regs::LDR => Ok(self.ldr.load(Ordering::Relaxed)),
            regs::DFR => Ok(self.dfr.load(Ordering::Relaxed)),
            regs::SVR => Ok(self.svr.load(Ordering::Relaxed)),
            regs::ESR => Ok(self.esr.load(Ordering::Relaxed)),
            regs::ICR_LOW => Ok(self.icr.load(Ordering::Relaxed) as u32),
            regs::ICR_HIGH => Ok((self.icr.load(Ordering::Relaxed) >> 32) as u32),
            regs::LVT_TIMER => Ok(self.lvt_timer.load(Ordering::Relaxed)),
            regs::LVT_THERMAL => Ok(self.lvt_thermal.load(Ordering::Relaxed)),
            regs::LVT_PMC => Ok(self.lvt_pmc.load(Ordering::Relaxed)),
            regs::LVT_LINT0 => Ok(self.lvt_lint0.load(Ordering::Relaxed)),
            regs::LVT_LINT1 => Ok(self.lvt_lint1.load(Ordering::Relaxed)),
            regs::LVT_ERROR => Ok(self.lvt_error.load(Ordering::Relaxed)),
            regs::LVT_CMCI => Ok(self.lvt_cmci.load(Ordering::Relaxed)),
            regs::TIMER_ICR => Ok(self.timer_icr.load(Ordering::Relaxed)),
            regs::TIMER_CCR => Ok(self.timer_ccr.load(Ordering::Relaxed)),
            regs::TIMER_DCR => Ok(self.timer_dcr.load(Ordering::Relaxed)),
            _ => {
                // ISR, TMR, IRR banks
                if (regs::ISR_BASE..regs::ISR_BASE + 0x80).contains(&offset) {
                    let idx = ((offset - regs::ISR_BASE) / 0x10) as usize;
                    if idx < 8 {
                        let isr = self.isr.read();
                        return Ok(isr[idx]);
                    }
                }
                if (regs::TMR_BASE..regs::TMR_BASE + 0x80).contains(&offset) {
                    let idx = ((offset - regs::TMR_BASE) / 0x10) as usize;
                    if idx < 8 {
                        let tmr = self.tmr.read();
                        return Ok(tmr[idx]);
                    }
                }
                if (regs::IRR_BASE..regs::IRR_BASE + 0x80).contains(&offset) {
                    let idx = ((offset - regs::IRR_BASE) / 0x10) as usize;
                    if idx < 8 {
                        let irr = self.irr.read();
                        return Ok(irr[idx]);
                    }
                }
                Ok(0)
            }
        }
    }

    /// Write to MMIO address
    pub fn write(&self, offset: u64, value: u32) -> Result<()> {
        match offset {
            regs::ID => {
                // APIC ID is typically read-only
            }
            regs::TPR => {
                self.tpr.store(value & 0xFF, Ordering::Relaxed);
            }
            regs::EOI => {
                self.handle_eoi();
            }
            regs::LDR => {
                // Only bits 24-31 are writable
                self.ldr.store(value & 0xFF00_0000, Ordering::Relaxed);
            }
            regs::DFR => {
                // Only bits 28-31 are writable
                self.dfr.store(value | 0x0FFF_FFFF, Ordering::Relaxed);
            }
            regs::SVR => {
                let old_enabled = self.svr.load(Ordering::Relaxed) & svr::APIC_ENABLE != 0;
                self.svr.store(value, Ordering::Relaxed);
                let new_enabled = value & svr::APIC_ENABLE != 0;
                self.enabled.store(new_enabled, Ordering::Relaxed);

                if !old_enabled && new_enabled {
                    // APIC just enabled - could trigger pending interrupts
                }
            }
            regs::ESR => {
                // Writing clears ESR (value ignored)
                self.esr.store(0, Ordering::Relaxed);
            }
            regs::ICR_LOW => {
                let high = (self.icr.load(Ordering::Relaxed) >> 32) as u32;
                let new_icr = ((high as u64) << 32) | (value as u64);
                self.icr.store(new_icr, Ordering::Relaxed);
                self.handle_icr_write(new_icr);
            }
            regs::ICR_HIGH => {
                let low = self.icr.load(Ordering::Relaxed) as u32;
                let new_icr = ((value as u64) << 32) | (low as u64);
                self.icr.store(new_icr, Ordering::Relaxed);
            }
            regs::LVT_TIMER => {
                self.lvt_timer.store(value, Ordering::Relaxed);
            }
            regs::LVT_THERMAL => {
                self.lvt_thermal.store(value, Ordering::Relaxed);
            }
            regs::LVT_PMC => {
                self.lvt_pmc.store(value, Ordering::Relaxed);
            }
            regs::LVT_LINT0 => {
                self.lvt_lint0.store(value, Ordering::Relaxed);
            }
            regs::LVT_LINT1 => {
                self.lvt_lint1.store(value, Ordering::Relaxed);
            }
            regs::LVT_ERROR => {
                self.lvt_error.store(value, Ordering::Relaxed);
            }
            regs::LVT_CMCI => {
                self.lvt_cmci.store(value, Ordering::Relaxed);
            }
            regs::TIMER_ICR => {
                self.timer_icr.store(value, Ordering::Relaxed);
                // Reset current count to initial count
                self.timer_ccr.store(value, Ordering::Relaxed);
            }
            regs::TIMER_DCR => {
                self.timer_dcr.store(value & 0xB, Ordering::Relaxed);
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle EOI write
    fn handle_eoi(&self) {
        // Find highest priority in-service interrupt
        let vector = {
            let isr = self.isr.read();
            self.find_highest_set_bit(&isr)
        };

        if let Some(vec) = vector {
            // Clear ISR bit
            {
                let mut isr = self.isr.write();
                let idx = (vec / 32) as usize;
                let bit = vec % 32;
                isr[idx] &= !(1 << bit);
            }

            // Check TMR for level-triggered
            let is_level = {
                let tmr = self.tmr.read();
                let idx = (vec / 32) as usize;
                let bit = vec % 32;
                tmr[idx] & (1 << bit) != 0
            };

            if is_level {
                // Notify IOAPIC
                if let Some(ref cb) = *self.eoi_callback.read() {
                    cb(vec);
                }
            }
        }
    }

    /// Handle ICR write (send IPI)
    fn handle_icr_write(&self, icr: u64) {
        let vector = (icr & icr::VECTOR_MASK) as u8;
        let delmode =
            IcrDeliveryMode::from_bits(((icr & icr::DELMODE_MASK) >> icr::DELMODE_SHIFT) as u8);
        let dest_short = IcrDestShorthand::from_bits(
            ((icr & icr::DESTSHORT_MASK) >> icr::DESTSHORT_SHIFT) as u8,
        );
        let dest = ((icr & icr::DEST_MASK) >> icr::DEST_SHIFT) as u8;

        // Determine destination(s) based on shorthand
        let targets = match dest_short {
            IcrDestShorthand::SelfOnly => vec![self.id],
            IcrDestShorthand::AllIncludingSelf => vec![0xFF], // Special: broadcast
            IcrDestShorthand::AllExcludingSelf => vec![0xFE], // Special: broadcast except self
            IcrDestShorthand::None => vec![dest],
        };

        // Deliver via callback
        if let Some(ref cb) = *self.ipi_callback.read() {
            for &target in &targets {
                cb(target, vector, delmode);
            }
        }
    }

    /// Accept an interrupt
    pub fn accept_interrupt(&self, vector: u8, level_triggered: bool) {
        if !self.is_enabled() || vector < 16 {
            return;
        }

        let idx = (vector / 32) as usize;
        let bit = vector % 32;

        // Set IRR
        {
            let mut irr = self.irr.write();
            irr[idx] |= 1 << bit;
        }

        // Set TMR if level-triggered
        if level_triggered {
            let mut tmr = self.tmr.write();
            tmr[idx] |= 1 << bit;
        }
    }

    /// Get pending interrupt (highest priority from IRR that beats PPR)
    pub fn get_pending_interrupt(&self) -> Option<u8> {
        let ppr = {
            let tpr = self.tpr.load(Ordering::Relaxed);
            let isr_prio = self.get_highest_isr_priority() as u32;
            std::cmp::max(tpr & 0xF0, isr_prio << 4) as u8
        };

        let irr = self.irr.read();
        let vector = self.find_highest_set_bit(&irr)?;

        // Check if vector priority beats PPR
        if (vector & 0xF0) > ppr {
            Some(vector)
        } else {
            None
        }
    }

    /// Acknowledge interrupt (move from IRR to ISR)
    pub fn acknowledge_interrupt(&self, vector: u8) {
        let idx = (vector / 32) as usize;
        let bit = vector % 32;

        // Clear IRR
        {
            let mut irr = self.irr.write();
            irr[idx] &= !(1 << bit);
        }

        // Set ISR
        {
            let mut isr = self.isr.write();
            isr[idx] |= 1 << bit;
        }
    }

    /// Get highest priority in ISR
    fn get_highest_isr_priority(&self) -> u8 {
        let isr = self.isr.read();
        self.find_highest_set_bit(&isr).map(|v| v >> 4).unwrap_or(0)
    }

    /// Find highest set bit in 256-bit register
    fn find_highest_set_bit(&self, regs: &[u32; 8]) -> Option<u8> {
        for i in (0..8).rev() {
            if regs[i] != 0 {
                let bit = 31 - regs[i].leading_zeros();
                return Some((i * 32 + bit as usize) as u8);
            }
        }
        None
    }

    /// Timer tick (call periodically)
    pub fn timer_tick(&self) {
        let timer_mode = TimerMode::from_bits(
            ((self.lvt_timer.load(Ordering::Relaxed) & lvt::TIMER_MODE_MASK)
                >> lvt::TIMER_MODE_SHIFT) as u8,
        );

        if timer_mode == TimerMode::TscDeadline {
            return; // TSC deadline mode handled separately
        }

        let ccr = self.timer_ccr.load(Ordering::Relaxed);
        if ccr == 0 {
            return;
        }

        let divisor = timer_div::get_divisor(self.timer_dcr.load(Ordering::Relaxed));
        let new_ccr = ccr.saturating_sub(divisor);
        self.timer_ccr.store(new_ccr, Ordering::Relaxed);

        if new_ccr == 0 {
            // Timer expired
            let lvt = self.lvt_timer.load(Ordering::Relaxed);

            if lvt & lvt::MASKED == 0 {
                let vector = (lvt & lvt::VECTOR_MASK) as u8;
                self.accept_interrupt(vector, false);
            }

            // Reload if periodic
            if timer_mode == TimerMode::Periodic {
                self.timer_ccr
                    .store(self.timer_icr.load(Ordering::Relaxed), Ordering::Relaxed);
            }
        }
    }

    /// Get timer divisor
    pub fn get_timer_divisor(&self) -> u32 {
        timer_div::get_divisor(self.timer_dcr.load(Ordering::Relaxed))
    }

    /// Inject an external interrupt (from IOAPIC)
    pub fn inject_external_interrupt(&self, vector: u8, level_triggered: bool) {
        self.accept_interrupt(vector, level_triggered);
    }

    /// Reset the LAPIC
    pub fn reset(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        self.tpr.store(0, Ordering::Relaxed);
        self.ldr.store(0, Ordering::Relaxed);
        self.dfr.store(0xFFFF_FFFF, Ordering::Relaxed);
        self.svr.store(0xFF, Ordering::Relaxed);
        self.esr.store(0, Ordering::Relaxed);
        self.icr.store(0, Ordering::Relaxed);
        self.lvt_timer.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_thermal.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_pmc.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_lint0.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_lint1.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_error.store(lvt::MASKED, Ordering::Relaxed);
        self.lvt_cmci.store(lvt::MASKED, Ordering::Relaxed);
        self.timer_icr.store(0, Ordering::Relaxed);
        self.timer_ccr.store(0, Ordering::Relaxed);
        self.timer_dcr.store(0, Ordering::Relaxed);

        *self.isr.write() = [0; 8];
        *self.tmr.write() = [0; 8];
        *self.irr.write() = [0; 8];
    }
}

impl std::fmt::Debug for LocalApic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalApic")
            .field("id", &self.id)
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .field("svr", &format!("{:#x}", self.svr.load(Ordering::Relaxed)))
            .field("tpr", &format!("{:#x}", self.tpr.load(Ordering::Relaxed)))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lapic_creation() {
        let lapic = LocalApic::new(0);
        assert_eq!(lapic.id(), 0);
        assert!(!lapic.is_enabled());
    }

    #[test]
    fn test_lapic_id_register() {
        let lapic = LocalApic::new(5);
        let id = lapic.read(regs::ID).unwrap();
        assert_eq!((id >> 24) & 0xFF, 5);
    }

    #[test]
    fn test_lapic_version_register() {
        let lapic = LocalApic::new(0);
        let ver = lapic.read(regs::VERSION).unwrap();
        assert_eq!(ver & 0xFF, 0x14);
        assert_eq!((ver >> 16) & 0xFF, 5); // Max LVT
    }

    #[test]
    fn test_lapic_enable() {
        let lapic = LocalApic::new(0);

        // Initially disabled
        assert!(!lapic.is_enabled());

        // Enable via SVR
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();
        assert!(lapic.is_enabled());

        // Disable
        lapic.write(regs::SVR, 0xFF).unwrap();
        assert!(!lapic.is_enabled());
    }

    #[test]
    fn test_tpr_read_write() {
        let lapic = LocalApic::new(0);

        lapic.write(regs::TPR, 0x42).unwrap();
        assert_eq!(lapic.read(regs::TPR).unwrap(), 0x42);
    }

    #[test]
    fn test_lvt_timer() {
        let lapic = LocalApic::new(0);

        // Default is masked
        let lvt = lapic.read(regs::LVT_TIMER).unwrap();
        assert!(lvt & lvt::MASKED != 0);

        // Configure timer
        let config = 0x30 | (TimerMode::Periodic as u32) << lvt::TIMER_MODE_SHIFT;
        lapic.write(regs::LVT_TIMER, config).unwrap();

        let lvt = lapic.read(regs::LVT_TIMER).unwrap();
        assert_eq!(lvt & lvt::VECTOR_MASK, 0x30);
        assert!(lvt & lvt::MASKED == 0);
    }

    #[test]
    fn test_timer_icr_ccr() {
        let lapic = LocalApic::new(0);

        // Set initial count
        lapic.write(regs::TIMER_ICR, 1000).unwrap();

        // CCR should be loaded with ICR
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 1000);
    }

    #[test]
    fn test_timer_divisor() {
        let lapic = LocalApic::new(0);

        lapic.write(regs::TIMER_DCR, timer_div::DIV_16).unwrap();
        assert_eq!(lapic.get_timer_divisor(), 16);

        lapic.write(regs::TIMER_DCR, timer_div::DIV_1).unwrap();
        assert_eq!(lapic.get_timer_divisor(), 1);
    }

    #[test]
    fn test_accept_interrupt() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();

        lapic.accept_interrupt(0x42, false);

        // Check IRR
        let irr = lapic.read(regs::IRR_BASE + 0x20).unwrap(); // 0x42 is in register 2
        assert!(irr & (1 << 2) != 0); // bit 2 (0x42 % 32)
    }

    #[test]
    fn test_interrupt_acknowledge() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();

        lapic.accept_interrupt(0x42, false);
        lapic.acknowledge_interrupt(0x42);

        // IRR should be cleared
        let irr = lapic.read(regs::IRR_BASE + 0x20).unwrap();
        assert!(irr & (1 << 2) == 0);

        // ISR should be set
        let isr = lapic.read(regs::ISR_BASE + 0x20).unwrap();
        assert!(isr & (1 << 2) != 0);
    }

    #[test]
    fn test_eoi() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();

        lapic.accept_interrupt(0x42, false);
        lapic.acknowledge_interrupt(0x42);

        // EOI
        lapic.write(regs::EOI, 0).unwrap();

        // ISR should be cleared
        let isr = lapic.read(regs::ISR_BASE + 0x20).unwrap();
        assert!(isr & (1 << 2) == 0);
    }

    #[test]
    fn test_icr_write() {
        let lapic = LocalApic::new(0);
        use std::sync::atomic::AtomicU8;

        let received_dest = Arc::new(AtomicU8::new(0));
        let received_vec = Arc::new(AtomicU8::new(0));

        let dest_clone = received_dest.clone();
        let vec_clone = received_vec.clone();

        lapic.set_ipi_callback(move |dest, vec, _mode| {
            dest_clone.store(dest, Ordering::Relaxed);
            vec_clone.store(vec, Ordering::Relaxed);
        });

        // Write destination to ICR high
        lapic.write(regs::ICR_HIGH, 3 << 24).unwrap();
        // Write vector and trigger to ICR low
        lapic.write(regs::ICR_LOW, 0x42).unwrap();

        assert_eq!(received_dest.load(Ordering::Relaxed), 3);
        assert_eq!(received_vec.load(Ordering::Relaxed), 0x42);
    }

    #[test]
    fn test_timer_tick_oneshot() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();

        // Configure one-shot timer with vector 0x30
        lapic.write(regs::LVT_TIMER, 0x30).unwrap();
        lapic.write(regs::TIMER_DCR, timer_div::DIV_1).unwrap();
        lapic.write(regs::TIMER_ICR, 3).unwrap();

        // Tick 3 times
        lapic.timer_tick();
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 2);

        lapic.timer_tick();
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 1);

        lapic.timer_tick();
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 0);

        // Should have triggered interrupt
        let irr = lapic.read(regs::IRR_BASE + 0x10).unwrap();
        assert!(irr & (1 << 16) != 0); // 0x30 % 32 = 16

        // One-shot: should stay at 0
        lapic.timer_tick();
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 0);
    }

    #[test]
    fn test_timer_tick_periodic() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0xFF).unwrap();

        // Configure periodic timer
        let lvt = 0x30 | ((TimerMode::Periodic as u32) << lvt::TIMER_MODE_SHIFT);
        lapic.write(regs::LVT_TIMER, lvt).unwrap();
        lapic.write(regs::TIMER_DCR, timer_div::DIV_1).unwrap();
        lapic.write(regs::TIMER_ICR, 2).unwrap();

        // First period
        lapic.timer_tick();
        lapic.timer_tick();

        // Should reload
        assert_eq!(lapic.read(regs::TIMER_CCR).unwrap(), 2);
    }

    #[test]
    fn test_delivery_mode() {
        assert_eq!(IcrDeliveryMode::from_bits(0), IcrDeliveryMode::Fixed);
        assert_eq!(IcrDeliveryMode::from_bits(5), IcrDeliveryMode::Init);
        assert_eq!(IcrDeliveryMode::from_bits(6), IcrDeliveryMode::Sipi);
    }

    #[test]
    fn test_reset() {
        let lapic = LocalApic::new(0);
        lapic.write(regs::SVR, svr::APIC_ENABLE | 0x42).unwrap();
        lapic.write(regs::TPR, 0x80).unwrap();

        lapic.reset();

        assert!(!lapic.is_enabled());
        assert_eq!(lapic.read(regs::TPR).unwrap(), 0);
        assert_eq!(lapic.read(regs::SVR).unwrap(), 0xFF);
    }
}
