//! Serial console device emulation

use crate::{Device, DeviceType, Pic8259, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Serial port registers (16550 UART)
const THR_OFFSET: u64 = 0; // Transmitter Holding Register
const RBR_OFFSET: u64 = 0; // Receiver Buffer Register
const IER_OFFSET: u64 = 1; // Interrupt Enable Register
const IIR_OFFSET: u64 = 2; // Interrupt Identification Register (read)
const FCR_OFFSET: u64 = 2; // FIFO Control Register (write)

/// Ceiling on buffered guest output, in bytes.
///
/// The transmit buffer is drained only when a caller reads it, so a chatty
/// guest on an unattended VM would otherwise grow it without limit. 1 MiB is
/// far more than a boot log and small enough to be irrelevant per VM.
const MAX_TX_BUFFER_BYTES: usize = 1024 * 1024;
const LCR_OFFSET: u64 = 3; // Line Control Register
const MCR_OFFSET: u64 = 4; // Modem Control Register
const LSR_OFFSET: u64 = 5; // Line Status Register
const MSR_OFFSET: u64 = 6; // Modem Status Register
const SCR_OFFSET: u64 = 7; // Scratch Register

/// Divisor Latch registers (accessible when DLAB=1)
const DLL_OFFSET: u64 = 0; // Divisor Latch Low byte
const DLM_OFFSET: u64 = 1; // Divisor Latch High byte

/// Line Status Register bits
const LSR_DATA_READY: u8 = 1 << 0;
const LSR_THR_EMPTY: u8 = 1 << 5;
const LSR_TRANSMITTER_EMPTY: u8 = 1 << 6;

/// Interrupt Enable Register bits
const IER_RDA: u8 = 1 << 0; // Received Data Available
const IER_THRE: u8 = 1 << 1; // Transmitter Holding Register Empty

/// Line Control Register bits
const LCR_DLAB: u8 = 1 << 7; // Divisor Latch Access Bit

/// IIR (Interrupt Identification Register) constants
const IIR_NO_INTERRUPT: u8 = 0x01;
const IIR_RDA: u8 = 0x04; // Received Data Available (priority 2)
const IIR_THRE: u8 = 0x02; // THR Empty (priority 3)
const IIR_FIFO_ENABLED: u8 = 0xC0; // Bits 6-7 set when FIFOs are enabled

/// FCR (FIFO Control Register) bits
const FCR_FIFO_ENABLE: u8 = 1 << 0;
const FCR_RX_FIFO_RESET: u8 = 1 << 1;
const FCR_TX_FIFO_RESET: u8 = 1 << 2;

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
    /// FIFO Control Register (write-only, shadows for IIR FIFO bits)
    fcr: Mutex<u8>,
    /// Divisor Latch Low byte
    dll: Mutex<u8>,
    /// Divisor Latch High byte
    dlm: Mutex<u8>,
    /// THR empty flag (cleared on write to THR, set when output drained)
    thr_empty: Mutex<bool>,
    /// PIC for raising interrupts
    pic: Option<Arc<Pic8259>>,
}

impl SerialDevice {
    /// Create a new serial device at the given base address
    #[must_use]
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
            fcr: Mutex::new(0),
            dll: Mutex::new(0x0C), // Default divisor = 12 → 9600 baud
            dlm: Mutex::new(0x00),
            thr_empty: Mutex::new(true),
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

        if !data.is_empty() {
            *self.thr_empty.lock() = true;

            // Raise THR Empty interrupt if enabled and buffer is now empty
            let ier = *self.ier.lock();
            if (ier & IER_THRE) != 0 {
                if let Some(ref pic) = self.pic {
                    let _ = pic.raise_irq(self.irq_number);
                }
            }
        }

