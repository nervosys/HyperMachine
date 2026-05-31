//! Intel High Definition Audio (HDA) Controller
//!
//! This module provides Intel HDA controller emulation with
//! codec discovery, stream management, and widget support.

use super::core::{
    AudioParams, AudioStats, AudioStream, ChannelLayout, PcmStream, SampleFormat, SampleRate,
    StreamDirection,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// HDA register offsets
pub mod Registers {
    /// Global Capabilities
    pub const GCAP: u32 = 0x00;
    /// Minor Version
    pub const VMIN: u32 = 0x02;
    /// Major Version
    pub const VMAJ: u32 = 0x03;
    /// Output Payload Capability
    pub const OUTPAY: u32 = 0x04;
    /// Input Payload Capability
    pub const INPAY: u32 = 0x06;
    /// Global Control
    pub const GCTL: u32 = 0x08;
    /// Wake Enable
    pub const WAKEEN: u32 = 0x0C;
    /// State Change Status
    pub const STATESTS: u32 = 0x0E;
    /// Global Status
    pub const GSTS: u32 = 0x10;
    /// Output Stream Payload Capability
    pub const OUTSTRMPAY: u32 = 0x18;
    /// Input Stream Payload Capability
    pub const INSTRMPAY: u32 = 0x1A;
    /// Interrupt Control
    pub const INTCTL: u32 = 0x20;
    /// Interrupt Status
    pub const INTSTS: u32 = 0x24;
    /// Wall Clock Counter
    pub const WALLCLK: u32 = 0x30;
    /// Stream Synchronization
    pub const SSYNC: u32 = 0x38;
    /// CORB Lower Base Address
    pub const CORBLBASE: u32 = 0x40;
    /// CORB Upper Base Address
    pub const CORBUBASE: u32 = 0x44;
    /// CORB Write Pointer
    pub const CORBWP: u32 = 0x48;
    /// CORB Read Pointer
    pub const CORBRP: u32 = 0x4A;
    /// CORB Control
    pub const CORBCTL: u32 = 0x4C;
    /// CORB Status
    pub const CORBSTS: u32 = 0x4D;
    /// CORB Size
    pub const CORBSIZE: u32 = 0x4E;
    /// RIRB Lower Base Address
    pub const RIRBLBASE: u32 = 0x50;
    /// RIRB Upper Base Address
    pub const RIRBUBASE: u32 = 0x54;
    /// RIRB Write Pointer
    pub const RIRBWP: u32 = 0x58;
    /// RIRB Interrupt Count
    pub const RINTCNT: u32 = 0x5A;
    /// RIRB Control
    pub const RIRBCTL: u32 = 0x5C;
    /// RIRB Status
    pub const RIRBSTS: u32 = 0x5D;
    /// RIRB Size
    pub const RIRBSIZE: u32 = 0x5E;
    /// Immediate Command Output Interface
    pub const ICOI: u32 = 0x60;
    /// Immediate Response Input Interface
    pub const ICII: u32 = 0x64;
    /// Immediate Command Status
    pub const ICIS: u32 = 0x68;
    /// DMA Position Lower Base Address
    pub const DPLBASE: u32 = 0x70;
    /// DMA Position Upper Base Address
    pub const DPUBASE: u32 = 0x74;
    /// Stream descriptor base (input streams start at 0x80)
    pub const SD_BASE: u32 = 0x80;
    /// Stream descriptor stride
    pub const SD_STRIDE: u32 = 0x20;
}

/// Global Control bits
pub mod GlobalCtl {
    /// Controller reset
    pub const CRST: u32 = 1 << 0;
    /// Flush control
    pub const FCNTRL: u32 = 1 << 1;
    /// Accept unsolicited response enable
    pub const UNSOL: u32 = 1 << 8;
}

/// Stream descriptor register offsets
pub mod StreamReg {
    /// Control
    pub const CTL: u32 = 0x00;
    /// Status
    pub const STS: u32 = 0x03;
    /// Link Position in Buffer
    pub const LPIB: u32 = 0x04;
    /// Cyclic Buffer Length
    pub const CBL: u32 = 0x08;
    /// Last Valid Index
    pub const LVI: u32 = 0x0C;
    /// FIFO Size
    pub const FIFOS: u32 = 0x10;
    /// Format
    pub const FMT: u32 = 0x12;
    /// Buffer Descriptor List Pointer Lower
    pub const BDLPL: u32 = 0x18;
    /// Buffer Descriptor List Pointer Upper
    pub const BDLPU: u32 = 0x1C;
}

/// Stream control bits
pub mod StreamCtl {
    /// Stream reset
    pub const SRST: u32 = 1 << 0;
    /// Stream run
    pub const RUN: u32 = 1 << 1;
    /// Interrupt on completion enable
    pub const IOCE: u32 = 1 << 2;
    /// FIFO error interrupt enable
    pub const FEIE: u32 = 1 << 3;
    /// Descriptor error interrupt enable
    pub const DEIE: u32 = 1 << 4;
    /// Stripe control
    pub const STRIPE: u32 = 3 << 16;
    /// Traffic priority
    pub const TP: u32 = 1 << 18;
    /// Bidirectional direction
    pub const DIR: u32 = 1 << 19;
    /// Stream number
    pub const STRM: u32 = 0xF << 20;
}

/// Stream status bits
pub mod StreamSts {
    /// Buffer completion interrupt status
    pub const BCIS: u8 = 1 << 2;
    /// FIFO error
    pub const FIFOE: u8 = 1 << 3;
    /// Descriptor error
    pub const DESE: u8 = 1 << 4;
    /// FIFO ready
    pub const FIFORDY: u8 = 1 << 5;
}

/// HDA codec node types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetType {
    /// Audio output
    AudioOutput = 0,
    /// Audio input
    AudioInput = 1,
    /// Audio mixer
    AudioMixer = 2,
    /// Audio selector
    AudioSelector = 3,
    /// Pin complex
    PinComplex = 4,
    /// Power widget
    Power = 5,
    /// Volume knob
    VolumeKnob = 6,
    /// Beep generator
    BeepGenerator = 7,
    /// Vendor defined
    VendorDefined = 15,
}

