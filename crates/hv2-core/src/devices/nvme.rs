//! NVMe (Non-Volatile Memory Express) Controller Emulation
//!
//! This module implements an NVMe controller for high-performance storage access.
//! NVMe uses a command submission/completion queue model optimized for SSDs.
//!
//! # Architecture
//!
//! ```text
//! ┌────────────────────────────────────────────────────────────────────┐
//! │                       NVMe Controller                              │
//! │  ┌──────────────────────────────────────────────────────────────┐ │
//! │  │                    Admin Queue Pair                           │ │
//! │  │  ┌─────────────────────┐  ┌─────────────────────┐           │ │
//! │  │  │ Admin Submission Q  │  │ Admin Completion Q  │           │ │
//! │  │  │ (Commands)          │  │ (Responses)         │           │ │
//! │  │  └─────────────────────┘  └─────────────────────┘           │ │
//! │  └──────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────┐ │
//! │  │                    I/O Queue Pairs (1-N)                      │ │
//! │  │  ┌─────────────────────┐  ┌─────────────────────┐           │ │
//! │  │  │ I/O Submission Q    │  │ I/O Completion Q    │           │ │
//! │  │  │ (Read/Write cmds)   │  │ (Status)            │           │ │
//! │  │  └─────────────────────┘  └─────────────────────┘           │ │
//! │  └──────────────────────────────────────────────────────────────┘ │
//! │  ┌──────────────────────────────────────────────────────────────┐ │
//! │  │                    Controller Registers (BAR0)                │ │
//! │  │  CAP, VS, INTMS, INTMC, CC, CSTS, AQA, ASQ, ACQ, SQnTDBL...  │ │
//! │  └──────────────────────────────────────────────────────────────┘ │
//! └────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # NVMe Command Set
//!
//! **Admin Commands:**
//! - Identify Controller/Namespace
//! - Create/Delete I/O Submission/Completion Queue
//! - Get/Set Features
//! - Abort
//!
//! **I/O Commands:**
//! - Read
//! - Write
//! - Flush

use crate::Result;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use parking_lot::RwLock;

/// NVMe sector size (512 bytes standard, 4096 for 4Kn)
pub const NVME_SECTOR_SIZE: usize = 512;

/// NVMe namespace block size
pub const NVME_BLOCK_SIZE: usize = 512;

/// Maximum queue entries (power of 2)
pub const NVME_MAX_QUEUE_ENTRIES: u16 = 4096;

/// Maximum I/O queues
pub const NVME_MAX_IO_QUEUES: u16 = 64;

/// Controller register offsets
pub mod regs {
    /// Controller Capabilities (64-bit)
    pub const CAP: u64 = 0x00;
    /// Version (32-bit)
    pub const VS: u64 = 0x08;
    /// Interrupt Mask Set (32-bit)
    pub const INTMS: u64 = 0x0C;
    /// Interrupt Mask Clear (32-bit)
    pub const INTMC: u64 = 0x10;
    /// Controller Configuration (32-bit)
    pub const CC: u64 = 0x14;
    /// Controller Status (32-bit)
    pub const CSTS: u64 = 0x1C;
    /// NVM Subsystem Reset (32-bit)
    pub const NSSR: u64 = 0x20;
    /// Admin Queue Attributes (32-bit)
    pub const AQA: u64 = 0x24;
    /// Admin Submission Queue Base Address (64-bit)
    pub const ASQ: u64 = 0x28;
    /// Admin Completion Queue Base Address (64-bit)
    pub const ACQ: u64 = 0x30;
    /// Submission Queue y Tail Doorbell base
    pub const SQ_TDBL_BASE: u64 = 0x1000;
    /// Completion Queue y Head Doorbell base  
    pub const CQ_HDBL_BASE: u64 = 0x1004;
}

/// Controller Capabilities bits
pub mod cap {
    /// Maximum Queue Entries Supported (bits 0-15)
    pub const MQES_MASK: u64 = 0xFFFF;
    /// Contiguous Queues Required (bit 16)
    pub const CQR: u64 = 1 << 16;
    /// Arbitration Mechanism Supported (bits 17-18)
    pub const AMS_MASK: u64 = 0x3 << 17;
    /// Timeout (bits 24-31) - in 500ms units
    pub const TO_SHIFT: u64 = 24;
    /// Doorbell Stride (bits 32-35)
    pub const DSTRD_SHIFT: u64 = 32;
    /// NVM Subsystem Reset Supported (bit 36)
    pub const NSSRS: u64 = 1 << 36;
    /// Command Sets Supported (bits 37-44)
    pub const CSS_NVM: u64 = 1 << 37;
    /// Memory Page Size Minimum (bits 48-51)
    pub const MPSMIN_SHIFT: u64 = 48;
    /// Memory Page Size Maximum (bits 52-55)
    pub const MPSMAX_SHIFT: u64 = 52;
}

