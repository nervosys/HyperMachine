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

    /// Accept somewhere to report an interrupt raised outside a guest access.
    ///
    /// Called once, when the device is registered. The default ignores it,
    /// which is right for a device that only ever interrupts in response to
    /// something the guest just did.
    fn set_interrupt_sink(&mut self, sink: Arc<dyn InterruptSink>) {
        let _ = sink;
    }

    /// The interrupt line this device is asserting right now, if any.
    ///
    /// Polled after every access, because that is when a device's interrupt
    /// condition changes: a UART becomes ready to send the moment the guest
    /// writes a byte, and has something to report the moment one arrives.
    ///
    /// `None` -- the default -- means this device never interrupts, which is
    /// true of most of them and is why this is not a required method.
    fn pending_interrupt(&self) -> Option<u8> {
        None
    }

    /// Deliver input from the host to this device.
    ///
    /// The other direction from [`Self::console_output`]: what someone types
    /// at a guest's console. Defaults to refusing, because a device that
    /// cannot take input should say so rather than accept the bytes and drop
    /// them -- a caller whose keystrokes vanish has no way to tell that from a
    /// guest that ignored them.
    ///
    /// # Errors
    ///
    /// [`Error::Device`] when this device accepts no input.
    fn console_input(&mut self, data: &[u8]) -> Result<()> {
        let _ = data;
        Err(Error::Device(format!(
            "device '{}' accepts no console input",
            self.name()
        )))
    }

    /// Console bytes this device has buffered for the host to read, if it is
    /// the kind of device a guest writes a console to.
    ///
    /// Defaults to `None`, which means "not a console" and is distinct from
    /// `Some(vec![])`, "a console that has said nothing yet". Implementations
    /// must **not** consume the buffer: this exists so a caller can poll a
    /// guest's output without racing whatever else is draining it.
    fn console_output(&self) -> Option<Vec<u8>> {
        None
    }
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

/// Somewhere a device can report an interrupt it raised on its own.
///
/// [`Device::pending_interrupt`] only answers when the guest touches the
/// device, which covers a UART the guest is actively driving and nothing else.
/// A byte arriving from the host, a timer expiring, a virtqueue the host
/// filled -- all of those happen while the guest is idle, and a device with no
/// way to speak up then simply never gets serviced.
pub trait InterruptSink: Send + Sync + std::fmt::Debug {
    /// Raise `irq` as a pulse: asserted and released. Must not block, because
    /// this is called from device code that may hold a lock the interrupt
    /// handler will want.
    ///
    /// Right for a device whose interrupt the guest discovers by reading a
    /// status register it was going to read anyway -- a 16550's, say.
    fn raise(&self, irq: u8);

    /// Assert `irq` and leave it asserted.
    ///
    /// Right for a level-triggered line, which a virtio device's is: the
    /// specification has the driver clear the interrupt by writing
    /// `InterruptACK`, and a device that releases the line before the driver
    /// has acknowledged it is describing an interrupt rather than holding one.
    /// A pulse is not a safe stand-in -- it can be asserted and released
    /// between one delivery and the next, and the interrupt is then simply
    /// lost, which looks from the guest like a device that went quiet.
    ///
    /// Defaults to [`raise`](Self::raise), so a sink that predates this is a
    /// pulse and no worse than it was.
    fn assert_line(&self, irq: u8) {
        self.raise(irq);
    }

    /// Release `irq`, after the driver has acknowledged it.
    ///
    /// Defaults to doing nothing, which is correct for a sink whose
    /// [`assert_line`](Self::assert_line) is a pulse: there is nothing held.
    fn deassert_line(&self, irq: u8) {
        let _ = irq;
    }
}