impl WidgetType {
    /// Create from capability value
    pub fn from_caps(caps: u32) -> Option<Self> {
        match (caps >> 20) & 0xF {
            0 => Some(WidgetType::AudioOutput),
            1 => Some(WidgetType::AudioInput),
            2 => Some(WidgetType::AudioMixer),
            3 => Some(WidgetType::AudioSelector),
            4 => Some(WidgetType::PinComplex),
            5 => Some(WidgetType::Power),
            6 => Some(WidgetType::VolumeKnob),
            7 => Some(WidgetType::BeepGenerator),
            15 => Some(WidgetType::VendorDefined),
            _ => None,
        }
    }
}

/// Pin configuration
#[derive(Debug, Clone, Copy, Default)]
pub struct PinConfig {
    /// Default configuration
    pub default_config: u32,
    /// Pin capabilities
    pub pin_caps: u32,
    /// Widget capabilities
    pub widget_caps: u32,
}

impl PinConfig {
    /// Line out jack
    pub fn line_out() -> Self {
        Self {
            // Green jack, rear panel, line out
            default_config: 0x01014010,
            pin_caps: 0x0001001C, // Output capable, headphone amp, trigger required
            widget_caps: 0x00400000 | (WidgetType::PinComplex as u32) << 20,
        }
    }

    /// Headphone jack
    pub fn headphone() -> Self {
        Self {
            // Black jack, front panel, headphone
            default_config: 0x02211020,
            pin_caps: 0x0001001C,
            widget_caps: 0x00400000 | (WidgetType::PinComplex as u32) << 20,
        }
    }

    /// Microphone jack
    pub fn mic_in() -> Self {
        Self {
            // Pink jack, rear panel, mic in
            default_config: 0x01A19040,
            pin_caps: 0x00001734, // Input capable, presence detect
            widget_caps: 0x00400000 | (WidgetType::PinComplex as u32) << 20,
        }
    }

    /// Line in jack
    pub fn line_in() -> Self {
        Self {
            // Blue jack, rear panel, line in
            default_config: 0x01813050,
            pin_caps: 0x00001734,
            widget_caps: 0x00400000 | (WidgetType::PinComplex as u32) << 20,
        }
    }

    /// Get port connectivity
    pub fn port_connectivity(&self) -> u8 {
        ((self.default_config >> 30) & 0x3) as u8
    }

    /// Get location
    pub fn location(&self) -> u8 {
        ((self.default_config >> 24) & 0x3F) as u8
    }

    /// Get default device
    pub fn default_device(&self) -> u8 {
        ((self.default_config >> 20) & 0xF) as u8
    }

    /// Get connection type
    pub fn connection_type(&self) -> u8 {
        ((self.default_config >> 16) & 0xF) as u8
    }

    /// Get color
    pub fn color(&self) -> u8 {
        ((self.default_config >> 12) & 0xF) as u8
    }

    /// Get misc
    pub fn misc(&self) -> u8 {
        ((self.default_config >> 8) & 0xF) as u8
    }

    /// Get default association
    pub fn default_association(&self) -> u8 {
        ((self.default_config >> 4) & 0xF) as u8
    }

    /// Get sequence
    pub fn sequence(&self) -> u8 {
        (self.default_config & 0xF) as u8
    }
}

/// HDA codec widget
#[derive(Debug, Clone)]
pub struct Widget {
    /// Node ID
    pub nid: u8,
    /// Widget type
    pub widget_type: WidgetType,
    /// Widget capabilities
    pub caps: u32,
    /// Pin configuration (for pin widgets)
    pub pin_config: Option<PinConfig>,
    /// Amplifier gain/mute capabilities
    pub amp_caps_in: u32,
    pub amp_caps_out: u32,
    /// Connection list
    pub connections: Vec<u8>,
    /// Selected connection
    pub selected_connection: u8,
    /// Output amplifier state (gain + mute)
    pub amp_out: u32,
    /// Input amplifier state
    pub amp_in: Vec<u32>,
    /// Stream/channel assignment
    pub stream_channel: u8,
    /// Format
    pub format: u16,
    /// Power state
    pub power_state: u8,
}

impl Widget {
    /// Create new widget
    pub fn new(nid: u8, widget_type: WidgetType) -> Self {
        Self {
            nid,
            widget_type,
            caps: (widget_type as u32) << 20,
            pin_config: None,
            amp_caps_in: 0,
            amp_caps_out: 0,
            connections: Vec::new(),
            selected_connection: 0,
            amp_out: 0,
            amp_in: Vec::new(),
            stream_channel: 0,
            format: 0,
            power_state: 0,
        }
    }

