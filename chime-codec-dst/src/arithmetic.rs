//! Arithmetic decoder for DST bitstream.
//!
//! Implements a multi-symbol arithmetic coder as specified in the
//! DST / ISO/IEC 14496-3 specification. Uses a context-adaptive probability
//! model for efficient lossless compression of DSD residuals.

use chime_core::ChimeError;
use std::io::Read;

/// Probability context for arithmetic coding.
#[derive(Clone)]
struct ProbContext {
    /// Cumulative frequency table.
    cum_freq: Vec<u32>,
    /// Total cumulative frequency.
    total_freq: u32,

}

impl ProbContext {
    fn new(num_symbols: usize) -> Self {
        let mut cum_freq = Vec::with_capacity(num_symbols + 1);
        for i in 0..=num_symbols {
            cum_freq.push(i as u32);
        }
        Self {
            cum_freq,
            total_freq: num_symbols as u32,

        }
    }

    /// Update the probability model after observing a symbol.
    fn update(&mut self, symbol: u32) {
        // Increment frequencies for symbols >= symbol
        for i in (symbol as usize + 1)..self.cum_freq.len() {
            self.cum_freq[i] += 1;
        }
        self.total_freq += 1;

        // Rescale if total gets too large
        if self.total_freq >= (1 << 20) {
            self.rescale();
        }
    }

    fn rescale(&mut self) {
        self.total_freq = 0;
        for i in 1..self.cum_freq.len() {
            self.cum_freq[i] = (self.cum_freq[i] >> 1).max(self.cum_freq[i - 1] + 1);
            self.total_freq = self.cum_freq[i];
        }
    }
}

/// Multi-symbol arithmetic decoder for DST streams.
pub struct ArithmeticDecoder {
    /// Low bound of the current interval.
    low: u32,
    /// High bound of the current interval.
    high: u32,
    /// Current code value from the bitstream.
    code: u32,
    /// Number of bits in the code register.
    code_bits: u32,
    /// Probability contexts for different prediction orders.
    contexts: Vec<ProbContext>,
}

impl ArithmeticDecoder {
    pub fn new() -> Self {
        // 256 symbols (byte values) with uniform initial distribution
        let contexts = vec![ProbContext::new(256); 64];
        Self {
            low: 0,
            high: 0xFFFF_FFFF,
            code: 0,
            code_bits: 0,
            contexts,
        }
    }

    /// Initialize the decoder by reading the initial code value.
    pub fn init(&mut self, reader: &mut std::io::Cursor<&[u8]>) -> Result<(), ChimeError> {
        self.low = 0;
        self.high = 0xFFFF_FFFF;
        self.code = 0;

        // Read 4 bytes for initial code value
        let mut buf = [0u8; 4];
        reader.read_exact(&mut buf)?;
        self.code = u32::from_be_bytes(buf);
        self.code_bits = 32;
        Ok(())
    }

    /// Decode one symbol given a predicted value (used as context selector).
    pub fn decode_symbol(
        &mut self,
        reader: &mut std::io::Cursor<&[u8]>,
        predicted: u8,
    ) -> Result<u8, ChimeError> {
        let ctx_idx = (predicted as usize) % self.contexts.len();
        let ctx = &self.contexts[ctx_idx];
        let range = (self.high - self.low + 1) as u64;
        let total = ctx.total_freq as u64;

        // Calculate cumulative frequency
        let scaled = ((self.code as u64 - self.low as u64 + 1) * total - 1) / range;
        let cum_freq = scaled as u32;

        // Find symbol
        let mut symbol = 0u32;
        for i in 0..256 {
            if ctx.cum_freq[i + 1] > cum_freq {
                symbol = i as u32;
                break;
            }
        }

        // Update interval
        self.high = self.low + ((range * ctx.cum_freq[symbol as usize + 1] as u64) / total) as u32 - 1;
        self.low = self.low + ((range * ctx.cum_freq[symbol as usize] as u64) / total) as u32;

        // Renormalize
        loop {
            if (self.high & 0x8000_0000) == (self.low & 0x8000_0000) {
                // MSBs match — shift out
                self.low = (self.low << 1) & 0xFFFF_FFFF;
                self.high = ((self.high << 1) | 1) & 0xFFFF_FFFF;
                self.code = (self.code << 1) & 0xFFFF_FFFF;
                // Read one more bit
                if self.code_bits > 0 {
                    let mut b = [0u8; 1];
                    if reader.read_exact(&mut b).is_ok() {
                        self.code |= b[0] as u32 & 1;
                    }
                }
            } else if (self.low & 0x4000_0000) != 0 && (self.high & 0x4000_0000) == 0 {
                // Underflow case
                self.low = (self.low << 1) & 0x3FFF_FFFF;
                self.high = ((self.high << 1) | 0x8000_0001) & 0xFFFF_FFFF;
                self.code = (self.code ^ 0x4000_0000) & 0xFFFF_FFFF;
                if self.code_bits > 0 {
                    let mut b = [0u8; 1];
                    if reader.read_exact(&mut b).is_ok() {
                        self.code |= b[0] as u32 & 1;
                    }
                }
            } else {
                break;
            }
        }

        // Update probability model
        
        
                self.contexts[ctx_idx].update(symbol);

        Ok(symbol as u8)
    }
}
