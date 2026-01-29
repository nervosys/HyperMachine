//! IDE/ATA Disk Controller
//!
//! This module implements an IDE (Integrated Drive Electronics) controller
//! supporting PIO (Programmed I/O) mode for basic disk access.
//!
//! Features:
//! - Primary and secondary channels
//! - PIO mode transfers (Mode 0-4)
//! - Identify Device command
//! - Read/Write sectors
//! - LBA addressing (28-bit and 48-bit)
//!
//! I/O Ports:
//! - Primary:   0x1F0-0x1F7 (command/data), 0x3F6 (control)
//! - Secondary: 0x170-0x177 (command/data), 0x376 (control)

use crate::{Device, DeviceType, Error, Result};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// IDE primary channel command port base
pub const IDE_PRIMARY_BASE: u16 = 0x1F0;
/// IDE primary channel control port
pub const IDE_PRIMARY_CTRL: u16 = 0x3F6;
/// IDE secondary channel command port base
pub const IDE_SECONDARY_BASE: u16 = 0x170;
/// IDE secondary channel control port
pub const IDE_SECONDARY_CTRL: u16 = 0x376;

/// Sector size in bytes
pub const SECTOR_SIZE: usize = 512;

/// IDE register offsets (from base)
mod regs {
    pub const DATA: u16 = 0; // Data register (R/W)
    pub const ERROR: u16 = 1; // Error register (R)
    pub const FEATURES: u16 = 1; // Features register (W)
    pub const SECTOR_COUNT: u16 = 2;
    pub const LBA_LOW: u16 = 3; // LBA bits 0-7
    pub const LBA_MID: u16 = 4; // LBA bits 8-15
    pub const LBA_HIGH: u16 = 5; // LBA bits 16-23
    pub const DRIVE_HEAD: u16 = 6; // Drive/Head + LBA bits 24-27
    pub const STATUS: u16 = 7; // Status register (R)
    pub const COMMAND: u16 = 7; // Command register (W)
}

/// IDE status register bits
mod status {
    pub const ERR: u8 = 0x01; // Error occurred
    pub const IDX: u8 = 0x02; // Index mark
    pub const CORR: u8 = 0x04; // Corrected data
    pub const DRQ: u8 = 0x08; // Data request ready
    pub const DSC: u8 = 0x10; // Drive seek complete
    pub const DF: u8 = 0x20; // Drive fault
    pub const DRDY: u8 = 0x40; // Drive ready
    pub const BSY: u8 = 0x80; // Busy
}

/// IDE error register bits
mod error {
    pub const AMNF: u8 = 0x01; // Address mark not found
    pub const TK0NF: u8 = 0x02; // Track 0 not found
    pub const ABRT: u8 = 0x04; // Aborted command
    pub const MCR: u8 = 0x08; // Media change request
    pub const IDNF: u8 = 0x10; // ID not found
    pub const MC: u8 = 0x20; // Media changed
    pub const UNC: u8 = 0x40; // Uncorrectable data error
    pub const BBK: u8 = 0x80; // Bad block detected
}

/// IDE commands
mod cmd {
    pub const NOP: u8 = 0x00;
    pub const READ_SECTORS: u8 = 0x20;
    pub const READ_SECTORS_EXT: u8 = 0x24; // 48-bit LBA
    pub const WRITE_SECTORS: u8 = 0x30;
    pub const WRITE_SECTORS_EXT: u8 = 0x34; // 48-bit LBA
    pub const CACHE_FLUSH: u8 = 0xE7;
    pub const CACHE_FLUSH_EXT: u8 = 0xEA; // 48-bit LBA
    pub const IDENTIFY: u8 = 0xEC;
    pub const SET_FEATURES: u8 = 0xEF;
}

/// Control register bits
mod ctrl {
    pub const NIEN: u8 = 0x02; // Disable interrupts
    pub const SRST: u8 = 0x04; // Software reset
    pub const HOB: u8 = 0x80; // High order byte (48-bit LBA)
}

/// Drive selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveSelect {
    Master = 0,
    Slave = 1,
}

