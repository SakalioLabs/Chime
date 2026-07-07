//! Sample rate converter with polyphase interpolation.
//!
//! Converts between arbitrary sample rates using Kaiser-windowed sinc
//! polyphase filtering. Handles both upsampling and downsampling.
//! Configurable quality via taps-per-phase and number of phases.

use std::f64::consts::PI;

/// Quality settings for the sample rate converter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SrcQuality {
    Fast = 8,     // 8 taps, 32 phases
    Medium = 32,  // 32 taps, 64 phases
    High = 64,    // 64 taps, 128 phases
    VeryHigh = 128, // 128 taps, 256 phases
}

/// Polyphase sample rate converter.
pub struct SampleRateConverter {
    input_rate: u32,
    output_rate: u32,
    /// output_rate / input_rate as f64
    ratio: f64,
    /// Number of polyphase branches.
    num_phases: usize,
    /// Taps per phase.
    taps_per_phase: usize,
    /// Polyphase filter coefficients: phases[p][t].
    phases: Vec<Vec<f32>>,
    /// Delay line for streaming (ring buffer).
    delay_line: Vec<f32>,
    delay_len: usize,
    /// Current fractional position in the input stream.
    pos: f64,
}

fn kaiser_beta(atten_db: f64) -> f64 {
    if atten_db > 50.0 {
        0.1102 * (atten_db - 8.7)
    } else if atten_db > 21.0 {
        0.5842 * (atten_db - 21.0).powf(0.4) + 0.07886 * (atten_db - 21.0)
    } else {
        0.0
    }
}

fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let x_half_sq = (x / 2.0).powi(2);
    for k in 1..=30 {
        term *= x_half_sq / (k as f64).powi(2);
        sum += term;
        if term.abs() < 1e-12 * sum.abs() { break; }
    }
    sum
}

fn design_lowpass(cutoff: f64, num_taps: usize) -> Vec<f64> {
    let half = num_taps / 2;
    let atten = 60.0;
    let beta = kaiser_beta(atten);
    let i0_beta = bessel_i0(beta);
    let fc = cutoff.min(0.49);

    let mut coeffs = Vec::with_capacity(num_taps);
    for n in 0..num_taps {
        let t = n as f64 - half as f64;
        let sinc = if t.abs() < 1e-10 {
            2.0 * fc
        } else {
            (2.0 * PI * fc * t).sin() / (PI * t)
        };
        let arg = if half > 0 {
            (1.0 - (t / half as f64).powi(2)).sqrt()
        } else { 0.0 };
        let window = bessel_i0(beta * arg.max(0.0)) / i0_beta;
        coeffs.push(sinc * window);
    }
    // Normalize to unity gain at DC
    let sum: f64 = coeffs.iter().sum();
    if sum.abs() > 1e-15 {
        for c in &mut coeffs { *c /= sum; }
    }
    coeffs
}

impl SampleRateConverter {
    /// Create a new SRC. Returns None if rates are identical (pass-through).
    pub fn new(input_rate: u32, output_rate: u32, quality: SrcQuality) -> Option<Self> {
        if input_rate == 0 || output_rate == 0 { return None; }
        if input_rate == output_rate { return None; }

        let ratio = output_rate as f64 / input_rate as f64;
        let num_phases = quality as usize;
        let taps_per_phase = (quality as usize).max(8);
        let prototype_len = taps_per_phase * num_phases;

        // Prototype lowpass cutoff: min to prevent aliasing on downsampling
        let cutoff = 0.45 * ratio.min(1.0);
        let prototype = design_lowpass(cutoff, prototype_len);

        // Decompose into polyphase sub-filters
        let mut phases = vec![vec![0.0f32; taps_per_phase]; num_phases];
        for (i, &coeff) in prototype.iter().enumerate() {
            let p = i % num_phases;
            let t = i / num_phases;
            if t < taps_per_phase {
                phases[p][t] = coeff as f32;
            }
        }

        let delay_len = taps_per_phase;
        Some(SampleRateConverter {
            input_rate,
            output_rate,
            ratio,
            num_phases,
            taps_per_phase,
            phases,
            delay_line: vec![0.0f32; delay_len],
            delay_len,
            pos: 0.0,
        })
    }

