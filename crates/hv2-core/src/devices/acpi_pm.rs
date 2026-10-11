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
    registers: [u8; REGISTERS],
}

impl AcpiPm {
    /// The registers as a machine just started has them: no event, none
    /// enabled, ACPI mode on.
    #[must_use]
    pub fn new() -> Self {
        let mut registers = [0; REGISTERS];
        registers[CONTROL_OFFSET] = SCI_EN;
        Self { registers }
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
                at if at < REGISTERS => self.registers[at],
                _ => 0xFF,
            };
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        for (i, byte) in data.iter().enumerate() {
            let at = offset as usize + i;
            if (ENABLE_OFFSET..REGISTERS).contains(&at) {
                self.registers[at] = *byte;
            }
        }
        // Whatever a guest writes, it stays in ACPI mode.
        self.registers[CONTROL_OFFSET] |= SCI_EN;
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
}
