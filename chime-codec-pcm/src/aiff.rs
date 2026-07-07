//! AIFF (Audio Interchange File Format) decoder.
//!
//! Supports uncompressed PCM AIFF files with 8/16/24/32-bit sample sizes.
//! AIFF uses big-endian byte order (unlike WAV which is little-endian).
//! Also supports AIFF-C/sowt (little-endian PCM in AIFF container).

use chime_core::codec::{AudioCodec, AudioData, CodecInfo, ReadSeek};
use chime_core::sample::SampleFormat;
use chime_core::buffer::AudioBuffer;
use chime_core::{ChimeError, PcmInfo, StreamInfo};
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, SeekFrom};

pub struct AiffDecoder;

impl AiffDecoder {
    pub fn new() -> Self { Self }
}

struct AiffHeader {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    sample_frames: u32,
    data_offset: u64,
    data_size: u32,
    /// Whether sample data is big-endian (true for AIFF, false for AIFF-C/sowt).
    big_endian: bool,
}

fn read_extended(reader: &mut dyn Read) -> Result<u32, ChimeError> {
    // Read 80-bit SANE extended float, extract sample rate
    let exponent = reader.read_u16::<BigEndian>()?;
    let mantissa = reader.read_u64::<BigEndian>()?;
    if exponent == 0 {
        return Ok(0);
    }
    let exp = (exponent as i32) - 16383 - 63;
    let sample_rate = if exp >= 0 {
        (mantissa >> (63 - exp)) as u32
    } else {
        (mantissa >> 63) as u32 >> exp.abs() as u32
    };
    Ok(sample_rate.max(1))
}

fn read_aiff_header(r: &mut dyn ReadSeek) -> Result<AiffHeader, ChimeError> {
    r.seek(SeekFrom::Start(0))?;
    let mut form = [0u8; 4];
    r.read_exact(&mut form)?;
    if &form != b"FORM" {
        return Err(ChimeError::InvalidData("Not an AIFF file (missing FORM)".into()));
    }
    let _form_size = r.read_u32::<BigEndian>()?;
    let mut form_type = [0u8; 4];
    r.read_exact(&mut form_type)?;

    let big_endian = match &form_type {
        b"AIFF" => true,
        b"AIFC" => false, // AIFF-C or sowt (little-endian)
        _ => return Err(ChimeError::InvalidData(
            format!("Not an AIFF file (type: {:?})", form_type)
        )),
    };

    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut sample_frames = 0u32;
    let mut data_offset = 0u64;
    let mut data_size = 0u32;
    let mut found_comm = false;
    let mut found_ssnd = false;

    loop {
        let mut chunk_id = [0u8; 4];
        match r.read_exact(&mut chunk_id) {
            Ok(()) => {}
            Err(_) => break,
        }
        let chunk_size = r.read_u32::<BigEndian>()?;
        let chunk_data_start = r.stream_position()?;

        match &chunk_id {
            b"COMM" => {
                channels = r.read_u16::<BigEndian>()?;
                sample_frames = r.read_u32::<BigEndian>()?;
                bits_per_sample = r.read_u16::<BigEndian>()?;
                sample_rate = read_extended(r)?;
                // For AIFF-C, read compression type
                if !big_endian && chunk_size >= 22 {
                    let mut comp_type = [0u8; 4];
                    let _ = r.read_exact(&mut comp_type);
                }
                found_comm = true;
            }
            b"SSND" => {
                let offset = r.read_u32::<BigEndian>()?;
                let _block_size = r.read_u32::<BigEndian>()?;
                data_offset = chunk_data_start + 8 + offset as u64;
                data_size = chunk_size.saturating_sub(8);
                found_ssnd = true;
                break;
            }
            _ => {
                // Skip unknown chunk (pad to even byte)
                let skip = chunk_size as u64 + (chunk_size % 2) as u64;
                r.seek(SeekFrom::Start(chunk_data_start + skip))?;
            }
        }
    }

    if !found_comm || !found_ssnd {
        return Err(ChimeError::InvalidData("AIFF missing COMM or SSND chunk".into()));
    }

    Ok(AiffHeader { channels, sample_rate, bits_per_sample, sample_frames, data_offset, data_size, big_endian })
}

fn aiff_to_sample_format(bits: u16) -> Result<SampleFormat, ChimeError> {
    match bits {
        8 => Ok(SampleFormat::U8),
        16 => Ok(SampleFormat::I16),
        24 => Ok(SampleFormat::I24),
        32 => Ok(SampleFormat::I32),
        _ => Err(ChimeError::UnsupportedFormat(format!("AIFF {}-bit not supported", bits))),
    }
}

impl AudioCodec for AiffDecoder {
    fn name(&self) -> &'static str { "AIFF" }

    fn probe(&self, reader: &mut dyn ReadSeek) -> Result<CodecInfo, ChimeError> {
        reader.seek(SeekFrom::Start(0))?;
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        if &magic != b"FORM" {
            return Err(ChimeError::UnsupportedFormat("Not FORM".into()));
        }
        let hdr = read_aiff_header(reader)?;
        let fmt = aiff_to_sample_format(hdr.bits_per_sample)?;
        Ok(CodecInfo {
            stream: StreamInfo::Pcm(PcmInfo {
                sample_rate: hdr.sample_rate,
                channels: hdr.channels,
                sample_format: fmt,
                total_frames: Some(hdr.sample_frames as u64),
                bits_per_sample: hdr.bits_per_sample,
            }),
            format_name: "AIFF",
        })
    }

    fn decode(&self, reader: &mut dyn ReadSeek) -> Result<AudioData, ChimeError> {
        reader.seek(SeekFrom::Start(0))?;
        let hdr = read_aiff_header(reader)?;
        let fmt = aiff_to_sample_format(hdr.bits_per_sample)?;

        reader.seek(SeekFrom::Start(hdr.data_offset))?;
        let mut pcm_data = vec![0u8; hdr.data_size as usize];
        reader.read_exact(&mut pcm_data)?;

        // AIFF stores samples in big-endian; for 8-bit it uses unsigned
        // For big-endian formats, we need to byte-swap to little-endian
        // before passing to bytes_to_f32_le, or handle separately
        let buf = if hdr.big_endian {
            // Byte-swap from big-endian to little-endian per sample
            let bps = fmt.bytes_per_sample();
            let mut swapped = pcm_data.clone();
            for chunk in swapped.chunks_exact_mut(bps) {
                chunk.reverse();
            }
            AudioBuffer::from_pcm_bytes(&swapped, fmt, hdr.channels, hdr.sample_rate)
        } else {
            // Already little-endian (AIFF-C/sowt)
            AudioBuffer::from_pcm_bytes(&pcm_data, fmt, hdr.channels, hdr.sample_rate)
        };

        Ok(AudioData::Pcm(buf))
    }
}