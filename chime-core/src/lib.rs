//! Chime Core — fundamental audio types, buffers, and codec abstractions.

pub mod sample;
pub mod buffer;
pub mod codec;
pub mod error;

pub use sample::SampleFormat;
pub use buffer::AudioBuffer;
pub use codec::{AudioCodec, CodecInfo, AudioData};
pub use error::ChimeError;

/// Describes the layout of audio channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    /// Channel count for non-standard layouts.
    Custom(u16),
}

impl ChannelLayout {
    pub fn channel_count(&self) -> u16 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Custom(n) => *n,
        }
    }
}

/// Describes an audio stream's format (for both PCM and DSD).
#[derive(Debug, Clone)]
pub enum StreamInfo {
    Pcm(PcmInfo),
    Dsd(DsdInfo),
}

#[derive(Debug, Clone)]
pub struct PcmInfo {
    pub sample_rate: u32,
    pub channels: u16,
    pub sample_format: SampleFormat,
    pub total_frames: Option<u64>,
    pub bits_per_sample: u16,
}

#[derive(Debug, Clone)]
pub struct DsdInfo {
    /// DSD sample rate in Hz (e.g. 2_822_400 for DSD64).
    pub sample_rate: u32,
    pub channels: u16,
    /// Bits per DSD sample (1 for standard DSD, 8 for packed).
    pub bits_per_sample: u16,
    /// Total DSD bytes per channel (not frames).
    pub total_bytes_per_ch: Option<u64>,
}

impl StreamInfo {
    pub fn channels(&self) -> u16 {
        match self {
            Self::Pcm(i) => i.channels,
            Self::Dsd(i) => i.channels,
        }
    }
    pub fn is_dsd(&self) -> bool {
        matches!(self, Self::Dsd(_))
    }
}