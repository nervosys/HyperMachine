//! USB Device Framework
//!
//! This module provides the core USB device abstractions including
//! descriptors, endpoints, and device state management.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// USB device state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DeviceState {
    /// Device is not attached
    #[default]
    Detached,
    /// Device is attached but not powered
    Attached,
    /// Device is powered
    Powered,
    /// Default state (address 0)
    Default,
    /// Device has address
    Address,
    /// Device is configured
    Configured,
    /// Device is suspended
    Suspended,
}

/// USB device class
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
    /// Use interface descriptors
    PerInterface = 0x00,
    /// Audio device
    Audio = 0x01,
    /// Communications device
    Comm = 0x02,
    /// Human Interface Device
    Hid = 0x03,
    /// Physical device
    Physical = 0x05,
    /// Image device
    Image = 0x06,
    /// Printer
    Printer = 0x07,
    /// Mass storage
    MassStorage = 0x08,
    /// Hub
    Hub = 0x09,
    /// CDC-Data
    CdcData = 0x0A,
    /// Smart card
    SmartCard = 0x0B,
    /// Content security
    ContentSecurity = 0x0D,
    /// Video
    Video = 0x0E,
    /// Personal healthcare
    PersonalHealthcare = 0x0F,
    /// Audio/Video
    AudioVideo = 0x10,
    /// Billboard
    Billboard = 0x11,
    /// Type-C bridge
    TypeCBridge = 0x12,
    /// Diagnostic device
    Diagnostic = 0xDC,
    /// Wireless controller
    Wireless = 0xE0,
    /// Miscellaneous
    Misc = 0xEF,
    /// Application specific
    Application = 0xFE,
    /// Vendor specific
    VendorSpecific = 0xFF,
}

impl DeviceClass {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Self {
        match value {
            0x00 => Self::PerInterface,
            0x01 => Self::Audio,
            0x02 => Self::Comm,
            0x03 => Self::Hid,
            0x05 => Self::Physical,
            0x06 => Self::Image,
            0x07 => Self::Printer,
            0x08 => Self::MassStorage,
            0x09 => Self::Hub,
            0x0A => Self::CdcData,
            0x0B => Self::SmartCard,
            0x0D => Self::ContentSecurity,
            0x0E => Self::Video,
            0x0F => Self::PersonalHealthcare,
            0x10 => Self::AudioVideo,
            0x11 => Self::Billboard,
            0x12 => Self::TypeCBridge,
            0xDC => Self::Diagnostic,
            0xE0 => Self::Wireless,
            0xEF => Self::Misc,
            0xFE => Self::Application,
            _ => Self::VendorSpecific,
        }
    }
}

/// Descriptor type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescriptorType {
    /// Device descriptor
    Device = 1,
    /// Configuration descriptor
    Configuration = 2,
    /// String descriptor
    String = 3,
    /// Interface descriptor
    Interface = 4,
    /// Endpoint descriptor
    Endpoint = 5,
    /// Device qualifier
    DeviceQualifier = 6,
    /// Other speed configuration
    OtherSpeedConfig = 7,
    /// Interface power
    InterfacePower = 8,
    /// On-The-Go
    Otg = 9,
    /// Debug descriptor
    Debug = 10,
    /// Interface association
    InterfaceAssociation = 11,
    /// BOS (Binary Object Store)
    Bos = 15,
    /// Device capability
    DeviceCapability = 16,
    /// SuperSpeed endpoint companion
    SsEndpointCompanion = 48,
    /// SuperSpeedPlus isochronous endpoint companion
    SspIsocEndpointCompanion = 49,
    /// HID descriptor
    Hid = 33,
    /// HID report descriptor
    HidReport = 34,
    /// HID physical descriptor
    HidPhysical = 35,
}

