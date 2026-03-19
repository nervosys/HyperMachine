//! xHCI USB Host Controller
//!
//! This module implements the eXtensible Host Controller Interface (xHCI)
//! for USB 3.0/3.1/3.2 host controller emulation.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};

/// xHCI capability registers offset
pub const XHCI_CAP_LENGTH: u32 = 0x20;
/// xHCI operational registers offset
pub const XHCI_OP_OFFSET: u32 = XHCI_CAP_LENGTH;
/// xHCI runtime registers offset
pub const XHCI_RUNTIME_OFFSET: u32 = 0x600;
/// xHCI doorbell registers offset
pub const XHCI_DOORBELL_OFFSET: u32 = 0x800;

/// Maximum number of ports
pub const MAX_PORTS: usize = 16;
/// Maximum number of slots
pub const MAX_SLOTS: usize = 64;
/// Maximum number of interrupters
pub const MAX_INTERRUPTERS: usize = 8;

/// USB speed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    /// Full speed (12 Mbps)
    Full = 1,
    /// Low speed (1.5 Mbps)
    Low = 2,
    /// High speed (480 Mbps)
    High = 3,
    /// Super speed (5 Gbps)
    Super = 4,
    /// Super speed plus (10 Gbps)
    SuperPlus = 5,
}

impl UsbSpeed {
    /// Get speed from protocol speed ID
    pub fn from_psid(psid: u8) -> Option<Self> {
        match psid {
            1 => Some(Self::Full),
            2 => Some(Self::Low),
            3 => Some(Self::High),
            4 => Some(Self::Super),
            5 => Some(Self::SuperPlus),
            _ => None,
        }
    }

    /// Get maximum packet size for control endpoint
    pub fn max_packet_size(&self) -> u16 {
        match self {
            Self::Low => 8,
            Self::Full => 64,
            Self::High => 64,
            Self::Super | Self::SuperPlus => 512,
        }
    }
}

/// Port state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PortState {
    /// Port is disconnected
    #[default]
    Disconnected,
    /// Port is disabled
    Disabled,
    /// Port is in reset
    Resetting,
    /// Port is enabled
    Enabled,
    /// Port is suspended
    Suspended,
}

/// Port register set
#[derive(Debug, Clone)]
pub struct PortRegister {
    /// Port status and control
    pub portsc: u32,
    /// Port power management status and control
    pub portpmsc: u32,
    /// Port link info
    pub portli: u32,
    /// Port hardware LPM control
    pub porthlpmc: u32,
}

impl Default for PortRegister {
    fn default() -> Self {
        Self {
            portsc: 0x0002_0000, // Port power on
            portpmsc: 0,
            portli: 0,
            porthlpmc: 0,
        }
    }
}

/// Port status/control register bits
pub mod portsc {
    /// Current connect status
    pub const CCS: u32 = 1 << 0;
    /// Port enabled/disabled
    pub const PED: u32 = 1 << 1;
    /// Over-current active
    pub const OCA: u32 = 1 << 3;
    /// Port reset
    pub const PR: u32 = 1 << 4;
    /// Port link state (bits 8:5)
    pub const PLS_MASK: u32 = 0xF << 5;
    /// Port power
    pub const PP: u32 = 1 << 9;
    /// Port speed (bits 13:10)
    pub const SPEED_MASK: u32 = 0xF << 10;
    /// Port indicator control (bits 15:14)
    pub const PIC_MASK: u32 = 0x3 << 14;
    /// Port link state write strobe
    pub const LWS: u32 = 1 << 16;
    /// Connect status change
    pub const CSC: u32 = 1 << 17;
    /// Port enabled/disabled change
    pub const PEC: u32 = 1 << 18;
    /// Warm port reset change
    pub const WRC: u32 = 1 << 19;
    /// Over-current change
    pub const OCC: u32 = 1 << 20;
    /// Port reset change
    pub const PRC: u32 = 1 << 21;
    /// Port link state change
    pub const PLC: u32 = 1 << 22;
    /// Port config error change
    pub const CEC: u32 = 1 << 23;
    /// Cold attach status
    pub const CAS: u32 = 1 << 24;
    /// Wake on connect enable
    pub const WCE: u32 = 1 << 25;
    /// Wake on disconnect enable
    pub const WDE: u32 = 1 << 26;
    /// Wake on over-current enable
    pub const WOE: u32 = 1 << 27;
    /// Device removable
    pub const DR: u32 = 1 << 30;
    /// Warm port reset
    pub const WPR: u32 = 1 << 31;
}

/// xHCI port
#[derive(Debug)]
pub struct XhciPort {
    /// Port number (1-based)
    pub number: u8,
    /// Port registers
    pub regs: PortRegister,
    /// Port state
    pub state: PortState,
    /// Connected device slot ID (0 = none)
    pub slot_id: u8,
    /// Port speed
    pub speed: Option<UsbSpeed>,
    /// USB 3.0 port (vs USB 2.0)
    pub usb3: bool,
}

impl XhciPort {
    /// Create new port
    pub fn new(number: u8, usb3: bool) -> Self {
        Self {
            number,
            regs: PortRegister::default(),
            state: PortState::Disconnected,
            slot_id: 0,
            speed: None,
            usb3,
        }
    }

    /// Check if device is connected
    pub fn is_connected(&self) -> bool {
        self.regs.portsc & portsc::CCS != 0
    }

    /// Check if port is enabled
    pub fn is_enabled(&self) -> bool {
        self.regs.portsc & portsc::PED != 0
    }

    /// Connect device at speed
    pub fn connect(&mut self, speed: UsbSpeed) {
        self.speed = Some(speed);
        self.state = PortState::Disabled;

        // Set CCS and speed
        self.regs.portsc |= portsc::CCS;
        self.regs.portsc = (self.regs.portsc & !portsc::SPEED_MASK) | ((speed as u32) << 10);
        // Set connect status change
        self.regs.portsc |= portsc::CSC;
    }

    /// Disconnect device
    pub fn disconnect(&mut self) {
        self.speed = None;
        self.state = PortState::Disconnected;
        self.slot_id = 0;

        // Clear CCS, set CSC
        self.regs.portsc &= !portsc::CCS;
        self.regs.portsc &= !portsc::PED;
        self.regs.portsc |= portsc::CSC;
    }

