//! Live recording: MIDI in note-on/off events → pattern notes.

use seq_model::*;
use std::collections::HashMap;

/// Pairs each recorded note-on with its note-off (per channel and key).
#[derive(Debug, Default)]
pub struct Recorder {
    /// (channel, key) → (position, time, velocity) of the note-on.
    pending: HashMap<(ChannelId, u8), (f64, f64, u8)>,
}

/// A note-on matched with its note-off. `start` is the position within the pattern, as
/// sent by the engine (it wraps at the end of the loop); `len` is how long the key was
/// held, in ticks (it does not wrap).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedNote {
    pub channel: ChannelId,
    pub key: u8,
    pub vel: u8,
    pub start: f64,
    pub len: f64,
}

impl Recorder {
    /// Forgets the held notes (stop, mode change, recording toggled).
    pub fn clear(&mut self) {
        self.pending.clear();
    }

    /// `tick`: position within the pattern; `time`: monotonic time in ticks.
    /// Note-on: remembered. If the same key was already held, the engine cut that note,
    /// so it is returned, ending here. Note-off: returns the matching note-on, or `None`.
    pub fn event(&mut self, channel: ChannelId, key: u8, vel: u8, tick: f64, time: f64, on: bool) -> Option<RecordedNote> {
        let held = if on {
            self.pending.insert((channel, key), (tick, time, vel))
        } else {
            self.pending.remove(&(channel, key))
        };
        let (start, start_time, vel) = held?;
        Some(RecordedNote { channel, key, vel, start, len: time - start_time })
    }
}

