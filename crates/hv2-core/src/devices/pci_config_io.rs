//! The port pair a guest uses to find PCI devices.
//!
//! The [`pci`](crate::pci) module models a root complex, buses, config space
//! and capabilities in some detail, but nothing connected it to a guest: no
//! code registered it against an I/O port, so `find_io_device` never returned
//! it and a guest's first configuration read fell through to the unhandled-port
//! path and got `0xFF`. A guest could not enumerate a single device, however
//! complete the model behind it was.
//!
//! This is the connection. It implements the Configuration Space Access
//! Mechanism from PCI 3.0 §3.2.2.3.2 -- the one every x86 firmware and kernel
//! probes first -- as an ordinary [`Device`] over ports `0xCF8..=0xCFF`.
//!
//! # The mechanism
//!
//! Two registers. `CONFIG_ADDRESS` at `0xCF8` latches what to talk to:
//!
//! ```text
//!  31     30..24    23..16   15..11   10..8    7..2    1..0
//! ┌────┬──────────┬────────┬────────┬───────┬────────┬──────┐
//! │ EN │ reserved │  bus   │ device │ func  │ reg    │  00  │
//! └────┴──────────┴────────┴────────┴───────┴────────┴──────┘
//! ```
//!
//! `CONFIG_DATA` at `0xCFC` then reads or writes the selected dword. Bit 31
//! must be set for an access to mean anything; with it clear the mechanism is
//! idle and reads return all ones, which is how a guest tells "nothing there"
//! from "register happens to be zero".
//!
//! Accesses to `CONFIG_DATA` may be one, two or four bytes, and the two low
//! bits of the port select where in the dword a narrow access lands. A guest
//! reading the one-byte header type at register `0x0C` does so with a byte read
//! of `0xCFF`, not of `0xCFC`, so the byte lanes have to be honoured rather
//! than treated as four aliases of the same dword.
//!
//! # What this does not do
//!
//! It does not decode BARs or route memory accesses to devices; a device that
//! wants a BAR window still needs that wired separately. It answers
//! configuration cycles, which is what enumeration consists of.

use crate::pci::{PciAddress, PciRootComplex};
use crate::{Device, DeviceType, Result};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::sync::Arc;

/// First port of the pair. `CONFIG_ADDRESS` occupies `0xCF8..=0xCFB`.
pub const PCI_CONFIG_IO_BASE: u16 = 0xCF8;
/// Last port of the pair. `CONFIG_DATA` occupies `0xCFC..=0xCFF`.
pub const PCI_CONFIG_IO_LAST: u16 = 0xCFF;

/// Offset of `CONFIG_ADDRESS` within the range.
const ADDRESS_OFFSET: u64 = 0;
/// Offset of `CONFIG_DATA` within the range.
const DATA_OFFSET: u64 = 4;

/// Bit 31 of `CONFIG_ADDRESS`: the access is meaningful only when set.
const ENABLE: u32 = 1 << 31;

/// The `0xCF8`/`0xCFC` config-space window onto a [`PciRootComplex`].
///
/// Holds the root complex so devices can be added before or after the guest
/// starts; [`root`](Self::root) hands out the same handle for that.
pub struct PciConfigIo {
    name: String,
    /// The last value written to `CONFIG_ADDRESS`, verbatim.
    ///
    /// Kept exactly as written, including the reserved bits, because a guest
    /// is entitled to read back what it wrote.
    address: RwLock<u32>,
    root: Arc<RwLock<PciRootComplex>>,
}

impl PciConfigIo {
    /// A window onto a new, empty root complex holding only the root bus.
    pub fn new(name: impl Into<String>) -> Self {
        Self::with_root(name, Arc::new(RwLock::new(PciRootComplex::new())))
    }

    /// A window onto an existing root complex.
    pub fn with_root(name: impl Into<String>, root: Arc<RwLock<PciRootComplex>>) -> Self {
        Self {
            name: name.into(),
            address: RwLock::new(0),
            root,
        }
    }

    /// The root complex this window answers for.
    ///
    /// Add devices through this. They become visible to the guest on its next
    /// configuration read, with no further wiring.
    pub fn root(&self) -> &Arc<RwLock<PciRootComplex>> {
        &self.root
    }

