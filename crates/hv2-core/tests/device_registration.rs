//! Device Registration Integration Tests
//!
//! Tests the device MMIO and I/O port registration system.

use async_trait::async_trait;
use hv2_core::{Device, DeviceManager, DeviceType, Result};
use parking_lot::RwLock;
use std::sync::Arc;

/// Test device that supports both MMIO and I/O ports
struct TestDevice {
    name: String,
    registers: Arc<RwLock<Vec<u8>>>,
}

impl TestDevice {
    fn new(name: String, register_count: usize) -> Self {
        Self {
            name,
            registers: Arc::new(RwLock::new(vec![0; register_count])),
        }
    }
}

#[async_trait]
impl Device for TestDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Custom
    }

    async fn init(&mut self) -> Result<()> {
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        let registers = self.registers.read();
        let start = offset as usize;
        let end = (start + data.len()).min(registers.len());
        data[..end - start].copy_from_slice(&registers[start..end]);
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let mut registers = self.registers.write();
        let start = offset as usize;
        let end = (start + data.len()).min(registers.len());
        registers[start..end].copy_from_slice(&data[..end - start]);
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut registers = self.registers.write();
        registers.fill(0);
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn test_mmio_region_registration() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device
    let device = Arc::new(RwLock::new(TestDevice::new("test-mmio".to_string(), 256)));
    manager.register_device("test-mmio".to_string(), device)?;

    // Register MMIO region
    manager.register_mmio_region("test-mmio".to_string(), 0x1000, 256)?;

    // Find device by MMIO address
    let handle = manager.find_mmio_device(0x1000);
    assert!(handle.is_some());

    let handle = handle.unwrap();
    assert_eq!(handle.base_address(), 0x1000);
    assert_eq!(handle.device_name(), "test-mmio");

    // Address in the middle of the region
    let handle = manager.find_mmio_device(0x1080);
    assert!(handle.is_some());

    // Address at the end (exclusive)
    let handle = manager.find_mmio_device(0x1100);
    assert!(handle.is_none());

    // Address before the region
    let handle = manager.find_mmio_device(0x0FFF);
    assert!(handle.is_none());

    Ok(())
}

#[tokio::test]
async fn test_io_port_registration() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device
    let device = Arc::new(RwLock::new(TestDevice::new("test-io".to_string(), 8)));
    manager.register_device("test-io".to_string(), device)?;

    // Register I/O port range
    manager.register_io_port_range("test-io".to_string(), 0x3F8, 0x3FF)?;

    // Find device by I/O port
    let handle = manager.find_io_device(0x3F8);
    assert!(handle.is_some());

    let handle = handle.unwrap();
    assert_eq!(handle.base_port(), 0x3F8);
    assert_eq!(handle.device_name(), "test-io");

    // Port in the middle of the range
    let handle = manager.find_io_device(0x3FC);
    assert!(handle.is_some());

    // Port at the end (inclusive)
    let handle = manager.find_io_device(0x3FF);
    assert!(handle.is_some());

    // Port after the range
    let handle = manager.find_io_device(0x400);
    assert!(handle.is_none());

    // Port before the range
    let handle = manager.find_io_device(0x3F7);
    assert!(handle.is_none());

    Ok(())
}

#[tokio::test]
async fn test_mmio_read_write() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device
    let device = Arc::new(RwLock::new(TestDevice::new("test-rw".to_string(), 256)));
    manager.register_device("test-rw".to_string(), device)?;
    manager.register_mmio_region("test-rw".to_string(), 0x2000, 256)?;

    // Find and write to device
    let handle = manager.find_mmio_device(0x2000).unwrap();

    handle.write_register(0, 0x12345678).await?;
    let value = handle.read_register(0).await?;
    assert_eq!(value, 0x12345678);

    // Write to different offset
    handle.write_register(4, 0xDEADBEEF).await?;
    let value = handle.read_register(4).await?;
    assert_eq!(value, 0xDEADBEEF);

    // First value should be unchanged
    let value = handle.read_register(0).await?;
    assert_eq!(value, 0x12345678);

    Ok(())
}

#[tokio::test]
async fn test_io_port_read_write() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device
    let device = Arc::new(RwLock::new(TestDevice::new("test-io-rw".to_string(), 16)));
    manager.register_device("test-io-rw".to_string(), device)?;
    manager.register_io_port_range("test-io-rw".to_string(), 0x500, 0x50F)?;

    // Find and write to device
    let handle = manager.find_io_device(0x500).unwrap();

    handle.write_register(0, 0xABCDEF00).await?;
    let value = handle.read_register(0).await?;
    assert_eq!(value, 0xABCDEF00);

    Ok(())
}

