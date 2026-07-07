//! FIR predictor for DST decoding.
//!
//! Uses a Finite Impulse Response filter for linear prediction of DSD
//! samples. The predictor estimates the next sample based on previous
//! samples, and only the residual (prediction error) is entropy-coded.

/// FIR predictor with configurable filter coefficients.
#[derive(Clone)]
pub struct FirPredictor {
    /// Filter coefficients (signed integers in DST spec).
    coef: Vec<i32>,
    /// Quantization step size for the residual.
    step_size: i32,
    /// Filter order (number of taps).
    order: usize,
}

impl FirPredictor {
    /// Create a new predictor from i8 coefficients and a quantization step.
    pub fn new(coef: &[i8], step_size: i32) -> Self {
        Self {
            coef: coef.iter().map(|&c| c as i32).collect(),
            step_size: step_size.max(1),
            order: coef.len(),
        }
    }

    /// Filter order (number of taps).
    pub fn order(&self) -> usize {
        self.order
    }

    /// Predict the next byte value from previous DSD bytes.
    /// Returns a predicted value in [0, 255].
    pub fn predict(&self, history: &[u8]) -> u8 {
        assert!(history.len() >= self.order, "history must be at least filter order");

        let mut acc: i32 = 0;
        for i in 0..self.order {
            let sample = history[i] as i32;
            acc = acc.wrapping_add(sample.wrapping_mul(self.coef[i]));
        }

        // Apply quantization step
        acc /= self.step_size;

        // Clamp to byte range
        ((acc.clamp(0, 255)) & 0xFF) as u8
    }

    /// Predict on a bit level (8 predictions per byte).
    /// Each bit is predicted from the previous bits using a shift-register
    /// style approach common in DSD.
    pub fn predict_bits(&self, bit_history: &[u8], num_bits: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(num_bits);
        let mut running_context: u32 = 0;

        for i in 0..num_bits {
            let byte_idx = i / 8;
            let bit_idx = i % 8;

            if byte_idx < bit_history.len() {
                let bit = (bit_history[byte_idx] >> (7 - bit_idx)) & 1;
                running_context = (running_context << 1) | bit as u32;
            }

            // Use lower bits of running context as prediction
            let predicted_bit = ((running_context >> 1) ^ running_context) & 1;

            let byte_out = if i % 8 == 0 { 0u8 } else { *result.last().unwrap_or(&0) };
            result.push(byte_out | ((predicted_bit as u8) << (7 - bit_idx)));
        }

        result
    }
}
