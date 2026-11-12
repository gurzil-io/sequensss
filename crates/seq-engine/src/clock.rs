//! Tick ↔ sample conversion.
//!
//! Rounding rule: an event at tick `t` (relative to the playback origin) is played
//! at frame `ceil(t * samples_per_tick)`, i.e. the first frame whose position
//! is >= t.

use seq_model::PPQ;

pub fn samples_per_tick(sample_rate: f64, tempo_bpm: f64) -> f64 {
    sample_rate * 60.0 / (tempo_bpm.max(1.0) * PPQ as f64)
}

/// Frame (from the origin) on which tick `tick` (relative to the origin) lands.
pub fn tick_to_frame(tick: f64, spt: f64) -> u64 {
    (tick * spt).ceil().max(0.0) as u64
}

/// Position in ticks of frame `frame` (from the origin).
pub fn frame_to_tick(frame: u64, spt: f64) -> f64 {
    frame as f64 / spt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spt_120bpm() {
        assert_eq!(samples_per_tick(48_000.0, 120.0), 25.0);
        assert_eq!(samples_per_tick(44_100.0, 120.0), 22.96875);
    }

    #[test]
    fn beats_land_on_exact_frames() {
        for &(sr, expected_beat) in &[(48_000.0, 24_000u64), (44_100.0, 22_050u64)] {
            let spt = samples_per_tick(sr, 120.0);
            for beat in 0..1200u64 {
                assert_eq!(tick_to_frame((beat * PPQ as u64) as f64, spt), beat * expected_beat);
            }
        }
    }

    #[test]
    fn no_drift_over_10_minutes() {
        // 10 min at 120 BPM = 1200 beats. Beat n must land at n * 0.5 s.
        for &sr in &[44_100.0f64, 48_000.0, 96_000.0] {
            let spt = samples_per_tick(sr, 120.0);
            let frame = tick_to_frame(1200.0 * PPQ as f64, spt);
            assert_eq!(frame, (600.0 * sr) as u64);
        }
        // Non-"round" tempo: the error stays < 1 sample.
        let sr = 44_100.0;
        let spt = samples_per_tick(sr, 133.0);
        let beats = 133.0 * 10.0;
        let frame = tick_to_frame(beats * PPQ as f64, spt) as f64;
        assert!((frame - 600.0 * sr).abs() <= 1.0);
    }
}
