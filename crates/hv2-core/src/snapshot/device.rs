//! Device state serialization for snapshots
//!
//! This module provides a framework for serializing and deserializing
//! device state for VM snapshots.

use super::types::DeviceSnapshot;

/// Device state serialization result
pub type DeviceResult<T> = Result<T, DeviceStateError>;

/// Device state error
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceStateError {
    /// Device not found
    #[error("Device not found: {0}")]
    DeviceNotFound(String),
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Deserialization error
    #[error("Deserialization error: {0}")]
    DeserializationError(String),
    /// Version mismatch
    #[error("Version mismatch: expected {expected}, found {found}")]
    VersionMismatch { expected: u32, found: u32 },
    /// Invalid state data
    #[error("Invalid state: {0}")]
    InvalidState(String),
    /// Device not registered
    #[error("Device not registered: {0}")]
    NotRegistered(String),
    /// Checksum error
    #[error("Checksum verification failed")]
    ChecksumError,
}

/// Trait for devices that can be snapshotted
pub trait Snapshottable {
    /// Get the device type identifier
    fn device_type(&self) -> &str;

    /// Get the device instance name
    fn device_name(&self) -> &str;

    /// Get the state format version
    fn state_version(&self) -> u32 {
        1
    }

    /// Serialize the device state
    fn save_state(&self) -> DeviceResult<Vec<u8>>;

    /// Restore the device state
    fn restore_state(&mut self, data: &[u8]) -> DeviceResult<()>;

    /// Check if state can be migrated from an older version
    fn can_migrate_from(&self, version: u32) -> bool {
        version == self.state_version()
    }
}

/// Device state serializer
#[derive(Debug, Default)]
pub struct DeviceStateSerializer {
    /// Buffer for serialized data
    buffer: Vec<u8>,
    /// Current position
    position: usize,
}

