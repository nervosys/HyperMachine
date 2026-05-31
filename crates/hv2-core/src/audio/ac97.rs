//! AC'97 Audio Codec Emulation
//!
//! This module provides AC'97 audio controller emulation with
//! PCM playback/capture streams and mixer control.

use super::core::{
    AudioParams, AudioStats, AudioStream, ChannelLayout, PcmStream, SampleFormat, SampleRate,
    StereoVolume, StreamDirection, Volume,
};
use std::sync::atomic::{AtomicU32, Ordering};

/// AC97 codec register addresses
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Ac97Register {
    /// Reset
    Reset = 0x00,
    /// Master Volume
    MasterVolume = 0x02,
    /// Aux Out Volume
    AuxOutVolume = 0x04,
    /// Mono Volume
    MonoVolume = 0x06,
    /// Master Tone
    MasterTone = 0x08,
    /// PC Beep Volume
    PcBeepVolume = 0x0A,
    /// Phone Volume
    PhoneVolume = 0x0C,
    /// Mic Volume
    MicVolume = 0x0E,
    /// Line In Volume
    LineInVolume = 0x10,
    /// CD Volume
    CdVolume = 0x12,
    /// Video Volume
    VideoVolume = 0x14,
    /// Aux In Volume
    AuxInVolume = 0x16,
    /// PCM Out Volume
    PcmOutVolume = 0x18,
    /// Record Select
    RecordSelect = 0x1A,
    /// Record Gain
    RecordGain = 0x1C,
    /// Record Gain Mic
    RecordGainMic = 0x1E,
    /// General Purpose
    GeneralPurpose = 0x20,
    /// 3D Control
    Control3d = 0x22,
    /// Audio Interrupt and Paging
    AudioIntPaging = 0x24,
    /// Powerdown Control/Status
    Powerdown = 0x26,
    /// Extended Audio ID
    ExtendedAudioId = 0x28,
    /// Extended Audio Status/Control
    ExtendedAudioCtl = 0x2A,
    /// PCM Front DAC Rate
    PcmFrontDacRate = 0x2C,
    /// PCM Surround DAC Rate
    PcmSurroundDacRate = 0x2E,
    /// PCM LFE DAC Rate
    PcmLfeDacRate = 0x30,
    /// PCM LR ADC Rate
    PcmLrAdcRate = 0x32,
    /// Mic ADC Rate
    MicAdcRate = 0x34,
    /// Center/LFE Volume
    CenterLfeVolume = 0x36,
    /// Surround Volume
    SurroundVolume = 0x38,
    /// S/PDIF Control
    SpdifControl = 0x3A,
    /// Vendor ID1
    VendorId1 = 0x7C,
    /// Vendor ID2
    VendorId2 = 0x7E,
}

impl Ac97Register {
    /// Create from register offset
    pub fn from_offset(offset: u8) -> Option<Self> {
        match offset {
            0x00 => Some(Ac97Register::Reset),
            0x02 => Some(Ac97Register::MasterVolume),
            0x04 => Some(Ac97Register::AuxOutVolume),
            0x06 => Some(Ac97Register::MonoVolume),
            0x08 => Some(Ac97Register::MasterTone),
            0x0A => Some(Ac97Register::PcBeepVolume),
            0x0C => Some(Ac97Register::PhoneVolume),
            0x0E => Some(Ac97Register::MicVolume),
            0x10 => Some(Ac97Register::LineInVolume),
            0x12 => Some(Ac97Register::CdVolume),
            0x14 => Some(Ac97Register::VideoVolume),
            0x16 => Some(Ac97Register::AuxInVolume),
            0x18 => Some(Ac97Register::PcmOutVolume),
            0x1A => Some(Ac97Register::RecordSelect),
            0x1C => Some(Ac97Register::RecordGain),
            0x1E => Some(Ac97Register::RecordGainMic),
            0x20 => Some(Ac97Register::GeneralPurpose),
            0x22 => Some(Ac97Register::Control3d),
            0x24 => Some(Ac97Register::AudioIntPaging),
            0x26 => Some(Ac97Register::Powerdown),
            0x28 => Some(Ac97Register::ExtendedAudioId),
            0x2A => Some(Ac97Register::ExtendedAudioCtl),
            0x2C => Some(Ac97Register::PcmFrontDacRate),
            0x2E => Some(Ac97Register::PcmSurroundDacRate),
            0x30 => Some(Ac97Register::PcmLfeDacRate),
            0x32 => Some(Ac97Register::PcmLrAdcRate),
            0x34 => Some(Ac97Register::MicAdcRate),
            0x36 => Some(Ac97Register::CenterLfeVolume),
            0x38 => Some(Ac97Register::SurroundVolume),
            0x3A => Some(Ac97Register::SpdifControl),
            0x7C => Some(Ac97Register::VendorId1),
            0x7E => Some(Ac97Register::VendorId2),
            _ => None,
        }
    }
}