impl DescriptorType {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Device),
            2 => Some(Self::Configuration),
            3 => Some(Self::String),
            4 => Some(Self::Interface),
            5 => Some(Self::Endpoint),
            6 => Some(Self::DeviceQualifier),
            7 => Some(Self::OtherSpeedConfig),
            8 => Some(Self::InterfacePower),
            9 => Some(Self::Otg),
            10 => Some(Self::Debug),
            11 => Some(Self::InterfaceAssociation),
            15 => Some(Self::Bos),
            16 => Some(Self::DeviceCapability),
            48 => Some(Self::SsEndpointCompanion),
            49 => Some(Self::SspIsocEndpointCompanion),
            33 => Some(Self::Hid),
            34 => Some(Self::HidReport),
            35 => Some(Self::HidPhysical),
            _ => None,
        }
    }
}

/// USB device descriptor (18 bytes)
#[derive(Debug, Clone)]
pub struct DeviceDescriptor {
    /// USB specification version (BCD)
    pub usb_version: u16,
    /// Device class
    pub device_class: u8,
    /// Device subclass
    pub device_subclass: u8,
    /// Device protocol
    pub device_protocol: u8,
    /// Maximum packet size for endpoint 0
    pub max_packet_size0: u8,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device version (BCD)
    pub device_version: u16,
    /// Manufacturer string index
    pub manufacturer_index: u8,
    /// Product string index
    pub product_index: u8,
    /// Serial number string index
    pub serial_index: u8,
    /// Number of configurations
    pub num_configurations: u8,
}

impl DeviceDescriptor {
    /// Create new device descriptor
    pub fn new(vendor_id: u16, product_id: u16) -> Self {
        Self {
            usb_version: 0x0200, // USB 2.0
            device_class: 0,
            device_subclass: 0,
            device_protocol: 0,
            max_packet_size0: 64,
            vendor_id,
            product_id,
            device_version: 0x0100,
            manufacturer_index: 0,
            product_index: 0,
            serial_index: 0,
            num_configurations: 1,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 18] {
        [
            18, // bLength
            DescriptorType::Device as u8,
            (self.usb_version & 0xFF) as u8,
            (self.usb_version >> 8) as u8,
            self.device_class,
            self.device_subclass,
            self.device_protocol,
            self.max_packet_size0,
            (self.vendor_id & 0xFF) as u8,
            (self.vendor_id >> 8) as u8,
            (self.product_id & 0xFF) as u8,
            (self.product_id >> 8) as u8,
            (self.device_version & 0xFF) as u8,
            (self.device_version >> 8) as u8,
            self.manufacturer_index,
            self.product_index,
            self.serial_index,
            self.num_configurations,
        ]
    }
}

/// Configuration descriptor (9 bytes header)
#[derive(Debug, Clone)]
pub struct ConfigDescriptor {
    /// Total length of all descriptors
    pub total_length: u16,
    /// Number of interfaces
    pub num_interfaces: u8,
    /// Configuration value
    pub config_value: u8,
    /// Configuration string index
    pub config_index: u8,
    /// Attributes (self-powered, remote wakeup)
    pub attributes: u8,
    /// Maximum power (2mA units)
    pub max_power: u8,
}

impl ConfigDescriptor {
    /// Create new configuration descriptor
    pub fn new(config_value: u8, num_interfaces: u8) -> Self {
        Self {
            total_length: 9,
            num_interfaces,
            config_value,
            config_index: 0,
            attributes: 0x80, // Bus powered
            max_power: 50,    // 100mA
        }
    }

    /// Self-powered attribute
    pub const ATTR_SELF_POWERED: u8 = 0x40;
    /// Remote wakeup attribute
    pub const ATTR_REMOTE_WAKEUP: u8 = 0x20;

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 9] {
        [
            9, // bLength
            DescriptorType::Configuration as u8,
            (self.total_length & 0xFF) as u8,
            (self.total_length >> 8) as u8,
            self.num_interfaces,
            self.config_value,
            self.config_index,
            self.attributes,
            self.max_power,
        ]
    }
}

