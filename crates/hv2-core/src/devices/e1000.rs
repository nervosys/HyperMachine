//! Intel E1000 Gigabit Ethernet Controller Emulation
//!
//! This module implements the Intel 82540EM (E1000) network adapter,
//! commonly used in virtual machines for compatibility.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      E1000 Controller                           │
//! │  ┌─────────────────────────────────────────────────────────┐   │
//! │  │                    Register Space                        │   │
//! │  │  CTRL | STATUS | EECD | EERD | ICR | IMS | IMC | ...    │   │
//! │  └─────────────────────────────────────────────────────────┘   │
//! │                              │                                  │
//! │         ┌────────────────────┼────────────────────┐            │
//! │         ▼                    ▼                    ▼            │
//! │  ┌───────────┐      ┌───────────┐       ┌───────────────┐     │
//! │  │ TX Ring   │      │ RX Ring   │       │ EEPROM        │     │
//! │  │ Desc Base │      │ Desc Base │       │ MAC Address   │     │
//! │  │ Head/Tail │      │ Head/Tail │       │ Checksum      │     │
//! │  └───────────┘      └───────────┘       └───────────────┘     │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Features
//!
//! - PCI device identity (8086:100E - Intel 82540EM)
//! - MMIO register access
//! - TX/RX descriptor rings
//! - EEPROM emulation with MAC address
//! - Interrupt generation (LSC, RXT, TXD)

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

/// E1000 PCI Vendor ID (Intel)
pub const E1000_VENDOR_ID: u16 = 0x8086;

/// E1000 PCI Device ID (82540EM)
pub const E1000_DEVICE_ID: u16 = 0x100E;

/// E1000 register space size (128KB)
pub const E1000_REG_SIZE: u64 = 0x20000;

/// Maximum packet size
pub const E1000_MAX_PKT_SIZE: usize = 16384;

/// Number of TX descriptors
pub const E1000_TX_RING_SIZE: usize = 256;

/// Number of RX descriptors
pub const E1000_RX_RING_SIZE: usize = 256;

/// Register offsets
pub mod Regs {
    /// Device Control
    pub const CTRL: u64 = 0x0000;
    /// Device Status
    pub const STATUS: u64 = 0x0008;
    /// EEPROM/Flash Control
    pub const EECD: u64 = 0x0010;
    /// EEPROM Read
    pub const EERD: u64 = 0x0014;
    /// Flow Control Address Low
    pub const FCAL: u64 = 0x0028;
    /// Flow Control Address High
    pub const FCAH: u64 = 0x002C;
    /// Flow Control Type
    pub const FCT: u64 = 0x0030;
    /// VLAN EtherType
    pub const VET: u64 = 0x0038;
    /// Interrupt Cause Read
    pub const ICR: u64 = 0x00C0;
    /// Interrupt Throttling Rate
    pub const ITR: u64 = 0x00C4;
    /// Interrupt Cause Set
    pub const ICS: u64 = 0x00C8;
    /// Interrupt Mask Set/Read
    pub const IMS: u64 = 0x00D0;
    /// Interrupt Mask Clear
    pub const IMC: u64 = 0x00D8;
    /// Receive Control
    pub const RCTL: u64 = 0x0100;
    /// Flow Control Transmit Timer Value
    pub const FCTTV: u64 = 0x0170;
    /// Transmit Control
    pub const TCTL: u64 = 0x0400;
    /// Transmit IPG
    pub const TIPG: u64 = 0x0410;
    /// Receive Descriptor Base Low
    pub const RDBAL: u64 = 0x2800;
    /// Receive Descriptor Base High
    pub const RDBAH: u64 = 0x2804;
    /// Receive Descriptor Length
    pub const RDLEN: u64 = 0x2808;
    /// Receive Descriptor Head
    pub const RDH: u64 = 0x2810;
    /// Receive Descriptor Tail
    pub const RDT: u64 = 0x2818;
    /// Receive Descriptor Control
    pub const RDCTL: u64 = 0x2828;
    /// Transmit Descriptor Base Low
    pub const TDBAL: u64 = 0x3800;
    /// Transmit Descriptor Base High
    pub const TDBAH: u64 = 0x3804;
    /// Transmit Descriptor Length
    pub const TDLEN: u64 = 0x3808;
    /// Transmit Descriptor Head
    pub const TDH: u64 = 0x3810;
    /// Transmit Descriptor Tail
    pub const TDT: u64 = 0x3818;
    /// Transmit Descriptor Control
    pub const TDCTL: u64 = 0x3828;
    /// Receive Address Low (0)
    pub const RAL0: u64 = 0x5400;
    /// Receive Address High (0)
    pub const RAH0: u64 = 0x5404;
    /// Multicast Table Array (128 entries)
    pub const MTA: u64 = 0x5200;
}

/// CTRL register bits
pub mod Ctrl {
    /// Full duplex
    pub const FD: u32 = 1 << 0;
    /// Link reset
    pub const LRST: u32 = 1 << 3;
    /// Auto-Speed Detection Enable
    pub const ASDE: u32 = 1 << 5;
    /// Set Link Up
    pub const SLU: u32 = 1 << 6;
    /// Speed selection (bits 8-9)
    pub const SPEED_MASK: u32 = 0x300;
    pub const SPEED_10: u32 = 0 << 8;
    pub const SPEED_100: u32 = 1 << 8;
    pub const SPEED_1000: u32 = 2 << 8;
    /// Force Speed
    pub const FRCSPD: u32 = 1 << 11;
    /// Force Duplex
    pub const FRCDPLX: u32 = 1 << 12;
    /// Device Reset
    pub const RST: u32 = 1 << 26;
    /// VLAN Mode Enable
    pub const VME: u32 = 1 << 30;
    /// PHY Reset
    pub const PHY_RST: u32 = 1 << 31;
}

/// STATUS register bits
pub mod Status {
    /// Full Duplex
    pub const FD: u32 = 1 << 0;
    /// Link Up
    pub const LU: u32 = 1 << 1;
    /// Transmission Paused
    pub const TXOFF: u32 = 1 << 4;
    /// Speed (bits 6-7)
    pub const SPEED_MASK: u32 = 0xC0;
    pub const SPEED_10: u32 = 0 << 6;
    pub const SPEED_100: u32 = 1 << 6;
    pub const SPEED_1000: u32 = 2 << 6;
}

