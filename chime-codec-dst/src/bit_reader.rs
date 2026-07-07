//! Bit-level reader for DST frame parsing.
//!
//! DST frames pack fields at arbitrary bit boundaries (12-bit frame_nr, 4-bit channels, etc.).
//! This provides a cursor-based bit reader for the DST bitstream.


/// Bit-level reader for DST frame parsing.
pub struct DstBitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,  // 0-7, next bit to read (MSB first)
}

impl<'a> DstBitReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, byte_pos: 0, bit_pos: 0 }
    }

    /// Read `count` bits as a u32 (MSB first).
    pub fn read_bits(&mut self, count: u8) -> u32 {
        let mut result: u32 = 0;
        for _ in 0..count {
            result <<= 1;
            if self.byte_pos < self.data.len() {
                let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1;
                result |= bit as u32;
            }
            self.bit_pos += 1;
            if self.bit_pos >= 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        result
    }

    /// Read `count` bits as a signed integer (sign-extended).
    pub fn read_signed_bits(&mut self, count: u8) -> i32 {
        let val = self.read_bits(count);
        if count > 0 && (val >> (count - 1)) & 1 != 0 {
            // Sign extend
            (val | (!0u32 << count)) as i32
        } else {
            val as i32
        }
    }

    /// Read a u8 byte (8 bits, MSB first).
    pub fn read_byte(&mut self) -> u8 {
        self.read_bits(8) as u8
    }

    /// Skip to the next byte boundary.
    pub fn align_to_byte(&mut self) {
        if self.bit_pos != 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }

    /// Current position in bytes (fractional).
    pub fn position_f64(&self) -> f64 {
        self.byte_pos as f64 + self.bit_pos as f64 / 8.0
    }

    /// Number of bits remaining.
    pub fn bits_remaining(&self) -> usize {
        self.data.len().saturating_sub(self.byte_pos) * 8 - self.bit_pos as usize
    }

    /// Number of bytes consumed so far.
    pub fn bytes_consumed(&self) -> usize {
        self.byte_pos + if self.bit_pos > 0 { 1 } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_reader() {
        let data = [0b10101010u8, 0b11110000];
        let mut reader = DstBitReader::new(&data);
        assert_eq!(reader.read_bits(4), 0b1010);
        assert_eq!(reader.read_bits(4), 0b1010);
        assert_eq!(reader.read_bits(4), 0b1111);
        assert_eq!(reader.read_bits(4), 0b0000);
    }

    #[test]
    fn test_signed_bits() {
        let data = [0b10000000u8];
        let mut reader = DstBitReader::new(&data);
        assert_eq!(reader.read_signed_bits(4), -8);
    }
}