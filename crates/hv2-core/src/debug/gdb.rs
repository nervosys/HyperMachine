//! GDB Remote Serial Protocol Implementation
//!
//! This module provides a GDB stub for debugging guest VMs using the
//! GDB remote serial protocol (RSP).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use parking_lot::RwLock;

/// GDB packet state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketState {
    /// Waiting for packet start '$'
    WaitingStart,
    /// Reading packet data
    ReadingData,
    /// Reading checksum (first digit)
    ReadingChecksum1,
    /// Reading checksum (second digit)
    ReadingChecksum2,
}

/// GDB stop reason
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// No stop (running)
    None,
    /// Hit breakpoint
    Breakpoint,
    /// Single step completed
    Step,
    /// Signal received
    Signal(u8),
    /// Watchpoint triggered
    Watchpoint(u64),
    /// VM exited
    Exited(u8),
    /// VM terminated
    Terminated(u8),
}

impl StopReason {
    /// Convert to GDB stop reply
    pub fn to_reply(&self) -> String {
        match self {
            Self::None => String::new(),
            Self::Breakpoint => "S05".to_string(), // SIGTRAP
            Self::Step => "S05".to_string(),
            Self::Signal(sig) => format!("S{:02x}", sig),
            Self::Watchpoint(addr) => format!("T05watch:{:x};", addr),
            Self::Exited(code) => format!("W{:02x}", code),
            Self::Terminated(sig) => format!("X{:02x}", sig),
        }
    }
}

/// Breakpoint type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakpointType {
    /// Software breakpoint (INT3)
    Software,
    /// Hardware execution breakpoint
    HardwareExec,
    /// Hardware write watchpoint
    WriteWatch,
    /// Hardware read watchpoint
    ReadWatch,
    /// Hardware access watchpoint
    AccessWatch,
}

impl BreakpointType {
    /// Parse from GDB type number
    pub fn from_gdb(type_num: u8) -> Option<Self> {
        match type_num {
            0 => Some(Self::Software),
            1 => Some(Self::HardwareExec),
            2 => Some(Self::WriteWatch),
            3 => Some(Self::ReadWatch),
            4 => Some(Self::AccessWatch),
            _ => None,
        }
    }
}

/// Breakpoint
#[derive(Debug, Clone)]
pub struct Breakpoint {
    /// Breakpoint ID
    pub id: u64,
    /// Breakpoint type
    pub bp_type: BreakpointType,
    /// Address
    pub address: u64,
    /// Size (for watchpoints)
    pub size: u64,
    /// Enabled flag
    pub enabled: bool,
    /// Hit count
    pub hit_count: u64,
    /// Original byte (for software breakpoints)
    pub original_byte: Option<u8>,
}

impl Breakpoint {
    /// Create new breakpoint
    pub fn new(id: u64, bp_type: BreakpointType, address: u64, size: u64) -> Self {
        Self {
            id,
            bp_type,
            address,
            size,
            enabled: true,
            hit_count: 0,
            original_byte: None,
        }
    }

    /// Check if address is in watchpoint range
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.address && addr < self.address + self.size
    }
}

/// x86-64 register index for GDB
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GdbRegister {
    Rax = 0,
    Rbx = 1,
    Rcx = 2,
    Rdx = 3,
    Rsi = 4,
    Rdi = 5,
    Rbp = 6,
    Rsp = 7,
    R8 = 8,
    R9 = 9,
    R10 = 10,
    R11 = 11,
    R12 = 12,
    R13 = 13,
    R14 = 14,
    R15 = 15,
    Rip = 16,
    Eflags = 17,
    Cs = 18,
    Ss = 19,
    Ds = 20,
    Es = 21,
    Fs = 22,
    Gs = 23,
}

impl GdbRegister {
    /// Get register from index
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Rax),
            1 => Some(Self::Rbx),
            2 => Some(Self::Rcx),
            3 => Some(Self::Rdx),
            4 => Some(Self::Rsi),
            5 => Some(Self::Rdi),
            6 => Some(Self::Rbp),
            7 => Some(Self::Rsp),
            8 => Some(Self::R8),
            9 => Some(Self::R9),
            10 => Some(Self::R10),
            11 => Some(Self::R11),
            12 => Some(Self::R12),
            13 => Some(Self::R13),
            14 => Some(Self::R14),
            15 => Some(Self::R15),
            16 => Some(Self::Rip),
            17 => Some(Self::Eflags),
            18 => Some(Self::Cs),
            19 => Some(Self::Ss),
            20 => Some(Self::Ds),
            21 => Some(Self::Es),
            22 => Some(Self::Fs),
            23 => Some(Self::Gs),
            _ => None,
        }
    }

    /// Get register size in bytes
    pub fn size(&self) -> usize {
        match self {
            Self::Cs | Self::Ss | Self::Ds | Self::Es | Self::Fs | Self::Gs => 4,
            Self::Eflags => 4,
            _ => 8,
        }
    }

    /// Total number of registers
    pub const COUNT: usize = 24;
}

