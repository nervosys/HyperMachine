//! Audio and Sound Emulation
//!
//! This module provides audio device emulation including:
//! - Common audio types and mixing infrastructure
//! - AC97 audio controller
//! - Intel High Definition Audio (HDA)
//! - VirtIO sound device

pub mod core;
pub mod ac97;
pub mod hda;
pub mod virtio_sound;

// Re-export common types
pub use core::{
    AudioBuffer, AudioMixer, AudioParams, AudioStats, AudioStatsSnapshot, AudioStream,
    ChannelLayout, PcmStream, SampleFormat, SampleRate, StereoVolume, StreamDirection,
    StreamState, Volume,
};

// Re-export AC97 types
pub use ac97::{
    Ac97Controller, Ac97Mixer, Ac97Register, BufferDescriptor, ControlBits, DmaChannel,
    GlobalControl, GlobalStatus, StatusBits,
};

// Re-export HDA types
pub use hda::{
    HdaCodec, HdaController, PinConfig, StreamDescriptor, Widget, WidgetType,
};

// Re-export VirtIO sound types
pub use virtio_sound::{
    ChannelMap, Jack, JackType, PcmParams, PcmState, PcmStreamInfo, VirtioSndChmapPosition,
    VirtioSndDirection, VirtioSndPcmFormat, VirtioSndPcmRate, VirtioSndPcmStream,
    VirtioSndRequestType, VirtioSndStatus, VirtioSoundDevice,
};
