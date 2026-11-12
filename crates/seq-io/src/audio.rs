//! Audio output. Desktop: cpal (WASAPI on Windows). Android: oboe/AAudio used directly,
//! because cpal 0.15 does not request low-latency mode there (stream routed through the normal
//! mixer, ~300 ms measured on a Xiaomi 13T).
//!
//! Shared API: `list_output_devices`, `prepare_output` (sample rate known before creating
//! the engine), `start_output` (the engine is moved into the callback), `AudioOut`.

#[cfg(not(target_os = "android"))]
#[path = "audio_cpal.rs"]
mod backend;
#[cfg(target_os = "android")]
#[path = "audio_android.rs"]
mod backend;

pub use backend::*;