/// CPU register state for GDB
#[derive(Debug, Clone, Default)]
pub struct GdbRegisters {
    /// General purpose registers
    pub gprs: [u64; 16],
    /// Instruction pointer
    pub rip: u64,
    /// Flags register
    pub rflags: u64,
    /// Segment registers
    pub cs: u32,
    pub ss: u32,
    pub ds: u32,
    pub es: u32,
    pub fs: u32,
    pub gs: u32,
}

impl GdbRegisters {
    /// Get register value by index
    pub fn get(&self, reg: GdbRegister) -> u64 {
        match reg {
            GdbRegister::Rax => self.gprs[0],
            GdbRegister::Rbx => self.gprs[1],
            GdbRegister::Rcx => self.gprs[2],
            GdbRegister::Rdx => self.gprs[3],
            GdbRegister::Rsi => self.gprs[4],
            GdbRegister::Rdi => self.gprs[5],
            GdbRegister::Rbp => self.gprs[6],
            GdbRegister::Rsp => self.gprs[7],
            GdbRegister::R8 => self.gprs[8],
            GdbRegister::R9 => self.gprs[9],
            GdbRegister::R10 => self.gprs[10],
            GdbRegister::R11 => self.gprs[11],
            GdbRegister::R12 => self.gprs[12],
            GdbRegister::R13 => self.gprs[13],
            GdbRegister::R14 => self.gprs[14],
            GdbRegister::R15 => self.gprs[15],
            GdbRegister::Rip => self.rip,
            GdbRegister::Eflags => self.rflags,
            GdbRegister::Cs => self.cs as u64,
            GdbRegister::Ss => self.ss as u64,
            GdbRegister::Ds => self.ds as u64,
            GdbRegister::Es => self.es as u64,
            GdbRegister::Fs => self.fs as u64,
            GdbRegister::Gs => self.gs as u64,
        }
    }

    /// Set register value by index
    pub fn set(&mut self, reg: GdbRegister, value: u64) {
        match reg {
            GdbRegister::Rax => self.gprs[0] = value,
            GdbRegister::Rbx => self.gprs[1] = value,
            GdbRegister::Rcx => self.gprs[2] = value,
            GdbRegister::Rdx => self.gprs[3] = value,
            GdbRegister::Rsi => self.gprs[4] = value,
            GdbRegister::Rdi => self.gprs[5] = value,
            GdbRegister::Rbp => self.gprs[6] = value,
            GdbRegister::Rsp => self.gprs[7] = value,
            GdbRegister::R8 => self.gprs[8] = value,
            GdbRegister::R9 => self.gprs[9] = value,
            GdbRegister::R10 => self.gprs[10] = value,
            GdbRegister::R11 => self.gprs[11] = value,
            GdbRegister::R12 => self.gprs[12] = value,
            GdbRegister::R13 => self.gprs[13] = value,
            GdbRegister::R14 => self.gprs[14] = value,
            GdbRegister::R15 => self.gprs[15] = value,
            GdbRegister::Rip => self.rip = value,
            GdbRegister::Eflags => self.rflags = value,
            GdbRegister::Cs => self.cs = value as u32,
            GdbRegister::Ss => self.ss = value as u32,
            GdbRegister::Ds => self.ds = value as u32,
            GdbRegister::Es => self.es = value as u32,
            GdbRegister::Fs => self.fs = value as u32,
            GdbRegister::Gs => self.gs = value as u32,
        }
    }

