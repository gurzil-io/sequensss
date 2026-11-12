//! Builds instruments (off the audio thread) from the project's channels.

use crate::{Result, samples};
use seq_engine::sampler::Sampler;
use seq_engine::synth::Synth;
use seq_engine::{Instrument, SampleData, SlotKind, drums};
use seq_model::{Channel, ChannelKind, SampleSource};
use std::collections::HashMap;
use std::sync::Arc;

/// Sample cache, already resampled to the device sample rate.
#[derive(Default)]
pub struct SampleCache {
    map: HashMap<(SampleSource, u32), Arc<SampleData>>,
}

impl SampleCache {
    pub fn get(&mut self, src: &SampleSource, sample_rate: u32) -> Result<Arc<SampleData>> {
        let key = (src.clone(), sample_rate);
        if let Some(s) = self.map.get(&key) {
            return Ok(s.clone());
        }
        let data = match src {
            SampleSource::Builtin(d) => drums::render(*d, sample_rate),
            SampleSource::File(path) => samples::load_wav(path)?.resampled(sample_rate),
        };
        let arc = Arc::new(data);
        self.map.insert(key, arc.clone());
        Ok(arc)
    }

    /// Forgets a file (so it is reloaded after being modified).
    pub fn invalidate(&mut self, src: &SampleSource) {
        self.map.retain(|(s, _), _| s != src);
    }
}

pub fn build_slot(channel: &Channel, sample_rate: u32, cache: &mut SampleCache) -> Result<SlotKind> {
    Ok(match &channel.kind {
        ChannelKind::Sampler { source } => {
            let s = cache.get(source, sample_rate)?;
            SlotKind::Audio(Box::new(Sampler::new(s)) as Box<dyn Instrument>)
        }
        ChannelKind::Synth(p) => SlotKind::Audio(Box::new(Synth::new(*p, sample_rate))),
        ChannelKind::MidiOut { .. } => SlotKind::Midi,
    })
}