/// IDE drive state
#[derive(Debug)]
struct IdeDrive {
    /// Drive present
    present: bool,
    /// Total sectors
    total_sectors: u64,
    /// Model string
    model: String,
    /// Serial number
    serial: String,
    /// Firmware revision
    firmware: String,
    /// Disk image data (in-memory for now)
    data: Vec<u8>,
    /// Supports 48-bit LBA
    lba48_support: bool,
}

impl IdeDrive {
    fn new_empty() -> Self {
        Self {
            present: false,
            total_sectors: 0,
            model: String::new(),
            serial: String::new(),
            firmware: String::new(),
            data: Vec::new(),
            lba48_support: false,
        }
    }

    fn new_disk(size_bytes: u64, model: &str) -> Self {
        let total_sectors = size_bytes / SECTOR_SIZE as u64;
        Self {
            present: true,
            total_sectors,
            model: model.to_string(),
            serial: "AETHVM00001".to_string(),
            firmware: "1.0".to_string(),
            data: vec![0u8; size_bytes as usize],
            lba48_support: total_sectors > 0x0FFF_FFFF, // > 128GB needs LBA48
        }
    }

    /// Generate IDENTIFY DEVICE response
    fn identify(&self) -> [u16; 256] {
        let mut id = [0u16; 256];

        if !self.present {
            return id;
        }

        // Word 0: General configuration
        id[0] = 0x0040; // Fixed device, not removable

        // Words 10-19: Serial number (20 ASCII chars)
        copy_string_to_id(&mut id[10..20], &self.serial);

        // Word 22: Vendor specific
        id[22] = 0;

        // Words 23-26: Firmware revision (8 ASCII chars)
        copy_string_to_id(&mut id[23..27], &self.firmware);

        // Words 27-46: Model number (40 ASCII chars)
        copy_string_to_id(&mut id[27..47], &self.model);

        // Word 47: Max sectors per interrupt
        id[47] = 0x8001; // 1 sector per interrupt

        // Word 49: Capabilities
        id[49] = 0x0200; // LBA supported

        // Word 50: Capabilities 2
        id[50] = 0x4001;

        // Word 53: Field validity
        id[53] = 0x0007;

        // Words 60-61: Total sectors (28-bit LBA)
        let sectors_28 = self.total_sectors.min(0x0FFF_FFFF) as u32;
        id[60] = (sectors_28 & 0xFFFF) as u16;
        id[61] = ((sectors_28 >> 16) & 0xFFFF) as u16;

        // Word 83: Command set supported (LBA48)
        if self.lba48_support {
            id[83] = 0x0400; // LBA48 supported
        }

        // Word 86: Command set enabled
        if self.lba48_support {
            id[86] = 0x0400;
        }

        // Words 100-103: Total sectors (48-bit LBA)
        if self.lba48_support {
            id[100] = (self.total_sectors & 0xFFFF) as u16;
            id[101] = ((self.total_sectors >> 16) & 0xFFFF) as u16;
            id[102] = ((self.total_sectors >> 32) & 0xFFFF) as u16;
            id[103] = ((self.total_sectors >> 48) & 0xFFFF) as u16;
        }

        id
    }

    /// Read sectors from the drive
    fn read_sectors(&self, lba: u64, count: u32) -> Option<Vec<u8>> {
        if !self.present {
            return None;
        }

        let start_byte = lba * SECTOR_SIZE as u64;
        let end_byte = start_byte + (count as u64 * SECTOR_SIZE as u64);

        if end_byte > self.data.len() as u64 {
            return None;
        }

        Some(self.data[start_byte as usize..end_byte as usize].to_vec())
    }

    /// Write sectors to the drive
    fn write_sectors(&mut self, lba: u64, data: &[u8]) -> bool {
        if !self.present {
            return false;
        }

        let start_byte = lba * SECTOR_SIZE as u64;
        let end_byte = start_byte + data.len() as u64;

        if end_byte > self.data.len() as u64 {
            return false;
        }

        self.data[start_byte as usize..end_byte as usize].copy_from_slice(data);
        true
    }
}

