//! Subtractive synth: 2 oscillators (polyBLEP), SVF low-pass filter, ADSR, 8 voices.

use crate::instrument::Instrument;
use seq_model::{SynthParams, Wave};

const VOICES: usize = 8;

#[derive(Clone, Copy, PartialEq, Default)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Default)]
struct Voice {
    stage: Stage,
    key: u8,
    vel: f32,
    phase1: f32,
    phase2: f32,
    inc1: f32,
    inc2: f32,
    env: f32,
    ic1: f32,
    ic2: f32,
    age: u64,
    /// Fast fade for voice stealing / all_off.
    kill: bool,
}

pub struct Synth {
    params: SynthParams,
    sr: f32,
    voices: [Voice; VOICES],
    counter: u64,
}

impl Synth {
    pub fn new(params: SynthParams, sample_rate: u32) -> Self {
        Synth { params, sr: sample_rate.max(1) as f32, voices: [Voice::default(); VOICES], counter: 0 }
    }

    fn freq(key: u8) -> f32 {
        440.0 * 2f32.powf((key as f32 - 69.0) / 12.0)
    }
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

fn osc(wave: Wave, phase: f32, dt: f32) -> f32 {
    match wave {
        Wave::Sine => (phase * std::f32::consts::TAU).sin(),
        Wave::Saw => 2.0 * phase - 1.0 - poly_blep(phase, dt),
        Wave::Square => {
            let mut v = if phase < 0.5 { 1.0 } else { -1.0 };
            v += poly_blep(phase, dt);
            v -= poly_blep((phase + 0.5) % 1.0, dt);
            v
        }
        Wave::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
    }
}

impl Instrument for Synth {
    fn note_on(&mut self, key: u8, vel: u8) {
        self.counter += 1;
        let idx = self
            .voices
            .iter()
            .position(|v| v.stage == Stage::Idle)
            .or_else(|| {
                // Voice stealing: first a voice in release (the quietest), otherwise the oldest.
                self.voices
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v.stage == Stage::Release)
                    .min_by(|a, b| a.1.env.total_cmp(&b.1.env))
                    .map(|(i, _)| i)
            })
            .unwrap_or_else(|| {
                self.voices.iter().enumerate().min_by_key(|(_, v)| v.age).map(|(i, _)| i).unwrap_or(0)
            });
        let f = Self::freq(key);
        let f2 = f * 2f32.powf(self.params.osc2_detune / 12.0);
        let v = &mut self.voices[idx];
        let keep_env = if v.stage != Stage::Idle { v.env } else { 0.0 };
        *v = Voice {
            stage: Stage::Attack,
            key,
            vel: vel as f32 / 127.0,
            phase1: 0.0,
            phase2: 0.0,
            inc1: (f / self.sr).min(0.49),
            inc2: (f2 / self.sr).min(0.49),
            // Restart from the current level to avoid a click on voice stealing.
            env: keep_env,
            ic1: 0.0,
            ic2: 0.0,
            age: self.counter,
            kill: false,
        };
    }

    fn note_off(&mut self, key: u8) {
        for v in self.voices.iter_mut() {
            if v.key == key && matches!(v.stage, Stage::Attack | Stage::Decay | Stage::Sustain) {
                v.stage = Stage::Release;
            }
        }
    }

    fn all_off(&mut self) {
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            v.stage = Stage::Release;
            v.kill = true;
        }
    }

    fn set_synth_params(&mut self, params: &SynthParams) {
        self.params = *params;
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            let f2 = Self::freq(v.key) * 2f32.powf(params.osc2_detune / 12.0);
            v.inc2 = (f2 / self.sr).min(0.49);
        }
    }

    fn process(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        out_l.fill(0.0);
        let p = self.params;
        let sr = self.sr;
        let atk = 1.0 / (p.attack.max(0.0005) * sr);
        let dec = 1.0 / (p.decay.max(0.0005) * sr);
        let rel = 1.0 / (p.release.max(0.0005) * sr);
        let kill_rate = 1.0 / (0.003 * sr);
        let sustain = p.sustain.clamp(0.0, 1.0);
        let k = 2.0 - 1.95 * p.resonance.clamp(0.0, 1.0);
        let mix = p.osc_mix.clamp(0.0, 1.0);
        for v in self.voices.iter_mut().filter(|v| v.stage != Stage::Idle) {
            for out in out_l.iter_mut() {
                // Envelope
                match v.stage {
                    Stage::Attack => {
                        v.env += atk;
                        if v.env >= 1.0 {
                            v.env = 1.0;
                            v.stage = Stage::Decay;
                        }
                    }
                    Stage::Decay => {
                        v.env -= dec * (1.0 - sustain).max(0.001);
                        if v.env <= sustain {
                            v.env = sustain;
                            v.stage = Stage::Sustain;
                        }
                    }
                    Stage::Sustain => v.env = sustain,
                    Stage::Release => {
                        v.env -= if v.kill { kill_rate } else { rel * v.env.max(0.05) };
                        if v.env <= 0.0005 {
                            v.env = 0.0;
                            v.stage = Stage::Idle;
                            break;
                        }
                    }
                    Stage::Idle => break,
                }
                // Oscillators
                let s1 = osc(p.osc1, v.phase1, v.inc1);
                let s2 = osc(p.osc2, v.phase2, v.inc2);
                v.phase1 = (v.phase1 + v.inc1) % 1.0;
                v.phase2 = (v.phase2 + v.inc2) % 1.0;
                let x = s1 * (1.0 - mix) + s2 * mix;
                // SVF filter (Simper/Cytomic), cutoff modulated by the envelope.
                let fc = (p.cutoff * 2f32.powf(4.0 * p.filter_env.clamp(0.0, 1.0) * v.env)).clamp(20.0, sr * 0.45);
                let g = (std::f32::consts::PI * fc / sr).tan();
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let v3 = x - v.ic2;
                let v1 = a1 * v.ic1 + a2 * v3;
                let v2 = v.ic2 + a2 * v.ic1 + a3 * v3;
                v.ic1 = 2.0 * v1 - v.ic1;
                v.ic2 = 2.0 * v2 - v.ic2;
                *out += v2 * v.env * v.vel * p.gain;
            }
        }
        out_r.copy_from_slice(out_l);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_produces_sound_then_releases() {
        let mut s = Synth::new(SynthParams::default(), 48_000);
        let mut l = vec![0.0; 4800];
        let mut r = vec![0.0; 4800];
        s.note_on(48, 100);
        s.process(&mut l, &mut r);
        assert!(l.iter().any(|x| x.abs() > 0.05));
        assert!(l.iter().all(|x| x.is_finite()));
        s.note_off(48);
        for _ in 0..20 {
            s.process(&mut l, &mut r);
        }
        assert!(l.iter().all(|x| x.abs() < 1e-6));
    }
}