/// Controller Configuration bits
pub mod cc {
    /// Enable (bit 0)
    pub const EN: u32 = 1 << 0;
    /// I/O Command Set Selected (bits 4-6)
    pub const CSS_SHIFT: u32 = 4;
    /// Memory Page Size (bits 7-10)
    pub const MPS_SHIFT: u32 = 7;
    /// Arbitration Mechanism Selected (bits 11-13)
    pub const AMS_SHIFT: u32 = 11;
    /// Shutdown Notification (bits 14-15)
    pub const SHN_SHIFT: u32 = 14;
    /// I/O Submission Queue Entry Size (bits 16-19)
    pub const IOSQES_SHIFT: u32 = 16;
    /// I/O Completion Queue Entry Size (bits 20-23)
    pub const IOCQES_SHIFT: u32 = 20;
}

/// Controller Status bits
pub mod csts {
    /// Ready (bit 0)
    pub const RDY: u32 = 1 << 0;
    /// Controller Fatal Status (bit 1)
    pub const CFS: u32 = 1 << 1;
    /// Shutdown Status (bits 2-3)
    pub const SHST_MASK: u32 = 0x3 << 2;
    /// Processing Paused (bit 5)
    pub const PP: u32 = 1 << 5;
}

/// Admin command opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AdminOpcode {
    /// Delete I/O Submission Queue
    DeleteIoSq = 0x00,
    /// Create I/O Submission Queue
    CreateIoSq = 0x01,
    /// Get Log Page
    GetLogPage = 0x02,
    /// Delete I/O Completion Queue
    DeleteIoCq = 0x04,
    /// Create I/O Completion Queue
    CreateIoCq = 0x05,
    /// Identify
    Identify = 0x06,
    /// Abort
    Abort = 0x08,
    /// Set Features
    SetFeatures = 0x09,
    /// Get Features
    GetFeatures = 0x0A,
    /// Async Event Request
    AsyncEventReq = 0x0C,
    /// Namespace Management
    NsManagement = 0x0D,
    /// Unknown opcode
    Unknown = 0xFF,
}

impl From<u8> for AdminOpcode {
    fn from(val: u8) -> Self {
        match val {
            0x00 => Self::DeleteIoSq,
            0x01 => Self::CreateIoSq,
            0x02 => Self::GetLogPage,
            0x04 => Self::DeleteIoCq,
            0x05 => Self::CreateIoCq,
            0x06 => Self::Identify,
            0x08 => Self::Abort,
            0x09 => Self::SetFeatures,
            0x0A => Self::GetFeatures,
            0x0C => Self::AsyncEventReq,
            0x0D => Self::NsManagement,
            _ => Self::Unknown,
        }
    }
}

/// I/O command opcodes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IoOpcode {
    /// Flush
    Flush = 0x00,
    /// Write
    Write = 0x01,
    /// Read
    Read = 0x02,
    /// Write Uncorrectable
    WriteUncorrectable = 0x04,
    /// Compare
    Compare = 0x05,
    /// Write Zeroes
    WriteZeroes = 0x08,
    /// Dataset Management
    DatasetManagement = 0x09,
    /// Unknown opcode
    Unknown = 0xFF,
}

impl From<u8> for IoOpcode {
    fn from(val: u8) -> Self {
        match val {
            0x00 => Self::Flush,
            0x01 => Self::Write,
            0x02 => Self::Read,
            0x04 => Self::WriteUncorrectable,
            0x05 => Self::Compare,
            0x08 => Self::WriteZeroes,
            0x09 => Self::DatasetManagement,
            _ => Self::Unknown,
        }
    }
}

/// NVMe status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum StatusCode {
    /// Successful completion
    Success = 0x0000,
    /// Invalid Command Opcode
    InvalidOpcode = 0x0001,
    /// Invalid Field in Command
    InvalidField = 0x0002,
    /// Command ID Conflict
    CmdIdConflict = 0x0003,
    /// Data Transfer Error
    DataTransferError = 0x0004,
    /// Commands Aborted due to Power Loss
    PowerLoss = 0x0005,
    /// Internal Error
    InternalError = 0x0006,
    /// Command Abort Requested
    CmdAbortReq = 0x0007,
    /// Invalid Namespace or Format
    InvalidNsOrFormat = 0x000B,
    /// Invalid Queue Identifier
    InvalidQueueId = 0x0100,
    /// Invalid Queue Size
    InvalidQueueSize = 0x0101,
}

/// NVMe submission queue entry (64 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct SubmissionQueueEntry {
    /// Command Dword 0: Opcode, Fused, PSDT, CID
    pub cdw0: u32,
    /// Namespace ID
    pub nsid: u32,
    /// Command Dword 2-3 (reserved or command specific)
    pub cdw2: u32,
    pub cdw3: u32,
    /// Metadata Pointer
    pub mptr: u64,
    /// Data Pointer (PRP1)
    pub dptr_prp1: u64,
    /// Data Pointer (PRP2)
    pub dptr_prp2: u64,
    /// Command Dword 10-15 (command specific)
    pub cdw10: u32,
    pub cdw11: u32,
    pub cdw12: u32,
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl SubmissionQueueEntry {
    /// Get opcode
    pub fn opcode(&self) -> u8 {
        (self.cdw0 & 0xFF) as u8
    }

    /// Get command ID
    pub fn cid(&self) -> u16 {
        ((self.cdw0 >> 16) & 0xFFFF) as u16
    }

    /// Get fused operation
    pub fn fused(&self) -> u8 {
        ((self.cdw0 >> 8) & 0x3) as u8
    }
}

