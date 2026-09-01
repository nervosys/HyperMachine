//! Real-Time Clock (RTC) Device
//!
//! This module implements the MC146818 Real-Time Clock, which provides:
//! - Date and time tracking
//! - 128 bytes of CMOS RAM
//! - Periodic interrupts (IRQ 8)
//! - Alarm functionality
//!
//! I/O Ports:
//! - 0x70: Index register (write only)
//! - 0x71: Data register (read/write)

use crate::{Device, DeviceType, Result};
use async_trait::async_trait;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot::Mutex;

/// RTC register indices
const RTC_SECONDS: u8 = 0x00;
const RTC_SECONDS_ALARM: u8 = 0x01;
const RTC_MINUTES: u8 = 0x02;
const RTC_MINUTES_ALARM: u8 = 0x03;
const RTC_HOURS: u8 = 0x04;
const RTC_HOURS_ALARM: u8 = 0x05;
const RTC_DAY_OF_WEEK: u8 = 0x06;
const RTC_DAY_OF_MONTH: u8 = 0x07;
const RTC_MONTH: u8 = 0x08;
const RTC_YEAR: u8 = 0x09;
const RTC_STATUS_A: u8 = 0x0A;
const RTC_STATUS_B: u8 = 0x0B;
const RTC_STATUS_C: u8 = 0x0C;
const RTC_STATUS_D: u8 = 0x0D;

/// Status Register A flags
/// The RTC's interrupt line on a PC.
const RTC_IRQ: u8 = 8;

/// Index register, relative to the base port the device is registered at.
///
/// Relative, not absolute: `DeviceManager` subtracts the base port before
/// calling, so a device that decodes 0x70 directly never matches.
pub const RTC_INDEX_OFFSET: u64 = 0;

/// Data register, relative to the base port.
pub const RTC_DATA_OFFSET: u64 = 1;

const STATUS_A_UIP: u8 = 0x80; // Update in progress

/// Status Register B flags
const STATUS_B_DSE: u8 = 0x01; // Daylight Savings Enable
const STATUS_B_24H: u8 = 0x02; // 24-hour mode
const STATUS_B_BCD: u8 = 0x04; // BCD mode (0=binary, 1=BCD)
const STATUS_B_SQWE: u8 = 0x08; // Square Wave Enable
const STATUS_B_UIE: u8 = 0x10; // Update-ended Interrupt Enable
const STATUS_B_AIE: u8 = 0x20; // Alarm Interrupt Enable
const STATUS_B_PIE: u8 = 0x40; // Periodic Interrupt Enable
const STATUS_B_SET: u8 = 0x80; // SET bit (1=disable updates)

/// Status Register C flags (read-only, cleared on read)
const STATUS_C_UF: u8 = 0x10; // Update-ended Flag
const STATUS_C_AF: u8 = 0x20; // Alarm Flag
const STATUS_C_PF: u8 = 0x40; // Periodic Interrupt Flag
const STATUS_C_IRQF: u8 = 0x80; // Interrupt Request Flag

/// Status Register D flags
const STATUS_D_VRT: u8 = 0x80; // Valid RAM and Time (battery good)

/// Internal state of the RTC
#[derive(Debug)]
struct RtcState {
    /// Currently selected register index
    index: u8,
    /// CMOS RAM (128 bytes, includes time/date registers)
    cmos_ram: [u8; 128],
    /// Status registers
    status_a: u8,
    status_b: u8,
    status_c: u8,
    status_d: u8,
    /// NMI disable bit (bit 7 of port 0x70)
    nmi_disabled: bool,
}

impl RtcState {
    fn new() -> Self {
        Self {
            index: 0,
            cmos_ram: [0; 128],
            status_a: 0x26,         // Default: 32kHz rate, no UIP
            status_b: STATUS_B_24H, // 24-hour binary mode
            status_c: 0,
            status_d: STATUS_D_VRT, // Battery good
            nmi_disabled: false,
        }
    }

