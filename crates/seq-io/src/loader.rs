//! Background sample loading: decoding and resampling run on worker threads, results come
//! back to the UI thread through a channel (polled every frame, like the engine events).
//!
//! Each request gets an id; only the latest request of a channel is delivered, so results made
//! stale by a newer request or a cancel (channel deleted, source changed…) are dropped.

use crate::Result;
use crate::instruments::SampleCache;
use seq_engine::SampleData;
use seq_model::{ChannelId, SampleSource};
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

struct Job {
    id: u64,
    channel: ChannelId,
    source: SampleSource,
    sample_rate: u32,
}

/// Finished load, for the latest request of its channel.
pub struct Loaded {
    pub id: u64,
    pub channel: ChannelId,
    pub source: SampleSource,
    pub sample_rate: u32,
    pub result: Result<Arc<SampleData>>,
}

pub struct SampleLoader {
    cache: Arc<SampleCache>,
    /// `None` if no worker could be started: requests are then loaded inline.
    jobs: Option<Sender<Job>>,
    done_tx: Sender<Loaded>,
    done: Receiver<Loaded>,
    /// Latest request id per channel still loading.
    pending: HashMap<ChannelId, u64>,
    next_id: u64,
}

impl SampleLoader {
    pub fn new(workers: usize) -> Self {
        let cache = Arc::new(SampleCache::default());
        let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        let jobs_rx = Arc::new(Mutex::new(jobs_rx));
        let mut started = 0;
        for i in 0..workers.max(1) {
            let (cache, jobs, done) = (cache.clone(), jobs_rx.clone(), done_tx.clone());
            let spawned = std::thread::Builder::new().name(format!("sample-loader-{i}")).spawn(move || {
                loop {
                    // Lock only while waiting for the next job, not while decoding.
                    let job = jobs.lock().unwrap_or_else(PoisonError::into_inner).recv();
                    let Ok(job) = job else { return };
                    if done.send(run(&cache, job)).is_err() {
                        return;
                    }
                }
            });
            started += spawned.is_ok() as usize;
        }
        SampleLoader {
            cache,
            jobs: (started > 0).then_some(jobs_tx),
            done_tx,
            done,
            pending: HashMap::new(),
            next_id: 0,
        }
    }

    /// Cache shared with the workers (synchronous loads go through it too).
    pub fn cache(&self) -> &SampleCache {
        &self.cache
    }

