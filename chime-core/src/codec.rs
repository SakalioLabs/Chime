//! Codec trait definitions for audio decoders.

use crate::StreamInfo;
use crate::buffer::AudioBuffer;
use std::io::{Read, Seek};

/// Information returned after probing a file.
#[derive(Debug, Clone)]
pub struct CodecInfo {
    pub stream: StreamInfo,
    pub format_name: &'static str,
}

/// Decoded audio data — either PCM or raw DSD bytes.
#[derive(Debug)]
pub enum AudioData {
    /// Fully decoded PCM audio.
    Pcm(AudioBuffer),
    /// Raw DSD data (packed LSB-first, interleaved channels).
    Dsd {
        data: Vec<u8>,
        sample_rate: u32,
        channels: u16,
    },
}

/// Trait for audio format decoders.
pub trait AudioCodec: Send {
    /// Human-readable name of this codec.
    fn name(&self) -> &'static str;

    /// Probe the stream to determine if this codec can decode it.
    fn probe(&self, reader: &mut dyn ReadSeek) -> Result<CodecInfo, crate::ChimeError>;

    /// Decode the entire stream into AudioData.
    fn decode(&self, reader: &mut dyn ReadSeek) -> Result<AudioData, crate::ChimeError>;
}

/// Convenience alias for a readable + seekable stream.
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}