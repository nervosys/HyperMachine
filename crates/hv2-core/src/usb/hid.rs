//! USB HID (Human Interface Device) Class
//!
//! This module provides USB HID device implementations including
//! keyboard, mouse, and tablet devices.

use super::device::{
    BaseUsbDevice, ConfigDescriptor, ControlResult, DeviceClass, DeviceDescriptor,
    DescriptorType, Endpoint, EndpointDescriptor, EndpointDirection, InterfaceDescriptor,
    SetupPacket, TransferResult, TransferType, UsbDevice,
};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

/// HID subclass
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidSubclass {
    /// No subclass
    None = 0,
    /// Boot interface subclass
    Boot = 1,
}

/// HID protocol
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HidProtocol {
    /// None
    None = 0,
    /// Keyboard
    Keyboard = 1,
    /// Mouse
    Mouse = 2,
}

/// HID class requests
pub mod hid_request {
    pub const GET_REPORT: u8 = 0x01;
    pub const GET_IDLE: u8 = 0x02;
    pub const GET_PROTOCOL: u8 = 0x03;
    pub const SET_REPORT: u8 = 0x09;
    pub const SET_IDLE: u8 = 0x0A;
    pub const SET_PROTOCOL: u8 = 0x0B;
}

/// HID report type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportType {
    /// Input report
    Input = 1,
    /// Output report
    Output = 2,
    /// Feature report
    Feature = 3,
}

/// HID descriptor (variable length)
#[derive(Debug, Clone)]
pub struct HidDescriptor {
    /// HID specification version (BCD)
    pub hid_version: u16,
    /// Country code
    pub country_code: u8,
    /// Number of descriptors
    pub num_descriptors: u8,
    /// Descriptor type (usually Report)
    pub descriptor_type: u8,
    /// Descriptor length
    pub descriptor_length: u16,
}

impl HidDescriptor {
    /// Create new HID descriptor
    pub fn new(report_length: u16) -> Self {
        Self {
            hid_version: 0x0111, // HID 1.11
            country_code: 0,
            num_descriptors: 1,
            descriptor_type: DescriptorType::HidReport as u8,
            descriptor_length: report_length,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            9, // bLength
            DescriptorType::Hid as u8,
            (self.hid_version & 0xFF) as u8,
            (self.hid_version >> 8) as u8,
            self.country_code,
            self.num_descriptors,
            self.descriptor_type,
            (self.descriptor_length & 0xFF) as u8,
            (self.descriptor_length >> 8) as u8,
        ]
    }
}

/// USB keyboard modifier keys
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardModifiers {
    /// Left Control
    pub left_ctrl: bool,
    /// Left Shift
    pub left_shift: bool,
    /// Left Alt
    pub left_alt: bool,
    /// Left GUI (Windows/Command)
    pub left_gui: bool,
    /// Right Control
    pub right_ctrl: bool,
    /// Right Shift
    pub right_shift: bool,
    /// Right Alt
    pub right_alt: bool,
    /// Right GUI
    pub right_gui: bool,
}

impl KeyboardModifiers {
    /// Convert to byte
    pub fn to_byte(&self) -> u8 {
        let mut b = 0u8;
        if self.left_ctrl { b |= 1 << 0; }
        if self.left_shift { b |= 1 << 1; }
        if self.left_alt { b |= 1 << 2; }
        if self.left_gui { b |= 1 << 3; }
        if self.right_ctrl { b |= 1 << 4; }
        if self.right_shift { b |= 1 << 5; }
        if self.right_alt { b |= 1 << 6; }
        if self.right_gui { b |= 1 << 7; }
        b
    }

    /// Create from byte
    pub fn from_byte(b: u8) -> Self {
        Self {
            left_ctrl: b & (1 << 0) != 0,
            left_shift: b & (1 << 1) != 0,
            left_alt: b & (1 << 2) != 0,
            left_gui: b & (1 << 3) != 0,
            right_ctrl: b & (1 << 4) != 0,
            right_shift: b & (1 << 5) != 0,
            right_alt: b & (1 << 6) != 0,
            right_gui: b & (1 << 7) != 0,
        }
    }
}