    /// Split the latched `CONFIG_ADDRESS` into an address and a register.
    ///
    /// `None` when bit 31 is clear, which means the mechanism is idle rather
    /// than pointing at device 0 of bus 0.
    fn selected(&self) -> Option<(PciAddress, u16)> {
        let address = *self.address.read();
        if address & ENABLE == 0 {
            return None;
        }
        let bus = ((address >> 16) & 0xFF) as u8;
        let device = ((address >> 11) & 0x1F) as u8;
        let function = ((address >> 8) & 0x07) as u8;
        // Bits 1..0 are always zero in the latched address; the byte lane for a
        // narrow access comes from the port, not from here.
        let register = (address & 0xFC) as u16;
        Some((PciAddress::new(bus, device, function), register))
    }

    /// The selected dword, or all ones when the mechanism is idle or the
    /// address names a device that is not present.
    fn read_data_dword(&self) -> u32 {
        match self.selected() {
            Some((address, register)) => self.root.read().read_config(&address, register),
            None => 0xFFFF_FFFF,
        }
    }
}

#[async_trait]
impl Device for PciConfigIo {
    fn device_type(&self) -> DeviceType {
        // There is no PCI variant, and inventing one would change a public
        // enum for a device that is infrastructure rather than a peripheral.
        DeviceType::Custom
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "PCI configuration mechanism at {:#06X}..={:#06X}, {} device(s) on the root bus",
            PCI_CONFIG_IO_BASE,
            PCI_CONFIG_IO_LAST,
            self.root.read().device_count()
        );
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        // A read that starts in CONFIG_ADDRESS returns the latch; one that
        // starts in CONFIG_DATA returns the selected dword. Nothing here
        // straddles the two, because an access is at most four bytes and the
        // two registers are dword-aligned.
        let (dword, lane) = if offset < DATA_OFFSET {
            (*self.address.read(), offset - ADDRESS_OFFSET)
        } else {
            (self.read_data_dword(), offset - DATA_OFFSET)
        };

        let bytes = dword.to_le_bytes();
        for (i, byte) in data.iter_mut().enumerate() {
            let index = lane as usize + i;
            // Past the end of the dword a guest gets all ones, the same answer
            // it gets for a device that is not there.
            *byte = bytes.get(index).copied().unwrap_or(0xFF);
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        if offset < DATA_OFFSET {
            // Writing the latch. A narrow write touches only its own bytes,
            // so the rest of the previous address survives.
            let mut bytes = self.address.read().to_le_bytes();
            let lane = (offset - ADDRESS_OFFSET) as usize;
            for (i, byte) in data.iter().enumerate() {
                if let Some(slot) = bytes.get_mut(lane + i) {
                    *slot = *byte;
                }
            }
            *self.address.write() = u32::from_le_bytes(bytes);
            return Ok(());
        }

        let Some((address, register)) = self.selected() else {
            // Idle mechanism. A write goes nowhere rather than landing on
            // whatever device happens to be at address zero.
            return Ok(());
        };

        // Read, patch the addressed bytes, write back: config space is written
        // a dword at a time, and a byte write must not clear its neighbours.
        let mut bytes = {
            let root = self.root.read();
            root.read_config(&address, register).to_le_bytes()
        };
        let lane = (offset - DATA_OFFSET) as usize;
        for (i, byte) in data.iter().enumerate() {
            if let Some(slot) = bytes.get_mut(lane + i) {
                *slot = *byte;
            }
        }
        self.root
            .write()
            .write_config(&address, register, u32::from_le_bytes(bytes));
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        *self.address.write() = 0;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pci::{ClassCode, ConfigSpace, DeviceId, VendorId};

    /// `CONFIG_ADDRESS` for one register of one device, as a guest builds it.
    fn address_of(bus: u8, device: u8, function: u8, register: u8) -> u32 {
        ENABLE
            | (u32::from(bus) << 16)
            | (u32::from(device) << 11)
            | (u32::from(function) << 8)
            | u32::from(register & 0xFC)
    }

    async fn select(io: &mut PciConfigIo, address: u32) {
        io.write(ADDRESS_OFFSET, &address.to_le_bytes())
            .await
            .unwrap();
    }

    async fn read_dword(io: &PciConfigIo, lane: u64) -> u32 {
        let mut buf = [0u8; 4];
        io.read(DATA_OFFSET + lane, &mut buf).await.unwrap();
        u32::from_le_bytes(buf)
    }

    /// A device with a recognisable vendor and device id: Red Hat's virtio
    /// vendor, and the modern virtio-vsock device id.
    fn a_device() -> ConfigSpace {
        ConfigSpace::with_device(
            VendorId(0x1AF4),
            DeviceId(0x1053),
            ClassCode {
                base: 0x08,
                sub: 0x80,
                prog_if: 0x00,
            },
            0x01,
        )
    }

    #[tokio::test]
    async fn an_empty_bus_reads_all_ones() {
        let mut io = PciConfigIo::new("pci-config");
        select(&mut io, address_of(0, 0, 0, 0)).await;

        // All ones is how a guest recognises an empty slot. Zero would look
        // like a real device from vendor 0.
        assert_eq!(read_dword(&io, 0).await, 0xFFFF_FFFF);
    }

    #[tokio::test]
    async fn a_device_added_to_the_root_bus_is_visible_to_the_guest() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(3, 0, a_device());

        select(&mut io, address_of(0, 3, 0, 0)).await;
        let id = read_dword(&io, 0).await;

        assert_eq!(id & 0xFFFF, 0x1AF4, "vendor id");
        assert_eq!(id >> 16, 0x1053, "device id");
    }

