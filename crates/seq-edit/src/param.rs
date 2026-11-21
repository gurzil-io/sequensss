//! Note parameters edited as a whole (piano roll side panel, lane and two-finger gesture):
//! the value of a selection, and a relative change applied to several notes at once.

use seq_model::*;

/// Note parameter chosen in the piano roll. More can be added: `ALL`, `get` and
/// `adjust_param` are the only places that list them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoteParam {
    #[default]
    Velocity,
    Length,
    Pan,
}

impl NoteParam {
    pub const ALL: [NoteParam; 3] = [NoteParam::Velocity, NoteParam::Length, NoteParam::Pan];

    /// The next one, cycling.
    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&p| p == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    /// Value of a note: velocity, length in ticks, or pan.
    pub fn get(self, n: &Note) -> i64 {
        match self {
            NoteParam::Velocity => n.velocity as i64,
            NoteParam::Length => n.len as i64,
            NoteParam::Pan => n.pan as i64,
        }
    }
}

/// Lowest and highest value of `param` among the notes `sel`; `None` without notes.
pub fn param_range(notes: &[Note], sel: &[usize], param: NoteParam) -> Option<(i64, i64)> {
    sel.iter().filter_map(|&i| notes.get(i)).map(|n| param.get(n)).fold(None, |r, v| match r {
        None => Some((v, v)),
        Some((lo, hi)) => Some((lo.min(v), hi.max(v))),
    })
}

/// Shortest note a resize or a length change leaves at snap `snap`.
pub fn min_note_len(snap: Tick) -> Tick {
    snap.clamp(1, STEP / 4)
}