/// NVMe completion queue entry (16 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct CompletionQueueEntry {
    /// Command Specific
    pub dw0: u32,
    /// Reserved
    pub dw1: u32,
    /// SQ Head Pointer, SQ Identifier
    pub sq_head_sqid: u32,
    /// Status Field, Phase Tag, Command Identifier
    pub status_cid: u32,
}

impl CompletionQueueEntry {
    /// Create a completion entry
    pub fn new(cid: u16, sq_id: u16, sq_head: u16, status: StatusCode, phase: bool) -> Self {
        let phase_bit = if phase { 1 } else { 0 };
        Self {
            dw0: 0,
            dw1: 0,
            sq_head_sqid: (sq_head as u32) | ((sq_id as u32) << 16),
            status_cid: (cid as u32) | ((status as u32) << 17) | (phase_bit << 16),
        }
    }

    /// Get command ID
    pub fn cid(&self) -> u16 {
        (self.status_cid & 0xFFFF) as u16
    }

    /// Get phase tag
    pub fn phase(&self) -> bool {
        (self.status_cid >> 16) & 1 != 0
    }

    /// Get status code
    pub fn status(&self) -> u16 {
        ((self.status_cid >> 17) & 0x7FFF) as u16
    }
}

/// NVMe Queue
#[derive(Debug, Clone)]
pub struct NvmeQueue {
    /// Queue ID
    pub id: u16,
    /// Queue size (number of entries)
    pub size: u16,
    /// Base address in guest memory
    pub base_addr: u64,
    /// Current head pointer
    pub head: u16,
    /// Current tail pointer
    pub tail: u16,
    /// Phase bit (for CQ only)
    pub phase: bool,
    /// Associated completion queue ID (for SQ only)
    pub cq_id: u16,
    /// Interrupt vector
    pub iv: u16,
    /// Interrupts enabled
    pub ien: bool,
}

impl NvmeQueue {
    /// Create a new queue
    pub fn new(id: u16, size: u16, base_addr: u64) -> Self {
        Self {
            id,
            size,
            base_addr,
            head: 0,
            tail: 0,
            phase: true,
            cq_id: 0,
            iv: 0,
            ien: true,
        }
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Check if queue is full
    pub fn is_full(&self) -> bool {
        ((self.tail + 1) % self.size) == self.head
    }

    /// Get number of entries
    pub fn count(&self) -> u16 {
        if self.tail >= self.head {
            self.tail - self.head
        } else {
            self.size - self.head + self.tail
        }
    }

    /// Advance head
    pub fn advance_head(&mut self) {
        self.head = (self.head + 1) % self.size;
    }

    /// Advance tail (for CQ, also toggles phase at wrap)
    pub fn advance_tail(&mut self) {
        self.tail = (self.tail + 1) % self.size;
        if self.tail == 0 {
            self.phase = !self.phase;
        }
    }
}

/// Identify Controller data structure (4096 bytes, simplified)
#[derive(Debug, Clone)]
pub struct IdentifyController {
    /// PCI Vendor ID
    pub vid: u16,
    /// PCI Subsystem Vendor ID
    pub ssvid: u16,
    /// Serial Number (20 bytes)
    pub sn: [u8; 20],
    /// Model Number (40 bytes)
    pub mn: [u8; 40],
    /// Firmware Revision (8 bytes)
    pub fr: [u8; 8],
    /// Recommended Arbitration Burst
    pub rab: u8,
    /// IEEE OUI Identifier
    pub ieee: [u8; 3],
    /// Controller Multi-Path I/O and Namespace Sharing
    pub cmic: u8,
    /// Maximum Data Transfer Size
    pub mdts: u8,
    /// Controller ID
    pub cntlid: u16,
    /// Version
    pub ver: u32,
    /// Number of Namespaces
    pub nn: u32,
}

impl Default for IdentifyController {
    fn default() -> Self {
        let mut sn = [0u8; 20];
        sn[..8].copy_from_slice(b"NVME0001");

        let mut mn = [0u8; 40];
        mn[..16].copy_from_slice(b"AetherVM NVMe   ");

        let mut fr = [0u8; 8];
        fr[..4].copy_from_slice(b"1.0 ");

        Self {
            vid: 0x8086, // Intel
            ssvid: 0x8086,
            sn,
            mn,
            fr,
            rab: 6,
            ieee: [0x00, 0x00, 0x00],
            cmic: 0,
            mdts: 5, // 128KB max transfer (2^5 * 4KB)
            cntlid: 1,
            ver: 0x00010400, // NVMe 1.4
            nn: 1,
        }
    }
}

impl IdentifyController {
    /// Serialize to bytes (4096 bytes)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = vec![0u8; 4096];

        data[0..2].copy_from_slice(&self.vid.to_le_bytes());
        data[2..4].copy_from_slice(&self.ssvid.to_le_bytes());
        data[4..24].copy_from_slice(&self.sn);
        data[24..64].copy_from_slice(&self.mn);
        data[64..72].copy_from_slice(&self.fr);
        data[72] = self.rab;
        data[73..76].copy_from_slice(&self.ieee);
        data[76] = self.cmic;
        data[77] = self.mdts;
        data[78..80].copy_from_slice(&self.cntlid.to_le_bytes());
        data[80..84].copy_from_slice(&self.ver.to_le_bytes());
        // NN at offset 516
        data[516..520].copy_from_slice(&self.nn.to_le_bytes());