    /// Reset port
    pub fn reset(&mut self) {
        if self.is_connected() {
            self.state = PortState::Resetting;
            // After reset completes, enable the port
            self.state = PortState::Enabled;
            self.regs.portsc |= portsc::PED;
            self.regs.portsc &= !portsc::PR;
            self.regs.portsc |= portsc::PRC; // Reset change
        }
    }

    /// Get port speed
    pub fn get_speed(&self) -> Option<UsbSpeed> {
        self.speed
    }
}

/// TRB (Transfer Request Block) type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbType {
    /// Normal transfer
    Normal = 1,
    /// Setup stage
    Setup = 2,
    /// Data stage
    Data = 3,
    /// Status stage
    Status = 4,
    /// Isoch transfer
    Isoch = 5,
    /// Link TRB
    Link = 6,
    /// Event data
    EventData = 7,
    /// No-op transfer
    NoOpTransfer = 8,
    /// Enable slot command
    EnableSlot = 9,
    /// Disable slot command
    DisableSlot = 10,
    /// Address device command
    AddressDevice = 11,
    /// Configure endpoint command
    ConfigureEndpoint = 12,
    /// Evaluate context command
    EvaluateContext = 13,
    /// Reset endpoint command
    ResetEndpoint = 14,
    /// Stop endpoint command
    StopEndpoint = 15,
    /// Set TR dequeue pointer command
    SetTrDequeue = 16,
    /// Reset device command
    ResetDevice = 17,
    /// Force event command
    ForceEvent = 18,
    /// Negotiate bandwidth command
    NegotiateBandwidth = 19,
    /// Set latency tolerance command
    SetLatencyTolerance = 20,
    /// Get port bandwidth command
    GetPortBandwidth = 21,
    /// Force header command
    ForceHeader = 22,
    /// No-op command
    NoOpCommand = 23,
    /// Transfer event
    TransferEvent = 32,
    /// Command completion event
    CommandCompletion = 33,
    /// Port status change event
    PortStatusChange = 34,
    /// Bandwidth request event
    BandwidthRequest = 35,
    /// Doorbell event
    DoorbellEvent = 36,
    /// Host controller event
    HostControllerEvent = 37,
    /// Device notification event
    DeviceNotification = 38,
    /// MFINDEX wrap event
    MfindexWrap = 39,
}

impl TrbType {
    /// Create from raw value
    pub fn from_raw(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Normal),
            2 => Some(Self::Setup),
            3 => Some(Self::Data),
            4 => Some(Self::Status),
            5 => Some(Self::Isoch),
            6 => Some(Self::Link),
            7 => Some(Self::EventData),
            8 => Some(Self::NoOpTransfer),
            9 => Some(Self::EnableSlot),
            10 => Some(Self::DisableSlot),
            11 => Some(Self::AddressDevice),
            12 => Some(Self::ConfigureEndpoint),
            13 => Some(Self::EvaluateContext),
            14 => Some(Self::ResetEndpoint),
            15 => Some(Self::StopEndpoint),
            16 => Some(Self::SetTrDequeue),
            17 => Some(Self::ResetDevice),
            18 => Some(Self::ForceEvent),
            19 => Some(Self::NegotiateBandwidth),
            20 => Some(Self::SetLatencyTolerance),
            21 => Some(Self::GetPortBandwidth),
            22 => Some(Self::ForceHeader),
            23 => Some(Self::NoOpCommand),
            32 => Some(Self::TransferEvent),
            33 => Some(Self::CommandCompletion),
            34 => Some(Self::PortStatusChange),
            35 => Some(Self::BandwidthRequest),
            36 => Some(Self::DoorbellEvent),
            37 => Some(Self::HostControllerEvent),
            38 => Some(Self::DeviceNotification),
            39 => Some(Self::MfindexWrap),
            _ => None,
        }
    }
}

/// TRB completion code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrbCompletionCode {
    /// Invalid (not used)
    Invalid = 0,
    /// Success
    Success = 1,
    /// Data buffer error
    DataBufferError = 2,
    /// Babble detected
    BabbleDetected = 3,
    /// USB transaction error
    UsbTransactionError = 4,
    /// TRB error
    TrbError = 5,
    /// Stall error
    StallError = 6,
    /// Resource error
    ResourceError = 7,
    /// Bandwidth error
    BandwidthError = 8,
    /// No slots available
    NoSlotsAvailable = 9,
    /// Invalid stream type
    InvalidStreamType = 10,
    /// Slot not enabled
    SlotNotEnabled = 11,
    /// Endpoint not enabled
    EndpointNotEnabled = 12,
    /// Short packet
    ShortPacket = 13,
    /// Ring underrun
    RingUnderrun = 14,
    /// Ring overrun
    RingOverrun = 15,
    /// VF event ring full
    VfEventRingFull = 16,
    /// Parameter error
    ParameterError = 17,
    /// Bandwidth overrun
    BandwidthOverrun = 18,
    /// Context state error
    ContextStateError = 19,
    /// No ping response
    NoPingResponse = 20,
    /// Event ring full
    EventRingFull = 21,
    /// Incompatible device
    IncompatibleDevice = 22,
    /// Missed service
    MissedService = 23,
    /// Command ring stopped
    CommandRingStopped = 24,
    /// Command aborted
    CommandAborted = 25,
    /// Stopped
    Stopped = 26,
    /// Stopped - length invalid
    StoppedLengthInvalid = 27,
    /// Stopped - short packet
    StoppedShortPacket = 28,
    /// Max exit latency too large
    MaxExitLatencyTooLarge = 29,
    /// Isoch buffer overrun
    IsochBufferOverrun = 31,
    /// Event lost
    EventLost = 32,
    /// Undefined error
    UndefinedError = 33,
    /// Invalid stream ID
    InvalidStreamId = 34,
    /// Secondary bandwidth error
    SecondaryBandwidthError = 35,
    /// Split transaction error
    SplitTransactionError = 36,
}

