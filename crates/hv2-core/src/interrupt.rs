//! Interrupt Controller
//!
//! This module implements the Intel 8259 Programmable Interrupt Controller (PIC).
//! The 8259 is used to manage hardware interrupts from devices like timers,
//! keyboards, and serial ports.

use crate::{Device, DeviceType, Error, Result};
use async_trait::async_trait;
use std::sync::Arc;

use parking_lot::Mutex;

/// PIC initialization command word 1 flags
const ICW1_ICW4: u8 = 0x01; // ICW4 needed
const ICW1_SINGLE: u8 = 0x02; // Single (cascade) mode
const ICW1_INTERVAL4: u8 = 0x04; // Call address interval 4 (8)
const ICW1_LEVEL: u8 = 0x08; // Level triggered (edge) mode
const ICW1_INIT: u8 = 0x10; // Initialization

/// PIC initialization command word 4 flags
const ICW4_8086: u8 = 0x01; // 8086/88 (MCS-80/85) mode
const ICW4_AUTO: u8 = 0x02; // Auto (normal) EOI
const ICW4_BUF_SLAVE: u8 = 0x08; // Buffered mode/slave
const ICW4_BUF_MASTER: u8 = 0x0C; // Buffered mode/master
const ICW4_SFNM: u8 = 0x10; // Special fully nested (not)

/// PIC operation command word 2 flags
const OCW2_EOI: u8 = 0x20; // End of interrupt
const OCW2_SPECIFIC: u8 = 0x40; // Specific EOI
const OCW2_ROTATE: u8 = 0x80; // Rotate priority

/// Single PIC chip (master or slave)
#[derive(Debug, Clone)]
struct PicChip {
    /// Interrupt Request Register - pending interrupts
    irr: u8,
    /// In-Service Register - interrupts being serviced
    isr: u8,
    /// Interrupt Mask Register - masked interrupts
    imr: u8,
    /// Base interrupt vector (e.g., 0x20 for master, 0x28 for slave)
    base_vector: u8,
    /// Auto EOI mode enabled
    auto_eoi: bool,
    /// Initialization sequence state
    init_state: InitState,
    /// Read register select (false=IRR, true=ISR)
    read_isr: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InitState {
    Ready,
    ExpectingIcw2,
    ExpectingIcw3,
    ExpectingIcw4,
}

impl PicChip {
    fn new(base_vector: u8) -> Self {
        Self {
            irr: 0,
            isr: 0,
            imr: 0xFF, // All interrupts masked by default
            base_vector,
            auto_eoi: false,
            init_state: InitState::Ready,
            read_isr: false,
        }
    }

    /// Raise an interrupt
    fn raise_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.irr |= 1 << irq;
        }
    }

    /// Lower an interrupt
    fn lower_irq(&mut self, irq: u8) {
        if irq < 8 {
            self.irr &= !(1 << irq);
        }
    }

    /// Get the highest priority pending interrupt
    fn get_pending(&self) -> Option<u8> {
        let pending = self.irr & !self.imr & !self.isr;
        if pending == 0 {
            None
        } else {
            Some(pending.trailing_zeros() as u8)
        }
    }

    /// Acknowledge an interrupt
    fn acknowledge(&mut self, irq: u8) {
        if irq < 8 {
            self.irr &= !(1 << irq);
            if !self.auto_eoi {
                self.isr |= 1 << irq;
            }
        }
    }

    /// End of interrupt
    fn eoi(&mut self, irq: Option<u8>) {
        if let Some(irq) = irq {
            // Specific EOI
            if irq < 8 {
                self.isr &= !(1 << irq);
            }
        } else {
            // Non-specific EOI - clear highest priority ISR bit
            if self.isr != 0 {
                let highest = self.isr.trailing_zeros();
                self.isr &= !(1 << highest);
            }
        }
    }

    /// Read status (IRR or ISR depending on read_isr flag)
    fn read_status(&self) -> u8 {
        if self.read_isr {
            self.isr
        } else {
            self.irr
        }
    }
}

/// Intel 8259 Programmable Interrupt Controller
///
/// This implements a cascaded master-slave PIC configuration,
/// which is the standard PC/AT configuration.
///
/// IRQ mapping:
/// - Master: IRQ 0-7 (vectors 0x20-0x27)
/// - Slave: IRQ 8-15 (vectors 0x28-0x2F)
/// - Cascade: Slave connected to master IRQ 2
#[derive(Debug, Clone)]
pub struct Pic8259 {
    master: Arc<Mutex<PicChip>>,
    slave: Arc<Mutex<PicChip>>,
}