/// AC97 bus master register offsets
pub mod BusMaster {
    /// PCM Input buffer descriptor base address
    pub const PI_BDBAR: u32 = 0x00;
    /// PCM Input current index value
    pub const PI_CIV: u32 = 0x04;
    /// PCM Input last valid index
    pub const PI_LVI: u32 = 0x05;
    /// PCM Input status register
    pub const PI_SR: u32 = 0x06;
    /// PCM Input position in current buffer
    pub const PI_PICB: u32 = 0x08;
    /// PCM Input prefetched index value
    pub const PI_PIV: u32 = 0x0A;
    /// PCM Input control register
    pub const PI_CR: u32 = 0x0B;

    /// PCM Output buffer descriptor base address
    pub const PO_BDBAR: u32 = 0x10;
    /// PCM Output current index value
    pub const PO_CIV: u32 = 0x14;
    /// PCM Output last valid index
    pub const PO_LVI: u32 = 0x15;
    /// PCM Output status register
    pub const PO_SR: u32 = 0x16;
    /// PCM Output position in current buffer
    pub const PO_PICB: u32 = 0x18;
    /// PCM Output prefetched index value
    pub const PO_PIV: u32 = 0x1A;
    /// PCM Output control register
    pub const PO_CR: u32 = 0x1B;

    /// Mic Input buffer descriptor base address
    pub const MC_BDBAR: u32 = 0x20;
    /// Mic Input current index value
    pub const MC_CIV: u32 = 0x24;
    /// Mic Input last valid index
    pub const MC_LVI: u32 = 0x25;
    /// Mic Input status register
    pub const MC_SR: u32 = 0x26;
    /// Mic Input position in current buffer
    pub const MC_PICB: u32 = 0x28;
    /// Mic Input prefetched index value
    pub const MC_PIV: u32 = 0x2A;
    /// Mic Input control register
    pub const MC_CR: u32 = 0x2B;

    /// Global control register
    pub const GLOB_CNT: u32 = 0x2C;
    /// Global status register
    pub const GLOB_STA: u32 = 0x30;
}

/// Status register bits
pub mod StatusBits {
    /// DMA controller halted
    pub const DCH: u16 = 1 << 0;
    /// Codec ready
    pub const CELV: u16 = 1 << 1;
    /// Last valid buffer completion interrupt
    pub const LVBCI: u16 = 1 << 2;
    /// Buffer completion interrupt
    pub const BCIS: u16 = 1 << 3;
    /// FIFO error
    pub const FIFOE: u16 = 1 << 4;
}

/// Control register bits
pub mod ControlBits {
    /// Run/Pause bus master
    pub const RPBM: u8 = 1 << 0;
    /// Reset registers
    pub const RR: u8 = 1 << 1;
    /// Last valid buffer interrupt enable
    pub const LVBIE: u8 = 1 << 2;
    /// Buffer completion interrupt enable
    pub const IOCE: u8 = 1 << 3;
    /// FIFO error interrupt enable
    pub const FEIE: u8 = 1 << 4;
}

/// Global control register bits
pub mod GlobalControl {
    /// Global interrupt enable
    pub const GIE: u32 = 1 << 0;
    /// Cold reset
    pub const COLD: u32 = 1 << 1;
    /// Warm reset
    pub const WARM: u32 = 1 << 2;
    /// Shut down
    pub const SHUT: u32 = 1 << 3;
    /// Primary resume interrupt
    pub const PRIE: u32 = 1 << 4;
    /// Secondary resume interrupt
    pub const SRIE: u32 = 1 << 5;
    /// AC link powerdown
    pub const ACLINK: u32 = 1 << 6;
    /// 2 channels
    pub const PCM_2: u32 = 0 << 20;
    /// 4 channels
    pub const PCM_4: u32 = 1 << 20;
    /// 6 channels
    pub const PCM_6: u32 = 2 << 20;
}

