//! Device Registration Integration Tests
//!
//! Tests the device MMIO and I/O port registration system.

use async_trait::async_trait;
use hv2_core::{Device, DeviceManager, DeviceType, Result};
use std::sync::Arc;
// Use parking_lot RwLock for internal synchronous state
use parking_lot::RwLock as SyncRwLock;
// Use tokio RwLock for async device registration (what DeviceManager expects)
use tokio::sync::RwLock;

/// Test device that supports both MMIO and I/O ports
struct TestDevice {
    name: String,
    registers: Arc<SyncRwLock<Vec<u8>>>,
}

impl TestDevice {
    fn new(name: String, register_count: usize) -> Self {
        Self {
            name,
            registers: Arc::new(SyncRwLock::new(vec![0; register_count])),
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

    // Create and register device - cast to dyn Device
    let device: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("test-mmio".to_string(), 256)));
    manager
        .register_device("test-mmio".to_string(), device)
        .await?;

    // Register MMIO region
    manager
        .register_mmio_region("test-mmio".to_string(), 0x1000, 256)
        .await?;

    // Find device by MMIO address
    let handle = manager.find_mmio_device(0x1000).await;
    assert!(handle.is_some());

    let handle = handle.unwrap();
    assert_eq!(handle.base_address(), 0x1000);
    assert_eq!(handle.device_name(), "test-mmio");

    // Address in the middle of the region
    let handle = manager.find_mmio_device(0x1080).await;
    assert!(handle.is_some());

    // Address at the end (exclusive)
    let handle = manager.find_mmio_device(0x1100).await;
    assert!(handle.is_none());

    // Address before the region
    let handle = manager.find_mmio_device(0x0FFF).await;
    assert!(handle.is_none());

    Ok(())
}

#[tokio::test]
async fn test_io_port_registration() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device - cast to dyn Device
    let device: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("test-io".to_string(), 8)));
    manager
        .register_device("test-io".to_string(), device)
        .await?;

    // Register I/O port range
    manager
        .register_io_port_range("test-io".to_string(), 0x3F8, 0x3FF)
        .await?;

    // Find device by I/O port
    let handle = manager.find_io_device(0x3F8).await;
    assert!(handle.is_some());

    let handle = handle.unwrap();
    assert_eq!(handle.base_port(), 0x3F8);
    assert_eq!(handle.device_name(), "test-io");

    // Port in the middle of the range
    let handle = manager.find_io_device(0x3FC).await;
    assert!(handle.is_some());

    // Port at the end (inclusive)
    let handle = manager.find_io_device(0x3FF).await;
    assert!(handle.is_some());

    // Port after the range
    let handle = manager.find_io_device(0x400).await;
    assert!(handle.is_none());

    // Port before the range
    let handle = manager.find_io_device(0x3F7).await;
    assert!(handle.is_none());

    Ok(())
}

#[tokio::test]
async fn test_mmio_read_write() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device - cast to dyn Device
    let device: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("test-rw".to_string(), 256)));
    manager
        .register_device("test-rw".to_string(), device)
        .await?;
    manager
        .register_mmio_region("test-rw".to_string(), 0x2000, 256)
        .await?;

    // Find and write to device
    let handle = manager.find_mmio_device(0x2000).await.unwrap();

    handle.write_register(0, 0x12345678, 4).await?;
    let value = handle.read_register(0, 4).await?;
    assert_eq!(value, 0x12345678);

    // Write to different offset
    handle.write_register(4, 0xDEADBEEF, 4).await?;
    let value = handle.read_register(4, 4).await?;
    assert_eq!(value, 0xDEADBEEF);

    // First value should be unchanged
    let value = handle.read_register(0, 4).await?;
    assert_eq!(value, 0x12345678);

    Ok(())
}

#[tokio::test]
async fn test_io_port_read_write() -> Result<()> {
    let manager = DeviceManager::new();

    // Create and register device - cast to dyn Device
    let device: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("test-io-rw".to_string(), 16)));
    manager
        .register_device("test-io-rw".to_string(), device)
        .await?;
    manager
        .register_io_port_range("test-io-rw".to_string(), 0x500, 0x50F)
        .await?;

    // Find and write to device
    let handle = manager.find_io_device(0x500).await.unwrap();

    handle.write_register(0, 0xABCDEF00, 4).await?;
    let value = handle.read_register(0, 4).await?;
    assert_eq!(value, 0xABCDEF00);

    Ok(())
}

