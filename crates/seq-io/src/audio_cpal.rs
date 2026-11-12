//! cpal audio output (WASAPI on Windows). The callback owns the `Engine`.

use crate::{IoError, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use seq_engine::Engine;
use seq_i18n::{tr, trf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

pub fn list_output_devices() -> Vec<String> {
    let host = cpal::default_host();
    match host.output_devices() {
        Ok(devs) => devs.filter_map(|d| d.name().ok()).collect(),
        Err(_) => Vec::new(),
    }
}

/// Device opened but not started yet: its sample rate is known so the engine can be created.
pub struct PreparedOutput {
    device: cpal::Device,
    config: cpal::StreamConfig,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct AudioOut {
    _stream: cpal::Stream,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Number of errors reported by cpal (device disconnected, etc.).
    pub errors: Arc<AtomicU32>,
}

impl AudioOut {
    pub fn error_count(&self) -> u32 {
        self.errors.load(Ordering::Relaxed)
    }
}

/// Opens device `name` (or the default one). `f32` format only.
/// Buffer size: the device's own (shared-mode WASAPI often rejects a fixed size).
pub fn prepare_output(name: Option<&str>) -> Result<PreparedOutput> {
    let host = cpal::default_host();
    let device = match name {
        Some(n) => host
            .output_devices()
            .map_err(|e| IoError::Audio(e.to_string()))?
            .find(|d| d.name().map(|x| x == n).unwrap_or(false))
            .ok_or_else(|| IoError::Audio(trf!("device introuvable : {n}", "device not found: {n}")))?,
        None => host.default_output_device().ok_or_else(|| IoError::Audio(tr!("aucun device de sortie", "no output device").into()))?,
    };
    let dev_name = device.name().unwrap_or_else(|_| "?".into());
    let default = device.default_output_config().map_err(|e| IoError::Audio(e.to_string()))?;
    let supported = if default.sample_format() == cpal::SampleFormat::F32 {
        default
    } else {
        // Look for an f32 config at the same sample rate.
        let sr = default.sample_rate();
        device
            .supported_output_configs()
            .map_err(|e| IoError::Audio(e.to_string()))?
            .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
            .find(|c| c.min_sample_rate() <= sr && c.max_sample_rate() >= sr)
            .map(|c| c.with_sample_rate(sr))
            .ok_or_else(|| IoError::Audio(trf!("{dev_name} : format f32 non supporté", "{dev_name}: f32 format not supported")))?
    };
    let config: cpal::StreamConfig = supported.config();
    Ok(PreparedOutput {
        name: dev_name,
        sample_rate: config.sample_rate.0,
        channels: config.channels,
        device,
        config,
    })
}

/// Starts the stream; the engine is moved into the callback.
pub fn start_output(prepared: PreparedOutput, mut engine: Engine) -> Result<AudioOut> {
    let channels = prepared.channels as usize;
    let errors = Arc::new(AtomicU32::new(0));
    let err_count = errors.clone();
    let stream = prepared
        .device
        .build_output_stream(
            &prepared.config,
            move |data: &mut [f32], info: &cpal::OutputCallbackInfo| {
                let ts = info.timestamp();
                let latency = ts.playback.duration_since(&ts.callback).map(|d| d.as_secs_f64()).unwrap_or(0.0);
                engine.process(data, channels, latency);
            },
            move |_err| {
                err_count.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| IoError::Audio(e.to_string()))?;
    stream.play().map_err(|e| IoError::Audio(e.to_string()))?;
    Ok(AudioOut {
        _stream: stream,
        name: prepared.name,
        sample_rate: prepared.sample_rate,
        channels: prepared.channels,
        errors,
    })
}
