//! Audio Core Types and Abstractions
//!
//! This module provides common types for audio device emulation,
//! including sample formats, buffers, streams, and mixing.

use std::sync::atomic::{AtomicU64, Ordering};

/// Audio sample format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleFormat {
    /// Unsigned 8-bit
    U8,
    /// Signed 16-bit little-endian
    #[default]
    S16Le,
    /// Signed 16-bit big-endian
    S16Be,
    /// Signed 24-bit little-endian (in 32-bit container)
    S24Le,
    /// Signed 32-bit little-endian
    S32Le,
    /// 32-bit float little-endian
    F32Le,
}

impl SampleFormat {
    /// Get bytes per sample
    pub const fn bytes_per_sample(&self) -> usize {
        match self {
            SampleFormat::U8 => 1,
            SampleFormat::S16Le | SampleFormat::S16Be => 2,
            SampleFormat::S24Le | SampleFormat::S32Le | SampleFormat::F32Le => 4,
        }
    }

    /// Get bits per sample
    pub const fn bits_per_sample(&self) -> u32 {
        match self {
            SampleFormat::U8 => 8,
            SampleFormat::S16Le | SampleFormat::S16Be => 16,
            SampleFormat::S24Le => 24,
            SampleFormat::S32Le | SampleFormat::F32Le => 32,
        }
    }

    /// Check if format is signed
    pub const fn is_signed(&self) -> bool {
        !matches!(self, SampleFormat::U8)
    }

    /// Check if format is floating point
    pub const fn is_float(&self) -> bool {
        matches!(self, SampleFormat::F32Le)
    }
}

/// Common sample rates
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SampleRate {
    /// 8000 Hz (telephone)
    Hz8000,
    /// 11025 Hz
    Hz11025,
    /// 16000 Hz (wideband)
    Hz16000,
    /// 22050 Hz
    Hz22050,
    /// 32000 Hz
    Hz32000,
    /// 44100 Hz (CD quality)
    #[default]
    Hz44100,
    /// 48000 Hz (DVD/DAT quality)
    Hz48000,
    /// 96000 Hz (high-resolution)
    Hz96000,
    /// 192000 Hz (studio quality)
    Hz192000,
}

impl SampleRate {
    /// Get rate in Hz
    pub const fn hz(&self) -> u32 {
        match self {
            SampleRate::Hz8000 => 8000,
            SampleRate::Hz11025 => 11025,
            SampleRate::Hz16000 => 16000,
            SampleRate::Hz22050 => 22050,
            SampleRate::Hz32000 => 32000,
            SampleRate::Hz44100 => 44100,
            SampleRate::Hz48000 => 48000,
            SampleRate::Hz96000 => 96000,
            SampleRate::Hz192000 => 192000,
        }
    }

    /// Create from Hz value
    pub fn from_hz(hz: u32) -> Option<Self> {
        match hz {
            8000 => Some(SampleRate::Hz8000),
            11025 => Some(SampleRate::Hz11025),
            16000 => Some(SampleRate::Hz16000),
            22050 => Some(SampleRate::Hz22050),
            32000 => Some(SampleRate::Hz32000),
            44100 => Some(SampleRate::Hz44100),
            48000 => Some(SampleRate::Hz48000),
            96000 => Some(SampleRate::Hz96000),
            192000 => Some(SampleRate::Hz192000),
            _ => None,
        }
    }
}

/// Channel layout
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChannelLayout {
    /// Mono (1 channel)
    Mono,
    /// Stereo (2 channels)
    #[default]
    Stereo,
    /// 2.1 (stereo + LFE)
    Surround21,
    /// Quadraphonic (4 channels)
    Quad,
    /// 5.1 surround
    Surround51,
    /// 7.1 surround
    Surround71,
}

impl ChannelLayout {
    /// Get number of channels
    pub const fn channels(&self) -> u32 {
        match self {
            ChannelLayout::Mono => 1,
            ChannelLayout::Stereo => 2,
            ChannelLayout::Surround21 => 3,
            ChannelLayout::Quad => 4,
            ChannelLayout::Surround51 => 6,
            ChannelLayout::Surround71 => 8,
        }
    }

    /// Create from channel count
    pub fn from_channels(channels: u32) -> Option<Self> {
        match channels {
            1 => Some(ChannelLayout::Mono),
            2 => Some(ChannelLayout::Stereo),
            3 => Some(ChannelLayout::Surround21),
            4 => Some(ChannelLayout::Quad),
            6 => Some(ChannelLayout::Surround51),
            8 => Some(ChannelLayout::Surround71),
            _ => None,
        }
    }
}