    #[tokio::test]
    async fn an_idle_mechanism_reads_all_ones_even_where_a_device_exists() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(0, 0, a_device());

        // Bit 31 clear: the guest has not asked for anything.
        select(&mut io, address_of(0, 0, 0, 0) & !ENABLE).await;

        assert_eq!(read_dword(&io, 0).await, 0xFFFF_FFFF);
    }

    /// A guest reads the one-byte header type at register 0x0C+3 with a byte
    /// access to 0xCFF. Aliasing every narrow access to the low byte would
    /// hand it the cache line size instead.
    #[tokio::test]
    async fn a_byte_read_takes_its_lane_from_the_port() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(3, 0, a_device());
        select(&mut io, address_of(0, 3, 0, 0)).await;

        let mut byte = [0u8; 1];
        io.read(DATA_OFFSET + 2, &mut byte).await.unwrap();

        // Third byte of the id dword is the low byte of the device id.
        assert_eq!(byte[0], 0x53);
    }

    #[tokio::test]
    async fn the_latched_address_reads_back_as_written() {
        let mut io = PciConfigIo::new("pci-config");
        let address = address_of(0, 7, 1, 0x10);
        select(&mut io, address).await;

        let mut buf = [0u8; 4];
        io.read(ADDRESS_OFFSET, &mut buf).await.unwrap();
        assert_eq!(u32::from_le_bytes(buf), address);
    }

    #[tokio::test]
    async fn a_write_reaches_config_space_and_reads_back() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(3, 0, a_device());

        // The command register at 0x04. Enabling memory and bus master is the
        // first thing a driver does after it finds a device.
        select(&mut io, address_of(0, 3, 0, 0x04)).await;
        io.write(DATA_OFFSET, &0x0006u32.to_le_bytes())
            .await
            .unwrap();

        assert_eq!(read_dword(&io, 0).await & 0x0006, 0x0006);
    }

    #[tokio::test]
    async fn a_byte_write_leaves_its_neighbours_alone() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(3, 0, a_device());
        select(&mut io, address_of(0, 3, 0, 0x04)).await;

        io.write(DATA_OFFSET, &0xFFFFu32.to_le_bytes())
            .await
            .unwrap();
        let before = read_dword(&io, 0).await;

        // Rewrite only the lowest byte.
        io.write(DATA_OFFSET, &[0x00]).await.unwrap();
        let after = read_dword(&io, 0).await;

        assert_eq!(after & 0xFF, 0x00, "the addressed byte changed");
        assert_eq!(
            after >> 8,
            before >> 8,
            "a byte write cleared bytes it did not address"
        );
    }

    #[tokio::test]
    async fn a_write_while_idle_goes_nowhere() {
        let mut io = PciConfigIo::new("pci-config");
        io.root().write().add_device(0, 0, a_device());

        select(&mut io, address_of(0, 0, 0, 0x04) & !ENABLE).await;
        io.write(DATA_OFFSET, &0xFFFFu32.to_le_bytes())
            .await
            .unwrap();

        // Now enable and look: the command register must be untouched.
        select(&mut io, address_of(0, 0, 0, 0x04)).await;
        assert_ne!(read_dword(&io, 0).await & 0xFFFF, 0xFFFF);
    }

    #[tokio::test]
    async fn reset_clears_the_latch() {
        let mut io = PciConfigIo::new("pci-config");
        select(&mut io, address_of(0, 3, 0, 0)).await;
        io.reset().await.unwrap();

        let mut buf = [0u8; 4];
        io.read(ADDRESS_OFFSET, &mut buf).await.unwrap();
        assert_eq!(u32::from_le_bytes(buf), 0);
    }
}
