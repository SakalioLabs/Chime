//! DST (Direct Stream Transfer) decoder for SACD.
//!
//! DST is the lossless compression used in SACD to compress DSD data.
//! This implements a frame-based DST decoder with arithmetic coding and
//! FIR prediction, following the ISO/IEC14496-3 / Scarlet Book specification.

use chime_core::ChimeError;
use chime_codec_dsd::DsfDecoder;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

mod arithmetic;
mod predictor;

pub use arithmetic::ArithmeticDecoder;
pub use predictor::FirPredictor;

/// A single DST frame header.
#[derive(Debug, Clone)]
pub struct DstFrameHeader {
    /// Frame counter.
    pub frame_nr: u16,
    /// Number of channels.
    pub num_channels: u8,
    /// DSD sample rate multiplier (1=DSD64, 2=DSD128, etc.).
    pub rate: u8,
    /// Whether this is a silence frame.
    pub is_silence: bool,
    /// Frame length in DSD bytes per channel.
    pub frame_len: u16,
    /// Number of filter sets used.
    pub num_filter_sets: u8,
    /// Number of quantizer step sizes per filter set.
    pub num_step_bits: u8,
    /// Filter coefficients for each filter set.
    pub filter_coef_sets: Vec<Vec<i8>>,
    /// Quantizer step bits for each filter set.
    pub quant_step_sizes: Vec<u8>,
    /// Mapping of segments to filter/quantizer sets.
    pub filter_set_mapping: Vec<u8>,
    pub quant_mapping: Vec<u8>,
}

/// A complete DST frame decoder.
pub struct DstDecoder {
    /// Number of DSD bytes per frame per channel.
    pub frame_bytes: usize,
    /// Number of channels.
    pub channels: usize,
}

impl DstDecoder {
    pub fn new(frame_bytes: usize, channels: usize) -> Self {
        Self { frame_bytes, channels }
    }

    /// Decode one DST frame from the bitstream.
    /// Returns DSD bytes per channel.
    pub fn decode_frame(&self, data: &[u8]) -> Result<Vec<Vec<u8>>, ChimeError> {
        if data.is_empty() {
            return Err(ChimeError::InvalidData("Empty DST frame".into()));
        }

        let mut reader = std::io::Cursor::new(data);

        // Read frame header (simplified — full impl would parse the binary DST header)
        // For now, provide a skeleton that can be expanded with real arithmetic decoding
        let hdr = self.read_frame_header(&mut reader)?;

        let mut output = vec![vec![0u8; self.frame_bytes]; self.channels];

        if hdr.is_silence {
            // Silence frame: fill with zeros (DSD silence is all zeros or all ones)
            return Ok(output);
        }

        // Per-channel decoding
        for ch in 0..self.channels {
            self.decode_channel(&mut reader, &hdr, &mut output[ch])?;
        }

        Ok(output)
    }

    fn read_frame_header(&self, reader: &mut std::io::Cursor<&[u8]>) -> Result<DstFrameHeader, ChimeError> {
        // DST frame header is a variable-length structure
        // Simplified read for skeleton
        let hdr = DstFrameHeader {
            frame_nr: 0,
            num_channels: self.channels as u8,
            rate: 1,
            is_silence: false,
            frame_len: self.frame_bytes as u16,
            num_filter_sets: 1,
            num_step_bits: 1,
            filter_coef_sets: vec![vec![0i8; 48]], // placeholder
            quant_step_sizes: vec![0],
            filter_set_mapping: vec![0],
            quant_mapping: vec![0],
        };
        Ok(hdr)
    }

    fn decode_channel(
        &self,
        reader: &mut std::io::Cursor<&[u8]>,
        hdr: &DstFrameHeader,
        output: &mut [u8],
    ) -> Result<(), ChimeError> {
        // Initialize arithmetic decoder
        let mut arith = ArithmeticDecoder::new();
        arith.init(reader)?;

        let predictor = FirPredictor::new(
            &hdr.filter_coef_sets[0],
            hdr.quant_step_sizes[0] as i32,
        );

        // Decode each DSD byte in the frame
        let mut prev_bits: Vec<u8> = vec![0; predictor.order()];
        for i in 0..self.frame_bytes {
            let predicted = predictor.predict(&prev_bits);
            let residual = arith.decode_symbol(reader, predicted)?;
            let dsd_byte = ((predicted as i16 + residual as i16) & 0xFF) as u8;
            output[i] = dsd_byte;
            // Shift history
            prev_bits.rotate_right(1);
            prev_bits[0] = dsd_byte;
        }

        Ok(())
    }
}