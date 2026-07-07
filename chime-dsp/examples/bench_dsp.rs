//! Chime DSP Benchmarks
//!
//! Run with: cargo bench --bench dsp_bench

use chime_dsp::{DsdToPcmConverter, DsdPcmConfig, DecimationFilter, SincFilter};
use std::time::Instant;

fn bench_dsd_lut_unpack() {
    // Simulate 1 second of DSD64 stereo (2.8224 MHz, 2ch)
    let dsd_bytes_per_sec = 2_822_400 / 8; // 352800 bytes/sec
    let dsd_data = vec![0xA5u8; dsd_bytes_per_sec]; // alternating pattern

    let config = DsdPcmConfig {
        target_sample_rate: 176_400,
        remove_dc: true,
        apply_lowpass: true,
    };
    let converter = DsdToPcmConverter::new(config);

    let start = Instant::now();
    let _result = converter.convert(&dsd_data, 2_822_400, 1).unwrap();
    let elapsed = start.elapsed();
    println!("DSD64 mono 1s conversion: {:.2}ms ({:.1}x realtime)",
        elapsed.as_secs_f64() * 1000.0,
        1.0 / elapsed.as_secs_f64());
}

fn bench_dsd_stereo_conversion() {
    // 1 second of DSD64 stereo
    let dsd_bytes_per_sec = 2_822_400 / 8;
    let dsd_data = vec![0xA5u8; dsd_bytes_per_sec * 2];

    let config = DsdPcmConfig {
        target_sample_rate: 176_400,
        remove_dc: true,
        apply_lowpass: true,
    };
    let converter = DsdToPcmConverter::new(config);

    let start = Instant::now();
    let _result = converter.convert(&dsd_data, 2_822_400, 2).unwrap();
    let elapsed = start.elapsed();
    println!("DSD64 stereo 1s conversion: {:.2}ms ({:.1}x realtime)",
        elapsed.as_secs_f64() * 1000.0,
        1.0 / elapsed.as_secs_f64());
}

fn bench_fir_filter() {
    // Generate test signal: 1 second of DSD64 rate
    let len = 2_822_400;
    let signal: Vec<f32> = (0..len).map(|i| {
        if i % 2 == 0 { 1.0 } else { -1.0 }
    }).collect();

    let filter = SincFilter::design(2_822_400, 176_400, 50_000.0, 96.0);

    let start = Instant::now();
    let _output = filter.apply(&signal);
    let elapsed = start.elapsed();
    println!("FIR filter ({} taps, {} samples): {:.2}ms",
        filter.length, len, elapsed.as_secs_f64() * 1000.0);
}

fn bench_decimation_filter() {
    let len = 2_822_400;
    let signal: Vec<f32> = (0..len).map(|i| {
        if i % 2 == 0 { 1.0 } else { -1.0 }
    }).collect();

    let filter = DecimationFilter::design(2_822_400, 176_400);

    let start = Instant::now();
    let _output = filter.apply(&signal);
    let elapsed = start.elapsed();
    println!("Multi-stage decimation ({}s DSD64): {:.2}ms",
        1, elapsed.as_secs_f64() * 1000.0);
}

fn main() {
    println!("=== Chime DSP Benchmarks ===");
    println!();

    bench_dsd_lut_unpack();
    bench_dsd_stereo_conversion();
    bench_fir_filter();
    bench_decimation_filter();

    println!();
    println!("Benchmark complete.");
}