impl Pic8259 {
    /// Create a new PIC with standard base vectors
    pub fn new() -> Self {
        Self {
            master: Arc::new(Mutex::new(PicChip::new(0x20))),
            slave: Arc::new(Mutex::new(PicChip::new(0x28))),
        }
    }

    /// Raise an interrupt request
    pub fn raise_irq(&self, irq: u8) -> Result<()> {
        if irq < 8 {
            // Master PIC
            self.master.lock().raise_irq(irq);
        } else if irq < 16 {
            // Slave PIC
            self.slave.lock().raise_irq(irq - 8);
            // Cascade to master IRQ 2
            self.master.lock().raise_irq(2);
        } else {
            return Err(Error::Device(format!("Invalid IRQ number: {}", irq)));
        }
        Ok(())
    }

    /// Lower an interrupt request
    pub fn lower_irq(&self, irq: u8) -> Result<()> {
        if irq < 8 {
            self.master.lock().lower_irq(irq);
        } else if irq < 16 {
            self.slave.lock().lower_irq(irq - 8);
            // Check if slave has any pending interrupts
            let slave = self.slave.lock();
            if slave.get_pending().is_none() {
                drop(slave);
                self.master.lock().lower_irq(2);
            }
        } else {
            return Err(Error::Device(format!("Invalid IRQ number: {}", irq)));
        }
        Ok(())
    }

    /// Set interrupt mask for the master PIC (0x00 = all unmasked, 0xFF = all masked)
    pub fn set_master_mask(&self, mask: u8) {
        let mut master = self.master.lock();
        master.imr = mask;
    }

    /// Set interrupt mask for the slave PIC (0x00 = all unmasked, 0xFF = all masked)
    pub fn set_slave_mask(&self, mask: u8) {
        let mut slave = self.slave.lock();
        slave.imr = mask;
    }

    /// Send End of Interrupt (EOI) for a given vector
    ///
    /// This clears the ISR bit so the same IRQ can fire again.
    /// Should be called after handling an interrupt.
    pub fn send_eoi(&self, vector: u8) -> Result<()> {
        let master = self.master.lock();

        if vector >= master.base_vector && vector < master.base_vector + 8 {
            // Master interrupt
            let irq = vector - master.base_vector;
            drop(master);
            self.master.lock().eoi(Some(irq));
        } else {
            // Slave interrupt
            let slave = self.slave.lock();
            if vector >= slave.base_vector && vector < slave.base_vector + 8 {
                let irq = vector - slave.base_vector;
                drop(slave);
                self.slave.lock().eoi(Some(irq));
                // Also send EOI to master for cascade
                self.master.lock().eoi(Some(2));
            } else {
                drop(master);
                return Err(Error::Device(format!(
                    "Invalid interrupt vector for EOI: {:#x}",
                    vector
                )));
            }
        }

        Ok(())
    }

    /// Get the highest priority pending interrupt and its vector
    pub fn get_pending_interrupt(&self) -> Option<u8> {
        let master = self.master.lock();

        if let Some(irq) = master.get_pending() {
            if irq == 2 {
                // Check slave cascade
                drop(master);
                let slave = self.slave.lock();
                if let Some(slave_irq) = slave.get_pending() {
                    return Some(slave.base_vector + slave_irq);
                }
            } else {
                return Some(master.base_vector + irq);
            }
        }

        None
    }

    /// Acknowledge an interrupt
    pub fn acknowledge_interrupt(&self, vector: u8) -> Result<()> {
        let master = self.master.lock();

        if vector >= master.base_vector && vector < master.base_vector + 8 {
            // Master interrupt
            let irq = vector - master.base_vector;
            drop(master);
            self.master.lock().acknowledge(irq);
        } else {
            // Slave interrupt
            let slave = self.slave.lock();
            if vector >= slave.base_vector && vector < slave.base_vector + 8 {
                let irq = vector - slave.base_vector;
                drop(slave);
                self.slave.lock().acknowledge(irq);
                // Also acknowledge cascade on master
                self.master.lock().acknowledge(2);
            } else {
                drop(master);
                return Err(Error::Device(format!(
                    "Invalid interrupt vector: {:#x}",
                    vector
                )));
            }
        }

        Ok(())
    }

