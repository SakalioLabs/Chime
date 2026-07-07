//! SACD audio frame parser.
//!
//! Parses SACD audio frames from raw track data, identifying DST vs DSD frames.
//! Bridges the gap between SacdIsoParser and the DST decoder.
//!
//! Audio frame structure (Scarlet Book):
//! - Each frame = 1/75 second of audio
//! - DSD64 stereo: 37632 DSD samples/channel/frame = 9408 bytes/frame
//! - DST frames start with sync 0xC000C000
//! - Raw DSD frames have no header

use chime_core::ChimeError;
use byteorder::{BigEndian, ReadBytesExt};
use std::io::{Read, Cursor};

/// SACD audio frame type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SacdFrameType {
    /// Raw DSD data (no compression).
    Dsd,
    /// DST-compressed frame.
    Dst,
}

/// One SACD audio frame (1/75 second).
#[derive(Debug, Clone)]
pub struct SacdAudioFrame {
    /// Frame type (DST or raw DSD).
    pub frame_type: SacdFrameType,
    /// Frame number within the track.
    pub frame_number: u32,
    /// Raw frame data.
    pub data: Vec<u8>,
}

/// DST frame sync word.
const DST_SYNC: u32 = 0xC000_C000;

/// Parse SACD audio frames from raw track data.
///
/// The raw data is a sequence of 2048-byte sectors or a continuous byte stream.
/// Frames are 1/75 second each. The function scans for DST sync words to
/// identify DST frames; everything else is treated as raw DSD.
pub fn parse_audio_frames(
    raw_data: &[u8],
    channels: u16,
    dsd_rate: u32,
) -> Result<Vec<SacdAudioFrame>, ChimeError> {
    if raw_data.is_empty() {
        return Err(ChimeError::InvalidData("Empty audio data".into()));
    }

    // Frame size in bytes for raw DSD (DSD64 default)
    let dsd_samples_per_frame = dsd_rate as u64 / 75;
    let raw_frame_size = (dsd_samples_per_frame * channels as u64 / 8) as usize;

    let mut frames = Vec::new();
    let mut cursor = Cursor::new(raw_data);
    let mut frame_number = 0u32;

    while (cursor.position() as usize) < raw_data.len() {
        // Peek ahead for DST sync word
        let remaining = raw_data.len() - cursor.position() as usize;
        if remaining < 4 {
            break;
        }

        // Try to read the next 4 bytes as a potential sync word
        let peek_pos = cursor.position();
        let sync_word = raw_data[peek_pos as usize..peek_pos as usize + 4]
            .try_into()
            .map(u32::from_be_bytes)
            .unwrap_or(0);

        if sync_word == DST_SYNC {
            // DST frame: read 4-byte sync, then 4-byte frame size, then data
            cursor.read_u32::<BigEndian>()?; // skip sync
            let dst_data_size = cursor.read_u32::<BigEndian>()? as usize;

            if remaining < 8 + dst_data_size {
                break; // truncated frame
            }

            let mut frame_data = vec![0u8; dst_data_size];
            cursor.read_exact(&mut frame_data)?;

            frames.push(SacdAudioFrame {
                frame_type: SacdFrameType::Dst,
                frame_number,
                data: frame_data,
            });
        } else {
            // Raw DSD frame: read raw_frame_size bytes
            let frame_size = raw_frame_size.min(remaining);
            let mut frame_data = vec![0u8; frame_size];
            cursor.read_exact(&mut frame_data)?;

            frames.push(SacdAudioFrame {
                frame_type: SacdFrameType::Dsd,
                frame_number,
                data: frame_data,
            });
        }

        // Align to 2048-byte sector boundary if we're in a sector structure
        frame_number += 1;
    }

    Ok(frames)
}

/// Process SACD audio frames through the DSD-to-PCM or DST decoder pipeline.
/// Returns decompressed DSD data per channel.
pub fn process_audio_frames(
    frames: &[SacdAudioFrame],
    channels: u16,
    dsd_rate: u32,
) -> Result<Vec<u8>, ChimeError> {
    let ch = channels as usize;
    let dsd_samples_per_frame = (dsd_rate as u64) / 75;
    let bytes_per_ch_per_frame = (dsd_samples_per_frame / 8) as usize;
    let frame_dsd_bytes = bytes_per_ch_per_frame * ch;

    let mut output = Vec::with_capacity(frames.len() * frame_dsd_bytes);

    for frame in frames {
        match frame.frame_type {
            SacdFrameType::Dsd => {
                // Raw DSD: pass through
                output.extend_from_slice(&frame.data);
            }
            SacdFrameType::Dst => {
                // DST: decompress using DST decoder, then interleave channels
                let decoder = chime_codec_dst::DstDecoder::new(bytes_per_ch_per_frame, ch);
                let channels_data = decoder.decode_frame(&frame.data)?;
                // Interleave channels: [ch0_byte0, ch1_byte0, ch0_byte1, ch1_byte1, ...]
                for byte_idx in 0..bytes_per_ch_per_frame {
                    for ch_idx in 0..ch {
                        if byte_idx < channels_data[ch_idx].len() {
                            output.push(channels_data[ch_idx][byte_idx]);
                        }
                    }
                }
            }
        }
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_dsd_frames() {
        // Mock raw DSD data: 2 frames at DSD64 stereo
        let dsd_samples_per_frame = 2822400 / 75; // 37632
        let raw_frame_size = dsd_samples_per_frame * 2 / 8; // 9408
        let data = vec![0xA5u8; raw_frame_size * 2];

        let frames = parse_audio_frames(&data, 2, 2822400).unwrap();
        assert_eq!(frames.len(), 2, "should parse 2 DSD frames");
        assert_eq!(frames[0].frame_type, SacdFrameType::Dsd);
        assert_eq!(frames[1].frame_type, SacdFrameType::Dsd);
    }

    #[test]
    fn test_parse_dst_frames() {
        // Mock DST data: sync + size + data
        let dst_data_size = 512u32;
        let mut data = Vec::new();
        data.extend_from_slice(&DST_SYNC.to_be_bytes());
        data.extend_from_slice(&dst_data_size.to_be_bytes());
        data.extend_from_slice(&vec![0x00u8; dst_data_size as usize]);
        // Second DST frame
        data.extend_from_slice(&DST_SYNC.to_be_bytes());
        data.extend_from_slice(&dst_data_size.to_be_bytes());
        data.extend_from_slice(&vec![0x00u8; dst_data_size as usize]);

        let frames = parse_audio_frames(&data, 2, 2822400).unwrap();
        assert_eq!(frames.len(), 2, "should parse 2 DST frames");
        assert_eq!(frames[0].frame_type, SacdFrameType::Dst);
        assert_eq!(frames[1].frame_type, SacdFrameType::Dst);
    }
}