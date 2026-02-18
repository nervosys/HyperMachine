//! Serial port (UART) driver for Type-1 hypervisor
//!
//! This module provides:
//! - Serial port initialization
//! - Character output for debugging
//! - Serial console input

use core::fmt::{self, Write};

/// Standard COM port I/O addresses
pub mod port {
    pub const COM1: u16 = 0x3F8;
    pub const COM2: u16 = 0x2F8;
    pub const COM3: u16 = 0x3E8;
    pub const COM4: u16 = 0x2E8;
}

/// UART register offsets
mod reg {
    pub const DATA: u16 = 0; // Data register (R/W)
    pub const IER: u16 = 1; // Interrupt Enable Register
    pub const IIR: u16 = 2; // Interrupt ID Register (Read)
    pub const FCR: u16 = 2; // FIFO Control Register (Write)
    pub const LCR: u16 = 3; // Line Control Register
    pub const MCR: u16 = 4; // Modem Control Register
    pub const LSR: u16 = 5; // Line Status Register
    pub const MSR: u16 = 6; // Modem Status Register
    pub const DLL: u16 = 0; // Divisor Latch Low (when DLAB=1)
    pub const DLH: u16 = 1; // Divisor Latch High (when DLAB=1)
}

/// Line Status Register bits
mod lsr {
    pub const DATA_READY: u8 = 1 << 0;
    pub const OVERRUN_ERROR: u8 = 1 << 1;
    pub const PARITY_ERROR: u8 = 1 << 2;
    pub const FRAMING_ERROR: u8 = 1 << 3;
    pub const BREAK_INDICATOR: u8 = 1 << 4;
    pub const TX_EMPTY: u8 = 1 << 5;
    pub const TX_IDLE: u8 = 1 << 6;
    pub const FIFO_ERROR: u8 = 1 << 7;
}

/// Line Control Register bits
mod lcr {
    pub const DATA_5: u8 = 0b00;
    pub const DATA_6: u8 = 0b01;
    pub const DATA_7: u8 = 0b10;
    pub const DATA_8: u8 = 0b11;
    pub const STOP_1: u8 = 0 << 2;
    pub const STOP_2: u8 = 1 << 2;
    pub const PARITY_NONE: u8 = 0b000 << 3;
    pub const PARITY_ODD: u8 = 0b001 << 3;
    pub const PARITY_EVEN: u8 = 0b011 << 3;
    pub const DLAB: u8 = 1 << 7;
}

/// FIFO Control Register bits
mod fcr {
    pub const ENABLE: u8 = 1 << 0;
    pub const CLEAR_RX: u8 = 1 << 1;
    pub const CLEAR_TX: u8 = 1 << 2;
    pub const TRIGGER_1: u8 = 0b00 << 6;
    pub const TRIGGER_4: u8 = 0b01 << 6;
    pub const TRIGGER_8: u8 = 0b10 << 6;
    pub const TRIGGER_14: u8 = 0b11 << 6;
}

/// Serial port driver
pub struct SerialPort {
    /// Base I/O port address
    base: u16,
    /// Whether the port has been initialized
    initialized: bool,
}

impl SerialPort {
    /// Create a new serial port driver
    pub const fn new(base: u16) -> Self {
        Self {
            base,
            initialized: false,
        }
    }

    /// Initialize the serial port
    ///
    /// # Safety
    /// This function performs I/O port operations.
    pub unsafe fn init(&mut self) {
        // Disable interrupts
        self.write_reg(reg::IER, 0x00);

        // Enable DLAB to set baud rate
        self.write_reg(reg::LCR, lcr::DLAB);

        // Set divisor to 1 (115200 baud)
        self.write_reg(reg::DLL, 0x01);
        self.write_reg(reg::DLH, 0x00);

        // 8 data bits, no parity, 1 stop bit
        self.write_reg(reg::LCR, lcr::DATA_8 | lcr::STOP_1 | lcr::PARITY_NONE);

        // Enable FIFO, clear buffers, 14-byte threshold
        self.write_reg(
            reg::FCR,
            fcr::ENABLE | fcr::CLEAR_RX | fcr::CLEAR_TX | fcr::TRIGGER_14,
        );

        // Enable DTR, RTS, and OUT2
        self.write_reg(reg::MCR, 0x0B);

        self.initialized = true;
    }

    /// Initialize with a specific baud rate
    pub unsafe fn init_with_baud(&mut self, baud: u32) {
        let divisor = (115200 / baud) as u16;

        // Disable interrupts
        self.write_reg(reg::IER, 0x00);

        // Enable DLAB to set baud rate
        self.write_reg(reg::LCR, lcr::DLAB);

        // Set divisor
        self.write_reg(reg::DLL, (divisor & 0xFF) as u8);
        self.write_reg(reg::DLH, ((divisor >> 8) & 0xFF) as u8);

        // 8 data bits, no parity, 1 stop bit
        self.write_reg(reg::LCR, lcr::DATA_8 | lcr::STOP_1 | lcr::PARITY_NONE);

        // Enable FIFO, clear buffers, 14-byte threshold
        self.write_reg(
            reg::FCR,
            fcr::ENABLE | fcr::CLEAR_RX | fcr::CLEAR_TX | fcr::TRIGGER_14,
        );

        // Enable DTR, RTS, and OUT2
        self.write_reg(reg::MCR, 0x0B);

        self.initialized = true;
    }