impl RecordedNote {
    /// The note to add to a pattern of length `pattern_len`. `quantize`: start and length
    /// rounded to the step (at least one step). The length is kept within
    /// `STEP / 4..=pattern_len` (or the whole pattern if it is shorter than `STEP / 4`).
    pub fn to_note(&self, pattern_len: Tick, quantize: bool) -> Note {
        let plen = pattern_len.max(1);
        let raw_start = self.start.max(0.0) as Tick % plen;
        let mut len = self.len.max(0.0) as Tick;
        let mut s = raw_start;
        if quantize {
            s = crate::snap_round(raw_start, STEP) % plen;
            len = crate::snap_round(len, STEP).max(STEP);
        }
        len = len.clamp((STEP / 4).min(plen), plen);
        Note { start: s, len, key: self.key, velocity: self.vel.max(1) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: Tick = 16 * STEP;
    const A: ChannelId = ChannelId(1);
    const B: ChannelId = ChannelId(2);

    /// Note-on at position `start`, released `held` ticks later, then converted.
    fn rec(start: f64, held: f64, len: Tick, quantize: bool) -> Note {
        let mut r = Recorder::default();
        assert_eq!(r.event(A, 60, 100, start, 1000.0, true), None);
        r.event(A, 60, 0, 0.0, 1000.0 + held, false).unwrap().to_note(len, quantize)
    }

    fn note(start: Tick, len: Tick) -> Note {
        Note { start, len, key: 60, velocity: 100 }
    }

    #[test]
    fn simple_note() {
        assert_eq!(rec(480.0, 960.0, BAR, false), note(480, 960));
        // Sub-tick positions are truncated.
        assert_eq!(rec(480.9, 960.0, BAR, false), note(480, 960));
    }

    #[test]
    fn note_off_returns_matching_note_on() {
        let mut r = Recorder::default();
        r.event(A, 60, 90, 10.0, 10.0, true);
        let n = r.event(A, 60, 64, 20.0, 20.0, false).unwrap();
        // The release velocity is ignored.
        assert_eq!(n, RecordedNote { channel: A, key: 60, vel: 90, start: 10.0, len: 10.0 });
        // Consumed.
        assert_eq!(r.event(A, 60, 0, 30.0, 30.0, false), None);
    }

    #[test]
    fn note_crossing_loop_end_keeps_its_length() {
        // Starts 1 step before the end, released 2 steps after the loop restarted.
        assert_eq!(rec((BAR - STEP) as f64, (3 * STEP) as f64, BAR, false), note(BAR - STEP, 3 * STEP));
        // Released exactly at the loop start.
        assert_eq!(rec((BAR - STEP) as f64, STEP as f64, BAR, false), note(BAR - STEP, STEP));
    }

    #[test]
    fn quantize_rounds_start_and_length() {
        // Start 100 → 0 (rounded down), length 300 → 240.
        assert_eq!(rec(100.0, 300.0, BAR, true), note(0, STEP));
        // Start 130 → 240 (rounded up), length 400 → 480.
        assert_eq!(rec(130.0, 400.0, BAR, true), note(STEP, 2 * STEP));
        // Halfway rounds up.
        assert_eq!(rec(120.0, 480.0, BAR, true), note(STEP, 2 * STEP));
        // Without quantize, nothing moves.
        assert_eq!(rec(130.0, 400.0, BAR, false), note(130, 400));
    }

    #[test]
    fn quantize_start_rounded_past_the_end_wraps_to_zero() {
        assert_eq!(rec((BAR - 10) as f64, 210.0, BAR, true), note(0, STEP));
    }

    #[test]
    fn quantize_length_is_at_least_one_step() {
        assert_eq!(rec(0.0, 10.0, BAR, true), note(0, STEP));
        assert_eq!(rec(0.0, 0.0, BAR, true), note(0, STEP));
    }

    #[test]
    fn short_note_clamped_to_quarter_step() {
        assert_eq!(rec(500.0, 10.0, BAR, false), note(500, STEP / 4));
        assert_eq!(rec(500.0, 0.0, BAR, false), note(500, STEP / 4));
    }

    #[test]
    fn length_clamped_to_pattern() {
        // Quantized length rounded up past a pattern that is not a multiple of the step.
        let plen = 2 * STEP + 200;
        assert_eq!(rec(0.0, (2 * STEP + 199) as f64, plen, true), note(0, plen));
        // A note held for almost a whole loop.
        assert_eq!(rec(0.0, (BAR - 1) as f64, BAR, false), note(0, BAR - 1));
    }

    #[test]
    fn note_held_longer_than_pattern_is_clamped() {
        // Held for 1.5 loops: the whole pattern.
        assert_eq!(rec(0.0, (BAR + BAR / 2) as f64, BAR, false), note(0, BAR));
        // Held for exactly 2 loops.
        assert_eq!(rec(0.0, (2 * BAR) as f64, BAR, false), note(0, BAR));
    }

    #[test]
    fn tiny_pattern_does_not_panic() {
        // Pattern lengths are not validated when a project file is loaded.
        assert_eq!(rec(0.0, 10.0, 30, false), note(0, 30));
        assert_eq!(rec(0.0, 10.0, 30, true), note(0, 30));
        // Length 0 is treated as 1 tick.
        assert_eq!(rec(0.0, 10.0, 0, false), note(0, 1));
    }

    #[test]
    fn note_off_without_note_on_is_ignored() {
        let mut r = Recorder::default();
        assert_eq!(r.event(A, 60, 0, 100.0, 100.0, false), None);
        // Another key or channel does not match.
        r.event(A, 60, 100, 0.0, 0.0, true);
        assert_eq!(r.event(A, 61, 0, 100.0, 100.0, false), None);
        assert_eq!(r.event(B, 60, 0, 100.0, 100.0, false), None);
        assert!(r.event(A, 60, 0, 100.0, 100.0, false).is_some());
    }

    #[test]
    fn clear_forgets_held_notes() {
        let mut r = Recorder::default();
        r.event(A, 60, 100, 0.0, 0.0, true);
        r.clear();
        assert_eq!(r.event(A, 60, 0, 100.0, 100.0, false), None);
    }

    #[test]
    fn same_key_twice_records_both_notes() {
        // The engine cuts the first voice when the key is struck again, so the first note
        // ends there.
        let mut r = Recorder::default();
        let mut notes = Vec::new();
        for (tick, vel, on) in [(0.0, 80, true), (480.0, 110, true), (960.0, 0, false)] {
            notes.extend(r.event(A, 60, vel, tick, tick, on).map(|n| n.to_note(BAR, false)));
        }
        let n = |start, len, velocity| Note { start, len, key: 60, velocity };
        assert_eq!(notes, vec![n(0, 480, 80), n(480, 480, 110)]);
        assert_eq!(r.event(A, 60, 0, 1000.0, 1000.0, false), None);
    }

    #[test]
    fn velocity_zero_becomes_one() {
        let mut r = Recorder::default();
        r.event(A, 60, 0, 0.0, 0.0, true);
        assert_eq!(r.event(A, 60, 0, 480.0, 480.0, false).unwrap().to_note(BAR, false).velocity, 1);
    }

    #[test]
    fn out_of_range_values() {
        // Negative position → 0.
        assert_eq!(rec(-50.0, 480.0, BAR, false), note(0, 480));
        // NaN → 0 (f64::max ignores NaN).
        assert_eq!(rec(f64::NAN, 480.0, BAR, false), note(0, 480));
        // Beyond the pattern → modulo.
        assert_eq!(rec((BAR + 480) as f64, 480.0, BAR, false), note(480, 480));
        // A negative or NaN duration → the shortest note.
        assert_eq!(rec(100.0, -1.0, BAR, false), note(100, STEP / 4));
        assert_eq!(rec(100.0, f64::NAN, BAR, false), note(100, STEP / 4));
    }

    #[test]
    fn channels_and_keys_in_parallel() {
        let mut r = Recorder::default();
        r.event(A, 60, 100, 0.0, 0.0, true);
        r.event(B, 60, 90, 240.0, 240.0, true);
        r.event(A, 64, 80, 480.0, 480.0, true);
        let b = r.event(B, 60, 0, 720.0, 720.0, false).unwrap();
        let a64 = r.event(A, 64, 0, 960.0, 960.0, false).unwrap();
        let a60 = r.event(A, 60, 0, 1200.0, 1200.0, false).unwrap();
        assert_eq!((b.channel, b.key, b.to_note(BAR, false)), (B, 60, Note { start: 240, len: 480, key: 60, velocity: 90 }));
        assert_eq!((a64.channel, a64.to_note(BAR, false)), (A, Note { start: 480, len: 480, key: 64, velocity: 80 }));
        assert_eq!((a60.channel, a60.to_note(BAR, false)), (A, Note { start: 0, len: 1200, key: 60, velocity: 100 }));
    }
}