/// Copy ASCII string to IDENTIFY response (swapped byte order per ATA spec)
fn copy_string_to_id(dest: &mut [u16], src: &str) {
    let bytes = src.as_bytes();
    for (i, chunk) in dest.iter_mut().enumerate() {
        let idx = i * 2;
        let b0 = bytes.get(idx).copied().unwrap_or(b' ');
        let b1 = bytes.get(idx + 1).copied().unwrap_or(b' ');
        // ATA spec: characters are swapped in each word
        *chunk = ((b0 as u16) << 8) | (b1 as u16);
    }
}

/// IDE channel state
#[derive(Debug)]
struct IdeChannel {
    /// Master drive
    master: IdeDrive,
    /// Slave drive
    slave: IdeDrive,
    /// Currently selected drive
    selected_drive: DriveSelect,
    /// Status register
    status: u8,
    /// Error register
    error: u8,
    /// Sector count register
    sector_count: u16,
    /// LBA low (bits 0-7, or 24-31 for HOB)
    lba_low: [u8; 2],
    /// LBA mid (bits 8-15, or 32-39 for HOB)
    lba_mid: [u8; 2],
    /// LBA high (bits 16-23, or 40-47 for HOB)
    lba_high: [u8; 2],
    /// High order byte index (0 or 1)
    hob_index: usize,
    /// Control register
    control: u8,
    /// Interrupt pending
    interrupt_pending: bool,
    /// Data buffer for transfers
    data_buffer: Vec<u8>,
    /// Current position in data buffer
    data_position: usize,
    /// Write mode (true = writing, false = reading)
    write_mode: bool,
    /// Current LBA for multi-sector operations
    current_lba: u64,
    /// Sectors remaining
    sectors_remaining: u32,
}

impl IdeChannel {
    fn new() -> Self {
        Self {
            master: IdeDrive::new_empty(),
            slave: IdeDrive::new_empty(),
            selected_drive: DriveSelect::Master,
            status: status::DRDY | status::DSC,
            error: 0,
            sector_count: 0,
            lba_low: [0; 2],
            lba_mid: [0; 2],
            lba_high: [0; 2],
            hob_index: 0,
            control: 0,
            interrupt_pending: false,
            data_buffer: Vec::new(),
            data_position: 0,
            write_mode: false,
            current_lba: 0,
            sectors_remaining: 0,
        }
    }

    fn selected_drive_mut(&mut self) -> &mut IdeDrive {
        match self.selected_drive {
            DriveSelect::Master => &mut self.master,
            DriveSelect::Slave => &mut self.slave,
        }
    }

    fn selected_drive(&self) -> &IdeDrive {
        match self.selected_drive {
            DriveSelect::Master => &self.master,
            DriveSelect::Slave => &self.slave,
        }
    }

    /// Get current LBA from registers
    fn get_lba(&self) -> u64 {
        if self.control & ctrl::HOB != 0 {
            // 48-bit LBA
            let low = (self.lba_low[1] as u64) | ((self.lba_low[0] as u64) << 8);
            let mid = (self.lba_mid[1] as u64) | ((self.lba_mid[0] as u64) << 8);
            let high = (self.lba_high[1] as u64) | ((self.lba_high[0] as u64) << 8);
            low | (mid << 16) | (high << 32)
        } else {
            // 28-bit LBA
            let low = self.lba_low[0] as u64;
            let mid = self.lba_mid[0] as u64;
            let high = self.lba_high[0] as u64;
            low | (mid << 8) | (high << 16)
        }
    }

    /// Read from command block register
    fn read_command_reg(&mut self, offset: u16) -> u8 {
        match offset {
            regs::DATA => {
                if self.data_position < self.data_buffer.len() {
                    let value = self.data_buffer[self.data_position];
                    self.data_position += 1;

                    // Check if we've read a complete sector
                    if self.data_position % SECTOR_SIZE == 0 && !self.write_mode {
                        self.sectors_remaining = self.sectors_remaining.saturating_sub(1);
                        if self.sectors_remaining == 0 {
                            self.status &= !status::DRQ;
                            self.data_buffer.clear();
                            self.data_position = 0;
                        }
                    }
                    value
                } else {
                    0
                }
            }
            regs::ERROR => self.error,
            regs::SECTOR_COUNT => {
                if self.control & ctrl::HOB != 0 {
                    (self.sector_count >> 8) as u8
                } else {
                    self.sector_count as u8
                }
            }
            regs::LBA_LOW => {
                let idx = if self.control & ctrl::HOB != 0 { 1 } else { 0 };
                self.lba_low[idx]
            }
            regs::LBA_MID => {
                let idx = if self.control & ctrl::HOB != 0 { 1 } else { 0 };
                self.lba_mid[idx]
            }
            regs::LBA_HIGH => {
                let idx = if self.control & ctrl::HOB != 0 { 1 } else { 0 };
                self.lba_high[idx]
            }
            regs::DRIVE_HEAD => {
                let drive_bit = if self.selected_drive == DriveSelect::Slave {
                    0x10
                } else {
                    0
                };
                0xA0 | drive_bit | (self.lba_high[0] & 0x0F)
            }
            regs::STATUS => {
                // Reading status clears interrupt
                self.interrupt_pending = false;
                self.status
            }
            _ => 0,
        }
    }