/// USB HID keyboard
#[derive(Debug)]
pub struct UsbKeyboard {
    /// Base device
    base: BaseUsbDevice,
    /// Report descriptor
    report_descriptor: Vec<u8>,
    /// Current modifiers
    modifiers: KeyboardModifiers,
    /// Currently pressed keys (up to 6)
    pressed_keys: [u8; 6],
    /// LED state
    led_state: u8,
    /// Idle rate (4ms units, 0 = infinite)
    idle_rate: u8,
    /// Protocol (0 = boot, 1 = report)
    protocol: u8,
    /// Pending reports queue
    pending_reports: VecDeque<[u8; 8]>,
    /// Statistics
    pub stats: HidStats,
}

/// HID statistics
#[derive(Debug, Default)]
pub struct HidStats {
    /// Reports sent
    pub reports_sent: AtomicU64,
    /// Key presses
    pub key_presses: AtomicU64,
    /// Key releases
    pub key_releases: AtomicU64,
}

impl UsbKeyboard {
    /// Create new USB keyboard
    pub fn new() -> Self {
        let mut base = BaseUsbDevice::new(0x1234, 0x0001);

        // Configure device descriptor
        base.device_desc.device_class = 0;
        base.device_desc.device_subclass = 0;
        base.device_desc.device_protocol = 0;

        base.set_manufacturer("AetherVM");
        base.set_product("Virtual Keyboard");

        // Boot keyboard report descriptor
        let report_descriptor = vec![
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x06, // Usage (Keyboard)
            0xA1, 0x01, // Collection (Application)
            0x05, 0x07, //   Usage Page (Key Codes)
            0x19, 0xE0, //   Usage Minimum (224)
            0x29, 0xE7, //   Usage Maximum (231)
            0x15, 0x00, //   Logical Minimum (0)
            0x25, 0x01, //   Logical Maximum (1)
            0x75, 0x01, //   Report Size (1)
            0x95, 0x08, //   Report Count (8)
            0x81, 0x02, //   Input (Data, Variable, Absolute) - Modifier byte
            0x95, 0x01, //   Report Count (1)
            0x75, 0x08, //   Report Size (8)
            0x81, 0x01, //   Input (Constant) - Reserved byte
            0x95, 0x05, //   Report Count (5)
            0x75, 0x01, //   Report Size (1)
            0x05, 0x08, //   Usage Page (LEDs)
            0x19, 0x01, //   Usage Minimum (1)
            0x29, 0x05, //   Usage Maximum (5)
            0x91, 0x02, //   Output (Data, Variable, Absolute) - LED report
            0x95, 0x01, //   Report Count (1)
            0x75, 0x03, //   Report Size (3)
            0x91, 0x01, //   Output (Constant) - Padding
            0x95, 0x06, //   Report Count (6)
            0x75, 0x08, //   Report Size (8)
            0x15, 0x00, //   Logical Minimum (0)
            0x25, 0x65, //   Logical Maximum (101)
            0x05, 0x07, //   Usage Page (Key Codes)
            0x19, 0x00, //   Usage Minimum (0)
            0x29, 0x65, //   Usage Maximum (101)
            0x81, 0x00, //   Input (Data, Array) - Key arrays (6 keys)
            0xC0,       // End Collection
        ];

        // Build configuration descriptor
        let mut config = ConfigDescriptor::new(1, 1);
        let interface = InterfaceDescriptor::new(
            0,
            DeviceClass::Hid as u8,
            HidSubclass::Boot as u8,
            HidProtocol::Keyboard as u8,
        );
        let hid_desc = HidDescriptor::new(report_descriptor.len() as u16);
        let endpoint = EndpointDescriptor::new(1, EndpointDirection::In, TransferType::Interrupt);

        // Calculate total length
        let total_len = 9 + 9 + 9 + 7; // config + interface + hid + endpoint
        config.total_length = total_len;

        // Serialize all descriptors
        let mut config_data = Vec::new();
        config_data.extend_from_slice(&config.to_bytes());

        let mut iface_bytes = interface.to_bytes();
        iface_bytes[4] = 1; // num_endpoints
        config_data.extend_from_slice(&iface_bytes);

        config_data.extend_from_slice(&hid_desc.to_bytes());

        let mut ep_bytes = endpoint.to_bytes();
        ep_bytes[4] = 8; // max packet size low byte
        ep_bytes[6] = 10; // interval (10ms)
        config_data.extend_from_slice(&ep_bytes);

        base.config_descs.push(config_data);

        // Add interrupt IN endpoint
        let mut ep = Endpoint::new(EndpointDescriptor::new(
            1,
            EndpointDirection::In,
            TransferType::Interrupt,
        ));
        ep.descriptor.max_packet_size = 8;
        ep.descriptor.interval = 10;
        base.add_endpoint(ep);

        Self {
            base,
            report_descriptor,
            modifiers: KeyboardModifiers::default(),
            pressed_keys: [0; 6],
            led_state: 0,
            idle_rate: 0,
            protocol: 1, // Report protocol
            pending_reports: VecDeque::new(),
            stats: HidStats::default(),
        }
    }