    /// Create audio output widget
    pub fn audio_output(nid: u8) -> Self {
        let mut w = Self::new(nid, WidgetType::AudioOutput);
        w.caps = 0x00000011 | (WidgetType::AudioOutput as u32) << 20; // Digital, stereo
        w.amp_caps_out = 0x80004F4F; // Mute capable, 0-79 steps
        w
    }

    /// Create audio input widget
    pub fn audio_input(nid: u8) -> Self {
        let mut w = Self::new(nid, WidgetType::AudioInput);
        w.caps = 0x00100011 | (WidgetType::AudioInput as u32) << 20;
        w.amp_caps_in = 0x80004F4F;
        w
    }

    /// Create mixer widget
    pub fn mixer(nid: u8, connections: Vec<u8>) -> Self {
        let mut w = Self::new(nid, WidgetType::AudioMixer);
        w.caps = 0x00200000 | (WidgetType::AudioMixer as u32) << 20;
        w.amp_in = vec![0; connections.len()];
        w.connections = connections;
        w
    }

    /// Create pin widget
    pub fn pin(nid: u8, config: PinConfig) -> Self {
        let mut w = Self::new(nid, WidgetType::PinComplex);
        w.caps = config.widget_caps;
        w.pin_config = Some(config);
        w
    }
}

/// HDA codec
#[derive(Debug)]
pub struct HdaCodec {
    /// Codec address
    pub address: u8,
    /// Vendor ID
    pub vendor_id: u32,
    /// Subsystem ID
    pub subsystem_id: u32,
    /// Revision ID
    pub revision_id: u32,
    /// Function group start node
    pub fg_start: u8,
    /// Function group count
    pub fg_count: u8,
    /// Widgets
    pub widgets: HashMap<u8, Widget>,
    /// Output converter node
    output_converter: u8,
    /// Input converter node
    input_converter: u8,
}

impl HdaCodec {
    /// Create new HDA codec
    pub fn new(address: u8) -> Self {
        let mut codec = Self {
            address,
            vendor_id: 0x8086_2668, // Intel ICH6
            subsystem_id: 0x00000000,
            revision_id: 0x00100101,
            fg_start: 1,
            fg_count: 1,
            widgets: HashMap::new(),
            output_converter: 2,
            input_converter: 3,
        };
        codec.setup_widgets();
        codec
    }

    /// Setup default widget tree
    fn setup_widgets(&mut self) {
        // Audio function group (NID 1)
        let mut afg = Widget::new(1, WidgetType::VendorDefined);
        afg.caps = 0x00000001; // Audio function group
        self.widgets.insert(1, afg);

        // Output DAC (NID 2)
        let dac = Widget::audio_output(2);
        self.widgets.insert(2, dac);

        // Input ADC (NID 3)
        let adc = Widget::audio_input(3);
        self.widgets.insert(3, adc);

        // Output mixer (NID 4)
        let out_mix = Widget::mixer(4, vec![2]);
        self.widgets.insert(4, out_mix);

        // Input mixer (NID 5)
        let in_mix = Widget::mixer(5, vec![6, 7]);
        self.widgets.insert(5, in_mix);

        // Line out pin (NID 6)
        let line_out = Widget::pin(6, PinConfig::line_out());
        self.widgets.insert(6, line_out);

        // Mic in pin (NID 7)
        let mic_in = Widget::pin(7, PinConfig::mic_in());
        self.widgets.insert(7, mic_in);

        // Headphone pin (NID 8)
        let hp = Widget::pin(8, PinConfig::headphone());
        self.widgets.insert(8, hp);
    }

    /// Get subordinate node count
    pub fn subordinate_count(&self) -> u8 {
        self.widgets.len() as u8
    }

    /// Process command and return response
    pub fn process_verb(&mut self, verb: u32) -> u32 {
        let nid = ((verb >> 20) & 0x7F) as u8;
        let cmd = (verb >> 8) & 0xFFF;
        let data = verb & 0xFF;

        // Get parameters
        if cmd == 0xF00 {
            return self.get_parameter(nid, data as u8);
        }

        // Get/Set converters
        if cmd == 0xF06 {
            // Get stream/channel
            if let Some(w) = self.widgets.get(&nid) {
                return w.stream_channel as u32;
            }
        }
        if (cmd >> 4) == 0x70 {
            // Set stream/channel
            if let Some(w) = self.widgets.get_mut(&nid) {
                w.stream_channel = data as u8;
            }
            return 0;
        }

        // Get/Set amplifier gain
        if cmd == 0xB00 {
            // Get amp gain
            if let Some(w) = self.widgets.get(&nid) {
                return w.amp_out;
            }
        }
        if (cmd >> 4) == 0x30 {
            // Set amp gain
            if let Some(w) = self.widgets.get_mut(&nid) {
                w.amp_out = (data as u32) | ((cmd & 0xF) << 8) as u32;
            }
            return 0;
        }

        // Get/Set pin control
        if cmd == 0xF07 {
            return 0x40; // Out enable
        }

        // Get connection list
        if cmd == 0xF02 {
            if let Some(w) = self.widgets.get(&nid) {
                if !w.connections.is_empty() {
                    let idx = (data as usize) & 0x7;
                    if idx < w.connections.len() {
                        return w.connections[idx] as u32;
                    }
                }
            }
        }

        // Get connection select
        if cmd == 0xF01 {
            if let Some(w) = self.widgets.get(&nid) {
                return w.selected_connection as u32;
            }
        }

        // Set connection select
        if (cmd >> 4) == 0x70 && (cmd & 0xF) == 1 {
            if let Some(w) = self.widgets.get_mut(&nid) {
                w.selected_connection = data as u8;
            }
            return 0;
        }

        // Get EAPD/BTL enable
        if cmd == 0xF0C {
            return 0x02; // EAPD enabled
        }

        // Get/Set power state
        if cmd == 0xF05 {
            if let Some(w) = self.widgets.get(&nid) {
                return w.power_state as u32;
            }
        }
        if (cmd >> 4) == 0x70 && (cmd & 0xF) == 5 {
            if let Some(w) = self.widgets.get_mut(&nid) {
                w.power_state = data as u8;
            }
            return 0;
        }

        // Get pin configuration default
        if cmd == 0xF1C {
            if let Some(w) = self.widgets.get(&nid) {
                if let Some(pin) = &w.pin_config {
                    return (pin.default_config >> (data * 8)) & 0xFF;
                }
            }
        }

        0
    }