impl DeviceStateSerializer {
    /// Create a new serializer
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(4096),
            position: 0,
        }
    }

    /// Create with pre-allocated capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            position: 0,
        }
    }

    /// Write a u8
    pub fn write_u8(&mut self, value: u8) {
        self.buffer.push(value);
    }

    /// Write a u16 (little-endian)
    pub fn write_u16(&mut self, value: u16) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a u32 (little-endian)
    pub fn write_u32(&mut self, value: u32) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a u64 (little-endian)
    pub fn write_u64(&mut self, value: u64) {
        self.buffer.extend_from_slice(&value.to_le_bytes());
    }

    /// Write a bool
    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(if value { 1 } else { 0 });
    }

    /// Write a byte slice with length prefix
    pub fn write_bytes(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.buffer.extend_from_slice(data);
    }

    /// Write a string with length prefix
    pub fn write_string(&mut self, s: &str) {
        self.write_bytes(s.as_bytes());
    }

    /// Write a fixed-size array
    pub fn write_array(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Get the serialized data
    pub fn into_bytes(self) -> Vec<u8> {
        self.buffer
    }

    /// Get current length
    pub fn len(&self) -> usize {
        self.buffer.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

/// Device state deserializer
#[derive(Debug)]
pub struct DeviceStateDeserializer<'a> {
    /// Data to deserialize
    data: &'a [u8],
    /// Current position
    position: usize,
}

impl<'a> DeviceStateDeserializer<'a> {
    /// Create a new deserializer
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }

    /// Read a u8
    pub fn read_u8(&mut self) -> DeviceResult<u8> {
        if self.position >= self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let value = self.data[self.position];
        self.position += 1;
        Ok(value)
    }

    /// Read a u16 (little-endian)
    pub fn read_u16(&mut self) -> DeviceResult<u16> {
        if self.position + 2 > self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let bytes = &self.data[self.position..self.position + 2];
        self.position += 2;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Read a u32 (little-endian)
    pub fn read_u32(&mut self) -> DeviceResult<u32> {
        if self.position + 4 > self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let bytes = &self.data[self.position..self.position + 4];
        self.position += 4;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a u64 (little-endian)
    pub fn read_u64(&mut self) -> DeviceResult<u64> {
        if self.position + 8 > self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let bytes = &self.data[self.position..self.position + 8];
        self.position += 8;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Read a bool
    pub fn read_bool(&mut self) -> DeviceResult<bool> {
        Ok(self.read_u8()? != 0)
    }

    /// Read a byte slice with length prefix
    pub fn read_bytes(&mut self) -> DeviceResult<Vec<u8>> {
        let len = self.read_u32()? as usize;
        if self.position + len > self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let data = self.data[self.position..self.position + len].to_vec();
        self.position += len;
        Ok(data)
    }

    /// Read a string with length prefix
    pub fn read_string(&mut self) -> DeviceResult<String> {
        let bytes = self.read_bytes()?;
        String::from_utf8(bytes)
            .map_err(|e| DeviceStateError::DeserializationError(format!("Invalid UTF-8: {}", e)))
    }

    /// Read a fixed-size array
    pub fn read_array(&mut self, len: usize) -> DeviceResult<Vec<u8>> {
        if self.position + len > self.data.len() {
            return Err(DeviceStateError::DeserializationError(
                "Unexpected end of data".to_string(),
            ));
        }
        let data = self.data[self.position..self.position + len].to_vec();
        self.position += len;
        Ok(data)
    }

    /// Get remaining bytes
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    /// Check if at end
    pub fn is_at_end(&self) -> bool {
        self.position >= self.data.len()
    }
}

/// Device state manager for coordinating device snapshots
#[derive(Default)]
pub struct DeviceStateManager {
    /// Device snapshots
    snapshots: Vec<DeviceSnapshot>,
    /// Statistics
    stats: DeviceStateStats,
}

impl std::fmt::Debug for DeviceStateManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceStateManager")
            .field("snapshots", &self.snapshots)
            .field("stats", &self.stats)
            .finish()
    }
}

impl DeviceStateManager {
    /// Create a new device state manager
    pub fn new() -> Self {
        Self {
            snapshots: Vec::new(),
            stats: DeviceStateStats::default(),
        }
    }

    /// Capture state from a device
    pub fn capture_device(&mut self, device: &dyn Snapshottable) -> DeviceResult<DeviceSnapshot> {
        let state_data = device.save_state()?;

        let mut snapshot = DeviceSnapshot::new(device.device_type(), device.device_name())
            .with_state(state_data)
            .with_version(device.state_version());

        snapshot.calculate_checksum();

        self.stats.devices_captured += 1;
        self.stats.bytes_captured += snapshot.size_bytes() as u64;

        self.snapshots.push(snapshot.clone());
        Ok(snapshot)
    }

    /// Restore state to a device
    pub fn restore_device(
        &mut self,
        device: &mut dyn Snapshottable,
        snapshot: &DeviceSnapshot,
    ) -> DeviceResult<()> {
        // Verify checksum
        if !snapshot.verify_checksum() {
            self.stats.checksum_failures += 1;
            return Err(DeviceStateError::ChecksumError);
        }

        // Check version compatibility
        if !device.can_migrate_from(snapshot.version) {
            return Err(DeviceStateError::VersionMismatch {
                expected: device.state_version(),
                found: snapshot.version,
            });
        }

        device.restore_state(&snapshot.state_data)?;

        self.stats.devices_restored += 1;
        self.stats.bytes_restored += snapshot.size_bytes() as u64;

        Ok(())
    }

    /// Get a snapshot by device name
    pub fn get_snapshot(&self, name: &str) -> Option<&DeviceSnapshot> {
        self.snapshots.iter().find(|s| s.name == name)
    }

    /// Get all snapshots
    pub fn snapshots(&self) -> &[DeviceSnapshot] {
        &self.snapshots
    }

    /// Clear all snapshots
    pub fn clear(&mut self) {
        self.snapshots.clear();
    }

    /// Get statistics
    pub fn stats(&self) -> &DeviceStateStats {
        &self.stats
    }

    /// Total size of all snapshots
    pub fn total_size(&self) -> u64 {
        self.snapshots.iter().map(|s| s.size_bytes() as u64).sum()
    }
}

/// Device state statistics
#[derive(Debug, Clone, Default)]
pub struct DeviceStateStats {
    /// Devices captured
    pub devices_captured: u64,
    /// Devices restored
    pub devices_restored: u64,
    /// Bytes captured
    pub bytes_captured: u64,
    /// Bytes restored
    pub bytes_restored: u64,
    /// Checksum verification failures
    pub checksum_failures: u64,
    /// Version migration count
    pub version_migrations: u64,
}

/// Example snapshottable device for testing
#[derive(Debug, Default)]
pub struct TestDevice {
    name: String,
    value: u64,
    enabled: bool,
    buffer: Vec<u8>,
}

impl TestDevice {
    /// Create a new test device
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: 0,
            enabled: false,
            buffer: Vec::new(),
        }
    }

    /// Set value
    pub fn set_value(&mut self, value: u64) {
        self.value = value;
    }

    /// Set enabled
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Set buffer
    pub fn set_buffer(&mut self, buffer: Vec<u8>) {
        self.buffer = buffer;
    }
}