        data
    }
}

/// Identify Namespace data structure (4096 bytes, simplified)
#[derive(Debug, Clone)]
pub struct IdentifyNamespace {
    /// Namespace Size (in blocks)
    pub nsze: u64,
    /// Namespace Capacity (in blocks)
    pub ncap: u64,
    /// Namespace Utilization (in blocks)
    pub nuse: u64,
    /// Formatted LBA Size
    pub flbas: u8,
    /// Number of LBA Formats
    pub nlbaf: u8,
    /// LBA Format 0
    pub lbaf0: u32,
}

impl IdentifyNamespace {
    /// Create for a given size in bytes
    pub fn new(size_bytes: u64) -> Self {
        let blocks = size_bytes / NVME_BLOCK_SIZE as u64;
        Self {
            nsze: blocks,
            ncap: blocks,
            nuse: blocks,
            flbas: 0, // LBA format 0
            nlbaf: 0, // 1 LBA format
            lbaf0: 9, // 2^9 = 512 byte blocks
        }
    }

    /// Serialize to bytes (4096 bytes)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = vec![0u8; 4096];

        data[0..8].copy_from_slice(&self.nsze.to_le_bytes());
        data[8..16].copy_from_slice(&self.ncap.to_le_bytes());
        data[16..24].copy_from_slice(&self.nuse.to_le_bytes());
        data[26] = self.flbas;
        data[25] = self.nlbaf;
        data[128..132].copy_from_slice(&self.lbaf0.to_le_bytes());

        data
    }
}

/// NVMe Controller
pub struct NvmeController {
    /// Controller enabled
    enabled: AtomicBool,
    /// Controller ready
    ready: AtomicBool,
    /// Controller configuration
    cc: AtomicU32,
    /// Controller status
    csts: AtomicU32,
    /// Interrupt mask
    intms: AtomicU32,
    /// Admin queue attributes
    aqa: AtomicU32,
    /// Admin submission queue base
    asq: AtomicU64,
    /// Admin completion queue base
    acq: AtomicU64,
    /// Admin submission queue
    admin_sq: RwLock<Option<NvmeQueue>>,
    /// Admin completion queue
    admin_cq: RwLock<Option<NvmeQueue>>,
    /// I/O submission queues
    io_sq: RwLock<Vec<Option<NvmeQueue>>>,
    /// I/O completion queues
    io_cq: RwLock<Vec<Option<NvmeQueue>>>,
    /// Controller identify data
    identify_ctrl: IdentifyController,
    /// Namespace identify data
    identify_ns: IdentifyNamespace,
    /// Storage backend (block data)
    storage: RwLock<Vec<u8>>,
    /// Pending completions
    completions: RwLock<VecDeque<(u16, CompletionQueueEntry)>>,
}