    /// Get parameter
    fn get_parameter(&self, nid: u8, param: u8) -> u32 {
        match param {
            0x00 => self.vendor_id,    // Vendor ID
            0x01 => self.subsystem_id, // Subsystem ID
            0x02 => self.revision_id,  // Revision ID
            0x04 => {
                // Subordinate node count
                if nid == 0 {
                    ((self.fg_start as u32) << 16) | self.fg_count as u32
                } else if nid == 1 {
                    // Audio function group: nodes 2-8
                    (2 << 16) | 7
                } else {
                    0
                }
            }
            0x05
                // Function group type
                if nid == 1 => {
                    0x01 // Audio function group
                }
            0x09 => {
                // Audio widget capabilities
                if let Some(w) = self.widgets.get(&nid) {
                    w.caps
                } else {
                    0
                }
            }
            0x0A => {
                // PCM rates/bits
                0x001E0060 // 16/20/24 bit, 44.1/48kHz
            }
            0x0B => {
                // Stream formats
                0x00000001 // PCM
            }
            0x0C => {
                // Pin capabilities
                if let Some(w) = self.widgets.get(&nid) {
                    if let Some(pin) = &w.pin_config {
                        return pin.pin_caps;
                    }
                }
                0
            }
            0x0D => {
                // Input amp caps
                if let Some(w) = self.widgets.get(&nid) {
                    w.amp_caps_in
                } else {
                    0
                }
            }
            0x0E => {
                // Connection list length
                if let Some(w) = self.widgets.get(&nid) {
                    w.connections.len() as u32
                } else {
                    0
                }
            }
            0x0F => {
                // Supported power states
                0x0000000F // D0-D3
            }
            0x12 => {
                // Output amp caps
                if let Some(w) = self.widgets.get(&nid) {
                    w.amp_caps_out
                } else {
                    0
                }
            }
            _ => 0,
        }
    }

    /// Get output converter node ID
    pub fn output_converter_nid(&self) -> u8 {
        self.output_converter
    }

    /// Get input converter node ID
    pub fn input_converter_nid(&self) -> u8 {
        self.input_converter
    }
}

/// Stream descriptor state
#[derive(Debug)]
pub struct StreamDescriptor {
    /// Control register
    pub ctl: u32,
    /// Status register
    pub sts: u8,
    /// Link position in buffer
    pub lpib: u32,
    /// Cyclic buffer length
    pub cbl: u32,
    /// Last valid index
    pub lvi: u16,
    /// FIFO size
    pub fifos: u16,
    /// Format
    pub fmt: u16,
    /// BDL pointer
    pub bdl_addr: u64,
    /// Stream number
    pub stream_num: u8,
    /// Direction (true = output)
    pub is_output: bool,
    /// PCM stream
    stream: PcmStream,
}

impl StreamDescriptor {
    /// Create new stream descriptor
    pub fn new(index: u8, is_output: bool) -> Self {
        let direction = if is_output {
            StreamDirection::Playback
        } else {
            StreamDirection::Capture
        };
        let params = AudioParams::new(
            SampleFormat::S16Le,
            SampleRate::Hz48000,
            ChannelLayout::Stereo,
        );

        Self {
            ctl: 0,
            sts: 0,
            lpib: 0,
            cbl: 0,
            lvi: 0,
            fifos: 256,
            fmt: 0,
            bdl_addr: 0,
            stream_num: index,
            is_output,
            stream: PcmStream::new(params, direction, 100),
        }
    }

    /// Reset stream
    pub fn reset(&mut self) {
        self.ctl = 0;
        self.sts = 0;
        self.lpib = 0;
        self.stream.reset();
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.ctl & StreamCtl::RUN != 0
    }

    /// Start stream
    pub fn start(&mut self) {
        self.ctl |= StreamCtl::RUN;
        self.stream.start();
    }

    /// Stop stream
    pub fn stop(&mut self) {
        self.ctl &= !StreamCtl::RUN;
        self.stream.stop();
    }

    /// Parse format register
    pub fn parse_format(&self) -> AudioParams {
        let base = if self.fmt & (1 << 14) != 0 {
            44100
        } else {
            48000
        };
        let mult = ((self.fmt >> 11) & 0x7) + 1;
        let div = ((self.fmt >> 8) & 0x7) + 1;
        let rate = (base * mult as u32) / div as u32;

        let bits = match (self.fmt >> 4) & 0x7 {
            0 => 8,
            1 => 16,
            2 => 20,
            3 => 24,
            4 => 32,
            _ => 16,
        };

        let channels = ((self.fmt & 0xF) + 1) as u32;

        let format = match bits {
            8 => SampleFormat::U8,
            16 => SampleFormat::S16Le,
            24 => SampleFormat::S24Le,
            32 => SampleFormat::S32Le,
            _ => SampleFormat::S16Le,
        };

        let sample_rate = SampleRate::from_hz(rate).unwrap_or(SampleRate::Hz48000);
        let layout = ChannelLayout::from_channels(channels).unwrap_or(ChannelLayout::Stereo);

        AudioParams::new(format, sample_rate, layout)
    }