/// Global status register bits
pub mod GlobalStatus {
    /// Mic input channel ready
    pub const MIINT: u32 = 1 << 0;
    /// Modem input channel ready
    pub const MOINT: u32 = 1 << 1;
    /// PCM input channel ready
    pub const PIINT: u32 = 1 << 2;
    /// PCM output channel ready
    pub const POINT: u32 = 1 << 3;
    /// Modem output channel ready
    pub const MINT: u32 = 1 << 4;
    /// Global status write clear
    pub const GSCI: u32 = 1 << 6;
    /// Secondary codec ready
    pub const S2RI: u32 = 1 << 11;
    /// Primary codec ready
    pub const PRI: u32 = 1 << 13;
    /// Secondary resume interrupt
    pub const SCR: u32 = 1 << 14;
    /// Primary resume interrupt
    pub const PCR: u32 = 1 << 15;
    /// Multichannel capable
    pub const MC: u32 = 1 << 16;
    /// Primary codec ready
    pub const MD: u32 = 1 << 17;
    /// Primary codec attached
    pub const AD: u32 = 1 << 18;
    /// Secondary codec attached
    pub const S2AD: u32 = 1 << 19;
    /// Read completion status
    pub const RCS: u32 = 1 << 28;
    /// Bits per slot
    pub const BIT32: u32 = 1 << 29;
    /// AC97 spec version
    pub const VER_20: u32 = 0 << 30;
    pub const VER_21: u32 = 1 << 30;
    pub const VER_22: u32 = 2 << 30;
    pub const VER_23: u32 = 3 << 30;
}

/// Buffer descriptor entry
#[derive(Debug, Clone, Copy, Default)]
pub struct BufferDescriptor {
    /// Physical address of buffer
    pub address: u32,
    /// Number of samples (not bytes)
    pub samples: u16,
    /// Flags
    pub flags: u16,
}

impl BufferDescriptor {
    /// Buffer completion interrupt
    pub const BUP: u16 = 1 << 14;
    /// Interrupt on completion
    pub const IOC: u16 = 1 << 15;

    /// Create from memory
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(Self {
            address: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            samples: u16::from_le_bytes([bytes[4], bytes[5]]),
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
        })
    }

    /// Check if interrupt on completion
    pub fn interrupt_on_completion(&self) -> bool {
        self.flags & Self::IOC != 0
    }

    /// Check if buffer underrun protection
    pub fn buffer_underrun_protection(&self) -> bool {
        self.flags & Self::BUP != 0
    }

    /// Get byte length
    pub fn byte_length(&self) -> usize {
        // Each sample is 16-bit stereo = 4 bytes
        self.samples as usize * 4
    }
}

/// DMA channel state
#[derive(Debug, Clone)]
pub struct DmaChannel {
    /// Buffer descriptor list base address
    pub bdbar: u32,
    /// Current index value
    pub civ: u8,
    /// Last valid index
    pub lvi: u8,
    /// Status register
    pub sr: u16,
    /// Position in current buffer (samples)
    pub picb: u16,
    /// Prefetched index value
    pub piv: u8,
    /// Control register
    pub cr: u8,
    /// Channel direction
    direction: StreamDirection,
}

impl DmaChannel {
    /// Create new DMA channel
    pub fn new(direction: StreamDirection) -> Self {
        Self {
            bdbar: 0,
            civ: 0,
            lvi: 0,
            sr: StatusBits::DCH, // Halted initially
            picb: 0,
            piv: 0,
            cr: 0,
            direction,
        }
    }

    /// Reset channel
    pub fn reset(&mut self) {
        self.bdbar = 0;
        self.civ = 0;
        self.lvi = 0;
        self.sr = StatusBits::DCH;
        self.picb = 0;
        self.piv = 0;
        self.cr = 0;
    }

    /// Check if running
    pub fn is_running(&self) -> bool {
        self.cr & ControlBits::RPBM != 0
    }

    /// Check if halted
    pub fn is_halted(&self) -> bool {
        self.sr & StatusBits::DCH != 0
    }

    /// Start channel
    pub fn start(&mut self) {
        self.cr |= ControlBits::RPBM;
        self.sr &= !StatusBits::DCH;
    }

    /// Stop channel
    pub fn stop(&mut self) {
        self.cr &= !ControlBits::RPBM;
        self.sr |= StatusBits::DCH;
    }

    /// Clear status bits
    pub fn clear_status(&mut self, bits: u16) {
        self.sr &= !bits;
    }

