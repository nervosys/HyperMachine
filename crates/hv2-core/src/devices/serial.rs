//! Serial console device emulation

use crate::{Device, DeviceType, Error, Pic8259, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Serial port registers (16550 UART)
const THR_OFFSET: u64 = 0; // Transmitter Holding Register
const RBR_OFFSET: u64 = 0; // Receiver Buffer Register
const IER_OFFSET: u64 = 1; // Interrupt Enable Register
const IIR_OFFSET: u64 = 2; // Interrupt Identification Register
const LCR_OFFSET: u64 = 3; // Line Control Register
const MCR_OFFSET: u64 = 4; // Modem Control Register
const LSR_OFFSET: u64 = 5; // Line Status Register
const MSR_OFFSET: u64 = 6; // Modem Status Register
const SCR_OFFSET: u64 = 7; // Scratch Register

/// Line Status Register bits
const LSR_DATA_READY: u8 = 1 << 0;
const LSR_THR_EMPTY: u8 = 1 << 5;
const LSR_TRANSMITTER_EMPTY: u8 = 1 << 6;

/// Interrupt Enable Register bits
const IER_RDA: u8 = 1 << 0; // Received Data Available
const IER_THRE: u8 = 1 << 1; // Transmitter Holding Register Empty

/// Serial console device (16550 UART emulation)
pub struct SerialDevice {
    name: String,
    base_address: u64,
    /// IRQ number (3 for COM2, 4 for COM1)
    irq_number: u8,
    /// Receive buffer
    rx_buffer: Mutex<VecDeque<u8>>,
    /// Transmit buffer
    tx_buffer: Mutex<VecDeque<u8>>,
    /// Interrupt Enable Register
    ier: Mutex<u8>,
    /// Line Control Register
    lcr: Mutex<u8>,
    /// Modem Control Register
    mcr: Mutex<u8>,
    /// Scratch Register
    scr: Mutex<u8>,
    /// PIC for raising interrupts
    pic: Option<Arc<Pic8259>>,
}

impl SerialDevice {
    /// Create a new serial device at the given base address
    pub fn new(name: String, base_address: u64) -> Self {
        // Determine IRQ based on standard COM port addresses
        let irq_number = match base_address {
            0x3F8 => 4, // COM1
            0x2F8 => 3, // COM2
            0x3E8 => 4, // COM3 (shares IRQ with COM1)
            0x2E8 => 3, // COM4 (shares IRQ with COM2)
            _ => 4,     // Default to IRQ 4
        };

        Self {
            name,
            base_address,
            irq_number,
            rx_buffer: Mutex::new(VecDeque::new()),
            tx_buffer: Mutex::new(VecDeque::new()),
            ier: Mutex::new(0),
            lcr: Mutex::new(0),
            mcr: Mutex::new(0),
            scr: Mutex::new(0),
            pic: None,
        }
    }

    /// Set the PIC for interrupt generation
    pub fn set_pic(&mut self, pic: Arc<Pic8259>) {
        self.pic = Some(pic);
    }

    /// Get the base address
    pub fn base_address(&self) -> u64 {
        self.base_address
    }

    /// Enable interrupts for receive and/or transmit
    ///
    /// This is normally done by the guest OS writing to the IER register,
    /// but for testing purposes we provide a direct method.
    pub fn enable_interrupts(&self, receive: bool, transmit: bool) {
        let mut ier = self.ier.lock();
        if receive {
            *ier |= IER_RDA;
        }
        if transmit {
            *ier |= IER_THRE;
        }
    }

    /// Write data to the receive buffer (data coming from external source)
    pub fn input(&self, data: &[u8]) -> Result<()> {
        let mut rx = self.rx_buffer.lock();
        let was_empty = rx.is_empty();

        for &byte in data {
            rx.push_back(byte);
        }

        // Raise IRQ if interrupts enabled and data arrives in empty buffer
        if was_empty && !data.is_empty() {
            let ier = *self.ier.lock();
            if (ier & IER_RDA) != 0 {
                // Received Data Available Interrupt enabled
                if let Some(ref pic) = self.pic {
                    pic.raise_irq(self.irq_number)?;
                }
            }
        }

        Ok(())
    }

    /// Read data from the transmit buffer (data sent by guest)
    pub fn output(&self) -> Vec<u8> {
        let mut tx = self.tx_buffer.lock();
        let data: Vec<u8> = tx.drain(..).collect();

        // Raise THR Empty interrupt if enabled and buffer is now empty
        if !data.is_empty() {
            let ier = *self.ier.lock();
            if (ier & IER_THRE) != 0 {
                if let Some(ref pic) = self.pic {
                    let _ = pic.raise_irq(self.irq_number);
                }
            }
        }

        data
    }

    /// Get pending output as a string (if valid UTF-8)
    pub fn output_string(&self) -> String {
        let data = self.output();
        String::from_utf8_lossy(&data).into_owned()
    }
}

#[async_trait]
impl Device for SerialDevice {
    fn device_type(&self) -> DeviceType {
        DeviceType::Serial
    }

    fn name(&self) -> &str {
        &self.name
    }

    async fn init(&mut self) -> Result<()> {
        tracing::info!(
            "Initializing serial device '{}' at 0x{:X}",
            self.name,
            self.base_address
        );
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        if data.len() != 1 {
            return Err(Error::Device(
                "Serial device only supports single-byte reads".to_string(),
            ));
        }

        let value = match offset {
            RBR_OFFSET => {
                // Read from receive buffer
                let mut rx = self.rx_buffer.lock();
                rx.pop_front().unwrap_or(0)
            }
            IER_OFFSET => *self.ier.lock(),
            IIR_OFFSET => {
                // For now, no interrupts pending
                0x01 // No interrupt pending
            }
            LCR_OFFSET => *self.lcr.lock(),
            MCR_OFFSET => *self.mcr.lock(),
            LSR_OFFSET => {
                // Line status register
                let rx = self.rx_buffer.lock();
                let tx = self.tx_buffer.lock();

                let mut lsr = LSR_THR_EMPTY | LSR_TRANSMITTER_EMPTY;
                if !rx.is_empty() {
                    lsr |= LSR_DATA_READY;
                }
                lsr
            }
            MSR_OFFSET => {
                // Modem status - all lines active
                0xB0
            }
            SCR_OFFSET => *self.scr.lock(),
            _ => 0,
        };

        data[0] = value;
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }

        match offset {
            THR_OFFSET => {
                // Write to transmit buffer
                let mut tx = self.tx_buffer.lock();
                tx.push_back(data[0]);
            }
            IER_OFFSET => {
                *self.ier.lock() = data[0];
            }
            LCR_OFFSET => {
                *self.lcr.lock() = data[0];
            }
            MCR_OFFSET => {
                *self.mcr.lock() = data[0];
            }
            SCR_OFFSET => {
                *self.scr.lock() = data[0];
            }
            _ => {}
        }

        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        self.rx_buffer.lock().clear();
        self.tx_buffer.lock().clear();
        *self.ier.lock() = 0;
        *self.lcr.lock() = 0;
        *self.mcr.lock() = 0;
        *self.scr.lock() = 0;
        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        tracing::info!("Shutting down serial device '{}'", self.name);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_serial_device() {
        let mut device = SerialDevice::new("com1".to_string(), 0x3F8);

        device.init().await.unwrap();

        // Test input
        device.input(b"Hello");

        // Read data
        let mut buf = [0u8; 1];
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'H');

        // Test output
        device.write(THR_OFFSET, b"W").await.unwrap();
        device.write(THR_OFFSET, b"o").await.unwrap();
        device.write(THR_OFFSET, b"r").await.unwrap();
        device.write(THR_OFFSET, b"l").await.unwrap();
        device.write(THR_OFFSET, b"d").await.unwrap();

        let output = device.output_string();
        assert_eq!(output, "World");
    }

    #[tokio::test]
    async fn test_serial_irq_numbers() {
        // Test COM1 (0x3F8) -> IRQ 4
        let device1 = SerialDevice::new("COM1".to_string(), 0x3F8);
        assert_eq!(device1.irq_number, 4);

        // Test COM2 (0x2F8) -> IRQ 3
        let device2 = SerialDevice::new("COM2".to_string(), 0x2F8);
        assert_eq!(device2.irq_number, 3);

        // Test COM3 (0x3E8) -> IRQ 4 (shared with COM1)
        let device3 = SerialDevice::new("COM3".to_string(), 0x3E8);
        assert_eq!(device3.irq_number, 4);

        // Test COM4 (0x2E8) -> IRQ 3 (shared with COM2)
        let device4 = SerialDevice::new("COM4".to_string(), 0x2E8);
        assert_eq!(device4.irq_number, 3);
    }

    #[tokio::test]
    async fn test_serial_receive_interrupt() {
        let pic = Arc::new(Pic8259::new());
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);
        device.set_pic(Arc::clone(&pic));

        // Enable Received Data Available interrupt
        device.write(IER_OFFSET, &[IER_RDA]).await.unwrap();

        // Input should raise IRQ 4 (COM1)
        device.input(b"Test").unwrap();

        // Verify data is available
        let mut buf = [0u8; 1];
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'T');
    }

    #[tokio::test]
    async fn test_serial_receive_interrupt_disabled() {
        let pic = Arc::new(Pic8259::new());
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);
        device.set_pic(Arc::clone(&pic));

        // Don't enable interrupts (IER = 0)

        // Input should NOT raise IRQ
        device.input(b"Test").unwrap();

        // But data should still be available
        let mut buf = [0u8; 1];
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'T');
    }

    #[tokio::test]
    async fn test_serial_transmit_interrupt() {
        let pic = Arc::new(Pic8259::new());
        let mut device = SerialDevice::new("COM2".to_string(), 0x2F8);
        device.set_pic(Arc::clone(&pic));

        // Enable THR Empty interrupt
        device.write(IER_OFFSET, &[IER_THRE]).await.unwrap();

        // Write data to transmit buffer
        device.write(THR_OFFSET, b"A").await.unwrap();
        device.write(THR_OFFSET, b"B").await.unwrap();

        // Reading output should raise IRQ 3 (COM2)
        let output = device.output();
        assert_eq!(output, b"AB");
    }

    #[tokio::test]
    async fn test_serial_multiple_ports() {
        let pic = Arc::new(Pic8259::new());

        // Create COM1 and COM2
        let mut com1 = SerialDevice::new("COM1".to_string(), 0x3F8);
        let mut com2 = SerialDevice::new("COM2".to_string(), 0x2F8);

        com1.set_pic(Arc::clone(&pic));
        com2.set_pic(Arc::clone(&pic));

        // Enable interrupts on both
        com1.write(IER_OFFSET, &[IER_RDA]).await.unwrap();
        com2.write(IER_OFFSET, &[IER_RDA]).await.unwrap();

        // Input to COM1 (IRQ 4)
        com1.input(b"Hello").unwrap();

        // Input to COM2 (IRQ 3)
        com2.input(b"World").unwrap();

        // Verify data on both ports
        let mut buf = [0u8; 1];
        com1.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'H');

        com2.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'W');
    }

    #[tokio::test]
    async fn test_serial_buffer_management() {
        let pic = Arc::new(Pic8259::new());
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);
        device.set_pic(Arc::clone(&pic));

        device.write(IER_OFFSET, &[IER_RDA]).await.unwrap();

        // Fill receive buffer
        device.input(b"ABCDEFGH").unwrap();

        // Read all data
        let mut buf = [0u8; 1];
        for expected in b"ABCDEFGH" {
            device.read(RBR_OFFSET, &mut buf).await.unwrap();
            assert_eq!(buf[0], *expected);
        }

        // Buffer should be empty now
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0); // Should return 0 when buffer empty
    }
}
