//! Sample format definitions for PCM audio.

/// Supported PCM sample formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    /// 8-bit unsigned integer
    U8,
    /// 16-bit signed integer
    I16,
    /// 24-bit signed integer (stored in 3 bytes, little-endian)
    I24,
    /// 32-bit signed integer
    I32,
    /// 32-bit IEEE 754 float
    F32,
    /// 64-bit IEEE 754 float
    F64,
}

impl SampleFormat {
    /// Bytes per sample for this format.
    pub fn bytes_per_sample(&self) -> usize {
        match self {
            Self::U8 => 1,
            Self::I16 => 2,
            Self::I24 => 3,
            Self::I32 => 4,
            Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    /// Bits per sample.
    pub fn bits_per_sample(&self) -> u16 {
        (self.bytes_per_sample() * 8) as u16
    }

    /// Whether this is a floating-point format.
    pub fn is_float(&self) -> bool {
        matches!(self, Self::F32 | Self::F64)
    }
}

/// Convert an interleaved byte buffer of the given format to f32 [-1.0, 1.0].
/// The output has one f32 per sample (still interleaved).
pub fn bytes_to_f32_le(bytes: &[u8], format: SampleFormat) -> Vec<f32> {
    let bps = format.bytes_per_sample();
    assert!(bytes.len() % bps == 0, "byte buffer length must be multiple of sample size");
    let count = bytes.len() / bps;
    let mut out = Vec::with_capacity(count);

    match format {
        SampleFormat::U8 => {
            for &b in bytes {
                out.push((b as f32 - 128.0) / 128.0);
            }
        }
        SampleFormat::I16 => {
            for chunk in bytes.chunks_exact(2) {
                let val = i16::from_le_bytes([chunk[0], chunk[1]]);
                out.push(val as f32 / 32768.0);
            }
        }
        SampleFormat::I24 => {
            for chunk in bytes.chunks_exact(3) {
                let val = (chunk[0] as i32) | ((chunk[1] as i32) << 8) | ((chunk[2] as i32) << 16);
                let val = if val & 0x800000 != 0 { val | !0xFFFFFF } else { val } ;
                out.push(val as f32 / 8388608.0);
            }
        }
        SampleFormat::I32 => {
            for chunk in bytes.chunks_exact(4) {
                let val = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                out.push(val as f32 / 2147483648.0);
            }
        }
        SampleFormat::F32 => {
            for chunk in bytes.chunks_exact(4) {
                let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                out.push(val);
            }
        }
        SampleFormat::F64 => {
            for chunk in bytes.chunks_exact(8) {
                let val = f64::from_le_bytes([
                    chunk[0], chunk[1], chunk[2], chunk[3],
                    chunk[4], chunk[5], chunk[6], chunk[7],
                ]);
                out.push(val as f32);
            }
        }
    }
    out
}

/// De-interleave f32 samples from interleaved to per-channel planar layout.
pub fn deinterleave_f32(interleaved: &[f32], channels: u16) -> Vec<Vec<f32>> {
    let ch = channels as usize;
    let frames = interleaved.len() / ch;
    let mut planes = vec![Vec::with_capacity(frames); ch];
    for (i, &s) in interleaved.iter().enumerate() {
        planes[i % ch].push(s);
    }
    planes
}