//! Synthesized drum sounds (default kits, no files required).
//! Generated off the audio thread, directly at the device sample rate.

use crate::sample::SampleData;
use seq_model::Drum;
use std::f32::consts::TAU;

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        // xorshift32
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole high-pass / low-pass filter.
struct OnePole {
    a: f32,
    z: f32,
}
impl OnePole {
    fn new(fc: f32, sr: f32) -> Self {
        OnePole { a: (-TAU * fc / sr).exp(), z: 0.0 }
    }
    fn lp(&mut self, x: f32) -> f32 {
        self.z = x * (1.0 - self.a) + self.z * self.a;
        self.z
    }
    fn hp(&mut self, x: f32) -> f32 {
        x - self.lp(x)
    }
}

fn env(t: f32, decay: f32) -> f32 {
    (-t / decay).exp()
}

fn kick(sr: f32, f0: f32, f1: f32, pitch_decay: f32, amp_decay: f32, drive: f32, click: f32) -> Vec<f32> {
    let n = (sr * amp_decay * 7.0) as usize;
    let mut phase = 0.0f32;
    let mut noise = Noise(0x1234_5678);
    let mut hp = OnePole::new(3000.0, sr);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let f = f1 + (f0 - f1) * env(t, pitch_decay);
            phase = (phase + f / sr) % 1.0;
            let body = (phase * TAU).sin() * env(t, amp_decay);
            let c = hp.hp(noise.next()) * env(t, 0.003) * click;
            ((body + c) * drive).tanh()
        })
        .collect()
}

fn snare(sr: f32, tone: f32, tone_decay: f32, noise_decay: f32, noise_amt: f32) -> Vec<f32> {
    let n = (sr * noise_decay * 7.0) as usize;
    let mut noise = Noise(0x9e37_79b9);
    let mut hp = OnePole::new(1200.0, sr);
    let mut lp = OnePole::new(9000.0, sr);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let body = ((t * tone * TAU).sin() + 0.5 * (t * tone * 1.8 * TAU).sin()) * env(t, tone_decay);
            let nz = lp.lp(hp.hp(noise.next())) * env(t, noise_decay) * noise_amt;
            body * 0.6 + nz
        })
        .collect()
}

fn clap(sr: f32) -> Vec<f32> {
    let n = (sr * 0.5) as usize;
    let mut noise = Noise(0xdead_beef);
    let mut hp = OnePole::new(900.0, sr);
    let mut lp = OnePole::new(2500.0, sr);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let bursts = [0.0f32, 0.011, 0.022];
            let mut e = 0.0f32;
            for b in bursts {
                if t >= b {
                    e = e.max(env(t - b, 0.006));
                }
            }
            if t >= 0.03 {
                e = e.max(env(t - 0.03, 0.11) * 0.8);
            }
            lp.lp(hp.hp(noise.next())) * e * 2.5
        })
        .collect()
}

fn hat(sr: f32, decay: f32) -> Vec<f32> {
    // Six "metallic" squares (808 frequencies) + noise, high-pass filtered.
    let freqs = [205.3f32, 304.4, 369.6, 522.7, 540.0, 800.0];
    let n = (sr * decay * 7.0) as usize;
    let mut noise = Noise(0x0bad_cafe);
    let mut hp1 = OnePole::new(7000.0, sr);
    let mut hp2 = OnePole::new(7000.0, sr);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let metal: f32 = freqs
                .iter()
                .map(|f| if (t * f * 1.7).fract() < 0.5 { 1.0 } else { -1.0 })
                .sum::<f32>()
                / 6.0;
            let x = metal * 0.6 + noise.next() * 0.4;
            hp2.hp(hp1.hp(x)) * env(t, decay) * 1.8
        })
        .collect()
}

fn tone(sr: f32, f0: f32, f1: f32, decay: f32, noise_amt: f32) -> Vec<f32> {
    let n = (sr * decay * 7.0) as usize;
    let mut phase = 0.0f32;
    let mut noise = Noise(0x5eed_1234);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let f = f1 + (f0 - f1) * env(t, 0.04);
            phase = (phase + f / sr) % 1.0;
            ((phase * TAU).sin() + noise.next() * noise_amt * env(t, 0.01)) * env(t, decay)
        })
        .collect()
}

fn cowbell(sr: f32) -> Vec<f32> {
    let n = (sr * 0.5) as usize;
    let mut bp_hp = OnePole::new(400.0, sr);
    let mut bp_lp = OnePole::new(3000.0, sr);
    (0..n)
        .map(|i| {
            let t = i as f32 / sr;
            let sq = |f: f32| if (t * f).fract() < 0.5 { 1.0 } else { -1.0 };
            let x = (sq(540.0) + sq(800.0)) * 0.5;
            let e = 0.6 * env(t, 0.02) + 0.4 * env(t, 0.12);
            bp_lp.lp(bp_hp.hp(x)) * e * 2.0
        })
        .collect()
}

fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if peak > 0.0 {
        let g = 0.9 / peak;
        v.iter_mut().for_each(|x| *x *= g);
    }
    // Short fade-out to avoid a click at the end of the sample.
    let fade = 64.min(v.len());
    let n = v.len();
    for i in 0..fade {
        v[n - 1 - i] *= i as f32 / fade as f32;
    }
    v
}

pub fn render(drum: Drum, sample_rate: u32) -> SampleData {
    let sr = sample_rate as f32;
    let mono = match drum {
        Drum::Kick => kick(sr, 160.0, 48.0, 0.03, 0.18, 1.5, 0.5),
        Drum::Kick808 => kick(sr, 120.0, 45.0, 0.06, 0.5, 1.2, 0.2),
        Drum::KickHard => kick(sr, 220.0, 52.0, 0.02, 0.14, 4.0, 1.0),
        Drum::Snare => snare(sr, 185.0, 0.05, 0.12, 0.9),
        Drum::SnareTight => snare(sr, 240.0, 0.03, 0.07, 1.1),
        Drum::Clap => clap(sr),
        Drum::HatClosed => hat(sr, 0.025),
        Drum::HatOpen => hat(sr, 0.18),
        Drum::Rim => tone(sr, 1700.0, 800.0, 0.012, 0.8),
        Drum::TomLow => tone(sr, 160.0, 95.0, 0.18, 0.3),
        Drum::TomHigh => tone(sr, 260.0, 165.0, 0.14, 0.3),
        Drum::Cowbell => cowbell(sr),
    };
    SampleData::from_mono(sample_rate, normalize(mono))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_drums_render() {
        for d in Drum::ALL {
            let s = render(d, 48_000);
            assert!(s.len() > 100, "{d:?}");
            let peak = s.left.iter().fold(0.0f32, |m, x| m.max(x.abs()));
            assert!(peak > 0.5 && peak <= 0.91, "{d:?} peak {peak}");
            assert!(s.left.iter().all(|x| x.is_finite()));
        }
    }
}