    /// Press a key
    pub fn key_press(&mut self, keycode: u8) {
        // Check if already pressed
        if self.pressed_keys.contains(&keycode) {
            return;
        }

        // Find empty slot
        for i in 0..6 {
            if self.pressed_keys[i] == 0 {
                self.pressed_keys[i] = keycode;
                self.queue_report();
                self.stats.key_presses.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }

        // Rollover - too many keys
    }

    /// Release a key
    pub fn key_release(&mut self, keycode: u8) {
        for i in 0..6 {
            if self.pressed_keys[i] == keycode {
                self.pressed_keys[i] = 0;
                self.queue_report();
                self.stats.key_releases.fetch_add(1, Ordering::Relaxed);
                return;
            }
        }
    }

    /// Set modifier state
    pub fn set_modifiers(&mut self, modifiers: KeyboardModifiers) {
        self.modifiers = modifiers;
        self.queue_report();
    }

    /// Get LED state
    pub fn led_state(&self) -> u8 {
        self.led_state
    }

    /// Queue a keyboard report
    fn queue_report(&mut self) {
        let report = [
            self.modifiers.to_byte(),
            0, // Reserved
            self.pressed_keys[0],
            self.pressed_keys[1],
            self.pressed_keys[2],
            self.pressed_keys[3],
            self.pressed_keys[4],
            self.pressed_keys[5],
        ];
        self.pending_reports.push_back(report);
    }

    /// Handle HID class request
    fn handle_hid_request(&mut self, setup: &SetupPacket, data: &[u8]) -> ControlResult {
        match setup.request {
            hid_request::GET_REPORT => {
                // Return current report
                let report = vec![
                    self.modifiers.to_byte(),
                    0,
                    self.pressed_keys[0],
                    self.pressed_keys[1],
                    self.pressed_keys[2],
                    self.pressed_keys[3],
                    self.pressed_keys[4],
                    self.pressed_keys[5],
                ];
                ControlResult::Ok(report)
            }
            hid_request::SET_REPORT => {
                // LED output report
                if !data.is_empty() {
                    self.led_state = data[0];
                }
                ControlResult::Ok(Vec::new())
            }
            hid_request::GET_IDLE => {
                ControlResult::Ok(vec![self.idle_rate])
            }
            hid_request::SET_IDLE => {
                self.idle_rate = (setup.value >> 8) as u8;
                ControlResult::Ok(Vec::new())
            }
            hid_request::GET_PROTOCOL => {
                ControlResult::Ok(vec![self.protocol])
            }
            hid_request::SET_PROTOCOL => {
                self.protocol = (setup.value & 0xFF) as u8;
                ControlResult::Ok(Vec::new())
            }
            _ => ControlResult::Stall,
        }
    }
}

impl Default for UsbKeyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbDevice for UsbKeyboard {
    fn device_descriptor(&self) -> &DeviceDescriptor {
        &self.base.device_desc
    }

    fn configuration_descriptor(&self, index: u8) -> Option<Vec<u8>> {
        self.base.config_descs.get(index as usize).cloned()
    }

    fn string_descriptor(&self, index: u8, _lang_id: u16) -> Option<Vec<u8>> {
        self.base.string_descs.get(&index).map(|s| s.to_bytes().to_vec())
    }

    fn control_transfer(&mut self, setup: &SetupPacket, data: &[u8]) -> ControlResult {
        let req_type = setup.request_type_type();

        match req_type {
            0 => {
                // Standard request
                if setup.request == 6 && (setup.value >> 8) == DescriptorType::HidReport as u16 {
                    // GET_DESCRIPTOR for HID Report
                    let len = setup.length as usize;
                    return ControlResult::Ok(
                        self.report_descriptor[..len.min(self.report_descriptor.len())].to_vec(),
                    );
                }
                if setup.request == 6 && (setup.value >> 8) == DescriptorType::Hid as u16 {
                    // GET_DESCRIPTOR for HID
                    let hid_desc = HidDescriptor::new(self.report_descriptor.len() as u16);
                    return ControlResult::Ok(hid_desc.to_bytes());
                }
                self.base.handle_standard_request(setup)
            }
            1 => {
                // Class request
                self.handle_hid_request(setup, data)
            }
            _ => ControlResult::Stall,
        }
    }

