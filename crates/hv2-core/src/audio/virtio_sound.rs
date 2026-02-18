//! VirtIO Sound Device
//!
//! This module provides VirtIO sound device emulation with
//! PCM streams, jacks, and channel mapping support.

use super::core::{
    AudioParams, AudioStats, AudioStream, ChannelLayout, PcmStream, SampleFormat,
    SampleRate, StreamDirection,
};

/// VirtIO sound request types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtioSndRequestType {
    /// Jack info
    JackInfo = 1,
    /// Jack remap
    JackRemap = 2,
    /// PCM info
    PcmInfo = 0x0100,
    /// PCM set params
    PcmSetParams = 0x0101,
    /// PCM prepare
    PcmPrepare = 0x0102,
    /// PCM release
    PcmRelease = 0x0103,
    /// PCM start
    PcmStart = 0x0104,
    /// PCM stop
    PcmStop = 0x0105,
    /// Channel map info
    ChmapInfo = 0x0200,
}

impl VirtioSndRequestType {
    /// Create from value
    pub fn from_u32(value: u32) -> Option<Self> {
        match value {
            1 => Some(VirtioSndRequestType::JackInfo),
            2 => Some(VirtioSndRequestType::JackRemap),
            0x0100 => Some(VirtioSndRequestType::PcmInfo),
            0x0101 => Some(VirtioSndRequestType::PcmSetParams),
            0x0102 => Some(VirtioSndRequestType::PcmPrepare),
            0x0103 => Some(VirtioSndRequestType::PcmRelease),
            0x0104 => Some(VirtioSndRequestType::PcmStart),
            0x0105 => Some(VirtioSndRequestType::PcmStop),
            0x0200 => Some(VirtioSndRequestType::ChmapInfo),
            _ => None,
        }
    }
}

/// VirtIO sound status codes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum VirtioSndStatus {
    /// Success
    Ok = 0x8000,
    /// Bad message
    BadMsg = 0x8001,
    /// Not supported
    NotSupp = 0x8002,
    /// I/O error
    IoErr = 0x8003,
}

/// VirtIO sound directions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VirtioSndDirection {
    /// Output (playback)
    Output = 0,
    /// Input (capture)
    Input = 1,
}

impl From<VirtioSndDirection> for StreamDirection {
    fn from(dir: VirtioSndDirection) -> Self {
        match dir {
            VirtioSndDirection::Output => StreamDirection::Playback,
            VirtioSndDirection::Input => StreamDirection::Capture,
        }
    }
}

/// VirtIO sound PCM formats
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VirtioSndPcmFormat {
    /// IMA ADPCM
    ImaAdpcm = 0,
    /// Mu-law
    MuLaw = 1,
    /// A-law
    ALaw = 2,
    /// Signed 8-bit
    S8 = 3,
    /// Unsigned 8-bit
    U8 = 4,
    /// Signed 16-bit LE
    S16 = 5,
    /// Unsigned 16-bit LE
    U16 = 6,
    /// Signed 18-bit LE
    S18_3 = 7,
    /// Unsigned 18-bit LE
    U18_3 = 8,
    /// Signed 20-bit LE
    S20_3 = 9,
    /// Unsigned 20-bit LE
    U20_3 = 10,
    /// Signed 24-bit LE
    S24_3 = 11,
    /// Unsigned 24-bit LE
    U24_3 = 12,
    /// Signed 20-bit in 32-bit LE
    S20 = 13,
    /// Unsigned 20-bit in 32-bit LE
    U20 = 14,
    /// Signed 24-bit in 32-bit LE
    S24 = 15,
    /// Unsigned 24-bit in 32-bit LE
    U24 = 16,
    /// Signed 32-bit LE
    S32 = 17,
    /// Unsigned 32-bit LE
    U32 = 18,
    /// 32-bit float LE
    Float = 19,
    /// 64-bit float LE
    Float64 = 20,
    /// DSD unsigned 8-bit
    DsdU8 = 21,
    /// DSD unsigned 16-bit LE
    DsdU16 = 22,
    /// DSD unsigned 32-bit LE
    DsdU32 = 23,
    /// IEC958 subframe LE
    Iec958Subframe = 24,
}