    /// Write to master command port (0x20)
    fn write_master_command(&self, value: u8) {
        let mut master = self.master.lock();

        if value & ICW1_INIT != 0 {
            // ICW1 - Initialization
            master.init_state = InitState::ExpectingIcw2;
            master.imr = 0xFF;
            master.irr = 0;
            master.isr = 0;
            master.auto_eoi = false;
        } else if value & OCW2_EOI != 0 {
            // OCW2 - End of Interrupt
            if value & OCW2_SPECIFIC != 0 {
                // Specific EOI
                let irq = value & 0x07;
                master.eoi(Some(irq));
            } else {
                // Non-specific EOI
                master.eoi(None);
            }
        } else if value & 0x08 != 0 {
            // OCW3 - Read register command
            if value & 0x02 != 0 {
                master.read_isr = (value & 0x01) != 0;
            }
        }
    }

    /// Write to master data port (0x21)
    fn write_master_data(&self, value: u8) {
        let mut master = self.master.lock();

        match master.init_state {
            InitState::ExpectingIcw2 => {
                // ICW2 - Base interrupt vector
                master.base_vector = value & 0xF8;
                master.init_state = InitState::ExpectingIcw3;
            }
            InitState::ExpectingIcw3 => {
                // ICW3 - Cascade configuration (master: bitmask of slave IRQs)
                master.init_state = InitState::ExpectingIcw4;
            }
            InitState::ExpectingIcw4 => {
                // ICW4 - Mode configuration
                master.auto_eoi = (value & ICW4_AUTO) != 0;
                master.init_state = InitState::Ready;
            }
            InitState::Ready => {
                // OCW1 - Interrupt Mask Register
                master.imr = value;
            }
        }
    }

    /// Write to slave command port (0xA0)
    fn write_slave_command(&self, value: u8) {
        let mut slave = self.slave.lock();

        if value & ICW1_INIT != 0 {
            // ICW1 - Initialization
            slave.init_state = InitState::ExpectingIcw2;
            slave.imr = 0xFF;
            slave.irr = 0;
            slave.isr = 0;
            slave.auto_eoi = false;
        } else if value & OCW2_EOI != 0 {
            // OCW2 - End of Interrupt
            if value & OCW2_SPECIFIC != 0 {
                let irq = value & 0x07;
                slave.eoi(Some(irq));
            } else {
                slave.eoi(None);
            }
            // Also send EOI to master for cascade
            drop(slave);
            self.master.lock().eoi(Some(2));
        } else if value & 0x08 != 0 {
            // OCW3
            if value & 0x02 != 0 {
                slave.read_isr = (value & 0x01) != 0;
            }
        }
    }

    /// Write to slave data port (0xA1)
    fn write_slave_data(&self, value: u8) {
        let mut slave = self.slave.lock();

        match slave.init_state {
            InitState::ExpectingIcw2 => {
                slave.base_vector = value & 0xF8;
                slave.init_state = InitState::ExpectingIcw3;
            }
            InitState::ExpectingIcw3 => {
                // ICW3 - Cascade configuration (slave: cascade identity)
                slave.init_state = InitState::ExpectingIcw4;
            }
            InitState::ExpectingIcw4 => {
                slave.auto_eoi = (value & ICW4_AUTO) != 0;
                slave.init_state = InitState::Ready;
            }
            InitState::Ready => {
                // OCW1 - Interrupt Mask Register
                slave.imr = value;
            }
        }
    }

    /// Read from master command port (0x20)
    fn read_master_command(&self) -> u8 {
        self.master.lock().read_status()
    }

    /// Read from master data port (0x21)
    fn read_master_data(&self) -> u8 {
        self.master.lock().imr
    }

    /// Read from slave command port (0xA0)
    fn read_slave_command(&self) -> u8 {
        self.slave.lock().read_status()
    }

    /// Read from slave data port (0xA1)
    fn read_slave_data(&self) -> u8 {
        self.slave.lock().imr
    }

    /// Check if the PIC handles the given I/O port
    pub fn handles_port(&self, port: u16) -> bool {
        matches!(port, 0x20 | 0x21 | 0xA0 | 0xA1)
    }