    /// Serialize all registers to hex string (GDB 'g' packet response)
    pub fn to_hex(&self) -> String {
        let mut result = String::with_capacity(GdbRegister::COUNT * 16);

        // GPRs (64-bit each, little endian)
        for i in 0..16 {
            result.push_str(&format!("{:016x}", self.gprs[i].swap_bytes()));
        }

        // RIP
        result.push_str(&format!("{:016x}", self.rip.swap_bytes()));

        // EFLAGS (32-bit)
        result.push_str(&format!("{:08x}", (self.rflags as u32).swap_bytes()));

        // Segment registers (32-bit each)
        result.push_str(&format!("{:08x}", self.cs.swap_bytes()));
        result.push_str(&format!("{:08x}", self.ss.swap_bytes()));
        result.push_str(&format!("{:08x}", self.ds.swap_bytes()));
        result.push_str(&format!("{:08x}", self.es.swap_bytes()));
        result.push_str(&format!("{:08x}", self.fs.swap_bytes()));
        result.push_str(&format!("{:08x}", self.gs.swap_bytes()));

        result
    }
    /// Deserialize registers from hex string (GDB 'G' packet data)
    ///
    /// Parses the same format produced by `to_hex()`:
    /// - 16 GPRs (64-bit each, little-endian byte order)
    /// - RIP (64-bit)
    /// - EFLAGS (32-bit)
    /// - 6 segment registers (32-bit each: CS, SS, DS, ES, FS, GS)
    pub fn from_hex(hex: &[u8]) -> Option<Self> {
        // Minimum: 16 GPRs * 16 + RIP * 16 + EFLAGS * 8 + 6 segs * 8 = 328
        if hex.len() < 328 {
            return None;
        }

        let bytes = parse_hex(hex)?;
        let mut regs = GdbRegisters::default();
        let mut offset = 0;

        // 16 GPRs (8 bytes each, byte-swapped)
        for i in 0..16 {
            let val = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
            regs.gprs[i] = val.swap_bytes();
            offset += 8;
        }

        // RIP (8 bytes, byte-swapped)
        let val = u64::from_be_bytes(bytes[offset..offset + 8].try_into().ok()?);
        regs.rip = val.swap_bytes();
        offset += 8;

        // EFLAGS (4 bytes, byte-swapped)
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.rflags = val.swap_bytes() as u64;
        offset += 4;

        // Segment registers (4 bytes each, byte-swapped)
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.cs = val.swap_bytes();
        offset += 4;
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.ss = val.swap_bytes();
        offset += 4;
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.ds = val.swap_bytes();
        offset += 4;
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.es = val.swap_bytes();
        offset += 4;
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.fs = val.swap_bytes();
        offset += 4;
        let val = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?);
        regs.gs = val.swap_bytes();

        Some(regs)
    }
}

/// GDB target operations trait
pub trait GdbTarget {
    /// Read memory from guest
    fn read_memory(&self, addr: u64, size: usize) -> Option<Vec<u8>>;

    /// Write memory to guest
    fn write_memory(&mut self, addr: u64, data: &[u8]) -> bool;

    /// Get CPU registers
    fn get_registers(&self) -> GdbRegisters;

    /// Set CPU registers
    fn set_registers(&mut self, regs: &GdbRegisters);

    /// Continue execution
    fn continue_execution(&mut self);

    /// Single step
    fn single_step(&mut self);

    /// Stop execution
    fn stop(&mut self);

    /// Check if stopped
    fn is_stopped(&self) -> bool;

    /// Get stop reason
    fn stop_reason(&self) -> StopReason;
}

/// GDB stub error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GdbError {
    /// Invalid packet
    #[error("Invalid packet: {0}")]
    InvalidPacket(String),
    /// Checksum error
    #[error("Checksum error")]
    ChecksumError,
    /// Unknown command
    #[error("Unknown command: {0}")]
    UnknownCommand(char),
    /// Invalid address
    #[error("Invalid address: 0x{0:x}")]
    InvalidAddress(u64),
    /// Memory access failed
    #[error("Memory error at 0x{0:x} size {1}")]
    MemoryError(u64, usize),
    /// Register access failed
    #[error("Register error: {0}")]
    RegisterError(usize),
    /// Connection closed
    #[error("Connection closed")]
    ConnectionClosed,
}

/// Result type for GDB operations
pub type GdbResult<T> = Result<T, GdbError>;

/// GDB packet parser
#[derive(Debug)]
pub struct PacketParser {
    /// Current state
    state: PacketState,
    /// Accumulated data
    data: Vec<u8>,
    /// Checksum accumulator
    checksum: u8,
    /// Received checksum
    received_checksum: u8,
}

impl PacketParser {
    /// Create new parser
    pub fn new() -> Self {
        Self {
            state: PacketState::WaitingStart,
            data: Vec::new(),
            checksum: 0,
            received_checksum: 0,
        }
    }

    /// Reset parser state
    pub fn reset(&mut self) {
        self.state = PacketState::WaitingStart;
        self.data.clear();
        self.checksum = 0;
        self.received_checksum = 0;
    }