/// Device manager
pub struct DeviceManager {
    devices: Arc<RwLock<HashMap<String, Arc<RwLock<dyn Device>>>>>,
    mmio_regions: Arc<RwLock<Vec<MmioRegionMapping>>>,
    io_ports: Arc<RwLock<Vec<IoPortMapping>>>,
    /// Handed to each device as it is registered, so a device never has to be
    /// told about it separately and cannot be left without one by accident.
    ///
    /// A synchronous lock because it is installed from `VM::new`, which is not
    /// async, and read on a path that already holds an async lock.
    interrupt_sink: parking_lot::RwLock<Option<Arc<dyn InterruptSink>>>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            devices: Arc::new(RwLock::new(HashMap::new())),
            mmio_regions: Arc::new(RwLock::new(Vec::new())),
            io_ports: Arc::new(RwLock::new(Vec::new())),
            interrupt_sink: parking_lot::RwLock::new(None),
        }
    }

    /// Register a device
    pub async fn register_device(
        &self,
        name: impl Into<String>,
        device: Arc<RwLock<dyn Device>>,
    ) -> Result<()> {
        let name = name.into();
        let mut devices = self.devices.write().await;

        if devices.contains_key(&name) {
            return Err(Error::Device(format!(
                "Device '{}' already registered",
                name
            )));
        }

        // Hand it the sink now rather than expecting a caller to remember. A
        // device registered without one cannot raise an interrupt of its own
        // and nothing would say so.
        let sink = self.interrupt_sink.read().clone();
        if let Some(sink) = sink {
            device.write().await.set_interrupt_sink(sink);
        }

        devices.insert(name, device);
        Ok(())
    }

    /// Install the sink devices report self-raised interrupts to.
    ///
    /// Applies to every device registered afterwards. Installed by `VM::new`
    /// before any device can be registered, so in practice that is all of
    /// them; a caller building a manager by hand has to install it first.
    pub fn set_interrupt_sink(&self, sink: Arc<dyn InterruptSink>) {
        *self.interrupt_sink.write() = Some(sink);
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

    /// Console output buffered by every registered console device.
    ///
    /// Returns `(device_name, bytes)` pairs sorted by name, so the order is
    /// stable across calls rather than following `HashMap` iteration. Devices
    /// that are not consoles are omitted entirely; a registered console that
    /// has produced nothing yet appears with an empty `Vec`, which is what
    /// lets a caller distinguish "no console attached" from "console is quiet".
    ///
    /// Reading does not consume: see [`Device::console_output`].
    pub async fn console_output(&self) -> Vec<(String, Vec<u8>)> {
        let devices: Vec<_> = self
            .devices
            .read()
            .await
            .iter()
            .map(|(name, device)| (name.clone(), device.clone()))
            .collect();

        let mut out = Vec::new();
        for (name, device) in devices {
            if let Some(bytes) = device.read().await.console_output() {
                out.push((name, bytes));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
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
    pub async fn read_register(&self, offset: u64, width: u8) -> Result<u32> {
        // `width` is the width the guest actually asked for, and handing the
        // device anything else is not a rounding error. Register files are
        // byte-wide and reads have side effects -- reading a UART receive
        // buffer pops a byte, reading its interrupt identification register
        // clears a pending interrupt -- so asking for four bytes when the
        // guest asked for one silently disturbs three registers it never
        // touched.
        let width = width.clamp(1, 4) as usize;
        let device = self.device.read().await;
        let mut data = [0u8; 4];
        device.read(offset, &mut data[..width]).await?;
        Ok(u32::from_le_bytes(data))
    }

    /// Write a register at the given offset
    pub async fn write_register(&self, offset: u64, value: u32, width: u8) -> Result<()> {
        // Same reason as the read, and worse: a one-byte OUT expanded to four
        // wrote the three registers after the target with the zero bytes of
        // the padding. On a serial port that meant every character printed
        // also cleared the interrupt-enable, FIFO-control and line-control
        // registers -- which looks like it works, right up until something
        // depends on one of them.
        let width = width.clamp(1, 4) as usize;
        let mut device = self.device.write().await;
        let data = value.to_le_bytes();
        device.write(offset, &data[..width]).await
    }
    /// The interrupt line this device is asserting, if any.
    pub async fn pending_interrupt(&self) -> Option<u8> {
        self.device.read().await.pending_interrupt()
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
    pub async fn read_register(&self, offset: u64, width: u8) -> Result<u32> {
        // `width` is the width the guest actually asked for, and handing the
        // device anything else is not a rounding error. Register files are
        // byte-wide and reads have side effects -- reading a UART receive
        // buffer pops a byte, reading its interrupt identification register
        // clears a pending interrupt -- so asking for four bytes when the
        // guest asked for one silently disturbs three registers it never
        // touched.
        let width = width.clamp(1, 4) as usize;
        let device = self.device.read().await;
        let mut data = [0u8; 4];
        device.read(offset, &mut data[..width]).await?;
        Ok(u32::from_le_bytes(data))
    }

    /// Write a register at the given offset
    pub async fn write_register(&self, offset: u64, value: u32, width: u8) -> Result<()> {
        // Same reason as the read, and worse: a one-byte OUT expanded to four
        // wrote the three registers after the target with the zero bytes of
        // the padding. On a serial port that meant every character printed
        // also cleared the interrupt-enable, FIFO-control and line-control
        // registers -- which looks like it works, right up until something
        // depends on one of them.
        let width = width.clamp(1, 4) as usize;
        let mut device = self.device.write().await;
        let data = value.to_le_bytes();
        device.write(offset, &data[..width]).await
    }

    /// The interrupt line this device is asserting, if any.
    pub async fn pending_interrupt(&self) -> Option<u8> {
        self.device.read().await.pending_interrupt()
    }

    /// Deliver host input to this device.
    ///
    /// # Errors
    ///
    /// Propagates [`Error::Device`] if the device accepts no input.
    pub async fn console_input(&self, data: &[u8]) -> Result<()> {
        self.device.write().await.console_input(data)
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

    /// Register a serial device and write `text` to its transmit buffer the
    /// way a guest would: one byte at a time through the THR.
    async fn register_console(manager: &DeviceManager, name: &str, text: &str) {
        let device = Arc::new(RwLock::new(crate::SerialDevice::new(
            name.to_string(),
            0x3F8,
        )));
        {
            let mut guard = device.write().await;
            for byte in text.as_bytes() {
                guard.write(0, &[*byte]).await.unwrap();
            }
        }
        manager.register_device(name, device).await.unwrap();
    }

    #[tokio::test]
    async fn console_output_omits_devices_that_are_not_consoles() {
        let manager = DeviceManager::new();
        manager
            .register_device(
                "dummy",
                Arc::new(RwLock::new(DummyDevice {
                    name: "dummy".to_string(),
                })),
            )
            .await
            .unwrap();

        assert!(
            manager.console_output().await.is_empty(),
            "a device with nothing to say about consoles must not appear as a silent one"
        );
    }

    #[tokio::test]
    async fn console_output_reports_each_console_in_name_order() {
        let manager = DeviceManager::new();
        register_console(&manager, "COM2", "second").await;
        register_console(&manager, "COM1", "first").await;

        let out = manager.console_output().await;

        // Sorted, not HashMap order: a caller concatenating these would
        // otherwise get a different boot log on every call.
        assert_eq!(
            out.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            vec!["COM1", "COM2"]
        );
        assert_eq!(out[0].1, b"first");
        assert_eq!(out[1].1, b"second");
    }

    #[tokio::test]
    async fn console_output_does_not_consume() {
        let manager = DeviceManager::new();
        register_console(&manager, "COM1", "boot").await;

        assert_eq!(manager.console_output().await[0].1, b"boot");
        assert_eq!(
            manager.console_output().await[0].1,
            b"boot",
            "polling the console must not eat the log it is reporting"
        );
    }

    #[tokio::test]
    async fn a_registered_console_that_said_nothing_still_appears() {
        let manager = DeviceManager::new();
        register_console(&manager, "COM1", "").await;

        let out = manager.console_output().await;
        assert_eq!(out.len(), 1, "an attached-but-quiet console is not absent");
        assert!(out[0].1.is_empty());
    }
    #[tokio::test]
    async fn a_one_byte_port_write_reaches_exactly_one_register() {
        // The dispatch layer used to hand every device a four-byte buffer
        // whatever the guest asked for, so a one-byte OUT of a character also
        // wrote the three registers after it with the padding. On a serial
        // port that cleared interrupt-enable, FIFO-control and line-control on
        // every character printed -- which looks like it works.
        use crate::devices::serial::SerialDevice;

        let manager = DeviceManager::new();
        let serial = Arc::new(RwLock::new(SerialDevice::new("COM1".to_string(), 0x3F8)));
        manager.register_device("COM1", serial).await.unwrap();
        manager
            .register_io_port_range("COM1".to_string(), 0x3F8, 0x3FF)
            .await
            .unwrap();

        let handle = manager.find_io_device(0x3F8).await.expect("COM1");

        // Set the interrupt-enable register, then print a character.
        handle.write_register(1, 0x0F, 1).await.unwrap();
        handle.write_register(0, u32::from(b'h'), 1).await.unwrap();

        assert_eq!(
            handle.read_register(1, 1).await.unwrap(),
            0x0F,
            "printing a character must not disturb the register beside it"
        );

        let console = manager.console_output().await;
        assert_eq!(
            console.first().map(|(_, bytes)| bytes.as_slice()),
            Some(b"h".as_slice()),
            "the character has to reach the transmit buffer: {console:?}"
        );
    }
}
