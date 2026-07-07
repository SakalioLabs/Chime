//! Cross-platform audio output backend using cpal.
//!
//! Provides real-time audio playback through the system's audio stack
//! (WASAPI on Windows, CoreAudio on macOS, ALSA/PulseAudio on Linux).

use chime_core::buffer::AudioBuffer;
use chime_core::ChimeError;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// Configuration for audio output.
#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub sample_rate: u32,
    pub channels: u16,
    pub buffer_size: usize,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44100,
            channels: 2,
            buffer_size: 4096,
        }
    }
}

/// Handle to a running audio output stream.
pub struct AudioOutput {
    stream: cpal::Stream,
    state: Arc<Mutex<PlaybackState>>,
}

struct PlaybackState {
    samples: Vec<f32>,
    position: usize,
    playing: bool,
}

/// List available audio output devices.
pub fn list_devices() -> Result<Vec<String>, ChimeError> {
    let host = cpal::default_host();
    let mut names = Vec::new();
    if let Ok(devices) = host.output_devices() {
        for d in devices {
            if let Ok(name) = d.name() {
                names.push(name);
            }
        }
    }
    Ok(names)
}

impl AudioOutput {
    /// Create a new audio output with the given configuration.
    pub fn new(config: OutputConfig) -> Result<Self, ChimeError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| ChimeError::Codec("No output device available".into()))?;

        let cpal_config = cpal::StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: cpal::BufferSize::Default,
        };

        let state = Arc::new(Mutex::new(PlaybackState {
            samples: Vec::new(),
            position: 0,
            playing: false,
        }));

        let state_clone = state.clone();
        let channels = config.channels as usize;

        let stream = device
            .build_output_stream(
                &cpal_config,
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let mut state = state_clone.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        for sample in frame.iter_mut() {
                            if state.playing && state.position < state.samples.len() {
                                *sample = state.samples[state.position];
                                state.position += 1;
                            } else {
                                *sample = 0.0;
                            }
                        }
                    }
                },
                move |err| {
                    tracing::error!("Audio output error: {}", err);
                },
                None,
            )
            .map_err(|e| ChimeError::Codec(format!("Failed to build output stream: {}", e)))?;

        Ok(Self { stream, state })
    }

    /// Load an AudioBuffer for playback.
    pub fn load(&self, buffer: &AudioBuffer) {
        let mut state = self.state.lock().unwrap();
        state.samples = buffer.samples.clone();
        state.position = 0;
    }

    /// Start playback.
    pub fn play(&self) -> Result<(), ChimeError> {
        {
            let mut state = self.state.lock().unwrap();
            state.playing = true;
        }
        self.stream
            .play()
            .map_err(|e| ChimeError::Codec(format!("Play failed: {}", e)))
    }

    /// Pause playback.
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        state.playing = false;
    }

    /// Stop and reset to beginning.
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.playing = false;
        state.position = 0;
    }

    /// Check if playback has finished.
    pub fn is_finished(&self) -> bool {
        let state = self.state.lock().unwrap();
        !state.playing && state.position >= state.samples.len()
    }

    /// Get current playback position in seconds.
    pub fn position_secs(&self, sample_rate: u32, channels: u16) -> f64 {
        let state = self.state.lock().unwrap();
        state.position as f64 / sample_rate as f64 / channels as f64
    }
}