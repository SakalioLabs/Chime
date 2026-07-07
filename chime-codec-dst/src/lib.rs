//! DST (Direct Stream Transfer) decoder for SACD.
//!
//! DST is the lossless compression used in SACD to compress DSD data.
//! This implements a frame-based DST decoder with:
//! - Bit-level frame header parser (per Scarlet Book spec)
//! - Arithmetic coding for segment residual decoding
//! - FIR prediction for DSD sample reconstruction
//!
//! Frame structure:
//!   1. Frame header: frame_nr, channels, rate, silence, length, filter sets
//!   2. Filter coefficient sets for each segment
//!   3. Segment-to-filter-set mapping
//!   4. Arithmetic-coded segment residual data

use chime_core::ChimeError;

pub mod bit_reader;
mod arithmetic;
mod predictor;

pub use bit_reader::DstBitReader;
pub use arithmetic::ArithmeticDecoder;
pub use predictor::FirPredictor;

/// A parsed DST frame header.
#[derive(Debug, Clone)]
pub struct DstFrameHeader {
    pub frame_nr: u16,
    pub num_channels: u8,
    pub rate: u8,
    pub is_silence: bool,
    pub frame_len: u16,
    pub num_filter_sets: u8,
    pub filter_sets: Vec<DstFilterSet>,
    pub segment_mapping: Vec<u8>,
    /// Offset in bytes where segment data (arithmetic bitstream) starts.
    pub segment_data_offset: usize,
}

/// A filter set within a DST frame.
#[derive(Debug, Clone)]
pub struct DstFilterSet {
    pub segment_width_log2: u8,
    pub num_segments: u16,
    pub num_bands: u8,
    pub step_bits: u8,
    pub quant_step_size: u8,
    pub coefficients: Vec<i16>,
}

/// DST frame decoder.
pub struct DstDecoder {
    pub frame_bytes: usize,
    pub channels: usize,
}

impl DstDecoder {
    pub fn new(frame_bytes: usize, channels: usize) -> Self {
        Self { frame_bytes, channels }
    }

    /// Parse a DST frame header from raw bytes.
    pub fn parse_frame_header(data: &[u8]) -> Result<DstFrameHeader, ChimeError> {
        let mut reader = DstBitReader::new(data);

        let frame_nr = reader.read_bits(12) as u16;
        let num_channels_code = reader.read_bits(4) as u8;
        let num_channels = if num_channels_code <= 1 { 1 } else { num_channels_code };
        let rate = reader.read_bits(1) as u8;
        let is_silence = reader.read_bits(1) != 0;
        let frame_len = reader.read_bits(14) as u16;
        let num_filter_sets = reader.read_bits(4) as u8;

        let mut filter_sets = Vec::with_capacity(num_filter_sets as usize);
        let mut total_segments = 0u16;

        for _ in 0..num_filter_sets {
            let seg_width_log2 = reader.read_bits(4) as u8;
            let num_seg_bits = 16u8.saturating_sub(seg_width_log2);
            let num_segments = reader.read_bits(num_seg_bits.min(10)) as u16;
            let num_bands = reader.read_bits(3) as u8;
            let step_bits = reader.read_bits(3) as u8;
            let quant_step_size = if step_bits > 0 { 1u8 << (step_bits - 1) } else { 0 };

            let mut coefficients = Vec::with_capacity(num_bands as usize);
            for _ in 0..num_bands {
                // Filter coefficients are s2.8 fixed-point (10 bits, signed)
                let coef = reader.read_signed_bits(10) as i16;
                coefficients.push(coef);
            }

            filter_sets.push(DstFilterSet {
                segment_width_log2: seg_width_log2,
                num_segments,
                num_bands,
                step_bits,
                quant_step_size,
                coefficients,
            });
            total_segments += num_segments;
        }

        // Segment-to-filter-set mapping
        let mut segment_mapping = Vec::with_capacity(total_segments as usize);
        let map_bits = if num_filter_sets <= 1 { 0 } else { (num_filter_sets as f64).log2().ceil() as u8 };
        if map_bits > 0 {
            for _ in 0..total_segments {
                let filter_set = reader.read_bits(map_bits) as u8;
                segment_mapping.push(filter_set);
            }
        } else if total_segments > 0 {
            // Only one filter set, all segments use it
            for _ in 0..total_segments {
                segment_mapping.push(0);
            }
        }

        let segment_data_offset = reader.bytes_consumed();

        Ok(DstFrameHeader {
            frame_nr,
            num_channels,
            rate,
            is_silence,
            frame_len,
            num_filter_sets,
            filter_sets,
            segment_mapping,
            segment_data_offset,
        })
    }