    fn data_in(&mut self, endpoint: u8, _max_length: u16) -> TransferResult {
        if endpoint == 0x81 {
            // Interrupt IN
            if let Some(report) = self.pending_reports.pop_front() {
                self.stats.reports_sent.fetch_add(1, Ordering::Relaxed);
                return TransferResult::Ok(report.to_vec());
            }
            return TransferResult::Nak;
        }
        TransferResult::Stall
    }

    fn data_out(&mut self, _endpoint: u8, _data: &[u8]) -> TransferResult {
        TransferResult::Stall
    }

    fn reset(&mut self) {
        self.base.reset();
        self.modifiers = KeyboardModifiers::default();
        self.pressed_keys = [0; 6];
        self.pending_reports.clear();
    }

    fn set_address(&mut self, address: u8) {
        self.base.address = address;
    }

    fn address(&self) -> u8 {
        self.base.address
    }

    fn set_configuration(&mut self, config: u8) -> bool {
        self.base.config = config;
        true
    }

    fn configuration(&self) -> u8 {
        self.base.config
    }
}

/// Mouse button state
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseButtons {
    /// Left button
    pub left: bool,
    /// Right button
    pub right: bool,
    /// Middle button
    pub middle: bool,
    /// Button 4
    pub button4: bool,
    /// Button 5
    pub button5: bool,
}

impl MouseButtons {
    /// Convert to byte
    pub fn to_byte(&self) -> u8 {
        let mut b = 0u8;
        if self.left { b |= 1 << 0; }
        if self.right { b |= 1 << 1; }
        if self.middle { b |= 1 << 2; }
        if self.button4 { b |= 1 << 3; }
        if self.button5 { b |= 1 << 4; }
        b
    }
}

/// USB HID mouse
#[derive(Debug)]
pub struct UsbMouse {
    /// Base device
    base: BaseUsbDevice,
    /// Report descriptor
    report_descriptor: Vec<u8>,
    /// Button state
    buttons: MouseButtons,
    /// Pending reports
    pending_reports: VecDeque<[u8; 4]>,
    /// Idle rate
    idle_rate: u8,
    /// Protocol
    protocol: u8,
    /// Statistics
    pub stats: HidStats,
}

impl UsbMouse {
    /// Create new USB mouse
    pub fn new() -> Self {
        let mut base = BaseUsbDevice::new(0x1234, 0x0002);

        base.device_desc.device_class = 0;
        base.set_manufacturer("AetherVM");
        base.set_product("Virtual Mouse");

        // Boot mouse report descriptor (relative movement)
        let report_descriptor = vec![
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x02, // Usage (Mouse)
            0xA1, 0x01, // Collection (Application)
            0x09, 0x01, //   Usage (Pointer)
            0xA1, 0x00, //   Collection (Physical)
            0x05, 0x09, //     Usage Page (Buttons)
            0x19, 0x01, //     Usage Minimum (1)
            0x29, 0x05, //     Usage Maximum (5)
            0x15, 0x00, //     Logical Minimum (0)
            0x25, 0x01, //     Logical Maximum (1)
            0x95, 0x05, //     Report Count (5)
            0x75, 0x01, //     Report Size (1)
            0x81, 0x02, //     Input (Data, Variable, Absolute) - Buttons
            0x95, 0x01, //     Report Count (1)
            0x75, 0x03, //     Report Size (3)
            0x81, 0x01, //     Input (Constant) - Padding
            0x05, 0x01, //     Usage Page (Generic Desktop)
            0x09, 0x30, //     Usage (X)
            0x09, 0x31, //     Usage (Y)
            0x09, 0x38, //     Usage (Wheel)
            0x15, 0x81, //     Logical Minimum (-127)
            0x25, 0x7F, //     Logical Maximum (127)
            0x75, 0x08, //     Report Size (8)
            0x95, 0x03, //     Report Count (3)
            0x81, 0x06, //     Input (Data, Variable, Relative)
            0xC0,       //   End Collection
            0xC0,       // End Collection
        ];

        // Build configuration descriptor
        let mut config = ConfigDescriptor::new(1, 1);
        let interface = InterfaceDescriptor::new(
            0,
            DeviceClass::Hid as u8,
            HidSubclass::Boot as u8,
            HidProtocol::Mouse as u8,
        );
        let hid_desc = HidDescriptor::new(report_descriptor.len() as u16);
        let endpoint = EndpointDescriptor::new(1, EndpointDirection::In, TransferType::Interrupt);

        config.total_length = 9 + 9 + 9 + 7;

        let mut config_data = Vec::new();
        config_data.extend_from_slice(&config.to_bytes());

        let mut iface_bytes = interface.to_bytes();
        iface_bytes[4] = 1;
        config_data.extend_from_slice(&iface_bytes);

        config_data.extend_from_slice(&hid_desc.to_bytes());

        let mut ep_bytes = endpoint.to_bytes();
        ep_bytes[4] = 4;
        ep_bytes[6] = 10;
        config_data.extend_from_slice(&ep_bytes);

        base.config_descs.push(config_data);

        Self {
            base,
            report_descriptor,
            buttons: MouseButtons::default(),
            pending_reports: VecDeque::new(),
            idle_rate: 0,
            protocol: 1,
            stats: HidStats::default(),
        }
    }