impl NvmeController {
    /// Create a new NVMe controller
    pub fn new(storage_size: u64) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            cc: AtomicU32::new(0),
            csts: AtomicU32::new(0),
            intms: AtomicU32::new(0),
            aqa: AtomicU32::new(0),
            asq: AtomicU64::new(0),
            acq: AtomicU64::new(0),
            admin_sq: RwLock::new(None),
            admin_cq: RwLock::new(None),
            io_sq: RwLock::new(vec![None; NVME_MAX_IO_QUEUES as usize]),
            io_cq: RwLock::new(vec![None; NVME_MAX_IO_QUEUES as usize]),
            identify_ctrl: IdentifyController::default(),
            identify_ns: IdentifyNamespace::new(storage_size),
            storage: RwLock::new(vec![0u8; storage_size as usize]),
            completions: RwLock::new(VecDeque::new()),
        }
    }

    /// Get controller capabilities
    pub fn capabilities(&self) -> u64 {
        let mqes = (NVME_MAX_QUEUE_ENTRIES - 1) as u64;
        let to = 40u64; // 20 seconds timeout
        let dstrd = 0u64; // 4 byte doorbell stride
        let css_nvm = cap::CSS_NVM;
        let mpsmin = 0u64; // 4KB minimum
        let mpsmax = 0u64; // 4KB maximum

        mqes | cap::CQR
            | (to << cap::TO_SHIFT)
            | (dstrd << cap::DSTRD_SHIFT)
            | css_nvm
            | (mpsmin << cap::MPSMIN_SHIFT)
            | (mpsmax << cap::MPSMAX_SHIFT)
    }

    /// Read register
    pub fn read_reg(&self, offset: u64, size: u8) -> u64 {
        match offset {
            regs::CAP => self.capabilities(),
            regs::VS => 0x00010400, // NVMe 1.4.0
            regs::INTMS => self.intms.load(Ordering::Relaxed) as u64,
            regs::INTMC => 0,
            regs::CC => self.cc.load(Ordering::Relaxed) as u64,
            regs::CSTS => self.csts.load(Ordering::Relaxed) as u64,
            regs::AQA => self.aqa.load(Ordering::Relaxed) as u64,
            regs::ASQ => self.asq.load(Ordering::Relaxed),
            regs::ACQ => self.acq.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Write register
    pub fn write_reg(&self, offset: u64, value: u64, size: u8) {
        match offset {
            regs::INTMS => {
                self.intms.fetch_or(value as u32, Ordering::Relaxed);
            }
            regs::INTMC => {
                self.intms.fetch_and(!(value as u32), Ordering::Relaxed);
            }
            regs::CC => {
                let old_cc = self.cc.load(Ordering::Relaxed);
                self.cc.store(value as u32, Ordering::Relaxed);

                let was_enabled = old_cc & cc::EN != 0;
                let is_enabled = value as u32 & cc::EN != 0;

                if !was_enabled && is_enabled {
                    self.enable_controller();
                } else if was_enabled && !is_enabled {
                    self.disable_controller();
                }
            }
            regs::AQA => {
                self.aqa.store(value as u32, Ordering::Relaxed);
            }
            regs::ASQ => {
                self.asq.store(value, Ordering::Relaxed);
            }
            regs::ACQ => {
                self.acq.store(value, Ordering::Relaxed);
            }
            _ => {
                // Check for doorbell writes
                if offset >= regs::SQ_TDBL_BASE {
                    self.handle_doorbell(offset, value as u16);
                }
            }
        }
    }

    /// Enable controller
    fn enable_controller(&self) {
        let aqa = self.aqa.load(Ordering::Relaxed);
        let asqsize = ((aqa & 0xFFF) + 1) as u16;
        let acqsize = (((aqa >> 16) & 0xFFF) + 1) as u16;

        let asq_addr = self.asq.load(Ordering::Relaxed);
        let acq_addr = self.acq.load(Ordering::Relaxed);

        *self.admin_sq.write() = Some(NvmeQueue::new(0, asqsize, asq_addr));
        *self.admin_cq.write() = Some(NvmeQueue::new(0, acqsize, acq_addr));

        self.enabled.store(true, Ordering::Relaxed);
        self.ready.store(true, Ordering::Relaxed);
        self.csts.fetch_or(csts::RDY, Ordering::Relaxed);
    }

    /// Disable controller
    fn disable_controller(&self) {
        self.enabled.store(false, Ordering::Relaxed);
        self.ready.store(false, Ordering::Relaxed);
        self.csts.fetch_and(!csts::RDY, Ordering::Relaxed);

        *self.admin_sq.write() = None;
        *self.admin_cq.write() = None;

        let mut io_sq = self.io_sq.write();
        let mut io_cq = self.io_cq.write();
        for i in 0..NVME_MAX_IO_QUEUES as usize {
            io_sq[i] = None;
            io_cq[i] = None;
        }
    }

    /// Handle doorbell write
    fn handle_doorbell(&self, offset: u64, value: u16) {
        let db_offset = offset - regs::SQ_TDBL_BASE;
        let stride = 8u64; // 4 bytes per doorbell, SQ and CQ interleaved
        let qid = (db_offset / stride) as u16;
        let is_cq = (db_offset % stride) >= 4;

        if is_cq {
            // Completion queue head doorbell
            self.update_cq_head(qid, value);
        } else {
            // Submission queue tail doorbell
            self.update_sq_tail(qid, value);
        }
    }

    /// Update submission queue tail
    fn update_sq_tail(&self, qid: u16, tail: u16) {
        if qid == 0 {
            if let Some(ref mut sq) = *self.admin_sq.write() {
                sq.tail = tail;
            }
        } else if (qid as usize) <= NVME_MAX_IO_QUEUES as usize {
            let mut io_sq = self.io_sq.write();
            if let Some(ref mut sq) = io_sq[(qid - 1) as usize] {
                sq.tail = tail;
            }
        }
    }

    /// Update completion queue head
    fn update_cq_head(&self, qid: u16, head: u16) {
        if qid == 0 {
            if let Some(ref mut cq) = *self.admin_cq.write() {
                cq.head = head;
            }
        } else if (qid as usize) <= NVME_MAX_IO_QUEUES as usize {
            let mut io_cq = self.io_cq.write();
            if let Some(ref mut cq) = io_cq[(qid - 1) as usize] {
                cq.head = head;
            }
        }
    }

    /// Process a submission queue entry
    pub fn process_command(&self, sqe: &SubmissionQueueEntry, sq_id: u16) -> CompletionQueueEntry {
        let opcode = sqe.opcode();
        let cid = sqe.cid();

        if sq_id == 0 {
            // Admin command
            self.process_admin_command(sqe, cid)
        } else {
            // I/O command
            self.process_io_command(sqe, sq_id, cid)
        }
    }

    /// Process admin command
    fn process_admin_command(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let opcode = AdminOpcode::from(sqe.opcode());

        match opcode {
            AdminOpcode::Identify => self.cmd_identify(sqe, cid),
            AdminOpcode::CreateIoCq => self.cmd_create_io_cq(sqe, cid),
            AdminOpcode::CreateIoSq => self.cmd_create_io_sq(sqe, cid),
            AdminOpcode::DeleteIoCq => self.cmd_delete_io_cq(sqe, cid),
            AdminOpcode::DeleteIoSq => self.cmd_delete_io_sq(sqe, cid),
            AdminOpcode::GetFeatures => self.cmd_get_features(sqe, cid),
            AdminOpcode::SetFeatures => self.cmd_set_features(sqe, cid),
            _ => CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidOpcode, true),
        }
    }

    /// Process I/O command
    fn process_io_command(
        &self,
        sqe: &SubmissionQueueEntry,
        sq_id: u16,
        cid: u16,
    ) -> CompletionQueueEntry {
        let opcode = IoOpcode::from(sqe.opcode());

        match opcode {
            IoOpcode::Read => self.cmd_read(sqe, sq_id, cid),
            IoOpcode::Write => self.cmd_write(sqe, sq_id, cid),
            IoOpcode::Flush => CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::Success, true),
            _ => CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::InvalidOpcode, true),
        }
    }

    /// Identify command
    fn cmd_identify(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let cns = sqe.cdw10 & 0xFF; // Controller or Namespace Structure

        // Data would be written to PRP1 address in real implementation
        // For now, just return success
        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Create I/O Completion Queue
    fn cmd_create_io_cq(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let qid = (sqe.cdw10 & 0xFFFF) as u16;
        let qsize = ((sqe.cdw10 >> 16) & 0xFFFF) as u16 + 1;
        let iv = (sqe.cdw11 & 0xFFFF) as u16;
        let ien = (sqe.cdw11 >> 16) & 1 != 0;
        let pc = (sqe.cdw11 >> 16) & 2 != 0; // Physically contiguous

        if qid == 0 || qid as usize > NVME_MAX_IO_QUEUES as usize {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueId, true);
        }

        if !(2..=NVME_MAX_QUEUE_ENTRIES).contains(&qsize) {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueSize, true);
        }

        let mut io_cq = self.io_cq.write();
        let mut cq = NvmeQueue::new(qid, qsize, sqe.dptr_prp1);
        cq.iv = iv;
        cq.ien = ien;
        io_cq[(qid - 1) as usize] = Some(cq);

        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Create I/O Submission Queue
    fn cmd_create_io_sq(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let qid = (sqe.cdw10 & 0xFFFF) as u16;
        let qsize = ((sqe.cdw10 >> 16) & 0xFFFF) as u16 + 1;
        let cqid = (sqe.cdw11 & 0xFFFF) as u16;

        if qid == 0 || qid as usize > NVME_MAX_IO_QUEUES as usize {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueId, true);
        }

        if !(2..=NVME_MAX_QUEUE_ENTRIES).contains(&qsize) {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueSize, true);
        }

        // Verify CQ exists
        {
            let io_cq = self.io_cq.read();
            if cqid == 0
                || cqid as usize > NVME_MAX_IO_QUEUES as usize
                || io_cq[(cqid - 1) as usize].is_none()
            {
                return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueId, true);
            }
        }

        let mut io_sq = self.io_sq.write();
        let mut sq = NvmeQueue::new(qid, qsize, sqe.dptr_prp1);
        sq.cq_id = cqid;
        io_sq[(qid - 1) as usize] = Some(sq);

        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Delete I/O Completion Queue
    fn cmd_delete_io_cq(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let qid = (sqe.cdw10 & 0xFFFF) as u16;

        if qid == 0 || qid as usize > NVME_MAX_IO_QUEUES as usize {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueId, true);
        }

        let mut io_cq = self.io_cq.write();
        io_cq[(qid - 1) as usize] = None;

        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Delete I/O Submission Queue
    fn cmd_delete_io_sq(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let qid = (sqe.cdw10 & 0xFFFF) as u16;

        if qid == 0 || qid as usize > NVME_MAX_IO_QUEUES as usize {
            return CompletionQueueEntry::new(cid, 0, 0, StatusCode::InvalidQueueId, true);
        }

        let mut io_sq = self.io_sq.write();
        io_sq[(qid - 1) as usize] = None;

        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Get Features
    fn cmd_get_features(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let fid = sqe.cdw10 & 0xFF;

        // Return default/dummy values
        let mut cqe = CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true);

        if fid == 0x07 {
            cqe.dw0 = NVME_MAX_IO_QUEUES as u32; // Number of Queues
        }

        cqe
    }

    /// Set Features
    fn cmd_set_features(&self, sqe: &SubmissionQueueEntry, cid: u16) -> CompletionQueueEntry {
        let fid = sqe.cdw10 & 0xFF;

        // Accept but mostly ignore
        CompletionQueueEntry::new(cid, 0, 0, StatusCode::Success, true)
    }

    /// Read command
    fn cmd_read(&self, sqe: &SubmissionQueueEntry, sq_id: u16, cid: u16) -> CompletionQueueEntry {
        let slba = ((sqe.cdw11 as u64) << 32) | (sqe.cdw10 as u64);
        let nlb = (sqe.cdw12 & 0xFFFF) as u64 + 1; // Number of logical blocks

        let offset = slba * NVME_BLOCK_SIZE as u64;
        let length = nlb * NVME_BLOCK_SIZE as u64;

        let storage = self.storage.read();
        if offset + length > storage.len() as u64 {
            return CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::InvalidField, true);
        }

        // In real implementation, data would be DMA'd to PRP1/PRP2
        CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::Success, true)
    }

    /// Write command
    fn cmd_write(&self, sqe: &SubmissionQueueEntry, sq_id: u16, cid: u16) -> CompletionQueueEntry {
        let slba = ((sqe.cdw11 as u64) << 32) | (sqe.cdw10 as u64);
        let nlb = (sqe.cdw12 & 0xFFFF) as u64 + 1;

        let offset = slba * NVME_BLOCK_SIZE as u64;
        let length = nlb * NVME_BLOCK_SIZE as u64;

        let storage = self.storage.read();
        if offset + length > storage.len() as u64 {
            return CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::InvalidField, true);
        }

        // In real implementation, data would be DMA'd from PRP1/PRP2
        CompletionQueueEntry::new(cid, sq_id, 0, StatusCode::Success, true)
    }

    /// Get identify controller data
    pub fn get_identify_controller(&self) -> &IdentifyController {
        &self.identify_ctrl
    }

    /// Get identify namespace data
    pub fn get_identify_namespace(&self) -> &IdentifyNamespace {
        &self.identify_ns
    }

    /// Check if controller is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Check if controller is ready
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Relaxed)
    }

    /// Get storage size
    pub fn storage_size(&self) -> u64 {
        self.storage.read().len() as u64
    }

    /// Read from storage (for testing)
    pub fn read_storage(&self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let storage = self.storage.read();
        let end = offset as usize + buf.len();
        if end > storage.len() {
            return Err(crate::Error::Memory("Read past end of storage".to_string()));
        }
        buf.copy_from_slice(&storage[offset as usize..end]);
        Ok(())
    }

    /// Write to storage (for testing)
    pub fn write_storage(&self, offset: u64, data: &[u8]) -> Result<()> {
        let mut storage = self.storage.write();
        let end = offset as usize + data.len();
        if end > storage.len() {
            return Err(crate::Error::Memory(
                "Write past end of storage".to_string(),
            ));
        }
        storage[offset as usize..end].copy_from_slice(data);
        Ok(())
    }
}