/// Audio stream parameters
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioParams {
    /// Sample format
    pub format: SampleFormat,
    /// Sample rate
    pub rate: SampleRate,
    /// Channel layout
    pub channels: ChannelLayout,
}

impl Default for AudioParams {
    fn default() -> Self {
        Self {
            format: SampleFormat::S16Le,
            rate: SampleRate::Hz44100,
            channels: ChannelLayout::Stereo,
        }
    }
}

impl AudioParams {
    /// Create new audio parameters
    pub const fn new(format: SampleFormat, rate: SampleRate, channels: ChannelLayout) -> Self {
        Self {
            format,
            rate,
            channels,
        }
    }

    /// CD quality stereo (44.1kHz, 16-bit, stereo)
    pub const CD_QUALITY: AudioParams = AudioParams {
        format: SampleFormat::S16Le,
        rate: SampleRate::Hz44100,
        channels: ChannelLayout::Stereo,
    };

    /// DVD quality (48kHz, 16-bit, stereo)
    pub const DVD_QUALITY: AudioParams = AudioParams {
        format: SampleFormat::S16Le,
        rate: SampleRate::Hz48000,
        channels: ChannelLayout::Stereo,
    };

    /// High resolution (96kHz, 24-bit, stereo)
    pub const HIGH_RES: AudioParams = AudioParams {
        format: SampleFormat::S24Le,
        rate: SampleRate::Hz96000,
        channels: ChannelLayout::Stereo,
    };

    /// Get bytes per frame (all channels for one sample)
    pub const fn bytes_per_frame(&self) -> usize {
        self.format.bytes_per_sample() * self.channels.channels() as usize
    }

    /// Get bytes per second
    pub const fn bytes_per_second(&self) -> usize {
        self.bytes_per_frame() * self.rate.hz() as usize
    }

    /// Calculate duration in microseconds for given byte count
    pub fn duration_us(&self, bytes: usize) -> u64 {
        let frames = bytes / self.bytes_per_frame();
        (frames as u64 * 1_000_000) / self.rate.hz() as u64
    }

    /// Calculate bytes for given duration in microseconds
    pub fn bytes_for_duration_us(&self, duration_us: u64) -> usize {
        let frames = (duration_us * self.rate.hz() as u64) / 1_000_000;
        frames as usize * self.bytes_per_frame()
    }
}

/// Audio buffer for PCM data
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Raw sample data
    data: Vec<u8>,
    /// Audio parameters
    params: AudioParams,
    /// Read position
    read_pos: usize,
    /// Write position
    write_pos: usize,
    /// Buffer capacity
    capacity: usize,
}

impl AudioBuffer {
    /// Create new audio buffer with given capacity in frames
    pub fn new(params: AudioParams, capacity_frames: usize) -> Self {
        let capacity = capacity_frames * params.bytes_per_frame();
        Self {
            data: vec![0u8; capacity],
            params,
            read_pos: 0,
            write_pos: 0,
            capacity,
        }
    }

    /// Create buffer with duration capacity
    pub fn with_duration_ms(params: AudioParams, duration_ms: u32) -> Self {
        let capacity_frames = (params.rate.hz() * duration_ms) / 1000;
        Self::new(params, capacity_frames as usize)
    }

    /// Get audio parameters
    pub fn params(&self) -> AudioParams {
        self.params
    }

