/// Decoded stereo sample, at the device sample rate.
#[derive(Debug, Clone, PartialEq)]
pub struct SampleData {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

impl SampleData {
    pub fn len(&self) -> usize {
        self.left.len()
    }

    pub fn is_empty(&self) -> bool {
        self.left.is_empty()
    }

    pub fn from_mono(sample_rate: u32, mono: Vec<f32>) -> Self {
        SampleData { sample_rate, right: mono.clone(), left: mono }
    }

    /// Resampling at load time (never in real time). Cubic (Hermite) interpolation:
    /// good enough for drum one-shots in v1.
    pub fn resampled(&self, target_rate: u32) -> SampleData {
        if target_rate == self.sample_rate || self.is_empty() {
            return SampleData { sample_rate: target_rate, ..self.clone() };
        }
        let ratio = self.sample_rate as f64 / target_rate as f64;
        let out_len = ((self.len() as f64) / ratio).floor().max(1.0) as usize;
        let conv = |src: &[f32]| -> Vec<f32> {
            (0..out_len).map(|i| hermite(src, i as f64 * ratio)).collect()
        };
        SampleData { sample_rate: target_rate, left: conv(&self.left), right: conv(&self.right) }
    }
}

/// 4-point Hermite interpolation at position `pos` (0 out of bounds).
pub fn hermite(src: &[f32], pos: f64) -> f32 {
    let i = pos.floor() as isize;
    let t = (pos - i as f64) as f32;
    let at = |k: isize| -> f32 {
        if k < 0 || k as usize >= src.len() { 0.0 } else { src[k as usize] }
    };
    let (xm1, x0, x1, x2) = (at(i - 1), at(i), at(i + 1), at(i + 2));
    let c0 = x0;
    let c1 = 0.5 * (x1 - xm1);
    let c2 = xm1 - 2.5 * x0 + 2.0 * x1 - 0.5 * x2;
    let c3 = 0.5 * (x2 - xm1) + 1.5 * (x0 - x1);
    ((c3 * t + c2) * t + c1) * t + c0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_length_and_shape() {
        let sr = 44_100;
        let mono: Vec<f32> = (0..44_100).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / sr as f32).sin()).collect();
        let s = SampleData::from_mono(sr, mono).resampled(48_000);
        assert_eq!(s.sample_rate, 48_000);
        assert!((s.len() as i64 - 48_000).abs() <= 1);
        // Still a 440 Hz sine: check one point.
        let i = 12_345;
        let expected = (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin();
        assert!((s.left[i] - expected).abs() < 0.01);
    }
}
