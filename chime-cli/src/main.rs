//! Chime CLI — command-line audio player supporting PCM and DSD formats.
//!
//! Usage: chime <file> [--output-rate <hz>] [--dsd-target-rate <hz>]

use clap::Parser;
use chime_core::codec::{AudioCodec, AudioData};
use chime_core::buffer::AudioBuffer;
use chime_core::ChimeError;
use chime_codec_pcm::{WavDecoder, AiffDecoder};
use chime_codec_dsd::{DsfDecoder, DffDecoder, SacdIsoParser, parse_audio_frames, SacdFrameType};
use chime_codec_dst::DstDecoder;
use chime_dsp::DsdToPcmConverter;
use chime_output::{AudioOutput, OutputConfig};
use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::Path;

#[derive(Parser, Debug)]
#[command(name = "chime", version, about = "High-performance audio player with DSD/SACD support")]
struct Cli {
    /// Audio file to play (WAV, DSF, DFF/DSDIFF)
    file: String,

    /// Target PCM sample rate for DSD conversion
    #[arg(long, default_value_t = 176400)]
    dsd_target_rate: u32,

    /// Output sample rate (resample if different from decoded rate)
    #[arg(long)]
    output_rate: Option<u32>,

    /// List available audio output devices
    #[arg(long)]
    list_devices: bool,

    /// Show file info without playing
    #[arg(long)]
    info: bool,
}

fn detect_and_decode(path: &str) -> Result<AudioData, ChimeError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);

    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "aif" | "aiff" | "aifc" => {
            let decoder = AiffDecoder::new();
            decoder.decode(&mut reader)
        }
        "wav" => {
            let decoder = WavDecoder::new();
            decoder.decode(&mut reader)
        }
        "dsf" => {
            let decoder = DsfDecoder::new();
            decoder.decode(&mut reader)
        }
        "dff" | "dsdiff" | "dffd" => {
            let decoder = DffDecoder::new();
            decoder.decode(&mut reader)
        }
        _ => {
            // Try probing each codec
            let codecs: Vec<Box<dyn AudioCodec>> = vec![
                Box::new(WavDecoder::new()),
                Box::new(DsfDecoder::new()),
                Box::new(DffDecoder::new()),
            ];
            for codec in &codecs {
                reader.seek(std::io::SeekFrom::Start(0))?;
                if codec.probe(&mut reader).is_ok() {
                    reader.seek(std::io::SeekFrom::Start(0))?;
                    return codec.decode(&mut reader);
                }
            }
            Err(ChimeError::UnsupportedFormat(format!(
                "No codec found for file: {}",
                path
            )))
        }
    }
}

fn print_info(data: &AudioData) {
    match data {
        AudioData::Pcm(buf) => {
            println!("Format: PCM");
            println!("Channels: {}", buf.channels);
            println!("Sample rate: {} Hz", buf.sample_rate);
            println!("Frames: {}", buf.frames);
            println!("Duration: {:.2}s", buf.duration_secs());
        }
        AudioData::Dsd { sample_rate, channels, data } => {
            let multiplier = *sample_rate as f64 / 44100.0;
            println!("Format: DSD (DSD{})", multiplier as u32);
            println!("Channels: {}", channels);
            println!("DSD rate: {} Hz", sample_rate);
            println!("Data size: {} bytes", data.len());
            let bytes_per_ch = data.len() / *channels as usize;
            let seconds = bytes_per_ch as f64 * 8.0 / *sample_rate as f64;
            println!("Duration: {:.2}s", seconds);
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("chime=info".parse()?),
        )
        .init();

    let cli = Cli::parse();

    if cli.list_devices {
        let devices = chime_output::list_devices()?;
        println!("Available output devices:");
        for (i, name) in devices.iter().enumerate() {
            println!("  {}: {}", i, name);
        }
        return Ok(());
    }

    println!("Loading: {}", cli.file);
    let data = detect_and_decode(&cli.file)?;

    if cli.info {
        print_info(&data);
        return Ok(());
    }

    // Convert to PCM AudioBuffer
    let pcm_buffer: AudioBuffer = match data {
        AudioData::Pcm(buf) => buf,
        AudioData::Dsd { data, sample_rate, channels } => {
            println!("Converting DSD ({} Hz) to PCM ({} Hz)...", sample_rate, cli.dsd_target_rate);
            let converter = DsdToPcmConverter::new(chime_dsp::DsdPcmConfig {
                target_sample_rate: cli.dsd_target_rate,
                remove_dc: true,
                apply_lowpass: true,
            });
            converter.convert(&data, sample_rate, channels)?
        }
    };

    println!(
        "Playing: {}ch {}Hz, {:.1}s",
        pcm_buffer.channels,
        pcm_buffer.sample_rate,
        pcm_buffer.duration_secs()
    );

    let output_config = OutputConfig {
        sample_rate: cli.output_rate.unwrap_or(pcm_buffer.sample_rate),
        channels: pcm_buffer.channels,
        buffer_size: 4096,
    };

    let output = AudioOutput::new(output_config)?;
    output.load(&pcm_buffer);
    output.play()?;

    // Wait for playback to finish
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if output.is_finished() {
            break;
        }
    }

    println!("Playback complete.");
    Ok(())
}
