//! Polyphase FIR decimation filter for optimal DSP performance.
//!
//! A polyphase filter decomposes a single FIR with decimation factor M into M
//! sub-filters (phases), each operating at the lower output rate. This provides:
//! - Better cache locality (coefficients are contiguous per phase)
//! - Reduced memory bandwidth (only load input samples needed for each phase)
//! - Natural SIMD alignment (each phase's data fits in SIMD registers)

#[derive(Clone)]
pub struct PolyphaseFilter {
    /// Coefficients organized as [phase][tap].
    phase_coeffs: Vec<Vec<f32>>,
    /// Number of polyphase branches (= decimation factor).
    pub decimation: usize,
    /// Number of taps per phase.
    taps_per_phase: usize,
}

impl PolyphaseFilter {
    /// Design a polyphase filter from a prototype lowpass FIR.
    pub fn from_prototype(h: &[f32], decimation: usize) -> Self {
        assert!(decimation > 0);
        let taps_per_phase = (h.len() + decimation - 1) / decimation;
        let mut phase_coeffs = vec![vec![0.0f32; taps_per_phase]; decimation];

        for (i, &coeff) in h.iter().enumerate() {
            let phase = i % decimation;
            let tap = i / decimation;
            phase_coeffs[phase][tap] = coeff;
        }

        PolyphaseFilter { phase_coeffs, decimation, taps_per_phase }
    }

    /// Apply the polyphase decimation filter with zero-padding at boundaries.
    pub fn apply(&self, input: &[f32]) -> Vec<f32> {
        if input.is_empty() || self.decimation == 0 {
            return Vec::new();
        }
        let m = self.decimation;
        let k = self.taps_per_phase;
        let input_len = input.len();
        let output_len = input_len / m;
        let mut output = Vec::with_capacity(output_len);

        for n in 0..output_len {
            let phase = n % m;
            let coeffs = &self.phase_coeffs[phase];
            // Center position in the input for this output sample
            let center = n * m;

            let mut acc = 0.0f32;
            for t in 0..k {
                // Input index: center - t * m
                // Use signed arithmetic to safely detect boundary
                let offset = t * m;
                if offset <= center {
                    acc += coeffs[t] * input[center - offset];
                }
                // else: zero-padded boundary, skip
            }
            output.push(acc);
        }
        output
    }

    /// Apply with reusable output buffer.
    pub fn apply_into(&self, input: &[f32], output: &mut Vec<f32>) {
        output.clear();
        if input.is_empty() || self.decimation == 0 {
            return;
        }
        let m = self.decimation;
        let k = self.taps_per_phase;
        let input_len = input.len();
        let output_len = input_len / m;
        output.reserve(output_len);

        for n in 0..output_len {
            let phase = n % m;
            let coeffs = &self.phase_coeffs[phase];
            let center = n * m;

            let mut acc = 0.0f32;
            for t in 0..k {
                let offset = t * m;
                if offset <= center {
                    acc += coeffs[t] * input[center - offset];
                }
            }
            output.push(acc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polyphase_basic() {
        let h = [1.0f32, 2.0, 3.0, 4.0];
        let pf = PolyphaseFilter::from_prototype(&h, 2);
        assert_eq!(pf.decimation, 2);
        assert_eq!(pf.taps_per_phase, 2);
        assert_eq!(pf.phase_coeffs[0], vec![1.0, 3.0]);
        assert_eq!(pf.phase_coeffs[1], vec![2.0, 4.0]);
    }

    #[test]
    fn test_polyphase_output_len() {
        use crate::filters::SincFilter;
        let filter = SincFilter::design(2822400, 176400, 50000.0, 96.0);
        let pf = PolyphaseFilter::from_prototype(&filter.coefficients, filter.decimation);
        let signal: Vec<f32> = (0..282240).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let output = pf.apply(&signal);
        let expected_len = signal.len() / pf.decimation;
        assert_eq!(output.len(), expected_len);
        for (i, &s) in output.iter().enumerate() {
            assert!(s.is_finite(), "non-finite at index {}: {}", i, s);
        }
    }
}