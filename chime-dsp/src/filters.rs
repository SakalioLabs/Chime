//! FIR filters for DSD-to-PCM decimation.

/// A sinc-based decimation filter with Kaiser window.
///
/// This is the core DSP component for DSD-to-PCM conversion.
/// The filter specifications follow the SACD reference implementation:
/// - Passband: 0 to 50kHz (for PCM output at 100kHz+ sample rate)
/// - Stopband: > 100kHz with > 90dB attenuation
/// - Transition band: 50kHz to 100kHz
#[derive(Clone)]
pub struct SincFilter {
    /// Filter coefficients.
    pub coefficients: Vec<f64>,
    /// Decimation factor.
    pub decimation: usize,
    /// Filter length (number of taps).
    pub length: usize,
}

impl SincFilter {
    /// Design a lowpass sinc filter for DSD decimation.
    ///
    /// Parameters:
    /// - `dsd_rate`: DSD sample rate in Hz (e.g. 2_822_400 for DSD64)
    /// - `target_rate`: Target PCM sample rate in Hz (e.g. 176_400)
    /// - `cutoff_hz`: Cutoff frequency in Hz
    /// - `attenuation_db`: Desired stopband attenuation in dB
    pub fn design(dsd_rate: u32, target_rate: u32, cutoff_hz: f64, attenuation_db: f64) -> Self {
        let decimation = (dsd_rate as f64 / target_rate as f64).round() as usize;

        // Estimate filter order from desired attenuation
        // Kaiser window: N ≈ (A - 7.95) / (2.285 * Δω)
        // where A is attenuation in dB and Δω is transition width in radians
        let transition_width = 0.1; // normalized transition width
        let order = if attenuation_db > 50.0 {
            ((attenuation_db - 7.95) / (2.285 * std::f64::consts::PI * transition_width)).ceil() as usize
        } else {
            64
        };
        // Ensure odd length for symmetry
        let length = if order % 2 == 0 { order + 1 } else { order };
        let half = length / 2;

        let fc = cutoff_hz / dsd_rate as f64; // normalized cutoff
        let mut coeffs = Vec::with_capacity(length);

        // Kaiser window parameter
        let alpha = if attenuation_db > 50.0 {
            0.1102 * (attenuation_db - 8.7)
        } else if attenuation_db > 21.0 {
            0.5842 * (attenuation_db - 21.0).powf(0.4) + 0.07886 * (attenuation_db - 21.0)
        } else {
            0.0
        };

        let i0_alpha = bessel_i0(alpha);

        for n in 0..length {
            let n_f = n as f64 - half as f64;
            // Sinc function
            let sinc = if n_f.abs() < 1e-10 {
                2.0 * fc
            } else {
                (2.0 * std::f64::consts::PI * fc * n_f).sin() / (std::f64::consts::PI * n_f)
            };
            // Kaiser window
            let arg = 1.0 - ((n as f64 - half as f64) / half as f64).powi(2);
            let window = if arg >= 0.0 {
                bessel_i0(alpha * arg.sqrt()) / i0_alpha
            } else {
                0.0
            };
            coeffs.push(sinc * window);
        }

        // Normalize to unity DC gain
        let sum: f64 = coeffs.iter().sum();
        if sum.abs() > 1e-15 {
            for c in &mut coeffs {
                *c /= sum;
            }
        }

        SincFilter { coefficients: coeffs, decimation, length }
    }

    /// Apply the filter to an input signal with decimation.
    /// Input is at DSD rate, output is at DSD_rate / decimation.
    pub fn apply(&self, input: &[f64]) -> Vec<f64> {
        if input.is_empty() || self.decimation == 0 {
            return Vec::new();
        }
        let output_len = input.len() / self.decimation;
        let mut output = Vec::with_capacity(output_len);
        let half = self.length / 2;

        for i in 0..output_len {
            let center = i * self.decimation;
            let mut acc = 0.0;
            for k in 0..self.length {
                let idx = center + k;
                if idx >= half && idx - half < input.len() {
                    acc += self.coefficients[k] * input[idx - half];
                }
            }
            output.push(acc);
        }
        output
    }
}

/// Multi-stage decimation filter for efficient DSD-to-PCM conversion.
///
/// Instead of a single massive filter at the DSD rate, this uses multiple
/// cascaded stages with lower-order filters at progressively lower rates.
/// This dramatically reduces computational cost.
pub struct DecimationFilter {
    stages: Vec<DecimationStage>,
}

struct DecimationStage {
    filter: SincFilter,
    decimation: usize,
}

impl DecimationFilter {
    /// Design a multi-stage decimation filter.
    ///
    /// For DSD64 (2.8224 MHz) → 176.4 kHz, a typical design uses:
    /// - Stage 1: /4 → 705.6 kHz (lower-order filter)
    /// - Stage 2: /2 → 352.8 kHz
    /// - Stage 3: /2 → 176.4 kHz
    pub fn design(dsd_rate: u32, target_rate: u32) -> Self {
        let total_decimation = dsd_rate / target_rate;
        let mut stages = Vec::new();
        let mut remaining = total_decimation as usize;
        let mut current_rate = dsd_rate;

        // Decompose into factors of 2 and4 for efficiency
        while remaining > 1 {
            let stage_dec = if remaining >= 4 && remaining % 4 == 0 {
                4
            } else if remaining >= 2 && remaining % 2 == 0 {
                2
            } else {
                remaining
            };
            let stage_target = current_rate / stage_dec as u32;
            let cutoff = (target_rate as f64 * 0.45).min(current_rate as f64 * 0.45);
            let atten = 96.0; // dB — aggressive for each stage
            let filter = SincFilter::design(current_rate, stage_target, cutoff, atten);
            stages.push(DecimationStage { filter, decimation: stage_dec });
            current_rate = stage_target;
            remaining /= stage_dec;
        }

        DecimationFilter { stages }
    }

    /// Apply the multi-stage decimation filter.
    pub fn apply(&self, input: &[f64]) -> Vec<f64> {
        let mut signal = input.to_vec();
        for stage in &self.stages {
            signal = stage.filter.apply(&signal);
        }
        signal
    }
}

/// Modified Bessel function of the first kind, order 0.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let x_half_sq = (x / 2.0).powi(2);
    for k in 1..=30 {
        term *= x_half_sq / (k as f64).powi(2);
        sum += term;
        if term.abs() < 1e-12 * sum.abs() {
            break;
        }
    }
    sum
}