//! DSD audio codec — DSF and DFF/DSDIFF parsers.
//!
//! Supports:
//! - DSF (DSD Stream File) — Sony format
//! - DFF/DSDIFF — Philips format

use chime_core::codec::{AudioCodec, AudioData, CodecInfo, ReadSeek};
use chime_core::{ChimeError, DsdInfo, StreamInfo};
use byteorder::{LittleEndian, BigEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

pub mod sacd_iso;
pub use sacd_iso::{SacdIsoParser, SacdMasterToc, SacdAreaToc, SacdTrack, SacdChannelType};

// ─── DSF Format ─────────────────────────────────────────────

/// DSF file header
struct DsfHeader {
    channels: u16,
    sampling_freq: u32,
    bits_per_sample: u16,
    sample_count: u64,
    data_size: u64,
    data_offset: u64,
}

fn read_dsf_header(r: &mut dyn ReadSeek) -> Result<DsfHeader, ChimeError> {
    r.seek(SeekFrom::Start(0))?;
    let mut chunk_id = [0u8; 4];
    r.read_exact(&mut chunk_id)?;
    if &chunk_id != b"DSD " {
        return Err(ChimeError::InvalidData("Not a DSF file".into()));
    }
    let _chunk_size = r.read_u64::<LittleEndian>()?;
    let _total_size = r.read_u64::<LittleEndian>()?;
    let meta_offset = r.read_u64::<LittleEndian>()?;

    // fmt chunk
    r.seek(SeekFrom::Start(meta_offset))?;
    let mut fmt_id = [0u8; 4];
    r.read_exact(&mut fmt_id)?;
    if &fmt_id != b"fmt " {
        return Err(ChimeError::InvalidData("DSF missing fmt chunk".into()));
    }
    let _fmt_size = r.read_u64::<LittleEndian>()?;
    let format_version = r.read_u32::<LittleEndian>()?;
    let format_id = r.read_u32::<LittleEndian>()?;
    let channel_type = r.read_u32::<LittleEndian>()?;
    let channel_num = r.read_u32::<LittleEndian>()?;
    let sampling_freq = r.read_u32::<LittleEndian>()?;
    let bits_per_sample = r.read_u32::<LittleEndian>()?;
    let sample_count = r.read_u64::<LittleEndian>()?;
    let block_size = r.read_u32::<LittleEndian>()?;
    let _reserved = r.read_u32::<LittleEndian>()?;

    // data chunk
    let data_offset_pos = meta_offset + 52; // fmt chunk data starts after header
    // Read data chunk offset from the file structure
    let mut cur = meta_offset + 12 + _fmt_size;
    r.seek(SeekFrom::Start(cur))?;
    let mut data_id = [0u8; 4];
    r.read_exact(&mut data_id)?;
    if &data_id != b"data" {
        return Err(ChimeError::InvalidData("DSF missing data chunk".into()));
    }
    let data_size = r.read_u64::<LittleEndian>()?;
    let data_start = cur + 12;

    Ok(DsfHeader {
        channels: channel_num as u16,
        sampling_freq,
        bits_per_sample: bits_per_sample as u16,
        sample_count,
        data_size,
        data_offset: data_start,
    })
}

fn dsf_sampling_to_dsd_rate(freq: u32) -> u32 {
    // DSF stores the base frequency (2822400 for DSD64), but we multiply by bits_per_sample
    // Actually DSF sampling_freq is already in Hz per channel
    freq
}

// ─── DFF/DSDIFF Format ──────────────────────────────────────

struct DffHeader {
    channels: u16,
    sample_rate: u32,
    data_size: u64,
    data_offset: u64,
}

fn read_dff_header(r: &mut dyn ReadSeek) -> Result<DffHeader, ChimeError> {
    r.seek(SeekFrom::Start(0))?;
    let mut form_type = [0u8; 4];
    r.read_exact(&mut form_type)?;
    // DSDIFF starts with "FRM8" then size, then "DSD "
    let mut header_buf = [0u8; 4];
    r.read_exact(&mut header_buf)?;
    if &header_buf != b"FRM8" && &form_type != b"FRM8" {
        return Err(ChimeError::InvalidData("Not a DFF file".into()));
    }

    // Re-read: FRM8 is actually the first 4 bytes
    r.seek(SeekFrom::Start(0))?;
    let mut frm8 = [0u8; 4];
    r.read_exact(&mut frm8)?;
    let _frm_size = r.read_u64::<BigEndian>()?;
    let mut dsd_id = [0u8; 4];
    r.read_exact(&mut dsd_id)?;
    if &dsd_id != b"DSD " {
        return Err(ChimeError::InvalidData("DFF missing DSD form type".into()));
    }

    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut data_size = 0u64;
    let mut data_offset = 0u64;
    let mut found_prop = false;

    // Scan chunks
    loop {
        let pos = r.stream_position().unwrap_or(0);
        let mut chunk_id = [0u8; 4];
        match r.read_exact(&mut chunk_id) {
            Ok(()) => {}
            Err(_) => break,
        }
        let chunk_size = r.read_u64::<BigEndian>()?;
        let chunk_data_start = r.stream_position().unwrap_or(0);

        match &chunk_id {
            b"PROP" => {
                let mut prop_type = [0u8; 4];
                r.read_exact(&mut prop_type)?;
                if &prop_type != b"SND " {
                    r.seek(SeekFrom::Start(chunk_data_start + chunk_size))?;
                    continue;
                }
                // Scan property chunks
                let prop_end = chunk_data_start + chunk_size;
                while r.stream_position().unwrap_or(0) < prop_end {
                    let mut sub_id = [0u8; 4];
                    if r.read_exact(&mut sub_id).is_err() { break; }
                    let sub_size = r.read_u64::<BigEndian>()?;
                    let sub_start = r.stream_position().unwrap_or(0);
                    match &sub_id {
                        b"FS  " => {
                            sample_rate = r.read_u32::<BigEndian>()?;
                        }
                        b"CHNL" => {
                            channels = r.read_u16::<BigEndian>()?;
                            // skip channel IDs
                        }
                        _ => {}
                    }
                    r.seek(SeekFrom::Start(sub_start + sub_size))?;
                }
                found_prop = true;
            }
            b"DSD " => {
                data_size = chunk_size;
                data_offset = chunk_data_start;
                break;
            }
            _ => {
                r.seek(SeekFrom::Start(chunk_data_start + chunk_size))?;
            }
        }
    }

    if !found_prop || data_size == 0 {
        return Err(ChimeError::InvalidData("DFF missing PROP or DSD chunk".into()));
    }

    Ok(DffHeader { channels, sample_rate, data_size, data_offset })
}

// ─── DsfDecoder ─────────────────────────────────────────────

pub struct DsfDecoder;
impl DsfDecoder { pub fn new() -> Self { Self } }

impl AudioCodec for DsfDecoder {
    fn name(&self) -> &'static str { "DSF" }

    fn probe(&self, reader: &mut dyn ReadSeek) -> Result<CodecInfo, ChimeError> {
        let hdr = read_dsf_header(reader)?;
        Ok(CodecInfo {
            stream: StreamInfo::Dsd(DsdInfo {
                sample_rate: hdr.sampling_freq,
                channels: hdr.channels,
                bits_per_sample: hdr.bits_per_sample,
                total_bytes_per_ch: Some(hdr.sample_count / 8),
            }),
            format_name: "DSF",
        })
    }

    fn decode(&self, reader: &mut dyn ReadSeek) -> Result<AudioData, ChimeError> {
        let hdr = read_dsf_header(reader)?;
        // DSF stores DSD in LSB-first per byte, interleaved by channel
        // and each channel block is stored sequentially within a block
        reader.seek(SeekFrom::Start(hdr.data_offset))?;
        let mut data = vec![0u8; hdr.data_size as usize];
        reader.read_exact(&mut data)?;

        // DSF uses channel-interleaved blocks (block_size bytes per channel per block)
        // Rearrange to interleaved per-sample: each byte is8 bits of DSD for one channel
        // DSF: [ch0_block][ch1_block]... → interleaved per byte
        let block_size = 4096u64; // DSF default block size
        let ch = hdr.channels as usize;
        let total_blocks = hdr.data_size / (block_size * ch as u64);
        let mut interleaved = Vec::with_capacity(hdr.data_size as usize);
        for block in 0..total_blocks {
            for sample_in_block in 0..block_size {
                for c in 0..ch {
                    let offset = (block * ch as u64 * block_size + c as u64 * block_size + sample_in_block) as usize;
                    if offset < data.len() {
                        interleaved.push(data[offset]);
                    }
                }
            }
        }

        Ok(AudioData::Dsd {
            data: interleaved,
            sample_rate: hdr.sampling_freq,
            channels: hdr.channels,
        })
    }
}

// ─── DffDecoder ─────────────────────────────────────────────

pub struct DffDecoder;
impl DffDecoder { pub fn new() -> Self { Self } }

impl AudioCodec for DffDecoder {
    fn name(&self) -> &'static str { "DFF" }

    fn probe(&self, reader: &mut dyn ReadSeek) -> Result<CodecInfo, ChimeError> {
        let hdr = read_dff_header(reader)?;
        Ok(CodecInfo {
            stream: StreamInfo::Dsd(DsdInfo {
                sample_rate: hdr.sample_rate,
                channels: hdr.channels,
                bits_per_sample: 1,
                total_bytes_per_ch: Some(hdr.data_size / hdr.channels as u64),
            }),
            format_name: "DFF",
        })
    }

    fn decode(&self, reader: &mut dyn ReadSeek) -> Result<AudioData, ChimeError> {
        let hdr = read_dff_header(reader)?;
        reader.seek(SeekFrom::Start(hdr.data_offset))?;
        let mut data = vec![0u8; hdr.data_size as usize];
        reader.read_exact(&mut data)?;
        // DFF stores DSD interleaved by channel, MSB-first
        Ok(AudioData::Dsd {
            data,
            sample_rate: hdr.sample_rate,
            channels: hdr.channels,
        })
    }
}