/// Interface descriptor (9 bytes)
#[derive(Debug, Clone)]
pub struct InterfaceDescriptor {
    /// Interface number
    pub interface_number: u8,
    /// Alternate setting
    pub alternate_setting: u8,
    /// Number of endpoints
    pub num_endpoints: u8,
    /// Interface class
    pub interface_class: u8,
    /// Interface subclass
    pub interface_subclass: u8,
    /// Interface protocol
    pub interface_protocol: u8,
    /// Interface string index
    pub interface_index: u8,
}

impl InterfaceDescriptor {
    /// Create new interface descriptor
    pub fn new(interface_number: u8, class: u8, subclass: u8, protocol: u8) -> Self {
        Self {
            interface_number,
            alternate_setting: 0,
            num_endpoints: 0,
            interface_class: class,
            interface_subclass: subclass,
            interface_protocol: protocol,
            interface_index: 0,
        }
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 9] {
        [
            9, // bLength
            DescriptorType::Interface as u8,
            self.interface_number,
            self.alternate_setting,
            self.num_endpoints,
            self.interface_class,
            self.interface_subclass,
            self.interface_protocol,
            self.interface_index,
        ]
    }
}

/// Endpoint direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointDirection {
    /// OUT (host to device)
    Out = 0,
    /// IN (device to host)
    In = 1,
}

/// Endpoint transfer type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferType {
    /// Control transfer
    Control = 0,
    /// Isochronous transfer
    Isochronous = 1,
    /// Bulk transfer
    Bulk = 2,
    /// Interrupt transfer
    Interrupt = 3,
}

impl TransferType {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value & 0x03 {
            0 => Some(Self::Control),
            1 => Some(Self::Isochronous),
            2 => Some(Self::Bulk),
            3 => Some(Self::Interrupt),
            _ => None,
        }
    }
}

/// Endpoint descriptor (7 bytes)
#[derive(Debug, Clone)]
pub struct EndpointDescriptor {
    /// Endpoint address (number + direction)
    pub endpoint_address: u8,
    /// Attributes (transfer type, sync, usage)
    pub attributes: u8,
    /// Maximum packet size
    pub max_packet_size: u16,
    /// Polling interval (frames/microframes)
    pub interval: u8,
}

impl EndpointDescriptor {
    /// Create new endpoint descriptor
    pub fn new(number: u8, direction: EndpointDirection, transfer_type: TransferType) -> Self {
        Self {
            endpoint_address: number | ((direction as u8) << 7),
            attributes: transfer_type as u8,
            max_packet_size: 64,
            interval: 0,
        }
    }

    /// Get endpoint number
    pub fn number(&self) -> u8 {
        self.endpoint_address & 0x0F
    }

    /// Get endpoint direction
    pub fn direction(&self) -> EndpointDirection {
        if self.endpoint_address & 0x80 != 0 {
            EndpointDirection::In
        } else {
            EndpointDirection::Out
        }
    }

    /// Get transfer type
    pub fn transfer_type(&self) -> TransferType {
        TransferType::from_raw(self.attributes).unwrap_or(TransferType::Control)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 7] {
        [
            7, // bLength
            DescriptorType::Endpoint as u8,
            self.endpoint_address,
            self.attributes,
            (self.max_packet_size & 0xFF) as u8,
            (self.max_packet_size >> 8) as u8,
            self.interval,
        ]
    }
}

/// String descriptor
#[derive(Debug, Clone)]
pub struct StringDescriptor {
    /// String content (UTF-16LE for non-zero index)
    pub data: Vec<u8>,
}

impl StringDescriptor {
    /// Create language ID descriptor (index 0)
    pub fn language_ids(lang_ids: &[u16]) -> Self {
        let mut data = vec![2 + (lang_ids.len() * 2) as u8, DescriptorType::String as u8];
        for &id in lang_ids {
            data.push((id & 0xFF) as u8);
            data.push((id >> 8) as u8);
        }
        Self { data }
    }