/// Relative change: `param` of each note of `orig` (index, state when the gesture began)
/// becomes its initial value plus `delta`, each note clamped on its own, so the
/// differences between notes are kept (except where clamped). Velocity stays within
/// 1..=127, pan within `PAN_MIN..=PAN_MAX`; a length (`delta` in ticks) stays at least
/// `min_len` and, when growing, doesn't go past `limit` (a note already past it isn't cut).
/// Nothing else changes.
pub fn adjust_param(notes: &mut [Note], orig: &[(usize, Note)], param: NoteParam, delta: i64, min_len: Tick, limit: Tick) {
    for &(i, n) in orig {
        let Some(m) = notes.get_mut(i) else { continue };
        let v = param.get(&n).saturating_add(delta);
        match param {
            NoteParam::Velocity => m.velocity = if delta == 0 { n.velocity } else { v.clamp(1, 127) as u8 },
            NoteParam::Pan => m.pan = v.clamp(PAN_MIN as i64, PAN_MAX as i64) as i8,
            NoteParam::Length => {
                let len = v.max(min_len as i64) as Tick;
                m.len = if delta > 0 { len.min(limit.saturating_sub(n.start).max(n.len)) } else { len };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::History;

    fn note(start: Tick, len: Tick, velocity: u8, pan: i8) -> Note {
        Note { start, len, key: 60, velocity, pan }
    }

    fn orig(notes: &[Note]) -> Vec<(usize, Note)> {
        notes.iter().copied().enumerate().collect()
    }

    #[test]
    fn params_cycle() {
        assert_eq!(NoteParam::default(), NoteParam::Velocity);
        assert_eq!(NoteParam::Velocity.next(), NoteParam::Length);
        assert_eq!(NoteParam::Length.next(), NoteParam::Pan);
        assert_eq!(NoteParam::Pan.next(), NoteParam::Velocity);
    }

    #[test]
    fn range_of_selection() {
        let notes = [note(0, STEP, 100, 0), note(0, 2 * STEP, 40, -10), note(0, STEP, 90, 30)];
        assert_eq!(param_range(&notes, &[], NoteParam::Velocity), None);
        assert_eq!(param_range(&notes, &[0], NoteParam::Velocity), Some((100, 100)));
        assert_eq!(param_range(&notes, &[0, 1, 2], NoteParam::Velocity), Some((40, 100)));
        assert_eq!(param_range(&notes, &[0, 2], NoteParam::Length), Some((STEP as i64, STEP as i64)));
        assert_eq!(param_range(&notes, &[1, 2, 7], NoteParam::Pan), Some((-10, 30)));
    }

    #[test]
    fn velocity_is_relative_and_clamped_per_note() {
        let start = [note(0, STEP, 100, 0), note(STEP, STEP, 40, 0), note(0, STEP, 1, 5)];
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &orig(&start), NoteParam::Velocity, 20, 1, 10_000);
        assert_eq!(notes.iter().map(|n| n.velocity).collect::<Vec<_>>(), vec![120, 60, 21]);
        // Always from the initial values, not cumulative.
        adjust_param(&mut notes, &orig(&start), NoteParam::Velocity, 30, 1, 10_000);
        assert_eq!(notes.iter().map(|n| n.velocity).collect::<Vec<_>>(), vec![127, 70, 31]);
        adjust_param(&mut notes, &orig(&start), NoteParam::Velocity, -60, 1, 10_000);
        assert_eq!(notes.iter().map(|n| n.velocity).collect::<Vec<_>>(), vec![40, 1, 1]);
        // Back to zero: the initial state.
        adjust_param(&mut notes, &orig(&start), NoteParam::Velocity, 0, 1, 10_000);
        assert_eq!(notes, start);
    }

    #[test]
    fn pan_is_relative_and_clamped_per_note() {
        let start = [note(0, STEP, 100, 0), note(0, STEP, 100, -50), note(0, STEP, 100, 60)];
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &orig(&start), NoteParam::Pan, 10, 1, 10_000);
        assert_eq!(notes.iter().map(|n| n.pan).collect::<Vec<_>>(), vec![10, -40, 63]);
        adjust_param(&mut notes, &orig(&start), NoteParam::Pan, -1000, 1, 10_000);
        assert_eq!(notes.iter().map(|n| n.pan).collect::<Vec<_>>(), vec![-64, -64, -64]);
        // Nothing else changes.
        assert!(notes.iter().zip(&start).all(|(a, b)| (a.start, a.len, a.key, a.velocity) == (b.start, b.len, b.key, b.velocity)));
    }

    #[test]
    fn only_listed_notes_change() {
        let start = [note(0, STEP, 100, 0), note(STEP, STEP, 50, 0)];
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &[(1, start[1]), (9, start[0])], NoteParam::Velocity, 10, 1, 10_000);
        assert_eq!((notes[0].velocity, notes[1].velocity), (100, 60));
    }

    #[test]
    fn length_changes_by_the_same_amount_on_every_note() {
        let start = [note(0, STEP, 100, 0), note(STEP, 2 * STEP, 100, 0), note(0, STEP + 7, 100, 0)];
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &orig(&start), NoteParam::Length, STEP as i64, STEP / 4, 10_000);
        assert_eq!(notes.iter().map(|n| n.len).collect::<Vec<_>>(), vec![2 * STEP, 3 * STEP, 2 * STEP + 7]);
        // Start and pitch untouched.
        assert!(notes.iter().zip(&start).all(|(a, b)| (a.start, a.key) == (b.start, b.key)));
    }

    #[test]
    fn length_keeps_a_minimum() {
        let start = [note(0, STEP, 100, 0), note(0, 4 * STEP, 100, 0)];
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &orig(&start), NoteParam::Length, -10 * STEP as i64, min_note_len(STEP), 10_000);
        assert_eq!(notes.iter().map(|n| n.len).collect::<Vec<_>>(), vec![STEP / 4, STEP / 4]);
        // Free snap: down to one tick.
        adjust_param(&mut notes, &orig(&start), NoteParam::Length, -10 * STEP as i64, min_note_len(1), 10_000);
        assert_eq!(notes.iter().map(|n| n.len).collect::<Vec<_>>(), vec![1, 1]);
    }

    #[test]
    fn length_growth_stops_at_the_limit() {
        let start = [note(0, STEP, 100, 0), note(3 * STEP, STEP, 100, 0), note(10 * STEP, STEP, 100, 0)];
        let limit = 4 * STEP;
        let mut notes = start.to_vec();
        adjust_param(&mut notes, &orig(&start), NoteParam::Length, 100 * STEP as i64, STEP / 4, limit);
        assert_eq!(notes.iter().map(|n| n.len).collect::<Vec<_>>(), vec![4 * STEP, STEP, STEP]);
        // Shrinking a note already past the limit still works.
        let past = [note(10 * STEP, 2 * STEP, 100, 0)];
        let mut notes = past.to_vec();
        adjust_param(&mut notes, &orig(&past), NoteParam::Length, -(STEP as i64), STEP / 4, limit);
        assert_eq!(notes[0].len, STEP);
    }

    #[test]
    fn whole_gesture_is_one_undo_step() {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ch = p.add_channel("s", ChannelKind::Synth(SynthParams::default())).unwrap();
        let start = vec![note(0, STEP, 100, 0), note(STEP, STEP, 60, 0), note(2 * STEP, STEP, 80, 0)];
        p.pattern_mut(pat).unwrap().notes.insert(ch, start.clone());
        let mut h = History::new(&p, 100);
        // First finger: its press nudges the held note, undone when the second finger lands.
        p.pattern_mut(pat).unwrap().notes.get_mut(&ch).unwrap()[1].start += STEP;
        assert!(!h.commit_if_changed(&p, true));
        p.pattern_mut(pat).unwrap().notes.insert(ch, start.clone());
        // Second finger slides over several frames, fingers down all along.
        let sel = [(0, start[0]), (1, start[1])];
        for delta in [5, 12, 30, 25] {
            let notes = p.pattern_mut(pat).unwrap().notes.get_mut(&ch).unwrap();
            adjust_param(notes, &sel, NoteParam::Velocity, delta, 1, 10_000);
            assert!(!h.commit_if_changed(&p, true));
        }
        // Fingers lifted.
        assert!(h.commit_if_changed(&p, false));
        let edited = p.pattern(pat).unwrap().notes[&ch].clone();
        assert_eq!(edited.iter().map(|n| n.velocity).collect::<Vec<_>>(), vec![125, 85, 80]);
        assert_eq!(h.undo_len(), 1);
        assert!(h.undo(&mut p));
        assert_eq!(p.pattern(pat).unwrap().notes[&ch], start);
        assert!(h.redo(&mut p));
        assert_eq!(p.pattern(pat).unwrap().notes[&ch], edited);
    }
}