/// Transfer Request Block (16 bytes)
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct Trb {
    /// Parameter (64-bit)
    pub parameter: u64,
    /// Status (32-bit)
    pub status: u32,
    /// Control (32-bit)
    pub control: u32,
}

impl Trb {
    /// Create new TRB
    pub fn new(trb_type: TrbType) -> Self {
        Self {
            parameter: 0,
            status: 0,
            control: (trb_type as u32) << 10,
        }
    }

    /// Get TRB type
    pub fn trb_type(&self) -> Option<TrbType> {
        TrbType::from_raw(((self.control >> 10) & 0x3F) as u8)
    }

    /// Get cycle bit
    pub fn cycle(&self) -> bool {
        self.control & 1 != 0
    }

    /// Set cycle bit
    pub fn set_cycle(&mut self, cycle: bool) {
        if cycle {
            self.control |= 1;
        } else {
            self.control &= !1;
        }
    }

    /// Get chain bit
    pub fn chain(&self) -> bool {
        self.control & (1 << 4) != 0
    }

    /// Get IOC (interrupt on completion)
    pub fn ioc(&self) -> bool {
        self.control & (1 << 5) != 0
    }

    /// Create command completion event TRB
    pub fn command_completion(command_ptr: u64, code: TrbCompletionCode, slot_id: u8) -> Self {
        Self {
            parameter: command_ptr,
            status: ((code as u32) << 24),
            control: ((TrbType::CommandCompletion as u32) << 10) | ((slot_id as u32) << 24),
        }
    }

    /// Create port status change event TRB
    pub fn port_status_change(port_id: u8) -> Self {
        Self {
            parameter: (port_id as u64) << 24,
            status: (TrbCompletionCode::Success as u32) << 24,
            control: (TrbType::PortStatusChange as u32) << 10,
        }
    }

    /// Create transfer event TRB
    pub fn transfer_event(
        trb_ptr: u64,
        code: TrbCompletionCode,
        length: u32,
        slot_id: u8,
        endpoint_id: u8,
    ) -> Self {
        Self {
            parameter: trb_ptr,
            status: (length & 0xFFFFFF) | ((code as u32) << 24),
            control: ((TrbType::TransferEvent as u32) << 10)
                | ((endpoint_id as u32) << 16)
                | ((slot_id as u32) << 24),
        }
    }
}

/// Ring segment
#[derive(Debug)]
pub struct RingSegment {
    /// Base address
    pub base: u64,
    /// Size in TRBs
    pub size: u32,
    /// Current index
    pub index: u32,
    /// Producer cycle state
    pub cycle: bool,
}

impl RingSegment {
    /// Create new ring segment
    pub fn new(base: u64, size: u32) -> Self {
        Self {
            base,
            size,
            index: 0,
            cycle: true,
        }
    }

    /// Get current TRB address
    pub fn current_addr(&self) -> u64 {
        self.base + (self.index as u64) * 16
    }

    /// Advance to next TRB
    pub fn advance(&mut self) {
        self.index += 1;
        if self.index >= self.size {
            self.index = 0;
            self.cycle = !self.cycle;
        }
    }
}

/// Command ring
#[derive(Debug)]
pub struct CommandRing {
    /// Ring segment
    pub segment: RingSegment,
    /// Running flag
    pub running: bool,
    /// Command ring control
    pub crcr: u64,
}

impl CommandRing {
    /// Create new command ring
    pub fn new() -> Self {
        Self {
            segment: RingSegment::new(0, 256),
            running: false,
            crcr: 0,
        }
    }

    /// Set command ring pointer
    pub fn set_pointer(&mut self, ptr: u64) {
        self.crcr = ptr;
        self.segment.base = ptr & !0x3F;
        self.segment.cycle = (ptr & 1) != 0;
        self.segment.index = 0;
    }
}

impl Default for CommandRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Event ring segment table entry
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct ErstEntry {
    /// Ring segment base address
    pub base: u64,
    /// Ring segment size
    pub size: u32,
    /// Reserved
    pub reserved: u32,
}

/// Event ring
#[derive(Debug)]
pub struct EventRing {
    /// Segment table base address
    pub erst_base: u64,
    /// Segment table size
    pub erst_size: u32,
    /// Current segment
    pub segment: RingSegment,
    /// Dequeue pointer
    pub dequeue: u64,
    /// Consumer cycle state
    pub ccs: bool,
    /// Pending events
    pub pending: VecDeque<Trb>,
}

impl EventRing {
    /// Create new event ring
    pub fn new() -> Self {
        Self {
            erst_base: 0,
            erst_size: 0,
            segment: RingSegment::new(0, 256),
            dequeue: 0,
            ccs: true,
            pending: VecDeque::new(),
        }
    }

    /// Queue event
    pub fn queue_event(&mut self, mut event: Trb) {
        event.set_cycle(self.segment.cycle);
        self.pending.push_back(event);
    }

    /// Check if events pending
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Get next pending event
    pub fn pop_event(&mut self) -> Option<Trb> {
        self.pending.pop_front()
    }
}

impl Default for EventRing {
    fn default() -> Self {
        Self::new()
    }
}

/// Interrupter
#[derive(Debug, Default)]
pub struct Interrupter {
    /// Interrupter management
    pub iman: u32,
    /// Interrupter moderation
    pub imod: u32,
    /// Event ring
    pub event_ring: EventRing,
    /// Interrupt pending
    pub pending: bool,
}

impl Interrupter {
    /// Create new interrupter
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if interrupt enabled
    pub fn is_enabled(&self) -> bool {
        self.iman & (1 << 1) != 0
    }

    /// Set interrupt pending
    pub fn set_pending(&mut self) {
        self.iman |= 1;
        self.pending = true;
    }

    /// Clear interrupt
    pub fn clear_interrupt(&mut self) {
        self.iman &= !1;
        self.pending = false;
    }
}

/// Slot state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SlotState {
    /// Slot is disabled
    #[default]
    Disabled,
    /// Slot is enabled
    Enabled,
    /// Slot is addressed (default state)
    Default,
    /// Slot is addressed
    Addressed,
    /// Slot is configured
    Configured,
}