impl VirtioSndPcmFormat {
    /// Create from value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(VirtioSndPcmFormat::ImaAdpcm),
            1 => Some(VirtioSndPcmFormat::MuLaw),
            2 => Some(VirtioSndPcmFormat::ALaw),
            3 => Some(VirtioSndPcmFormat::S8),
            4 => Some(VirtioSndPcmFormat::U8),
            5 => Some(VirtioSndPcmFormat::S16),
            6 => Some(VirtioSndPcmFormat::U16),
            7 => Some(VirtioSndPcmFormat::S18_3),
            8 => Some(VirtioSndPcmFormat::U18_3),
            9 => Some(VirtioSndPcmFormat::S20_3),
            10 => Some(VirtioSndPcmFormat::U20_3),
            11 => Some(VirtioSndPcmFormat::S24_3),
            12 => Some(VirtioSndPcmFormat::U24_3),
            13 => Some(VirtioSndPcmFormat::S20),
            14 => Some(VirtioSndPcmFormat::U20),
            15 => Some(VirtioSndPcmFormat::S24),
            16 => Some(VirtioSndPcmFormat::U24),
            17 => Some(VirtioSndPcmFormat::S32),
            18 => Some(VirtioSndPcmFormat::U32),
            19 => Some(VirtioSndPcmFormat::Float),
            20 => Some(VirtioSndPcmFormat::Float64),
            21 => Some(VirtioSndPcmFormat::DsdU8),
            22 => Some(VirtioSndPcmFormat::DsdU16),
            23 => Some(VirtioSndPcmFormat::DsdU32),
            24 => Some(VirtioSndPcmFormat::Iec958Subframe),
            _ => None,
        }
    }

    /// Convert to internal format
    pub fn to_sample_format(&self) -> Option<SampleFormat> {
        match self {
            VirtioSndPcmFormat::U8 => Some(SampleFormat::U8),
            VirtioSndPcmFormat::S16 => Some(SampleFormat::S16Le),
            VirtioSndPcmFormat::S24 => Some(SampleFormat::S24Le),
            VirtioSndPcmFormat::S32 => Some(SampleFormat::S32Le),
            VirtioSndPcmFormat::Float => Some(SampleFormat::F32Le),
            _ => None,
        }
    }

    /// Get format bit mask for capabilities
    pub fn format_mask() -> u64 {
        // Support U8, S16, S24, S32, Float
        (1 << 4) | (1 << 5) | (1 << 15) | (1 << 17) | (1 << 19)
    }
}

/// VirtIO sound PCM rates
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VirtioSndPcmRate {
    /// 5512 Hz
    Rate5512 = 0,
    /// 8000 Hz
    Rate8000 = 1,
    /// 11025 Hz
    Rate11025 = 2,
    /// 16000 Hz
    Rate16000 = 3,
    /// 22050 Hz
    Rate22050 = 4,
    /// 32000 Hz
    Rate32000 = 5,
    /// 44100 Hz
    Rate44100 = 6,
    /// 48000 Hz
    Rate48000 = 7,
    /// 64000 Hz
    Rate64000 = 8,
    /// 88200 Hz
    Rate88200 = 9,
    /// 96000 Hz
    Rate96000 = 10,
    /// 176400 Hz
    Rate176400 = 11,
    /// 192000 Hz
    Rate192000 = 12,
    /// 384000 Hz
    Rate384000 = 13,
}

impl VirtioSndPcmRate {
    /// Create from value
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(VirtioSndPcmRate::Rate5512),
            1 => Some(VirtioSndPcmRate::Rate8000),
            2 => Some(VirtioSndPcmRate::Rate11025),
            3 => Some(VirtioSndPcmRate::Rate16000),
            4 => Some(VirtioSndPcmRate::Rate22050),
            5 => Some(VirtioSndPcmRate::Rate32000),
            6 => Some(VirtioSndPcmRate::Rate44100),
            7 => Some(VirtioSndPcmRate::Rate48000),
            8 => Some(VirtioSndPcmRate::Rate64000),
            9 => Some(VirtioSndPcmRate::Rate88200),
            10 => Some(VirtioSndPcmRate::Rate96000),
            11 => Some(VirtioSndPcmRate::Rate176400),
            12 => Some(VirtioSndPcmRate::Rate192000),
            13 => Some(VirtioSndPcmRate::Rate384000),
            _ => None,
        }
    }

    /// Convert to internal rate
    pub fn to_sample_rate(&self) -> Option<SampleRate> {
        match self {
            VirtioSndPcmRate::Rate8000 => Some(SampleRate::Hz8000),
            VirtioSndPcmRate::Rate11025 => Some(SampleRate::Hz11025),
            VirtioSndPcmRate::Rate16000 => Some(SampleRate::Hz16000),
            VirtioSndPcmRate::Rate22050 => Some(SampleRate::Hz22050),
            VirtioSndPcmRate::Rate32000 => Some(SampleRate::Hz32000),
            VirtioSndPcmRate::Rate44100 => Some(SampleRate::Hz44100),
            VirtioSndPcmRate::Rate48000 => Some(SampleRate::Hz48000),
            VirtioSndPcmRate::Rate96000 => Some(SampleRate::Hz96000),
            VirtioSndPcmRate::Rate192000 => Some(SampleRate::Hz192000),
            _ => None,
        }
    }

    /// Get rate bit mask for capabilities
    pub fn rate_mask() -> u64 {
        // Support common rates: 8k, 11k, 16k, 22k, 32k, 44.1k, 48k, 96k, 192k
        (1 << 1)
            | (1 << 2)
            | (1 << 3)
            | (1 << 4)
            | (1 << 5)
            | (1 << 6)
            | (1 << 7)
            | (1 << 10)
            | (1 << 12)
    }
}

