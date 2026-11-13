//! Audio engine: transport, scheduler, instruments, mix.
//!
//! Knows nothing about cpal or egui: [`Engine::process`] fills a `&mut [f32]`.
//! RT rules in `process`: no locks, no allocation/deallocation, no I/O.

mod arp;
pub mod clock;
pub mod drums;
mod engine;
pub mod graph;
pub mod instrument;
pub mod offline;
pub mod sample;
pub mod sampler;
pub mod synth;

pub use engine::{
    Engine, EngineCmd, EngineEvent, EngineHandle, Garbage, MidiInMsg, MidiOutMsg, PlayMode, Shared,
    SlotKind, midi_msg_len,
};
pub use graph::{Compiler, PlaybackGraph, compile};
pub use instrument::Instrument;
pub use sample::SampleData;

/// Maximum internal render block size. Larger cpal buffers are split.
pub const MAX_BLOCK: usize = 512;