    /// Create string descriptor from UTF-8 string
    pub fn from_str(s: &str) -> Self {
        let utf16: Vec<u16> = s.encode_utf16().collect();
        let len = 2 + (utf16.len() * 2);
        let mut data = vec![len as u8, DescriptorType::String as u8];
        for ch in utf16 {
            data.push((ch & 0xFF) as u8);
            data.push((ch >> 8) as u8);
        }
        Self { data }
    }

    /// Get descriptor bytes
    pub fn to_bytes(&self) -> &[u8] {
        &self.data
    }
}

/// USB setup packet (8 bytes)
#[derive(Debug, Clone, Copy)]
pub struct SetupPacket {
    /// Request type
    pub request_type: u8,
    /// Request
    pub request: u8,
    /// Value
    pub value: u16,
    /// Index
    pub index: u16,
    /// Length
    pub length: u16,
}

impl SetupPacket {
    /// Create from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(Self {
            request_type: bytes[0],
            request: bytes[1],
            value: u16::from_le_bytes([bytes[2], bytes[3]]),
            index: u16::from_le_bytes([bytes[4], bytes[5]]),
            length: u16::from_le_bytes([bytes[6], bytes[7]]),
        })
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; 8] {
        [
            self.request_type,
            self.request,
            (self.value & 0xFF) as u8,
            (self.value >> 8) as u8,
            (self.index & 0xFF) as u8,
            (self.index >> 8) as u8,
            (self.length & 0xFF) as u8,
            (self.length >> 8) as u8,
        ]
    }

    /// Check if host-to-device
    pub fn is_host_to_device(&self) -> bool {
        self.request_type & 0x80 == 0
    }

    /// Check if device-to-host
    pub fn is_device_to_host(&self) -> bool {
        self.request_type & 0x80 != 0
    }

    /// Get request type (standard, class, vendor)
    pub fn request_type_type(&self) -> u8 {
        (self.request_type >> 5) & 0x03
    }

    /// Get recipient (device, interface, endpoint, other)
    pub fn recipient(&self) -> u8 {
        self.request_type & 0x1F
    }
}

/// Standard USB requests
pub mod request {
    pub const GET_STATUS: u8 = 0;
    pub const CLEAR_FEATURE: u8 = 1;
    pub const SET_FEATURE: u8 = 3;
    pub const SET_ADDRESS: u8 = 5;
    pub const GET_DESCRIPTOR: u8 = 6;
    pub const SET_DESCRIPTOR: u8 = 7;
    pub const GET_CONFIGURATION: u8 = 8;
    pub const SET_CONFIGURATION: u8 = 9;
    pub const GET_INTERFACE: u8 = 10;
    pub const SET_INTERFACE: u8 = 11;
    pub const SYNCH_FRAME: u8 = 12;
    pub const SET_SEL: u8 = 48;
    pub const SET_ISOCH_DELAY: u8 = 49;
}

/// USB endpoint
#[derive(Debug)]
pub struct Endpoint {
    /// Endpoint descriptor
    pub descriptor: EndpointDescriptor,
    /// Data buffer
    pub buffer: Vec<u8>,
    /// Stalled flag
    pub stalled: bool,
    /// Data toggle
    pub data_toggle: bool,
}

impl Endpoint {
    /// Create new endpoint
    pub fn new(descriptor: EndpointDescriptor) -> Self {
        Self {
            descriptor,
            buffer: Vec::new(),
            stalled: false,
            data_toggle: false,
        }
    }

