//! Offline rendering (WAV export, tests), without an audio device.

use crate::MAX_BLOCK;
use crate::clock::samples_per_tick;
use crate::engine::{Engine, EngineCmd, PlayMode, SlotKind};
use crate::graph::PlaybackGraph;
use seq_model::ChannelId;

/// Renders the region (pattern or whole song) once, plus `tail_secs` of tail.
/// Returns interleaved stereo audio.
pub fn render(
    graph: PlaybackGraph,
    slots: Vec<(ChannelId, SlotKind)>,
    sample_rate: u32,
    mode: PlayMode,
    tail_secs: f64,
) -> Vec<f32> {
    let (mut engine, mut handle) = Engine::new(sample_rate, None, None);
    engine.set_one_shot(true);
    for (id, kind) in slots {
        engine.handle_cmd(EngineCmd::SetSlot(id, kind));
    }
    let len_ticks = match mode {
        PlayMode::Pattern(id) => graph.pattern_index(id).map(|i| graph.patterns[i].length).unwrap_or(0),
        PlayMode::Song => graph.song_length,
    };
    let spt = samples_per_tick(sample_rate as f64, graph.tempo_bpm);
    engine.handle_cmd(EngineCmd::SwapGraph(Box::new(graph)));
    engine.handle_cmd(EngineCmd::SetMode(mode));
    engine.handle_cmd(EngineCmd::Stop);
    engine.handle_cmd(EngineCmd::Play);
    let frames = (len_ticks as f64 * spt).ceil() as usize + (tail_secs * sample_rate as f64) as usize;
    let mut out = vec![0.0f32; frames * 2];
    for chunk in out.chunks_mut(MAX_BLOCK * 2) {
        engine.process(chunk, 2, 0.0);
        while handle.poll_event().is_some() {}
        handle.collect_garbage();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::compile;
    use crate::sampler::Sampler;
    use crate::{SampleData, drums};
    use seq_model::{Drum, Project};
    use std::sync::Arc;

    #[test]
    fn render_kit_pattern() {
        let p = Project::from_kit(0);
        let slots = p
            .channels
            .iter()
            .map(|c| {
                let s: Arc<SampleData> = Arc::new(drums::render(Drum::Kick, 48_000));
                (c.id, SlotKind::Audio(Box::new(Sampler::new(s)) as Box<dyn crate::Instrument>))
            })
            .collect();
        let pat = p.patterns[0].id;
        let out = render(compile(&p), slots, 48_000, PlayMode::Pattern(pat), 0.5);
        // 1 bar at 120 BPM = 2 s, + 0.5 s of tail.
        assert_eq!(out.len(), (2.5 * 48_000.0) as usize * 2);
        assert!(out.iter().any(|x| x.abs() > 0.1));
        assert!(out.iter().all(|x| x.abs() <= 1.0));
    }
}
