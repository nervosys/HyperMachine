//! VM Exit handling
//!
//! This module defines the types and mechanisms for handling VM exits.
//! When a VM exits hardware virtualization mode, it returns an exit reason
//! that the hypervisor must handle before resuming execution.


/// Direction of I/O operation
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDirection {
    /// I/O read (IN instruction)
    In,
    /// I/O write (OUT instruction)
    Out,
}

/// VM exit reasons
///
/// This enum represents all the reasons why a VM might exit from
/// hardware virtualization mode and return control to the hypervisor.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum VmExit {
    /// Memory-mapped I/O access
    ///
    /// The guest attempted to access a memory address that is mapped to a device.
    /// The hypervisor must emulate this access by forwarding it to the appropriate device.
    Mmio {
        /// Physical address being accessed
        phys_addr: u64,
        /// Data being written (if is_write) or buffer for read data
        data: [u8; 8],
        /// Length of access in bytes (1, 2, 4, or 8)
        len: u32,
        /// True if this is a write, false if read
        is_write: bool,
    },

    /// I/O port access
    ///
    /// The guest executed an IN or OUT instruction to access an I/O port.
    /// Common on x86 for accessing legacy devices like PIC, PIT, serial ports.
    Io {
        /// I/O port number (0-65535)
        port: u16,
        /// Direction of the I/O operation
        direction: IoDirection,
        /// Size of access in bytes (1, 2, or 4)
        size: u8,
        /// Data being written (OUT) or read (IN)
        data: u32,
    },

    /// HLT instruction executed
    ///
    /// The guest executed a HLT (halt) instruction, typically when idle
    /// waiting for an interrupt. The hypervisor should wait for a pending
    /// interrupt before resuming execution.
    Hlt,

    /// Shutdown requested
    ///
    /// The guest has initiated a shutdown sequence (e.g., triple fault,
    /// ACPI shutdown). The VM should stop execution.
    Shutdown,

    /// Interrupt window opened
    ///
    /// The guest is now able to receive interrupts. The hypervisor should
    /// inject any pending interrupts.
    InterruptWindow,

    /// Exception occurred
    ///
    /// A CPU exception occurred that needs to be handled by the hypervisor
    /// or injected into the guest.
    Exception {
        /// Exception vector (0-31 for x86_64)
        vector: u8,
        /// Optional error code (for exceptions that push error codes)
        error_code: Option<u32>,
    },

    /// Debug/breakpoint
    ///
    /// A breakpoint or debug exception occurred. Used for debugging and tracing.
    Debug {
        /// Debug information string
        info: String,
    },

    /// Hypercall from the guest
    ///
    /// The guest issued a hypercall (VMCALL/VMMCALL instruction).
    Hypercall {
        /// Hypercall number
        nr: u64,
        /// Hypercall arguments
        args: [u64; 6],
    },

    /// System event (e.g. reset, shutdown, crash)
    ///
    /// A system-level event occurred in the guest, such as a reset
    /// request or a guest crash notification.
    SystemEvent {
        /// Event type (e.g. KVM_SYSTEM_EVENT_SHUTDOWN, RESET, CRASH)
        type_: u32,
        /// Event flags
        flags: u64,
    },

    /// Non-maskable interrupt
    ///
    /// A non-maskable interrupt (NMI) was delivered to the guest.
    Nmi,

    /// MSR read (RDMSR instruction)
    ///
    /// The guest attempted to read a model-specific register that the
    /// hypervisor must handle.
    Rdmsr {
        /// MSR index being read
        index: u32,
    },

    /// MSR write (WRMSR instruction)
    ///
    /// The guest attempted to write a model-specific register that the
    /// hypervisor must handle.
    Wrmsr {
        /// MSR index being written
        index: u32,
        /// Data being written
        data: u64,
    },

    /// IOAPIC end-of-interrupt
    ///
    /// An end-of-interrupt (EOI) was signalled for an IOAPIC vector.
    IoapicEoi {
        /// Interrupt vector that completed
        vector: u8,
    },

    /// Unknown or unhandled exit reason
    Unknown {
        /// Exit reason code (platform-specific)
        reason: u32,
    },
}

impl VmExit {
    /// Create an MMIO read exit
    pub fn mmio_read(phys_addr: u64, len: u32) -> Self {
        Self::Mmio {
            phys_addr,
            data: [0; 8],
            len,
            is_write: false,
        }
    }

    /// Create an MMIO write exit
    pub fn mmio_write(phys_addr: u64, data: &[u8]) -> Self {
        let mut data_array = [0u8; 8];
        let len = data.len().min(8);
        data_array[..len].copy_from_slice(&data[..len]);

        Self::Mmio {
            phys_addr,
            data: data_array,
            len: len as u32,
            is_write: true,
        }
    }

    /// Create an I/O port IN exit
    pub fn io_in(port: u16, size: u8) -> Self {
        Self::Io {
            port,
            direction: IoDirection::In,
            size,
            data: 0,
        }
    }

    /// Create an I/O port OUT exit
    pub fn io_out(port: u16, size: u8, data: u32) -> Self {
        Self::Io {
            port,
            direction: IoDirection::Out,
            size,
            data,
        }
    }