    /// Write audio data
    pub fn write_audio(&mut self, data: &[u8]) -> usize {
        self.stream.write(data)
    }

    /// Read audio data
    pub fn read_audio(&mut self, buffer: &mut [u8]) -> usize {
        self.stream.read(buffer)
    }
}

/// Intel HDA controller
pub struct HdaController {
    /// Global capabilities
    gcap: u16,
    /// Version
    version: u16,
    /// Global control
    gctl: AtomicU32,
    /// Wake enable
    wakeen: u16,
    /// State change status
    statests: u16,
    /// Global status
    gsts: u16,
    /// Interrupt control
    intctl: AtomicU32,
    /// Interrupt status
    intsts: AtomicU32,
    /// Wall clock counter
    wallclk: AtomicU64,
    /// CORB base address
    corb_addr: u64,
    /// CORB write pointer
    corb_wp: u16,
    /// CORB read pointer
    corb_rp: u16,
    /// CORB control
    corb_ctl: u8,
    /// CORB status
    corb_sts: u8,
    /// CORB size
    corb_size: u8,
    /// RIRB base address
    rirb_addr: u64,
    /// RIRB write pointer
    rirb_wp: u16,
    /// RIRB interrupt count
    rint_cnt: u16,
    /// RIRB control
    rirb_ctl: u8,
    /// RIRB status
    rirb_sts: u8,
    /// RIRB size
    rirb_size: u8,
    /// DMA position base address
    dma_pos_addr: u64,
    /// Input streams
    input_streams: Vec<StreamDescriptor>,
    /// Output streams
    output_streams: Vec<StreamDescriptor>,
    /// Codecs
    codecs: Vec<HdaCodec>,
    /// Pending RIRB responses
    rirb_responses: Vec<(u32, u32)>,
    /// Pending CORB commands (queued via enqueue_corb_command)
    corb_commands: VecDeque<u32>,
    /// Statistics
    stats: AudioStats,
    /// Pending interrupt
    pending_interrupt: bool,
}

impl Default for HdaController {
    fn default() -> Self {
        Self::new()
    }
}

impl HdaController {
    /// Create new HDA controller
    pub fn new() -> Self {
        // Default: 4 input, 4 output streams
        let input_streams: Vec<_> = (0..4).map(|i| StreamDescriptor::new(i, false)).collect();
        let output_streams: Vec<_> = (0..4).map(|i| StreamDescriptor::new(i, true)).collect();

        let mut controller = Self {
            // GCAP: 4 ISS, 4 OSS, 0 BSS, 64-bit addresses, SDO signals
            gcap: (4 << 12) | (4 << 8) | (1 << 0),
            version: 0x0101, // Version 1.0
            gctl: AtomicU32::new(0),
            wakeen: 0,
            statests: 0,
            gsts: 0,
            intctl: AtomicU32::new(0),
            intsts: AtomicU32::new(0),
            wallclk: AtomicU64::new(0),
            corb_addr: 0,
            corb_wp: 0,
            corb_rp: 0,
            corb_ctl: 0,
            corb_sts: 0,
            corb_size: 0x02, // 256 entries
            rirb_addr: 0,
            rirb_wp: 0,
            rint_cnt: 0,
            rirb_ctl: 0,
            rirb_sts: 0,
            rirb_size: 0x02, // 256 entries
            dma_pos_addr: 0,
            input_streams,
            output_streams,
            codecs: Vec::new(),
            rirb_responses: Vec::new(),
            corb_commands: VecDeque::new(),
            stats: AudioStats::new(),
            pending_interrupt: false,
        };

        // Add default codec
        controller.codecs.push(HdaCodec::new(0));
        controller.statests = 0x0001; // Codec 0 present

        controller
    }

    /// Reset controller
    pub fn reset(&mut self) {
        self.gctl.store(0, Ordering::Relaxed);
        self.intctl.store(0, Ordering::Relaxed);
        self.intsts.store(0, Ordering::Relaxed);
        self.wallclk.store(0, Ordering::Relaxed);

        for stream in &mut self.input_streams {
            stream.reset();
        }
        for stream in &mut self.output_streams {
            stream.reset();
        }

        self.rirb_responses.clear();
        self.corb_commands.clear();
        self.pending_interrupt = false;
    }