    /// Set status bits
    pub fn set_status(&mut self, bits: u16) {
        self.sr |= bits;
    }

    /// Check if interrupt enabled
    pub fn interrupt_enabled(&self, status_bit: u16) -> bool {
        match status_bit {
            StatusBits::LVBCI => self.cr & ControlBits::LVBIE != 0,
            StatusBits::BCIS => self.cr & ControlBits::IOCE != 0,
            StatusBits::FIFOE => self.cr & ControlBits::FEIE != 0,
            _ => false,
        }
    }

    /// Advance to next buffer
    pub fn advance_buffer(&mut self) {
        self.civ = (self.civ + 1) % 32;
        self.picb = 0;

        if self.civ == self.lvi {
            self.set_status(StatusBits::LVBCI);
            self.stop();
        }
    }
}

/// AC97 mixer state
#[derive(Debug)]
pub struct Ac97Mixer {
    /// Codec registers
    registers: [u16; 64],
    /// Master volume
    master_volume: StereoVolume,
    /// PCM out volume
    pcm_volume: StereoVolume,
    /// Sample rate
    sample_rate: u32,
}

impl Default for Ac97Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Ac97Mixer {
    /// Create new mixer
    pub fn new() -> Self {
        let mut mixer = Self {
            registers: [0u16; 64],
            master_volume: StereoVolume::MAX,
            pcm_volume: StereoVolume::MAX,
            sample_rate: 48000,
        };
        mixer.reset();
        mixer
    }

    /// Reset mixer to defaults
    pub fn reset(&mut self) {
        // Initialize default register values
        self.registers.fill(0);

        // Extended audio ID - VRA (variable rate audio) supported
        self.registers[0x28 / 2] = 0x0001;
        // Extended audio status - VRA enabled
        self.registers[0x2A / 2] = 0x0001;
        // Default sample rate
        self.registers[0x2C / 2] = 48000;
        self.registers[0x32 / 2] = 48000;
        self.registers[0x34 / 2] = 48000;

        // Vendor ID (Intel ICH)
        self.registers[0x7C / 2] = 0x4144; // "AD"
        self.registers[0x7E / 2] = 0x5370; // "Sp"

        self.master_volume = StereoVolume::MAX;
        self.pcm_volume = StereoVolume::MAX;
        self.sample_rate = 48000;
    }

    /// Read register
    pub fn read(&self, offset: u8) -> u16 {
        let index = (offset as usize) / 2;
        if index < self.registers.len() {
            self.registers[index]
        } else {
            0
        }
    }

    /// Write register
    pub fn write(&mut self, offset: u8, value: u16) {
        let index = (offset as usize) / 2;
        if index >= self.registers.len() {
            return;
        }

        match Ac97Register::from_offset(offset) {
            Some(Ac97Register::Reset) => {
                self.reset();
            }
            Some(Ac97Register::MasterVolume) => {
                self.registers[index] = value;
                self.update_master_volume(value);
            }
            Some(Ac97Register::PcmOutVolume) => {
                self.registers[index] = value;
                self.update_pcm_volume(value);
            }
            Some(Ac97Register::PcmFrontDacRate) => {
                let rate = value.clamp(8000, 48000);
                self.registers[index] = rate;
                self.sample_rate = rate as u32;
            }
            Some(Ac97Register::VendorId1) | Some(Ac97Register::VendorId2) => {
                // Read-only
            }
            _ => {
                self.registers[index] = value;
            }
        }
    }

    /// Update master volume from register value
    fn update_master_volume(&mut self, value: u16) {
        let mute = (value & 0x8000) != 0;
        let left_attn = ((value >> 8) & 0x3F) as u8;
        let right_attn = (value & 0x3F) as u8;

        // Attenuation is in 1.5dB steps, 0 = 0dB, 63 = -94.5dB
        // Convert to linear volume (simplified)
        let left_vol = if left_attn >= 63 {
            0
        } else {
            255 - (left_attn * 4)
        };
        let right_vol = if right_attn >= 63 {
            0
        } else {
            255 - (right_attn * 4)
        };

        self.master_volume = StereoVolume {
            left: Volume(left_vol),
            right: Volume(right_vol),
            muted: mute,
        };
    }