impl Snapshottable for TestDevice {
    fn device_type(&self) -> &str {
        "test-device"
    }

    fn device_name(&self) -> &str {
        &self.name
    }

    fn save_state(&self) -> DeviceResult<Vec<u8>> {
        let mut ser = DeviceStateSerializer::new();
        ser.write_u64(self.value);
        ser.write_bool(self.enabled);
        ser.write_bytes(&self.buffer);
        Ok(ser.into_bytes())
    }

    fn restore_state(&mut self, data: &[u8]) -> DeviceResult<()> {
        let mut de = DeviceStateDeserializer::new(data);
        self.value = de.read_u64()?;
        self.enabled = de.read_bool()?;
        self.buffer = de.read_bytes()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_state_error_display() {
        let err = DeviceStateError::DeviceNotFound("test".to_string());
        assert!(format!("{}", err).contains("test"));
    }

    #[test]
    fn test_serializer_primitives() {
        let mut ser = DeviceStateSerializer::new();
        ser.write_u8(0x12);
        ser.write_u16(0x3456);
        ser.write_u32(0x789ABCDE);
        ser.write_u64(0x0123456789ABCDEF);
        ser.write_bool(true);
        ser.write_bool(false);

        let data = ser.into_bytes();

        let mut de = DeviceStateDeserializer::new(&data);
        assert_eq!(de.read_u8().unwrap(), 0x12);
        assert_eq!(de.read_u16().unwrap(), 0x3456);
        assert_eq!(de.read_u32().unwrap(), 0x789ABCDE);
        assert_eq!(de.read_u64().unwrap(), 0x0123456789ABCDEF);
        assert!(de.read_bool().unwrap());
        assert!(!de.read_bool().unwrap());
    }

    #[test]
    fn test_serializer_bytes() {
        let mut ser = DeviceStateSerializer::new();
        ser.write_bytes(&[1, 2, 3, 4, 5]);

        let data = ser.into_bytes();

        let mut de = DeviceStateDeserializer::new(&data);
        let bytes = de.read_bytes().unwrap();
        assert_eq!(bytes, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn test_serializer_string() {
        let mut ser = DeviceStateSerializer::new();
        ser.write_string("Hello, World!");

        let data = ser.into_bytes();

        let mut de = DeviceStateDeserializer::new(&data);
        let s = de.read_string().unwrap();
        assert_eq!(s, "Hello, World!");
    }

    #[test]
    fn test_serializer_array() {
        let mut ser = DeviceStateSerializer::new();
        ser.write_array(&[0xAA, 0xBB, 0xCC, 0xDD]);

        let data = ser.into_bytes();

        let mut de = DeviceStateDeserializer::new(&data);
        let arr = de.read_array(4).unwrap();
        assert_eq!(arr, vec![0xAA, 0xBB, 0xCC, 0xDD]);
    }

    #[test]
    fn test_deserializer_end_of_data() {
        let data = [0x12];
        let mut de = DeviceStateDeserializer::new(&data);

        assert!(de.read_u8().is_ok());
        assert!(de.read_u8().is_err());
    }

    #[test]
    fn test_deserializer_remaining() {
        let data = [1, 2, 3, 4, 5];
        let mut de = DeviceStateDeserializer::new(&data);

        assert_eq!(de.remaining(), 5);
        de.read_u8().unwrap();
        assert_eq!(de.remaining(), 4);
    }

    #[test]
    fn test_test_device_save_restore() {
        let mut device = TestDevice::new("dev1");
        device.set_value(12345);
        device.set_enabled(true);
        device.set_buffer(vec![1, 2, 3]);

        let state = device.save_state().unwrap();

        let mut restored = TestDevice::new("dev1");
        restored.restore_state(&state).unwrap();

        assert_eq!(restored.value, 12345);
        assert!(restored.enabled);
        assert_eq!(restored.buffer, vec![1, 2, 3]);
    }

    #[test]
    fn test_device_state_manager_capture() {
        let mut manager = DeviceStateManager::new();
        let device = TestDevice::new("test1");

        let snapshot = manager.capture_device(&device).unwrap();

        assert_eq!(snapshot.device_type, "test-device");
        assert_eq!(snapshot.name, "test1");
        assert_eq!(manager.stats().devices_captured, 1);
    }

    #[test]
    fn test_device_state_manager_restore() {
        let mut manager = DeviceStateManager::new();

        let mut device = TestDevice::new("test1");
        device.set_value(999);
        device.set_enabled(true);

        let snapshot = manager.capture_device(&device).unwrap();

        let mut new_device = TestDevice::new("test1");
        manager.restore_device(&mut new_device, &snapshot).unwrap();

        assert_eq!(new_device.value, 999);
        assert!(new_device.enabled);
        assert_eq!(manager.stats().devices_restored, 1);
    }

    #[test]
    fn test_device_state_manager_get_snapshot() {
        let mut manager = DeviceStateManager::new();
        let device = TestDevice::new("test1");

        manager.capture_device(&device).unwrap();

        assert!(manager.get_snapshot("test1").is_some());
        assert!(manager.get_snapshot("nonexistent").is_none());
    }

    #[test]
    fn test_device_state_manager_clear() {
        let mut manager = DeviceStateManager::new();
        let device = TestDevice::new("test1");

        manager.capture_device(&device).unwrap();
        assert!(!manager.snapshots().is_empty());

        manager.clear();
        assert!(manager.snapshots().is_empty());
    }

    #[test]
    fn test_device_state_manager_total_size() {
        let mut manager = DeviceStateManager::new();

        let mut device1 = TestDevice::new("test1");
        device1.set_buffer(vec![0; 100]);

        let mut device2 = TestDevice::new("test2");
        device2.set_buffer(vec![0; 200]);

        manager.capture_device(&device1).unwrap();
        manager.capture_device(&device2).unwrap();

        assert!(manager.total_size() > 300);
    }

    #[test]
    fn test_device_checksum_verification() {
        let mut manager = DeviceStateManager::new();
        let device = TestDevice::new("test1");

        let mut snapshot = manager.capture_device(&device).unwrap();

        // Corrupt the data
        if !snapshot.state_data.is_empty() {
            snapshot.state_data[0] ^= 0xFF;
        }

        let mut new_device = TestDevice::new("test1");
        let result = manager.restore_device(&mut new_device, &snapshot);

        assert!(matches!(result, Err(DeviceStateError::ChecksumError)));
    }

    #[test]
    fn test_snapshottable_trait() {
        let device = TestDevice::new("test");
        assert_eq!(device.device_type(), "test-device");
        assert_eq!(device.device_name(), "test");
        assert_eq!(device.state_version(), 1);
        assert!(device.can_migrate_from(1));
        assert!(!device.can_migrate_from(2));
    }
}
