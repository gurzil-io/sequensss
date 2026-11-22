//! Live recording: MIDI in note-on/off events → pattern notes.
//!
//! Pattern mode: while recording, the engine does not loop the pattern: notes played past
//! its end land there and the pattern grows by whole bars (like FL Studio).
//!
//! Song mode: notes go into the pattern of the clip under the playhead (see
//! [`song_record_target`]), wrapped into it as the clip plays it; patterns do not grow.
//!
//! The app does not commit an undo step during the take, so the notes and the growth undo
//! together.

use seq_model::*;
use std::cmp::Reverse;
use std::collections::HashMap;

/// Pairs each recorded note-on with its note-off (per channel and key).
#[derive(Debug, Default)]
pub struct Recorder {
    /// (channel, key) → (position, time, velocity) of the note-on.
    pending: HashMap<(ChannelId, u8), (f64, f64, u8)>,
}

/// A note-on matched with its note-off. `start` is the position sent by the engine: in the
/// pattern (past its end if it was played there) in Pattern mode, in the song in Song mode;
/// `len` is how long the key was held, in ticks.
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

    /// `tick`: position in the pattern; `time`: monotonic time in ticks.
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
    /// The note, where it was played (it may start past the end of the pattern: see
    /// [`add_recorded_note`]). `grid`: quantization in ticks (0 or 1 = off), start and
    /// length rounded to it (at least one grid unit). It lasts at least `STEP / 4`.
    pub fn to_note(&self, grid: Tick) -> Note {
        let mut start = self.start.max(0.0) as Tick;
        let mut len = self.len.max(0.0) as Tick;
        if grid > 1 {
            start = crate::snap_round(start, grid);
            len = crate::snap_round(len, grid).max(grid);
        }
        Note { start, len: len.max(STEP / 4), key: self.key, velocity: self.vel.max(1), pan: 0 }
    }
}

/// Adds a recorded note to the pattern, which grows by whole bars to hold it.
pub fn add_recorded_note(project: &mut Project, pattern: PatternId, channel: ChannelId, note: Note) {
    let bar = project.bar_len();
    let Some(pat) = project.pattern_mut(pattern) else { return };
    pat.length = crate::length_for_end(pat.length, note.start + note.len, bar);
    pat.notes.entry(channel).or_default().push(note);
}

/// Song mode: the pattern recorded into at the song position `pos`, and the position inside
/// it. The clip of `selected` (the edited pattern) wins if one plays there; otherwise the
/// topmost clip under the playhead (lowest track; on that track, the one that started last).
/// Clips on muted tracks are not heard, so they are not recorded into. None when no clip
/// plays there.
pub fn song_record_target(project: &Project, selected: PatternId, pos: f64) -> Option<(PatternId, f64)> {
    let arr = &project.arrangement;
    let len = |id| project.pattern(id).map(|p| p.length);
    if let Some(l) = len(selected)
        && let Some(p) = arr.pattern_pos(selected, l, pos)
    {
        return Some((selected, p));
    }
    arr.audible_clips()
        .filter_map(|c| Some((c, c.pattern_pos(len(c.pattern)?, pos)?)))
        .min_by_key(|(c, _)| (c.track, Reverse(c.start)))
        .map(|(c, p)| (c.pattern, p))
}

/// Song mode: adds a note recorded at the song position `rec.start` to the pattern of the
/// clip playing there (see [`song_record_target`]), quantized to `grid` (see
/// [`RecordedNote::to_note`]). The pattern does not grow: the note wraps into it as the clip
/// plays it, and ends at the end of the pattern at the latest (where its playback stops in
/// Pattern mode). Returns `false` when no clip plays there: the note is dropped.
pub fn add_song_recorded_note(project: &mut Project, selected: PatternId, rec: &RecordedNote, grid: Tick) -> bool {
    let Some((pattern, pos)) = song_record_target(project, selected, rec.start) else { return false };
    let Some(pat) = project.pattern_mut(pattern) else { return false };
    let plen = pat.length.max(1);
    let mut note = RecordedNote { start: pos, ..*rec }.to_note(grid);
    // Quantized up to the end of the pattern: the start of its next repetition.
    note.start %= plen;
    note.len = note.len.min(plen - note.start);
    pat.notes.entry(rec.channel).or_default().push(note);
    true
}