    /// Check if data is available to read
    pub fn data_available(&self) -> bool {
        unsafe { self.read_reg(reg::LSR) & lsr::DATA_READY != 0 }
    }

    /// Check if the transmit buffer is empty
    pub fn tx_empty(&self) -> bool {
        unsafe { self.read_reg(reg::LSR) & lsr::TX_EMPTY != 0 }
    }

    /// Read a byte (blocking)
    pub fn read(&self) -> u8 {
        while !self.data_available() {
            core::hint::spin_loop();
        }
        unsafe { self.read_reg(reg::DATA) }
    }

    /// Try to read a byte (non-blocking)
    pub fn try_read(&self) -> Option<u8> {
        if self.data_available() {
            Some(unsafe { self.read_reg(reg::DATA) })
        } else {
            None
        }
    }

    /// Write a byte (blocking)
    pub fn write(&self, byte: u8) {
        while !self.tx_empty() {
            core::hint::spin_loop();
        }
        unsafe { self.write_reg(reg::DATA, byte) }
    }

    /// Write a string
    pub fn write_str(&self, s: &str) {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write(b'\r');
            }
            self.write(byte);
        }
    }

    /// Read a register
    unsafe fn read_reg(&self, offset: u16) -> u8 {
        let port = self.base + offset;
        let value: u8;
        core::arch::asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack)
        );
        value
    }

    /// Write a register
    unsafe fn write_reg(&self, offset: u16, value: u8) {
        let port = self.base + offset;
        core::arch::asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack)
        );
    }
}

impl Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        SerialPort::write_str(self, s);
        Ok(())
    }
}

/// Global serial port for debugging (COM1)
///
/// # Safety
/// Must be initialized before use.
pub static mut SERIAL1: SerialPort = SerialPort::new(port::COM1);

/// Initialize the global serial port
///
/// # Safety
/// Must only be called once during initialization.
#[allow(static_mut_refs)]
pub unsafe fn init_global_serial() {
    SERIAL1.init();
}

/// Print to the serial port
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*))
    };
}

/// Print to the serial port with a newline
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)))
}

/// Internal print function
#[doc(hidden)]
#[allow(static_mut_refs)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    unsafe {
        SERIAL1.write_fmt(args).ok();
    }
}

/// Virtual serial port for guest
pub struct VirtualSerial {
    /// Base port
    base: u16,
    /// Transmit buffer
    tx_buffer: [u8; 256],
    /// Transmit buffer head
    tx_head: usize,
    /// Transmit buffer tail
    tx_tail: usize,
    /// Receive buffer
    rx_buffer: [u8; 256],
    /// Receive buffer head
    rx_head: usize,
    /// Receive buffer tail
    rx_tail: usize,
    /// Line control register
    lcr: u8,
    /// Modem control register
    mcr: u8,
    /// Interrupt enable register
    ier: u8,
    /// Divisor latch
    divisor: u16,
}

impl VirtualSerial {
    /// Create a new virtual serial port
    pub fn new(base: u16) -> Self {
        Self {
            base,
            tx_buffer: [0; 256],
            tx_head: 0,
            tx_tail: 0,
            rx_buffer: [0; 256],
            rx_head: 0,
            rx_tail: 0,
            lcr: 0,
            mcr: 0,
            ier: 0,
            divisor: 1,
        }
    }

    /// Handle port read
    pub fn read_port(&self, port: u16) -> u8 {
        let offset = port - self.base;

        match offset {
            0 if self.lcr & lcr::DLAB != 0 => (self.divisor & 0xFF) as u8,
            0 => self.rx_buffer[self.rx_tail],
            1 if self.lcr & lcr::DLAB != 0 => ((self.divisor >> 8) & 0xFF) as u8,
            1 => self.ier,
            2 => 0x01, // IIR: no pending interrupt
            3 => self.lcr,
            4 => self.mcr,
            5 => {
                // LSR: TX empty, TX idle, RX ready based on buffer state
                let mut lsr = lsr::TX_EMPTY | lsr::TX_IDLE;
                if self.rx_head != self.rx_tail {
                    lsr |= lsr::DATA_READY;
                }
                lsr
            }
            6 => 0xB0, // MSR: CTS, DSR, CD
            _ => 0,
        }
    }

    /// Handle port write
    pub fn write_port(&mut self, port: u16, value: u8) {
        let offset = port - self.base;

        match offset {
            0 if self.lcr & lcr::DLAB != 0 => {
                self.divisor = (self.divisor & 0xFF00) | value as u16;
            }
            0 => {
                // Data write - add to TX buffer
                self.tx_buffer[self.tx_head] = value;
                self.tx_head = (self.tx_head + 1) % 256;
            }
            1 if self.lcr & lcr::DLAB != 0 => {
                self.divisor = (self.divisor & 0x00FF) | ((value as u16) << 8);
            }
            1 => self.ier = value,
            2 => {} // FCR: ignore
            3 => self.lcr = value,
            4 => self.mcr = value,
            _ => {}
        }
    }

    /// Get transmitted data
    pub fn get_tx_data(&mut self) -> Option<u8> {
        if self.tx_head != self.tx_tail {
            let byte = self.tx_buffer[self.tx_tail];
            self.tx_tail = (self.tx_tail + 1) % 256;
            Some(byte)
        } else {
            None
        }
    }

    /// Add received data
    pub fn add_rx_data(&mut self, byte: u8) {
        self.rx_buffer[self.rx_head] = byte;
        self.rx_head = (self.rx_head + 1) % 256;
    }
}