    /// Write to command block register
    fn write_command_reg(&mut self, offset: u16, value: u8) {
        match offset {
            regs::DATA => {
                if self.write_mode && self.data_position < self.data_buffer.len() {
                    self.data_buffer[self.data_position] = value;
                    self.data_position += 1;

                    // Check if we've written a complete sector
                    if self.data_position % SECTOR_SIZE == 0 {
                        // Write the sector to disk
                        let sector_start = self.data_position - SECTOR_SIZE;
                        // Copy sector data to avoid borrow conflict
                        let sector_data: Vec<u8> =
                            self.data_buffer[sector_start..self.data_position].to_vec();
                        let current_lba = self.current_lba;

                        if self
                            .selected_drive_mut()
                            .write_sectors(current_lba, &sector_data)
                        {
                            self.current_lba += 1;
                            self.sectors_remaining = self.sectors_remaining.saturating_sub(1);

                            if self.sectors_remaining == 0 {
                                self.status &= !status::DRQ;
                                self.status |= status::DRDY;
                                self.data_buffer.clear();
                                self.data_position = 0;
                                self.write_mode = false;
                                self.raise_interrupt();
                            }
                        } else {
                            self.set_error(error::ABRT);
                        }
                    }
                }
            }
            regs::FEATURES => {
                // Features register - used by SET_FEATURES command
            }
            regs::SECTOR_COUNT => {
                // Shift previous value for 48-bit LBA
                self.sector_count = ((self.sector_count as u16) << 8) | (value as u16);
            }
            regs::LBA_LOW => {
                self.lba_low[1] = self.lba_low[0];
                self.lba_low[0] = value;
            }
            regs::LBA_MID => {
                self.lba_mid[1] = self.lba_mid[0];
                self.lba_mid[0] = value;
            }
            regs::LBA_HIGH => {
                self.lba_high[1] = self.lba_high[0];
                self.lba_high[0] = value;
            }
            regs::DRIVE_HEAD => {
                self.selected_drive = if value & 0x10 != 0 {
                    DriveSelect::Slave
                } else {
                    DriveSelect::Master
                };
                // Top 4 bits of LBA (for 28-bit mode)
                self.lba_high[0] = (self.lba_high[0] & 0xF0) | (value & 0x0F);
            }
            regs::COMMAND => {
                self.execute_command(value);
            }
            _ => {}
        }
    }

    /// Read from control register
    fn read_control_reg(&self) -> u8 {
        // Alternate status (same as status but doesn't clear interrupt)
        self.status
    }

    /// Write to control register
    fn write_control_reg(&mut self, value: u8) {
        // Software reset
        if value & ctrl::SRST != 0 && self.control & ctrl::SRST == 0 {
            self.reset();
        }
        self.control = value;
    }

