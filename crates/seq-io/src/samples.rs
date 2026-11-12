//! WAV sample loading (hound). v1: WAV only, synchronous loading.

use crate::Result;
use seq_engine::SampleData;
use std::path::Path;

pub fn load_wav(path: &Path) -> Result<SampleData> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let ch = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<std::result::Result<_, _>>()?
        }
    };
    let frames = interleaved.len() / ch;
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    for f in 0..frames {
        let l = interleaved[f * ch];
        let r = if ch > 1 { interleaved[f * ch + 1] } else { l };
        left.push(l);
        right.push(r);
    }
    Ok(SampleData { sample_rate: spec.sample_rate, left, right })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_round_trip_16bit_mono() {
        let dir = std::env::temp_dir().join("sequensss-test-wav");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let spec = hound::WavSpec { channels: 1, sample_rate: 22_050, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..1000 {
            w.write_sample(((i as f32 / 1000.0) * 16000.0) as i16).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav(&path).unwrap();
        assert_eq!(s.sample_rate, 22_050);
        assert_eq!(s.len(), 1000);
        assert_eq!(s.left, s.right);
        assert!((s.left[500] - 0.5 * 16000.0 / 32768.0).abs() < 1e-3);
    }
}
