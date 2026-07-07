//! SACD ISO image parser.
//!
//! Parses SACD (Super Audio CD) ISO disc images based on the Scarlet Book specification.
//! Supports extracting track listings and reading audio data from:
//! - Stereo area (2-channel DSD/DST)
//! - Multi-channel area (up to6-channel DSD/DST)
//!
//! SACD ISO structure:
//! - Sectors are2048 bytes each
//! - Master TOC at sector510 (offset 510*2048)
//! - Area TOCs follow the master TOC
//! - Audio data sectors contain DST-compressed DSD frames

use chime_core::ChimeError;
use byteorder::{LittleEndian, ReadBytesExt};
use std::io::SeekFrom;

const SECTOR_SIZE: u64 = 2048;
const MASTER_TOC_SECTOR: u64 = 510;
const SACD_MAGIC: &[u8; 8] = b"SACDMTOC";

/// SACD disc channel type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SacdChannelType {
    /// Two-channel (stereo) only
    TwoChannel,
    /// Multi-channel only (up to6ch)
    MultiChannel,
    /// Both two-channel and multi-channel areas
    Both,
}

/// A track in the SACD TOC.
#[derive(Debug, Clone)]
pub struct SacdTrack {
    /// Track number (1-based).
    pub number: u8,
    /// Track title (DSD-encoded, up to160 chars).
    pub title: Vec<u8>,
    /// Duration in frames (at44100 Hz base rate).
    pub duration_frames: u32,
    /// Starting sector offset within the area.
    pub start_sector: u32,
    /// Ending sector offset within the area.
    pub end_sector: u32,
}

/// SACD Area TOC containing track information for one area (stereo or multi-channel).
#[derive(Debug, Clone)]
pub struct SacdAreaToc {
    /// Version of this area TOC.
    pub version: u16,
    /// Number of channels in this area (2 for stereo, up to6 for MCH).
    pub channel_count: u16,
    /// Channel assignment (see SACD spec for mapping).
    pub channel_assignment: u8,
    /// DSD sample rate (typically2822400 for DSD64).
    pub dsd_sample_rate: u32,
    /// Track list.
    pub tracks: Vec<SacdTrack>,
    /// Starting sector of audio data for this area.
    pub audio_start_sector: u32,
    /// Total size of audio data in sectors.
    pub audio_size_sectors: u32,
}

/// Master SACD TOC parsed from the ISO image.
#[derive(Debug, Clone)]
pub struct SacdMasterToc {
    /// SACD version.
    pub version: u16,
    /// Channel type (stereo, multi-channel, or both).
    pub channel_type: SacdChannelType,
    /// Catalog number.
    pub catalog_number: Vec<u8>,
    /// Area1 (stereo) TOC info.
    pub area1: Option<SacdAreaToc>,
    /// Area2 (multi-channel) TOC info.
    pub area2: Option<SacdAreaToc>,
}

/// SACD ISO image parser.
pub struct SacdIsoParser;

impl SacdIsoParser {
    /// Probe an ISO image to determine if it's a SACD image.
    pub fn probe<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Result<bool, ChimeError> {
        reader.seek(SeekFrom::Start(MASTER_TOC_SECTOR * SECTOR_SIZE))?;
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        Ok(&magic == SACD_MAGIC)
    }