    /// Execute an ATA command
    fn execute_command(&mut self, cmd: u8) {
        // Check if drive is present
        if !self.selected_drive().present {
            self.set_error(error::ABRT);
            return;
        }

        self.status |= status::BSY;
        self.error = 0;

        match cmd {
            cmd::NOP => {
                self.status &= !status::BSY;
            }

            cmd::IDENTIFY => {
                let id = self.selected_drive().identify();
                self.data_buffer = id.iter().flat_map(|&w| w.to_le_bytes()).collect();
                self.data_position = 0;
                self.status = status::DRDY | status::DSC | status::DRQ;
                self.write_mode = false;
                self.raise_interrupt();
            }

            cmd::READ_SECTORS | cmd::READ_SECTORS_EXT => {
                let lba = self.get_lba();
                let count = if self.sector_count == 0 {
                    256
                } else {
                    self.sector_count as u32
                };

                if let Some(data) = self.selected_drive().read_sectors(lba, count) {
                    self.data_buffer = data;
                    self.data_position = 0;
                    self.sectors_remaining = count;
                    self.current_lba = lba;
                    self.status = status::DRDY | status::DSC | status::DRQ;
                    self.write_mode = false;
                    self.raise_interrupt();
                } else {
                    self.set_error(error::IDNF);
                }
            }

            cmd::WRITE_SECTORS | cmd::WRITE_SECTORS_EXT => {
                let lba = self.get_lba();
                let count = if self.sector_count == 0 {
                    256
                } else {
                    self.sector_count as u32
                };

                self.data_buffer = vec![0u8; count as usize * SECTOR_SIZE];
                self.data_position = 0;
                self.sectors_remaining = count;
                self.current_lba = lba;
                self.status = status::DRDY | status::DSC | status::DRQ;
                self.write_mode = true;
                // Don't raise interrupt until data is received
            }

            cmd::CACHE_FLUSH | cmd::CACHE_FLUSH_EXT => {
                // Nothing to flush in memory-backed disk
                self.status = status::DRDY | status::DSC;
                self.raise_interrupt();
            }

            cmd::SET_FEATURES => {
                // Accept but ignore most features
                self.status = status::DRDY | status::DSC;
                self.raise_interrupt();
            }

            _ => {
                self.set_error(error::ABRT);
            }
        }
    }

    fn set_error(&mut self, err: u8) {
        self.error = err;
        self.status = status::DRDY | status::ERR;
        self.raise_interrupt();
    }

    fn raise_interrupt(&mut self) {
        if self.control & ctrl::NIEN == 0 {
            self.interrupt_pending = true;
        }
    }

    fn reset(&mut self) {
        self.status = status::DRDY | status::DSC;
        self.error = 0x01; // Diagnostic code: no error
        self.sector_count = 1;
        self.lba_low = [1, 0];
        self.lba_mid = [0, 0];
        self.lba_high = [0, 0];
        self.data_buffer.clear();
        self.data_position = 0;
        self.write_mode = false;
        self.interrupt_pending = false;
    }
}

/// IDE Controller
///
/// Emulates a dual-channel IDE controller with primary and secondary channels.
#[derive(Debug)]
pub struct IdeController {
    primary: Arc<Mutex<IdeChannel>>,
    secondary: Arc<Mutex<IdeChannel>>,
}

impl IdeController {
    /// Create a new IDE controller
    pub fn new() -> Self {
        Self {
            primary: Arc::new(Mutex::new(IdeChannel::new())),
            secondary: Arc::new(Mutex::new(IdeChannel::new())),
        }
    }

    /// Attach a disk to the primary master
    pub fn attach_primary_master(&self, size_bytes: u64, model: &str) {
        let mut channel = self.primary.lock().unwrap();
        channel.master = IdeDrive::new_disk(size_bytes, model);
    }

    /// Attach a disk to the primary slave
    pub fn attach_primary_slave(&self, size_bytes: u64, model: &str) {
        let mut channel = self.primary.lock().unwrap();
        channel.slave = IdeDrive::new_disk(size_bytes, model);
    }

    /// Attach a disk to the secondary master
    pub fn attach_secondary_master(&self, size_bytes: u64, model: &str) {
        let mut channel = self.secondary.lock().unwrap();
        channel.master = IdeDrive::new_disk(size_bytes, model);
    }

    /// Attach a disk to the secondary slave
    pub fn attach_secondary_slave(&self, size_bytes: u64, model: &str) {
        let mut channel = self.secondary.lock().unwrap();
        channel.slave = IdeDrive::new_disk(size_bytes, model);
    }

    /// Check if primary channel has pending interrupt (IRQ 14)
    pub fn primary_interrupt_pending(&self) -> bool {
        self.primary.lock().unwrap().interrupt_pending
    }