    /// Update time registers from system time
    fn update_time(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();

        // Calculate time components
        let total_seconds = now.as_secs();
        let seconds = (total_seconds % 60) as u8;
        let minutes = ((total_seconds / 60) % 60) as u8;
        let hours = ((total_seconds / 3600) % 24) as u8;

        // Calculate date components
        // Simplified: Unix epoch is Jan 1, 1970 (Thursday)
        let days_since_epoch = total_seconds / 86400;
        let day_of_week = ((days_since_epoch + 4) % 7 + 1) as u8; // 1=Sunday

        // Simplified date calculation (doesn't account for leap years properly)
        let year = 1970 + (days_since_epoch / 365) as u16;
        let year_offset = year - 2000; // RTC stores 2-digit year
        let day_of_year = days_since_epoch % 365;
        let month = ((day_of_year / 30) + 1).min(12) as u8;
        let day = ((day_of_year % 30) + 1).min(31) as u8;

        // Store in BCD or binary depending on mode
        if self.status_b & STATUS_B_BCD != 0 {
            // BCD mode
            self.cmos_ram[RTC_SECONDS as usize] = to_bcd(seconds);
            self.cmos_ram[RTC_MINUTES as usize] = to_bcd(minutes);
            self.cmos_ram[RTC_HOURS as usize] = to_bcd(hours);
            self.cmos_ram[RTC_DAY_OF_WEEK as usize] = day_of_week;
            self.cmos_ram[RTC_DAY_OF_MONTH as usize] = to_bcd(day);
            self.cmos_ram[RTC_MONTH as usize] = to_bcd(month);
            self.cmos_ram[RTC_YEAR as usize] = to_bcd((year_offset % 100) as u8);
        } else {
            // Binary mode
            self.cmos_ram[RTC_SECONDS as usize] = seconds;
            self.cmos_ram[RTC_MINUTES as usize] = minutes;
            self.cmos_ram[RTC_HOURS as usize] = hours;
            self.cmos_ram[RTC_DAY_OF_WEEK as usize] = day_of_week;
            self.cmos_ram[RTC_DAY_OF_MONTH as usize] = day;
            self.cmos_ram[RTC_MONTH as usize] = month;
            self.cmos_ram[RTC_YEAR as usize] = (year_offset % 100) as u8;
        }

        // Check alarm after updating time
        self.check_alarm();
    }

    /// Check whether the current time matches the alarm registers.
    ///
    /// An alarm register value >= 0xC0 acts as a "don't care" wildcard.
    /// If all three components match (or are wildcards) and AIE is enabled
    /// in Status B, the alarm flag and IRQF are set in Status C.
    fn check_alarm(&mut self) {
        if self.status_b & STATUS_B_AIE == 0 {
            return;
        }

        let sec_match = self.alarm_matches(RTC_SECONDS as usize, RTC_SECONDS_ALARM as usize);
        let min_match = self.alarm_matches(RTC_MINUTES as usize, RTC_MINUTES_ALARM as usize);
        let hr_match = self.alarm_matches(RTC_HOURS as usize, RTC_HOURS_ALARM as usize);

        if sec_match && min_match && hr_match {
            self.status_c |= STATUS_C_AF | STATUS_C_IRQF;
        }
    }

    /// Returns true if the alarm register matches the current time register
    /// or is a wildcard (>= 0xC0).
    fn alarm_matches(&self, time_reg: usize, alarm_reg: usize) -> bool {
        let alarm_val = self.cmos_ram[alarm_reg];
        alarm_val >= 0xC0 || alarm_val == self.cmos_ram[time_reg]
    }

    /// Read from the currently indexed register
    fn read_data(&mut self) -> u8 {
        match self.index {
            RTC_STATUS_A => self.status_a,
            RTC_STATUS_B => self.status_b,
            RTC_STATUS_C => {
                // Reading status C clears it
                let value = self.status_c;
                self.status_c = 0;
                value
            }
            RTC_STATUS_D => self.status_d,
            i if (i as usize) < self.cmos_ram.len() => {
                // Update time before reading time registers
                if i <= RTC_YEAR {
                    self.update_time();
                }
                self.cmos_ram[i as usize]
            }
            _ => 0,
        }
    }