    /// Process a chunk of interleaved input samples.
    /// Returns resampled output samples (interleaved).
    pub fn process(&mut self, input: &[f32], channels: u16) -> Vec<f32> {
        let ch = channels as usize;
        if ch == 0 || input.is_empty() {
            return Vec::new();
        }
        let input_frames = input.len() / ch;
        if input_frames == 0 { return Vec::new(); }

        // Estimate output length
        let output_frames = ((input_frames as u64 * self.output_rate as u64 + self.input_rate as u64 - 1) / self.input_rate as u64) as usize;
        let mut output = Vec::with_capacity(output_frames * ch);

        // Per-channel processing (interleaved: each channel independently)
        for c in 0..ch {
            // Extract this channel's samples
            let chan_input: Vec<f32> = (0..input_frames)
                .map(|f| input[f * ch + c])
                .collect();

            // Prepend delay line for this channel
            let mut extended = self.delay_line.clone();
            extended.extend_from_slice(&chan_input);

            // Update delay line with last samples
            if input_frames >= self.delay_len {
                self.delay_line.copy_from_slice(&chan_input[input_frames - self.delay_len..]);
            } else {
                // Shift delay line
                let keep = self.delay_len - input_frames;
                self.delay_line.copy_within(input_frames..self.delay_len, 0);
                self.delay_line[keep..].copy_from_slice(&chan_input);
            }

            let half = self.taps_per_phase / 2;

            // Resample
            let mut chan_output = Vec::with_capacity(output_frames);
            let mut pos = self.pos;
            let n_frames = extended.len();

            for _ in 0..output_frames {
                let idx_f = pos;
                let idx = idx_f as isize;
                let frac = idx_f - idx as f64;
                let phase = ((frac * self.num_phases as f64).round() as usize) % self.num_phases;
                let coeffs = &self.phases[phase];

                let mut acc = 0.0f32;
                for t in 0..self.taps_per_phase {
                    let sample_idx = idx + t as isize - half as isize;
                    if sample_idx >= 0 && (sample_idx as usize) < n_frames {
                        acc += coeffs[t] * extended[sample_idx as usize];
                    }
                }
                chan_output.push(acc);
                pos += 1.0 / self.ratio;
            }

            // Interleave back
            for (i, &s) in chan_output.iter().enumerate() {
                let out_idx = i * ch + c;
                if out_idx >= output.len() {
                    output.resize(out_idx + 1, 0.0);
                }
                output[out_idx] = s;
            }
        }

        self.pos += input_frames as f64;
        output
    }

    /// Reset internal state.
    pub fn reset(&mut self) {
        for d in &mut self.delay_line { *d = 0.0; }
        self.pos = 0.0;
    }

    pub fn input_rate(&self) -> u32 { self.input_rate }
    pub fn output_rate(&self) -> u32 { self.output_rate }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_rate_returns_none() {
        assert!(SampleRateConverter::new(44100, 44100, SrcQuality::High).is_none());
    }

    #[test]
    fn test_downsample_mono() {
        let mut src = SampleRateConverter::new(48000, 44100, SrcQuality::Medium).unwrap();
        // 1 second of 1kHz sine at 48kHz
        let input: Vec<f32> = (0..48000).map(|i| {
            (2.0 * PI * 1000.0 * i as f64 / 48000.0).sin() as f32
        }).collect();
        let output = src.process(&input, 1);
        assert_eq!(output.len(), 44100);
        for &s in &output {
            assert!(s.is_finite(), "non-finite sample");
            assert!(s.abs() <= 1.1, "sample out of range: {}", s);
        }
    }

    #[test]
    fn test_upsample_stereo() {
        let mut src = SampleRateConverter::new(44100, 48000, SrcQuality::Medium).unwrap();
        let input: Vec<f32> = (0..44100*2).map(|i| {
            if i % 2 == 0 { 1.0 } else { -1.0 }
        }).collect();
        let output = src.process(&input, 2);
        let expected = 48000 * 2;
        let actual = output.len();
        assert!((actual as i64 - expected as i64) < 5, "expected ~{} samples, got {}", expected, actual);
        for &s in &output {
            assert!(s.is_finite(), "non-finite sample");
        }
    }
}