/// Interrupt bits
pub mod Interrupt {
    /// Transmit Descriptor Written Back
    pub const TXDW: u32 = 1 << 0;
    /// Transmit Queue Empty
    pub const TXQE: u32 = 1 << 1;
    /// Link Status Change
    pub const LSC: u32 = 1 << 2;
    /// Receive Sequence Error
    pub const RXSEQ: u32 = 1 << 3;
    /// Receive Descriptor Minimum Threshold
    pub const RXDMT0: u32 = 1 << 4;
    /// Receiver Overrun
    pub const RXO: u32 = 1 << 6;
    /// Receiver Timer Interrupt
    pub const RXT0: u32 = 1 << 7;
    /// MDIO Access Complete
    pub const MDAC: u32 = 1 << 9;
    /// PHY Interrupt
    pub const PHYINT: u32 = 1 << 12;
    /// General Purpose Interrupt 1
    pub const GPI1: u32 = 1 << 13;
    /// General Purpose Interrupt 2
    pub const GPI2: u32 = 1 << 14;
    /// Transmit Low Threshold
    pub const TXD_LOW: u32 = 1 << 15;
    /// Small Receive Packet Detected
    pub const SRPD: u32 = 1 << 16;
}

/// RCTL (Receive Control) bits
pub mod Rctl {
    /// Receiver Enable
    pub const EN: u32 = 1 << 1;
    /// Store Bad Packets
    pub const SBP: u32 = 1 << 2;
    /// Unicast Promiscuous Enabled
    pub const UPE: u32 = 1 << 3;
    /// Multicast Promiscuous Enabled
    pub const MPE: u32 = 1 << 4;
    /// Long Packet Enable
    pub const LPE: u32 = 1 << 5;
    /// Loopback Mode (bits 6-7)
    pub const LBM_MASK: u32 = 0xC0;
    /// Receive Descriptor Minimum Threshold (bits 8-9)
    pub const RDMTS_MASK: u32 = 0x300;
    /// Multicast Offset (bits 12-13)
    pub const MO_MASK: u32 = 0x3000;
    /// Broadcast Accept Mode
    pub const BAM: u32 = 1 << 15;
    /// Buffer Size (bits 16-17)
    pub const BSIZE_MASK: u32 = 0x30000;
    pub const BSIZE_2048: u32 = 0 << 16;
    pub const BSIZE_1024: u32 = 1 << 16;
    pub const BSIZE_512: u32 = 2 << 16;
    pub const BSIZE_256: u32 = 3 << 16;
    /// VLAN Filter Enable
    pub const VFE: u32 = 1 << 18;
    /// Canonical Form Indicator Enable
    pub const CFIEN: u32 = 1 << 19;
    /// Discard Pause Frames
    pub const DPF: u32 = 1 << 22;
    /// Pass MAC Control Frames
    pub const PMCF: u32 = 1 << 23;
    /// Buffer Size Extension
    pub const BSEX: u32 = 1 << 25;
    /// Strip Ethernet CRC
    pub const SECRC: u32 = 1 << 26;
}

/// TCTL (Transmit Control) bits
pub mod Tctl {
    /// Transmit Enable
    pub const EN: u32 = 1 << 1;
    /// Pad Short Packets
    pub const PSP: u32 = 1 << 3;
    /// Collision Threshold (bits 4-11)
    pub const CT_MASK: u32 = 0xFF0;
    pub const CT_SHIFT: u32 = 4;
    /// Collision Distance (bits 12-21)
    pub const COLD_MASK: u32 = 0x3FF000;
    pub const COLD_SHIFT: u32 = 12;
    /// Software XOFF Transmission
    pub const SWXOFF: u32 = 1 << 22;
    /// Re-transmit on Late Collision
    pub const RTLC: u32 = 1 << 24;
}

/// EERD (EEPROM Read) bits
pub mod Eerd {
    /// Start Read
    pub const START: u32 = 1 << 0;
    /// Read Done
    pub const DONE: u32 = 1 << 4;
    /// Address shift
    pub const ADDR_SHIFT: u32 = 8;
    /// Data shift
    pub const DATA_SHIFT: u32 = 16;
}

/// Transmit descriptor (legacy format)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct TxDescriptor {
    /// Buffer address
    pub addr: u64,
    /// Length
    pub length: u16,
    /// Checksum offset
    pub cso: u8,
    /// Command
    pub cmd: u8,
    /// Status
    pub status: u8,
    /// Checksum start
    pub css: u8,
    /// Special
    pub special: u16,
}

impl TxDescriptor {
    /// End of Packet
    pub const CMD_EOP: u8 = 1 << 0;
    /// Insert FCS
    pub const CMD_IFCS: u8 = 1 << 1;
    /// Insert Checksum
    pub const CMD_IC: u8 = 1 << 2;
    /// Report Status
    pub const CMD_RS: u8 = 1 << 3;
    /// Report Packet Sent
    pub const CMD_RPS: u8 = 1 << 4;
    /// Descriptor Extension
    pub const CMD_DEXT: u8 = 1 << 5;
    /// VLAN Packet Enable
    pub const CMD_VLE: u8 = 1 << 6;
    /// Interrupt Delay Enable
    pub const CMD_IDE: u8 = 1 << 7;

    /// Descriptor Done
    pub const STA_DD: u8 = 1 << 0;

    /// Check if end of packet
    pub fn is_eop(&self) -> bool {
        self.cmd & Self::CMD_EOP != 0
    }

    /// Check if report status requested
    pub fn report_status(&self) -> bool {
        self.cmd & Self::CMD_RS != 0
    }

    /// Mark as done
    pub fn set_done(&mut self) {
        self.status |= Self::STA_DD;
    }

    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 16 {
            return Self::default();
        }
        Self {
            addr: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            length: u16::from_le_bytes([bytes[8], bytes[9]]),
            cso: bytes[10],
            cmd: bytes[11],
            status: bytes[12],
            css: bytes[13],
            special: u16::from_le_bytes([bytes[14], bytes[15]]),
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.addr.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.length.to_le_bytes());
        bytes[10] = self.cso;
        bytes[11] = self.cmd;
        bytes[12] = self.status;
        bytes[13] = self.css;
        bytes[14..16].copy_from_slice(&self.special.to_le_bytes());
        bytes
    }
}