    /// Update PCM volume from register value
    fn update_pcm_volume(&mut self, value: u16) {
        let mute = (value & 0x8000) != 0;
        let left_attn = ((value >> 8) & 0x1F) as u8;
        let right_attn = (value & 0x1F) as u8;

        // PCM attenuation is 1.5dB steps, 0-31
        let left_vol = 255 - (left_attn * 8);
        let right_vol = 255 - (right_attn * 8);

        self.pcm_volume = StereoVolume {
            left: Volume(left_vol),
            right: Volume(right_vol),
            muted: mute,
        };
    }

    /// Get master volume
    pub fn master_volume(&self) -> StereoVolume {
        self.master_volume
    }

    /// Get PCM volume
    pub fn pcm_volume(&self) -> StereoVolume {
        self.pcm_volume
    }

    /// Get sample rate
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

/// AC97 audio controller
pub struct Ac97Controller {
    /// Mixer state
    mixer: Ac97Mixer,
    /// PCM input channel
    pi_channel: DmaChannel,
    /// PCM output channel
    po_channel: DmaChannel,
    /// Mic input channel
    mc_channel: DmaChannel,
    /// Global control register
    glob_cnt: AtomicU32,
    /// Global status register
    glob_sta: AtomicU32,
    /// PCM output stream
    output_stream: PcmStream,
    /// PCM input stream
    input_stream: PcmStream,
    /// Statistics
    stats: AudioStats,
    /// Pending interrupt
    pending_interrupt: bool,
}

impl Default for Ac97Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Ac97Controller {
    /// Create new AC97 controller
    pub fn new() -> Self {
        let params = AudioParams::new(
            SampleFormat::S16Le,
            SampleRate::Hz48000,
            ChannelLayout::Stereo,
        );

        Self {
            mixer: Ac97Mixer::new(),
            pi_channel: DmaChannel::new(StreamDirection::Capture),
            po_channel: DmaChannel::new(StreamDirection::Playback),
            mc_channel: DmaChannel::new(StreamDirection::Capture),
            glob_cnt: AtomicU32::new(0),
            glob_sta: AtomicU32::new(GlobalStatus::PRI | GlobalStatus::AD | GlobalStatus::VER_21),
            output_stream: PcmStream::new(params, StreamDirection::Playback, 100),
            input_stream: PcmStream::new(params, StreamDirection::Capture, 100),
            stats: AudioStats::new(),
            pending_interrupt: false,
        }
    }

    /// Reset controller
    pub fn reset(&mut self) {
        self.mixer.reset();
        self.pi_channel.reset();
        self.po_channel.reset();
        self.mc_channel.reset();
        self.glob_cnt.store(0, Ordering::Relaxed);
        self.glob_sta.store(
            GlobalStatus::PRI | GlobalStatus::AD | GlobalStatus::VER_21,
            Ordering::Relaxed,
        );
        self.output_stream.reset();
        self.input_stream.reset();
        self.pending_interrupt = false;
    }

    /// Get mixer reference
    pub fn mixer(&self) -> &Ac97Mixer {
        &self.mixer
    }

    /// Get mutable mixer reference
    pub fn mixer_mut(&mut self) -> &mut Ac97Mixer {
        &mut self.mixer
    }

    /// Read mixer register
    pub fn read_mixer(&self, offset: u8) -> u16 {
        self.mixer.read(offset)
    }

    /// Write mixer register
    pub fn write_mixer(&mut self, offset: u8, value: u16) {
        self.mixer.write(offset, value);
    }

    /// Read bus master register (8-bit)
    pub fn read_bm8(&self, offset: u32) -> u8 {
        match offset {
            BusMaster::PI_CIV => self.pi_channel.civ,
            BusMaster::PI_LVI => self.pi_channel.lvi,
            BusMaster::PI_PIV => self.pi_channel.piv,
            BusMaster::PI_CR => self.pi_channel.cr,
            BusMaster::PO_CIV => self.po_channel.civ,
            BusMaster::PO_LVI => self.po_channel.lvi,
            BusMaster::PO_PIV => self.po_channel.piv,
            BusMaster::PO_CR => self.po_channel.cr,
            BusMaster::MC_CIV => self.mc_channel.civ,
            BusMaster::MC_LVI => self.mc_channel.lvi,
            BusMaster::MC_PIV => self.mc_channel.piv,
            BusMaster::MC_CR => self.mc_channel.cr,
            _ => 0,
        }
    }