    /// Write to the currently indexed register
    fn write_data(&mut self, value: u8) {
        match self.index {
            RTC_STATUS_A => {
                // Only allow certain bits to be changed
                self.status_a = (self.status_a & STATUS_A_UIP) | (value & !STATUS_A_UIP);
            }
            RTC_STATUS_B => {
                self.status_b = value;
            }
            RTC_STATUS_C | RTC_STATUS_D => {
                // Read-only, ignore writes
            }
            i if (i as usize) < self.cmos_ram.len() => {
                self.cmos_ram[i as usize] = value;
            }
            _ => {}
        }
    }

    /// Trigger periodic interrupt
    fn trigger_periodic_interrupt(&mut self) -> bool {
        if self.status_b & STATUS_B_PIE != 0 {
            self.status_c |= STATUS_C_PF | STATUS_C_IRQF;
            true
        } else {
            false
        }
    }

    /// Check if there's a pending interrupt
    fn has_pending_interrupt(&self) -> bool {
        (self.status_c & STATUS_C_IRQF) != 0
    }
}

/// Convert binary to BCD
fn to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

/// MC146818 Real-Time Clock
///
/// This device emulates the classic PC RTC/CMOS chip.
/// It provides date/time tracking and 128 bytes of CMOS RAM.
#[derive(Debug)]
pub struct RtcDevice {
    state: Arc<Mutex<RtcState>>,
}

impl RtcDevice {
    /// Create a new RTC device
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RtcState::new())),
        }
    }

    /// Read from index port (0x70) - returns NMI status
    pub fn read_index(&self) -> u8 {
        let state = self.state.lock();
        if state.nmi_disabled {
            0x80
        } else {
            0x00
        }
    }

    /// Write to index port (0x70)
    pub fn write_index(&self, value: u8) {
        let mut state = self.state.lock();
        state.nmi_disabled = (value & 0x80) != 0;
        state.index = value & 0x7F;
    }

    /// Read from data port (0x71)
    pub fn read_data(&self) -> u8 {
        let mut state = self.state.lock();
        state.read_data()
    }

    /// Write to data port (0x71)
    pub fn write_data(&self, value: u8) {
        let mut state = self.state.lock();
        state.write_data(value);
    }

    /// Check if the RTC has a pending interrupt (IRQ 8)
    pub fn has_pending_interrupt(&self) -> bool {
        self.state.lock().has_pending_interrupt()
    }

    /// Trigger a periodic interrupt (called by timer subsystem)
    pub fn trigger_periodic(&self) -> bool {
        self.state.lock().trigger_periodic_interrupt()
    }

    /// Manually trigger alarm check against current time registers.
    /// Returns true if the alarm fired.
    pub fn check_alarm(&self) -> bool {
        let mut state = self.state.lock();
        let before = state.status_c & STATUS_C_AF;
        state.check_alarm();
        before == 0 && (state.status_c & STATUS_C_AF) != 0
    }
}

impl Default for RtcDevice {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for RtcDevice {
    fn name(&self) -> &str {
        "MC146818 RTC"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::RTC
    }

    fn pending_interrupt(&self) -> Option<u8> {
        // IRQ 8 is the RTC's line on a PC. The inherent
        // `has_pending_interrupt` existed and the trait method did not, so the
        // dispatch layer could never see this device's interrupt: a guest that
        // enabled the periodic interrupt would wait for one that had no way to
        // be delivered.
        self.has_pending_interrupt().then_some(RTC_IRQ)
    }

