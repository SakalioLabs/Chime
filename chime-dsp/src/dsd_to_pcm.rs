//! DSD-to-PCM converter.
//!
//! Converts packed DSD data (1-bit per sample, LSB-first) to f32 PCM.
//! Supports DSD64, DSD128, DSD256, and DSD512.

use chime_core::ChimeError;
use chime_core::buffer::AudioBuffer;
use crate::filters::DecimationFilter;

/// Configuration for DSD-to-PCM conversion.
#[derive(Debug, Clone)]
pub struct DsdPcmConfig {
    /// Target PCM sample rate in Hz.
    pub target_sample_rate: u32,
    /// Whether to apply DC offset removal.
    pub remove_dc: bool,
    /// Apply 50kHz lowpass to reduce DSD out-of-band noise.
    pub apply_lowpass: bool,
}

impl Default for DsdPcmConfig {
    fn default() -> Self {
        Self {
            target_sample_rate: 176_400,
            remove_dc: true,
            apply_lowpass: true,
        }
    }
}

/// Converts raw DSD data to PCM audio.
pub struct DsdToPcmConverter {
    config: DsdPcmConfig,
}

impl DsdToPcmConverter {
    pub fn new(config: DsdPcmConfig) -> Self {
        Self { config }
    }

    /// Convert interleaved DSD bytes to an AudioBuffer.
    ///
    /// The DSD data should be interleaved per-channel (one byte = 8 DSD samples for one channel).
    /// Each bit represents one DSD sample, with MSB = earliest sample in time.
    pub fn convert(
        &self,
        dsd_data: &[u8],
        dsd_rate: u32,
        channels: u16,
    ) -> Result<AudioBuffer, ChimeError> {
        if dsd_data.is_empty() {
            return Err(ChimeError::InvalidData("Empty DSD data".into()));
        }
        if channels == 0 {
            return Err(ChimeError::InvalidData("Zero channels".into()));
        }
        let ch = channels as usize;

        // Step1: Unpack DSD bits to +1.0 / -1.0 f64 samples
        // DSD: bit=1 → +1.0, bit=0 → -1.0
        let total_bits = dsd_data.len() * 8;
        let frames = total_bits / ch;

        // De-interleave channels
        let mut channel_data: Vec<Vec<f64>> = vec![Vec::with_capacity(frames); ch];
        let mut bit_idx = 0;
        for &byte in dsd_data {
            for bit_pos in 0..8 {
                let ch_idx = bit_idx % ch;
                let bit = (byte >> (7 - bit_pos)) & 1;
                channel_data[ch_idx].push(if bit != 0 { 1.0 } else { -1.0 });
                bit_idx += 1;
            }
        }

        // Step2: Apply decimation filter per channel
        let dec_filter = DecimationFilter::design(dsd_rate, self.config.target_sample_rate);
        let mut pcm_channels: Vec<Vec<f64>> = Vec::with_capacity(ch);
        for chan in &channel_data {
            let filtered = dec_filter.apply(chan);
            pcm_channels.push(filtered);
        }

        // Step3: Optional DC removal (subtract mean)
        if self.config.remove_dc {
            for chan in &mut pcm_channels {
                let mean: f64 = chan.iter().sum::<f64>() / chan.len() as f64;
                for s in chan.iter_mut() {
                    *s -= mean;
                }
            }
        }

        // Step4: Soft-clip to [-1.0, 1.0] using tanh
        for chan in &mut pcm_channels {
            for s in chan.iter_mut() {
                *s = s.tanh();
            }
        }

        // Step5: Interleave channels for output
        let pcm_frames = pcm_channels[0].len();
        let mut interleaved = Vec::with_capacity(pcm_frames * ch);
        for i in 0..pcm_frames {
            for c in 0..ch {
                if i < pcm_channels[c].len() {
                    interleaved.push(pcm_channels[c][i] as f32);
                }
            }
        }

        Ok(AudioBuffer::new(interleaved, channels, self.config.target_sample_rate))
    }

    /// Convert a single channel of DSD to f32 PCM.
    pub fn convert_mono(&self, dsd_data: &[u8], dsd_rate: u32) -> Result<Vec<f32>, ChimeError> {
        let buf = self.convert(dsd_data, dsd_rate, 1)?;
        Ok(buf.samples)
    }

    /// Perform DoP (DSD over PCM) encoding: pack DSD bits into PCM frames.
    /// DoP uses16-bit or24-bit PCM to carry DSD data.
    /// Each PCM frame carries 8 or16 DSD bits per channel.
    pub fn encode_dop(dsd_data: &[u8], dsd_rate: u32, channels: u16, bits: u16) -> Vec<u8> {
        let ch = channels as usize;
        let bytes_per_frame = bits as usize / 8;
        let dsd_per_frame = bits as usize; // bits of DSD per PCM frame
        let marker = match bits {
            16 => 0x05u8, // DoP16 marker
            24 => 0x69u8, // DoP24 marker  
            _ => 0x05,
        };
        let mut output = Vec::new();
        let mut frame_nr = 0u8;
        let mut dsd_pos = 0;

        while dsd_pos < dsd_data.len() {
            for c in 0..ch {
                // Write marker byte
                output.push(marker ^ (frame_nr & 1));
                // Write DSD bytes
                for b in 0..(bytes_per_frame - 1) {
                    let idx = dsd_pos + c + b * ch;
                    if idx < dsd_data.len() {
                        output.push(dsd_data[idx]);
                    } else {
                        output.push(0);
                    }
                }
            }
            dsd_pos += ch * (bytes_per_frame - 1);
            frame_nr = frame_nr.wrapping_add(1);
        }

        output
    }
}