    /// Write bus master register (8-bit)
    pub fn write_bm8(&mut self, offset: u32, value: u8) {
        match offset {
            BusMaster::PI_LVI => self.pi_channel.lvi = value & 0x1F,
            BusMaster::PI_CR => self.handle_control_write(&mut self.pi_channel.clone(), value),
            BusMaster::PO_LVI => self.po_channel.lvi = value & 0x1F,
            BusMaster::PO_CR => {
                let mut channel = self.po_channel.clone();
                self.handle_control_write(&mut channel, value);
                self.po_channel = channel;
            }
            BusMaster::MC_LVI => self.mc_channel.lvi = value & 0x1F,
            BusMaster::MC_CR => self.handle_control_write(&mut self.mc_channel.clone(), value),
            _ => {}
        }
    }

    /// Handle control register write
    fn handle_control_write(&mut self, channel: &mut DmaChannel, value: u8) {
        if value & ControlBits::RR != 0 {
            channel.reset();
            return;
        }

        let was_running = channel.is_running();
        channel.cr = value & !ControlBits::RR;

        if !was_running && channel.is_running() {
            // Start DMA
            channel.sr &= !StatusBits::DCH;
            match channel.direction {
                StreamDirection::Playback => {
                    self.output_stream.start();
                }
                StreamDirection::Capture => {
                    self.input_stream.start();
                }
            }
        } else if was_running && !channel.is_running() {
            // Stop DMA
            channel.sr |= StatusBits::DCH;
            match channel.direction {
                StreamDirection::Playback => {
                    self.output_stream.stop();
                }
                StreamDirection::Capture => {
                    self.input_stream.stop();
                }
            }
        }
    }

    /// Read bus master register (16-bit)
    pub fn read_bm16(&self, offset: u32) -> u16 {
        match offset {
            BusMaster::PI_SR => self.pi_channel.sr,
            BusMaster::PI_PICB => self.pi_channel.picb,
            BusMaster::PO_SR => self.po_channel.sr,
            BusMaster::PO_PICB => self.po_channel.picb,
            BusMaster::MC_SR => self.mc_channel.sr,
            BusMaster::MC_PICB => self.mc_channel.picb,
            _ => 0,
        }
    }

    /// Write bus master register (16-bit)
    pub fn write_bm16(&mut self, offset: u32, value: u16) {
        match offset {
            BusMaster::PI_SR => self.pi_channel.clear_status(value & 0x1C),
            BusMaster::PO_SR => self.po_channel.clear_status(value & 0x1C),
            BusMaster::MC_SR => self.mc_channel.clear_status(value & 0x1C),
            _ => {}
        }
    }

    /// Read bus master register (32-bit)
    pub fn read_bm32(&self, offset: u32) -> u32 {
        match offset {
            BusMaster::PI_BDBAR => self.pi_channel.bdbar,
            BusMaster::PO_BDBAR => self.po_channel.bdbar,
            BusMaster::MC_BDBAR => self.mc_channel.bdbar,
            BusMaster::GLOB_CNT => self.glob_cnt.load(Ordering::Relaxed),
            BusMaster::GLOB_STA => self.glob_sta.load(Ordering::Relaxed),
            _ => 0,
        }
    }

