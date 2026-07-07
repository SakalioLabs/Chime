//! PCM audio codec — WAV decoder.
//!
//! Supports WAV files with formats: U8, I16, I24, I32, F32, F64.

use chime_core::codec::{AudioCodec, AudioData, CodecInfo, ReadSeek};
use chime_core::sample::SampleFormat;
use chime_core::buffer::AudioBuffer;
use chime_core::{ChimeError, PcmInfo, StreamInfo};
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::{Read, Seek, SeekFrom};

/// WAV format codes
const WAVE_FORMAT_PCM: u16 = 0x0001;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 0x0003;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

pub struct WavDecoder;

impl WavDecoder {
    pub fn new() -> Self { Self }
}

struct WavHeader {
    channels: u16,
    sample_rate: u32,
    bits_per_sample: u16,
    format_tag: u16,
    data_size: u32,
}

fn read_wav_header(r: &mut dyn ReadSeek) -> Result<WavHeader, ChimeError> {
    let mut riff = [0u8; 4];
    r.read_exact(&mut riff)?;
    if &riff != b"RIFF" {
        return Err(ChimeError::InvalidData("Not a WAV file (missing RIFF)".into()));
    }
    let _file_size = r.read_u32::<LittleEndian>()?;
    let mut wave = [0u8; 4];
    r.read_exact(&mut wave)?;
    if &wave != b"WAVE" {
        return Err(ChimeError::InvalidData("Not a WAV file (missing WAVE)".into()));
    }

    let mut channels = 0u16;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u16;
    let mut format_tag = 0u16;
    let mut data_size = 0u32;
    let mut found_fmt = false;
    let mut found_data = false;

    loop {
        let mut chunk_id = [0u8; 4];
        match r.read_exact(&mut chunk_id) {
            Ok(()) => {}
            Err(_) => break,
        }
        let chunk_size = r.read_u32::<LittleEndian>()?;
        match &chunk_id {
            b"fmt " => {
                format_tag = r.read_u16::<LittleEndian>()?;
                channels = r.read_u16::<LittleEndian>()?;
                sample_rate = r.read_u32::<LittleEndian>()?;
                let _byte_rate = r.read_u32::<LittleEndian>()?;
                let _block_align = r.read_u16::<LittleEndian>()?;
                bits_per_sample = r.read_u16::<LittleEndian>()?;
                // skip extra fmt bytes
                let extra = chunk_size.saturating_sub(16);
                if extra > 0 {
                    let mut skip = vec![0u8; extra as usize];
                    r.read_exact(&mut skip)?;
                }
                found_fmt = true;
            }
            b"data" => {
                data_size = chunk_size;
                found_data = true;
                break;
            }
            _ => {
                // skip unknown chunk
                let mut skip = vec![0u8; chunk_size as usize];
                r.read_exact(&mut skip)?;
            }
        }
    }

    if !found_fmt || !found_data {
        return Err(ChimeError::InvalidData("WAV missing fmt or data chunk".into()));
    }
    Ok(WavHeader { channels, sample_rate, bits_per_sample, format_tag, data_size })
}

fn wav_format_to_sample_format(tag: u16, bits: u16) -> Result<SampleFormat, ChimeError> {
    match (tag, bits) {
        (WAVE_FORMAT_PCM, 8) => Ok(SampleFormat::U8),
        (WAVE_FORMAT_PCM, 16) => Ok(SampleFormat::I16),
        (WAVE_FORMAT_PCM, 24) => Ok(SampleFormat::I24),
        (WAVE_FORMAT_PCM, 32) => Ok(SampleFormat::I32),
        (WAVE_FORMAT_IEEE_FLOAT, 32) => Ok(SampleFormat::F32),
        (WAVE_FORMAT_IEEE_FLOAT, 64) => Ok(SampleFormat::F64),
        (WAVE_FORMAT_EXTENSIBLE, _) => {
            // For extensible, treat same as PCM/float by bits
            // In a full impl we'd parse the subformat GUID
            match bits {
                16 => Ok(SampleFormat::I16),
                24 => Ok(SampleFormat::I24),
                32 => Ok(SampleFormat::F32),
                _ => Err(ChimeError::UnsupportedFormat(format!("WAV extensible {}-bit", bits))),
            }
        }
        _ => Err(ChimeError::UnsupportedFormat(
            format!("WAV format_tag=0x{:04X}, bits={}", tag, bits),
        )),
    }
}

impl AudioCodec for WavDecoder {
    fn name(&self) -> &'static str { "WAV" }

    fn probe(&self, reader: &mut dyn ReadSeek) -> Result<CodecInfo, ChimeError> {
        reader.seek(SeekFrom::Start(0))?;
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        reader.seek(SeekFrom::Start(0))?;
        if &magic != b"RIFF" {
            return Err(ChimeError::UnsupportedFormat("Not RIFF".into()));
        }
        let hdr = read_wav_header(reader)?;
        let fmt = wav_format_to_sample_format(hdr.format_tag, hdr.bits_per_sample)?;
        Ok(CodecInfo {
            stream: StreamInfo::Pcm(PcmInfo {
                sample_rate: hdr.sample_rate,
                channels: hdr.channels,
                sample_format: fmt,
                total_frames: Some(hdr.data_size as u64 / (hdr.bits_per_sample as u64 / 8) / hdr.channels as u64),
                bits_per_sample: hdr.bits_per_sample,
            }),
            format_name: "WAV",
        })
    }

    fn decode(&self, reader: &mut dyn ReadSeek) -> Result<AudioData, ChimeError> {
        reader.seek(SeekFrom::Start(0))?;
        let hdr = read_wav_header(reader)?;
        let fmt = wav_format_to_sample_format(hdr.format_tag, hdr.bits_per_sample)?;
        let mut pcm_data = vec![0u8; hdr.data_size as usize];
        reader.read_exact(&mut pcm_data)?;
        let buf = AudioBuffer::from_pcm_bytes(&pcm_data, fmt, hdr.channels, hdr.sample_rate);
        Ok(AudioData::Pcm(buf))
    }
}