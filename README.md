# Chime

High-performance audio playback engine in Rust with first-class DSD/SACD support.

## Features

- **PCM**: WAV (8/16/24/32-bit int, f32/f64)
- **DSD**: DSF (Sony), DFF/DSDIFF (Philips)
- **SACD**: ISO image parsing with track extraction
- **DST**: Direct Stream Transfer decompression (arithmetic coding + FIR prediction)
- **DSD-to-PCM**: Multi-stage Kaiser-windowed FIR decimation
- **Polyphase filter**: Cache-optimized coefficient layout
- **SIMD-friendly**: 4-wide unrolled FIR inner product for auto-vectorization
- **Fast DSP**: f32 pipeline, 256-entry LUT bit unpacking, polynomial tanh
- **Cross-platform output**: cpal (WASAPI/CoreAudio/ALSA)

## Architecture

```
chime-core/        SampleFormat, AudioBuffer, codec traits
chime-codec-pcm/   WAV decoder
chime-codec-dsd/   DSF, DFF/DSDIFF, SACD ISO parsers
chime-codec-dst/   DST decoder (arithmetic coding + FIR prediction)
chime-dsp/         DSD-to-PCM, FIR filters, polyphase decimation
chime-output/      Cross-platform audio output via cpal
chime-cli/         Command-line player
```

## Usage

```bash
cargo run --bin chime -- song.wav
cargo run --bin chime -- track.dsf --dsd-target-rate 176400
cargo run --bin chime -- disc.iso --info
cargo run --bin chime -- --list-devices
```

## Performance Optimizations

| Technique | Impact |
|---|---|
| f32 pipeline (vs f64) | 2x memory bandwidth |
| 256-entry LUT bit unpack | Zero-branch DSD decoding |
| Polynomial tanh | ~5x faster than libm |
| 4-wide FIR unrolling | SIMD auto-vectorization |
| Padded FIR input | Bounds-check-free inner loop |
| Polyphase filter | Cache-optimized decimation |
| Fat LTO + codegen-units=1 | Maximum cross-crate optimization |

## License

MIT OR Apache-2.0