    /// Write bus master register (32-bit)
    pub fn write_bm32(&mut self, offset: u32, value: u32) {
        match offset {
            BusMaster::PI_BDBAR => self.pi_channel.bdbar = value & !0x7,
            BusMaster::PO_BDBAR => self.po_channel.bdbar = value & !0x7,
            BusMaster::MC_BDBAR => self.mc_channel.bdbar = value & !0x7,
            BusMaster::GLOB_CNT => {
                self.glob_cnt.store(value, Ordering::Relaxed);
                if value & GlobalControl::COLD != 0 {
                    self.reset();
                }
            }
            BusMaster::GLOB_STA => {
                // Write to clear bits
                let current = self.glob_sta.load(Ordering::Relaxed);
                self.glob_sta
                    .store(current & !(value & 0x7F), Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Write audio data to output
    pub fn write_audio(&mut self, data: &[u8]) -> usize {
        let written = self.output_stream.write(data);
        self.stats.record_played(written as u64);
        written
    }

    /// Read audio data from input
    pub fn read_audio(&mut self, buffer: &mut [u8]) -> usize {
        let read = self.input_stream.read(buffer);
        self.stats.record_captured(read as u64);
        read
    }

    /// Check for pending interrupt
    pub fn has_interrupt(&self) -> bool {
        self.pending_interrupt
    }

    /// Clear pending interrupt
    pub fn clear_interrupt(&mut self) {
        self.pending_interrupt = false;
    }

    /// Generate interrupt if enabled
    pub fn generate_interrupt(&mut self) {
        let glob_cnt = self.glob_cnt.load(Ordering::Relaxed);
        if glob_cnt & GlobalControl::GIE != 0 {
            self.pending_interrupt = true;
            self.stats.record_interrupt();
        }
    }

    /// Process buffer completion
    pub fn process_buffer_completion(&mut self, is_output: bool) {
        // First, determine the interrupt conditions
        let (bcis_int, lvbci_int) = {
            let channel = if is_output {
                &mut self.po_channel
            } else {
                &mut self.pi_channel
            };

            channel.set_status(StatusBits::BCIS);
            let bcis_int = channel.interrupt_enabled(StatusBits::BCIS);

            channel.advance_buffer();

            let lvbci_int =
                channel.sr & StatusBits::LVBCI != 0 && channel.interrupt_enabled(StatusBits::LVBCI);

            (bcis_int, lvbci_int)
        };

        // Now generate interrupts outside the borrow
        if bcis_int || lvbci_int {
            self.generate_interrupt();
        }
    }

    /// Get statistics
    pub fn stats(&self) -> &AudioStats {
        &self.stats
    }

    /// Check if output is running
    pub fn is_output_running(&self) -> bool {
        self.po_channel.is_running()
    }

    /// Check if input is running
    pub fn is_input_running(&self) -> bool {
        self.pi_channel.is_running()
    }

    /// Get current sample rate
    pub fn sample_rate(&self) -> u32 {
        self.mixer.sample_rate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ac97_register_from_offset() {
        assert_eq!(Ac97Register::from_offset(0x00), Some(Ac97Register::Reset));
        assert_eq!(
            Ac97Register::from_offset(0x02),
            Some(Ac97Register::MasterVolume)
        );
        assert_eq!(
            Ac97Register::from_offset(0x18),
            Some(Ac97Register::PcmOutVolume)
        );
        assert_eq!(
            Ac97Register::from_offset(0x7C),
            Some(Ac97Register::VendorId1)
        );
        assert_eq!(Ac97Register::from_offset(0xFF), None);
    }

    #[test]
    fn test_buffer_descriptor() {
        let bytes = [0x00, 0x10, 0x00, 0x00, 0x00, 0x04, 0x00, 0x80];
        let bd = BufferDescriptor::from_bytes(&bytes).unwrap();

        assert_eq!(bd.address, 0x1000);
        assert_eq!(bd.samples, 1024);
        assert!(bd.interrupt_on_completion());
        assert_eq!(bd.byte_length(), 4096);
    }

    #[test]
    fn test_buffer_descriptor_flags() {
        let bd = BufferDescriptor {
            address: 0x1000,
            samples: 256,
            flags: BufferDescriptor::IOC | BufferDescriptor::BUP,
        };

        assert!(bd.interrupt_on_completion());
        assert!(bd.buffer_underrun_protection());
    }

    #[test]
    fn test_dma_channel_creation() {
        let channel = DmaChannel::new(StreamDirection::Playback);
        assert!(channel.is_halted());
        assert!(!channel.is_running());
    }

    #[test]
    fn test_dma_channel_start_stop() {
        let mut channel = DmaChannel::new(StreamDirection::Playback);

        channel.start();
        assert!(channel.is_running());
        assert!(!channel.is_halted());

        channel.stop();
        assert!(!channel.is_running());
        assert!(channel.is_halted());
    }

    #[test]
    fn test_dma_channel_reset() {
        let mut channel = DmaChannel::new(StreamDirection::Playback);
        channel.bdbar = 0x1000;
        channel.start();

        channel.reset();

        assert_eq!(channel.bdbar, 0);
        assert!(channel.is_halted());
    }

    #[test]
    fn test_dma_channel_advance() {
        let mut channel = DmaChannel::new(StreamDirection::Playback);
        channel.lvi = 5;

        channel.advance_buffer();
        assert_eq!(channel.civ, 1);

        channel.civ = 4;
        channel.advance_buffer();
        assert_eq!(channel.civ, 5);
        assert!(channel.sr & StatusBits::LVBCI != 0);
    }

    #[test]
    fn test_mixer_creation() {
        let mixer = Ac97Mixer::new();
        assert_eq!(mixer.sample_rate(), 48000);
    }

    #[test]
    fn test_mixer_reset() {
        let mut mixer = Ac97Mixer::new();
        mixer.write(0x02, 0x8000); // Mute master

        mixer.reset();

        assert!(!mixer.master_volume().muted);
    }

    #[test]
    fn test_mixer_master_volume() {
        let mut mixer = Ac97Mixer::new();

        // Full volume
        mixer.write(0x02, 0x0000);
        assert_eq!(mixer.master_volume().left.0, 255);
        assert_eq!(mixer.master_volume().right.0, 255);

        // Muted
        mixer.write(0x02, 0x8000);
        assert!(mixer.master_volume().muted);
    }

    #[test]
    fn test_mixer_pcm_volume() {
        let mut mixer = Ac97Mixer::new();

        mixer.write(0x18, 0x0808);
        assert!(mixer.pcm_volume().left.0 < 255);
        assert!(mixer.pcm_volume().right.0 < 255);
    }

    #[test]
    fn test_mixer_sample_rate() {
        let mut mixer = Ac97Mixer::new();

        mixer.write(0x2C, 44100);
        assert_eq!(mixer.sample_rate(), 44100);

        // Test at the edge - u16 max is 65535, AC97 max is 48000
        mixer.write(0x2C, 65535);
        assert_eq!(mixer.sample_rate(), 48000);
    }

    #[test]
    fn test_mixer_vendor_id() {
        let mixer = Ac97Mixer::new();
        assert_eq!(mixer.read(0x7C), 0x4144);
        assert_eq!(mixer.read(0x7E), 0x5370);
    }

    #[test]
    fn test_controller_creation() {
        let controller = Ac97Controller::new();
        assert!(!controller.is_output_running());
        assert!(!controller.is_input_running());
    }

    #[test]
    fn test_controller_reset() {
        let mut controller = Ac97Controller::new();
        controller.write_bm32(BusMaster::PO_BDBAR, 0x1000);

        controller.reset();

        assert_eq!(controller.read_bm32(BusMaster::PO_BDBAR), 0);
    }

    #[test]
    fn test_controller_bdbar() {
        let mut controller = Ac97Controller::new();

        controller.write_bm32(BusMaster::PO_BDBAR, 0x1008);
        // Should be aligned to 8 bytes
        assert_eq!(controller.read_bm32(BusMaster::PO_BDBAR), 0x1008);
    }

    #[test]
    fn test_controller_lvi() {
        let mut controller = Ac97Controller::new();

        controller.write_bm8(BusMaster::PO_LVI, 15);
        assert_eq!(controller.read_bm8(BusMaster::PO_LVI), 15);

        // Masked to 5 bits
        controller.write_bm8(BusMaster::PO_LVI, 0xFF);
        assert_eq!(controller.read_bm8(BusMaster::PO_LVI), 31);
    }

    #[test]
    fn test_controller_global_control() {
        let mut controller = Ac97Controller::new();

        controller.write_bm32(BusMaster::GLOB_CNT, GlobalControl::GIE);
        assert_eq!(
            controller.read_bm32(BusMaster::GLOB_CNT) & GlobalControl::GIE,
            GlobalControl::GIE
        );
    }

    #[test]
    fn test_controller_global_status() {
        let controller = Ac97Controller::new();

        let status = controller.read_bm32(BusMaster::GLOB_STA);
        assert!(status & GlobalStatus::PRI != 0);
        assert!(status & GlobalStatus::AD != 0);
    }

    #[test]
    fn test_controller_mixer_access() {
        let mut controller = Ac97Controller::new();

        controller.write_mixer(0x02, 0x0808);
        let volume = controller.read_mixer(0x02);
        assert_eq!(volume, 0x0808);
    }

    #[test]
    fn test_controller_audio_write() {
        let mut controller = Ac97Controller::new();
        controller.output_stream.start();

        let data = vec![0u8; 1024];
        let written = controller.write_audio(&data);
        assert!(written > 0);
    }

    #[test]
    fn test_controller_interrupt() {
        let mut controller = Ac97Controller::new();

        // Enable global interrupt
        controller.write_bm32(BusMaster::GLOB_CNT, GlobalControl::GIE);

        controller.generate_interrupt();
        assert!(controller.has_interrupt());

        controller.clear_interrupt();
        assert!(!controller.has_interrupt());
    }

    #[test]
    fn test_controller_stats() {
        let mut controller = Ac97Controller::new();
        controller.output_stream.start();

        let data = vec![0u8; 100];
        controller.write_audio(&data);

        let snapshot = controller.stats().snapshot();
        assert!(snapshot.bytes_played > 0);
    }
}