    /// Move mouse (relative)
    pub fn move_relative(&mut self, dx: i8, dy: i8) {
        let report = [
            self.buttons.to_byte(),
            dx as u8,
            dy as u8,
            0, // Wheel
        ];
        self.pending_reports.push_back(report);
    }

    /// Move mouse with wheel
    pub fn move_with_wheel(&mut self, dx: i8, dy: i8, wheel: i8) {
        let report = [
            self.buttons.to_byte(),
            dx as u8,
            dy as u8,
            wheel as u8,
        ];
        self.pending_reports.push_back(report);
    }

    /// Set button state
    pub fn set_buttons(&mut self, buttons: MouseButtons) {
        self.buttons = buttons;
        let report = [self.buttons.to_byte(), 0, 0, 0];
        self.pending_reports.push_back(report);
    }

    /// Click button
    pub fn click(&mut self, button: u8) {
        let mut buttons = self.buttons;
        match button {
            0 => buttons.left = true,
            1 => buttons.right = true,
            2 => buttons.middle = true,
            _ => {}
        }
        self.set_buttons(buttons);

        // Release
        match button {
            0 => buttons.left = false,
            1 => buttons.right = false,
            2 => buttons.middle = false,
            _ => {}
        }
        self.set_buttons(buttons);
    }
}

impl Default for UsbMouse {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbDevice for UsbMouse {
    fn device_descriptor(&self) -> &DeviceDescriptor {
        &self.base.device_desc
    }

    fn configuration_descriptor(&self, index: u8) -> Option<Vec<u8>> {
        self.base.config_descs.get(index as usize).cloned()
    }

    fn string_descriptor(&self, index: u8, _lang_id: u16) -> Option<Vec<u8>> {
        self.base.string_descs.get(&index).map(|s| s.to_bytes().to_vec())
    }

    fn control_transfer(&mut self, setup: &SetupPacket, _data: &[u8]) -> ControlResult {
        let req_type = setup.request_type_type();

        match req_type {
            0 => {
                if setup.request == 6 && (setup.value >> 8) == DescriptorType::HidReport as u16 {
                    let len = setup.length as usize;
                    return ControlResult::Ok(
                        self.report_descriptor[..len.min(self.report_descriptor.len())].to_vec(),
                    );
                }
                self.base.handle_standard_request(setup)
            }
            1 => {
                // Class requests
                match setup.request {
                    hid_request::GET_IDLE => ControlResult::Ok(vec![self.idle_rate]),
                    hid_request::SET_IDLE => {
                        self.idle_rate = (setup.value >> 8) as u8;
                        ControlResult::Ok(Vec::new())
                    }
                    hid_request::GET_PROTOCOL => ControlResult::Ok(vec![self.protocol]),
                    hid_request::SET_PROTOCOL => {
                        self.protocol = (setup.value & 0xFF) as u8;
                        ControlResult::Ok(Vec::new())
                    }
                    _ => ControlResult::Stall,
                }
            }
            _ => ControlResult::Stall,
        }
    }

    fn data_in(&mut self, endpoint: u8, _max_length: u16) -> TransferResult {
        if endpoint == 0x81 {
            if let Some(report) = self.pending_reports.pop_front() {
                self.stats.reports_sent.fetch_add(1, Ordering::Relaxed);
                return TransferResult::Ok(report.to_vec());
            }
            return TransferResult::Nak;
        }
        TransferResult::Stall
    }

    fn data_out(&mut self, _endpoint: u8, _data: &[u8]) -> TransferResult {
        TransferResult::Stall
    }