    /// Parse the master TOC from an SACD ISO image.
    pub fn read_master_toc<R: std::io::Read + std::io::Seek>(reader: &mut R) -> Result<SacdMasterToc, ChimeError> {
        reader.seek(SeekFrom::Start(MASTER_TOC_SECTOR * SECTOR_SIZE))?;
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != SACD_MAGIC {
            return Err(ChimeError::InvalidData("Not a SACD ISO image (missing SACDMTOC magic)".into()));
        }

        let version = reader.read_u16::<LittleEndian>()?;
        let _reserved1 = reader.read_u16::<LittleEndian>()?;

        // Read disc type
        let disc_type = reader.read_u8()?;
        let channel_type = match disc_type {
            0 => SacdChannelType::TwoChannel,
            1 => SacdChannelType::MultiChannel,
            2 => SacdChannelType::Both,
            _ => return Err(ChimeError::InvalidData(format!("Unknown SACD disc type: {}", disc_type))),
        };

        let _reserved2 = reader.read_u8()?;

        // Read catalog number (16 bytes, DSD text)
        let mut catalog = [0u8; 16];
        reader.read_exact(&mut catalog)?;

        // Read area info
        let area1_channel_count = reader.read_u8()?;
        let area1_channel_assign = reader.read_u8()?;
        let area2_channel_count = reader.read_u8()?;
        let area2_channel_assign = reader.read_u8()?;

        // Skip to track count fields
        reader.seek(SeekFrom::Current(28))?; // skip various reserved fields
        let area1_track_count = reader.read_u8()?;
        let area2_track_count = reader.read_u8()?;

        // Parse area TOCs
        let area1 = if area1_track_count > 0 {
            Some(Self::read_area_toc(reader, MASTER_TOC_SECTOR + 2, area1_channel_count, area1_channel_assign)?)
        } else {
            None
        };

        let area2 = if area2_track_count > 0 {
            Some(Self::read_area_toc(reader, MASTER_TOC_SECTOR + 4, area2_channel_count, area2_channel_assign)?)
        } else {
            None
        };

        Ok(SacdMasterToc {
            version,
            channel_type,
            catalog_number: catalog.to_vec(),
            area1,
            area2,
        })
    }

    /// Read an area TOC from the given sector.
    fn read_area_toc<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        sector: u64,
        channel_count: u8,
        channel_assign: u8,
    ) -> Result<SacdAreaToc, ChimeError> {
        reader.seek(SeekFrom::Start(sector * SECTOR_SIZE))?;

        let version = reader.read_u16::<LittleEndian>()?;
        let _reserved = reader.read_u16::<LittleEndian>()?;

        // Read DSD sample rate (offset varies by version; simplified)
        let dsd_sample_rate = 2_822_400u32; // DSD64 default

        // Read track list offset and count
        reader.seek(SeekFrom::Current(8))?;
        let track_count = reader.read_u8()?;
        reader.seek(SeekFrom::Current(3))?; // reserved

        let mut tracks = Vec::new();
        for i in 0..track_count {
            let mut title = [0u8; 160];
            reader.read_exact(&mut title)?;

            // Skip to track timing info
            let duration_minutes = reader.read_u8()?;
            let duration_seconds = reader.read_u8()?;
            let duration_frames = reader.read_u16::<LittleEndian>()?;
            let start_sector = reader.read_u32::<LittleEndian>()?;
            let end_sector = reader.read_u32::<LittleEndian>()?;

            let total_frames = (duration_minutes as u32 *60 + duration_seconds as u32) *44100
                + (duration_frames as u32 *44100 /75);

            tracks.push(SacdTrack {
                number: i +1,
                title: title.to_vec(),
                duration_frames: total_frames,
                start_sector,
                end_sector,
            });
        }

        // Read audio start/size from the area TOC header
        reader.seek(SeekFrom::Start(sector * SECTOR_SIZE + 0x10))?;
        let audio_start_sector = reader.read_u32::<LittleEndian>()?;
        let audio_size_sectors = reader.read_u32::<LittleEndian>()?;

        Ok(SacdAreaToc {
            version,
            channel_count: channel_count as u16,
            channel_assignment: channel_assign,
            dsd_sample_rate,
            tracks,
            audio_start_sector,
            audio_size_sectors,
        })
    }

    /// Read raw audio data for a track from the ISO image.
    pub fn read_track_audio<R: std::io::Read + std::io::Seek>(
        reader: &mut R,
        area: &SacdAreaToc,
        track: &SacdTrack,
    ) -> Result<Vec<u8>, ChimeError> {
        let start_offset = (area.audio_start_sector as u64 + track.start_sector as u64) * SECTOR_SIZE;
        let end_offset = (area.audio_start_sector as u64 + track.end_sector as u64) * SECTOR_SIZE;
        let size = (end_offset - start_offset) as usize;

        reader.seek(SeekFrom::Start(start_offset))?;
        let mut data = vec![0u8; size];
        reader.read_exact(&mut data)?;
        Ok(data)
    }

    /// Get the best available area (prefer stereo, fall back to multi-channel).
    pub fn best_area(toc: &SacdMasterToc) -> Option<&SacdAreaToc> {
        toc.area1.as_ref().or(toc.area2.as_ref())
    }
}
