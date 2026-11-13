//! Builds instruments (off the audio thread) from the project's channels.

use crate::{IoError, Result, samples};
use seq_engine::sampler::Sampler;
use seq_engine::synth::Synth;
use seq_engine::{Instrument, SampleData, SlotKind, drums};
use seq_model::{Channel, ChannelKind, SampleSource};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

type Key = (SampleSource, u32);

enum Entry {
    /// Being decoded by some thread (token of that load).
    Loading(u64),
    Ready(Arc<SampleData>),
}

/// Sample cache, already resampled to the device sample rate. Thread-safe: shared between
/// the UI thread and the loader workers. A sample being decoded is never decoded a second
/// time concurrently: other callers wait for the first one.
#[derive(Default)]
pub struct SampleCache {
    map: Mutex<HashMap<Key, Entry>>,
    done: Condvar,
    next_token: AtomicU64,
    #[cfg(test)]
    pub(crate) decodes: std::sync::atomic::AtomicUsize,
}

impl SampleCache {
    fn lock(&self) -> MutexGuard<'_, HashMap<Key, Entry>> {
        self.map.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Cached sample, or decodes it (blocking). Waits if another thread is decoding it.
    pub fn get(&self, src: &SampleSource, sample_rate: u32) -> Result<Arc<SampleData>> {
        let key = (src.clone(), sample_rate);
        let mut map = self.lock();
        let token = loop {
            match map.get(&key) {
                Some(Entry::Ready(s)) => return Ok(s.clone()),
                Some(Entry::Loading(_)) => map = self.done.wait(map).unwrap_or_else(PoisonError::into_inner),
                None => {
                    let t = self.next_token.fetch_add(1, Ordering::Relaxed);
                    map.insert(key.clone(), Entry::Loading(t));
                    break t;
                }
            }
        };
        drop(map);

        #[cfg(test)]
        self.decodes.fetch_add(1, Ordering::Relaxed);
        // A panic must not leave the entry `Loading` forever (waiters would hang).
        let res = catch_unwind(AssertUnwindSafe(|| decode(src, sample_rate)))
            .unwrap_or_else(|_| Err(IoError::Other("sample decoding panicked".into())))
            .map(Arc::new);

        let mut map = self.lock();
        // Invalidated or cleared meanwhile: the result is returned but not cached.
        if matches!(map.get(&key), Some(Entry::Loading(t)) if *t == token) {
            match &res {
                Ok(s) => map.insert(key, Entry::Ready(s.clone())),
                Err(_) => map.remove(&key),
            };
        }
        drop(map);
        self.done.notify_all();
        res
    }

    /// Cached sample, without loading it.
    pub fn peek(&self, src: &SampleSource, sample_rate: u32) -> Option<Arc<SampleData>> {
        match self.lock().get(&(src.clone(), sample_rate)) {
            Some(Entry::Ready(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Forgets a file (so it is reloaded after being modified).
    pub fn invalidate(&self, src: &SampleSource) {
        self.lock().retain(|(s, _), _| s != src);
        self.done.notify_all();
    }

    /// Forgets everything (device sample rate changed).
    pub fn clear(&self) {
        self.lock().clear();
        self.done.notify_all();
    }
}

fn decode(src: &SampleSource, sample_rate: u32) -> Result<SampleData> {
    Ok(match src {
        SampleSource::Builtin(d) => drums::render(*d, sample_rate),
        SampleSource::File(path) => samples::load_wav(path)?.resampled(sample_rate),
    })
}

/// Engine slot playing an already loaded sample.
pub fn sampler_slot(sample: Arc<SampleData>) -> SlotKind {
    SlotKind::Audio(Box::new(Sampler::new(sample)) as Box<dyn Instrument>)
}

pub fn build_slot(channel: &Channel, sample_rate: u32, cache: &SampleCache) -> Result<SlotKind> {
    Ok(match &channel.kind {
        ChannelKind::Sampler { source } => sampler_slot(cache.get(source, sample_rate)?),
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
        let cache = SampleCache::default();

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
        let cache = SampleCache::default();
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 100);
        write_mono_wav(&path, 48_000, 300);
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 100);
        cache.invalidate(&src);
        assert_eq!(cache.get(&src, 48_000).unwrap().len(), 300);
    }

    #[test]
    fn concurrent_gets_decode_once() {
        let dir = std::env::temp_dir().join(format!("sequensss-test-cache-conc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.wav");
        write_mono_wav(&path, 22_050, 200_000);
        let src = SampleSource::File(path);
        let cache = SampleCache::default();
        let got: Vec<_> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..8).map(|_| s.spawn(|| cache.get(&src, 48_000).unwrap())).collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(got.iter().all(|x| Arc::ptr_eq(x, &got[0])));
        assert_eq!(cache.decodes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn build_slot_errors_on_missing_file() {
        let mut p = seq_model::Project::empty();
        let id = p
            .add_channel("x", ChannelKind::Sampler { source: SampleSource::File("/nonexistent/sequensss/x.wav".into()) })
            .unwrap();
        let cache = SampleCache::default();
        assert!(build_slot(p.channel(id).unwrap(), 48_000, &cache).is_err());
        let midi = p.add_channel("m", ChannelKind::MidiOut { channel: 3, out: 0 }).unwrap();
        assert!(matches!(build_slot(p.channel(midi).unwrap(), 48_000, &cache), Ok(SlotKind::Midi)));
    }
}