    /// Feed a byte to the parser
    /// Returns Some(data) when a complete packet is received
    pub fn feed(&mut self, byte: u8) -> Option<GdbResult<Vec<u8>>> {
        match self.state {
            PacketState::WaitingStart => {
                if byte == b'$' {
                    self.data.clear();
                    self.checksum = 0;
                    self.state = PacketState::ReadingData;
                }
                // Ignore other bytes (like '+' ack)
                None
            }
            PacketState::ReadingData => {
                if byte == b'#' {
                    self.state = PacketState::ReadingChecksum1;
                } else {
                    self.data.push(byte);
                    self.checksum = self.checksum.wrapping_add(byte);
                }
                None
            }
            PacketState::ReadingChecksum1 => {
                self.received_checksum = hex_digit(byte).unwrap_or(0) << 4;
                self.state = PacketState::ReadingChecksum2;
                None
            }
            PacketState::ReadingChecksum2 => {
                self.received_checksum |= hex_digit(byte).unwrap_or(0);
                self.state = PacketState::WaitingStart;

                if self.checksum == self.received_checksum {
                    Some(Ok(std::mem::take(&mut self.data)))
                } else {
                    Some(Err(GdbError::ChecksumError))
                }
            }
        }
    }
}

impl Default for PacketParser {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse hex digit
fn hex_digit(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Parse hex string to bytes
fn parse_hex(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }

    let mut result = Vec::with_capacity(s.len() / 2);
    for chunk in s.chunks(2) {
        let high = hex_digit(chunk[0])?;
        let low = hex_digit(chunk[1])?;
        result.push((high << 4) | low);
    }
    Some(result)
}

/// Parse hex string to u64
fn parse_hex_u64(s: &[u8]) -> Option<u64> {
    let mut result: u64 = 0;
    for &byte in s {
        let digit = hex_digit(byte)?;
        result = result.checked_mul(16)?.checked_add(digit as u64)?;
    }
    Some(result)
}

/// Encode bytes to hex string
fn encode_hex(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len() * 2);
    for byte in data {
        result.push_str(&format!("{:02x}", byte));
    }
    result
}

/// GDB stub
#[derive(Debug)]
pub struct GdbStub {
    /// Breakpoints
    breakpoints: RwLock<HashMap<u64, Breakpoint>>,
    /// Next breakpoint ID
    next_bp_id: AtomicU64,
    /// Single step mode
    single_step: AtomicBool,
    /// Connected flag
    connected: AtomicBool,
    /// No-ack mode
    no_ack_mode: AtomicBool,
    /// Packet parser
    parser: RwLock<PacketParser>,
    /// Supported features
    features: Vec<String>,
}