/// VirtIO sound channel positions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum VirtioSndChmapPosition {
    /// No position
    None = 0,
    /// Mono (single channel)
    Mono = 3,
    /// Front left
    FrontLeft = 4,
    /// Front right
    FrontRight = 5,
    /// Rear left
    RearLeft = 6,
    /// Rear right
    RearRight = 7,
    /// Front center
    FrontCenter = 8,
    /// LFE (subwoofer)
    Lfe = 9,
    /// Side left
    SideLeft = 10,
    /// Side right
    SideRight = 11,
    /// Rear center
    RearCenter = 12,
    /// Front left center
    FrontLeftCenter = 13,
    /// Front right center
    FrontRightCenter = 14,
    /// Top center
    TopCenter = 22,
    /// Top front left
    TopFrontLeft = 23,
    /// Top front right
    TopFrontRight = 24,
    /// Top front center
    TopFrontCenter = 25,
    /// Top rear left
    TopRearLeft = 26,
    /// Top rear right
    TopRearRight = 27,
    /// Top rear center
    TopRearCenter = 28,
}

/// Jack types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum JackType {
    /// Line out
    LineOut = 0,
    /// Speaker
    Speaker = 1,
    /// Headphone
    Headphone = 2,
    /// CD
    Cd = 3,
    /// SPDIF out
    SpdifOut = 4,
    /// Digital out
    DigitalOut = 5,
    /// Modem line
    ModemLine = 6,
    /// Handset
    Handset = 7,
    /// Line in
    LineIn = 8,
    /// Aux
    Aux = 9,
    /// Mic
    Mic = 10,
    /// Telephony
    Telephony = 11,
    /// SPDIF in
    SpdifIn = 12,
    /// Digital in
    DigitalIn = 13,
}

/// Jack state
#[derive(Debug, Clone)]
pub struct Jack {
    /// Jack ID
    pub id: u32,
    /// Jack type
    pub jack_type: JackType,
    /// Connected
    pub connected: bool,
    /// Associated stream (if any)
    pub stream_id: Option<u32>,
    /// Current features
    pub features: u32,
}

impl Jack {
    /// Create new jack
    pub fn new(id: u32, jack_type: JackType) -> Self {
        Self {
            id,
            jack_type,
            connected: true,
            stream_id: None,
            features: 0,
        }
    }

    /// Create line out jack
    pub fn line_out(id: u32) -> Self {
        Self::new(id, JackType::LineOut)
    }

    /// Create headphone jack
    pub fn headphone(id: u32) -> Self {
        Self::new(id, JackType::Headphone)
    }

    /// Create mic jack
    pub fn mic(id: u32) -> Self {
        Self::new(id, JackType::Mic)
    }

    /// Create line in jack
    pub fn line_in(id: u32) -> Self {
        Self::new(id, JackType::LineIn)
    }
}

/// PCM stream info
#[derive(Debug, Clone)]
pub struct PcmStreamInfo {
    /// Stream ID
    pub id: u32,
    /// Direction
    pub direction: VirtioSndDirection,
    /// Supported formats
    pub formats: u64,
    /// Supported rates
    pub rates: u64,
    /// Min channels
    pub channels_min: u8,
    /// Max channels
    pub channels_max: u8,
    /// Associated jack
    pub jack_id: Option<u32>,
}

impl PcmStreamInfo {
    /// Create playback stream info
    pub fn playback(id: u32) -> Self {
        Self {
            id,
            direction: VirtioSndDirection::Output,
            formats: VirtioSndPcmFormat::format_mask(),
            rates: VirtioSndPcmRate::rate_mask(),
            channels_min: 1,
            channels_max: 8,
            jack_id: None,
        }
    }