    /// Read register (32-bit)
    pub fn read32(&self, offset: u32) -> u32 {
        match offset {
            Registers::GCAP => self.gcap as u32 | ((self.version as u32) << 16),
            Registers::GCTL => self.gctl.load(Ordering::Relaxed),
            Registers::WAKEEN => self.wakeen as u32,
            Registers::STATESTS => self.statests as u32,
            Registers::GSTS => self.gsts as u32,
            Registers::INTCTL => self.intctl.load(Ordering::Relaxed),
            Registers::INTSTS => self.intsts.load(Ordering::Relaxed),
            Registers::WALLCLK => (self.wallclk.load(Ordering::Relaxed) & 0xFFFFFFFF) as u32,
            Registers::CORBLBASE => (self.corb_addr & 0xFFFFFFFF) as u32,
            Registers::CORBUBASE => (self.corb_addr >> 32) as u32,
            Registers::CORBWP => self.corb_wp as u32,
            Registers::CORBRP => self.corb_rp as u32,
            Registers::RIRBLBASE => (self.rirb_addr & 0xFFFFFFFF) as u32,
            Registers::RIRBUBASE => (self.rirb_addr >> 32) as u32,
            Registers::RIRBWP => self.rirb_wp as u32,
            Registers::DPLBASE => (self.dma_pos_addr & 0xFFFFFFFF) as u32,
            Registers::DPUBASE => (self.dma_pos_addr >> 32) as u32,
            _ => {
                // Check for stream descriptor access
                if offset >= Registers::SD_BASE {
                    return self.read_stream_reg(offset);
                }
                0
            }
        }
    }

    /// Write register (32-bit)
    pub fn write32(&mut self, offset: u32, value: u32) {
        match offset {
            Registers::GCTL => {
                let old = self.gctl.load(Ordering::Relaxed);
                self.gctl.store(value, Ordering::Relaxed);

                // Handle reset
                if old & GlobalCtl::CRST == 0 && value & GlobalCtl::CRST != 0 {
                    // Coming out of reset
                    self.statests = if self.codecs.is_empty() { 0 } else { 0x0001 };
                }
            }
            Registers::WAKEEN => self.wakeen = value as u16,
            Registers::STATESTS => self.statests &= !(value as u16),
            Registers::INTCTL => self.intctl.store(value, Ordering::Relaxed),
            Registers::INTSTS => {
                // Write to clear
                let current = self.intsts.load(Ordering::Relaxed);
                self.intsts.store(current & !value, Ordering::Relaxed);
            }
            Registers::CORBLBASE => {
                self.corb_addr = (self.corb_addr & 0xFFFFFFFF_00000000) | (value as u64 & !0x7F);
            }
            Registers::CORBUBASE => {
                self.corb_addr = (self.corb_addr & 0x00000000_FFFFFFFF) | ((value as u64) << 32);
            }
            Registers::CORBWP => {
                self.corb_wp = (value as u16) & 0xFF;
                self.process_corb();
            }
            Registers::CORBRP => {
                if value & 0x8000 != 0 {
                    self.corb_rp = 0;
                }
            }
            Registers::RIRBLBASE => {
                self.rirb_addr = (self.rirb_addr & 0xFFFFFFFF_00000000) | (value as u64 & !0x7F);
            }
            Registers::RIRBUBASE => {
                self.rirb_addr = (self.rirb_addr & 0x00000000_FFFFFFFF) | ((value as u64) << 32);
            }
            Registers::RIRBWP => {
                if value & 0x8000 != 0 {
                    self.rirb_wp = 0;
                }
            }
            Registers::DPLBASE => {
                self.dma_pos_addr = (self.dma_pos_addr & 0xFFFFFFFF_00000000) | (value as u64);
            }
            Registers::DPUBASE => {
                self.dma_pos_addr =
                    (self.dma_pos_addr & 0x00000000_FFFFFFFF) | ((value as u64) << 32);
            }
            _ => {
                if offset >= Registers::SD_BASE {
                    self.write_stream_reg(offset, value);
                }
            }
        }
    }

    /// Read stream descriptor register
    fn read_stream_reg(&self, offset: u32) -> u32 {
        let stream_offset = offset - Registers::SD_BASE;
        let stream_idx = (stream_offset / Registers::SD_STRIDE) as usize;
        let reg_offset = stream_offset % Registers::SD_STRIDE;

        let stream = if stream_idx < 4 {
            &self.input_streams[stream_idx]
        } else if stream_idx < 8 {
            &self.output_streams[stream_idx - 4]
        } else {
            return 0;
        };

        match reg_offset {
            StreamReg::CTL => stream.ctl,
            StreamReg::STS => stream.sts as u32,
            StreamReg::LPIB => stream.lpib,
            StreamReg::CBL => stream.cbl,
            StreamReg::LVI => stream.lvi as u32,
            StreamReg::FIFOS => stream.fifos as u32,
            StreamReg::FMT => stream.fmt as u32,
            StreamReg::BDLPL => (stream.bdl_addr & 0xFFFFFFFF) as u32,
            StreamReg::BDLPU => (stream.bdl_addr >> 32) as u32,
            _ => 0,
        }
    }

    /// Write stream descriptor register
    fn write_stream_reg(&mut self, offset: u32, value: u32) {
        let stream_offset = offset - Registers::SD_BASE;
        let stream_idx = (stream_offset / Registers::SD_STRIDE) as usize;
        let reg_offset = stream_offset % Registers::SD_STRIDE;

        let stream = if stream_idx < 4 {
            &mut self.input_streams[stream_idx]
        } else if stream_idx < 8 {
            &mut self.output_streams[stream_idx - 4]
        } else {
            return;
        };

        match reg_offset {
            StreamReg::CTL => {
                if value & StreamCtl::SRST != 0 {
                    stream.reset();
                } else {
                    let was_running = stream.is_running();
                    stream.ctl = value & !StreamCtl::SRST;

                    if !was_running && stream.ctl & StreamCtl::RUN != 0 {
                        stream.start();
                    } else if was_running && stream.ctl & StreamCtl::RUN == 0 {
                        stream.stop();
                    }
                }
            }
            StreamReg::STS => {
                stream.sts &= !(value as u8 & 0x1C);
            }
            StreamReg::CBL => stream.cbl = value,
            StreamReg::LVI => stream.lvi = value as u16,
            StreamReg::FMT => stream.fmt = value as u16,
            StreamReg::BDLPL => {
                stream.bdl_addr = (stream.bdl_addr & 0xFFFFFFFF_00000000) | (value as u64 & !0x7F);
            }
            StreamReg::BDLPU => {
                stream.bdl_addr = (stream.bdl_addr & 0x00000000_FFFFFFFF) | ((value as u64) << 32);
            }
            _ => {}
        }
    }