    fn reset(&mut self) {
        self.base.reset();
        self.buttons = MouseButtons::default();
        self.pending_reports.clear();
    }

    fn set_address(&mut self, address: u8) {
        self.base.address = address;
    }

    fn address(&self) -> u8 {
        self.base.address
    }

    fn set_configuration(&mut self, config: u8) -> bool {
        self.base.config = config;
        true
    }

    fn configuration(&self) -> u8 {
        self.base.config
    }
}

/// USB HID tablet (absolute positioning)
#[derive(Debug)]
pub struct UsbTablet {
    /// Base device
    base: BaseUsbDevice,
    /// Report descriptor
    report_descriptor: Vec<u8>,
    /// Button state
    buttons: MouseButtons,
    /// Current X position (0-32767)
    x: u16,
    /// Current Y position (0-32767)
    y: u16,
    /// Pending reports
    pending_reports: VecDeque<[u8; 6]>,
    /// Statistics
    pub stats: HidStats,
}

impl UsbTablet {
    /// Maximum X coordinate
    pub const MAX_X: u16 = 32767;
    /// Maximum Y coordinate
    pub const MAX_Y: u16 = 32767;

    /// Create new USB tablet
    pub fn new() -> Self {
        let mut base = BaseUsbDevice::new(0x1234, 0x0003);

        base.device_desc.device_class = 0;
        base.set_manufacturer("AetherVM");
        base.set_product("Virtual Tablet");

        // Tablet report descriptor (absolute positioning)
        let report_descriptor = vec![
            0x05, 0x01, // Usage Page (Generic Desktop)
            0x09, 0x02, // Usage (Mouse)
            0xA1, 0x01, // Collection (Application)
            0x09, 0x01, //   Usage (Pointer)
            0xA1, 0x00, //   Collection (Physical)
            0x05, 0x09, //     Usage Page (Buttons)
            0x19, 0x01, //     Usage Minimum (1)
            0x29, 0x05, //     Usage Maximum (5)
            0x15, 0x00, //     Logical Minimum (0)
            0x25, 0x01, //     Logical Maximum (1)
            0x95, 0x05, //     Report Count (5)
            0x75, 0x01, //     Report Size (1)
            0x81, 0x02, //     Input (Data, Variable, Absolute) - Buttons
            0x95, 0x01, //     Report Count (1)
            0x75, 0x03, //     Report Size (3)
            0x81, 0x01, //     Input (Constant) - Padding
            0x05, 0x01, //     Usage Page (Generic Desktop)
            0x09, 0x30, //     Usage (X)
            0x09, 0x31, //     Usage (Y)
            0x15, 0x00, //     Logical Minimum (0)
            0x26, 0xFF, 0x7F, // Logical Maximum (32767)
            0x35, 0x00, //     Physical Minimum (0)
            0x46, 0xFF, 0x7F, // Physical Maximum (32767)
            0x75, 0x10, //     Report Size (16)
            0x95, 0x02, //     Report Count (2)
            0x81, 0x02, //     Input (Data, Variable, Absolute)
            0xC0,       //   End Collection
            0xC0,       // End Collection
        ];

        let mut config = ConfigDescriptor::new(1, 1);
        let interface = InterfaceDescriptor::new(
            0,
            DeviceClass::Hid as u8,
            HidSubclass::None as u8,
            HidProtocol::None as u8,
        );
        let hid_desc = HidDescriptor::new(report_descriptor.len() as u16);
        let endpoint = EndpointDescriptor::new(1, EndpointDirection::In, TransferType::Interrupt);

        config.total_length = 9 + 9 + 9 + 7;

        let mut config_data = Vec::new();
        config_data.extend_from_slice(&config.to_bytes());

        let mut iface_bytes = interface.to_bytes();
        iface_bytes[4] = 1;
        config_data.extend_from_slice(&iface_bytes);

        config_data.extend_from_slice(&hid_desc.to_bytes());

        let mut ep_bytes = endpoint.to_bytes();
        ep_bytes[4] = 6;
        ep_bytes[6] = 10;
        config_data.extend_from_slice(&ep_bytes);

        base.config_descs.push(config_data);

        Self {
            base,
            report_descriptor,
            buttons: MouseButtons::default(),
            x: 0,
            y: 0,
            pending_reports: VecDeque::new(),
            stats: HidStats::default(),
        }
    }