#[tokio::test]
async fn test_mmio_region_overlap_detection() {
    let manager = DeviceManager::new();

    // Register first device at 0x3000-0x3100 (256 bytes)
    let device1: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device1".to_string(), 256)));
    manager
        .register_device("device1".to_string(), device1)
        .await
        .unwrap();
    manager
        .register_mmio_region("device1".to_string(), 0x3000, 256)
        .await
        .unwrap();

    // Test 1: Complete overlap
    let device2: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device2".to_string(), 128)));
    manager
        .register_device("device2".to_string(), device2)
        .await
        .unwrap();
    let result = manager
        .register_mmio_region("device2".to_string(), 0x3000, 128)
        .await;
    assert!(result.is_err());

    // Test 2: Partial overlap (start before, end inside)
    // device3: 0x2F00-0x3001 (257 bytes) should overlap device1 (0x3000-0x3100)
    let device3: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device3".to_string(), 257)));
    manager
        .register_device("device3".to_string(), device3)
        .await
        .unwrap();
    let result = manager
        .register_mmio_region("device3".to_string(), 0x2F00, 257)
        .await;
    assert!(result.is_err());

    // Test 3: Partial overlap (start inside, end after)
    let device4: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device4".to_string(), 256)));
    manager
        .register_device("device4".to_string(), device4)
        .await
        .unwrap();
    let result = manager
        .register_mmio_region("device4".to_string(), 0x30F0, 256)
        .await;
    assert!(result.is_err());

    // Test 4: No overlap (before) - region ends at 0x2F00, device1 starts at 0x3000
    let device5: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device5".to_string(), 256)));
    manager
        .register_device("device5".to_string(), device5)
        .await
        .unwrap();
    let result = manager
        .register_mmio_region("device5".to_string(), 0x2E00, 256)
        .await;
    assert!(result.is_ok());

    // Test 5: No overlap (after) - region starts at 0x3100, device1 ends at 0x3100
    let device6: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device6".to_string(), 128)));
    manager
        .register_device("device6".to_string(), device6)
        .await
        .unwrap();
    let result = manager
        .register_mmio_region("device6".to_string(), 0x3100, 128)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_io_port_overlap_detection() {
    let manager = DeviceManager::new();

    // Register first device
    let device1: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device1".to_string(), 16)));
    manager
        .register_device("device1".to_string(), device1)
        .await
        .unwrap();
    manager
        .register_io_port_range("device1".to_string(), 0x600, 0x60F)
        .await
        .unwrap();

    // Try to register overlapping range
    let device2: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("device2".to_string(), 8)));
    manager
        .register_device("device2".to_string(), device2)
        .await
        .unwrap();

    // Complete overlap
    let result = manager
        .register_io_port_range("device2".to_string(), 0x600, 0x607)
        .await;
    assert!(result.is_err());

    // Partial overlap
    let result = manager
        .register_io_port_range("device2".to_string(), 0x5F8, 0x605)
        .await;
    assert!(result.is_err());

    // No overlap (adjacent is OK)
    let result = manager
        .register_io_port_range("device2".to_string(), 0x610, 0x617)
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_devices() -> Result<()> {
    let manager = DeviceManager::new();

    // Register serial device
    let serial: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("serial".to_string(), 8)));
    manager
        .register_device("serial".to_string(), serial)
        .await?;
    manager
        .register_io_port_range("serial".to_string(), 0x3F8, 0x3FF)
        .await?;

    // Register timer device
    let timer: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("timer".to_string(), 4)));
    manager.register_device("timer".to_string(), timer).await?;
    manager
        .register_io_port_range("timer".to_string(), 0x40, 0x43)
        .await?;

    // Register video device with MMIO
    let video: Arc<RwLock<dyn Device>> =
        Arc::new(RwLock::new(TestDevice::new("video".to_string(), 1024)));
    manager.register_device("video".to_string(), video).await?;
    manager
        .register_mmio_region("video".to_string(), 0xA0000, 1024)
        .await?;

    // Find each device
    let handle = manager.find_io_device(0x3F8).await;
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "serial");

    let handle = manager.find_io_device(0x40).await;
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "timer");

    let handle = manager.find_mmio_device(0xA0000).await;
    assert!(handle.is_some());
    assert_eq!(handle.unwrap().device_name(), "video");

    // Write to each device
    let serial_handle = manager.find_io_device(0x3F8).await.unwrap();
    serial_handle.write_register(0, 0x41, 4).await?;

    let timer_handle = manager.find_io_device(0x40).await.unwrap();
    timer_handle.write_register(0, 100, 4).await?;

    let video_handle = manager.find_mmio_device(0xA0000).await.unwrap();
    video_handle.write_register(0, 0x12345678, 4).await?;

    // Verify each device
    assert_eq!(serial_handle.read_register(0, 4).await?, 0x41);
    assert_eq!(timer_handle.read_register(0, 4).await?, 100);
    assert_eq!(video_handle.read_register(0, 4).await?, 0x12345678);

    Ok(())
}
