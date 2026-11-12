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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_mono_wav(path: &std::path::Path, rate: u32, frames: usize) {
        let spec = hound::WavSpec { channels: 1, sample_rate: rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            w.write_sample(((i as f32 * 0.01).sin() * 16000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }

    #[test]
    fn file_sample_resampled_to_device_rate() {
        let dir = std::env::temp_dir().join(format!("sequensss-test-cache-rate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.wav");
        write_mono_wav(&path, 22_050, 2_205);
        let src = SampleSource::File(path.clone());
        let mut cache = SampleCache::default();

        let up = cache.get(&src, 44_100).unwrap();
        assert_eq!(up.sample_rate, 44_100);
        assert_eq!(up.len(), 4_410);
        assert_eq!(up.left, up.right);
        // Even output samples fall on the original ones.
        assert!((up.left[200] - ((100.0f32 * 0.01).sin() * 16000.0) as i16 as f32 / 32768.0).abs() < 1e-6);

        let down = cache.get(&src, 11_025).unwrap();
        assert_eq!(down.sample_rate, 11_025);
        assert_eq!(down.len(), 1_102);

        let same = cache.get(&src, 22_050).unwrap();
        assert_eq!(same.len(), 2_205);

        // Second call hits the cache (same Arc).
        assert!(Arc::ptr_eq(&up, &cache.get(&src, 44_100).unwrap()));
    }

    #[test]
    fn invalidate_reloads_modified_file() {
        let dir = std::env::temp_dir().join(format!("sequensss-test-cache-inval-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.wav");
        write_mono_wav(&path, 48_000, 100);
        let src = SampleSource::File(path.clone());
        let mut cache = SampleCache::default();
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 100);
        write_mono_wav(&path, 48_000, 300);
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 100);
        cache.invalidate(&src);
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 300);
    }

    #[test]
    fn build_slot_errors_on_missing_file() {
        let mut p = seq_model::Project::empty();
        let id = p
            .add_channel("x", ChannelKind::Sampler { source: SampleSource::File("/nonexistent/sequensss/x.wav".into()) })
            .unwrap();
        let mut cache = SampleCache::default();
        assert!(build_slot(p.channel(id).unwrap(), 48_000, &mut cache).is_err());
        let midi = p.add_channel("m", ChannelKind::MidiOut { channel: 3, out: 0 }).unwrap();
        assert!(matches!(build_slot(p.channel(midi).unwrap(), 48_000, &mut cache), Ok(SlotKind::Midi)));
    }
}