    /// Set absolute position
    pub fn set_position(&mut self, x: u16, y: u16) {
        self.x = x.min(Self::MAX_X);
        self.y = y.min(Self::MAX_Y);
        self.queue_report();
    }

    /// Set position from normalized coordinates (0.0 - 1.0)
    pub fn set_position_normalized(&mut self, x: f32, y: f32) {
        let x = ((x.clamp(0.0, 1.0) * Self::MAX_X as f32) as u16).min(Self::MAX_X);
        let y = ((y.clamp(0.0, 1.0) * Self::MAX_Y as f32) as u16).min(Self::MAX_Y);
        self.set_position(x, y);
    }

    /// Set button state
    pub fn set_buttons(&mut self, buttons: MouseButtons) {
        self.buttons = buttons;
        self.queue_report();
    }

    /// Queue report
    fn queue_report(&mut self) {
        let report = [
            self.buttons.to_byte(),
            (self.x & 0xFF) as u8,
            (self.x >> 8) as u8,
            (self.y & 0xFF) as u8,
            (self.y >> 8) as u8,
            0, // Padding
        ];
        self.pending_reports.push_back(report);
    }

    /// Get current position
    pub fn position(&self) -> (u16, u16) {
        (self.x, self.y)
    }
}

impl Default for UsbTablet {
    fn default() -> Self {
        Self::new()
    }
}

impl UsbDevice for UsbTablet {
    fn device_descriptor(&self) -> &DeviceDescriptor {
        &self.base.device_desc
    }

    fn configuration_descriptor(&self, index: u8) -> Option<Vec<u8>> {
        self.base.config_descs.get(index as usize).cloned()
    }

    fn string_descriptor(&self, index: u8, _lang_id: u16) -> Option<Vec<u8>> {
        self.base.string_descs.get(&index).map(|s| s.to_bytes().to_vec())
    }

    fn control_transfer(&mut self, setup: &SetupPacket, _data: &[u8]) -> ControlResult {
        let req_type = setup.request_type_type();

        if req_type == 0 {
            if setup.request == 6 && (setup.value >> 8) == DescriptorType::HidReport as u16 {
                let len = setup.length as usize;
                return ControlResult::Ok(
                    self.report_descriptor[..len.min(self.report_descriptor.len())].to_vec(),
                );
            }
            return self.base.handle_standard_request(setup);
        }

        ControlResult::Stall
    }

    fn data_in(&mut self, endpoint: u8, _max_length: u16) -> TransferResult {
        if endpoint == 0x81 {
            if let Some(report) = self.pending_reports.pop_front() {
                self.stats.reports_sent.fetch_add(1, Ordering::Relaxed);
                return TransferResult::Ok(report.to_vec());
            }
            return TransferResult::Nak;
        }
        TransferResult::Stall
    }

    fn data_out(&mut self, _endpoint: u8, _data: &[u8]) -> TransferResult {
        TransferResult::Stall
    }

    fn reset(&mut self) {
        self.base.reset();
        self.buttons = MouseButtons::default();
        self.x = 0;
        self.y = 0;
        self.pending_reports.clear();
    }

    fn set_address(&mut self, address: u8) {
        self.base.address = address;
    }

    fn address(&self) -> u8 {
        self.base.address
    }

    fn set_configuration(&mut self, config: u8) -> bool {
        self.base.config = config;
        true
    }