    /// Queue data for IN endpoint
    pub fn queue_data(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);
    }

    /// Get data from buffer
    pub fn get_data(&mut self, max_len: usize) -> Vec<u8> {
        let len = max_len.min(self.buffer.len());
        self.buffer.drain(..len).collect()
    }

    /// Check if buffer has data
    pub fn has_data(&self) -> bool {
        !self.buffer.is_empty()
    }

    /// Clear buffer
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.stalled = false;
    }

    /// Stall endpoint
    pub fn stall(&mut self) {
        self.stalled = true;
    }

    /// Clear stall
    pub fn clear_stall(&mut self) {
        self.stalled = false;
        self.data_toggle = false;
    }
}

/// USB device statistics
#[derive(Debug, Default)]
pub struct DeviceStats {
    /// Control transfers
    pub control_transfers: AtomicU64,
    /// Bulk transfers IN
    pub bulk_in: AtomicU64,
    /// Bulk transfers OUT
    pub bulk_out: AtomicU64,
    /// Interrupt transfers IN
    pub interrupt_in: AtomicU64,
    /// Interrupt transfers OUT
    pub interrupt_out: AtomicU64,
    /// Bytes received
    pub bytes_received: AtomicU64,
    /// Bytes sent
    pub bytes_sent: AtomicU64,
}

/// USB device trait
pub trait UsbDevice: Send + Sync {
    /// Get device descriptor
    fn device_descriptor(&self) -> &DeviceDescriptor;

    /// Get configuration descriptor (with all nested descriptors)
    fn configuration_descriptor(&self, index: u8) -> Option<Vec<u8>>;

    /// Get string descriptor
    fn string_descriptor(&self, index: u8, lang_id: u16) -> Option<Vec<u8>>;

    /// Handle control transfer
    fn control_transfer(&mut self, setup: &SetupPacket, data: &[u8]) -> ControlResult;

    /// Handle data IN (device to host)
    fn data_in(&mut self, endpoint: u8, max_length: u16) -> TransferResult;

    /// Handle data OUT (host to device)
    fn data_out(&mut self, endpoint: u8, data: &[u8]) -> TransferResult;

    /// Reset device
    fn reset(&mut self);

    /// Set address
    fn set_address(&mut self, address: u8);

    /// Get current address
    fn address(&self) -> u8;

    /// Set configuration
    fn set_configuration(&mut self, config: u8) -> bool;

    /// Get current configuration
    fn configuration(&self) -> u8;
}

/// Control transfer result
#[derive(Debug, Clone)]
pub enum ControlResult {
    /// Success with optional data
    Ok(Vec<u8>),
    /// Stall (request not supported)
    Stall,
    /// NAK (not ready)
    Nak,
}

/// Transfer result
#[derive(Debug, Clone)]
pub enum TransferResult {
    /// Success with data/length
    Ok(Vec<u8>),
    /// Stall
    Stall,
    /// NAK (no data available / not ready)
    Nak,
    /// Short packet
    Short(Vec<u8>),
}

/// Base USB device implementation
#[derive(Debug)]
pub struct BaseUsbDevice {
    /// Device descriptor
    pub device_desc: DeviceDescriptor,
    /// Configuration descriptors
    pub config_descs: Vec<Vec<u8>>,
    /// String descriptors
    pub string_descs: HashMap<u8, StringDescriptor>,
    /// Device state
    pub state: DeviceState,
    /// Device address
    pub address: u8,
    /// Current configuration
    pub config: u8,
    /// Endpoints (by address)
    pub endpoints: HashMap<u8, Endpoint>,
    /// Statistics
    pub stats: DeviceStats,
}

impl BaseUsbDevice {
    /// Create new base device
    pub fn new(vendor_id: u16, product_id: u16) -> Self {
        let device_desc = DeviceDescriptor::new(vendor_id, product_id);

        // Default language string
        let mut string_descs = HashMap::new();
        string_descs.insert(0, StringDescriptor::language_ids(&[0x0409])); // English US

        Self {
            device_desc,
            config_descs: Vec::new(),
            string_descs,
            state: DeviceState::Detached,
            address: 0,
            config: 0,
            endpoints: HashMap::new(),
            stats: DeviceStats::default(),
        }
    }