/// Recording went on past the end of the pattern, up to position `pos` (ticks): the
/// pattern grows to the end of the bar holding it. Returns `true` if it grew.
pub fn grow_to_position(project: &mut Project, pattern: PatternId, pos: f64) -> bool {
    let bar = project.bar_len();
    let Some(len) = project.pattern(pattern).map(|p| p.length) else { return false };
    // Also false for NaN.
    if !(pos > len as f64 && pos.is_finite()) {
        return false;
    }
    let new = crate::length_for_end(len, pos.ceil() as Tick, bar);
    project.pattern_mut(pattern).unwrap().length = new;
    new > len
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR: Tick = 16 * STEP;
    const A: ChannelId = ChannelId(1);
    const B: ChannelId = ChannelId(2);
    /// No quantization.
    const OFF: Tick = 0;

    /// Note-on at position `start`, released `held` ticks later, then converted.
    fn rec(start: f64, held: f64, grid: Tick) -> Note {
        let mut r = Recorder::default();
        assert_eq!(r.event(A, 60, 100, start, 1000.0, true), None);
        r.event(A, 60, 0, 0.0, 1000.0 + held, false).unwrap().to_note(grid)
    }

    fn note(start: Tick, len: Tick) -> Note {
        Note { start, len, key: 60, velocity: 100, pan: 0 }
    }

    #[test]
    fn simple_note() {
        assert_eq!(rec(480.0, 960.0, OFF), note(480, 960));
        // Sub-tick positions are truncated.
        assert_eq!(rec(480.9, 960.0, OFF), note(480, 960));
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
    fn notes_past_the_pattern_end_are_not_wrapped() {
        // Played in bar 2 of a one-bar pattern, held for two bars.
        assert_eq!(rec((BAR + 480) as f64, (2 * BAR) as f64, OFF), note(BAR + 480, 2 * BAR));
        // Quantized past the end: stays there.
        assert_eq!(rec((BAR - 10) as f64, 210.0, STEP), note(BAR, STEP));
    }

    #[test]
    fn quantize_rounds_start_and_length() {
        // Start 100 → 0 (rounded down), length 300 → 240.
        assert_eq!(rec(100.0, 300.0, STEP), note(0, STEP));
        // Start 130 → 240 (rounded up), length 400 → 480.
        assert_eq!(rec(130.0, 400.0, STEP), note(STEP, 2 * STEP));
        // Halfway rounds up.
        assert_eq!(rec(120.0, 480.0, STEP), note(STEP, 2 * STEP));
        // Without quantize, nothing moves.
        assert_eq!(rec(130.0, 400.0, OFF), note(130, 400));
    }

    #[test]
    fn quantize_length_is_at_least_one_step() {
        assert_eq!(rec(0.0, 10.0, STEP), note(0, STEP));
        assert_eq!(rec(0.0, 0.0, STEP), note(0, STEP));
    }

    #[test]
    fn quantize_to_every_piano_roll_grid() {
        // Played at 1100, held 700 ticks.
        let cases = [
            // 1/4 (960): length rounded up to one grid unit.
            (PPQ as Tick, note(960, 960)),
            // 1/8 (480).
            (PPQ as Tick / 2, note(960, 480)),
            // 1/16 (240).
            (STEP, note(1200, 720)),
            // 1/32 (120).
            (PPQ as Tick / 8, note(1080, 720)),
            // Triplet (1/6 beat = 160).
            (PPQ as Tick / 6, note(1120, 640)),
            // Off, or the piano roll's free snap (1 tick): nothing moves.
            (OFF, note(1100, 700)),
            (1, note(1100, 700)),
        ];
        for (grid, expected) in cases {
            assert_eq!(rec(1100.0, 700.0, grid), expected, "grid {grid}");
        }
    }

    #[test]
    fn short_note_clamped_to_quarter_step() {
        assert_eq!(rec(500.0, 10.0, OFF), note(500, STEP / 4));
        assert_eq!(rec(500.0, 0.0, OFF), note(500, STEP / 4));
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
            notes.extend(r.event(A, 60, vel, tick, tick, on).map(|n| n.to_note(OFF)));
        }
        let n = |start, len, velocity| Note { start, len, key: 60, velocity, pan: 0 };
        assert_eq!(notes, vec![n(0, 480, 80), n(480, 480, 110)]);
        assert_eq!(r.event(A, 60, 0, 1000.0, 1000.0, false), None);
    }

    #[test]
    fn velocity_zero_becomes_one() {
        let mut r = Recorder::default();
        r.event(A, 60, 0, 0.0, 0.0, true);
        assert_eq!(r.event(A, 60, 0, 480.0, 480.0, false).unwrap().to_note(OFF).velocity, 1);
    }

    #[test]
    fn out_of_range_values() {
        // Negative position → 0.
        assert_eq!(rec(-50.0, 480.0, OFF), note(0, 480));
        // NaN → 0 (f64::max ignores NaN).
        assert_eq!(rec(f64::NAN, 480.0, OFF), note(0, 480));
        // A negative or NaN duration → the shortest note.
        assert_eq!(rec(100.0, -1.0, OFF), note(100, STEP / 4));
        assert_eq!(rec(100.0, f64::NAN, OFF), note(100, STEP / 4));
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
        assert_eq!((b.channel, b.key, b.to_note(OFF)), (B, 60, Note { start: 240, len: 480, key: 60, velocity: 90, pan: 0 }));
        assert_eq!((a64.channel, a64.to_note(OFF)), (A, Note { start: 480, len: 480, key: 64, velocity: 80, pan: 0 }));
        assert_eq!((a60.channel, a60.to_note(OFF)), (A, Note { start: 0, len: 1200, key: 60, velocity: 100, pan: 0 }));
    }

    /// Project with a 4-bar pattern and a channel.
    fn four_bars() -> (Project, PatternId, ChannelId) {
        let mut p = Project::empty();
        let ch = p.add_channel("c", ChannelKind::Synth(SynthParams::default())).unwrap();
        let pat = p.patterns[0].id;
        p.pattern_mut(pat).unwrap().length = 4 * BAR;
        (p, pat, ch)
    }

    #[test]
    fn recorded_note_grows_the_pattern_by_whole_bars() {
        let (mut p, pat, ch) = four_bars();
        // Inside: no change.
        add_recorded_note(&mut p, pat, ch, note(BAR, STEP));
        assert_eq!(p.pattern(pat).unwrap().length, 4 * BAR);
        // Ending exactly at the end: no change.
        add_recorded_note(&mut p, pat, ch, note(4 * BAR - STEP, STEP));
        assert_eq!(p.pattern(pat).unwrap().length, 4 * BAR);
        // Held across the end: to the end of bar 5.
        add_recorded_note(&mut p, pat, ch, note(4 * BAR - STEP, 2 * STEP));
        assert_eq!(p.pattern(pat).unwrap().length, 5 * BAR);
        // Played in bar 7.
        add_recorded_note(&mut p, pat, ch, note(6 * BAR + 480, STEP));
        let pt = p.pattern(pat).unwrap();
        assert_eq!(pt.length, 7 * BAR);
        assert_eq!(pt.notes[&ch].len(), 4);
        assert_eq!(pt.notes[&ch][3], note(6 * BAR + 480, STEP));
    }

    #[test]
    fn stop_grows_the_pattern_to_the_bar_of_the_position() {
        let (mut p, pat, _) = four_bars();
        // Stopped before the end: no change.
        assert!(!grow_to_position(&mut p, pat, (3 * BAR) as f64 + 0.5));
        assert!(!grow_to_position(&mut p, pat, (4 * BAR) as f64));
        assert!(!grow_to_position(&mut p, pat, f64::NAN));
        assert!(!grow_to_position(&mut p, pat, f64::INFINITY));
        assert_eq!(p.pattern(pat).unwrap().length, 4 * BAR);
        // Stopped in bar 6: 6 bars.
        assert!(grow_to_position(&mut p, pat, (5 * BAR + 1000) as f64 + 0.25));
        assert_eq!(p.pattern(pat).unwrap().length, 6 * BAR);
        // Just past the end of bar 6: 7 bars.
        assert!(grow_to_position(&mut p, pat, (6 * BAR) as f64 + 0.1));
        assert_eq!(p.pattern(pat).unwrap().length, 7 * BAR);
        // Unknown pattern.
        assert!(!grow_to_position(&mut p, PatternId(999), (9 * BAR) as f64));
    }

    #[test]
    fn take_is_one_undo_step() {
        // The app holds the commit during the take: the notes and the growth undo together.
        let (mut p, pat, ch) = four_bars();
        let mut h = crate::History::new(&p, 100);
        add_recorded_note(&mut p, pat, ch, note(BAR, STEP));
        assert!(!h.commit_if_changed(&p, true));
        add_recorded_note(&mut p, pat, ch, note(4 * BAR + 100, STEP));
        grow_to_position(&mut p, pat, (5 * BAR + 10) as f64);
        assert!(h.commit_if_changed(&p, false));
        assert_eq!(p.pattern(pat).unwrap().length, 6 * BAR);
        assert!(h.undo(&mut p));
        let pt = p.pattern(pat).unwrap();
        assert_eq!(pt.length, 4 * BAR);
        assert!(pt.notes.get(&ch).is_none_or(|n| n.is_empty()));
    }

    /// Song mode: 4-bar song with an empty one-bar pattern `a` and a two-bar pattern `b`.
    fn song() -> (Project, PatternId, PatternId, ChannelId) {
        let mut p = Project::empty();
        let ch = p.add_channel("c", ChannelKind::Synth(SynthParams::default())).unwrap();
        let a = p.patterns[0].id;
        p.pattern_mut(a).unwrap().length = BAR;
        let b = p.add_pattern("b", 2 * BAR);
        p.arrangement.clips.clear();
        (p, a, b, ch)
    }

    fn clip(pattern: PatternId, track: u16, start: Tick, len: Tick) -> Clip {
        Clip { pattern, track, start, len }
    }

    /// Note played at song position `start` for `held` ticks.
    fn played(ch: ChannelId, start: f64, held: f64) -> RecordedNote {
        RecordedNote { channel: ch, key: 60, vel: 100, start, len: held }
    }

    fn notes(p: &Project, pat: PatternId, ch: ChannelId) -> Vec<Note> {
        p.pattern(pat).unwrap().notes.get(&ch).cloned().unwrap_or_default()
    }

    #[test]
    fn song_records_into_empty_pattern_clip() {
        let (mut p, a, b, ch) = song();
        // `a` in bar 2 only, the edited pattern being `b` (not in the song).
        p.arrangement.clips.push(clip(a, 0, BAR, BAR));
        assert!(add_song_recorded_note(&mut p, b, &played(ch, (BAR + 480) as f64, 960.0), OFF));
        assert_eq!(notes(&p, a, ch), vec![note(480, 960)]);
        assert!(notes(&p, b, ch).is_empty());
        // The pattern does not grow.
        assert_eq!(p.pattern(a).unwrap().length, BAR);
    }

    #[test]
    fn song_records_at_the_offset_of_the_clip() {
        let (mut p, a, _, ch) = song();
        p.arrangement.clips.push(clip(a, 1, 3 * BAR + 100, BAR));
        assert!(add_song_recorded_note(&mut p, a, &played(ch, (3 * BAR + 100 + STEP) as f64, STEP as f64), OFF));
        assert_eq!(notes(&p, a, ch), vec![note(STEP, STEP)]);
    }

    #[test]
    fn song_wraps_notes_into_a_looping_clip() {
        let (mut p, a, _, ch) = song();
        // A one-bar pattern looping over 3 bars.
        p.arrangement.clips.push(clip(a, 0, 0, 3 * BAR));
        assert!(add_song_recorded_note(&mut p, a, &played(ch, (2 * BAR + 720) as f64, STEP as f64), OFF));
        assert_eq!(notes(&p, a, ch), vec![note(720, STEP)]);
        // Held across the loop point: ends at the end of the pattern.
        assert!(add_song_recorded_note(&mut p, a, &played(ch, (2 * BAR - STEP) as f64, (4 * STEP) as f64), OFF));
        assert_eq!(notes(&p, a, ch)[1], note(BAR - STEP, STEP));
        // Quantized up to the loop point: the start of the pattern.
        assert!(add_song_recorded_note(&mut p, a, &played(ch, (BAR - 10) as f64, 200.0), STEP));
        assert_eq!(notes(&p, a, ch)[2], note(0, STEP));
        assert_eq!(p.pattern(a).unwrap().length, BAR);
    }

    #[test]
    fn song_quantizes_inside_the_pattern() {
        let (mut p, a, _, ch) = song();
        // Clip off the grid: the grid is the pattern's, as in the piano roll.
        p.arrangement.clips.push(clip(a, 0, 50, BAR));
        assert!(add_song_recorded_note(&mut p, a, &played(ch, (50 + STEP + 100) as f64, 300.0), STEP));
        assert_eq!(notes(&p, a, ch), vec![note(STEP, STEP)]);
    }

    #[test]
    fn song_without_clip_drops_the_note() {
        let (mut p, a, b, ch) = song();
        p.arrangement.clips.push(clip(a, 0, 0, BAR));
        // Past the clip end (exclusive), in an empty clip, on a muted track.
        assert!(!add_song_recorded_note(&mut p, a, &played(ch, BAR as f64, 100.0), OFF));
        p.arrangement.clips.push(clip(b, 1, 2 * BAR, 0));
        assert!(!add_song_recorded_note(&mut p, a, &played(ch, (2 * BAR) as f64, 100.0), OFF));
        p.arrangement.clips.push(clip(b, 2, 3 * BAR, BAR));
        p.arrangement.set_muted(2, true);
        assert!(!add_song_recorded_note(&mut p, a, &played(ch, (3 * BAR + 10) as f64, 100.0), OFF));
        assert!(!add_song_recorded_note(&mut p, a, &played(ch, f64::NAN, 100.0), OFF));
        assert!(notes(&p, a, ch).is_empty() && notes(&p, b, ch).is_empty());
    }

    #[test]
    fn song_target_prefers_the_edited_pattern_then_the_top_track() {
        let (mut p, a, b, _) = song();
        let c = p.add_pattern("c", BAR);
        p.arrangement.tracks = 4;
        p.arrangement.clips = vec![clip(b, 2, 0, 2 * BAR), clip(a, 1, BAR, BAR), clip(b, 3, 0, BAR)];
        // The edited pattern plays there: it wins, even on a lower track.
        assert_eq!(song_record_target(&p, b, (BAR + 10) as f64), Some((b, (BAR + 10) as f64)));
        // It does not (or not there): the topmost clip.
        assert_eq!(song_record_target(&p, c, (BAR + 10) as f64), Some((a, 10.0)));
        assert_eq!(song_record_target(&p, a, 10.0), Some((b, 10.0)));
        // Muted: the next track down.
        p.arrangement.set_muted(1, true);
        assert_eq!(song_record_target(&p, a, (BAR + 10) as f64), Some((b, (BAR + 10) as f64)));
        // Two clips on the same track: the one that started last.
        p.arrangement.clips = vec![clip(a, 0, 0, 4 * BAR), clip(b, 0, BAR, BAR)];
        assert_eq!(song_record_target(&p, c, (BAR + 10) as f64), Some((b, 10.0)));
        assert_eq!(song_record_target(&p, c, 10.0), Some((a, 10.0)));
        assert_eq!(song_record_target(&p, c, (4 * BAR) as f64), None);
    }

    #[test]
    fn song_take_is_one_undo_step() {
        let (mut p, a, _, ch) = song();
        p.arrangement.clips.push(clip(a, 0, 0, 2 * BAR));
        let mut h = crate::History::new(&p, 100);
        add_song_recorded_note(&mut p, a, &played(ch, 100.0, 100.0), OFF);
        assert!(!h.commit_if_changed(&p, true));
        add_song_recorded_note(&mut p, a, &played(ch, (BAR + 200) as f64, 100.0), OFF);
        assert!(h.commit_if_changed(&p, false));
        assert_eq!(notes(&p, a, ch).len(), 2);
        assert!(h.undo(&mut p));
        assert!(notes(&p, a, ch).is_empty());
    }
}