    fn configuration(&self) -> u8 {
        self.base.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyboard_modifiers() {
        let mut mods = KeyboardModifiers::default();
        assert_eq!(mods.to_byte(), 0);

        mods.left_ctrl = true;
        mods.left_shift = true;
        assert_eq!(mods.to_byte(), 0x03);

        let restored = KeyboardModifiers::from_byte(0x03);
        assert!(restored.left_ctrl);
        assert!(restored.left_shift);
    }

    #[test]
    fn test_hid_descriptor() {
        let desc = HidDescriptor::new(65);
        let bytes = desc.to_bytes();
        assert_eq!(bytes.len(), 9);
        assert_eq!(bytes[0], 9); // bLength
        assert_eq!(bytes[1], DescriptorType::Hid as u8);
    }

    #[test]
    fn test_usb_keyboard_creation() {
        let kb = UsbKeyboard::new();
        assert_eq!(kb.address(), 0);
        assert_eq!(kb.configuration(), 0);
        assert!(kb.pending_reports.is_empty());
    }

    #[test]
    fn test_keyboard_key_press() {
        let mut kb = UsbKeyboard::new();

        kb.key_press(0x04); // 'A'
        assert_eq!(kb.pressed_keys[0], 0x04);
        assert!(!kb.pending_reports.is_empty());
    }

    #[test]
    fn test_keyboard_key_release() {
        let mut kb = UsbKeyboard::new();

        kb.key_press(0x04);
        kb.key_release(0x04);

        assert_eq!(kb.pressed_keys[0], 0);
    }

    #[test]
    fn test_keyboard_set_modifiers() {
        let mut kb = UsbKeyboard::new();

        let mods = KeyboardModifiers {
            left_ctrl: true,
            left_shift: true,
            ..Default::default()
        };
        kb.set_modifiers(mods);

        assert!(kb.modifiers.left_ctrl);
        assert!(kb.modifiers.left_shift);
    }

    #[test]
    fn test_keyboard_data_in() {
        let mut kb = UsbKeyboard::new();
        kb.key_press(0x04);

        match kb.data_in(0x81, 8) {
            TransferResult::Ok(data) => {
                assert_eq!(data.len(), 8);
                assert_eq!(data[2], 0x04); // First key
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_keyboard_led_state() {
        let mut kb = UsbKeyboard::new();

        let setup = SetupPacket {
            request_type: 0x21, // Class, Interface, Host-to-device
            request: hid_request::SET_REPORT,
            value: 0x0200, // Output report
            index: 0,
            length: 1,
        };

        kb.control_transfer(&setup, &[0x07]); // Num+Caps+Scroll
        assert_eq!(kb.led_state(), 0x07);
    }

    #[test]
    fn test_mouse_buttons() {
        let mut buttons = MouseButtons::default();
        assert_eq!(buttons.to_byte(), 0);

        buttons.left = true;
        buttons.right = true;
        assert_eq!(buttons.to_byte(), 0x03);
    }

    #[test]
    fn test_usb_mouse_creation() {
        let mouse = UsbMouse::new();
        assert_eq!(mouse.address(), 0);
    }

    #[test]
    fn test_mouse_move() {
        let mut mouse = UsbMouse::new();

        mouse.move_relative(10, -5);
        assert!(!mouse.pending_reports.is_empty());

        match mouse.data_in(0x81, 4) {
            TransferResult::Ok(data) => {
                assert_eq!(data[1], 10);  // X
                assert_eq!(data[2] as i8, -5); // Y
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_mouse_click() {
        let mut mouse = UsbMouse::new();

        mouse.click(0); // Left click

        // Should have 2 reports (press + release)
        assert_eq!(mouse.pending_reports.len(), 2);
    }

    #[test]
    fn test_tablet_creation() {
        let tablet = UsbTablet::new();
        assert_eq!(tablet.position(), (0, 0));
    }

    #[test]
    fn test_tablet_position() {
        let mut tablet = UsbTablet::new();

        tablet.set_position(16383, 16383);
        assert_eq!(tablet.position(), (16383, 16383));
    }

    #[test]
    fn test_tablet_normalized_position() {
        let mut tablet = UsbTablet::new();

        tablet.set_position_normalized(0.5, 0.5);
        let (x, y) = tablet.position();
        assert!(x > 16000 && x < 17000);
        assert!(y > 16000 && y < 17000);
    }

    #[test]
    fn test_tablet_report() {
        let mut tablet = UsbTablet::new();
        tablet.set_position(1000, 2000);

        match tablet.data_in(0x81, 6) {
            TransferResult::Ok(data) => {
                let x = u16::from_le_bytes([data[1], data[2]]);
                let y = u16::from_le_bytes([data[3], data[4]]);
                assert_eq!(x, 1000);
                assert_eq!(y, 2000);
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_device_descriptors() {
        let kb = UsbKeyboard::new();
        let desc = kb.device_descriptor();
        assert_eq!(desc.vendor_id, 0x1234);
        assert_eq!(desc.product_id, 0x0001);
    }

    #[test]
    fn test_configuration_descriptor() {
        let kb = UsbKeyboard::new();
        let config = kb.configuration_descriptor(0).unwrap();
        assert!(!config.is_empty());
        assert_eq!(config[1], DescriptorType::Configuration as u8);
    }

    #[test]
    fn test_keyboard_reset() {
        let mut kb = UsbKeyboard::new();
        kb.set_address(5);
        kb.key_press(0x04);

        kb.reset();

        assert_eq!(kb.address(), 0);
        assert!(kb.pending_reports.is_empty());
        assert_eq!(kb.pressed_keys[0], 0);
    }
}