    async fn init(&mut self) -> Result<()> {
        // Initialize time on first boot
        self.state.lock().update_time();
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        // Offsets are relative to the registered base port -- `DeviceManager`
        // hands over `port - base_port`, the same as every other device on the
        // I/O path. This used to decode the absolute 0x70 and 0x71 and error on
        // anything else, which meant it worked when a test called it directly
        // and never once behind the device manager: every access arrived as 0
        // or 1, hit the fallback, and returned an error that stopped the VM.
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = match (offset + index as u64) & 1 {
                0 => self.read_index(),
                _ => self.read_data(),
            };
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        for (index, byte) in data.iter().enumerate() {
            match (offset + index as u64) & 1 {
                0 => self.write_index(*byte),
                _ => self.write_data(*byte),
            }
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut state = self.state.lock();
        *state = RtcState::new();
        state.update_time();
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rtc_creation() {
        let rtc = RtcDevice::new();
        assert_eq!(rtc.name(), "MC146818 RTC");
        assert_eq!(rtc.device_type(), DeviceType::RTC);
    }

    #[tokio::test]
    async fn test_rtc_time_read() {
        let mut rtc = RtcDevice::new();
        rtc.init().await.unwrap();

        // Select seconds register
        rtc.write_index(RTC_SECONDS);
        let seconds = rtc.read_data();

        // Should be in range 0-59
        assert!(seconds < 60);
    }

    #[tokio::test]
    async fn test_rtc_status_registers() {
        let rtc = RtcDevice::new();

        // Read status B (should default to 24-hour binary mode)
        rtc.write_index(RTC_STATUS_B);
        let status_b = rtc.read_data();
        assert_eq!(status_b & STATUS_B_24H, STATUS_B_24H);

        // Read status D (should indicate battery good)
        rtc.write_index(RTC_STATUS_D);
        let status_d = rtc.read_data();
        assert_eq!(status_d & STATUS_D_VRT, STATUS_D_VRT);
    }

    #[tokio::test]
    async fn test_rtc_cmos_ram() {
        let rtc = RtcDevice::new();

        // Write to CMOS RAM (using high addresses that aren't time registers)
        rtc.write_index(0x10);
        rtc.write_data(0x42);

        // Read back
        rtc.write_index(0x10);
        assert_eq!(rtc.read_data(), 0x42);
    }

    #[tokio::test]
    async fn test_rtc_nmi_disable() {
        let rtc = RtcDevice::new();

        // Set NMI disable bit
        rtc.write_index(0x80 | RTC_SECONDS);
        let index_read = rtc.read_index();
        assert_eq!(index_read & 0x80, 0x80);

        // Clear NMI disable bit
        rtc.write_index(RTC_SECONDS);
        let index_read = rtc.read_index();
        assert_eq!(index_read & 0x80, 0);
    }

    #[tokio::test]
    async fn test_rtc_periodic_interrupt() {
        let rtc = RtcDevice::new();

        // Enable periodic interrupts
        rtc.write_index(RTC_STATUS_B);
        rtc.write_data(STATUS_B_PIE | STATUS_B_24H);

        // Trigger periodic interrupt
        let triggered = rtc.trigger_periodic();
        assert!(triggered);
        assert!(rtc.has_pending_interrupt());

        // Read status C (should clear interrupt)
        rtc.write_index(RTC_STATUS_C);
        let status_c = rtc.read_data();
        assert_eq!(status_c & STATUS_C_PF, STATUS_C_PF);
        assert!(!rtc.has_pending_interrupt());
    }

    #[tokio::test]
    async fn test_rtc_device_trait() {
        let mut rtc = RtcDevice::new();

        // Test Device trait methods
        rtc.init().await.unwrap();

        let mut buf = [0u8; 1];
        rtc.read(RTC_INDEX_OFFSET, &mut buf).await.unwrap();

        rtc.write(RTC_INDEX_OFFSET, &[RTC_SECONDS]).await.unwrap();
        rtc.read(RTC_DATA_OFFSET, &mut buf).await.unwrap();

        rtc.reset().await.unwrap();
        rtc.shutdown().await.unwrap();
    }

    #[test]
    fn test_bcd_conversion() {
        assert_eq!(to_bcd(0), 0x00);
        assert_eq!(to_bcd(9), 0x09);
        assert_eq!(to_bcd(10), 0x10);
        assert_eq!(to_bcd(59), 0x59);
        assert_eq!(to_bcd(99), 0x99);
    }

    #[tokio::test]
    async fn test_rtc_alarm_match() {
        let mut rtc = RtcDevice::new();
        rtc.init().await.unwrap();

        // Enable alarm interrupt
        rtc.write_index(RTC_STATUS_B);
        rtc.write_data(STATUS_B_AIE | STATUS_B_24H);

        // Read current time to know what to set the alarm to
        rtc.write_index(RTC_SECONDS);
        let current_seconds = rtc.read_data();
        rtc.write_index(RTC_MINUTES);
        let current_minutes = rtc.read_data();
        rtc.write_index(RTC_HOURS);
        let current_hours = rtc.read_data();

        // Set alarm to match current time
        rtc.write_index(RTC_SECONDS_ALARM);
        rtc.write_data(current_seconds);
        rtc.write_index(RTC_MINUTES_ALARM);
        rtc.write_data(current_minutes);
        rtc.write_index(RTC_HOURS_ALARM);
        rtc.write_data(current_hours);

        // Trigger alarm check
        let fired = rtc.check_alarm();
        assert!(fired, "alarm should fire when time matches");
        assert!(rtc.has_pending_interrupt());

        // Read status C to clear
        rtc.write_index(RTC_STATUS_C);
        let status_c = rtc.read_data();
        assert_ne!(status_c & STATUS_C_AF, 0);
    }

    #[test]
    fn test_rtc_alarm_wildcard() {
        let rtc = RtcDevice::new();

        // Enable alarm interrupt
        rtc.write_index(RTC_STATUS_B);
        rtc.write_data(STATUS_B_AIE | STATUS_B_24H);

        // Set all alarm registers to wildcard (0xC0+)
        rtc.write_index(RTC_SECONDS_ALARM);
        rtc.write_data(0xC0);
        rtc.write_index(RTC_MINUTES_ALARM);
        rtc.write_data(0xFF);
        rtc.write_index(RTC_HOURS_ALARM);
        rtc.write_data(0xC0);

        // Force a time update so check_alarm runs
        let fired = rtc.check_alarm();
        assert!(fired, "wildcard alarm should always fire");
    }

    #[test]
    fn test_rtc_alarm_no_match() {
        let rtc = RtcDevice::new();

        // Enable alarm interrupt
        rtc.write_index(RTC_STATUS_B);
        rtc.write_data(STATUS_B_AIE | STATUS_B_24H);

        // Set alarm to an impossible time (seconds=99)
        rtc.write_index(RTC_SECONDS_ALARM);
        rtc.write_data(99);
        rtc.write_index(RTC_MINUTES_ALARM);
        rtc.write_data(99);
        rtc.write_index(RTC_HOURS_ALARM);
        rtc.write_data(99);

        let fired = rtc.check_alarm();
        assert!(!fired, "alarm should not fire with impossible time");
        assert!(!rtc.has_pending_interrupt());
    }

    #[test]
    fn test_rtc_alarm_disabled() {
        let rtc = RtcDevice::new();

        // AIE not set — alarm should never fire
        rtc.write_index(RTC_STATUS_B);
        rtc.write_data(STATUS_B_24H); // No AIE

        // Set alarm to wildcard (should always match if enabled)
        rtc.write_index(RTC_SECONDS_ALARM);
        rtc.write_data(0xC0);
        rtc.write_index(RTC_MINUTES_ALARM);
        rtc.write_data(0xC0);
        rtc.write_index(RTC_HOURS_ALARM);
        rtc.write_data(0xC0);

        let fired = rtc.check_alarm();
        assert!(!fired, "alarm should not fire when AIE is disabled");
    }
    #[tokio::test]
    async fn the_rtc_decodes_offsets_relative_to_its_base_port() {
        // The convention the whole I/O path uses. Decoding absolute ports here
        // meant a guest reading the RTC got a device error instead of a
        // register, and a device error on the I/O path stops the VM -- so the
        // symptom was a guest that died the first time it asked the time.
        let mut rtc = RtcDevice::new();

        rtc.write(RTC_INDEX_OFFSET, &[RTC_STATUS_A]).await.unwrap();
        let mut status = [0u8; 1];
        rtc.read(RTC_DATA_OFFSET, &mut status).await.unwrap();

        assert_eq!(
            status[0] & STATUS_A_UIP,
            0,
            "update-in-progress must read clear, or a kernel waiting for it to \
             clear waits forever"
        );
    }

    #[tokio::test]
    async fn a_wide_access_walks_the_two_registers() {
        let rtc = RtcDevice::new();
        let mut both = [0u8; 2];
        rtc.read(RTC_INDEX_OFFSET, &mut both)
            .await
            .expect("hardware answers a word read rather than refusing it");
    }
}