    /// Read from a PIC I/O port (synchronous version)
    pub fn read_port_sync(&self, port: u16) -> u8 {
        match port {
            0x20 => self.read_master_command(),
            0x21 => self.read_master_data(),
            0xA0 => self.read_slave_command(),
            0xA1 => self.read_slave_data(),
            _ => 0xFF, // Invalid port returns 0xFF
        }
    }

    /// Write to a PIC I/O port (synchronous version)
    pub fn write_port_sync(&self, port: u16, value: u8) {
        match port {
            0x20 => self.write_master_command(value),
            0x21 => self.write_master_data(value),
            0xA0 => self.write_slave_command(value),
            0xA1 => self.write_slave_data(value),
            _ => {} // Invalid port is ignored
        }
    }

    /// Read from a PIC I/O port
    pub async fn read_port(&self, port: u16) -> Result<u8> {
        match port {
            0x20 => Ok(self.read_master_command()),
            0x21 => Ok(self.read_master_data()),
            0xA0 => Ok(self.read_slave_command()),
            0xA1 => Ok(self.read_slave_data()),
            _ => Err(Error::Device(format!("Invalid PIC port: {:#x}", port))),
        }
    }

    /// Write to a PIC I/O port
    pub async fn write_port(&self, port: u16, value: u8) -> Result<()> {
        match port {
            0x20 => {
                self.write_master_command(value);
                Ok(())
            }
            0x21 => {
                self.write_master_data(value);
                Ok(())
            }
            0xA0 => {
                self.write_slave_command(value);
                Ok(())
            }
            0xA1 => {
                self.write_slave_data(value);
                Ok(())
            }
            _ => Err(Error::Device(format!("Invalid PIC port: {:#x}", port))),
        }
    }

    /// Create an I/O handler for this PIC
    ///
    /// Returns a closure that can be registered with `WhpxVm::register_io_handler()`.
    /// The handler will route I/O port accesses (0x20-0x21, 0xA0-0xA1) to the PIC.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use hv2_core::interrupt::Pic8259;
    /// use hv2_core::backends::whpx::WhpxVm;
    ///
    /// # fn example(vm: &WhpxVm) -> hv2_core::Result<()> {
    /// let pic = Pic8259::new();
    ///
    /// // Register handlers for all PIC ports
    /// for port in [0x20, 0x21, 0xA0, 0xA1] {
    ///     let handler = pic.create_io_handler();
    ///     vm.register_io_handler(port, handler);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn create_io_handler(
        &self,
    ) -> Box<dyn Fn(u16, bool, u8, &mut u32) -> Result<()> + Send + Sync> {
        let pic = Arc::new(self.clone());

        Box::new(move |port, is_write, _size, data| {
            let pic = pic.clone();

            if is_write {
                // Write to PIC port
                let value = (*data & 0xFF) as u8;
                match port {
                    0x20 => pic.write_master_command(value),
                    0x21 => pic.write_master_data(value),
                    0xA0 => pic.write_slave_command(value),
                    0xA1 => pic.write_slave_data(value),
                    _ => return Err(Error::Device(format!("Invalid PIC port: {:#x}", port))),
                }
                Ok(())
            } else {
                // Read from PIC port
                let value = match port {
                    0x20 => pic.read_master_command(),
                    0x21 => pic.read_master_data(),
                    0xA0 => pic.read_slave_command(),
                    0xA1 => pic.read_slave_data(),
                    _ => return Err(Error::Device(format!("Invalid PIC port: {:#x}", port))),
                };
                *data = value as u32;
                Ok(())
            }
        })
    }
}

impl Default for Pic8259 {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Device for Pic8259 {
    fn name(&self) -> &str {
        "Intel 8259 PIC"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::InterruptController
    }

    async fn init(&mut self) -> Result<()> {
        // PIC is initialized via I/O port writes, nothing to do here
        Ok(())
    }

    async fn read(&self, offset: u64, data: &mut [u8]) -> Result<()> {
        let value = match offset {
            0x20 => self.read_master_command(),
            0x21 => self.read_master_data(),
            0xA0 => self.read_slave_command(),
            0xA1 => self.read_slave_data(),
            _ => {
                return Err(Error::Device(format!(
                    "Invalid PIC register offset: {:#x}",
                    offset
                )))
            }
        };
        data[0] = value;
        Ok(())
    }

