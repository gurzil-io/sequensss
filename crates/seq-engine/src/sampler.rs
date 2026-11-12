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