impl GdbStub {
    /// Create new GDB stub
    pub fn new() -> Self {
        Self {
            breakpoints: RwLock::new(HashMap::new()),
            next_bp_id: AtomicU64::new(1),
            single_step: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            no_ack_mode: AtomicBool::new(false),
            parser: RwLock::new(PacketParser::new()),
            features: vec![
                "PacketSize=4096".to_string(),
                "swbreak+".to_string(),
                "hwbreak+".to_string(),
                "qXfer:features:read+".to_string(),
            ],
        }
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Set connected state
    pub fn set_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Release);
    }

    /// Check if in single step mode
    pub fn is_single_step(&self) -> bool {
        self.single_step.load(Ordering::Acquire)
    }

    /// Set single step mode
    pub fn set_single_step(&self, step: bool) {
        self.single_step.store(step, Ordering::Release);
    }

    /// Add breakpoint
    pub fn add_breakpoint(&self, bp_type: BreakpointType, addr: u64, size: u64) -> u64 {
        let id = self.next_bp_id.fetch_add(1, Ordering::SeqCst);
        let bp = Breakpoint::new(id, bp_type, addr, size);
        self.breakpoints.write().insert(addr, bp);
        id
    }

    /// Remove breakpoint
    pub fn remove_breakpoint(&self, addr: u64) -> Option<Breakpoint> {
        self.breakpoints.write().remove(&addr)
    }

    /// Check if address has breakpoint
    pub fn has_breakpoint(&self, addr: u64) -> bool {
        self.breakpoints.read().contains_key(&addr)
    }

    /// Get breakpoint at address
    pub fn get_breakpoint(&self, addr: u64) -> Option<Breakpoint> {
        self.breakpoints.read().get(&addr).cloned()
    }

    /// Check watchpoint hit
    pub fn check_watchpoint(&self, addr: u64, is_write: bool) -> Option<Breakpoint> {
        let bps = self.breakpoints.read();
        for bp in bps.values() {
            if !bp.enabled {
                continue;
            }
            if !bp.contains(addr) {
                continue;
            }
            match bp.bp_type {
                BreakpointType::AccessWatch => return Some(bp.clone()),
                BreakpointType::WriteWatch if is_write => return Some(bp.clone()),
                BreakpointType::ReadWatch if !is_write => return Some(bp.clone()),
                _ => {}
            }
        }
        None
    }

    /// List all breakpoints
    pub fn list_breakpoints(&self) -> Vec<Breakpoint> {
        self.breakpoints.read().values().cloned().collect()
    }

    /// Clear all breakpoints
    pub fn clear_breakpoints(&self) {
        self.breakpoints.write().clear();
    }

    /// Format packet for sending
    pub fn format_packet(&self, data: &str) -> Vec<u8> {
        let checksum: u8 = data.bytes().fold(0u8, |acc, b| acc.wrapping_add(b));
        format!("${}#{:02x}", data, checksum).into_bytes()
    }

    /// Format OK response
    pub fn ok_response(&self) -> Vec<u8> {
        self.format_packet("OK")
    }

    /// Format error response
    pub fn error_response(&self, code: u8) -> Vec<u8> {
        self.format_packet(&format!("E{:02x}", code))
    }

    /// Format empty response (unsupported)
    pub fn empty_response(&self) -> Vec<u8> {
        self.format_packet("")
    }

    /// Process incoming data
    pub fn process_data(&self, data: &[u8]) -> Vec<GdbResult<Vec<u8>>> {
        let mut parser = self.parser.write();
        let mut results = Vec::new();

        for &byte in data {
            if let Some(result) = parser.feed(byte) {
                results.push(result);
            }
        }

        results
    }

    /// Handle a GDB packet
    pub fn handle_packet<T: GdbTarget>(&self, packet: &[u8], target: &mut T) -> Vec<u8> {
        if packet.is_empty() {
            return self.empty_response();
        }

        let cmd = packet[0] as char;
        let args = &packet[1..];

        match cmd {
            '?' => {
                // Stop reason query
                let reason = target.stop_reason();
                self.format_packet(&reason.to_reply())
            }
            'g' => {
                // Read all registers
                let regs = target.get_registers();
                self.format_packet(&regs.to_hex())
            }
            'G' => {
                // Write all registers
                if let Some(regs) = GdbRegisters::from_hex(args) {
                    target.set_registers(&regs);
                    self.ok_response()
                } else {
                    self.error_response(0x01)
                }
            }
            'p' => {
                // Read single register
                if let Some(reg_num) = parse_hex_u64(args) {
                    if let Some(reg) = GdbRegister::from_index(reg_num as usize) {
                        let regs = target.get_registers();
                        let value = regs.get(reg);
                        let hex = if reg.size() == 4 {
                            format!("{:08x}", (value as u32).swap_bytes())
                        } else {
                            format!("{:016x}", value.swap_bytes())
                        };
                        self.format_packet(&hex)
                    } else {
                        self.error_response(0x01)
                    }
                } else {
                    self.error_response(0x01)
                }
            }
            'P' => {
                // Write single register
                if let Some(eq_pos) = args.iter().position(|&b| b == b'=') {
                    if let (Some(reg_num), Some(value_bytes)) = (
                        parse_hex_u64(&args[..eq_pos]),
                        parse_hex(&args[eq_pos + 1..]),
                    ) {
                        if let Some(reg) = GdbRegister::from_index(reg_num as usize) {
                            let mut regs = target.get_registers();
                            let value = if value_bytes.len() >= 8 {
                                u64::from_le_bytes(value_bytes[..8].try_into().expect("slice is exactly 8 bytes"))
                            } else if value_bytes.len() >= 4 {
                                u32::from_le_bytes(value_bytes[..4].try_into().expect("slice is exactly 4 bytes")) as u64
                            } else {
                                0
                            };
                            regs.set(reg, value);
                            target.set_registers(&regs);
                            return self.ok_response();
                        }
                    }
                }
                self.error_response(0x01)
            }
            'm' => {
                // Read memory
                if let Some(comma_pos) = args.iter().position(|&b| b == b',') {
                    if let (Some(addr), Some(len)) = (
                        parse_hex_u64(&args[..comma_pos]),
                        parse_hex_u64(&args[comma_pos + 1..]),
                    ) {
                        if let Some(data) = target.read_memory(addr, len as usize) {
                            return self.format_packet(&encode_hex(&data));
                        }
                    }
                }
                self.error_response(0x01)
            }
            'M' => {
                // Write memory
                if let Some(comma_pos) = args.iter().position(|&b| b == b',') {
                    if let Some(colon_pos) = args.iter().position(|&b| b == b':') {
                        if let (Some(addr), Some(_len), Some(data)) = (
                            parse_hex_u64(&args[..comma_pos]),
                            parse_hex_u64(&args[comma_pos + 1..colon_pos]),
                            parse_hex(&args[colon_pos + 1..]),
                        ) {
                            if target.write_memory(addr, &data) {
                                return self.ok_response();
                            }
                        }
                    }
                }
                self.error_response(0x01)
            }
            'c' => {
                // Continue
                self.set_single_step(false);
                target.continue_execution();
                // Response sent when target stops
                Vec::new()
            }
            's' => {
                // Single step
                self.set_single_step(true);
                target.single_step();
                // Response sent when target stops
                Vec::new()
            }
            'Z' => {
                // Insert breakpoint
                self.handle_breakpoint_insert(args, target)
            }
            'z' => {
                // Remove breakpoint
                self.handle_breakpoint_remove(args, target)
            }
            'q' => {
                // Query
                self.handle_query(args, target)
            }
            'Q' => {
                // Set
                self.handle_set(args)
            }
            'H' => {
                // Set thread (we're single-threaded)
                self.ok_response()
            }
            'T' => {
                // Thread alive query (always alive)
                self.ok_response()
            }
            'k' => {
                // Kill
                target.stop();
                self.set_connected(false);
                Vec::new()
            }
            'D' => {
                // Detach
                self.set_connected(false);
                self.ok_response()
            }
            _ => self.empty_response(),
        }
    }

    /// Handle breakpoint insert
    fn handle_breakpoint_insert<T: GdbTarget>(&self, args: &[u8], _target: &mut T) -> Vec<u8> {
        // Format: type,addr,kind
        let parts: Vec<&[u8]> = args.split(|&b| b == b',').collect();
        if parts.len() >= 3 {
            if let (Some(bp_type_num), Some(addr), Some(size)) = (
                parse_hex_u64(parts[0]),
                parse_hex_u64(parts[1]),
                parse_hex_u64(parts[2]),
            ) {
                if let Some(bp_type) = BreakpointType::from_gdb(bp_type_num as u8) {
                    self.add_breakpoint(bp_type, addr, size.max(1));
                    return self.ok_response();
                }
            }
        }
        self.error_response(0x01)
    }

    /// Handle breakpoint remove
    fn handle_breakpoint_remove<T: GdbTarget>(&self, args: &[u8], _target: &mut T) -> Vec<u8> {
        let parts: Vec<&[u8]> = args.split(|&b| b == b',').collect();
        if parts.len() >= 2 {
            if let Some(addr) = parse_hex_u64(parts[1]) {
                if self.remove_breakpoint(addr).is_some() {
                    return self.ok_response();
                }
            }
        }
        self.error_response(0x01)
    }

    /// Handle query packet
    fn handle_query<T: GdbTarget>(&self, args: &[u8], _target: &T) -> Vec<u8> {
        let query = String::from_utf8_lossy(args);

        if query.starts_with("Supported") {
            let features = self.features.join(";");
            return self.format_packet(&features);
        }

        if query == "Attached" {
            return self.format_packet("1");
        }

        if query.starts_with("Xfer:features:read:target.xml:") {
            // Target description
            let xml = r#"<?xml version="1.0"?>
<!DOCTYPE target SYSTEM "gdb-target.dtd">
<target version="1.0">
<architecture>i386:x86-64</architecture>
</target>"#;
            return self.format_packet(&format!("l{}", xml));
        }

        if query == "fThreadInfo" {
            return self.format_packet("m1");
        }

        if query == "sThreadInfo" {
            return self.format_packet("l");
        }

        if query == "C" {
            return self.format_packet("QC1");
        }

        self.empty_response()
    }

    /// Handle set packet
    fn handle_set(&self, args: &[u8]) -> Vec<u8> {
        let setting = String::from_utf8_lossy(args);

        if setting == "StartNoAckMode" {
            self.no_ack_mode.store(true, Ordering::Release);
            return self.ok_response();
        }

        self.empty_response()
    }

    /// Get statistics
    pub fn stats(&self) -> GdbStats {
        GdbStats {
            connected: self.is_connected(),
            single_step: self.is_single_step(),
            breakpoint_count: self.breakpoints.read().len(),
            no_ack_mode: self.no_ack_mode.load(Ordering::Acquire),
        }
    }
}

