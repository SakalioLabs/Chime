//! Audio buffer abstraction for decoded audio data.

use crate::sample::SampleFormat;

/// A buffer holding decoded PCM audio data as interleaved f32 samples.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Interleaved f32 samples in [-1.0, 1.0].
    pub samples: Vec<f32>,
    /// Number of audio channels.
    pub channels: u16,
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Total number of frames (samples.len() / channels).
    pub frames: usize,
}

impl AudioBuffer {
    /// Create a new buffer from interleaved f32 data.
    pub fn new(samples: Vec<f32>, channels: u16, sample_rate: u32) -> Self {
        let frames = samples.len() / channels as usize;
        Self { samples, channels, sample_rate, frames }
    }

    /// Create a silent buffer of the given duration.
    pub fn silence(channels: u16, sample_rate: u32, frames: usize) -> Self {
        Self {
            samples: vec![0.0; frames * channels as usize],
            channels,
            sample_rate,
            frames,
        }
    }

    /// Convert raw bytes in the given PCM format to an AudioBuffer.
    pub fn from_pcm_bytes(
        bytes: &[u8],
        format: SampleFormat,
        channels: u16,
        sample_rate: u32,
    ) -> Self {
        let samples = crate::sample::bytes_to_f32_le(bytes, format);
        Self::new(samples, channels, sample_rate)
    }

    /// Get a slice of samples for a single channel (deinterleaved on the fly).
    pub fn channel_samples(&self, ch: u16) -> Vec<f32> {
        let ch = ch as usize;
        let n = self.channels as usize;
        self.samples.iter().skip(ch).step_by(n).copied().collect()
    }

    /// Duration in seconds.
    pub fn duration_secs(&self) -> f64 {
        self.frames as f64 / self.sample_rate as f64
    }
}