    /// Check if this is an MMIO exit
    pub fn is_mmio(&self) -> bool {
        matches!(self, VmExit::Mmio { .. })
    }

    /// Check if this is an I/O exit
    pub fn is_io(&self) -> bool {
        matches!(self, VmExit::Io { .. })
    }

    /// Check if this is a HLT exit
    pub fn is_hlt(&self) -> bool {
        matches!(self, VmExit::Hlt)
    }

    /// Check if this is a shutdown exit
    pub fn is_shutdown(&self) -> bool {
        matches!(self, VmExit::Shutdown)
    }
}

impl std::fmt::Display for VmExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VmExit::Mmio {
                phys_addr,
                len,
                is_write,
                ..
            } => {
                let op = if *is_write { "WRITE" } else { "READ" };
                write!(f, "MMIO {} at {:#x} ({} bytes)", op, phys_addr, len)
            }
            VmExit::Io {
                port,
                direction,
                size,
                data,
            } => {
                let op = match direction {
                    IoDirection::In => "IN",
                    IoDirection::Out => "OUT",
                };
                write!(
                    f,
                    "IO {} port {:#x} ({} bytes, data={:#x})",
                    op, port, size, data
                )
            }
            VmExit::Hlt => write!(f, "HLT"),
            VmExit::Shutdown => write!(f, "SHUTDOWN"),
            VmExit::InterruptWindow => write!(f, "INTERRUPT_WINDOW"),
            VmExit::Exception { vector, error_code } => {
                if let Some(code) = error_code {
                    write!(f, "EXCEPTION vector={} error_code={:#x}", vector, code)
                } else {
                    write!(f, "EXCEPTION vector={}", vector)
                }
            }
            VmExit::Debug { info } => write!(f, "DEBUG: {}", info),
            VmExit::Hypercall { nr, .. } => write!(f, "HYPERCALL nr={:#x}", nr),
            VmExit::SystemEvent { type_, flags } => {
                write!(f, "SYSTEM_EVENT type={} flags={:#x}", type_, flags)
            }
            VmExit::Nmi => write!(f, "NMI"),
            VmExit::Rdmsr { index } => write!(f, "RDMSR index={:#x}", index),
            VmExit::Wrmsr { index, data } => {
                write!(f, "WRMSR index={:#x} data={:#x}", index, data)
            }
            VmExit::IoapicEoi { vector } => write!(f, "IOAPIC_EOI vector={}", vector),
            VmExit::Unknown { reason } => write!(f, "UNKNOWN exit reason={}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mmio_read() {
        let exit = VmExit::mmio_read(0x1000, 4);
        assert!(exit.is_mmio());
        assert!(!exit.is_io());
        assert!(!exit.is_hlt());

        if let VmExit::Mmio {
            phys_addr,
            len,
            is_write,
            ..
        } = exit
        {
            assert_eq!(phys_addr, 0x1000);
            assert_eq!(len, 4);
            assert!(!is_write);
        } else {
            panic!("Expected Mmio exit");
        }
    }

    #[test]
    fn test_mmio_write() {
        let data = [0x12, 0x34, 0x56, 0x78];
        let exit = VmExit::mmio_write(0x2000, &data);

        if let VmExit::Mmio {
            phys_addr,
            data: exit_data,
            len,
            is_write,
        } = exit
        {
            assert_eq!(phys_addr, 0x2000);
            assert_eq!(len, 4);
            assert!(is_write);
            assert_eq!(&exit_data[..4], &data);
        } else {
            panic!("Expected Mmio exit");
        }
    }

    #[test]
    fn test_io_in() {
        let exit = VmExit::io_in(0x3F8, 1);
        assert!(exit.is_io());

        if let VmExit::Io {
            port,
            direction,
            size,
            ..
        } = exit
        {
            assert_eq!(port, 0x3F8);
            assert_eq!(direction, IoDirection::In);
            assert_eq!(size, 1);
        } else {
            panic!("Expected Io exit");
        }
    }

    #[test]
    fn test_io_out() {
        let exit = VmExit::io_out(0x3F8, 1, 0x41);

        if let VmExit::Io {
            port,
            direction,
            size,
            data,
        } = exit
        {
            assert_eq!(port, 0x3F8);
            assert_eq!(direction, IoDirection::Out);
            assert_eq!(size, 1);
            assert_eq!(data, 0x41);
        } else {
            panic!("Expected Io exit");
        }
    }

    #[test]
    fn test_hlt() {
        let exit = VmExit::Hlt;
        assert!(exit.is_hlt());
        assert!(!exit.is_mmio());
    }

    #[test]
    fn test_shutdown() {
        let exit = VmExit::Shutdown;
        assert!(exit.is_shutdown());
    }

    #[test]
    fn test_display_formatting() {
        let exit = VmExit::mmio_read(0x1000, 4);
        assert_eq!(format!("{}", exit), "MMIO READ at 0x1000 (4 bytes)");

        let exit = VmExit::io_out(0x3F8, 1, 0x41);
        let formatted = format!("{}", exit);
        assert!(formatted.contains("IO OUT"));
        assert!(formatted.contains("0x3f8"));

        let exit = VmExit::Hlt;
        assert_eq!(format!("{}", exit), "HLT");
    }
}
