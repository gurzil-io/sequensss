//! Android audio output: oboe (AAudio), low-latency mode, exclusive sharing when possible
//! (otherwise oboe falls back to shared), hardware native rate (no system resampling).

use crate::{IoError, Result};
use oboe::{
    AudioOutputCallback, AudioOutputStreamSafe, AudioStream, AudioStreamAsync, AudioStreamBase, AudioStreamSafe,
    AudioStreamBuilder, DataCallbackResult, Error, Output, PerformanceMode, SharingMode, Stereo,
    Usage,
};
use seq_engine::Engine;
use seq_i18n::{tr, trf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

/// Name of the single output (follows the current language: never in a `const`).
fn device_name() -> &'static str {
    tr!("Sortie Android (AAudio)", "Android output (AAudio)")
}
/// Frames rendered per engine call; a longer callback is split up.
const SCRATCH_FRAMES: usize = 1024;

pub fn list_output_devices() -> Vec<String> {
    vec![device_name().to_string()]
}

pub struct PreparedOutput {
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
}

pub struct AudioOut {
    _stream: AudioStreamAsync<Output, Callback>,
    pub name: String,
    pub sample_rate: u32,
    pub channels: u16,
    /// Number of errors (disconnection: headphones plugged/unplugged, etc.).
    pub errors: Arc<AtomicU32>,
}

impl AudioOut {
    pub fn error_count(&self) -> u32 {
        self.errors.load(Ordering::Relaxed)
    }
}

fn builder() -> AudioStreamBuilder<Output, Stereo, f32> {
    AudioStreamBuilder::default()
        .set_output()
        .set_stereo()
        .set_f32()
        .set_performance_mode(PerformanceMode::LowLatency)
        .set_sharing_mode(SharingMode::Exclusive)
        .set_usage(Usage::Media)
}

/// Opens a probe stream (no callback) to get the native sample rate, then closes it.
pub fn prepare_output(_name: Option<&str>) -> Result<PreparedOutput> {
    let mut probe = builder().open_stream().map_err(|e| IoError::Audio(format!("{e:?}")))?;
    let sample_rate = probe.get_sample_rate();
    let _ = probe.close();
    if sample_rate <= 0 {
        return Err(IoError::Audio(tr!("fréquence d'échantillonnage inconnue", "unknown sample rate").into()));
    }
    Ok(PreparedOutput { name: device_name().into(), sample_rate: sample_rate as u32, channels: 2 })
}

pub struct Callback {
    engine: Engine,
    scratch: Vec<f32>,
    errors: Arc<AtomicU32>,
}

impl AudioOutputCallback for Callback {
    type FrameType = (f32, Stereo);

    fn on_audio_ready(
        &mut self,
        stream: &mut dyn AudioOutputStreamSafe,
        frames: &mut [(f32, f32)],
    ) -> DataCallbackResult {
        let latency = stream.calculate_latency_millis().map(|ms| ms / 1000.0).unwrap_or(0.0);
        // No (f32, f32) → [f32] cast: tuple layout is not guaranteed.
        for chunk in frames.chunks_mut(SCRATCH_FRAMES) {
            let buf = &mut self.scratch[..chunk.len() * 2];
            self.engine.process(buf, 2, latency);
            for (f, s) in chunk.iter_mut().zip(buf.chunks_exact(2)) {
                *f = (s[0], s[1]);
            }
        }
        DataCallbackResult::Continue
    }

    fn on_error_after_close(&mut self, _stream: &mut dyn AudioOutputStreamSafe, _error: Error) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn start_output(prepared: PreparedOutput, engine: Engine) -> Result<AudioOut> {
    let errors = Arc::new(AtomicU32::new(0));
    let cb = Callback { engine, scratch: vec![0.0; SCRATCH_FRAMES * 2], errors: errors.clone() };
    let mut stream = builder()
        .set_sample_rate(prepared.sample_rate as i32)
        .set_callback(cb)
        .open_stream()
        .map_err(|e| IoError::Audio(format!("{e:?}")))?;
    let sr = stream.get_sample_rate();
    if sr != prepared.sample_rate as i32 {
        return Err(IoError::Audio(trf!("fréquence obtenue {sr} Hz ≠ {} Hz", "got {sr} Hz instead of {} Hz", prepared.sample_rate)));
    }
    let sharing = match stream.get_sharing_mode() {
        SharingMode::Exclusive => tr!("exclusif", "exclusive"),
        _ => tr!("partagé", "shared"),
    };
    let perf = match stream.get_performance_mode() {
        PerformanceMode::LowLatency => tr!("basse latence", "low latency"),
        _ => tr!("latence normale", "normal latency"),
    };
    let burst = stream.get_frames_per_burst();
    let buffer = stream.get_buffer_size_in_frames();
    stream.start().map_err(|e| IoError::Audio(format!("{e:?}")))?;
    Ok(AudioOut {
        name: format!(
            "{} · {sr} Hz · {perf} · {sharing} · burst {burst} · buffer {buffer} ({:.1} ms)",
            device_name(),
            buffer as f64 * 1000.0 / sr as f64
        ),
        _stream: stream,
        sample_rate: prepared.sample_rate,
        channels: 2,
        errors,
    })
}