#[tokio::test]
async fn test_mmio_region_overlap_detection() {
    let manager = DeviceManager::new();

    // Register first device at 0x3000-0x3100 (256 bytes)
    let device1 = Arc::new(RwLock::new(TestDevice::new("device1".to_string(), 256)));
    manager
        .register_device("device1".to_string(), device1)
        .unwrap();
    manager
        .register_mmio_region("device1".to_string(), 0x3000, 256)
        .unwrap();

    // Test 1: Complete overlap
    let device2 = Arc::new(RwLock::new(TestDevice::new("device2".to_string(), 128)));
    manager
        .register_device("device2".to_string(), device2)
        .unwrap();
    let result = manager.register_mmio_region("device2".to_string(), 0x3000, 128);
    assert!(result.is_err());

    // Test 2: Partial overlap (start before, end inside)
    // device3: 0x2F00-0x3001 (257 bytes) should overlap device1 (0x3000-0x3100)
    let device3 = Arc::new(RwLock::new(TestDevice::new("device3".to_string(), 257)));
    manager
        .register_device("device3".to_string(), device3)
        .unwrap();
    let result = manager.register_mmio_region("device3".to_string(), 0x2F00, 257);
    assert!(result.is_err());

    // Test 3: Partial overlap (start inside, end after)
    let device4 = Arc::new(RwLock::new(TestDevice::new("device4".to_string(), 256)));
    manager
        .register_device("device4".to_string(), device4)
        .unwrap();
    let result = manager.register_mmio_region("device4".to_string(), 0x30F0, 256);
    assert!(result.is_err());

    // Test 4: No overlap (before) - region ends at 0x2F00, device1 starts at 0x3000
    let device5 = Arc::new(RwLock::new(TestDevice::new("device5".to_string(), 256)));
    manager
        .register_device("device5".to_string(), device5)
        .unwrap();
    let result = manager.register_mmio_region("device5".to_string(), 0x2E00, 256);
    assert!(result.is_ok());

    // Test 5: No overlap (after) - region starts at 0x3100, device1 ends at 0x3100
    let device6 = Arc::new(RwLock::new(TestDevice::new("device6".to_string(), 128)));
    manager
        .register_device("device6".to_string(), device6)
        .unwrap();
    let result = manager.register_mmio_region("device6".to_string(), 0x3100, 128);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_io_port_overlap_detection() {
    let manager = DeviceManager::new();

    // Register first device
    let device1 = Arc::new(RwLock::new(TestDevice::new("device1".to_string(), 16)));
    manager
        .register_device("device1".to_string(), device1)
        .unwrap();
    manager
        .register_io_port_range("device1".to_string(), 0x600, 0x60F)
        .unwrap();

    // Try to register overlapping range
    let device2 = Arc::new(RwLock::new(TestDevice::new("device2".to_string(), 8)));
    manager
        .register_device("device2".to_string(), device2)
        .unwrap();

    // Complete overlap
    let result = manager.register_io_port_range("device2".to_string(), 0x600, 0x607);
    assert!(result.is_err());

    // Partial overlap
    let result = manager.register_io_port_range("device2".to_string(), 0x5F8, 0x605);
    assert!(result.is_err());

    // No overlap (adjacent is OK)
    let result = manager.register_io_port_range("device2".to_string(), 0x610, 0x617);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_devices() -> Result<()> {
    let manager = DeviceManager::new();

    // Register serial device
    let serial = Arc::new(RwLock::new(TestDevice::new("serial".to_string(), 8)));
    manager.register_device("serial".to_string(), serial)?;
    manager.register_io_port_range("serial".to_string(), 0x3F8, 0x3FF)?;

    // Register timer device
    let timer = Arc::new(RwLock::new(TestDevice::new("timer".to_string(), 4)));
    manager.register_device("timer".to_string(), timer)?;
    manager.register_io_port_range("timer".to_string(), 0x40, 0x43)?;

    // Register video device with MMIO
    let video = Arc::new(RwLock::new(TestDevice::new("video".to_string(), 1024)));
    manager.register_device("video".to_string(), video)?;
    manager.register_mmio_region("video".to_string(), 0xA0000, 1024)?;

    // Find each device
    let handle = manager.find_io_device(0x3F8);
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "serial");

    let handle = manager.find_io_device(0x40);
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "timer");

    let handle = manager.find_mmio_device(0xA0000);
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "video");

    // Write to each device
    let serial_handle = manager.find_io_device(0x3F8).unwrap();
    serial_handle.write_register(0, 0x41).await?;

    let timer_handle = manager.find_io_device(0x40).unwrap();
    timer_handle.write_register(0, 100).await?;

    let video_handle = manager.find_mmio_device(0xA0000).unwrap();
    video_handle.write_register(0, 0x12345678).await?;

    // Verify each device
    assert_eq!(serial_handle.read_register(0).await?, 0x41);
    assert_eq!(timer_handle.read_register(0).await?, 100);
    assert_eq!(video_handle.read_register(0).await?, 0x12345678);

    Ok(())
}