    /// Decode one DST frame from the bitstream.
    /// Returns DSD bytes per channel.
    pub fn decode_frame(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, ChimeError> {
        if data.is_empty() {
            return Err(ChimeError::InvalidData("Empty DST frame".into()));
        }

        let hdr = Self::parse_frame_header(data)?;

        if hdr.is_silence {
            return Ok(vec![vec![0u8; self.frame_bytes]; self.channels]);
        }

        if hdr.num_channels as usize != self.channels {
            return Err(ChimeError::InvalidData(
                format!("Channel mismatch: header={}, decoder={}", hdr.num_channels, self.channels)
            ));
        }

        let mut output = vec![vec![0u8; self.frame_bytes]; self.channels];

        // Extract segment data from the bitstream
        let segment_data = &data[hdr.segment_data_offset..];

        // Decode each channel using the segments and arithmetic decoder
        for ch in 0..self.channels {
            self.decode_channel(segment_data, &hdr, &mut output[ch])?;
        }

        Ok(output)
    }

        fn decode_channel(
        &self,
        segment_data: &[u8],
        hdr: &DstFrameHeader,
        output: &mut [u8],
    ) -> Result<(), ChimeError> {
        // Arithmetic decoder for the residual bitstream
        let mut arith = ArithmeticDecoder::new();
        let mut arith_reader = std::io::Cursor::new(segment_data);
        arith.init(&mut arith_reader)?;

        // Determine the segment bit width
        let seg_bits: u8 = hdr.filter_sets.iter()
            .map(|fs| fs.segment_width_log2)
            .max().unwrap_or(4);

        // Pre-compute predictors per filter set (not recreated on every byte)
        let cached_predictors: Vec<FirPredictor> = hdr.filter_sets.iter()
            .map(|fs| {
                let coef: Vec<i8> = fs.coefficients.iter().map(|&c| c as i8).collect();
                FirPredictor::new(&coef, fs.quant_step_size as i32)
            })
            .collect();

        // Ring buffer for prediction history (most recent decoded byte first)
        let max_order = cached_predictors.iter().map(|p| p.order()).max().unwrap_or(8);
        let mut history: Vec<u8> = vec![0u8; max_order];

        // Track segment index to avoid redundant predictor lookups
        let mut current_seg_idx = usize::MAX;
        let mut current_predictor_idx = 0usize;

        for byte_idx in 0..self.frame_bytes {
            let seg_idx = (byte_idx * 8) >> seg_bits;

            if seg_idx != current_seg_idx {
                current_seg_idx = seg_idx;
                let filter_set_idx = if seg_idx < hdr.segment_mapping.len() {
                    hdr.segment_mapping[seg_idx] as usize
                } else {
                    0
                };
                current_predictor_idx = filter_set_idx.min(cached_predictors.len().saturating_sub(1));
            }

            // FIR prediction from history
            let predicted = cached_predictors[current_predictor_idx].predict(&history);

            // Decode arithmetic-coded residual (context = predicted value)
            let residual = arith.decode_symbol(&mut arith_reader, predicted)?;

            output[byte_idx] = (predicted as i16).wrapping_add(residual as i16) as u8;

            // Update ring buffer
            if max_order > 1 {
                history.copy_within(0..max_order - 1, 1);
            }
            history[0] = output[byte_idx];
        }

        Ok(())
    }
}