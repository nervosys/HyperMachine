//! Memory-mapped I/O (MMIO) manager

use crate::{Device, Error, Result};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

/// MMIO region descriptor
#[derive(Debug, Clone)]
pub struct MmioRegion {
    /// Base address
    pub base: u64,
    /// Size in bytes
    pub size: u64,
    /// Device name
    pub device_name: String,
}

/// Memory-mapped I/O manager
pub struct MmioManager {
    /// Regions mapped to devices
    regions: RwLock<BTreeMap<u64, (u64, Arc<RwLock<dyn Device>>)>>,
}

impl MmioManager {
    /// Create a new MMIO manager
    pub fn new() -> Self {
        Self {
            regions: RwLock::new(BTreeMap::new()),
        }
    }

    /// Map a device to a memory region
    pub fn map_device(&self, base: u64, size: u64, device: Arc<RwLock<dyn Device>>) -> Result<()> {
        let mut regions = self.regions.write();

        // Check for overlaps
        for (&region_base, &(region_size, _)) in regions.iter() {
            let region_end = region_base + region_size;
            let new_end = base + size;

            if (base < region_end && new_end > region_base) {
                return Err(Error::Device(format!(
                    "MMIO region overlap: new [0x{:X}-0x{:X}) overlaps with existing [0x{:X}-0x{:X})",
                    base, new_end, region_base, region_end
                )));
            }
        }

        regions.insert(base, (size, device));
        tracing::info!(
            "Mapped MMIO region: 0x{:X}-0x{:X} (size: {} bytes)",
            base,
            base + size,
            size
        );
        Ok(())
    }

    /// Unmap a device from a memory region
    pub fn unmap_device(&self, base: u64) -> Result<()> {
        let mut regions = self.regions.write();
        regions
            .remove(&base)
            .ok_or_else(|| Error::Device(format!("No MMIO region at 0x{:X}", base)))?;
        tracing::info!("Unmapped MMIO region: 0x{:X}", base);
        Ok(())
    }

    /// Find the device for a given address
    fn find_device(&self, address: u64) -> Option<(u64, Arc<RwLock<dyn Device>>)> {
        let regions = self.regions.read();

        // Find the region containing this address
        for (&base, &(size, ref device)) in regions.iter() {
            if address >= base && address < base + size {
                return Some((base, device.clone()));
            }
        }

        None
    }

    /// Read from MMIO
    pub async fn read(&self, address: u64, data: &mut [u8]) -> Result<()> {
        if let Some((base, device)) = self.find_device(address) {
            let offset = address - base;
            device.read().read(offset, data).await
        } else {
            // No device mapped, return zeros
            data.fill(0);
            Ok(())
        }
    }

    /// Write to MMIO
    pub async fn write(&self, address: u64, data: &[u8]) -> Result<()> {
        if let Some((base, device)) = self.find_device(address) {
            let offset = address - base;
            device.write().write(offset, data).await
        } else {
            // No device mapped, ignore write
            tracing::trace!("Write to unmapped MMIO address: 0x{:X}", address);
            Ok(())
        }
    }

    /// Get all mapped regions
    pub fn regions(&self) -> Vec<MmioRegion> {
        let regions = self.regions.read();
        regions
            .iter()
            .map(|(&base, &(size, ref device))| MmioRegion {
                base,
                size,
                device_name: device.read().name().to_string(),
            })
            .collect()
    }

    /// Check if an address is mapped
    pub fn is_mapped(&self, address: u64) -> bool {
        self.find_device(address).is_some()
    }
}

impl Default for MmioManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SerialDevice;

    #[tokio::test]
    async fn test_mmio_mapping() {
        let manager = MmioManager::new();
        let serial = Arc::new(RwLock::new(SerialDevice::new("com1".to_string(), 0x3F8)));

        // Map device
        manager.map_device(0x3F8, 8, serial.clone()).unwrap();

        // Check if mapped
        assert!(manager.is_mapped(0x3F8));
        assert!(manager.is_mapped(0x3FF));
        assert!(!manager.is_mapped(0x400));

        // Test read/write through MMIO (byte-by-byte for serial)
        for &byte in b"Test" {
            manager.write(0x3F8, &[byte]).await.unwrap();
        }

        let output = serial.read().output_string();
        assert_eq!(output, "Test");
    }

    #[tokio::test]
    async fn test_mmio_overlap() {
        let manager = MmioManager::new();
        let serial1 = Arc::new(RwLock::new(SerialDevice::new("com1".to_string(), 0x3F8)));
        let serial2 = Arc::new(RwLock::new(SerialDevice::new("com2".to_string(), 0x2F8)));

        manager.map_device(0x3F8, 8, serial1).unwrap();

        // Try to map overlapping region
        let result = manager.map_device(0x3FC, 8, serial2);
        assert!(result.is_err());
    }
}