        data
    }

    /// Get pending output as a string (if valid UTF-8).
    ///
    /// Consumes the buffer, like [`Self::output`].
    pub fn output_string(&self) -> String {
        let data = self.output();
        String::from_utf8_lossy(&data).into_owned()
    }

    /// Copy buffered output without consuming it.
    ///
    /// [`Self::output`] drains, which is right for a consumer that forwards
    /// bytes onward but wrong for anything that polls: two readers cannot both
    /// see the console, and a status check would silently eat the boot log it
    /// was trying to report.
    pub fn peek_output(&self) -> Vec<u8> {
        self.tx_buffer.lock().iter().copied().collect()
    }

    /// Buffered output as a string, without consuming it.
    pub fn peek_output_string(&self) -> String {
        String::from_utf8_lossy(&self.peek_output()).into_owned()
    }
    /// Read one byte-wide register.
    ///
    /// Separate from [`Device::read`] so that a wide access can walk
    /// consecutive registers the way the hardware does. `dlab` is re-read per
    /// register rather than latched for the whole access, because a write
    /// inside the same access can change it.
    fn read_register(&self, offset: u64) -> u8 {
        let dlab = (*self.lcr.lock() & LCR_DLAB) != 0;

        match offset {
            RBR_OFFSET if dlab => {
                // DLAB=1: Divisor Latch Low byte
                *self.dll.lock()
            }
            RBR_OFFSET => {
                // DLAB=0: Read from receive buffer
                let mut rx = self.rx_buffer.lock();
                rx.pop_front().unwrap_or(0)
            }
            IER_OFFSET if dlab => {
                // DLAB=1: Divisor Latch High byte
                *self.dlm.lock()
            }
            IER_OFFSET => *self.ier.lock(),
            IIR_OFFSET => {
                // Dynamic IIR: reflect actual pending interrupt sources
                let ier = *self.ier.lock();
                let fcr = *self.fcr.lock();
                let rx_has_data = !self.rx_buffer.lock().is_empty();
                let thr_empty = *self.thr_empty.lock();

                let mut iir = IIR_NO_INTERRUPT;

                // RDA has higher priority than THRE
                if (ier & IER_RDA) != 0 && rx_has_data {
                    iir = IIR_RDA;
                } else if (ier & IER_THRE) != 0 && thr_empty {
                    iir = IIR_THRE;
                }

                // Reflect FIFO state in bits 6-7
                if (fcr & FCR_FIFO_ENABLE) != 0 {
                    iir |= IIR_FIFO_ENABLED;
                }

                iir
            }
            LCR_OFFSET => *self.lcr.lock(),
            MCR_OFFSET => *self.mcr.lock(),
            LSR_OFFSET => {
                // Line status register
                let rx = self.rx_buffer.lock();

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
        }
    }

    /// Write one byte-wide register.
    fn write_register(&self, offset: u64, value: u8) {
        let dlab = (*self.lcr.lock() & LCR_DLAB) != 0;

        match offset {
            THR_OFFSET if dlab => {
                // DLAB=1: Divisor Latch Low byte
                *self.dll.lock() = value;
            }
            THR_OFFSET => {
                // DLAB=0: Write to transmit buffer
                let mut tx = self.tx_buffer.lock();
                tx.push_back(value);
                // A guest printing in a loop must not be able to exhaust host
                // memory. Nothing drains this buffer unless a caller asks for
                // output, so without a cap an unattended VM grows it forever.
                // Drop the oldest bytes: for a console, recent output is what
                // anyone wants, and losing the start of a boot log beats
                // losing the host.
                while tx.len() > MAX_TX_BUFFER_BYTES {
                    tx.pop_front();
                }
                *self.thr_empty.lock() = false;
            }
            IER_OFFSET if dlab => {
                // DLAB=1: Divisor Latch High byte
                *self.dlm.lock() = value;
            }
            IER_OFFSET => {
                *self.ier.lock() = value;
            }
            FCR_OFFSET => {
                // FIFO Control Register (write-only)
                let byte = value;
                *self.fcr.lock() = byte & FCR_FIFO_ENABLE; // Only persist the enable bit

                if (byte & FCR_RX_FIFO_RESET) != 0 {
                    self.rx_buffer.lock().clear();
                }
                if (byte & FCR_TX_FIFO_RESET) != 0 {
                    self.tx_buffer.lock().clear();
                    *self.thr_empty.lock() = true;
                }
            }
            LCR_OFFSET => {
                *self.lcr.lock() = value;
            }
            MCR_OFFSET => {
                *self.mcr.lock() = value;
            }
            SCR_OFFSET => {
                *self.scr.lock() = value;
            }
            _ => {}
        }
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

    fn console_output(&self) -> Option<Vec<u8>> {
        Some(self.peek_output())
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
        // A wide access reads consecutive registers, which is what the
        // hardware does: a 16550 register file is byte-wide, and an `inw` at
        // 0x3f8 returns RBR in the low byte and IER in the high one.
        //
        // This used to refuse anything but a single byte, and the refusal
        // reached `handle_exit` as a device error that stopped the VM. A Linux
        // kernel probing the port with a word read therefore killed the guest
        // outright -- the exact opposite of what real hardware does, which is
        // answer.
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = self.read_register(offset + index as u64);
        }
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        // Likewise, and for the same reason: a two-byte write to THR sends two
        // characters. Writing only `data[0]` dropped the rest silently, which
        // is how a console loses output nobody can account for.
        for (index, byte) in data.iter().enumerate() {
            self.write_register(offset + index as u64, *byte);
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
        *self.fcr.lock() = 0;
        *self.dll.lock() = 0x0C;
        *self.dlm.lock() = 0x00;
        *self.thr_empty.lock() = true;
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
    async fn guest_output_cannot_exhaust_host_memory() {
        // Nothing drains the transmit buffer unless a caller reads it, so a
        // guest printing in a loop on an unattended VM would grow it forever.
        let mut device = SerialDevice::new("com1".to_string(), 0x3F8);
        device.init().await.unwrap();

        for _ in 0..(MAX_TX_BUFFER_BYTES + 4096) {
            device.write(THR_OFFSET, b"x").await.unwrap();
        }

        assert!(
            device.peek_output().len() <= MAX_TX_BUFFER_BYTES,
            "buffered output must stay under the cap"
        );
    }

    #[tokio::test]
    async fn peeking_does_not_consume_the_console() {
        // output() drains, which is right for a consumer forwarding bytes on
        // and wrong for anything that polls: a status check must not eat the
        // boot log it is reporting.
        let mut device = SerialDevice::new("com1".to_string(), 0x3F8);
        device.init().await.unwrap();

        for byte in b"boot" {
            device.write(THR_OFFSET, &[*byte]).await.unwrap();
        }

        assert_eq!(device.peek_output_string(), "boot");
        assert_eq!(device.peek_output_string(), "boot", "peek is repeatable");

        assert_eq!(device.output_string(), "boot", "output still drains");
        assert_eq!(device.peek_output_string(), "", "and the drain was real");
    }

    #[tokio::test]
    async fn test_serial_device() {
        let mut device = SerialDevice::new("com1".to_string(), 0x3F8);

        device.init().await.unwrap();

        // Test input
        device.input(b"Hello").unwrap();

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

    #[tokio::test]
    async fn test_serial_dlab_divisor_latch() {
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);

        // Set DLAB=1 in LCR
        device.write(LCR_OFFSET, &[LCR_DLAB]).await.unwrap();

        // Write divisor latch: 1 → 115200 baud
        device.write(DLL_OFFSET, &[0x01]).await.unwrap();
        device.write(DLM_OFFSET, &[0x00]).await.unwrap();

        // Read divisor back
        let mut buf = [0u8; 1];
        device.read(DLL_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0x01);
        device.read(DLM_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0x00);

        // Clear DLAB
        device.write(LCR_OFFSET, &[0x00]).await.unwrap();

        // Now offset 0 should be THR/RBR again
        device.input(b"X").unwrap();
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], b'X');
    }

    #[tokio::test]
    async fn test_serial_fcr_fifo_control() {
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);

        // Put data in buffers
        device.input(b"ABC").unwrap();
        device.write(THR_OFFSET, b"D").await.unwrap();

        // Enable FIFO and clear both FIFOs
        device
            .write(
                FCR_OFFSET,
                &[FCR_FIFO_ENABLE | FCR_RX_FIFO_RESET | FCR_TX_FIFO_RESET],
            )
            .await
            .unwrap();

        // RX buffer should be cleared
        let mut buf = [0u8; 1];
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0], 0);

        // TX buffer should be cleared
        let output = device.output();
        assert!(output.is_empty());

        // IIR should show FIFOs enabled (bits 6-7)
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_ne!(
            buf[0] & IIR_FIFO_ENABLED,
            0,
            "FIFO bits should be set in IIR"
        );
    }

    #[tokio::test]
    async fn test_serial_dynamic_iir() {
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);

        // No interrupts enabled: IIR should show no interrupt pending
        let mut buf = [0u8; 1];
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_NO_INTERRUPT);

        // Enable RDA interrupt
        device.write(IER_OFFSET, &[IER_RDA]).await.unwrap();

        // No data → still no interrupt
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_NO_INTERRUPT);

        // Input data → RDA interrupt pending
        device.input(b"Z").unwrap();
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_RDA);

        // Drain RX → back to no interrupt
        device.read(RBR_OFFSET, &mut buf).await.unwrap();
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_NO_INTERRUPT);
    }

    #[tokio::test]
    async fn test_serial_iir_thre() {
        let mut device = SerialDevice::new("COM1".to_string(), 0x3F8);

        // Enable THRE interrupt only
        device.write(IER_OFFSET, &[IER_THRE]).await.unwrap();

        // THR starts empty → THRE interrupt pending
        let mut buf = [0u8; 1];
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_THRE);

        // Write to THR → THRE clears
        device.write(THR_OFFSET, b"A").await.unwrap();
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_NO_INTERRUPT);

        // Drain TX → THRE again
        let _ = device.output();
        device.read(IIR_OFFSET, &mut buf).await.unwrap();
        assert_eq!(buf[0] & 0x0F, IIR_THRE);
    }
    #[tokio::test]
    async fn a_wide_read_walks_consecutive_registers() {
        // A 16550 register file is byte-wide, so an `inw` at the base returns
        // RBR in the low byte and IER in the high one. Refusing the access
        // instead reached handle_exit as a device error and stopped the VM,
        // which is how a Linux kernel probing the port killed its own guest.
        let mut serial = SerialDevice::new("COM1".to_string(), 0x3F8);
        serial.write(IER_OFFSET, &[0x5A]).await.unwrap();

        let mut data = [0u8; 2];
        serial.read(RBR_OFFSET, &mut data).await.unwrap();
        assert_eq!(data[1], 0x5A, "the second byte should be IER");
    }

    #[tokio::test]
    async fn a_four_byte_read_is_answered_rather_than_refused() {
        let serial = SerialDevice::new("COM1".to_string(), 0x3F8);
        let mut data = [0u8; 4];
        serial
            .read(RBR_OFFSET, &mut data)
            .await
            .expect("real hardware answers a dword read; refusing it stops the guest");
    }

    #[tokio::test]
    async fn a_wide_write_reaches_consecutive_registers() {
        // Not two characters: an `outw` at the base writes THR and then IER,
        // because the register file is byte-wide. Writing only data[0] dropped
        // the second byte silently, which is a register that quietly never got
        // set -- and this is what a guest configuring the port in one access
        // actually expects to happen.
        let mut serial = SerialDevice::new("COM1".to_string(), 0x3F8);
        serial.write(THR_OFFSET, &[b'h', 0x0F]).await.unwrap();

        assert_eq!(serial.peek_output(), b"h", "THR took the first byte");

        let mut ier = [0u8; 1];
        serial.read(IER_OFFSET, &mut ier).await.unwrap();
        assert_eq!(ier[0], 0x0F, "IER took the second");
    }
}