    /// Add string descriptor
    pub fn add_string(&mut self, index: u8, s: &str) {
        self.string_descs
            .insert(index, StringDescriptor::from_str(s));
    }

    /// Set manufacturer string
    pub fn set_manufacturer(&mut self, s: &str) {
        self.device_desc.manufacturer_index = 1;
        self.add_string(1, s);
    }

    /// Set product string
    pub fn set_product(&mut self, s: &str) {
        self.device_desc.product_index = 2;
        self.add_string(2, s);
    }

    /// Set serial number string
    pub fn set_serial(&mut self, s: &str) {
        self.device_desc.serial_index = 3;
        self.add_string(3, s);
    }

    /// Add endpoint
    pub fn add_endpoint(&mut self, endpoint: Endpoint) {
        self.endpoints
            .insert(endpoint.descriptor.endpoint_address, endpoint);
    }

    /// Handle standard device request
    pub fn handle_standard_request(&mut self, setup: &SetupPacket) -> ControlResult {
        match setup.request {
            request::GET_DESCRIPTOR => self.get_descriptor(setup),
            request::SET_ADDRESS => {
                self.address = (setup.value & 0x7F) as u8;
                self.state = DeviceState::Address;
                ControlResult::Ok(Vec::new())
            }
            request::SET_CONFIGURATION => {
                self.config = (setup.value & 0xFF) as u8;
                if self.config != 0 {
                    self.state = DeviceState::Configured;
                } else {
                    self.state = DeviceState::Address;
                }
                ControlResult::Ok(Vec::new())
            }
            request::GET_CONFIGURATION => ControlResult::Ok(vec![self.config]),
            request::GET_STATUS => {
                ControlResult::Ok(vec![0, 0]) // Not self-powered, no remote wakeup
            }
            _ => ControlResult::Stall,
        }
    }

    /// Handle GET_DESCRIPTOR request
    fn get_descriptor(&self, setup: &SetupPacket) -> ControlResult {
        let desc_type = (setup.value >> 8) as u8;
        let desc_index = (setup.value & 0xFF) as u8;
        let length = setup.length as usize;

        match DescriptorType::from_raw(desc_type) {
            Some(DescriptorType::Device) => {
                let data = self.device_desc.to_bytes();
                ControlResult::Ok(data[..length.min(data.len())].to_vec())
            }
            Some(DescriptorType::Configuration) => {
                if let Some(config) = self.config_descs.get(desc_index as usize) {
                    ControlResult::Ok(config[..length.min(config.len())].to_vec())
                } else {
                    ControlResult::Stall
                }
            }
            Some(DescriptorType::String) => {
                if let Some(string_desc) = self.string_descs.get(&desc_index) {
                    let data = string_desc.to_bytes();
                    ControlResult::Ok(data[..length.min(data.len())].to_vec())
                } else {
                    ControlResult::Stall
                }
            }
            _ => ControlResult::Stall,
        }
    }

    /// Attach device
    pub fn attach(&mut self) {
        self.state = DeviceState::Attached;
    }

    /// Detach device
    pub fn detach(&mut self) {
        self.state = DeviceState::Detached;
        self.address = 0;
        self.config = 0;
    }

    /// Power device
    pub fn power_on(&mut self) {
        if self.state == DeviceState::Attached {
            self.state = DeviceState::Powered;
        }
    }