    /// Check if secondary channel has pending interrupt (IRQ 15)
    pub fn secondary_interrupt_pending(&self) -> bool {
        self.secondary.lock().unwrap().interrupt_pending
    }

    /// Read a word (16-bit) from the data port
    pub fn read_data_word(&self, primary: bool) -> u16 {
        let channel = if primary {
            &self.primary
        } else {
            &self.secondary
        };
        let mut ch = channel.lock().unwrap();

        let low = ch.read_command_reg(regs::DATA) as u16;
        let high = ch.read_command_reg(regs::DATA) as u16;
        low | (high << 8)
    }

    /// Write a word (16-bit) to the data port
    pub fn write_data_word(&self, primary: bool, value: u16) {
        let channel = if primary {
            &self.primary
        } else {
            &self.secondary
        };
        let mut ch = channel.lock().unwrap();

        ch.write_command_reg(regs::DATA, value as u8);
        ch.write_command_reg(regs::DATA, (value >> 8) as u8);
    }

    /// Read from an I/O port
    pub fn read_port(&self, port: u16) -> u8 {
        match port {
            IDE_PRIMARY_BASE..=0x1F7 => {
                let offset = port - IDE_PRIMARY_BASE;
                self.primary.lock().unwrap().read_command_reg(offset)
            }
            IDE_PRIMARY_CTRL => self.primary.lock().unwrap().read_control_reg(),
            IDE_SECONDARY_BASE..=0x177 => {
                let offset = port - IDE_SECONDARY_BASE;
                self.secondary.lock().unwrap().read_command_reg(offset)
            }
            IDE_SECONDARY_CTRL => self.secondary.lock().unwrap().read_control_reg(),
            _ => 0xFF,
        }
    }

    /// Write to an I/O port
    pub fn write_port(&self, port: u16, value: u8) {
        match port {
            IDE_PRIMARY_BASE..=0x1F7 => {
                let offset = port - IDE_PRIMARY_BASE;
                self.primary
                    .lock()
                    .unwrap()
                    .write_command_reg(offset, value);
            }
            IDE_PRIMARY_CTRL => {
                self.primary.lock().unwrap().write_control_reg(value);
            }
            IDE_SECONDARY_BASE..=0x177 => {
                let offset = port - IDE_SECONDARY_BASE;
                self.secondary
                    .lock()
                    .unwrap()
                    .write_command_reg(offset, value);
            }
            IDE_SECONDARY_CTRL => {
                self.secondary.lock().unwrap().write_control_reg(value);
            }
            _ => {}
        }
    }

    /// Get raw access to primary channel for direct data transfer
    pub fn primary_channel(&self) -> Arc<Mutex<IdeChannel>> {
        Arc::clone(&self.primary)
    }

    /// Get raw access to secondary channel for direct data transfer
    pub fn secondary_channel(&self) -> Arc<Mutex<IdeChannel>> {
        Arc::clone(&self.secondary)
    }
}

