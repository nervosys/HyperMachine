//! Device management and emulation

use crate::{Error, Result};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Device type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceType {
    Serial,
    Keyboard,
    Mouse,
    Disk,
    Network,
    Gpu,
    Display,
    Timer,
    RTC,
    Input,
    InterruptController,
    Custom,
}

/// Device trait for emulated devices
#[async_trait]
pub trait Device: Send + Sync {
    /// Get device type
    fn device_type(&self) -> DeviceType;

    /// Get device name
    fn name(&self) -> &str;

    /// Initialize the device
    async fn init(&mut self) -> Result<()>;

    /// Read from device
    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()>;

    /// Write to device
    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()>;

    /// Reset the device
    async fn reset(&mut self) -> Result<()>;

    /// Shutdown the device
    async fn shutdown(&mut self) -> Result<()>;
}

/// MMIO region mapping
#[derive(Clone)]
struct MmioRegionMapping {
    base_addr: u64,
    size: u64,
    device_name: String,
    device: Arc<RwLock<dyn Device>>,
}

/// I/O port range mapping
#[derive(Clone)]
struct IoPortMapping {
    start_port: u16,
    end_port: u16,
    device_name: String,
    device: Arc<RwLock<dyn Device>>,
}

/// Device manager
pub struct DeviceManager {
    devices: Arc<RwLock<HashMap<String, Arc<RwLock<dyn Device>>>>>,
    mmio_regions: Arc<RwLock<Vec<MmioRegionMapping>>>,
    io_ports: Arc<RwLock<Vec<IoPortMapping>>>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            mmio_regions: Arc::new(RwLock::new(Vec::new())),
            io_ports: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register a device
    pub async fn register_device(
        &self,
        name: String,
        device: Arc<RwLock<dyn Device>>,
    ) -> Result<()> {
        let mut devices = self.devices.write().await;

        if devices.contains_key(&name) {
            return Err(Error::Device(format!(
                "Device '{}' already registered",
                name
            )));
        }

        devices.insert(name, device);
        Ok(())
    }

    /// Unregister a device
    pub async fn unregister_device(&self, name: &str) -> Result<()> {
        let mut devices = self.devices.write().await;

        if devices.remove(name).is_none() {
            return Err(Error::Device(format!("Device '{}' not found", name)));
        }

        Ok(())
    }

    /// Get a device by name
    pub async fn get_device(&self, name: &str) -> Option<Arc<RwLock<dyn Device>>> {
        self.devices.read().await.get(name).cloned()
    }

    /// Get all devices of a specific type
    pub async fn get_devices_by_type(
        &self,
        device_type: DeviceType,
    ) -> Vec<Arc<RwLock<dyn Device>>> {
        let devices = self.devices.read().await;
        let mut result = Vec::new();
        for d in devices.values() {
            if d.read().await.device_type() == device_type {
                result.push(d.clone());
            }
        }
        result
    }

    /// Initialize all devices
    pub async fn init_all(&self) -> Result<()> {
        let devices: Vec<_> = self.devices.read().await.values().cloned().collect();

        for device in devices {
            device.write().await.init().await?;
        }

        Ok(())
    }

    /// Shutdown all devices
    pub async fn shutdown_all(&self) -> Result<()> {
        let devices: Vec<_> = self.devices.read().await.values().cloned().collect();

        for device in devices {
            device.write().await.shutdown().await?;
        }

        Ok(())
    }

    /// Register an MMIO region for a device
    pub async fn register_mmio_region(
        &self,
        device_name: String,
        base_addr: u64,
        size: u64,
    ) -> Result<()> {
        let devices = self.devices.read().await;
        let device = devices
            .get(&device_name)
            .ok_or_else(|| Error::Device(format!("Device '{}' not found", device_name)))?
            .clone();
        drop(devices);

        let mut mmio_regions = self.mmio_regions.write().await;

        // Check for overlapping regions
        let end_addr = base_addr + size;
        for region in mmio_regions.iter() {
            let region_end = region.base_addr + region.size;
            if base_addr < region_end && end_addr > region.base_addr {
                return Err(Error::Device(format!(
                    "MMIO region {:#x}-{:#x} overlaps with existing region {:#x}-{:#x} ({})",
                    base_addr, end_addr, region.base_addr, region_end, region.device_name
                )));
            }
        }

        mmio_regions.push(MmioRegionMapping {
            base_addr,
            size,
            device_name: device_name.clone(),
            device,
        });

        tracing::debug!(
            "Registered MMIO region: device='{}' base={:#x} size={:#x}",
            device_name,
            base_addr,
            size
        );

        Ok(())
    }