impl std::fmt::Debug for NvmeController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NvmeController")
            .field("enabled", &self.enabled.load(Ordering::Relaxed))
            .field("ready", &self.ready.load(Ordering::Relaxed))
            .field("storage_size", &self.storage.read().len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nvme_controller_creation() {
        let nvme = NvmeController::new(1024 * 1024); // 1MB
        assert!(!nvme.is_enabled());
        assert!(!nvme.is_ready());
        assert_eq!(nvme.storage_size(), 1024 * 1024);
    }

    #[test]
    fn test_capabilities() {
        let nvme = NvmeController::new(1024 * 1024);
        let cap = nvme.capabilities();

        // Check MQES
        assert_eq!(cap & cap::MQES_MASK, (NVME_MAX_QUEUE_ENTRIES - 1) as u64);
        // Check CQR
        assert!(cap & cap::CQR != 0);
        // Check CSS NVM
        assert!(cap & cap::CSS_NVM != 0);
    }

    #[test]
    fn test_register_read() {
        let nvme = NvmeController::new(1024 * 1024);

        assert_eq!(nvme.read_reg(regs::VS, 4), 0x00010400);
        assert_eq!(nvme.read_reg(regs::CSTS, 4), 0);
    }

    #[test]
    fn test_enable_disable() {
        let nvme = NvmeController::new(1024 * 1024);

        // Set up admin queues
        nvme.write_reg(regs::AQA, 0x001F001F, 4); // 32 entries each
        nvme.write_reg(regs::ASQ, 0x1000, 8);
        nvme.write_reg(regs::ACQ, 0x2000, 8);

        // Enable
        nvme.write_reg(regs::CC, cc::EN as u64, 4);

        assert!(nvme.is_enabled());
        assert!(nvme.is_ready());
        assert!(nvme.read_reg(regs::CSTS, 4) & csts::RDY as u64 != 0);

        // Disable
        nvme.write_reg(regs::CC, 0, 4);

        assert!(!nvme.is_enabled());
        assert!(!nvme.is_ready());
    }

    #[test]
    fn test_submission_queue_entry() {
        let mut sqe = SubmissionQueueEntry::default();
        sqe.cdw0 = 0x0042_0006; // CID=0x42, opcode=0x06 (Identify)

        assert_eq!(sqe.opcode(), 0x06);
        assert_eq!(sqe.cid(), 0x42);
    }

    #[test]
    fn test_completion_queue_entry() {
        let cqe = CompletionQueueEntry::new(0x42, 1, 5, StatusCode::Success, true);

        assert_eq!(cqe.cid(), 0x42);
        assert!(cqe.phase());
        assert_eq!(cqe.status(), 0);
    }

    #[test]
    fn test_nvme_queue() {
        let mut q = NvmeQueue::new(1, 16, 0x1000);

        assert!(q.is_empty());
        assert!(!q.is_full());
        assert_eq!(q.count(), 0);

        q.tail = 5;
        assert_eq!(q.count(), 5);
        assert!(!q.is_empty());

        q.advance_head();
        assert_eq!(q.head, 1);
        assert_eq!(q.count(), 4);
    }

    #[test]
    fn test_queue_wrap() {
        let mut q = NvmeQueue::new(0, 4, 0x1000);
        q.tail = 3;

        q.advance_tail();
        assert_eq!(q.tail, 0);
        assert!(!q.phase); // Phase toggles on wrap
    }

    #[test]
    fn test_identify_controller() {
        let id = IdentifyController::default();
        let bytes = id.to_bytes();

        assert_eq!(bytes.len(), 4096);
        assert_eq!(u16::from_le_bytes([bytes[0], bytes[1]]), 0x8086);
    }

    #[test]
    fn test_identify_namespace() {
        let ns = IdentifyNamespace::new(1024 * 1024 * 1024); // 1GB

        assert_eq!(ns.nsze, 1024 * 1024 * 1024 / NVME_BLOCK_SIZE as u64);

        let bytes = ns.to_bytes();
        assert_eq!(bytes.len(), 4096);
    }

    #[test]
    fn test_admin_opcode_from() {
        assert_eq!(AdminOpcode::from(0x06), AdminOpcode::Identify);
        assert_eq!(AdminOpcode::from(0x01), AdminOpcode::CreateIoSq);
        assert_eq!(AdminOpcode::from(0xFF), AdminOpcode::Unknown);
    }

    #[test]
    fn test_io_opcode_from() {
        assert_eq!(IoOpcode::from(0x00), IoOpcode::Flush);
        assert_eq!(IoOpcode::from(0x01), IoOpcode::Write);
        assert_eq!(IoOpcode::from(0x02), IoOpcode::Read);
    }

    #[test]
    fn test_create_io_queues() {
        let nvme = NvmeController::new(1024 * 1024);

        // Enable controller first
        nvme.write_reg(regs::AQA, 0x001F001F, 4);
        nvme.write_reg(regs::ASQ, 0x1000, 8);
        nvme.write_reg(regs::ACQ, 0x2000, 8);
        nvme.write_reg(regs::CC, cc::EN as u64, 4);

        // Create CQ
        let mut sqe = SubmissionQueueEntry::default();
        sqe.cdw0 = 0x0001_0005; // CID=1, opcode=5 (Create I/O CQ)
        sqe.cdw10 = 1 | (31 << 16); // QID=1, QSIZE=32
        sqe.dptr_prp1 = 0x3000;

        let cqe = nvme.process_command(&sqe, 0);
        assert_eq!(cqe.status(), 0);

        // Create SQ
        sqe.cdw0 = 0x0002_0001; // CID=2, opcode=1 (Create I/O SQ)
        sqe.cdw10 = 1 | (31 << 16); // QID=1, QSIZE=32
        sqe.cdw11 = 1; // CQID=1
        sqe.dptr_prp1 = 0x4000;

        let cqe = nvme.process_command(&sqe, 0);
        assert_eq!(cqe.status(), 0);
    }

    #[test]
    fn test_invalid_queue_id() {
        let nvme = NvmeController::new(1024 * 1024);
        nvme.write_reg(regs::AQA, 0x001F001F, 4);
        nvme.write_reg(regs::ASQ, 0x1000, 8);
        nvme.write_reg(regs::ACQ, 0x2000, 8);
        nvme.write_reg(regs::CC, cc::EN as u64, 4);

        // Try to create SQ with QID 0 (invalid)
        let mut sqe = SubmissionQueueEntry::default();
        sqe.cdw0 = 0x0001_0001;
        sqe.cdw10 = 0 | (31 << 16); // QID=0 is invalid for I/O queue

        let cqe = nvme.process_command(&sqe, 0);
        assert_eq!(cqe.status(), StatusCode::InvalidQueueId as u16);
    }

    #[test]
    fn test_storage_read_write() {
        let nvme = NvmeController::new(4096);

        let data = [0x42u8; 512];
        nvme.write_storage(0, &data).unwrap();

        let mut buf = [0u8; 512];
        nvme.read_storage(0, &mut buf).unwrap();

        assert_eq!(buf, data);
    }

    #[test]
    fn test_storage_bounds() {
        let nvme = NvmeController::new(1024);

        let data = [0u8; 512];
        assert!(nvme.write_storage(1024, &data).is_err());

        let mut buf = [0u8; 512];
        assert!(nvme.read_storage(1024, &mut buf).is_err());
    }

    #[test]
    fn test_doorbell_sq_tail() {
        let nvme = NvmeController::new(1024 * 1024);

        // Enable and create queues
        nvme.write_reg(regs::AQA, 0x001F001F, 4);
        nvme.write_reg(regs::ASQ, 0x1000, 8);
        nvme.write_reg(regs::ACQ, 0x2000, 8);
        nvme.write_reg(regs::CC, cc::EN as u64, 4);

        // Update admin SQ tail via doorbell
        nvme.write_reg(regs::SQ_TDBL_BASE, 5, 4);

        let admin_sq = nvme.admin_sq.read();
        assert_eq!(admin_sq.as_ref().unwrap().tail, 5);
    }
}