    /// Create capture stream info
    pub fn capture(id: u32) -> Self {
        Self {
            id,
            direction: VirtioSndDirection::Input,
            formats: VirtioSndPcmFormat::format_mask(),
            rates: VirtioSndPcmRate::rate_mask(),
            channels_min: 1,
            channels_max: 8,
            jack_id: None,
        }
    }
}

/// PCM stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PcmState {
    /// Not allocated
    #[default]
    Disabled,
    /// Allocated but not prepared
    Enabled,
    /// Prepared
    Prepared,
    /// Running
    Running,
}

/// PCM stream parameters
#[derive(Debug, Clone, Default)]
pub struct PcmParams {
    /// Buffer bytes
    pub buffer_bytes: u32,
    /// Period bytes
    pub period_bytes: u32,
    /// Features
    pub features: u32,
    /// Channels
    pub channels: u8,
    /// Format
    pub format: u8,
    /// Rate
    pub rate: u8,
}

impl PcmParams {
    /// Convert to audio params
    pub fn to_audio_params(&self) -> Option<AudioParams> {
        let format = VirtioSndPcmFormat::from_u8(self.format)?.to_sample_format()?;
        let rate = VirtioSndPcmRate::from_u8(self.rate)?.to_sample_rate()?;
        let channels = ChannelLayout::from_channels(self.channels as u32)?;
        Some(AudioParams::new(format, rate, channels))
    }
}

/// PCM stream
pub struct VirtioSndPcmStream {
    /// Stream info
    pub info: PcmStreamInfo,
    /// Current state
    pub state: PcmState,
    /// Current parameters
    pub params: PcmParams,
    /// PCM stream
    stream: Option<PcmStream>,
    /// Latency bytes
    pub latency_bytes: u32,
}

impl VirtioSndPcmStream {
    /// Create new stream
    pub fn new(info: PcmStreamInfo) -> Self {
        Self {
            info,
            state: PcmState::Disabled,
            params: PcmParams::default(),
            stream: None,
            latency_bytes: 0,
        }
    }

    /// Set parameters
    pub fn set_params(&mut self, params: PcmParams) -> VirtioSndStatus {
        if self.state == PcmState::Running {
            return VirtioSndStatus::BadMsg;
        }

        self.params = params;
        self.state = PcmState::Enabled;
        VirtioSndStatus::Ok
    }

    /// Prepare stream
    pub fn prepare(&mut self) -> VirtioSndStatus {
        if self.state != PcmState::Enabled && self.state != PcmState::Prepared {
            return VirtioSndStatus::BadMsg;
        }

        let Some(audio_params) = self.params.to_audio_params() else {
            return VirtioSndStatus::NotSupp;
        };

        let direction = self.info.direction.into();
        let buffer_ms = (self.params.buffer_bytes as u64 * 1000)
            / (audio_params.bytes_per_second() as u64).max(1);

        self.stream = Some(PcmStream::new(audio_params, direction, buffer_ms as u32));
        self.state = PcmState::Prepared;
        VirtioSndStatus::Ok
    }

    /// Release stream
    pub fn release(&mut self) -> VirtioSndStatus {
        if self.state == PcmState::Running {
            return VirtioSndStatus::BadMsg;
        }

        self.stream = None;
        self.state = PcmState::Disabled;
        VirtioSndStatus::Ok
    }

    /// Start stream
    pub fn start(&mut self) -> VirtioSndStatus {
        if self.state != PcmState::Prepared {
            return VirtioSndStatus::BadMsg;
        }

        if let Some(stream) = &mut self.stream {
            stream.start();
            self.state = PcmState::Running;
            VirtioSndStatus::Ok
        } else {
            VirtioSndStatus::IoErr
        }
    }

    /// Stop stream
    pub fn stop(&mut self) -> VirtioSndStatus {
        if self.state != PcmState::Running {
            return VirtioSndStatus::BadMsg;
        }

        if let Some(stream) = &mut self.stream {
            stream.stop();
            self.state = PcmState::Prepared;
            VirtioSndStatus::Ok
        } else {
            VirtioSndStatus::IoErr
        }
    }

    /// Write audio data
    pub fn write(&mut self, data: &[u8]) -> usize {
        if self.state != PcmState::Running {
            return 0;
        }
        if let Some(stream) = &mut self.stream {
            stream.write(data)
        } else {
            0
        }
    }