    /// Get available bytes for reading
    pub fn available(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.write_pos - self.read_pos
        } else {
            self.capacity - self.read_pos + self.write_pos
        }
    }

    /// Get available space for writing
    pub fn space(&self) -> usize {
        self.capacity - self.available() - 1
    }

    /// Check if buffer is empty
    pub fn is_empty(&self) -> bool {
        self.read_pos == self.write_pos
    }

    /// Check if buffer is full
    pub fn is_full(&self) -> bool {
        self.space() == 0
    }

    /// Write samples to buffer
    pub fn write(&mut self, samples: &[u8]) -> usize {
        let space = self.space();
        let to_write = samples.len().min(space);

        if to_write == 0 {
            return 0;
        }

        let first_part = (self.capacity - self.write_pos).min(to_write);
        self.data[self.write_pos..self.write_pos + first_part]
            .copy_from_slice(&samples[..first_part]);

        if to_write > first_part {
            let second_part = to_write - first_part;
            self.data[..second_part].copy_from_slice(&samples[first_part..to_write]);
            self.write_pos = second_part;
        } else {
            self.write_pos += first_part;
            if self.write_pos >= self.capacity {
                self.write_pos = 0;
            }
        }

        to_write
    }

    /// Read samples from buffer
    pub fn read(&mut self, output: &mut [u8]) -> usize {
        let available = self.available();
        let to_read = output.len().min(available);

        if to_read == 0 {
            return 0;
        }

        let first_part = (self.capacity - self.read_pos).min(to_read);
        output[..first_part].copy_from_slice(&self.data[self.read_pos..self.read_pos + first_part]);

        if to_read > first_part {
            let second_part = to_read - first_part;
            output[first_part..to_read].copy_from_slice(&self.data[..second_part]);
            self.read_pos = second_part;
        } else {
            self.read_pos += first_part;
            if self.read_pos >= self.capacity {
                self.read_pos = 0;
            }
        }

        to_read
    }

    /// Peek at samples without consuming
    pub fn peek(&self, output: &mut [u8]) -> usize {
        let available = self.available();
        let to_read = output.len().min(available);

        if to_read == 0 {
            return 0;
        }

        let first_part = (self.capacity - self.read_pos).min(to_read);
        output[..first_part].copy_from_slice(&self.data[self.read_pos..self.read_pos + first_part]);

        if to_read > first_part {
            let second_part = to_read - first_part;
            output[first_part..to_read].copy_from_slice(&self.data[..second_part]);
        }

        to_read
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.read_pos = 0;
        self.write_pos = 0;
    }

    /// Get raw data slice (for direct access)
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Stream direction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamDirection {
    /// Playback (output)
    Playback,
    /// Capture (input)
    Capture,
}

/// Stream state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StreamState {
    /// Stream is stopped
    #[default]
    Stopped,
    /// Stream is running
    Running,
    /// Stream is paused
    Paused,
    /// Stream has an error
    Error,
}

/// Audio stream trait
pub trait AudioStream {
    /// Get stream parameters
    fn params(&self) -> AudioParams;

    /// Get stream direction
    fn direction(&self) -> StreamDirection;

    /// Get current state
    fn state(&self) -> StreamState;

    /// Start the stream
    fn start(&mut self) -> bool;

    /// Stop the stream
    fn stop(&mut self) -> bool;

    /// Pause the stream
    fn pause(&mut self) -> bool;

    /// Resume from pause
    fn resume(&mut self) -> bool;

    /// Write samples (for playback)
    fn write(&mut self, samples: &[u8]) -> usize;

    /// Read samples (for capture)
    fn read(&mut self, output: &mut [u8]) -> usize;

    /// Get available bytes (for capture) or space (for playback)
    fn available(&self) -> usize;

    /// Get buffer underrun count
    fn underruns(&self) -> u64;
}

/// PCM stream implementation
#[derive(Debug)]
pub struct PcmStream {
    /// Stream parameters
    params: AudioParams,
    /// Stream direction
    direction: StreamDirection,
    /// Current state
    state: StreamState,
    /// Sample buffer
    buffer: AudioBuffer,
    /// Underrun counter
    underruns: AtomicU64,
    /// Bytes processed
    bytes_processed: AtomicU64,
}

impl PcmStream {
    /// Create new PCM stream
    pub fn new(params: AudioParams, direction: StreamDirection, buffer_ms: u32) -> Self {
        Self {
            params,
            direction,
            state: StreamState::Stopped,
            buffer: AudioBuffer::with_duration_ms(params, buffer_ms),
            underruns: AtomicU64::new(0),
            bytes_processed: AtomicU64::new(0),
        }
    }

    /// Get bytes processed
    pub fn bytes_processed(&self) -> u64 {
        self.bytes_processed.load(Ordering::Relaxed)
    }

    /// Reset stream
    pub fn reset(&mut self) {
        self.state = StreamState::Stopped;
        self.buffer.clear();
        self.underruns.store(0, Ordering::Relaxed);
        self.bytes_processed.store(0, Ordering::Relaxed);
    }
}

impl AudioStream for PcmStream {
    fn params(&self) -> AudioParams {
        self.params
    }

    fn direction(&self) -> StreamDirection {
        self.direction
    }

    fn state(&self) -> StreamState {
        self.state
    }

    fn start(&mut self) -> bool {
        if self.state == StreamState::Stopped || self.state == StreamState::Paused {
            self.state = StreamState::Running;
            true
        } else {
            false
        }
    }

