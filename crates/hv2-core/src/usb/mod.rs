//! USB Subsystem
//!
//! This module provides USB device emulation including:
//! - xHCI (USB 3.0) host controller
//! - USB device framework
//! - HID class devices (keyboard, mouse, tablet)

pub mod device;
pub mod hid;
pub mod xhci;

// Re-export key types
pub use device::{
    BaseUsbDevice, ConfigDescriptor, ControlResult, DescriptorType, DeviceClass, DeviceDescriptor,
    DeviceState, Endpoint, EndpointDescriptor, EndpointDirection, InterfaceDescriptor, SetupPacket,
    StringDescriptor, TransferResult, TransferType, UsbDevice,
};

pub use hid::{
    HidDescriptor, HidProtocol, HidStats, HidSubclass, KeyboardModifiers, MouseButtons, ReportType,
    UsbKeyboard, UsbMouse, UsbTablet,
};

pub use xhci::{
    CommandRing, DeviceSlot, ErstEntry, EventRing, Interrupter, PortRegister, PortState,
    RingSegment, SlotState, Trb, TrbCompletionCode, TrbType, UsbSpeed, XhciController, XhciPort,
};

/// USB constants
pub mod constants {
    /// USB PID (Packet ID) tokens
    pub mod pid {
        pub const OUT: u8 = 0xE1;
        pub const IN: u8 = 0x69;
        pub const SOF: u8 = 0xA5;
        pub const SETUP: u8 = 0x2D;
        pub const DATA0: u8 = 0xC3;
        pub const DATA1: u8 = 0x4B;
        pub const DATA2: u8 = 0x87;
        pub const MDATA: u8 = 0x0F;
        pub const ACK: u8 = 0xD2;
        pub const NAK: u8 = 0x5A;
        pub const STALL: u8 = 0x1E;
        pub const NYET: u8 = 0x96;
        pub const PRE: u8 = 0x3C;
        pub const SPLIT: u8 = 0x78;
        pub const PING: u8 = 0xB4;
    }

    /// USB language IDs
    pub mod language {
        pub const ENGLISH_US: u16 = 0x0409;
        pub const ENGLISH_UK: u16 = 0x0809;
        pub const GERMAN: u16 = 0x0407;
        pub const FRENCH: u16 = 0x040C;
        pub const JAPANESE: u16 = 0x0411;
        pub const CHINESE_SIMPLIFIED: u16 = 0x0804;
    }

    /// USB speeds
    pub mod speed {
        pub const LOW: u64 = 1_500_000;
        pub const FULL: u64 = 12_000_000;
        pub const HIGH: u64 = 480_000_000;
        pub const SUPER: u64 = 5_000_000_000;
        pub const SUPER_PLUS: u64 = 10_000_000_000;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_exports() {
        // Verify key types are exported
        let _ = UsbSpeed::High;
        let _ = DeviceState::Configured;
        let _ = DeviceClass::Hid;
    }

    #[test]
    fn test_create_keyboard() {
        let kb = UsbKeyboard::new();
        assert_eq!(kb.device_descriptor().device_class, 0);
    }

    #[test]
    fn test_create_mouse() {
        let mouse = UsbMouse::new();
        assert_eq!(mouse.device_descriptor().device_class, 0);
    }

    #[test]
    fn test_create_tablet() {
        let tablet = UsbTablet::new();
        assert_eq!(tablet.device_descriptor().device_class, 0);
    }

    #[test]
    fn test_create_xhci() {
        let xhci = XhciController::new("xhci0", 2, 2);
        assert_eq!(xhci.num_ports(), 4);
    }

    #[test]
    fn test_constants() {
        assert_eq!(constants::pid::SETUP, 0x2D);
        assert_eq!(constants::language::ENGLISH_US, 0x0409);
    }
}