    /// Reset device
    pub fn reset(&mut self) {
        self.state = DeviceState::Default;
        self.address = 0;
        self.config = 0;
        for endpoint in self.endpoints.values_mut() {
            endpoint.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_state() {
        assert_eq!(DeviceState::default(), DeviceState::Detached);
    }

    #[test]
    fn test_device_class() {
        assert_eq!(DeviceClass::from_raw(0x03), DeviceClass::Hid);
        assert_eq!(DeviceClass::from_raw(0x08), DeviceClass::MassStorage);
        assert_eq!(DeviceClass::from_raw(0x09), DeviceClass::Hub);
    }

    #[test]
    fn test_descriptor_type() {
        assert_eq!(DescriptorType::from_raw(1), Some(DescriptorType::Device));
        assert_eq!(
            DescriptorType::from_raw(2),
            Some(DescriptorType::Configuration)
        );
        assert_eq!(DescriptorType::from_raw(5), Some(DescriptorType::Endpoint));
        assert_eq!(DescriptorType::from_raw(33), Some(DescriptorType::Hid));
    }

    #[test]
    fn test_device_descriptor() {
        let desc = DeviceDescriptor::new(0x1234, 0x5678);
        assert_eq!(desc.vendor_id, 0x1234);
        assert_eq!(desc.product_id, 0x5678);
        assert_eq!(desc.usb_version, 0x0200);

        let bytes = desc.to_bytes();
        assert_eq!(bytes.len(), 18);
        assert_eq!(bytes[0], 18); // bLength
        assert_eq!(bytes[1], 1); // bDescriptorType
    }

    #[test]
    fn test_config_descriptor() {
        let desc = ConfigDescriptor::new(1, 2);
        assert_eq!(desc.config_value, 1);
        assert_eq!(desc.num_interfaces, 2);

        let bytes = desc.to_bytes();
        assert_eq!(bytes.len(), 9);
        assert_eq!(bytes[0], 9); // bLength
        assert_eq!(bytes[1], 2); // bDescriptorType
    }

    #[test]
    fn test_interface_descriptor() {
        let desc = InterfaceDescriptor::new(0, DeviceClass::Hid as u8, 1, 2);
        assert_eq!(desc.interface_number, 0);
        assert_eq!(desc.interface_class, 3);

        let bytes = desc.to_bytes();
        assert_eq!(bytes.len(), 9);
        assert_eq!(bytes[1], 4); // bDescriptorType
    }

    #[test]
    fn test_endpoint_descriptor() {
        let desc = EndpointDescriptor::new(1, EndpointDirection::In, TransferType::Interrupt);
        assert_eq!(desc.number(), 1);
        assert_eq!(desc.direction(), EndpointDirection::In);
        assert_eq!(desc.transfer_type(), TransferType::Interrupt);

        let bytes = desc.to_bytes();
        assert_eq!(bytes.len(), 7);
        assert_eq!(bytes[1], 5); // bDescriptorType
        assert_eq!(bytes[2], 0x81); // IN endpoint 1
    }

    #[test]
    fn test_string_descriptor() {
        let lang = StringDescriptor::language_ids(&[0x0409]);
        let data = lang.to_bytes();
        assert_eq!(data[0], 4); // bLength
        assert_eq!(data[1], 3); // bDescriptorType

        let string = StringDescriptor::from_str("Test");
        let data = string.to_bytes();
        assert_eq!(data[1], 3); // bDescriptorType
    }

    #[test]
    fn test_setup_packet() {
        let bytes = [0x80, 0x06, 0x00, 0x01, 0x00, 0x00, 0x12, 0x00];
        let setup = SetupPacket::from_bytes(&bytes).unwrap();

        assert_eq!(setup.request_type, 0x80);
        assert_eq!(setup.request, 0x06); // GET_DESCRIPTOR
        assert_eq!(setup.value, 0x0100); // Device descriptor
        assert_eq!(setup.length, 18);
        assert!(setup.is_device_to_host());
    }

    #[test]
    fn test_setup_packet_roundtrip() {
        let setup = SetupPacket {
            request_type: 0x21,
            request: 0x09,
            value: 0x0200,
            index: 0,
            length: 8,
        };

        let bytes = setup.to_bytes();
        let restored = SetupPacket::from_bytes(&bytes).unwrap();

        assert_eq!(setup.request_type, restored.request_type);
        assert_eq!(setup.request, restored.request);
        assert_eq!(setup.value, restored.value);
    }

    #[test]
    fn test_endpoint() {
        let desc = EndpointDescriptor::new(1, EndpointDirection::In, TransferType::Bulk);
        let mut ep = Endpoint::new(desc);

        assert!(!ep.has_data());

        ep.queue_data(&[1, 2, 3, 4]);
        assert!(ep.has_data());

        let data = ep.get_data(2);
        assert_eq!(data, vec![1, 2]);
        assert!(ep.has_data());

        ep.clear();
        assert!(!ep.has_data());
    }

    #[test]
    fn test_endpoint_stall() {
        let desc = EndpointDescriptor::new(2, EndpointDirection::Out, TransferType::Bulk);
        let mut ep = Endpoint::new(desc);

        assert!(!ep.stalled);

        ep.stall();
        assert!(ep.stalled);

        ep.clear_stall();
        assert!(!ep.stalled);
    }

    #[test]
    fn test_base_usb_device() {
        let mut device = BaseUsbDevice::new(0x1234, 0x5678);

        assert_eq!(device.state, DeviceState::Detached);
        assert_eq!(device.address, 0);

        device.set_manufacturer("Test Vendor");
        device.set_product("Test Product");

        assert_eq!(device.device_desc.manufacturer_index, 1);
        assert!(device.string_descs.contains_key(&1));
    }

    #[test]
    fn test_base_device_state_transitions() {
        let mut device = BaseUsbDevice::new(0x1234, 0x5678);

        device.attach();
        assert_eq!(device.state, DeviceState::Attached);

        device.power_on();
        assert_eq!(device.state, DeviceState::Powered);

        device.reset();
        assert_eq!(device.state, DeviceState::Default);

        device.detach();
        assert_eq!(device.state, DeviceState::Detached);
    }

    #[test]
    fn test_base_device_get_descriptor() {
        let device = BaseUsbDevice::new(0x1234, 0x5678);

        // GET_DESCRIPTOR for device descriptor
        let setup = SetupPacket {
            request_type: 0x80,
            request: request::GET_DESCRIPTOR,
            value: 0x0100, // Device, index 0
            index: 0,
            length: 18,
        };

        match device.get_descriptor(&setup) {
            ControlResult::Ok(data) => {
                assert_eq!(data.len(), 18);
                assert_eq!(data[0], 18); // bLength
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_transfer_type() {
        assert_eq!(TransferType::from_raw(0), Some(TransferType::Control));
        assert_eq!(TransferType::from_raw(1), Some(TransferType::Isochronous));
        assert_eq!(TransferType::from_raw(2), Some(TransferType::Bulk));
        assert_eq!(TransferType::from_raw(3), Some(TransferType::Interrupt));
    }

    #[test]
    fn test_standard_requests() {
        let mut device = BaseUsbDevice::new(0x1234, 0x5678);

        // SET_ADDRESS
        let setup = SetupPacket {
            request_type: 0x00,
            request: request::SET_ADDRESS,
            value: 5,
            index: 0,
            length: 0,
        };

        match device.handle_standard_request(&setup) {
            ControlResult::Ok(_) => {
                assert_eq!(device.address, 5);
                assert_eq!(device.state, DeviceState::Address);
            }
            _ => panic!("Expected Ok"),
        }
    }

    #[test]
    fn test_set_configuration() {
        let mut device = BaseUsbDevice::new(0x1234, 0x5678);
        device.state = DeviceState::Address;

        let setup = SetupPacket {
            request_type: 0x00,
            request: request::SET_CONFIGURATION,
            value: 1,
            index: 0,
            length: 0,
        };

        match device.handle_standard_request(&setup) {
            ControlResult::Ok(_) => {
                assert_eq!(device.config, 1);
                assert_eq!(device.state, DeviceState::Configured);
            }
            _ => panic!("Expected Ok"),
        }
    }
}