    fn stop(&mut self) -> bool {
        if self.state == StreamState::Running || self.state == StreamState::Paused {
            self.state = StreamState::Stopped;
            self.buffer.clear();
            true
        } else {
            false
        }
    }

    fn pause(&mut self) -> bool {
        if self.state == StreamState::Running {
            self.state = StreamState::Paused;
            true
        } else {
            false
        }
    }

    fn resume(&mut self) -> bool {
        if self.state == StreamState::Paused {
            self.state = StreamState::Running;
            true
        } else {
            false
        }
    }

    fn write(&mut self, samples: &[u8]) -> usize {
        if self.state != StreamState::Running && self.state != StreamState::Paused {
            return 0;
        }
        let written = self.buffer.write(samples);
        self.bytes_processed
            .fetch_add(written as u64, Ordering::Relaxed);
        written
    }

    fn read(&mut self, output: &mut [u8]) -> usize {
        if self.state != StreamState::Running {
            return 0;
        }

        let read = self.buffer.read(output);
        if read < output.len() && self.state == StreamState::Running {
            // Underrun - fill with silence
            let silence_start = read;
            match self.params.format {
                SampleFormat::U8 => {
                    // Unsigned 8-bit silence is 128
                    output[silence_start..].fill(128);
                }
                _ => {
                    // Signed formats silence is 0
                    output[silence_start..].fill(0);
                }
            }
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        self.bytes_processed
            .fetch_add(read as u64, Ordering::Relaxed);
        read
    }

    fn available(&self) -> usize {
        match self.direction {
            StreamDirection::Playback => self.buffer.space(),
            StreamDirection::Capture => self.buffer.available(),
        }
    }

    fn underruns(&self) -> u64 {
        self.underruns.load(Ordering::Relaxed)
    }
}

/// Volume level (0-255)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Volume(pub u8);

impl Volume {
    /// Muted (0%)
    pub const MUTED: Volume = Volume(0);
    /// Full volume (100%)
    pub const MAX: Volume = Volume(255);
    /// Half volume (50%)
    pub const HALF: Volume = Volume(128);

    /// Create from percentage (0-100)
    pub fn from_percent(percent: u8) -> Self {
        Volume(((percent.min(100) as u32 * 255) / 100) as u8)
    }

    /// Get as percentage
    pub fn percent(&self) -> u8 {
        ((self.0 as u32 * 100) / 255) as u8
    }

    /// Check if muted
    pub fn is_muted(&self) -> bool {
        self.0 == 0
    }

    /// Get linear multiplier (0.0 - 1.0)
    pub fn linear(&self) -> f32 {
        self.0 as f32 / 255.0
    }

    /// Apply volume to sample (S16)
    pub fn apply_s16(&self, sample: i16) -> i16 {
        ((sample as i32 * self.0 as i32) / 255) as i16
    }
}

/// Stereo volume control
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StereoVolume {
    /// Left channel volume
    pub left: Volume,
    /// Right channel volume
    pub right: Volume,
    /// Master mute
    pub muted: bool,
}

impl StereoVolume {
    /// Create with same volume for both channels
    pub fn mono(volume: Volume) -> Self {
        Self {
            left: volume,
            right: volume,
            muted: false,
        }
    }

    /// Create with different volumes
    pub fn stereo(left: Volume, right: Volume) -> Self {
        Self {
            left,
            right,
            muted: false,
        }
    }

    /// Full volume
    pub const MAX: StereoVolume = StereoVolume {
        left: Volume::MAX,
        right: Volume::MAX,
        muted: false,
    };

    /// Muted
    pub const MUTED: StereoVolume = StereoVolume {
        left: Volume::MUTED,
        right: Volume::MUTED,
        muted: true,
    };

    /// Get effective left volume (considering mute)
    pub fn effective_left(&self) -> Volume {
        if self.muted {
            Volume::MUTED
        } else {
            self.left
        }
    }

    /// Get effective right volume (considering mute)
    pub fn effective_right(&self) -> Volume {
        if self.muted {
            Volume::MUTED
        } else {
            self.right
        }
    }
}

/// Audio mixer for combining multiple streams
pub struct AudioMixer {
    /// Output parameters
    params: AudioParams,
    /// Input streams with volume
    inputs: Vec<(AudioBuffer, StereoVolume)>,
    /// Master volume
    master: StereoVolume,
    /// Mix buffer
    mix_buffer: Vec<i32>,
    /// Output buffer
    output_buffer: Vec<u8>,
}

impl AudioMixer {
    /// Create new mixer
    pub fn new(params: AudioParams) -> Self {
        Self {
            params,
            inputs: Vec::new(),
            master: StereoVolume::MAX,
            mix_buffer: Vec::new(),
            output_buffer: Vec::new(),
        }
    }

