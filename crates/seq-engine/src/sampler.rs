//! One-shot sampler: pre-allocated voices, pitch from the key (60 = original pitch).

use crate::instrument::Instrument;
use crate::sample::SampleData;
use std::sync::Arc;

const VOICES: usize = 16;
/// Fade duration on choke / all_off (seconds).
const FADE_SECS: f32 = 0.004;

#[derive(Clone, Copy, Default)]
struct Voice {
    active: bool,
    pos: f64,
    rate: f64,
    gain: f32,
    /// 1.0 = normal; < 1.0 = fading out.
    fade: f32,
    fading: bool,
    age: u64,
}

pub struct Sampler {
    sample: Arc<SampleData>,
    voices: [Voice; VOICES],
    fade_step: f32,
    counter: u64,
}

impl Sampler {
    /// `sample` must already be at the device sample rate.
    pub fn new(sample: Arc<SampleData>) -> Self {
        let sr = sample.sample_rate.max(1) as f32;
        Sampler { sample, voices: [Voice::default(); VOICES], fade_step: 1.0 / (FADE_SECS * sr), counter: 0 }
    }
}

impl Instrument for Sampler {
    fn note_on(&mut self, key: u8, vel: u8) {
        self.counter += 1;
        let idx = match self.voices.iter().position(|v| !v.active) {
            Some(i) => i,
            // Voice stealing: the oldest one.
            None => self
                .voices
                .iter()
                .enumerate()
                .min_by_key(|(_, v)| v.age)
                .map(|(i, _)| i)
                .unwrap_or(0),
        };
        self.voices[idx] = Voice {
            active: true,
            pos: 0.0,
            rate: 2f64.powf((key as f64 - 60.0) / 12.0),
            gain: (vel as f32 / 127.0).powf(1.5),
            fade: 1.0,
            fading: false,
            age: self.counter,
        };
    }

    fn note_off(&mut self, _key: u8) {
        // One-shot: the sample plays to the end.
    }

    fn all_off(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.fading = true;
        }
    }

    fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        out_l.fill(0.0);
        out_r.fill(0.0);
        let s = &*self.sample;
        let len = s.len();
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for (l, r) in out_l.iter_mut().zip(out_r.iter_mut()) {
                let i = v.pos as usize;
                if i + 1 >= len || v.fade <= 0.0 {
                    v.active = false;
                    break;
                }
                let t = (v.pos - i as f64) as f32;
                let g = v.gain * v.fade;
                *l += (s.left[i] + (s.left[i + 1] - s.left[i]) * t) * g;
                *r += (s.right[i] + (s.right[i + 1] - s.right[i]) * t) * g;
                v.pos += v.rate;
                if v.fading {
                    v.fade -= self.fade_step;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampler(mono: Vec<f32>) -> Sampler {
        Sampler::new(Arc::new(SampleData::from_mono(48_000, mono)))
    }

    fn render(s: &mut Sampler, n: usize) -> Vec<f32> {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.process(&mut l, &mut r);
        assert_eq!(l, r, "mono sample: both sides identical");
        l
    }

    fn sounding_frames(key: u8) -> usize {
        // DC sample of 1001 frames: at rate 1 the last frame is only used for interpolation.
        let mut s = sampler(vec![1.0; 1001]);
        s.note_on(key, 127);
        render(&mut s, 4000).iter().filter(|x| **x != 0.0).count()
    }

    #[test]
    fn pitch_follows_key() {
        assert_eq!(sounding_frames(60), 1000);
        // One octave up: double speed, half the length.
        assert_eq!(sounding_frames(72), 500);
        // One octave down: half speed, double the length.
        assert_eq!(sounding_frames(48), 2000);
        // Fifth up: rate 2^(7/12).
        let expected = (1000.0 / 2f64.powf(7.0 / 12.0)).ceil() as usize;
        assert_eq!(sounding_frames(67), expected);
    }

    #[test]
    fn pitch_reads_sample_at_rate() {
        // Ramp sample: the output value is the read position.
        let mut s = sampler((0..1000).map(|i| i as f32).collect());
        s.note_on(72, 127);
        let out = render(&mut s, 10);
        assert_eq!(out, (0..10).map(|i| 2.0 * i as f32).collect::<Vec<_>>());
    }

    #[test]
    fn polyphony_limit_steals_oldest_voice() {
        // Ramp sample: each voice outputs its own position / 1000.
        let mut s = sampler((0..10_000).map(|i| i as f32 / 1000.0).collect());
        s.note_on(60, 127);
        render(&mut s, 100); // oldest voice now at position 100
        for _ in 0..15 {
            s.note_on(60, 127);
        }
        // 16 voices: the 17th steals the oldest (the one at position 100).
        s.note_on(60, 127);
        let out = render(&mut s, 2);
        assert_eq!(out[0], 0.0, "oldest voice must be the one stolen");
        // Exactly 16 voices at position 1.
        assert!((out[1] - 16.0 * 0.001).abs() < 1e-6, "got {}", out[1]);
    }

    #[test]
    fn note_off_does_not_cut_one_shot() {
        let mut s = sampler(vec![1.0; 1001]);
        s.note_on(60, 64);
        let gain = (64.0f32 / 127.0).powf(1.5);
        let a = render(&mut s, 100);
        s.note_off(60);
        let b = render(&mut s, 100);
        assert!(a.iter().chain(&b).all(|x| (x - gain).abs() < 1e-6));
    }

    #[test]
    fn all_off_fades_out_quickly() {
        let mut s = sampler(vec![1.0; 48_000]);
        s.note_on(60, 127);
        render(&mut s, 10);
        s.all_off();
        let out = render(&mut s, 400);
        // A short fade (4 ms = 192 frames), not a hard cut.
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[96] > 0.4 && out[96] < 0.6, "half way: {}", out[96]);
        assert!(out.windows(2).all(|w| w[1] <= w[0]));
        let last = out.iter().rposition(|x| *x != 0.0).unwrap();
        assert!((190..=194).contains(&last), "silent after {last} frames");
        // The voice is freed: a new note plays at full level.
        s.note_on(60, 127);
        assert!((render(&mut s, 1)[0] - 1.0).abs() < 1e-6);
    }
}
