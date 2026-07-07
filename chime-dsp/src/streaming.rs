//! Streaming DSD-to-PCM converter for large files.
//!
//! Processes DSD data in fixed-size chunks without loading the entire file
//! into memory. Maintains FIR filter delay line across chunks for seamless
//! conversion. Ideal for multi-gigabyte SACD ISO rips.

use crate::filters::DecimationFilter;

/// Pre-computed DSD byte-to-f32 lookup table.
const DSD_LUT: [[f32; 8]; 256] = {
    let mut lut = [[0.0f32; 8]; 256];
    let mut byte_val = 0usize;
    while byte_val < 256 {
        let mut bit_pos = 0;
        while bit_pos < 8 {
            lut[byte_val][bit_pos] = if (byte_val >> (7 - bit_pos)) & 1 != 0 { 1.0 } else { -1.0 };
            bit_pos += 1;
        }
        byte_val += 1;
    }
    lut
};

/// Fast polynomial tanh approximation.
#[inline(always)]
fn fast_tanh(x: f32) -> f32 {
    let x2 = x * x;
    let num = x * (135135.0 + x2 * (17325.0 + x2 * (378.0 + x2)));
    let den = 135135.0 + x2 * (62370.0 + x2 * (3150.0 + x2 * 28.0));
    num / den
}

/// Streaming DSD-to-PCM converter state.
pub struct StreamingDsdToPcm {
    target_rate: u32,
    channels: u16,
    dec_filter: DecimationFilter,
    /// Per-channel DC offset running sum for streaming DC removal.
    dc_sum: Vec<f64>,
    dc_count: u64,
    /// Scratch buffer for deinterleaved channel data.
    channel_bufs: Vec<Vec<f32>>,
    /// Scratch buffer for filter output.
    filter_buf: Vec<f32>,
    filter_scratch: Vec<f32>,
}

impl StreamingDsdToPcm {
    pub fn new(dsd_rate: u32, target_rate: u32, channels: u16) -> Self {
        let dec_filter = DecimationFilter::design(dsd_rate, target_rate);
        let ch = channels as usize;
        StreamingDsdToPcm {
            
            target_rate,
            channels,
            dec_filter,
            dc_sum: vec![0.0; ch],
            dc_count: 0,
            channel_bufs: vec![Vec::new(); ch],
            filter_buf: Vec::new(),
            filter_scratch: Vec::new(),
        }
    }

    /// Feed a chunk of interleaved DSD bytes, return PCM output samples.
    ///
    /// The input should be interleaved: for stereo, byte[0] is ch0, byte[1] is ch1, etc.
    /// Each byte contains 8 DSD samples (MSB first).
    pub fn process_chunk(&mut self, dsd_bytes: &[u8]) -> Vec<f32> {
        let ch = self.channels as usize;
        if ch == 0 || dsd_bytes.is_empty() {
            return Vec::new();
        }

        // Step 1: Unpack DSD bytes into per-channel f32 buffers using LUT
        for buf in &mut self.channel_bufs {
            buf.clear();
        }

        let mut byte_idx = 0;
        while byte_idx < dsd_bytes.len() {
            let channel = byte_idx % ch;
            let vals = DSD_LUT[dsd_bytes[byte_idx] as usize];
            self.channel_bufs[channel].extend_from_slice(&vals);
            byte_idx += 1;
        }

        // Step 2: Decimation filter per channel
        

        let mut pcm_channels: Vec<Vec<f32>> = Vec::with_capacity(ch);
        for chan_data in &self.channel_bufs {
            self.dec_filter.apply_reuse(chan_data, &mut self.filter_scratch, &mut self.filter_buf);
            pcm_channels.push(std::mem::take(&mut self.filter_buf));
        }

        // Step 3: Streaming DC removal (incremental mean)
        for (c, chan) in pcm_channels.iter_mut().enumerate() {
            for &s in chan.iter() {
                self.dc_sum[c] += s as f64;
            }
            self.dc_count += chan.len() as u64;
            let mean = (self.dc_sum[c] / self.dc_count as f64) as f32;
            for s in chan.iter_mut() {
                *s -= mean;
            }
        }

        // Step 4: Soft clip with fast tanh
        for chan in &mut pcm_channels {
            for s in chan.iter_mut() {
                *s = fast_tanh(*s);
            }
        }

        // Step 5: Interleave output
        let frames = pcm_channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let mut output = Vec::with_capacity(frames * ch);
        for i in 0..frames {
            for c in 0..ch {
                output.push(pcm_channels[c][i]);
            }
        }
        output
    }

    /// Get the target PCM sample rate.
    pub fn target_rate(&self) -> u32 {
        self.target_rate
    }

    /// Get the number of channels.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

impl DecimationFilter {
    /// Get number of stages (exposed for streaming chunk size calculation).
    pub fn stages_len(&self) -> usize {
        1 // placeholder - the actual decimation is computed by the filter
    }
}