    /// Read audio data
    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        if self.state != PcmState::Running {
            return 0;
        }
        if let Some(stream) = &mut self.stream {
            stream.read(buffer)
        } else {
            0
        }
    }

    /// Get available bytes
    pub fn available(&self) -> usize {
        if let Some(stream) = &self.stream {
            stream.available()
        } else {
            0
        }
    }
}

/// Channel map info
#[derive(Debug, Clone)]
pub struct ChannelMap {
    /// Channel map ID
    pub id: u32,
    /// Associated stream ID
    pub stream_id: u32,
    /// Channel positions
    pub positions: Vec<VirtioSndChmapPosition>,
}

impl ChannelMap {
    /// Create stereo channel map
    pub fn stereo(id: u32, stream_id: u32) -> Self {
        Self {
            id,
            stream_id,
            positions: vec![
                VirtioSndChmapPosition::FrontLeft,
                VirtioSndChmapPosition::FrontRight,
            ],
        }
    }

    /// Create 5.1 surround channel map
    pub fn surround51(id: u32, stream_id: u32) -> Self {
        Self {
            id,
            stream_id,
            positions: vec![
                VirtioSndChmapPosition::FrontLeft,
                VirtioSndChmapPosition::FrontRight,
                VirtioSndChmapPosition::RearLeft,
                VirtioSndChmapPosition::RearRight,
                VirtioSndChmapPosition::FrontCenter,
                VirtioSndChmapPosition::Lfe,
            ],
        }
    }

    /// Create 7.1 surround channel map
    pub fn surround71(id: u32, stream_id: u32) -> Self {
        Self {
            id,
            stream_id,
            positions: vec![
                VirtioSndChmapPosition::FrontLeft,
                VirtioSndChmapPosition::FrontRight,
                VirtioSndChmapPosition::RearLeft,
                VirtioSndChmapPosition::RearRight,
                VirtioSndChmapPosition::FrontCenter,
                VirtioSndChmapPosition::Lfe,
                VirtioSndChmapPosition::SideLeft,
                VirtioSndChmapPosition::SideRight,
            ],
        }
    }
}

/// VirtIO features
pub mod Features {
    /// Control queue support
    pub const CTRL_VQ: u64 = 1 << 0;
    /// Event queue support
    pub const EVENT_VQ: u64 = 1 << 1;
}

/// VirtIO sound device
pub struct VirtioSoundDevice {
    /// Device features
    features: u64,
    /// Negotiated features
    driver_features: u64,
    /// Jacks
    jacks: Vec<Jack>,
    /// PCM streams
    streams: Vec<VirtioSndPcmStream>,
    /// Channel maps
    channel_maps: Vec<ChannelMap>,
    /// Statistics
    stats: AudioStats,
    /// Device enabled
    enabled: bool,
    /// Pending interrupt
    pending_interrupt: bool,
}

impl Default for VirtioSoundDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtioSoundDevice {
    /// Create new VirtIO sound device
    pub fn new() -> Self {
        let mut device = Self {
            features: Features::CTRL_VQ | Features::EVENT_VQ,
            driver_features: 0,
            jacks: Vec::new(),
            streams: Vec::new(),
            channel_maps: Vec::new(),
            stats: AudioStats::new(),
            enabled: false,
            pending_interrupt: false,
        };
        device.setup_default_config();
        device
    }

    /// Setup default configuration
    fn setup_default_config(&mut self) {
        // Add jacks
        self.jacks.push(Jack::line_out(0));
        self.jacks.push(Jack::headphone(1));
        self.jacks.push(Jack::mic(2));
        self.jacks.push(Jack::line_in(3));

        // Add playback streams
        let mut playback = PcmStreamInfo::playback(0);
        playback.jack_id = Some(0);
        self.streams.push(VirtioSndPcmStream::new(playback));

        let mut headphone = PcmStreamInfo::playback(1);
        headphone.jack_id = Some(1);
        self.streams.push(VirtioSndPcmStream::new(headphone));

        // Add capture streams
        let mut mic_stream = PcmStreamInfo::capture(2);
        mic_stream.jack_id = Some(2);
        self.streams.push(VirtioSndPcmStream::new(mic_stream));

        let mut line_in_stream = PcmStreamInfo::capture(3);
        line_in_stream.jack_id = Some(3);
        self.streams.push(VirtioSndPcmStream::new(line_in_stream));

        // Add channel maps
        self.channel_maps.push(ChannelMap::stereo(0, 0));
        self.channel_maps.push(ChannelMap::stereo(1, 1));
        self.channel_maps.push(ChannelMap::stereo(2, 2));
        self.channel_maps.push(ChannelMap::stereo(3, 3));
    }