impl Default for GdbStub {
    fn default() -> Self {
        Self::new()
    }
}

/// GDB stub statistics
#[derive(Debug, Clone)]
pub struct GdbStats {
    /// Connected flag
    pub connected: bool,
    /// Single step mode
    pub single_step: bool,
    /// Number of breakpoints
    pub breakpoint_count: usize,
    /// No-ack mode enabled
    pub no_ack_mode: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTarget {
        regs: GdbRegisters,
        memory: HashMap<u64, u8>,
        stopped: bool,
        stop_reason: StopReason,
    }

    impl MockTarget {
        fn new() -> Self {
            Self {
                regs: GdbRegisters::default(),
                memory: HashMap::new(),
                stopped: true,
                stop_reason: StopReason::Breakpoint,
            }
        }
    }

    impl GdbTarget for MockTarget {
        fn read_memory(&self, addr: u64, size: usize) -> Option<Vec<u8>> {
            let mut result = Vec::with_capacity(size);
            for i in 0..size {
                result.push(*self.memory.get(&(addr + i as u64)).unwrap_or(&0));
            }
            Some(result)
        }

        fn write_memory(&mut self, addr: u64, data: &[u8]) -> bool {
            for (i, &byte) in data.iter().enumerate() {
                self.memory.insert(addr + i as u64, byte);
            }
            true
        }