    async fn write(&mut self, offset: u64, data: &[u8]) -> Result<()> {
        let value = data[0];
        match offset {
            0x20 => self.write_master_command(value),
            0x21 => self.write_master_data(value),
            0xA0 => self.write_slave_command(value),
            0xA1 => self.write_slave_data(value),
            _ => {
                return Err(Error::Device(format!(
                    "Invalid PIC register offset: {:#x}",
                    offset
                )))
            }
        }
        Ok(())
    }

    async fn reset(&mut self) -> Result<()> {
        let mut master = self.master.lock();
        let mut slave = self.slave.lock();

        *master = PicChip::new(0x20);
        *slave = PicChip::new(0x28);

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_pic_creation() {
        let pic = Pic8259::new();
        assert_eq!(pic.name(), "Intel 8259 PIC");
    }

    #[tokio::test]
    async fn test_raise_and_get_pending() {
        let pic = Pic8259::new();

        // Initially no pending interrupts (all masked)
        assert_eq!(pic.get_pending_interrupt(), None);

        // Initialize PIC
        pic.write_master_command(ICW1_INIT | ICW1_ICW4);
        pic.write_master_data(0x20); // ICW2: base vector
        pic.write_master_data(0x04); // ICW3: slave on IRQ2
        pic.write_master_data(ICW4_8086); // ICW4
        pic.write_master_data(0x00); // Unmask all interrupts

        // Raise IRQ 0 (timer)
        pic.raise_irq(0).unwrap();
        assert_eq!(pic.get_pending_interrupt(), Some(0x20));
    }

    #[tokio::test]
    async fn test_interrupt_masking() {
        let pic = Pic8259::new();

        // Initialize PIC
        pic.write_master_command(ICW1_INIT | ICW1_ICW4);
        pic.write_master_data(0x20);
        pic.write_master_data(0x04);
        pic.write_master_data(ICW4_8086);
        pic.write_master_data(0xFE); // Mask all except IRQ 0

        // Raise IRQ 0 and IRQ 1
        pic.raise_irq(0).unwrap();
        pic.raise_irq(1).unwrap();

        // Only IRQ 0 should be pending (IRQ 1 is masked)
        assert_eq!(pic.get_pending_interrupt(), Some(0x20));
    }

    #[tokio::test]
    async fn test_acknowledge_and_eoi() {
        let pic = Pic8259::new();

        // Initialize
        pic.write_master_command(ICW1_INIT | ICW1_ICW4);
        pic.write_master_data(0x20);
        pic.write_master_data(0x04);
        pic.write_master_data(ICW4_8086);
        pic.write_master_data(0x00);

        // Raise and acknowledge IRQ 0
        pic.raise_irq(0).unwrap();
        pic.acknowledge_interrupt(0x20).unwrap();

        // Should not be pending anymore
        assert_eq!(pic.get_pending_interrupt(), None);

        // Send EOI
        pic.write_master_command(OCW2_EOI);
    }

    #[tokio::test]
    async fn test_slave_cascade() {
        let pic = Pic8259::new();

        // Initialize master
        pic.write_master_command(ICW1_INIT | ICW1_ICW4);
        pic.write_master_data(0x20);
        pic.write_master_data(0x04);
        pic.write_master_data(ICW4_8086);
        pic.write_master_data(0x00);

        // Initialize slave
        pic.write_slave_command(ICW1_INIT | ICW1_ICW4);
        pic.write_slave_data(0x28);
        pic.write_slave_data(0x02); // Slave ID
        pic.write_slave_data(ICW4_8086);
        pic.write_slave_data(0x00);

        // Raise IRQ 8 (first slave interrupt)
        pic.raise_irq(8).unwrap();
        assert_eq!(pic.get_pending_interrupt(), Some(0x28));
    }

    #[tokio::test]
    async fn test_device_trait_implementation() {
        let mut pic = Pic8259::new();

        // Test Device trait methods
        assert_eq!(pic.name(), "Intel 8259 PIC");
        assert_eq!(pic.device_type(), DeviceType::InterruptController);

        // Test init
        pic.init().await.unwrap();

        // Test reset
        pic.reset().await.unwrap();

        // Test shutdown
        pic.shutdown().await.unwrap();
    }
}