    /// Get device features
    pub fn features(&self) -> u64 {
        self.features
    }

    /// Set driver features
    pub fn set_driver_features(&mut self, features: u64) {
        self.driver_features = features & self.features;
    }

    /// Get driver features
    pub fn driver_features(&self) -> u64 {
        self.driver_features
    }

    /// Enable device
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable device
    pub fn disable(&mut self) {
        self.enabled = false;
        for stream in &mut self.streams {
            stream.release();
        }
    }

    /// Check if enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Get jack count
    pub fn jack_count(&self) -> u32 {
        self.jacks.len() as u32
    }

    /// Get stream count
    pub fn stream_count(&self) -> u32 {
        self.streams.len() as u32
    }

    /// Get channel map count
    pub fn channel_map_count(&self) -> u32 {
        self.channel_maps.len() as u32
    }

    /// Get jack
    pub fn jack(&self, id: u32) -> Option<&Jack> {
        self.jacks.iter().find(|j| j.id == id)
    }

    /// Get mutable jack
    pub fn jack_mut(&mut self, id: u32) -> Option<&mut Jack> {
        self.jacks.iter_mut().find(|j| j.id == id)
    }

    /// Get stream
    pub fn stream(&self, id: u32) -> Option<&VirtioSndPcmStream> {
        self.streams.iter().find(|s| s.info.id == id)
    }

    /// Get mutable stream
    pub fn stream_mut(&mut self, id: u32) -> Option<&mut VirtioSndPcmStream> {
        self.streams.iter_mut().find(|s| s.info.id == id)
    }

    /// Get channel map
    pub fn channel_map(&self, id: u32) -> Option<&ChannelMap> {
        self.channel_maps.iter().find(|c| c.id == id)
    }

    /// Process control request
    pub fn process_request(&mut self, request_type: u32, stream_id: u32) -> VirtioSndStatus {
        let Some(req_type) = VirtioSndRequestType::from_u32(request_type) else {
            return VirtioSndStatus::BadMsg;
        };

        match req_type {
            VirtioSndRequestType::JackInfo => VirtioSndStatus::Ok,
            VirtioSndRequestType::JackRemap => VirtioSndStatus::NotSupp,
            VirtioSndRequestType::PcmInfo => VirtioSndStatus::Ok,
            VirtioSndRequestType::PcmSetParams => VirtioSndStatus::Ok,
            VirtioSndRequestType::PcmPrepare => {
                if let Some(stream) = self.stream_mut(stream_id) {
                    stream.prepare()
                } else {
                    VirtioSndStatus::BadMsg
                }
            }
            VirtioSndRequestType::PcmRelease => {
                if let Some(stream) = self.stream_mut(stream_id) {
                    stream.release()
                } else {
                    VirtioSndStatus::BadMsg
                }
            }
            VirtioSndRequestType::PcmStart => {
                if let Some(stream) = self.stream_mut(stream_id) {
                    stream.start()
                } else {
                    VirtioSndStatus::BadMsg
                }
            }
            VirtioSndRequestType::PcmStop => {
                if let Some(stream) = self.stream_mut(stream_id) {
                    stream.stop()
                } else {
                    VirtioSndStatus::BadMsg
                }
            }
            VirtioSndRequestType::ChmapInfo => VirtioSndStatus::Ok,
        }
    }

    /// Set stream parameters
    pub fn set_stream_params(&mut self, stream_id: u32, params: PcmParams) -> VirtioSndStatus {
        if let Some(stream) = self.stream_mut(stream_id) {
            stream.set_params(params)
        } else {
            VirtioSndStatus::BadMsg
        }
    }

    /// Write audio to stream
    pub fn write_audio(&mut self, stream_id: u32, data: &[u8]) -> usize {
        if let Some(stream) = self.stream_mut(stream_id) {
            let written = stream.write(data);
            self.stats.record_played(written as u64);
            written
        } else {
            0
        }
    }

    /// Read audio from stream
    pub fn read_audio(&mut self, stream_id: u32, buffer: &mut [u8]) -> usize {
        if let Some(stream) = self.stream_mut(stream_id) {
            let read = stream.read(buffer);
            self.stats.record_captured(read as u64);
            read
        } else {
            0
        }
    }

    /// Check for pending interrupt
    pub fn has_interrupt(&self) -> bool {
        self.pending_interrupt
    }