/// Device slot
#[derive(Debug, Default)]
pub struct DeviceSlot {
    /// Slot ID
    pub id: u8,
    /// Slot state
    pub state: SlotState,
    /// Device context base address
    pub context_addr: u64,
    /// Port number
    pub port: u8,
    /// Device speed
    pub speed: Option<UsbSpeed>,
    /// Enabled endpoints (bitmap)
    pub enabled_endpoints: u32,
}

impl DeviceSlot {
    /// Create new slot
    pub fn new(id: u8) -> Self {
        Self {
            id,
            state: SlotState::Disabled,
            context_addr: 0,
            port: 0,
            speed: None,
            enabled_endpoints: 0,
        }
    }

    /// Enable slot
    pub fn enable(&mut self) {
        self.state = SlotState::Enabled;
    }

    /// Disable slot
    pub fn disable(&mut self) {
        self.state = SlotState::Disabled;
        self.context_addr = 0;
        self.port = 0;
        self.speed = None;
        self.enabled_endpoints = 0;
    }

    /// Address slot
    pub fn address(&mut self, context_addr: u64, port: u8, speed: UsbSpeed) {
        self.state = SlotState::Addressed;
        self.context_addr = context_addr;
        self.port = port;
        self.speed = Some(speed);
        self.enabled_endpoints = 1; // EP0 always enabled
    }

    /// Configure slot
    pub fn configure(&mut self, endpoints: u32) {
        self.state = SlotState::Configured;
        self.enabled_endpoints = endpoints | 1; // Keep EP0
    }

    /// Check if slot is enabled
    pub fn is_enabled(&self) -> bool {
        self.state != SlotState::Disabled
    }
}

/// xHCI operational register offsets
pub mod opreg {
    /// USB command
    pub const USBCMD: u32 = 0x00;
    /// USB status
    pub const USBSTS: u32 = 0x04;
    /// Page size
    pub const PAGESIZE: u32 = 0x08;
    /// Device notification control
    pub const DNCTRL: u32 = 0x14;
    /// Command ring control
    pub const CRCR: u32 = 0x18;
    /// Device context base address array pointer
    pub const DCBAAP: u32 = 0x30;
    /// Configure
    pub const CONFIG: u32 = 0x38;
}

/// USBCMD bits
pub mod usbcmd {
    /// Run/Stop
    pub const RS: u32 = 1 << 0;
    /// Host controller reset
    pub const HCRST: u32 = 1 << 1;
    /// Interrupter enable
    pub const INTE: u32 = 1 << 2;
    /// Host system error enable
    pub const HSEE: u32 = 1 << 3;
    /// Light host controller reset
    pub const LHCRST: u32 = 1 << 7;
    /// Controller save state
    pub const CSS: u32 = 1 << 8;
    /// Controller restore state
    pub const CRS: u32 = 1 << 9;
    /// Enable wrap event
    pub const EWE: u32 = 1 << 10;
    /// Enable U3 MFINDEX stop
    pub const EU3S: u32 = 1 << 11;
}

/// USBSTS bits
pub mod usbsts {
    /// Host controller halted
    pub const HCH: u32 = 1 << 0;
    /// Host system error
    pub const HSE: u32 = 1 << 2;
    /// Event interrupt
    pub const EINT: u32 = 1 << 3;
    /// Port change detect
    pub const PCD: u32 = 1 << 4;
    /// Save state status
    pub const SSS: u32 = 1 << 8;
    /// Restore state status
    pub const RSS: u32 = 1 << 9;
    /// Save/restore error
    pub const SRE: u32 = 1 << 10;
    /// Controller not ready
    pub const CNR: u32 = 1 << 11;
    /// Host controller error
    pub const HCE: u32 = 1 << 12;
}

/// xHCI controller state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum XhciState {
    /// Controller halted
    #[default]
    Halted,
    /// Controller running
    Running,
    /// Controller in reset
    Reset,
}

/// xHCI controller statistics
#[derive(Debug, Default)]
pub struct XhciStats {
    /// Commands processed
    pub commands: AtomicU64,
    /// Transfers completed
    pub transfers: AtomicU64,
    /// Events generated
    pub events: AtomicU64,
    /// Interrupts raised
    pub interrupts: AtomicU64,
    /// Port status changes
    pub port_changes: AtomicU64,
}

/// xHCI Host Controller
#[derive(Debug)]
pub struct XhciController {
    /// Controller name
    name: String,
    /// Controller state
    state: XhciState,
    /// USB command register
    usbcmd: u32,
    /// USB status register
    usbsts: u32,
    /// Page size
    pagesize: u32,
    /// Device notification control
    dnctrl: u32,
    /// Config register
    config: u32,
    /// Device context base address array pointer
    dcbaap: u64,
    /// Command ring
    command_ring: CommandRing,
    /// Ports (USB 2.0 + USB 3.0)
    ports: Vec<XhciPort>,
    /// Device slots
    slots: Vec<DeviceSlot>,
    /// Interrupters
    interrupters: Vec<Interrupter>,
    /// Next available slot ID
    next_slot: u8,
    /// Pending command TRBs (enqueued by software, processed on doorbell ring)
    pending_commands: VecDeque<Trb>,
    /// Pending transfer TRBs per (slot_id, endpoint_id)
    pending_transfers: HashMap<(u8, u8), VecDeque<Trb>>,
    /// Statistics
    stats: XhciStats,
}

impl XhciController {
    /// Create new xHCI controller
    #[must_use]
    pub fn new(name: &str, usb2_ports: u8, usb3_ports: u8) -> Self {
        let mut ports = Vec::new();

        // Create USB 3.0 ports first (convention)
        for i in 0..usb3_ports {
            ports.push(XhciPort::new(i + 1, true));
        }
        // Then USB 2.0 ports
        for i in 0..usb2_ports {
            ports.push(XhciPort::new(usb3_ports + i + 1, false));
        }

        let mut slots = Vec::with_capacity(MAX_SLOTS);
        for i in 0..MAX_SLOTS {
            slots.push(DeviceSlot::new((i + 1) as u8));
        }

        let mut interrupters = Vec::with_capacity(MAX_INTERRUPTERS);
        for _ in 0..MAX_INTERRUPTERS {
            interrupters.push(Interrupter::new());
        }

        Self {
            name: name.to_string(),
            state: XhciState::Halted,
            usbcmd: 0,
            usbsts: usbsts::HCH, // Halted
            pagesize: 1,         // 4KB
            dnctrl: 0,
            config: 0,
            dcbaap: 0,
            command_ring: CommandRing::new(),
            ports,
            slots,
            interrupters,
            next_slot: 1,
            pending_commands: VecDeque::new(),
            pending_transfers: HashMap::new(),
            stats: XhciStats::default(),
        }
    }