impl Default for IdeController {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for IdeController {
    fn name(&self) -> &str {
        "IDE Controller"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Disk
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device(
                "IDE only supports single-byte reads via this interface".into(),
            ));
        }
        data[0] = self.read_port(offset as u16);
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device(
                "IDE only supports single-byte writes via this interface".into(),
            ));
        }
        self.write_port(offset as u16, data[0]);
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        self.primary.lock().unwrap().reset();
        self.secondary.lock().unwrap().reset();
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ide_controller_creation() {
        let ide = IdeController::new();
        assert_eq!(ide.name(), "IDE Controller");
        assert_eq!(ide.device_type(), DeviceType::Disk);
    }

    #[test]
    fn test_attach_disk() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "AetherVM Virtual Disk"); // 1MB

        let channel = ide.primary.lock().unwrap();
        assert!(channel.master.present);
        assert_eq!(channel.master.total_sectors, 2048); // 1MB / 512
    }

    #[test]
    fn test_drive_selection() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "Master");
        ide.attach_primary_slave(1024 * 1024, "Slave");

        // Select slave (bit 4 = 1)
        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xF0);

        let channel = ide.primary.lock().unwrap();
        assert_eq!(channel.selected_drive, DriveSelect::Slave);
    }

    #[test]
    fn test_identify_command() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        // Issue IDENTIFY command
        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xA0);
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::IDENTIFY);

        // Status should have DRQ set
        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::DRQ != 0);

        // Read first word of identify data
        let word0 = ide.read_data_word(true);
        assert_eq!(word0, 0x0040); // Fixed device
    }

    #[test]
    fn test_read_sectors() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        // Write some data directly
        {
            let mut channel = ide.primary.lock().unwrap();
            channel.master.data[0..4].copy_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        }

        // Set up read: LBA 0, 1 sector
        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xE0); // LBA mode, master
        ide.write_port(IDE_PRIMARY_BASE + regs::SECTOR_COUNT, 1);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_LOW, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_MID, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_HIGH, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::READ_SECTORS);

        // Read status
        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::DRQ != 0);

        // Read data
        let word0 = ide.read_data_word(true);
        assert_eq!(word0, 0xADDE); // Little-endian: 0xDE, 0xAD
    }

    #[test]
    fn test_write_sectors() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        // Set up write: LBA 0, 1 sector
        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xE0);
        ide.write_port(IDE_PRIMARY_BASE + regs::SECTOR_COUNT, 1);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_LOW, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_MID, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::LBA_HIGH, 0);
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::WRITE_SECTORS);

        // Status should have DRQ
        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::DRQ != 0);

        // Write data (256 words = 512 bytes)
        ide.write_data_word(true, 0xCAFE);
        for _ in 1..256 {
            ide.write_data_word(true, 0);
        }

        // Verify data was written
        let channel = ide.primary.lock().unwrap();
        assert_eq!(channel.master.data[0], 0xFE);
        assert_eq!(channel.master.data[1], 0xCA);
    }

    #[test]
    fn test_no_drive_error() {
        let ide = IdeController::new();
        // Don't attach any drive

        // Issue IDENTIFY - should fail
        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xA0);
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::IDENTIFY);

        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::ERR != 0);
    }

    #[test]
    fn test_software_reset() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        // Set SRST bit
        ide.write_port(IDE_PRIMARY_CTRL, ctrl::SRST);
        // Clear SRST bit (reset happens on rising edge)
        ide.write_port(IDE_PRIMARY_CTRL, 0);

        // After reset, status should be DRDY
        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::DRDY != 0);
    }

    #[test]
    fn test_interrupt_disable() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        // Disable interrupts
        ide.write_port(IDE_PRIMARY_CTRL, ctrl::NIEN);

        // Issue command
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::IDENTIFY);

        // Interrupt should not be pending
        assert!(!ide.primary_interrupt_pending());
    }

    #[test]
    fn test_bcd_string_copy() {
        let mut dest = [0u16; 5];
        copy_string_to_id(&mut dest, "TestModel");

        // Characters are swapped in ATA format
        assert_eq!(dest[0], 0x5465); // 'T' << 8 | 'e'
        assert_eq!(dest[1], 0x7374); // 's' << 8 | 't'
    }

    #[test]
    fn test_cache_flush() {
        let ide = IdeController::new();
        ide.attach_primary_master(1024 * 1024, "TestDisk");

        ide.write_port(IDE_PRIMARY_BASE + regs::DRIVE_HEAD, 0xA0);
        ide.write_port(IDE_PRIMARY_BASE + regs::COMMAND, cmd::CACHE_FLUSH);

        let status = ide.read_port(IDE_PRIMARY_BASE + regs::STATUS);
        assert!(status & status::DRDY != 0);
        assert!(status & status::ERR == 0);
    }

    #[test]
    fn test_secondary_channel() {
        let ide = IdeController::new();
        ide.attach_secondary_master(1024 * 1024, "Secondary");

        // Access secondary channel
        ide.write_port(IDE_SECONDARY_BASE + regs::DRIVE_HEAD, 0xA0);
        ide.write_port(IDE_SECONDARY_BASE + regs::COMMAND, cmd::IDENTIFY);

        let status = ide.read_port(IDE_SECONDARY_BASE + regs::STATUS);
        assert!(status & status::DRQ != 0);
    }

    #[tokio::test]
    async fn test_device_trait() {
        let mut ide = IdeController::new();
        ide.init().await.unwrap();
        ide.reset().await.unwrap();
        ide.shutdown().await.unwrap();
    }
}