    /// Enqueue a CORB command for processing
    ///
    /// CORB entry format (32 bits):
    ///   `[31:28]` Codec Address
    ///   `[27:20]` Node ID (NID)
    ///   `[19:0]`  Verb payload
    pub fn enqueue_corb_command(&mut self, entry: u32) {
        self.corb_commands.push_back(entry);
        self.corb_wp = (self.corb_wp + 1) % 256;
    }

    /// Process CORB commands
    ///
    /// Parses each CORB entry to extract the codec address and verb,
    /// then dispatches to the appropriate codec for processing.
    fn process_corb(&mut self) {
        while self.corb_rp != self.corb_wp {
            self.corb_rp = (self.corb_rp + 1) % 256;

            // Try to get the command from the pending queue
            let entry = self.corb_commands.pop_front();

            // Parse CORB entry format:
            //   [31:28] = codec address
            //   [27:20] = NID (node ID)
            //   [19:0]  = verb payload
            let (codec_addr, verb) = if let Some(cmd) = entry {
                let cad = ((cmd >> 28) & 0x0F) as usize;
                // The verb includes NID and payload: bits [27:0]
                let verb = cmd & 0x0FFF_FFFF;
                (cad, verb)
            } else {
                // No command queued — use codec 0 with a NOP
                (0, 0)
            };

            if let Some(codec) = self.codecs.get_mut(codec_addr) {
                let response = codec.process_verb(verb);
                // RIRB response format: (response, solicited response + codec_addr)
                self.rirb_responses.push((response, codec_addr as u32));
                self.rirb_wp = (self.rirb_wp + 1) % 256;

                // Check for RIRB interrupt
                if self.rirb_ctl & 0x01 != 0 {
                    self.rirb_sts |= 0x01;
                    self.generate_interrupt();
                }
            }
        }
    }

    /// Generate interrupt
    fn generate_interrupt(&mut self) {
        let intctl = self.intctl.load(Ordering::Relaxed);
        if intctl & (1 << 31) != 0 {
            self.pending_interrupt = true;
            self.stats.record_interrupt();
        }
    }

    /// Check for pending interrupt
    pub fn has_interrupt(&self) -> bool {
        self.pending_interrupt
    }

    /// Clear pending interrupt
    pub fn clear_interrupt(&mut self) {
        self.pending_interrupt = false;
    }

    /// Get codec
    pub fn codec(&self, index: usize) -> Option<&HdaCodec> {
        self.codecs.get(index)
    }

    /// Get mutable codec
    pub fn codec_mut(&mut self, index: usize) -> Option<&mut HdaCodec> {
        self.codecs.get_mut(index)
    }

    /// Get output stream
    pub fn output_stream(&self, index: usize) -> Option<&StreamDescriptor> {
        self.output_streams.get(index)
    }

    /// Get mutable output stream
    pub fn output_stream_mut(&mut self, index: usize) -> Option<&mut StreamDescriptor> {
        self.output_streams.get_mut(index)
    }

    /// Write audio to output stream
    pub fn write_audio(&mut self, stream_idx: usize, data: &[u8]) -> usize {
        if let Some(stream) = self.output_streams.get_mut(stream_idx) {
            let written = stream.write_audio(data);
            self.stats.record_played(written as u64);
            written
        } else {
            0
        }
    }

    /// Read audio from input stream
    pub fn read_audio(&mut self, stream_idx: usize, buffer: &mut [u8]) -> usize {
        if let Some(stream) = self.input_streams.get_mut(stream_idx) {
            let read = stream.read_audio(buffer);
            self.stats.record_captured(read as u64);
            read
        } else {
            0
        }
    }