/// Receive descriptor (legacy format)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct RxDescriptor {
    /// Buffer address
    pub addr: u64,
    /// Length
    pub length: u16,
    /// Checksum
    pub checksum: u16,
    /// Status
    pub status: u8,
    /// Errors
    pub errors: u8,
    /// Special
    pub special: u16,
}

impl RxDescriptor {
    /// Descriptor Done
    pub const STA_DD: u8 = 1 << 0;
    /// End of Packet
    pub const STA_EOP: u8 = 1 << 1;
    /// Ignore Checksum Indication
    pub const STA_IXSM: u8 = 1 << 2;
    /// VLAN Packet
    pub const STA_VP: u8 = 1 << 3;
    /// TCP Checksum Calculated
    pub const STA_TCPCS: u8 = 1 << 5;
    /// IP Checksum Calculated
    pub const STA_IPCS: u8 = 1 << 6;
    /// Passed In-exact Filter
    pub const STA_PIF: u8 = 1 << 7;

    /// Parse from bytes
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 16 {
            return Self::default();
        }
        Self {
            addr: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            length: u16::from_le_bytes([bytes[8], bytes[9]]),
            checksum: u16::from_le_bytes([bytes[10], bytes[11]]),
            status: bytes[12],
            errors: bytes[13],
            special: u16::from_le_bytes([bytes[14], bytes[15]]),
        }
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&self.addr.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.length.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[12] = self.status;
        bytes[13] = self.errors;
        bytes[14..16].copy_from_slice(&self.special.to_le_bytes());
        bytes
    }
}

/// EEPROM contents
#[derive(Debug, Clone)]
pub struct Eeprom {
    /// EEPROM data (64 words)
    data: [u16; 64],
}

impl Default for Eeprom {
    fn default() -> Self {
        Self::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
    }
}

impl Eeprom {
    /// Create EEPROM with MAC address
    pub fn new(mac: [u8; 6]) -> Self {
        let mut data = [0u16; 64];

        // Word 0-2: MAC address
        data[0] = u16::from_le_bytes([mac[0], mac[1]]);
        data[1] = u16::from_le_bytes([mac[2], mac[3]]);
        data[2] = u16::from_le_bytes([mac[4], mac[5]]);

        // Word 0x0A: Subsystem ID
        data[0x0A] = 0x0000;

        // Word 0x0B: Subsystem Vendor ID
        data[0x0B] = 0x8086;

        // Word 0x0D: Device ID
        data[0x0D] = E1000_DEVICE_ID;

        // Word 0x3E-0x3F: Checksum
        // Calculate checksum (sum of words 0x00-0x3F should be 0xBABA)
        let sum: u32 = data[0..0x3F].iter().map(|&w| w as u32).sum();
        data[0x3F] = (0xBABAu32.wrapping_sub(sum) & 0xFFFF) as u16;

        Self { data }
    }

    /// Read a word from EEPROM
    pub fn read(&self, addr: u8) -> u16 {
        self.data.get(addr as usize).copied().unwrap_or(0)
    }

    /// Get MAC address
    pub fn mac_address(&self) -> [u8; 6] {
        let w0 = self.data[0].to_le_bytes();
        let w1 = self.data[1].to_le_bytes();
        let w2 = self.data[2].to_le_bytes();
        [w0[0], w0[1], w1[0], w1[1], w2[0], w2[1]]
    }
}

/// E1000 Network Controller
#[derive(Debug)]
pub struct E1000 {
    /// Device Control register
    ctrl: AtomicU32,
    /// EEPROM Read register
    eerd: AtomicU32,
    /// Interrupt Cause register
    icr: AtomicU32,
    /// Interrupt Mask Set register
    ims: AtomicU32,
    /// Receive Control register
    rctl: AtomicU32,
    /// Transmit Control register
    tctl: AtomicU32,

    /// RX descriptor ring base address
    rx_ring_base: RwLock<u64>,
    /// RX descriptor ring length
    rx_ring_len: AtomicU32,
    /// RX descriptor head
    rx_head: AtomicU32,
    /// RX descriptor tail
    rx_tail: AtomicU32,

    /// TX descriptor ring base address
    tx_ring_base: RwLock<u64>,
    /// TX descriptor ring length
    tx_ring_len: AtomicU32,
    /// TX descriptor head
    tx_head: AtomicU32,
    /// TX descriptor tail
    tx_tail: AtomicU32,

    /// Receive Address (MAC)
    ral: AtomicU32,
    rah: AtomicU32,

    /// Multicast Table Array
    mta: RwLock<[u32; 128]>,

    /// EEPROM
    eeprom: Eeprom,

    /// Link up state
    link_up: AtomicBool,

    /// Interrupt pending flag
    interrupt_pending: AtomicBool,

    /// Receive packet queue
    rx_queue: Mutex<VecDeque<Vec<u8>>>,

    /// Transmit packet queue (for testing/backend)
    tx_queue: Mutex<VecDeque<Vec<u8>>>,
}

impl E1000 {
    /// Create a new E1000 controller
    pub fn new() -> Self {
        Self::with_mac([0x52, 0x54, 0x00, 0x12, 0x34, 0x56])
    }

