//! DSP processing for Chime — DSD-to-PCM conversion and sample rate conversion.
//!
//! ## DSD-to-PCM conversion
//!
//! DSD (Direct Stream Digital) is a1-bit, high-sample-rate format (e.g. DSD64 at
//! 2.8224 MHz). Converting to PCM requires:
//! 1. Unpacking the1-bit stream from packed bytes
//! 2. Applying a decimation FIR filter to band-limit the signal
//! 3. Downsampling to the target PCM rate (typically176.4kHz or lower)
//!
//! The filter design uses a Kaiser-windowed sinc filter for the decimation
//! stage, which provides excellent stopband attenuation for DSD noise shaping.

use chime_core::ChimeError;

mod dsd_to_pcm;
mod filters;

pub use dsd_to_pcm::{DsdToPcmConverter, DsdPcmConfig};
pub use filters::{DecimationFilter, SincFilter};