    /// Advance wall clock
    pub fn tick(&mut self, samples: u64) {
        // Wall clock runs at 24MHz
        let clocks = (samples * 24_000_000) / 48000;
        self.wallclk.fetch_add(clocks, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn stats(&self) -> &AudioStats {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_widget_type_from_caps() {
        assert_eq!(
            WidgetType::from_caps(0x00000000),
            Some(WidgetType::AudioOutput)
        );
        assert_eq!(
            WidgetType::from_caps(0x00100000),
            Some(WidgetType::AudioInput)
        );
        assert_eq!(
            WidgetType::from_caps(0x00400000),
            Some(WidgetType::PinComplex)
        );
    }

    #[test]
    fn test_pin_config() {
        let line_out = PinConfig::line_out();
        assert_eq!(line_out.default_device(), 0); // Line out
        assert_eq!(line_out.sequence(), 0);

        let hp = PinConfig::headphone();
        assert_eq!(hp.default_device(), 2); // Headphone

        let mic = PinConfig::mic_in();
        assert_eq!(mic.default_device(), 10); // Mic in
    }

    #[test]
    fn test_widget_creation() {
        let dac = Widget::audio_output(2);
        assert_eq!(dac.nid, 2);
        assert_eq!(dac.widget_type, WidgetType::AudioOutput);

        let adc = Widget::audio_input(3);
        assert_eq!(adc.widget_type, WidgetType::AudioInput);
    }

    #[test]
    fn test_widget_mixer() {
        let mixer = Widget::mixer(4, vec![2, 3]);
        assert_eq!(mixer.connections.len(), 2);
        assert_eq!(mixer.amp_in.len(), 2);
    }

    #[test]
    fn test_codec_creation() {
        let codec = HdaCodec::new(0);
        assert_eq!(codec.address, 0);
        assert!(!codec.widgets.is_empty());
    }

    #[test]
    fn test_codec_parameters() {
        let codec = HdaCodec::new(0);

        // Vendor ID
        let vendor = codec.get_parameter(0, 0x00);
        assert_eq!(vendor, 0x8086_2668);

        // Subordinate node count
        let nodes = codec.get_parameter(0, 0x04);
        assert!((nodes >> 16) > 0);
    }

    #[test]
    fn test_codec_process_verb() {
        let mut codec = HdaCodec::new(0);

        // Get parameter (vendor ID)
        let response = codec.process_verb(0x000F0000);
        assert_eq!(response, codec.vendor_id);
    }

    #[test]
    fn test_stream_descriptor_creation() {
        let sd = StreamDescriptor::new(0, true);
        assert!(sd.is_output);
        assert!(!sd.is_running());
    }

    #[test]
    fn test_stream_descriptor_start_stop() {
        let mut sd = StreamDescriptor::new(0, true);

        sd.start();
        assert!(sd.is_running());

        sd.stop();
        assert!(!sd.is_running());
    }

    #[test]
    fn test_stream_descriptor_format() {
        let mut sd = StreamDescriptor::new(0, true);
        // 48kHz, 16-bit, stereo
        sd.fmt = 0x0011;

        let params = sd.parse_format();
        assert_eq!(params.rate, SampleRate::Hz48000);
        assert_eq!(params.format, SampleFormat::S16Le);
        assert_eq!(params.channels, ChannelLayout::Stereo);
    }

    #[test]
    fn test_controller_creation() {
        let controller = HdaController::new();
        assert!(!controller.has_interrupt());
        assert_eq!(controller.codecs.len(), 1);
    }

    #[test]
    fn test_controller_reset() {
        let mut controller = HdaController::new();
        controller.write32(Registers::GCTL, GlobalCtl::CRST);

        controller.reset();

        assert_eq!(controller.gctl.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_controller_gcap() {
        let controller = HdaController::new();
        let gcap = controller.read32(Registers::GCAP);

        // Check input stream count (bits 12-15)
        assert_eq!((gcap >> 12) & 0xF, 4);
        // Check output stream count (bits 8-11)
        assert_eq!((gcap >> 8) & 0xF, 4);
    }

    #[test]
    fn test_controller_gctl() {
        let mut controller = HdaController::new();

        controller.write32(Registers::GCTL, GlobalCtl::CRST);
        assert_eq!(
            controller.read32(Registers::GCTL) & GlobalCtl::CRST,
            GlobalCtl::CRST
        );
    }

    #[test]
    fn test_controller_statests() {
        let controller = HdaController::new();
        let statests = controller.read32(Registers::STATESTS);

        // Codec 0 present
        assert_eq!(statests & 0x1, 1);
    }

    #[test]
    fn test_controller_corb_rirb() {
        let mut controller = HdaController::new();

        controller.write32(Registers::CORBLBASE, 0x1000);
        assert_eq!(controller.read32(Registers::CORBLBASE), 0x1000 & !0x7F);

        controller.write32(Registers::RIRBLBASE, 0x2000);
        assert_eq!(controller.read32(Registers::RIRBLBASE), 0x2000 & !0x7F);
    }

    #[test]
    fn test_controller_stream_reg() {
        let mut controller = HdaController::new();

        // Write to output stream 0 CBL
        let offset = Registers::SD_BASE + 4 * Registers::SD_STRIDE + StreamReg::CBL;
        controller.write32(offset, 0x8000);
        assert_eq!(controller.read32(offset), 0x8000);
    }

    #[test]
    fn test_controller_stream_format() {
        let mut controller = HdaController::new();

        let offset = Registers::SD_BASE + 4 * Registers::SD_STRIDE + StreamReg::FMT;
        controller.write32(offset, 0x0011);
        assert_eq!(controller.read32(offset), 0x0011);
    }

    #[test]
    fn test_controller_write_audio() {
        let mut controller = HdaController::new();
        if let Some(stream) = controller.output_stream_mut(0) {
            stream.start();
        }

        let data = vec![0u8; 1024];
        let written = controller.write_audio(0, &data);
        assert!(written > 0);
    }

    #[test]
    fn test_controller_tick() {
        let controller = HdaController::new();
        let before = controller.wallclk.load(Ordering::Relaxed);

        let mut controller = controller;
        controller.tick(48000); // 1 second of audio

        let after = controller.wallclk.load(Ordering::Relaxed);
        assert!(after > before);
    }

    #[test]
    fn test_controller_stats() {
        let mut controller = HdaController::new();
        if let Some(stream) = controller.output_stream_mut(0) {
            stream.start();
        }

        let data = vec![0u8; 100];
        controller.write_audio(0, &data);

        let snapshot = controller.stats().snapshot();
        assert!(snapshot.bytes_played > 0);
    }
}
