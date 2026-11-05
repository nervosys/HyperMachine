//! The ACPI fixed-hardware power registers of a PC: PM1a event and control.
//!
//! A guest whose tables name fixed power hardware
//! ([`acpi_tables::build_pc`](crate::boot::acpi_tables::build_pc)) reads and
//! writes these ports while it starts, and writes the control register to
//! turn itself off. This answers the reads with what a machine already in
//! ACPI mode and with no event pending would say, and keeps what is written.
//!
//! It does not stop the machine. The write that enters S5 is a VM exit like
//! any other, and the run loop is what recognises it
//! ([`acpi_tables::is_power_off`](crate::boot::acpi_tables::is_power_off)),
//! as it recognises the i8042's reset pulse: stopping is the VM's to do, not
//! a device's.
//!
//! No event is ever raised, so the interrupt the tables name never fires.

use async_trait::async_trait;

use crate::boot::acpi_tables::{PM1A_CNT_PORT, PM1A_EVT_PORT};
use crate::device::{Device, DeviceType};
use crate::Result;

/// The first port this answers.
pub const ACPI_PM_BASE: u16 = PM1A_EVT_PORT;
/// The last: the second byte of the control register.
pub const ACPI_PM_LAST: u16 = PM1A_CNT_PORT + 1;

/// Offsets from [`ACPI_PM_BASE`]: status, enable, then control.
const ENABLE_OFFSET: usize = 2;
const CONTROL_OFFSET: usize = (PM1A_CNT_PORT - PM1A_EVT_PORT) as usize;
const REGISTERS: usize = (ACPI_PM_LAST - ACPI_PM_BASE) as usize + 1;

/// PM1 control `SCI_EN`: ACPI mode is on. Always set, since there is no
/// other mode to be in.
const SCI_EN: u8 = 1;

/// PM1a event and control registers.
#[derive(Debug)]
pub struct AcpiPm {
    registers: parking_lot::Mutex<[u8; REGISTERS]>,
}

impl AcpiPm {
    /// The registers as a machine just started has them: no event, none
    /// enabled, ACPI mode on.
    #[must_use]
    pub fn new() -> Self {
        let mut registers = [0; REGISTERS];
        registers[CONTROL_OFFSET] = SCI_EN;
        Self {
            registers: parking_lot::Mutex::new(registers),
        }
    }
}

impl Default for AcpiPm {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for AcpiPm {
    fn device_type(&self) -> DeviceType {
        DeviceType::Custom
    }

    fn name(&self) -> &str {
        "ACPI PM"
    }

    /// The enable and control registers the guest wrote.
    fn export_state(&self) -> Option<Vec<u8>> {
        Some(self.registers.lock().to_vec())
    }

    fn import_state(&self, bytes: &[u8]) -> Result<()> {
        let bytes: [u8; REGISTERS] = bytes
            .try_into()
            .map_err(|_| crate::Error::Device("ACPI PM state is malformed".into()))?;
        let mut registers = self.registers.lock();
        *registers = bytes;
        registers[CONTROL_OFFSET] |= SCI_EN;
        Ok(())
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        for (i, byte) in data.iter_mut().enumerate() {
            let at = offset as usize + i;
            // The status register reads as no event, whatever was written:
            // writing a one there clears a bit, and none is ever set.
            *byte = match at {
                0..ENABLE_OFFSET => 0,
                at if at < REGISTERS => self.registers.lock()[at],
                _ => 0xFF,
            };
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let mut registers = self.registers.lock();
        for (i, byte) in data.iter().enumerate() {
            let at = offset as usize + i;
            if (ENABLE_OFFSET..REGISTERS).contains(&at) {
                registers[at] = *byte;
            }
        }
        // Whatever a guest writes, it stays in ACPI mode.
        registers[CONTROL_OFFSET] |= SCI_EN;
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        *self = Self::new();
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

/// The port the SMP tables (`acpi.rs`) name as `PM_TMR_BLK`.
pub const ACPI_PM_TIMER_PORT: u16 = 0x408;
/// The timer's frequency, fixed by the ACPI specification.
const PM_TIMER_HZ: u128 = 3_579_545;

/// The ACPI power-management timer: a free-running 24-bit counter at
/// 3.579545 MHz.
///
/// A Linux guest that finds it calibrates its local APIC timer against it
/// and trusts the result, instead of timing the APIC against a TSC rate it
/// had to be told and checking that against jiffies. It counts host
/// monotonic time from when the device is made.
#[derive(Debug)]
pub struct AcpiPmTimer {
    started: parking_lot::Mutex<std::time::Instant>,
}

impl AcpiPmTimer {
    /// A timer at zero now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: parking_lot::Mutex::new(std::time::Instant::now()),
        }
    }

    fn count(&self) -> u32 {
        let ticks = self.started.lock().elapsed().as_nanos() * PM_TIMER_HZ / 1_000_000_000;
        (ticks & 0x00FF_FFFF) as u32
    }
}

impl Default for AcpiPmTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for AcpiPmTimer {
    fn device_type(&self) -> DeviceType {
        DeviceType::Timer
    }