    /// Get output parameters
    pub fn params(&self) -> AudioParams {
        self.params
    }

    /// Add input stream
    pub fn add_input(&mut self, buffer: AudioBuffer, volume: StereoVolume) -> usize {
        let index = self.inputs.len();
        self.inputs.push((buffer, volume));
        index
    }

    /// Remove input stream
    pub fn remove_input(&mut self, index: usize) -> Option<AudioBuffer> {
        if index < self.inputs.len() {
            Some(self.inputs.remove(index).0)
        } else {
            None
        }
    }

    /// Set input volume
    pub fn set_input_volume(&mut self, index: usize, volume: StereoVolume) {
        if let Some(input) = self.inputs.get_mut(index) {
            input.1 = volume;
        }
    }

    /// Set master volume
    pub fn set_master_volume(&mut self, volume: StereoVolume) {
        self.master = volume;
    }

    /// Get master volume
    pub fn master_volume(&self) -> StereoVolume {
        self.master
    }

    /// Get input count
    pub fn input_count(&self) -> usize {
        self.inputs.len()
    }

    /// Write to input buffer
    pub fn write_input(&mut self, index: usize, samples: &[u8]) -> usize {
        if let Some(input) = self.inputs.get_mut(index) {
            input.0.write(samples)
        } else {
            0
        }
    }

    /// Mix all inputs and produce output
    pub fn mix(&mut self, output: &mut [u8]) -> usize {
        if self.inputs.is_empty() || self.master.muted {
            // Fill with silence
            match self.params.format {
                SampleFormat::U8 => output.fill(128),
                _ => output.fill(0),
            }
            return output.len();
        }

        let frame_size = self.params.bytes_per_frame();
        let frames = output.len() / frame_size;
        let channels = self.params.channels.channels() as usize;

        // Ensure mix buffer is large enough
        let samples_needed = frames * channels;
        self.mix_buffer.resize(samples_needed, 0);
        self.mix_buffer.fill(0);

        // Temporary buffer for reading
        let read_size = frames * frame_size;
        self.output_buffer.resize(read_size, 0);

        // Mix all inputs
        for (buffer, volume) in &mut self.inputs {
            let read = buffer.read(&mut self.output_buffer[..read_size]);
            if read == 0 {
                continue;
            }

            let frames_read = read / frame_size;

            // Convert and mix based on format
            match self.params.format {
                SampleFormat::S16Le => {
                    for f in 0..frames_read {
                        for c in 0..channels {
                            let offset = (f * channels + c) * 2;
                            if offset + 1 < read {
                                let sample = i16::from_le_bytes([
                                    self.output_buffer[offset],
                                    self.output_buffer[offset + 1],
                                ]);
                                let vol = if c == 0 || channels == 1 {
                                    volume.effective_left()
                                } else {
                                    volume.effective_right()
                                };
                                self.mix_buffer[f * channels + c] += vol.apply_s16(sample) as i32;
                            }
                        }
                    }
                }
                SampleFormat::U8 => {
                    for f in 0..frames_read {
                        for c in 0..channels {
                            let offset = f * channels + c;
                            if offset < read {
                                let sample = (self.output_buffer[offset] as i16 - 128) * 256;
                                let vol = if c == 0 || channels == 1 {
                                    volume.effective_left()
                                } else {
                                    volume.effective_right()
                                };
                                self.mix_buffer[f * channels + c] += vol.apply_s16(sample) as i32;
                            }
                        }
                    }
                }
                _ => {
                    // Other formats: just copy first input
                    output[..read].copy_from_slice(&self.output_buffer[..read]);
                    return read;
                }
            }
        }

        // Apply master volume and convert back
        let master_left = self.master.effective_left();
        let master_right = self.master.effective_right();

        match self.params.format {
            SampleFormat::S16Le => {
                for f in 0..frames {
                    for c in 0..channels {
                        let idx = f * channels + c;
                        let master = if c == 0 || channels == 1 {
                            master_left
                        } else {
                            master_right
                        };
                        let mixed = self.mix_buffer[idx].clamp(-32768, 32767) as i16;
                        let final_sample = master.apply_s16(mixed);
                        let offset = idx * 2;
                        let bytes = final_sample.to_le_bytes();
                        output[offset] = bytes[0];
                        output[offset + 1] = bytes[1];
                    }
                }
            }
            SampleFormat::U8 => {
                for f in 0..frames {
                    for c in 0..channels {
                        let idx = f * channels + c;
                        let master = if c == 0 || channels == 1 {
                            master_left
                        } else {
                            master_right
                        };
                        let mixed = (self.mix_buffer[idx] / 256).clamp(-128, 127) as i8;
                        let final_sample =
                            ((master.apply_s16(mixed as i16 * 256) / 256) + 128) as u8;
                        output[idx] = final_sample;
                    }
                }
            }
            _ => {}
        }

        frames * frame_size
    }
}

/// Audio device statistics
#[derive(Debug, Default)]
pub struct AudioStats {
    /// Bytes played
    pub bytes_played: AtomicU64,
    /// Bytes captured
    pub bytes_captured: AtomicU64,
    /// Playback underruns
    pub playback_underruns: AtomicU64,
    /// Capture overruns
    pub capture_overruns: AtomicU64,
    /// Interrupts generated
    pub interrupts: AtomicU64,
}

impl AudioStats {
    /// Create new stats
    pub fn new() -> Self {
        Self::default()
    }