    /// Register an I/O port range for a device
    pub async fn register_io_port_range(
        &self,
        device_name: String,
        start_port: u16,
        end_port: u16,
    ) -> Result<()> {
        let devices = self.devices.read().await;
        let device = devices
            .get(&device_name)
            .ok_or_else(|| Error::Device(format!("Device '{}' not found", device_name)))?
            .clone();
        drop(devices);

        let mut io_ports = self.io_ports.write().await;

        // Check for overlapping ranges
        for mapping in io_ports.iter() {
            if start_port <= mapping.end_port && end_port >= mapping.start_port {
                return Err(Error::Device(format!(
                    "I/O port range {:#x}-{:#x} overlaps with existing range {:#x}-{:#x} ({})",
                    start_port, end_port, mapping.start_port, mapping.end_port, mapping.device_name
                )));
            }
        }

        io_ports.push(IoPortMapping {
            start_port,
            end_port,
            device_name: device_name.clone(),
            device,
        });

        tracing::debug!(
            "Registered I/O port range: device='{}' ports={:#x}-{:#x}",
            device_name,
            start_port,
            end_port
        );

        Ok(())
    }

    /// Find a device that handles the given MMIO address
    pub async fn find_mmio_device(&self, phys_addr: u64) -> Option<MmioDeviceHandle> {
        let mmio_regions = self.mmio_regions.read().await;

        for region in mmio_regions.iter() {
            let end_addr = region.base_addr + region.size;
            if phys_addr >= region.base_addr && phys_addr < end_addr {
                return Some(MmioDeviceHandle {
                    base_addr: region.base_addr,
                    device: region.device.clone(),
                    device_name: region.device_name.clone(),
                });
            }
        }

        None
    }

    /// Find a device that handles the given I/O port
    pub async fn find_io_device(&self, port: u16) -> Option<IoDeviceHandle> {
        let io_ports = self.io_ports.read().await;

        for mapping in io_ports.iter() {
            if port >= mapping.start_port && port <= mapping.end_port {
                return Some(IoDeviceHandle {
                    base_port: mapping.start_port,
                    device: mapping.device.clone(),
                    device_name: mapping.device_name.clone(),
                });
            }
        }

        None
    }
}

/// Handle for MMIO device access
pub struct MmioDeviceHandle {
    base_addr: u64,
    device: Arc<RwLock<dyn Device>>,
    device_name: String,
}

impl MmioDeviceHandle {
    /// Get the base address of this MMIO region
    pub fn base_address(&self) -> u64 {
        self.base_addr
    }

    /// Get the device name
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Read a register at the given offset
    pub async fn read_register(&self, offset: u64) -> Result<u32> {
        let device = self.device.read().await;
        let mut data = [0u8; 4];
        device.read(offset, &mut data).await?;
        Ok(u32::from_le_bytes(data))
    }

    /// Write a register at the given offset
    pub async fn write_register(&self, offset: u64, value: u32) -> Result<()> {
        let mut device = self.device.write().await;
        let data = value.to_le_bytes();
        device.write(offset, &data).await
    }
}

/// Handle for I/O port device access
pub struct IoDeviceHandle {
    base_port: u16,
    device: Arc<RwLock<dyn Device>>,
    device_name: String,
}

impl IoDeviceHandle {
    /// Get the base port of this I/O range
    pub fn base_port(&self) -> u16 {
        self.base_port
    }

    /// Get the device name
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Read a register at the given offset
    pub async fn read_register(&self, offset: u64) -> Result<u32> {
        let device = self.device.read().await;
        let mut data = [0u8; 4];
        device.read(offset, &mut data).await?;
        Ok(u32::from_le_bytes(data))
    }

    /// Write a register at the given offset
    pub async fn write_register(&self, offset: u64, value: u32) -> Result<()> {
        let mut device = self.device.write().await;
        let data = value.to_le_bytes();
        device.write(offset, &data).await
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyDevice {
        name: String,
    }

    #[async_trait]
    impl Device for DummyDevice {
        fn device_type(&self) -> DeviceType {
            DeviceType::Custom
        }

        fn name(&self) -> &str {
            &self.name
        }

        async fn init(&mut self) -> Result<()> {
            Ok(())
        }

        async fn read(&self, _offset: u64, _data: &mut [u8]) -> Result<()> {
            Ok(())
        }

        async fn write(&mut self, _offset: u64, _data: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn reset(&mut self) -> Result<()> {
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_device_registration() {
        let manager = DeviceManager::new();
        let device = Arc::new(RwLock::new(DummyDevice {
            name: "test".to_string(),
        }));

        manager
            .register_device("test".to_string(), device)
            .await
            .unwrap();
        assert!(manager.get_device("test").await.is_some());
    }
}