    /// Get controller name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get controller state
    pub fn state(&self) -> XhciState {
        self.state
    }

    /// Check if controller is running
    pub fn is_running(&self) -> bool {
        self.state == XhciState::Running
    }

    /// Get number of ports
    pub fn num_ports(&self) -> usize {
        self.ports.len()
    }

    /// Get port by number (1-based)
    pub fn get_port(&self, port_num: u8) -> Option<&XhciPort> {
        if port_num == 0 || port_num as usize > self.ports.len() {
            return None;
        }
        self.ports.get((port_num - 1) as usize)
    }

    /// Get mutable port by number (1-based)
    pub fn get_port_mut(&mut self, port_num: u8) -> Option<&mut XhciPort> {
        if port_num == 0 || port_num as usize > self.ports.len() {
            return None;
        }
        self.ports.get_mut((port_num - 1) as usize)
    }

    /// Connect device to port
    pub fn connect_device(&mut self, port_num: u8, speed: UsbSpeed) -> bool {
        if let Some(port) = self.get_port_mut(port_num) {
            port.connect(speed);
            // Queue port status change event
            let event = Trb::port_status_change(port_num);
            if let Some(intr) = self.interrupters.get_mut(0) {
                intr.event_ring.queue_event(event);
                intr.set_pending();
            }
            self.usbsts |= usbsts::PCD;
            self.stats.port_changes.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Disconnect device from port
    pub fn disconnect_device(&mut self, port_num: u8) -> bool {
        // First, get the slot_id to release (if any)
        let slot_to_release = {
            let port_idx = (port_num - 1) as usize;
            self.ports.get(port_idx).map(|p| p.slot_id).unwrap_or(0)
        };

        // Release the slot if assigned
        if slot_to_release != 0 {
            if let Some(slot) = self.slots.get_mut((slot_to_release - 1) as usize) {
                slot.disable();
            }
        }

        // Now disconnect the port
        if let Some(port) = self.get_port_mut(port_num) {
            port.disconnect();
            // Queue port status change event
            let event = Trb::port_status_change(port_num);
            if let Some(intr) = self.interrupters.get_mut(0) {
                intr.event_ring.queue_event(event);
                intr.set_pending();
            }
            self.usbsts |= usbsts::PCD;
            self.stats.port_changes.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    /// Reset controller
    pub fn reset(&mut self) {
        self.state = XhciState::Reset;
        self.usbcmd = 0;
        self.usbsts = usbsts::HCH;
        self.dnctrl = 0;
        self.config = 0;
        self.dcbaap = 0;
        self.command_ring = CommandRing::new();

        // Reset ports
        for port in &mut self.ports {
            port.state = PortState::Disconnected;
            port.slot_id = 0;
            port.regs = PortRegister::default();
        }

        // Reset slots
        for slot in &mut self.slots {
            slot.disable();
        }

        // Reset interrupters
        for intr in &mut self.interrupters {
            *intr = Interrupter::new();
        }

        self.next_slot = 1;
        self.state = XhciState::Halted;
    }

    /// Start controller
    pub fn start(&mut self) {
        if self.state == XhciState::Halted {
            self.state = XhciState::Running;
            self.usbsts &= !usbsts::HCH;
            self.command_ring.running = true;
        }
    }

    /// Stop controller
    pub fn stop(&mut self) {
        if self.state == XhciState::Running {
            self.state = XhciState::Halted;
            self.usbsts |= usbsts::HCH;
            self.command_ring.running = false;
        }
    }

    /// Write operational register
    pub fn write_opreg(&mut self, offset: u32, value: u32) {
        match offset {
            opreg::USBCMD => {
                let old = self.usbcmd;
                self.usbcmd = value;

                // Handle run/stop
                if value & usbcmd::RS != 0 && old & usbcmd::RS == 0 {
                    self.start();
                } else if value & usbcmd::RS == 0 && old & usbcmd::RS != 0 {
                    self.stop();
                }

                // Handle reset
                if value & usbcmd::HCRST != 0 {
                    self.reset();
                }
            }
            opreg::USBSTS => {
                // Write-1-to-clear bits
                self.usbsts &= !(value & 0x0000041C);
            }
            opreg::DNCTRL => {
                self.dnctrl = value;
            }
            opreg::CONFIG => {
                self.config = value & 0xFF; // Max slots enabled
            }
            _ => {}
        }
    }

    /// Read operational register
    pub fn read_opreg(&self, offset: u32) -> u32 {
        match offset {
            opreg::USBCMD => self.usbcmd,
            opreg::USBSTS => self.usbsts,
            opreg::PAGESIZE => self.pagesize,
            opreg::DNCTRL => self.dnctrl,
            opreg::CONFIG => self.config,
            _ => 0,
        }
    }

    /// Write CRCR (64-bit)
    pub fn write_crcr(&mut self, value: u64) {
        self.command_ring.set_pointer(value);
    }

    /// Write DCBAAP (64-bit)
    pub fn write_dcbaap(&mut self, value: u64) {
        self.dcbaap = value & !0x3F; // 64-byte aligned
    }

    /// Process enable slot command
    fn process_enable_slot(&mut self, _trb: &Trb) -> (TrbCompletionCode, u8) {
        // Find free slot
        for slot in &mut self.slots {
            if !slot.is_enabled() {
                slot.enable();
                return (TrbCompletionCode::Success, slot.id);
            }
        }
        (TrbCompletionCode::NoSlotsAvailable, 0)
    }

    /// Process disable slot command
    fn process_disable_slot(&mut self, slot_id: u8) -> TrbCompletionCode {
        if slot_id == 0 || slot_id as usize > self.slots.len() {
            return TrbCompletionCode::SlotNotEnabled;
        }

        // First check if slot is enabled and get port number
        let port_to_clear = {
            let slot = &self.slots[(slot_id - 1) as usize];
            if !slot.is_enabled() {
                return TrbCompletionCode::SlotNotEnabled;
            }
            slot.port
        };

        // Clear port association
        if port_to_clear != 0 {
            if let Some(port) = self.get_port_mut(port_to_clear) {
                port.slot_id = 0;
            }
        }

        // Now disable the slot
        self.slots[(slot_id - 1) as usize].disable();
        TrbCompletionCode::Success
    }

    /// Process command TRB
    pub fn process_command(&mut self, trb: Trb) -> Option<Trb> {
        let trb_type = trb.trb_type()?;
        self.stats.commands.fetch_add(1, Ordering::Relaxed);

        let (code, slot_id) = match trb_type {
            TrbType::EnableSlot => self.process_enable_slot(&trb),
            TrbType::DisableSlot => {
                let slot_id = ((trb.control >> 24) & 0xFF) as u8;
                (self.process_disable_slot(slot_id), slot_id)
            }
            TrbType::NoOpCommand => (TrbCompletionCode::Success, 0),
            _ => (TrbCompletionCode::TrbError, 0),
        };

        // Generate completion event
        let event =
            Trb::command_completion(self.command_ring.segment.current_addr(), code, slot_id);

        self.stats.events.fetch_add(1, Ordering::Relaxed);
        Some(event)
    }

    /// Enqueue a command TRB for processing on the next doorbell ring.
    ///
    /// In a real xHCI the guest writes TRBs to the command ring in memory;
    /// this method provides the same mechanism without guest memory access.
    pub fn enqueue_command(&mut self, trb: Trb) {
        self.pending_commands.push_back(trb);
    }

    /// Ring doorbell
    pub fn ring_doorbell(&mut self, slot_id: u8, target: u8) {
        if slot_id == 0 {
            // Host controller doorbell - process command ring
            if self.command_ring.running {
                // Drain pending command TRBs
                while let Some(trb) = self.pending_commands.pop_front() {
                    if let Some(event) = self.process_command(trb) {
                        self.interrupters[0].event_ring.queue_event(event);
                        self.interrupters[0].set_pending();
                    }
                    self.command_ring.segment.advance();
                }
                // If no pending commands, just advance (legacy behaviour)
                if self.command_ring.segment.index == 0 {
                    // Already advanced inside the loop
                } else if self.pending_commands.is_empty() {
                    // No commands were queued — nothing extra to do
                }
            }
        } else {
            // Device slot doorbell - process transfer ring
            // target is the endpoint ID (1-31)
            let slot_idx = (slot_id - 1) as usize;
            if let Some(slot) = self.slots.get(slot_idx) {
                if slot.is_enabled() && (slot.enabled_endpoints & (1 << target)) != 0 {
                    self.stats.transfers.fetch_add(1, Ordering::Relaxed);

                    // Drain pending transfer TRBs for this slot
                    let key = (slot_id, target);
                    if let Some(transfers) = self.pending_transfers.get_mut(&key) {
                        while let Some(trb) = transfers.pop_front() {
                            // Generate transfer completion event
                            let event = Trb::transfer_event(
                                0, // transfer TRB pointer (would be ring address)
                                TrbCompletionCode::Success,
                                trb.status, // transfer length
                                slot_id,
                                target,
                            );
                            self.interrupters[0].event_ring.queue_event(event);
                            self.interrupters[0].set_pending();
                            self.stats.events.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
        }
    }

    /// Check for pending interrupts
    pub fn has_interrupt(&self) -> bool {
        if self.usbcmd & usbcmd::INTE == 0 {
            return false;
        }
        self.interrupters
            .iter()
            .any(|i| i.pending && i.is_enabled())
    }

    /// Acknowledge interrupt
    pub fn acknowledge_interrupt(&mut self) {
        for intr in &mut self.interrupters {
            if intr.pending {
                intr.clear_interrupt();
                break;
            }
        }
        self.stats.interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn stats(&self) -> (u64, u64, u64, u64, u64) {
        (
            self.stats.commands.load(Ordering::Relaxed),
            self.stats.transfers.load(Ordering::Relaxed),
            self.stats.events.load(Ordering::Relaxed),
            self.stats.interrupts.load(Ordering::Relaxed),
            self.stats.port_changes.load(Ordering::Relaxed),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usb_speed() {
        assert_eq!(UsbSpeed::from_psid(1), Some(UsbSpeed::Full));
        assert_eq!(UsbSpeed::from_psid(3), Some(UsbSpeed::High));
        assert_eq!(UsbSpeed::from_psid(4), Some(UsbSpeed::Super));
        assert_eq!(UsbSpeed::Full.max_packet_size(), 64);
        assert_eq!(UsbSpeed::Super.max_packet_size(), 512);
    }

    #[test]
    fn test_port_creation() {
        let port = XhciPort::new(1, true);
        assert_eq!(port.number, 1);
        assert!(port.usb3);
        assert!(!port.is_connected());
        assert!(!port.is_enabled());
    }

    #[test]
    fn test_port_connect_disconnect() {
        let mut port = XhciPort::new(1, false);

        port.connect(UsbSpeed::High);
        assert!(port.is_connected());
        assert_eq!(port.speed, Some(UsbSpeed::High));
        assert!(port.regs.portsc & portsc::CSC != 0);

        port.disconnect();
        assert!(!port.is_connected());
        assert_eq!(port.speed, None);
    }

    #[test]
    fn test_port_reset() {
        let mut port = XhciPort::new(1, false);
        port.connect(UsbSpeed::High);

        port.reset();
        assert!(port.is_enabled());
        assert_eq!(port.state, PortState::Enabled);
        assert!(port.regs.portsc & portsc::PRC != 0);
    }

    #[test]
    fn test_trb_creation() {
        let trb = Trb::new(TrbType::EnableSlot);
        assert_eq!(trb.trb_type(), Some(TrbType::EnableSlot));
        assert!(!trb.cycle());
    }

    #[test]
    fn test_trb_cycle_bit() {
        let mut trb = Trb::new(TrbType::Normal);
        assert!(!trb.cycle());

        trb.set_cycle(true);
        assert!(trb.cycle());

        trb.set_cycle(false);
        assert!(!trb.cycle());
    }

    #[test]
    fn test_trb_type_conversion() {
        assert_eq!(TrbType::from_raw(1), Some(TrbType::Normal));
        assert_eq!(TrbType::from_raw(9), Some(TrbType::EnableSlot));
        assert_eq!(TrbType::from_raw(33), Some(TrbType::CommandCompletion));
        assert_eq!(TrbType::from_raw(255), None);
    }

    #[test]
    fn test_ring_segment() {
        let mut segment = RingSegment::new(0x1000, 16);
        assert_eq!(segment.current_addr(), 0x1000);
        assert!(segment.cycle);

        segment.advance();
        assert_eq!(segment.current_addr(), 0x1010);

        // Wrap around
        for _ in 0..15 {
            segment.advance();
        }
        assert_eq!(segment.index, 0);
        assert!(!segment.cycle);
    }

    #[test]
    fn test_command_ring() {
        let mut ring = CommandRing::new();
        ring.set_pointer(0x2000 | 1);

        assert_eq!(ring.segment.base, 0x2000);
        assert!(ring.segment.cycle);
    }

    #[test]
    fn test_event_ring() {
        let mut ring = EventRing::new();
        assert!(!ring.has_pending());

        ring.queue_event(Trb::new(TrbType::CommandCompletion));
        assert!(ring.has_pending());

        let event = ring.pop_event();
        assert!(event.is_some());
        assert!(!ring.has_pending());
    }

    #[test]
    fn test_interrupter() {
        let mut intr = Interrupter::new();
        assert!(!intr.pending);
        assert!(!intr.is_enabled());

        intr.iman |= 1 << 1; // Enable
        assert!(intr.is_enabled());

        intr.set_pending();
        assert!(intr.pending);

        intr.clear_interrupt();
        assert!(!intr.pending);
    }

    #[test]
    fn test_device_slot() {
        let mut slot = DeviceSlot::new(1);
        assert_eq!(slot.id, 1);
        assert!(!slot.is_enabled());

        slot.enable();
        assert!(slot.is_enabled());
        assert_eq!(slot.state, SlotState::Enabled);

        slot.address(0x5000, 1, UsbSpeed::High);
        assert_eq!(slot.state, SlotState::Addressed);
        assert_eq!(slot.port, 1);

        slot.configure(0x0E); // EP1-3
        assert_eq!(slot.state, SlotState::Configured);
        assert_eq!(slot.enabled_endpoints, 0x0F); // EP0 + EP1-3

        slot.disable();
        assert!(!slot.is_enabled());
    }

    #[test]
    fn test_xhci_creation() {
        let xhci = XhciController::new("xhci0", 4, 2);
        assert_eq!(xhci.name(), "xhci0");
        assert_eq!(xhci.num_ports(), 6);
        assert_eq!(xhci.state(), XhciState::Halted);
    }

    #[test]
    fn test_xhci_port_access() {
        let xhci = XhciController::new("xhci0", 2, 2);

        // Port 1 should be USB 3.0
        let port1 = xhci.get_port(1).unwrap();
        assert!(port1.usb3);

        // Port 3 should be USB 2.0
        let port3 = xhci.get_port(3).unwrap();
        assert!(!port3.usb3);

        // Invalid port
        assert!(xhci.get_port(0).is_none());
        assert!(xhci.get_port(10).is_none());
    }

    #[test]
    fn test_xhci_start_stop() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        assert_eq!(xhci.state(), XhciState::Halted);

        xhci.start();
        assert_eq!(xhci.state(), XhciState::Running);
        assert!(xhci.is_running());

        xhci.stop();
        assert_eq!(xhci.state(), XhciState::Halted);
        assert!(!xhci.is_running());
    }

    #[test]
    fn test_xhci_reset() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.start();
        xhci.connect_device(1, UsbSpeed::Super);

        xhci.reset();
        assert_eq!(xhci.state(), XhciState::Halted);
        assert_eq!(xhci.usbcmd, 0);
    }

    #[test]
    fn test_xhci_connect_device() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        assert!(xhci.connect_device(1, UsbSpeed::Super));

        let port = xhci.get_port(1).unwrap();
        assert!(port.is_connected());
        assert_eq!(port.speed, Some(UsbSpeed::Super));

        // Check event was queued
        let intr = &xhci.interrupters[0];
        assert!(intr.pending);
    }

    #[test]
    fn test_xhci_disconnect_device() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.connect_device(1, UsbSpeed::Super);

        assert!(xhci.disconnect_device(1));

        let port = xhci.get_port(1).unwrap();
        assert!(!port.is_connected());
    }

    #[test]
    fn test_xhci_opreg_read_write() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        // Write USBCMD with RS bit to start
        xhci.write_opreg(opreg::USBCMD, usbcmd::RS);
        assert!(xhci.is_running());

        // Read USBSTS
        let sts = xhci.read_opreg(opreg::USBSTS);
        assert_eq!(sts & usbsts::HCH, 0); // Not halted

        // Write CONFIG
        xhci.write_opreg(opreg::CONFIG, 32);
        assert_eq!(xhci.read_opreg(opreg::CONFIG), 32);
    }

    #[test]
    fn test_xhci_enable_slot() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        let trb = Trb::new(TrbType::EnableSlot);
        let event = xhci.process_command(trb).unwrap();

        assert_eq!(event.trb_type(), Some(TrbType::CommandCompletion));
        // Slot ID should be 1
        let slot_id = ((event.control >> 24) & 0xFF) as u8;
        assert_eq!(slot_id, 1);
    }

    #[test]
    fn test_xhci_disable_slot() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        // Enable slot first
        let trb = Trb::new(TrbType::EnableSlot);
        xhci.process_command(trb);

        // Disable slot 1
        let mut trb = Trb::new(TrbType::DisableSlot);
        trb.control |= 1 << 24; // Slot ID 1
        let event = xhci.process_command(trb).unwrap();

        let code = ((event.status >> 24) & 0xFF) as u8;
        assert_eq!(code, TrbCompletionCode::Success as u8);
    }

    #[test]
    fn test_xhci_noop_command() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        let trb = Trb::new(TrbType::NoOpCommand);
        let event = xhci.process_command(trb).unwrap();

        let code = ((event.status >> 24) & 0xFF) as u8;
        assert_eq!(code, TrbCompletionCode::Success as u8);
    }

    #[test]
    fn test_xhci_doorbell() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.start();

        // Ring host controller doorbell
        xhci.ring_doorbell(0, 0);

        let stats = xhci.stats();
        // Commands stat may not increase without memory reads
    }

    #[test]
    fn test_xhci_interrupt() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        // Enable interrupts
        xhci.usbcmd |= usbcmd::INTE;
        xhci.interrupters[0].iman |= 1 << 1; // Enable interrupter

        // Connect device to generate event
        xhci.connect_device(1, UsbSpeed::High);

        assert!(xhci.has_interrupt());

        xhci.acknowledge_interrupt();
        assert!(!xhci.has_interrupt());
    }

    #[test]
    fn test_xhci_stats() {
        let mut xhci = XhciController::new("xhci0", 2, 2);

        // Process a command
        let trb = Trb::new(TrbType::NoOpCommand);
        xhci.process_command(trb);

        let (cmds, _, events, _, _) = xhci.stats();
        assert_eq!(cmds, 1);
        assert_eq!(events, 1);
    }

    #[test]
    fn test_completion_code_values() {
        assert_eq!(TrbCompletionCode::Success as u8, 1);
        assert_eq!(TrbCompletionCode::ShortPacket as u8, 13);
        assert_eq!(TrbCompletionCode::NoSlotsAvailable as u8, 9);
    }

    #[test]
    fn test_port_status_change_event() {
        let event = Trb::port_status_change(5);
        assert_eq!(event.trb_type(), Some(TrbType::PortStatusChange));
        let port_id = ((event.parameter >> 24) & 0xFF) as u8;
        assert_eq!(port_id, 5);
    }

    #[test]
    fn test_transfer_event() {
        let event = Trb::transfer_event(0x1000, TrbCompletionCode::Success, 512, 1, 2);
        assert_eq!(event.trb_type(), Some(TrbType::TransferEvent));
        assert_eq!(event.parameter, 0x1000);
        let length = event.status & 0xFFFFFF;
        assert_eq!(length, 512);
    }

    #[test]
    fn test_xhci_doorbell_noop() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.start();

        // Enqueue a NoOp command and ring the host controller doorbell
        xhci.enqueue_command(Trb::new(TrbType::NoOpCommand));
        xhci.ring_doorbell(0, 0);

        // Verify a completion event was queued
        let event = xhci.interrupters[0].event_ring.pop_event().unwrap();
        assert_eq!(event.trb_type(), Some(TrbType::CommandCompletion));
        let code = ((event.status >> 24) & 0xFF) as u8;
        assert_eq!(code, TrbCompletionCode::Success as u8);

        // Interrupter should have been set pending
        assert!(xhci.interrupters[0].pending);

        // Stats should reflect one command and one event
        let (cmds, _, events, _, _) = xhci.stats();
        assert_eq!(cmds, 1);
        assert_eq!(events, 1);
    }

