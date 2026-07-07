# Chime

High-performance audio playback engine built in Rust, with first-class support for DSD/SACD formats alongside standard PCM.

## Features

- **PCM formats**: WAV (8/16/24/32-bit int, 32/64-bit float)
- **DSD formats**: DSF (Sony), DFF/DSDIFF (Philips)
- **DSD-to-PCM conversion**: Multi-stage decimation FIR filter with Kaiser windowing
- **DST decoding**: Direct Stream Transfer decompression for SACD (arithmetic coding + FIR prediction)
- **Cross-platform audio output**: via cpal (WASAPI, CoreAudio, ALSA/PulseAudio)
- **DoP encoding**: DSD over PCM for compatible DACs

## Architecture

```
chime/
+-- chime-core/        Core types: SampleFormat, AudioBuffer, codec traits
+-- chime-codec-pcm/   WAV decoder
+-- chime-codec-dsd/   DSF and DFF/DSDIFF parsers
+-- chime-codec-dst/   DST decoder (arithmetic coding + FIR prediction)
+-- chime-dsp/         DSD-to-PCM conversion, FIR filters, decimation
+-- chime-output/      Cross-platform audio output via cpal
+-- chime-cli/         Command-line player
```

## Usage

```bash
# Play a WAV file
cargo run --bin chime -- song.wav

# Play a DSF file with DSD-to-PCM at 176.4kHz
cargo run --bin chime -- track.dsf --dsd-target-rate 176400

# Show file info
cargo run --bin chime -- track.dff --info

# List audio devices
cargo run --bin chime -- --list-devices
```

## DSD-to-PCM Conversion Pipeline

1. Unpack 1-bit DSD stream from packed bytes (MSB-first per byte)
2. Map bits to +1.0/-1.0 amplitude values
3. Multi-stage FIR decimation filter (Kaiser-windowed sinc)
4. DC offset removal
5. Soft-clipping via tanh to remove DSD quantization noise
6. Interleave channels for output

### Filter Design

The decimation uses cascaded half-band filters for computational efficiency:
- DSD64 (2.8224 MHz) -> 176.4 kHz: 3 stages (/4, /2, /2)
- Each stage uses a Kaiser-windowed sinc with >96dB stopband attenuation
- Filter order adapts automatically to the desired attenuation

## License

MIT OR Apache-2.0