    /// Generate interrupt
    pub fn generate_interrupt(&mut self) {
        self.pending_interrupt = true;
        self.stats.record_interrupt();
    }

    /// Clear interrupt
    pub fn clear_interrupt(&mut self) {
        self.pending_interrupt = false;
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
    fn test_request_type_from_u32() {
        assert_eq!(
            VirtioSndRequestType::from_u32(1),
            Some(VirtioSndRequestType::JackInfo)
        );
        assert_eq!(
            VirtioSndRequestType::from_u32(0x0100),
            Some(VirtioSndRequestType::PcmInfo)
        );
        assert_eq!(
            VirtioSndRequestType::from_u32(0x0104),
            Some(VirtioSndRequestType::PcmStart)
        );
        assert_eq!(VirtioSndRequestType::from_u32(0xFFFF), None);
    }

    #[test]
    fn test_pcm_format_conversion() {
        assert_eq!(
            VirtioSndPcmFormat::U8.to_sample_format(),
            Some(SampleFormat::U8)
        );
        assert_eq!(
            VirtioSndPcmFormat::S16.to_sample_format(),
            Some(SampleFormat::S16Le)
        );
        assert_eq!(
            VirtioSndPcmFormat::S32.to_sample_format(),
            Some(SampleFormat::S32Le)
        );
    }

    #[test]
    fn test_pcm_rate_conversion() {
        assert_eq!(
            VirtioSndPcmRate::Rate44100.to_sample_rate(),
            Some(SampleRate::Hz44100)
        );
        assert_eq!(
            VirtioSndPcmRate::Rate48000.to_sample_rate(),
            Some(SampleRate::Hz48000)
        );
        assert_eq!(
            VirtioSndPcmRate::Rate96000.to_sample_rate(),
            Some(SampleRate::Hz96000)
        );
    }

    #[test]
    fn test_jack_creation() {
        let jack = Jack::line_out(0);
        assert_eq!(jack.id, 0);
        assert_eq!(jack.jack_type, JackType::LineOut);
        assert!(jack.connected);
    }

    #[test]
    fn test_pcm_stream_info() {
        let playback = PcmStreamInfo::playback(0);
        assert_eq!(playback.direction, VirtioSndDirection::Output);
        assert!(playback.formats != 0);
        assert!(playback.rates != 0);

        let capture = PcmStreamInfo::capture(1);
        assert_eq!(capture.direction, VirtioSndDirection::Input);
    }

    #[test]
    fn test_pcm_params() {
        let params = PcmParams {
            buffer_bytes: 8192,
            period_bytes: 1024,
            features: 0,
            channels: 2,
            format: VirtioSndPcmFormat::S16 as u8,
            rate: VirtioSndPcmRate::Rate48000 as u8,
        };

        let audio_params = params.to_audio_params().unwrap();
        assert_eq!(audio_params.format, SampleFormat::S16Le);
        assert_eq!(audio_params.rate, SampleRate::Hz48000);
        assert_eq!(audio_params.channels, ChannelLayout::Stereo);
    }

    #[test]
    fn test_pcm_stream_lifecycle() {
        let info = PcmStreamInfo::playback(0);
        let mut stream = VirtioSndPcmStream::new(info);
        assert_eq!(stream.state, PcmState::Disabled);

        let params = PcmParams {
            buffer_bytes: 8192,
            period_bytes: 1024,
            features: 0,
            channels: 2,
            format: VirtioSndPcmFormat::S16 as u8,
            rate: VirtioSndPcmRate::Rate48000 as u8,
        };

        assert_eq!(stream.set_params(params), VirtioSndStatus::Ok);
        assert_eq!(stream.state, PcmState::Enabled);

        assert_eq!(stream.prepare(), VirtioSndStatus::Ok);
        assert_eq!(stream.state, PcmState::Prepared);

        assert_eq!(stream.start(), VirtioSndStatus::Ok);
        assert_eq!(stream.state, PcmState::Running);

        assert_eq!(stream.stop(), VirtioSndStatus::Ok);
        assert_eq!(stream.state, PcmState::Prepared);

        assert_eq!(stream.release(), VirtioSndStatus::Ok);
        assert_eq!(stream.state, PcmState::Disabled);
    }

    #[test]
    fn test_channel_map() {
        let stereo = ChannelMap::stereo(0, 0);
        assert_eq!(stereo.positions.len(), 2);

        let surround51 = ChannelMap::surround51(1, 0);
        assert_eq!(surround51.positions.len(), 6);

        let surround71 = ChannelMap::surround71(2, 0);
        assert_eq!(surround71.positions.len(), 8);
    }

    #[test]
    fn test_device_creation() {
        let device = VirtioSoundDevice::new();
        assert!(!device.is_enabled());
        assert!(device.jack_count() > 0);
        assert!(device.stream_count() > 0);
        assert!(device.channel_map_count() > 0);
    }

    #[test]
    fn test_device_features() {
        let mut device = VirtioSoundDevice::new();
        let features = device.features();
        assert!(features & Features::CTRL_VQ != 0);

        device.set_driver_features(Features::CTRL_VQ);
        assert_eq!(device.driver_features(), Features::CTRL_VQ);
    }

    #[test]
    fn test_device_enable_disable() {
        let mut device = VirtioSoundDevice::new();

        device.enable();
        assert!(device.is_enabled());

        device.disable();
        assert!(!device.is_enabled());
    }

    #[test]
    fn test_device_jack_access() {
        let device = VirtioSoundDevice::new();

        let jack = device.jack(0).unwrap();
        assert_eq!(jack.jack_type, JackType::LineOut);

        let jack = device.jack(1).unwrap();
        assert_eq!(jack.jack_type, JackType::Headphone);
    }

    #[test]
    fn test_device_stream_access() {
        let device = VirtioSoundDevice::new();

        let stream = device.stream(0).unwrap();
        assert_eq!(stream.info.direction, VirtioSndDirection::Output);

        let stream = device.stream(2).unwrap();
        assert_eq!(stream.info.direction, VirtioSndDirection::Input);
    }

    #[test]
    fn test_device_process_request() {
        let mut device = VirtioSoundDevice::new();

        // Jack info should succeed
        assert_eq!(
            device.process_request(VirtioSndRequestType::JackInfo as u32, 0),
            VirtioSndStatus::Ok
        );

        // Invalid request
        assert_eq!(device.process_request(0xFFFF, 0), VirtioSndStatus::BadMsg);
    }

    #[test]
    fn test_device_stream_setup() {
        let mut device = VirtioSoundDevice::new();
        device.enable();

        let params = PcmParams {
            buffer_bytes: 8192,
            period_bytes: 1024,
            features: 0,
            channels: 2,
            format: VirtioSndPcmFormat::S16 as u8,
            rate: VirtioSndPcmRate::Rate48000 as u8,
        };

        assert_eq!(device.set_stream_params(0, params), VirtioSndStatus::Ok);

        // Prepare
        assert_eq!(
            device.process_request(VirtioSndRequestType::PcmPrepare as u32, 0),
            VirtioSndStatus::Ok
        );

        // Start
        assert_eq!(
            device.process_request(VirtioSndRequestType::PcmStart as u32, 0),
            VirtioSndStatus::Ok
        );
    }

    #[test]
    fn test_device_audio_write() {
        let mut device = VirtioSoundDevice::new();
        device.enable();

        let params = PcmParams {
            buffer_bytes: 8192,
            period_bytes: 1024,
            features: 0,
            channels: 2,
            format: VirtioSndPcmFormat::S16 as u8,
            rate: VirtioSndPcmRate::Rate48000 as u8,
        };

        device.set_stream_params(0, params);
        device.process_request(VirtioSndRequestType::PcmPrepare as u32, 0);
        device.process_request(VirtioSndRequestType::PcmStart as u32, 0);

        let data = vec![0u8; 1024];
        let written = device.write_audio(0, &data);
        assert!(written > 0);
    }

    #[test]
    fn test_device_interrupt() {
        let mut device = VirtioSoundDevice::new();

        assert!(!device.has_interrupt());

        device.generate_interrupt();
        assert!(device.has_interrupt());

        device.clear_interrupt();
        assert!(!device.has_interrupt());
    }

    #[test]
    fn test_device_stats() {
        let mut device = VirtioSoundDevice::new();
        device.enable();

        let params = PcmParams {
            buffer_bytes: 8192,
            period_bytes: 1024,
            features: 0,
            channels: 2,
            format: VirtioSndPcmFormat::S16 as u8,
            rate: VirtioSndPcmRate::Rate48000 as u8,
        };

        device.set_stream_params(0, params);
        device.process_request(VirtioSndRequestType::PcmPrepare as u32, 0);
        device.process_request(VirtioSndRequestType::PcmStart as u32, 0);

        let data = vec![0u8; 100];
        device.write_audio(0, &data);

        let snapshot = device.stats().snapshot();
        assert!(snapshot.bytes_played > 0);
    }
}