        fn get_registers(&self) -> GdbRegisters {
            self.regs.clone()
        }

        fn set_registers(&mut self, regs: &GdbRegisters) {
            self.regs = regs.clone();
        }

        fn continue_execution(&mut self) {
            self.stopped = false;
        }

        fn single_step(&mut self) {
            self.stopped = false;
        }

        fn stop(&mut self) {
            self.stopped = true;
        }

        fn is_stopped(&self) -> bool {
            self.stopped
        }

        fn stop_reason(&self) -> StopReason {
            self.stop_reason
        }
    }

    #[test]
    fn test_stop_reason_reply() {
        assert_eq!(StopReason::Breakpoint.to_reply(), "S05");
        assert_eq!(StopReason::Step.to_reply(), "S05");
        assert_eq!(StopReason::Signal(9).to_reply(), "S09");
        assert_eq!(StopReason::Exited(0).to_reply(), "W00");
    }

    #[test]
    fn test_breakpoint_type_from_gdb() {
        assert_eq!(BreakpointType::from_gdb(0), Some(BreakpointType::Software));
        assert_eq!(
            BreakpointType::from_gdb(1),
            Some(BreakpointType::HardwareExec)
        );
        assert_eq!(
            BreakpointType::from_gdb(2),
            Some(BreakpointType::WriteWatch)
        );
        assert_eq!(BreakpointType::from_gdb(5), None);
    }

    #[test]
    fn test_breakpoint_contains() {
        let bp = Breakpoint::new(1, BreakpointType::WriteWatch, 0x1000, 4);
        assert!(bp.contains(0x1000));
        assert!(bp.contains(0x1003));
        assert!(!bp.contains(0x1004));
        assert!(!bp.contains(0x0FFF));
    }

    #[test]
    fn test_gdb_register_from_index() {
        assert_eq!(GdbRegister::from_index(0), Some(GdbRegister::Rax));
        assert_eq!(GdbRegister::from_index(16), Some(GdbRegister::Rip));
        assert_eq!(GdbRegister::from_index(100), None);
    }

    #[test]
    fn test_gdb_registers_get_set() {
        let mut regs = GdbRegisters::default();
        regs.set(GdbRegister::Rax, 0x1234);
        assert_eq!(regs.get(GdbRegister::Rax), 0x1234);

        regs.set(GdbRegister::Rip, 0xDEADBEEF);
        assert_eq!(regs.get(GdbRegister::Rip), 0xDEADBEEF);
    }