    #[test]
    fn test_xhci_doorbell_enable_slot() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.start();

        xhci.enqueue_command(Trb::new(TrbType::EnableSlot));
        xhci.ring_doorbell(0, 0);

        // Verify slot 1 is now enabled
        assert!(xhci.slots[0].is_enabled());

        // Verify completion event carries slot_id = 1
        let event = xhci.interrupters[0].event_ring.pop_event().unwrap();
        let slot_id = ((event.control >> 24) & 0xFF) as u8;
        assert_eq!(slot_id, 1);
        let code = ((event.status >> 24) & 0xFF) as u8;
        assert_eq!(code, TrbCompletionCode::Success as u8);
    }

    #[test]
    fn test_xhci_doorbell_multiple_commands() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        xhci.start();

        // Enqueue three commands, ring doorbell once
        xhci.enqueue_command(Trb::new(TrbType::NoOpCommand));
        xhci.enqueue_command(Trb::new(TrbType::EnableSlot));
        let mut disable = Trb::new(TrbType::DisableSlot);
        disable.control |= 1 << 24; // slot 1
        xhci.enqueue_command(disable);
        xhci.ring_doorbell(0, 0);

        // All three should produce completion events
        let ev1 = xhci.interrupters[0].event_ring.pop_event().unwrap();
        let ev2 = xhci.interrupters[0].event_ring.pop_event().unwrap();
        let ev3 = xhci.interrupters[0].event_ring.pop_event().unwrap();
        assert!(xhci.interrupters[0].event_ring.pop_event().is_none());

        // NoOp => Success
        assert_eq!((ev1.status >> 24) & 0xFF, TrbCompletionCode::Success as u32);
        // EnableSlot => Success, slot 1
        assert_eq!((ev2.status >> 24) & 0xFF, TrbCompletionCode::Success as u32);
        assert_eq!((ev2.control >> 24) & 0xFF, 1);
        // DisableSlot => Success (slot was just enabled)
        assert_eq!((ev3.status >> 24) & 0xFF, TrbCompletionCode::Success as u32);

        let (cmds, _, events, _, _) = xhci.stats();
        assert_eq!(cmds, 3);
        assert_eq!(events, 3);
    }

    #[test]
    fn test_xhci_doorbell_no_commands_when_stopped() {
        let mut xhci = XhciController::new("xhci0", 2, 2);
        // Controller is NOT started — command ring not running

        xhci.enqueue_command(Trb::new(TrbType::NoOpCommand));
        xhci.ring_doorbell(0, 0);

        // No events should be produced
        assert!(xhci.interrupters[0].event_ring.pop_event().is_none());
        let (cmds, _, _, _, _) = xhci.stats();
        assert_eq!(cmds, 0);
    }
}