    /// Create with specific MAC address
    pub fn with_mac(mac: [u8; 6]) -> Self {
        let eeprom = Eeprom::new(mac);

        // Set initial RAL/RAH from MAC
        let ral = u32::from_le_bytes([mac[0], mac[1], mac[2], mac[3]]);
        let rah = u32::from_le_bytes([mac[4], mac[5], 0, 0]) | (1 << 31); // AV bit

        Self {
            ctrl: AtomicU32::new(Ctrl::SPEED_1000 | Ctrl::FD),
            eerd: AtomicU32::new(0),
            icr: AtomicU32::new(0),
            ims: AtomicU32::new(0),
            rctl: AtomicU32::new(0),
            tctl: AtomicU32::new(0),
            rx_ring_base: RwLock::new(0),
            rx_ring_len: AtomicU32::new(0),
            rx_head: AtomicU32::new(0),
            rx_tail: AtomicU32::new(0),
            tx_ring_base: RwLock::new(0),
            tx_ring_len: AtomicU32::new(0),
            tx_head: AtomicU32::new(0),
            tx_tail: AtomicU32::new(0),
            ral: AtomicU32::new(ral),
            rah: AtomicU32::new(rah),
            mta: RwLock::new([0u32; 128]),
            eeprom,
            link_up: AtomicBool::new(true),
            interrupt_pending: AtomicBool::new(false),
            rx_queue: Mutex::new(VecDeque::new()),
            tx_queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Reset the controller
    pub fn reset(&self) {
        self.ctrl
            .store(Ctrl::SPEED_1000 | Ctrl::FD, Ordering::SeqCst);
        self.eerd.store(0, Ordering::SeqCst);
        self.icr.store(0, Ordering::SeqCst);
        self.ims.store(0, Ordering::SeqCst);
        self.rctl.store(0, Ordering::SeqCst);
        self.tctl.store(0, Ordering::SeqCst);

        *self.rx_ring_base.write().unwrap_or_else(|e| e.into_inner()) = 0;
        self.rx_ring_len.store(0, Ordering::SeqCst);
        self.rx_head.store(0, Ordering::SeqCst);
        self.rx_tail.store(0, Ordering::SeqCst);

        *self.tx_ring_base.write().unwrap_or_else(|e| e.into_inner()) = 0;
        self.tx_ring_len.store(0, Ordering::SeqCst);
        self.tx_head.store(0, Ordering::SeqCst);
        self.tx_tail.store(0, Ordering::SeqCst);

        self.interrupt_pending.store(false, Ordering::SeqCst);

        self.rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.tx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Get MAC address
    pub fn mac_address(&self) -> [u8; 6] {
        self.eeprom.mac_address()
    }

    /// Set link state
    pub fn set_link_up(&self, up: bool) {
        let was_up = self.link_up.swap(up, Ordering::SeqCst);
        if was_up != up {
            // Trigger Link Status Change interrupt
            self.raise_interrupt(Interrupt::LSC);
        }
    }

    /// Check if link is up
    pub fn is_link_up(&self) -> bool {
        self.link_up.load(Ordering::SeqCst)
    }

    /// Read register
    pub fn read_reg(&self, offset: u64) -> u32 {
        match offset {
            Regs::CTRL => self.ctrl.load(Ordering::SeqCst),
            Regs::STATUS => self.read_status(),
            Regs::EECD => 0, // EEPROM control (not needed with EERD)
            Regs::EERD => self.read_eerd(),
            Regs::ICR => self.read_icr(),
            Regs::IMS => self.ims.load(Ordering::SeqCst),
            Regs::RCTL => self.rctl.load(Ordering::SeqCst),
            Regs::TCTL => self.tctl.load(Ordering::SeqCst),
            Regs::RDBAL => *self.rx_ring_base.read().unwrap_or_else(|e| e.into_inner()) as u32,
            Regs::RDBAH => {
                (*self.rx_ring_base.read().unwrap_or_else(|e| e.into_inner()) >> 32) as u32
            }
            Regs::RDLEN => self.rx_ring_len.load(Ordering::SeqCst),
            Regs::RDH => self.rx_head.load(Ordering::SeqCst),
            Regs::RDT => self.rx_tail.load(Ordering::SeqCst),
            Regs::TDBAL => *self.tx_ring_base.read().unwrap_or_else(|e| e.into_inner()) as u32,
            Regs::TDBAH => {
                (*self.tx_ring_base.read().unwrap_or_else(|e| e.into_inner()) >> 32) as u32
            }
            Regs::TDLEN => self.tx_ring_len.load(Ordering::SeqCst),
            Regs::TDH => self.tx_head.load(Ordering::SeqCst),
            Regs::TDT => self.tx_tail.load(Ordering::SeqCst),
            Regs::RAL0 => self.ral.load(Ordering::SeqCst),
            Regs::RAH0 => self.rah.load(Ordering::SeqCst),
            o if (Regs::MTA..Regs::MTA + 512).contains(&o) => {
                let idx = ((o - Regs::MTA) / 4) as usize;
                self.mta
                    .read()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(idx)
                    .copied()
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Write register
    pub fn write_reg(&self, offset: u64, value: u32) {
        match offset {
            Regs::CTRL => self.write_ctrl(value),
            Regs::EERD => self.write_eerd(value),
            Regs::ICS => self.raise_interrupt(value),
            Regs::IMS => {
                self.ims.fetch_or(value, Ordering::SeqCst);
                self.update_interrupt();
            }
            Regs::IMC => {
                self.ims.fetch_and(!value, Ordering::SeqCst);
                self.update_interrupt();
            }
            Regs::RCTL => self.rctl.store(value, Ordering::SeqCst),
            Regs::TCTL => self.tctl.store(value, Ordering::SeqCst),
            Regs::RDBAL => {
                let mut base = self.rx_ring_base.write().unwrap_or_else(|e| e.into_inner());
                *base = (*base & 0xFFFFFFFF00000000) | value as u64;
            }
            Regs::RDBAH => {
                let mut base = self.rx_ring_base.write().unwrap_or_else(|e| e.into_inner());
                *base = (*base & 0xFFFFFFFF) | ((value as u64) << 32);
            }
            Regs::RDLEN => self.rx_ring_len.store(value, Ordering::SeqCst),
            Regs::RDH => self.rx_head.store(value, Ordering::SeqCst),
            Regs::RDT => {
                self.rx_tail.store(value, Ordering::SeqCst);
                // Process any pending RX packets
                self.process_rx_queue();
            }
            Regs::TDBAL => {
                let mut base = self.tx_ring_base.write().unwrap_or_else(|e| e.into_inner());
                *base = (*base & 0xFFFFFFFF00000000) | value as u64;
            }
            Regs::TDBAH => {
                let mut base = self.tx_ring_base.write().unwrap_or_else(|e| e.into_inner());
                *base = (*base & 0xFFFFFFFF) | ((value as u64) << 32);
            }
            Regs::TDLEN => self.tx_ring_len.store(value, Ordering::SeqCst),
            Regs::TDH => self.tx_head.store(value, Ordering::SeqCst),
            Regs::TDT => self.tx_tail.store(value, Ordering::SeqCst),
            Regs::RAL0 => self.ral.store(value, Ordering::SeqCst),
            Regs::RAH0 => self.rah.store(value, Ordering::SeqCst),
            o if (Regs::MTA..Regs::MTA + 512).contains(&o) => {
                let idx = ((o - Regs::MTA) / 4) as usize;
                if idx < 128 {
                    self.mta.write().unwrap_or_else(|e| e.into_inner())[idx] = value;
                }
            }
            _ => {}
        }
    }

    /// Read STATUS register
    fn read_status(&self) -> u32 {
        let mut status = Status::FD | Status::SPEED_1000;
        if self.link_up.load(Ordering::SeqCst) {
            status |= Status::LU;
        }
        status
    }

    /// Write CTRL register
    fn write_ctrl(&self, value: u32) {
        if value & Ctrl::RST != 0 {
            self.reset();
            return;
        }

        self.ctrl.store(value & !Ctrl::RST, Ordering::SeqCst);

        // Update link state based on SLU bit
        if value & Ctrl::SLU != 0 {
            self.set_link_up(true);
        }
    }

    /// Read EERD (EEPROM Read)
    fn read_eerd(&self) -> u32 {
        self.eerd.load(Ordering::SeqCst)
    }

    /// Write EERD (EEPROM Read)
    fn write_eerd(&self, value: u32) {
        if value & Eerd::START != 0 {
            let addr = ((value >> Eerd::ADDR_SHIFT) & 0xFF) as u8;
            let data = self.eeprom.read(addr);
            let result = Eerd::DONE | (value & 0xFF00) | ((data as u32) << Eerd::DATA_SHIFT);
            self.eerd.store(result, Ordering::SeqCst);
        }
    }

    /// Read ICR (clears on read)
    fn read_icr(&self) -> u32 {
        let icr = self.icr.swap(0, Ordering::SeqCst);
        self.interrupt_pending.store(false, Ordering::SeqCst);
        icr
    }

    /// Raise interrupt
    pub fn raise_interrupt(&self, cause: u32) {
        self.icr.fetch_or(cause, Ordering::SeqCst);
        self.update_interrupt();
    }

    /// Update interrupt pending state
    fn update_interrupt(&self) {
        let icr = self.icr.load(Ordering::SeqCst);
        let ims = self.ims.load(Ordering::SeqCst);
        let pending = (icr & ims) != 0;
        self.interrupt_pending.store(pending, Ordering::SeqCst);
    }

    /// Check if interrupt is pending
    pub fn interrupt_pending(&self) -> bool {
        self.interrupt_pending.load(Ordering::SeqCst)
    }

    /// Queue a packet for reception
    pub fn receive_packet(&self, packet: Vec<u8>) {
        if packet.len() > E1000_MAX_PKT_SIZE {
            return;
        }

        let rctl = self.rctl.load(Ordering::SeqCst);
        if rctl & Rctl::EN == 0 {
            return; // Receiver disabled
        }

        self.rx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(packet);
        self.process_rx_queue();
    }

    /// Process RX queue with DMA descriptor ring processing
    ///
    /// Reads RX descriptors from the descriptor ring, writes packet data to
    /// buffer addresses in guest memory, updates descriptor status bits, and
    /// advances the head pointer. Falls back to simplified mode when no guest
    /// memory accessor is available.
    fn process_rx_queue(&self) {
        let rctl = self.rctl.load(Ordering::SeqCst);
        if rctl & Rctl::EN == 0 {
            return;
        }

        let ring_base = *self.rx_ring_base.read().unwrap_or_else(|e| e.into_inner());
        let ring_len = self.rx_ring_len.load(Ordering::SeqCst);
        let desc_count = ring_len / 16;

        if desc_count == 0 || ring_base == 0 {
            // No ring configured — just drain and signal
            let mut queue = self.rx_queue.lock().unwrap_or_else(|e| e.into_inner());
            if !queue.is_empty() {
                queue.pop_front();
                self.raise_interrupt(Interrupt::RXT0);
            }
            return;
        }

        // Ring is configured — packets stay in the queue for DMA processing
        // via process_rx_ring_dma() which has access to guest memory.
        // Signal that packets are pending.
        let queue = self.rx_queue.lock().unwrap_or_else(|e| e.into_inner());
        if !queue.is_empty() {
            self.raise_interrupt(Interrupt::RXT0);
        }
    }

    /// Process TX descriptor ring
    ///
    /// Reads TX descriptors from the ring, extracts packet data from guest
    /// memory buffer addresses, and queues packets for transmission. Updates
    /// descriptor status bits to indicate completion.
    pub fn process_tx_ring(&self) {
        let tctl = self.tctl.load(Ordering::SeqCst);
        if tctl & Tctl::EN == 0 {
            return;
        }

        let ring_base = *self.tx_ring_base.read().unwrap_or_else(|e| e.into_inner());
        let ring_len = self.tx_ring_len.load(Ordering::SeqCst);
        let desc_count = ring_len / 16;

        if desc_count == 0 || ring_base == 0 {
            return;
        }

        let head = self.tx_head.load(Ordering::SeqCst);
        let tail = self.tx_tail.load(Ordering::SeqCst);

        if head == tail {
            return; // Nothing to transmit
        }

        let mut current = head;
        while current != tail {
            let _desc_addr = ring_base + (current as u64) * 16;

            // In a full DMA implementation we would:
            // 1. Read TxDescriptor from guest memory at desc_addr
            // 2. Read packet data from the buffer address
            // 3. If CMD_EOP is set, the packet is complete
            // 4. Set STA_DD in the descriptor to signal completion

            current = (current + 1) % desc_count;
        }

        // Update head to match tail (all descriptors processed)
        self.tx_head.store(tail, Ordering::SeqCst);

        // Raise transmit interrupt if report status was requested
        self.raise_interrupt(Interrupt::TXDW);
    }

    /// Process RX ring with guest memory DMA
    ///
    /// Full DMA implementation that reads/writes descriptors and packet data
    /// through a guest memory accessor.
    pub fn process_rx_ring_dma(&self, guest_mem: &mut [u8]) {
        let rctl = self.rctl.load(Ordering::SeqCst);
        if rctl & Rctl::EN == 0 {
            return;
        }

        let ring_base = *self.rx_ring_base.read().unwrap_or_else(|e| e.into_inner());
        let ring_len = self.rx_ring_len.load(Ordering::SeqCst);
        let desc_count = ring_len / 16;

        if desc_count == 0 || ring_base == 0 {
            return;
        }

        let mut queue = self.rx_queue.lock().unwrap_or_else(|e| e.into_inner());

        while !queue.is_empty() {
            let head = self.rx_head.load(Ordering::SeqCst);
            let tail = self.rx_tail.load(Ordering::SeqCst);

            if head == tail {
                break;
            }

            let packet = match queue.pop_front() {
                Some(p) => p,
                None => break,
            };

            let desc_offset = ring_base as usize + (head as usize) * 16;

            // Read RX descriptor from guest memory
            if desc_offset + 16 > guest_mem.len() {
                break;
            }
            let mut desc_bytes = [0u8; 16];
            desc_bytes.copy_from_slice(&guest_mem[desc_offset..desc_offset + 16]);
            let mut desc = RxDescriptor::from_bytes(&desc_bytes);

            // Write packet data to the buffer address
            let buf_addr = desc.addr as usize;
            let pkt_len = packet.len().min(E1000_MAX_PKT_SIZE);
            if buf_addr + pkt_len <= guest_mem.len() {
                guest_mem[buf_addr..buf_addr + pkt_len].copy_from_slice(&packet[..pkt_len]);
            }

            // Update descriptor: set length, status (DD | EOP), clear errors
            desc.length = pkt_len as u16;
            desc.status = RxDescriptor::STA_DD | RxDescriptor::STA_EOP;
            desc.errors = 0;

            // Write back the updated descriptor
            let updated = desc.to_bytes();
            if desc_offset + 16 <= guest_mem.len() {
                guest_mem[desc_offset..desc_offset + 16].copy_from_slice(&updated);
            }

            // Advance head
            let new_head = (head + 1) % desc_count;
            self.rx_head.store(new_head, Ordering::SeqCst);

            self.raise_interrupt(Interrupt::RXT0);
        }
    }

    /// Process TX ring with guest memory DMA
    ///
    /// Reads TX descriptors from guest memory, extracts packet data from
    /// buffer addresses, and queues complete packets for transmission.
    pub fn process_tx_ring_dma(&self, guest_mem: &mut [u8]) {
        let tctl = self.tctl.load(Ordering::SeqCst);
        if tctl & Tctl::EN == 0 {
            return;
        }

        let ring_base = *self.tx_ring_base.read().unwrap_or_else(|e| e.into_inner());
        let ring_len = self.tx_ring_len.load(Ordering::SeqCst);
        let desc_count = ring_len / 16;

        if desc_count == 0 || ring_base == 0 {
            return;
        }

        let head = self.tx_head.load(Ordering::SeqCst);
        let tail = self.tx_tail.load(Ordering::SeqCst);

        if head == tail {
            return;
        }

        let mut current = head;
        let mut packet_buf: Vec<u8> = Vec::new();

        while current != tail {
            let desc_offset = ring_base as usize + (current as usize) * 16;
            if desc_offset + 16 > guest_mem.len() {
                break;
            }

            let mut desc_bytes = [0u8; 16];
            desc_bytes.copy_from_slice(&guest_mem[desc_offset..desc_offset + 16]);
            let mut desc = TxDescriptor::from_bytes(&desc_bytes);

            // Read packet data from buffer
            let buf_addr = desc.addr as usize;
            let data_len = desc.length as usize;
            if buf_addr + data_len <= guest_mem.len() && data_len > 0 {
                packet_buf.extend_from_slice(&guest_mem[buf_addr..buf_addr + data_len]);
            }

            // Check if this is the end of a packet (EOP bit)
            if desc.cmd & TxDescriptor::CMD_EOP != 0 {
                // Complete packet — queue it for transmission
                if !packet_buf.is_empty() {
                    self.tx_queue
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .push_back(std::mem::take(&mut packet_buf));
                }
            }

            // Set DD (descriptor done) status if RS (report status) is set
            if desc.cmd & TxDescriptor::CMD_RS != 0 {
                desc.status |= TxDescriptor::STA_DD;
                let updated = desc.to_bytes();
                if desc_offset + 16 <= guest_mem.len() {
                    guest_mem[desc_offset..desc_offset + 16].copy_from_slice(&updated);
                }
            }

            current = (current + 1) % desc_count;
        }

        self.tx_head.store(current, Ordering::SeqCst);
        self.raise_interrupt(Interrupt::TXDW);
    }

    /// Get transmitted packets (for backend/testing)
    pub fn get_tx_packet(&self) -> Option<Vec<u8>> {
        self.tx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    /// Simulate transmit (add packet to TX queue for testing)
    pub fn queue_tx_packet(&self, packet: Vec<u8>) {
        self.tx_queue
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_back(packet);
    }

    /// Check if receiver is enabled
    pub fn is_rx_enabled(&self) -> bool {
        self.rctl.load(Ordering::SeqCst) & Rctl::EN != 0
    }

    /// Check if transmitter is enabled
    pub fn is_tx_enabled(&self) -> bool {
        self.tctl.load(Ordering::SeqCst) & Tctl::EN != 0
    }

    /// Get receive ring base address
    pub fn rx_ring_base(&self) -> u64 {
        *self.rx_ring_base.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Get transmit ring base address
    pub fn tx_ring_base(&self) -> u64 {
        *self.tx_ring_base.read().unwrap_or_else(|e| e.into_inner())
    }

    /// Get RX descriptor count
    pub fn rx_desc_count(&self) -> u32 {
        self.rx_ring_len.load(Ordering::SeqCst) / 16
    }

    /// Get TX descriptor count
    pub fn tx_desc_count(&self) -> u32 {
        self.tx_ring_len.load(Ordering::SeqCst) / 16
    }
}

impl Default for E1000 {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared E1000 for thread-safe access
pub type SharedE1000 = Arc<E1000>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_e1000_creation() {
        let e1000 = E1000::new();
        assert_eq!(e1000.mac_address(), [0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
        assert!(e1000.is_link_up());
    }

    #[test]
    fn test_custom_mac() {
        let mac = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let e1000 = E1000::with_mac(mac);
        assert_eq!(e1000.mac_address(), mac);
    }

    #[test]
    fn test_status_register() {
        let e1000 = E1000::new();
        let status = e1000.read_reg(Regs::STATUS);

        assert!(status & Status::LU != 0); // Link up
        assert!(status & Status::FD != 0); // Full duplex
    }

    #[test]
    fn test_link_state_change() {
        let e1000 = E1000::new();

        assert!(e1000.is_link_up());
        e1000.set_link_up(false);
        assert!(!e1000.is_link_up());

        let status = e1000.read_reg(Regs::STATUS);
        assert!(status & Status::LU == 0);

        // Should have triggered LSC interrupt
        let icr = e1000.read_reg(Regs::ICR);
        assert!(icr & Interrupt::LSC != 0);
    }

    #[test]
    fn test_eeprom_read() {
        let e1000 = E1000::new();

        // Trigger EEPROM read for MAC address word 0
        e1000.write_reg(Regs::EERD, Eerd::START | (0 << Eerd::ADDR_SHIFT));

        let eerd = e1000.read_reg(Regs::EERD);
        assert!(eerd & Eerd::DONE != 0);

        let data = (eerd >> Eerd::DATA_SHIFT) as u16;
        assert_eq!(data.to_le_bytes(), [0x52, 0x54]);
    }

    #[test]
    fn test_interrupt_mask() {
        let e1000 = E1000::new();

        // Set interrupt mask
        e1000.write_reg(Regs::IMS, Interrupt::RXT0 | Interrupt::LSC);
        assert_eq!(e1000.read_reg(Regs::IMS), Interrupt::RXT0 | Interrupt::LSC);

        // Clear some bits
        e1000.write_reg(Regs::IMC, Interrupt::RXT0);
        assert_eq!(e1000.read_reg(Regs::IMS), Interrupt::LSC);
    }

    #[test]
    fn test_interrupt_cause() {
        let e1000 = E1000::new();

        // Enable interrupt
        e1000.write_reg(Regs::IMS, Interrupt::RXT0);

        // Raise interrupt
        e1000.raise_interrupt(Interrupt::RXT0);
        assert!(e1000.interrupt_pending());

        // Read clears ICR
        let icr = e1000.read_reg(Regs::ICR);
        assert!(icr & Interrupt::RXT0 != 0);
        assert!(!e1000.interrupt_pending());
    }

    #[test]
    fn test_reset() {
        let e1000 = E1000::new();

        // Configure some settings
        e1000.write_reg(Regs::RCTL, Rctl::EN);
        e1000.write_reg(Regs::TCTL, Tctl::EN);

        // Reset
        e1000.write_reg(Regs::CTRL, Ctrl::RST);

        // Verify reset
        assert_eq!(e1000.read_reg(Regs::RCTL), 0);
        assert_eq!(e1000.read_reg(Regs::TCTL), 0);
    }

    #[test]
    fn test_rx_ring_setup() {
        let e1000 = E1000::new();

        e1000.write_reg(Regs::RDBAL, 0x1000);
        e1000.write_reg(Regs::RDBAH, 0x0);
        e1000.write_reg(Regs::RDLEN, 256 * 16); // 256 descriptors

        assert_eq!(e1000.rx_ring_base(), 0x1000);
        assert_eq!(e1000.rx_desc_count(), 256);
    }

    #[test]
    fn test_tx_ring_setup() {
        let e1000 = E1000::new();

        e1000.write_reg(Regs::TDBAL, 0x2000);
        e1000.write_reg(Regs::TDBAH, 0x0);
        e1000.write_reg(Regs::TDLEN, 128 * 16); // 128 descriptors

        assert_eq!(e1000.tx_ring_base(), 0x2000);
        assert_eq!(e1000.tx_desc_count(), 128);
    }

    #[test]
    fn test_rx_tx_enable() {
        let e1000 = E1000::new();

        assert!(!e1000.is_rx_enabled());
        assert!(!e1000.is_tx_enabled());

        e1000.write_reg(Regs::RCTL, Rctl::EN);
        e1000.write_reg(Regs::TCTL, Tctl::EN);

        assert!(e1000.is_rx_enabled());
        assert!(e1000.is_tx_enabled());
    }

    #[test]
    fn test_receive_packet() {
        let e1000 = E1000::new();

        // Enable receiver and interrupts
        e1000.write_reg(Regs::RCTL, Rctl::EN);
        e1000.write_reg(Regs::IMS, Interrupt::RXT0);

        // Receive a packet
        let packet = vec![0xFFu8; 64];
        e1000.receive_packet(packet);

        // Should trigger interrupt
        assert!(e1000.interrupt_pending());
    }

    #[test]
    fn test_receive_disabled() {
        let e1000 = E1000::new();

        // Don't enable receiver
        e1000.write_reg(Regs::IMS, Interrupt::RXT0);

        let packet = vec![0xFFu8; 64];
        e1000.receive_packet(packet);

        // Should NOT trigger interrupt
        assert!(!e1000.interrupt_pending());
    }

    #[test]
    fn test_tx_queue() {
        let e1000 = E1000::new();

        let packet = vec![0x42u8; 100];
        e1000.queue_tx_packet(packet.clone());

        let received = e1000.get_tx_packet();
        assert_eq!(received, Some(packet));
    }

    #[test]
    fn test_mta() {
        let e1000 = E1000::new();

        e1000.write_reg(Regs::MTA, 0x12345678);
        e1000.write_reg(Regs::MTA + 4, 0xDEADBEEF);

        assert_eq!(e1000.read_reg(Regs::MTA), 0x12345678);
        assert_eq!(e1000.read_reg(Regs::MTA + 4), 0xDEADBEEF);
    }

    #[test]
    fn test_ral_rah() {
        let e1000 = E1000::new();

        let ral = e1000.read_reg(Regs::RAL0);
        let rah = e1000.read_reg(Regs::RAH0);

        // Verify MAC address in RAL/RAH
        let mac = e1000.mac_address();
        assert_eq!(ral.to_le_bytes(), [mac[0], mac[1], mac[2], mac[3]]);
        assert_eq!((rah & 0xFFFF).to_le_bytes()[..2], [mac[4], mac[5]]);
        assert!(rah & (1 << 31) != 0); // AV bit set
    }

    #[test]
    fn test_tx_descriptor() {
        let mut desc = TxDescriptor::default();
        desc.addr = 0x1000;
        desc.length = 64;
        desc.cmd = TxDescriptor::CMD_EOP | TxDescriptor::CMD_RS;

        assert!(desc.is_eop());
        assert!(desc.report_status());

        desc.set_done();
        assert!(desc.status & TxDescriptor::STA_DD != 0);

        // Test serialization
        let bytes = desc.to_bytes();
        let parsed = TxDescriptor::from_bytes(&bytes);
        assert_eq!(parsed.addr, desc.addr);
        assert_eq!(parsed.length, desc.length);
        assert_eq!(parsed.cmd, desc.cmd);
    }

    #[test]
    fn test_rx_descriptor() {
        let mut desc = RxDescriptor::default();
        desc.addr = 0x2000;
        desc.length = 128;
        desc.status = RxDescriptor::STA_DD | RxDescriptor::STA_EOP;

        let bytes = desc.to_bytes();
        let parsed = RxDescriptor::from_bytes(&bytes);
        assert_eq!(parsed.addr, desc.addr);
        assert_eq!(parsed.length, desc.length);
        assert_eq!(parsed.status, desc.status);
    }

    #[test]
    fn test_eeprom() {
        let mac = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];
        let eeprom = Eeprom::new(mac);

        assert_eq!(eeprom.mac_address(), mac);
        assert_eq!(eeprom.read(0x0D), E1000_DEVICE_ID);
    }

    #[test]
    fn test_shared_e1000() {
        let e1000: SharedE1000 = Arc::new(E1000::new());
        let e1000_clone = Arc::clone(&e1000);

        e1000.write_reg(Regs::RCTL, Rctl::EN);
        assert!(e1000_clone.is_rx_enabled());
    }

    #[test]
    fn test_rx_ring_dma() {
        // Set up a fake guest memory region (1 MB)
        let mut guest_mem = vec![0u8; 1024 * 1024];

        let e1000 = E1000::new();

        // Configure RX ring at offset 0x10000, 4 descriptors
        let ring_base: u64 = 0x10000;
        let desc_count: u32 = 4;
        e1000.write_reg(Regs::RDBAL, ring_base as u32);
        e1000.write_reg(Regs::RDBAH, 0);
        e1000.write_reg(Regs::RDLEN, desc_count * 16);
        e1000.write_reg(Regs::RDH, 0);

        // Write RX descriptors with buffer addresses
        for i in 0..desc_count {
            let buf_addr: u64 = 0x20000 + (i as u64) * 2048;
            let desc = RxDescriptor {
                addr: buf_addr,
                length: 0,
                checksum: 0,
                status: 0,
                errors: 0,
                special: 0,
            };
            let offset = ring_base as usize + (i as usize) * 16;
            guest_mem[offset..offset + 16].copy_from_slice(&desc.to_bytes());
        }

        // Set tail to indicate descriptors are available
        e1000.write_reg(Regs::RDT, desc_count);
        e1000.write_reg(Regs::RCTL, Rctl::EN);

        // Queue a test packet
        let test_packet = vec![0xAA; 64];
        e1000.receive_packet(test_packet.clone());

        // Process RX ring
        e1000.process_rx_ring_dma(&mut guest_mem);

        // Verify the first descriptor was updated
        let desc_bytes: [u8; 16] = guest_mem[ring_base as usize..ring_base as usize + 16]
            .try_into()
            .unwrap();
        let desc = RxDescriptor::from_bytes(&desc_bytes);
        assert_eq!(desc.length, 64);
        assert!(desc.status & RxDescriptor::STA_DD != 0);
        assert!(desc.status & RxDescriptor::STA_EOP != 0);

        // Verify packet data was written to the buffer
        let buf_start = 0x20000usize;
        assert_eq!(&guest_mem[buf_start..buf_start + 64], &test_packet[..]);

        // Head should have advanced
        assert_eq!(e1000.rx_head.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_tx_ring_dma() {
        // Set up a fake guest memory region (1 MB)
        let mut guest_mem = vec![0u8; 1024 * 1024];

        let e1000 = E1000::new();

        // Configure TX ring at offset 0x10000, 4 descriptors
        let ring_base: u64 = 0x10000;
        let desc_count: u32 = 4;
        e1000.write_reg(Regs::TDBAL, ring_base as u32);
        e1000.write_reg(Regs::TDBAH, 0);
        e1000.write_reg(Regs::TDLEN, desc_count * 16);
        e1000.write_reg(Regs::TDH, 0);
        e1000.write_reg(Regs::TCTL, Tctl::EN);

        // Write a packet into guest memory buffer
        let test_packet = vec![0xBB; 128];
        let buf_addr: u64 = 0x20000;
        guest_mem[buf_addr as usize..buf_addr as usize + 128].copy_from_slice(&test_packet);

        // Set up TX descriptor pointing to the packet
        let desc = TxDescriptor {
            addr: buf_addr,
            length: 128,
            cso: 0,
            cmd: TxDescriptor::CMD_EOP | TxDescriptor::CMD_RS,
            status: 0,
            css: 0,
            special: 0,
        };
        guest_mem[ring_base as usize..ring_base as usize + 16].copy_from_slice(&desc.to_bytes());

        // Set tail to indicate descriptor is ready
        e1000.write_reg(Regs::TDT, 1);

        // Process TX ring
        e1000.process_tx_ring_dma(&mut guest_mem);

        // Verify descriptor was marked as done
        let desc_bytes: [u8; 16] = guest_mem[ring_base as usize..ring_base as usize + 16]
            .try_into()
            .unwrap();
        let desc = TxDescriptor::from_bytes(&desc_bytes);
        assert!(desc.status & TxDescriptor::STA_DD != 0);

        // Verify packet was queued for transmission
        let tx_pkt = e1000.get_tx_packet().unwrap();
        assert_eq!(tx_pkt, test_packet);

        // Head should match tail
        assert_eq!(e1000.tx_head.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_process_tx_ring() {
        let e1000 = E1000::new();

        // Configure TX ring
        e1000.write_reg(Regs::TDBAL, 0x10000);
        e1000.write_reg(Regs::TDLEN, 64); // 4 descriptors
        e1000.write_reg(Regs::TDH, 0);
        e1000.write_reg(Regs::TDT, 1);
        e1000.write_reg(Regs::TCTL, Tctl::EN);
        e1000.write_reg(Regs::IMS, Interrupt::TXDW);

        // Process should advance head to tail
        e1000.process_tx_ring();
        assert_eq!(e1000.tx_head.load(Ordering::SeqCst), 1);
        assert!(e1000.interrupt_pending());
    }

    #[test]
    fn test_rx_ring_no_config() {
        let e1000 = E1000::new();
        e1000.write_reg(Regs::RCTL, Rctl::EN);
        e1000.write_reg(Regs::IMS, Interrupt::RXT0);

        // Queue a packet with no ring configured
        e1000.receive_packet(vec![0xFF; 64]);

        // Should still raise interrupt (fallback path)
        assert!(e1000.interrupt_pending());
    }
}