    #[test]
    fn test_packet_parser() {
        let mut parser = PacketParser::new();

        // Feed a valid packet
        let packet = b"$g#67";
        for &byte in packet {
            if let Some(result) = parser.feed(byte) {
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), b"g");
                return;
            }
        }
        panic!("Packet not parsed");
    }

    #[test]
    fn test_packet_parser_checksum_error() {
        let mut parser = PacketParser::new();

        // Feed a packet with wrong checksum
        let packet = b"$g#00";
        for &byte in packet {
            if let Some(result) = parser.feed(byte) {
                assert!(matches!(result, Err(GdbError::ChecksumError)));
                return;
            }
        }
        panic!("Packet not parsed");
    }

    #[test]
    fn test_parse_hex() {
        assert_eq!(parse_hex(b"48656c6c6f"), Some(b"Hello".to_vec()));
        assert_eq!(parse_hex(b"00ff"), Some(vec![0x00, 0xff]));
        assert_eq!(parse_hex(b"0"), None); // Odd length
    }

    #[test]
    fn test_parse_hex_u64() {
        assert_eq!(parse_hex_u64(b"0"), Some(0));
        assert_eq!(parse_hex_u64(b"ff"), Some(255));
        assert_eq!(parse_hex_u64(b"1000"), Some(0x1000));
        assert_eq!(parse_hex_u64(b"deadbeef"), Some(0xdeadbeef));
    }

    #[test]
    fn test_encode_hex() {
        assert_eq!(encode_hex(b"Hello"), "48656c6c6f");
        assert_eq!(encode_hex(&[0x00, 0xff]), "00ff");
    }

    #[test]
    fn test_gdb_stub_format_packet() {
        let stub = GdbStub::new();
        let packet = stub.format_packet("OK");
        assert_eq!(packet, b"$OK#9a");
    }

    #[test]
    fn test_gdb_stub_breakpoints() {
        let stub = GdbStub::new();

        let id = stub.add_breakpoint(BreakpointType::Software, 0x1000, 1);
        assert!(id > 0);
        assert!(stub.has_breakpoint(0x1000));

        let bp = stub.get_breakpoint(0x1000).unwrap();
        assert_eq!(bp.address, 0x1000);
        assert_eq!(bp.bp_type, BreakpointType::Software);

        stub.remove_breakpoint(0x1000);
        assert!(!stub.has_breakpoint(0x1000));
    }

    #[test]
    fn test_gdb_stub_watchpoint_check() {
        let stub = GdbStub::new();

        stub.add_breakpoint(BreakpointType::WriteWatch, 0x2000, 4);

        // Write to watched region
        let bp = stub.check_watchpoint(0x2001, true);
        assert!(bp.is_some());

        // Read from watched region (should not trigger WriteWatch)
        let bp = stub.check_watchpoint(0x2001, false);
        assert!(bp.is_none());
    }

    #[test]
    fn test_gdb_stub_handle_stop_query() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();

        let response = stub.handle_packet(b"?", &mut target);
        // Should return breakpoint stop reason
        assert!(response.starts_with(b"$S05"));
    }

    #[test]
    fn test_gdb_stub_handle_read_registers() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();
        target.regs.gprs[0] = 0x1234; // RAX

        let response = stub.handle_packet(b"g", &mut target);
        assert!(response.starts_with(b"$"));
    }

    #[test]
    fn test_gdb_stub_handle_read_memory() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();
        target.memory.insert(0x1000, 0xAA);
        target.memory.insert(0x1001, 0xBB);

        let response = stub.handle_packet(b"m1000,2", &mut target);
        assert!(String::from_utf8_lossy(&response).contains("aabb"));
    }

    #[test]
    fn test_gdb_stub_handle_write_memory() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();

        let response = stub.handle_packet(b"M1000,2:aabb", &mut target);
        assert!(response.ends_with(b"#9a")); // OK response

        assert_eq!(target.memory.get(&0x1000), Some(&0xAA));
        assert_eq!(target.memory.get(&0x1001), Some(&0xBB));
    }

    #[test]
    fn test_gdb_stub_handle_query_supported() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();

        let response = stub.handle_packet(b"qSupported", &mut target);
        let response_str = String::from_utf8_lossy(&response);
        assert!(response_str.contains("PacketSize"));
    }

    #[test]
    fn test_gdb_stub_handle_breakpoint() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();

        // Insert software breakpoint at 0x1000
        let response = stub.handle_packet(b"Z0,1000,1", &mut target);
        assert!(response.ends_with(b"#9a")); // OK
        assert!(stub.has_breakpoint(0x1000));

        // Remove breakpoint
        let response = stub.handle_packet(b"z0,1000,1", &mut target);
        assert!(response.ends_with(b"#9a")); // OK
        assert!(!stub.has_breakpoint(0x1000));
    }

    #[test]
    fn test_gdb_stub_stats() {
        let stub = GdbStub::new();
        stub.set_connected(true);
        stub.add_breakpoint(BreakpointType::Software, 0x1000, 1);

        let stats = stub.stats();
        assert!(stats.connected);
        assert_eq!(stats.breakpoint_count, 1);
    }

    #[test]
    fn test_gdb_stub_clear_breakpoints() {
        let stub = GdbStub::new();
        stub.add_breakpoint(BreakpointType::Software, 0x1000, 1);
        stub.add_breakpoint(BreakpointType::Software, 0x2000, 1);

        assert_eq!(stub.list_breakpoints().len(), 2);
        stub.clear_breakpoints();
        assert_eq!(stub.list_breakpoints().len(), 0);
    }

    #[test]
    fn test_gdb_stub_continue() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();
        target.stopped = true;

        stub.handle_packet(b"c", &mut target);
        assert!(!target.stopped);
        assert!(!stub.is_single_step());
    }

    #[test]
    fn test_gdb_stub_single_step() {
        let stub = GdbStub::new();
        let mut target = MockTarget::new();

        stub.handle_packet(b"s", &mut target);
        assert!(stub.is_single_step());
    }
}