    /// Record bytes played
    pub fn record_played(&self, bytes: u64) {
        self.bytes_played.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record bytes captured
    pub fn record_captured(&self, bytes: u64) {
        self.bytes_captured.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record playback underrun
    pub fn record_underrun(&self) {
        self.playback_underruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record capture overrun
    pub fn record_overrun(&self) {
        self.capture_overruns.fetch_add(1, Ordering::Relaxed);
    }

    /// Record interrupt
    pub fn record_interrupt(&self) {
        self.interrupts.fetch_add(1, Ordering::Relaxed);
    }

    /// Get snapshot
    pub fn snapshot(&self) -> AudioStatsSnapshot {
        AudioStatsSnapshot {
            bytes_played: self.bytes_played.load(Ordering::Relaxed),
            bytes_captured: self.bytes_captured.load(Ordering::Relaxed),
            playback_underruns: self.playback_underruns.load(Ordering::Relaxed),
            capture_overruns: self.capture_overruns.load(Ordering::Relaxed),
            interrupts: self.interrupts.load(Ordering::Relaxed),
        }
    }

    /// Reset stats
    pub fn reset(&self) {
        self.bytes_played.store(0, Ordering::Relaxed);
        self.bytes_captured.store(0, Ordering::Relaxed);
        self.playback_underruns.store(0, Ordering::Relaxed);
        self.capture_overruns.store(0, Ordering::Relaxed);
        self.interrupts.store(0, Ordering::Relaxed);
    }
}

/// Snapshot of audio statistics
#[derive(Debug, Clone, Default)]
pub struct AudioStatsSnapshot {
    /// Bytes played
    pub bytes_played: u64,
    /// Bytes captured
    pub bytes_captured: u64,
    /// Playback underruns
    pub playback_underruns: u64,
    /// Capture overruns
    pub capture_overruns: u64,
    /// Interrupts generated
    pub interrupts: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_format_bytes() {
        assert_eq!(SampleFormat::U8.bytes_per_sample(), 1);
        assert_eq!(SampleFormat::S16Le.bytes_per_sample(), 2);
        assert_eq!(SampleFormat::S24Le.bytes_per_sample(), 4);
        assert_eq!(SampleFormat::S32Le.bytes_per_sample(), 4);
        assert_eq!(SampleFormat::F32Le.bytes_per_sample(), 4);
    }

    #[test]
    fn test_sample_format_bits() {
        assert_eq!(SampleFormat::U8.bits_per_sample(), 8);
        assert_eq!(SampleFormat::S16Le.bits_per_sample(), 16);
        assert_eq!(SampleFormat::S24Le.bits_per_sample(), 24);
        assert_eq!(SampleFormat::S32Le.bits_per_sample(), 32);
    }

    #[test]
    fn test_sample_format_properties() {
        assert!(!SampleFormat::U8.is_signed());
        assert!(SampleFormat::S16Le.is_signed());
        assert!(!SampleFormat::S16Le.is_float());
        assert!(SampleFormat::F32Le.is_float());
    }

    #[test]
    fn test_sample_rate_hz() {
        assert_eq!(SampleRate::Hz8000.hz(), 8000);
        assert_eq!(SampleRate::Hz44100.hz(), 44100);
        assert_eq!(SampleRate::Hz48000.hz(), 48000);
        assert_eq!(SampleRate::Hz192000.hz(), 192000);
    }

    #[test]
    fn test_sample_rate_from_hz() {
        assert_eq!(SampleRate::from_hz(44100), Some(SampleRate::Hz44100));
        assert_eq!(SampleRate::from_hz(48000), Some(SampleRate::Hz48000));
        assert_eq!(SampleRate::from_hz(12345), None);
    }

    #[test]
    fn test_channel_layout() {
        assert_eq!(ChannelLayout::Mono.channels(), 1);
        assert_eq!(ChannelLayout::Stereo.channels(), 2);
        assert_eq!(ChannelLayout::Surround51.channels(), 6);
        assert_eq!(ChannelLayout::Surround71.channels(), 8);
    }

    #[test]
    fn test_channel_layout_from_channels() {
        assert_eq!(ChannelLayout::from_channels(1), Some(ChannelLayout::Mono));
        assert_eq!(ChannelLayout::from_channels(2), Some(ChannelLayout::Stereo));
        assert_eq!(ChannelLayout::from_channels(5), None);
    }

    #[test]
    fn test_audio_params_default() {
        let params = AudioParams::default();
        assert_eq!(params.format, SampleFormat::S16Le);
        assert_eq!(params.rate, SampleRate::Hz44100);
        assert_eq!(params.channels, ChannelLayout::Stereo);
    }

    #[test]
    fn test_audio_params_bytes_per_frame() {
        let params = AudioParams::CD_QUALITY;
        assert_eq!(params.bytes_per_frame(), 4); // 2 bytes * 2 channels
    }

    #[test]
    fn test_audio_params_bytes_per_second() {
        let params = AudioParams::CD_QUALITY;
        assert_eq!(params.bytes_per_second(), 176400); // 44100 * 4
    }

    #[test]
    fn test_audio_params_duration() {
        let params = AudioParams::CD_QUALITY;
        // 1 second of audio
        let bytes = params.bytes_for_duration_us(1_000_000);
        assert_eq!(bytes, 176400);

        // Reverse calculation
        let duration = params.duration_us(176400);
        assert_eq!(duration, 1_000_000);
    }

    #[test]
    fn test_audio_buffer_creation() {
        let params = AudioParams::CD_QUALITY;
        let buffer = AudioBuffer::new(params, 1024);
        assert!(buffer.is_empty());
        assert!(!buffer.is_full());
    }

    #[test]
    fn test_audio_buffer_write_read() {
        let params = AudioParams::CD_QUALITY;
        let mut buffer = AudioBuffer::new(params, 256);

        let data = vec![0u8; 512];
        let written = buffer.write(&data);
        assert_eq!(written, 512);
        assert!(!buffer.is_empty());

        let mut output = vec![0u8; 256];
        let read = buffer.read(&mut output);
        assert_eq!(read, 256);
    }

    #[test]
    fn test_audio_buffer_circular() {
        let params = AudioParams::new(SampleFormat::U8, SampleRate::Hz8000, ChannelLayout::Mono);
        let mut buffer = AudioBuffer::new(params, 16);

        // Write some data
        let data = vec![1u8; 8];
        buffer.write(&data);

        // Read it back
        let mut output = vec![0u8; 8];
        buffer.read(&mut output);
        assert_eq!(output, vec![1u8; 8]);

        // Write more (wraps around)
        let data2 = vec![2u8; 12];
        let written = buffer.write(&data2);
        assert_eq!(written, 12);

        // Read it back
        let mut output2 = vec![0u8; 12];
        let read = buffer.read(&mut output2);
        assert_eq!(read, 12);
        assert_eq!(output2, vec![2u8; 12]);
    }

    #[test]
    fn test_audio_buffer_peek() {
        let params = AudioParams::CD_QUALITY;
        let mut buffer = AudioBuffer::new(params, 256);

        let data = vec![42u8; 100];
        buffer.write(&data);

        let mut peek_output = vec![0u8; 50];
        let peeked = buffer.peek(&mut peek_output);
        assert_eq!(peeked, 50);
        assert_eq!(peek_output[0], 42);

        // Buffer should still have all data
        assert_eq!(buffer.available(), 100);
    }

    #[test]
    fn test_pcm_stream_creation() {
        let params = AudioParams::CD_QUALITY;
        let stream = PcmStream::new(params, StreamDirection::Playback, 100);
        assert_eq!(stream.state(), StreamState::Stopped);
        assert_eq!(stream.direction(), StreamDirection::Playback);
    }

    #[test]
    fn test_pcm_stream_state_transitions() {
        let params = AudioParams::CD_QUALITY;
        let mut stream = PcmStream::new(params, StreamDirection::Playback, 100);

        assert!(stream.start());
        assert_eq!(stream.state(), StreamState::Running);

        assert!(stream.pause());
        assert_eq!(stream.state(), StreamState::Paused);

        assert!(stream.resume());
        assert_eq!(stream.state(), StreamState::Running);

        assert!(stream.stop());
        assert_eq!(stream.state(), StreamState::Stopped);
    }

    #[test]
    fn test_pcm_stream_write() {
        let params = AudioParams::CD_QUALITY;
        let mut stream = PcmStream::new(params, StreamDirection::Playback, 100);

        // Can't write when stopped
        let data = vec![0u8; 100];
        assert_eq!(stream.write(&data), 0);

        stream.start();
        let written = stream.write(&data);
        assert!(written > 0);
    }

    #[test]
    fn test_volume() {
        assert_eq!(Volume::MUTED.0, 0);
        assert_eq!(Volume::MAX.0, 255);
        assert_eq!(Volume::HALF.0, 128);

        let vol = Volume::from_percent(50);
        assert!(vol.0 >= 127 && vol.0 <= 128);

        assert!(Volume::MUTED.is_muted());
        assert!(!Volume::MAX.is_muted());
    }

    #[test]
    fn test_volume_apply() {
        let vol = Volume::HALF;
        let sample: i16 = 1000;
        let result = vol.apply_s16(sample);
        assert!(result >= 490 && result <= 510);

        let muted = Volume::MUTED;
        assert_eq!(muted.apply_s16(1000), 0);
    }

    #[test]
    fn test_stereo_volume() {
        let vol = StereoVolume::mono(Volume::HALF);
        assert_eq!(vol.left, vol.right);
        assert!(!vol.muted);

        let stereo = StereoVolume::stereo(Volume::MAX, Volume::HALF);
        assert_eq!(stereo.left.0, 255);
        assert_eq!(stereo.right.0, 128);
    }

    #[test]
    fn test_stereo_volume_effective() {
        let mut vol = StereoVolume::MAX;
        assert_eq!(vol.effective_left().0, 255);

        vol.muted = true;
        assert_eq!(vol.effective_left().0, 0);
        assert_eq!(vol.effective_right().0, 0);
    }

    #[test]
    fn test_audio_mixer_creation() {
        let params = AudioParams::CD_QUALITY;
        let mixer = AudioMixer::new(params);
        assert_eq!(mixer.input_count(), 0);
    }

    #[test]
    fn test_audio_mixer_add_remove() {
        let params = AudioParams::CD_QUALITY;
        let mut mixer = AudioMixer::new(params);

        let buffer = AudioBuffer::new(params, 1024);
        let idx = mixer.add_input(buffer, StereoVolume::MAX);
        assert_eq!(mixer.input_count(), 1);

        mixer.remove_input(idx);
        assert_eq!(mixer.input_count(), 0);
    }

    #[test]
    fn test_audio_mixer_write_input() {
        let params = AudioParams::CD_QUALITY;
        let mut mixer = AudioMixer::new(params);

        let buffer = AudioBuffer::new(params, 1024);
        let idx = mixer.add_input(buffer, StereoVolume::MAX);

        let data = vec![0u8; 100];
        let written = mixer.write_input(idx, &data);
        assert_eq!(written, 100);
    }

    #[test]
    fn test_audio_mixer_mix_silence() {
        let params = AudioParams::CD_QUALITY;
        let mut mixer = AudioMixer::new(params);

        let mut output = vec![0xFFu8; 100];
        mixer.mix(&mut output);

        // Should be silence (0 for S16Le)
        assert!(output.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_audio_stats() {
        let stats = AudioStats::new();
        stats.record_played(1000);
        stats.record_captured(500);
        stats.record_underrun();
        stats.record_interrupt();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.bytes_played, 1000);
        assert_eq!(snapshot.bytes_captured, 500);
        assert_eq!(snapshot.playback_underruns, 1);
        assert_eq!(snapshot.interrupts, 1);
    }

    #[test]
    fn test_audio_stats_reset() {
        let stats = AudioStats::new();
        stats.record_played(1000);
        stats.reset();

        let snapshot = stats.snapshot();
        assert_eq!(snapshot.bytes_played, 0);
    }
}
