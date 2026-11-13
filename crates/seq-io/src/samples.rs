//! WAV sample loading (hound). v1: WAV only. Decoding runs off the UI thread (see `loader`).

use crate::Result;
use seq_engine::SampleData;
use std::path::Path;

/// Checks that the file exists and has a valid WAV header, without decoding it (fast).
pub fn probe_wav(path: &Path) -> Result<()> {
    hound::WavReader::open(path)?;
    Ok(())
}

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

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sequensss-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_int(path: &Path, channels: u16, bits: u16, samples: &[i32]) {
        let spec = hound::WavSpec { channels, sample_rate: 44_100, bits_per_sample: bits, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for &s in samples {
            if bits == 8 {
                w.write_sample(s as i8).unwrap();
            } else {
                w.write_sample(s).unwrap();
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn wav_8bit_scaling() {
        let path = tmp_dir("wav8").join("t.wav");
        write_int(&path, 1, 8, &[-128, -64, 0, 64, 127]);
        let s = load_wav(&path).unwrap();
        assert_eq!(s.left, vec![-1.0, -0.5, 0.0, 0.5, 127.0 / 128.0]);
        assert_eq!(s.left, s.right);
    }

    #[test]
    fn wav_24bit_stereo_scaling() {
        let path = tmp_dir("wav24").join("t.wav");
        let full = 1 << 23;
        // Frames: (min, max), (half, -half), (0, 1).
        write_int(&path, 2, 24, &[-full, full - 1, full / 2, -full / 2, 0, 1]);
        let s = load_wav(&path).unwrap();
        assert_eq!(s.sample_rate, 44_100);
        assert_eq!(s.len(), 3);
        assert_eq!(s.left, vec![-1.0, 0.5, 0.0]);
        assert_eq!(s.right[1], -0.5);
        assert!((s.right[0] - 1.0).abs() < 1e-6);
        assert!((s.right[2] - 1.0 / full as f32).abs() < 1e-12);
    }

    #[test]
    fn wav_32bit_int_scaling() {
        let path = tmp_dir("wav32i").join("t.wav");
        write_int(&path, 1, 32, &[i32::MIN, i32::MIN / 4, 0, i32::MAX]);
        let s = load_wav(&path).unwrap();
        assert_eq!(s.left[0], -1.0);
        assert_eq!(s.left[1], -0.25);
        assert_eq!(s.left[2], 0.0);
        assert!((s.left[3] - 1.0).abs() < 1e-6);
        assert!(s.left.iter().all(|x| (-1.0..=1.0).contains(x)));
    }

    #[test]
    fn wav_float_stereo_and_mono() {
        let dir = tmp_dir("wavf");
        let stereo = dir.join("s.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 96_000, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
        let mut w = hound::WavWriter::create(&stereo, spec).unwrap();
        for x in [0.25f32, -0.75, 1.0, -1.0, 0.0, 0.5] {
            w.write_sample(x).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav(&stereo).unwrap();
        assert_eq!(s.sample_rate, 96_000);
        assert_eq!(s.left, vec![0.25, 1.0, 0.0]);
        assert_eq!(s.right, vec![-0.75, -1.0, 0.5]);

        let mono = dir.join("m.wav");
        let mut w = hound::WavWriter::create(&mono, hound::WavSpec { channels: 1, ..spec }).unwrap();
        for x in [0.1f32, -0.2, 0.3] {
            w.write_sample(x).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav(&mono).unwrap();
        assert_eq!(s.left, vec![0.1, -0.2, 0.3]);
        assert_eq!(s.left, s.right);
    }

    #[test]
    fn wav_more_than_two_channels_keeps_first_two() {
        let path = tmp_dir("wav4ch").join("t.wav");
        let spec = hound::WavSpec { channels: 4, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for x in [16384i16, -16384, 1000, 2000, 8192, -8192, 3000, 4000] {
            w.write_sample(x).unwrap();
        }
        w.finalize().unwrap();
        let s = load_wav(&path).unwrap();
        assert_eq!(s.left, vec![0.5, 0.25]);
        assert_eq!(s.right, vec![-0.5, -0.25]);
    }

    #[test]
    fn invalid_wav_files_are_errors() {
        let dir = tmp_dir("wavbad");
        let empty = dir.join("empty.wav");
        std::fs::write(&empty, b"").unwrap();
        assert!(matches!(load_wav(&empty), Err(crate::IoError::Wav(_))));

        let garbage = dir.join("garbage.wav");
        std::fs::write(&garbage, b"RIFF\x10\x00\x00\x00WAVEnot really a wav file at all").unwrap();
        assert!(load_wav(&garbage).is_err());

        let text = dir.join("text.wav");
        std::fs::write(&text, "hello, this is plain text and not audio").unwrap();
        assert!(load_wav(&text).is_err());

        assert!(load_wav(&dir.join("missing.wav")).is_err());
    }

    #[test]
    fn truncated_wav_does_not_panic() {
        let dir = tmp_dir("wavtrunc");
        let path = dir.join("t.wav");
        let spec = hound::WavSpec { channels: 2, sample_rate: 44_100, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut w = hound::WavWriter::create(&path, spec).unwrap();
        for i in 0..1000 {
            w.write_sample(i as i16).unwrap();
        }
        w.finalize().unwrap();
        // Cut the data chunk in the middle of a frame: the header still announces 1000 samples.
        let bytes = std::fs::read(&path).unwrap();
        let cut = dir.join("cut.wav");
        std::fs::write(&cut, &bytes[..bytes.len() - 1001]).unwrap();
        // Either an error or fewer frames, never a panic or a misaligned stereo split.
        if let Ok(s) = load_wav(&cut) {
            assert!(s.len() < 500);
            assert_eq!(s.left.len(), s.right.len());
            assert!(s.left.iter().zip(&s.right).all(|(l, r)| (*r * 32768.0) as i32 == (*l * 32768.0) as i32 + 1));
        }
    }
}