    /// Loads `source` for `channel` in the background, replacing any earlier request of that channel.
    pub fn request(&mut self, channel: ChannelId, source: SampleSource, sample_rate: u32) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.pending.insert(channel, id);
        let job = Job { id, channel, source, sample_rate };
        let job = match &self.jobs {
            Some(tx) => match tx.send(job) {
                Ok(()) => return id,
                Err(e) => e.0,
            },
            None => job,
        };
        let _ = self.done_tx.send(run(&self.cache, job));
        id
    }

    /// Drops the pending request of `channel` (its result will be ignored).
    pub fn cancel(&mut self, channel: ChannelId) {
        self.pending.remove(&channel);
    }

    pub fn cancel_all(&mut self) {
        self.pending.clear();
    }

    pub fn is_loading(&self, channel: ChannelId) -> bool {
        self.pending.contains_key(&channel)
    }

    /// Channels whose sample is loading.
    pub fn loading(&self) -> impl Iterator<Item = ChannelId> + '_ {
        self.pending.keys().copied()
    }

    /// Next finished load, without blocking. Stale results are skipped.
    pub fn poll(&mut self) -> Option<Loaded> {
        while let Ok(l) = self.done.try_recv() {
            if let Some(l) = self.accept(l) {
                return Some(l);
            }
        }
        None
    }

    /// Like `poll`, but waits up to `timeout` for a result.
    pub fn wait(&mut self, timeout: Duration) -> Option<Loaded> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.done.recv_timeout(left) {
                Ok(l) => {
                    if let Some(l) = self.accept(l) {
                        return Some(l);
                    }
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn accept(&mut self, l: Loaded) -> Option<Loaded> {
        if self.pending.get(&l.channel) != Some(&l.id) {
            return None;
        }
        self.pending.remove(&l.channel);
        Some(l)
    }
}

fn run(cache: &SampleCache, job: Job) -> Loaded {
    let result = cache.get(&job.source, job.sample_rate);
    Loaded { id: job.id, channel: job.channel, source: job.source, sample_rate: job.sample_rate, result }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::Ordering;

    const T: Duration = Duration::from_secs(10);

    fn tmp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sequensss-test-loader-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_wav(path: &Path, frames: usize) -> SampleSource {
        let spec = hound::WavSpec { channels: 1, sample_rate: 22_050, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for i in 0..frames {
            w.write_sample(((i as f32 * 0.01).sin() * 16000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        SampleSource::File(path.to_path_buf())
    }

    fn decodes(l: &SampleLoader) -> usize {
        l.cache.decodes.load(Ordering::Relaxed)
    }

    #[test]
    fn result_is_delivered_resampled() {
        let src = write_wav(&tmp_dir("deliver").join("a.wav"), 2_205);
        let mut l = SampleLoader::new(2);
        let id = l.request(ChannelId(3), src.clone(), 44_100);
        assert!(l.is_loading(ChannelId(3)));
        let r = l.wait(T).unwrap();
        assert_eq!((r.id, r.channel, r.sample_rate), (id, ChannelId(3), 44_100));
        assert_eq!(r.source, src);
        let s = r.result.unwrap();
        assert_eq!((s.sample_rate, s.len()), (44_100, 4_410));
        assert!(!l.is_loading(ChannelId(3)));
        assert!(l.poll().is_none());
    }

    #[test]
    fn stale_results_are_dropped() {
        let dir = tmp_dir("stale");
        let a = write_wav(&dir.join("a.wav"), 1_000);
        let b = write_wav(&dir.join("b.wav"), 2_000);
        let mut l = SampleLoader::new(2);
        // Source changed before the first load finished: only the second one is delivered.
        l.request(ChannelId(0), a.clone(), 22_050);
        let id = l.request(ChannelId(0), b.clone(), 22_050);
        let r = l.wait(T).unwrap();
        assert_eq!((r.id, r.source), (id, b));
        assert_eq!(r.result.unwrap().len(), 2_000);
        // Channel deleted while loading: nothing is delivered.
        l.request(ChannelId(1), a, 22_050);
        l.cancel(ChannelId(1));
        assert!(!l.is_loading(ChannelId(1)));
        assert!(l.wait(Duration::from_millis(300)).is_none());
    }

    #[test]
    fn errors_are_reported() {
        let dir = tmp_dir("errors");
        let bad = dir.join("bad.wav");
        std::fs::write(&bad, b"not a wav").unwrap();
        let mut l = SampleLoader::new(1);
        l.request(ChannelId(0), SampleSource::File(dir.join("missing.wav")), 48_000);
        assert!(l.wait(T).unwrap().result.is_err());
        l.request(ChannelId(1), SampleSource::File(bad.clone()), 48_000);
        assert!(matches!(l.wait(T).unwrap().result, Err(crate::IoError::Wav(_))));
        // Errors are not cached: the fixed file loads on the next request.
        write_wav(&bad, 10);
        l.request(ChannelId(1), SampleSource::File(bad), 22_050);
        assert_eq!(l.wait(T).unwrap().result.unwrap().len(), 10);
    }

    #[test]
    fn same_file_is_decoded_once() {
        let src = write_wav(&tmp_dir("once").join("a.wav"), 200_000);
        let mut l = SampleLoader::new(4);
        for c in 0..6 {
            l.request(ChannelId(c), src.clone(), 48_000);
        }
        let mut got = Vec::new();
        while l.loading().next().is_some() {
            got.push(l.wait(T).unwrap().result.unwrap());
        }
        assert_eq!(got.len(), 6);
        assert!(got.iter().all(|s| Arc::ptr_eq(s, &got[0])));
        assert_eq!(decodes(&l), 1);
        // Synchronous loads (export) share the cache and wait for loads in progress.
        l.request(ChannelId(9), src.clone(), 44_100);
        let sync = l.cache().get(&src, 44_100).unwrap();
        assert!(Arc::ptr_eq(&sync, &l.wait(T).unwrap().result.unwrap()));
        assert_eq!(decodes(&l), 2);
    }

    #[test]
    fn cache_is_hit_on_reload_until_invalidated() {
        let src = write_wav(&tmp_dir("reload").join("a.wav"), 500);
        let mut l = SampleLoader::new(1);
        l.request(ChannelId(0), src.clone(), 22_050);
        let first = l.wait(T).unwrap().result.unwrap();
        assert!(Arc::ptr_eq(&first, &l.cache().peek(&src, 22_050).unwrap()));
        l.request(ChannelId(0), src.clone(), 22_050);
        assert!(Arc::ptr_eq(&first, &l.wait(T).unwrap().result.unwrap()));
        assert_eq!(decodes(&l), 1);
        // Another rate is another entry; invalidating forgets the file at every rate.
        assert!(l.cache().peek(&src, 48_000).is_none());
        l.cache().invalidate(&src);
        assert!(l.cache().peek(&src, 22_050).is_none());
        l.request(ChannelId(0), src, 22_050);
        assert!(!Arc::ptr_eq(&first, &l.wait(T).unwrap().result.unwrap()));
        assert_eq!(decodes(&l), 2);
    }
}