    fn name(&self) -> &str {
        "ACPI PM timer"
    }

    /// The 24-bit count now. A restored timer carries on from it, so the
    /// guest's calibration against this clock does not see it jump back.
    fn export_state(&self) -> Option<Vec<u8>> {
        Some(self.count().to_le_bytes().to_vec())
    }

    fn import_state(&self, bytes: &[u8]) -> Result<()> {
        let bytes: [u8; 4] = bytes
            .try_into()
            .map_err(|_| crate::Error::Device("ACPI PM timer state is malformed".into()))?;
        let count = u128::from(u32::from_le_bytes(bytes) & 0x00FF_FFFF);
        let elapsed = std::time::Duration::from_nanos((count * 1_000_000_000 / PM_TIMER_HZ) as u64);
        let now = std::time::Instant::now();
        *self.started.lock() = now.checked_sub(elapsed).unwrap_or(now);
        Ok(())
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        let count = self.count().to_le_bytes();
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = count.get(offset as usize + i).copied().unwrap_or(0xFF);
        }
        Ok(())
    }

    async fn write(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
        // Read-only.
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn read16(device: &AcpiPm, offset: u64) -> u16 {
        let mut bytes = [0u8; 2];
        device.read(offset, &mut bytes).await.unwrap();
        u16::from_le_bytes(bytes)
    }

    #[tokio::test]
    async fn a_fresh_machine_is_in_acpi_mode_with_no_event() {
        let device = AcpiPm::new();
        assert_eq!(read16(&device, 0).await, 0, "status");
        assert_eq!(read16(&device, 2).await, 0, "enable");
        assert_eq!(read16(&device, 4).await, 1, "control: SCI_EN");
    }

    #[tokio::test]
    async fn what_a_guest_writes_is_kept_except_where_it_would_be_untrue() {
        let mut device = AcpiPm::new();
        // Clearing status bits, as a guest does before it sleeps: still none.
        device.write(0, &0xFFFFu16.to_le_bytes()).await.unwrap();
        assert_eq!(read16(&device, 0).await, 0);
        // Enabling the power button event is remembered.
        device.write(2, &0x0100u16.to_le_bytes()).await.unwrap();
        assert_eq!(read16(&device, 2).await, 0x0100);
        // A control write that leaves SCI_EN clear does not leave ACPI mode.
        device.write(4, &0x1400u16.to_le_bytes()).await.unwrap();
        assert_eq!(read16(&device, 4).await, 0x1401);
        // Past the registers there is nothing.
        let mut beyond = [0u8; 1];
        device.read(6, &mut beyond).await.unwrap();
        assert_eq!(beyond, [0xFF]);
        device.reset().await.unwrap();
        assert_eq!(read16(&device, 2).await, 0);
    }

    #[tokio::test]
    async fn the_pm_timer_counts_at_3_58_mhz_and_stays_24_bit() {
        let timer = AcpiPmTimer::new();
        let mut a = [0u8; 4];
        timer.read(0, &mut a).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let mut b = [0u8; 4];
        timer.read(0, &mut b).await.unwrap();
        let (a, b) = (u32::from_le_bytes(a), u32::from_le_bytes(b));
        assert!(a <= 0x00FF_FFFF && b <= 0x00FF_FFFF, "24 bits");
        let delta = b.wrapping_sub(a) & 0x00FF_FFFF;
        // 50 ms is about 178,977 counts; allow a slow host.
        assert!((170_000..400_000).contains(&delta), "delta {delta}");
    }

    #[tokio::test]
    async fn the_pm1_registers_a_guest_wrote_survive_an_export_and_import() {
        let mut a = AcpiPm::new();
        a.write(ENABLE_OFFSET as u64, &[0x21, 0x05]).await.unwrap();
        let bytes = a.export_state().unwrap();
        let b = AcpiPm::new();
        b.import_state(&bytes).unwrap();
        let mut got = [0u8; 2];
        b.read(ENABLE_OFFSET as u64, &mut got).await.unwrap();
        assert_eq!(got, [0x21, 0x05]);
        assert_eq!(b.export_state().unwrap(), bytes);
        assert!(b.import_state(&bytes[1..]).is_err());
    }

    #[tokio::test]
    async fn a_restored_pm_timer_carries_on_from_the_saved_count() {
        let timer = AcpiPmTimer::new();
        timer.import_state(&0x0012_3456u32.to_le_bytes()).unwrap();
        let mut now = [0u8; 4];
        timer.read(0, &mut now).await.unwrap();
        let now = u32::from_le_bytes(now);
        // A moment after the saved count, not back at zero.
        let ahead = now.wrapping_sub(0x0012_3456) & 0x00FF_FFFF;
        assert!(
            ahead < 3_579_545 / 10,
            "carried on {ahead} counts from the saved one"
        );
        assert!(timer.import_state(&[1, 2, 3]).is_err());
    }
}
