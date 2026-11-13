//! Editing operations + undo/redo.
//!
//! Snapshot-based undo: the app calls [`History::commit_if_changed`] every frame.
//! While a gesture is in progress (mouse button held, keyboard input), nothing is
//! committed: a whole drag = a single undo entry. Snapshots share the channels and
//! patterns that did not change (`CowArc`), so an entry costs about what the gesture
//! modified, and comparing the project with the last snapshot is cheap.

use seq_model::*;
use std::collections::{HashSet, VecDeque};

mod record;
mod sync;
pub use record::{RecordedNote, Recorder};
pub use sync::{SlotUpdate, exports_song, slot_updates};

/// Default memory budget of the undo history (estimated).
pub const HISTORY_BUDGET: usize = 64 << 20;

pub struct History {
    committed: Project,
    /// Oldest first, with the memory each entry holds on its own (not shared with
    /// the next, more recent state).
    undo: VecDeque<(Project, usize)>,
    redo: Vec<Project>,
    limit: usize,
    budget: usize,
    undo_bytes: usize,
}

impl History {
    pub fn new(initial: &Project, limit: usize) -> Self {
        History {
            committed: initial.clone(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            limit: limit.max(1),
            budget: HISTORY_BUDGET,
            undo_bytes: 0,
        }
    }

    /// Caps the undo entries by estimated memory too (the most recent entry is always kept).
    pub fn with_budget(mut self, bytes: usize) -> Self {
        self.budget = bytes;
        self
    }

    /// Resets the history (new project / open).
    pub fn reset(&mut self, p: &Project) {
        self.committed = p.clone();
        self.undo.clear();
        self.redo.clear();
        self.undo_bytes = 0;
    }

    /// Records an undo entry if the project changed and no gesture is in progress.
    /// Returns `true` if an entry was created.
    pub fn commit_if_changed(&mut self, current: &Project, gesture_active: bool) -> bool {
        if gesture_active || *current == self.committed {
            return false;
        }
        let prev = std::mem::replace(&mut self.committed, current.clone());
        self.push_undo(prev);
        self.redo.clear();
        true
    }

    pub fn undo(&mut self, current: &mut Project) -> bool {
        // An uncommitted change is committed first, so it can be redone.
        self.commit_if_changed(current, false);
        match self.undo.pop_back() {
            Some((prev, bytes)) => {
                self.undo_bytes -= bytes;
                let now = std::mem::replace(&mut self.committed, prev.clone());
                self.redo.push(now);
                *current = prev;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self, current: &mut Project) -> bool {
        match self.redo.pop() {
            Some(next) => {
                let now = std::mem::replace(&mut self.committed, next.clone());
                self.push_undo(now);
                *current = next;
                true
            }
            None => false,
        }
    }

    /// `prev` precedes `self.committed`.
    fn push_undo(&mut self, prev: Project) {
        let bytes = unshared_bytes(&prev, &self.committed);
        self.undo_bytes += bytes;
        self.undo.push_back((prev, bytes));
        while self.undo.len() > self.limit || (self.undo_bytes > self.budget && self.undo.len() > 1) {
            let (_, b) = self.undo.pop_front().unwrap();
            self.undo_bytes -= b;
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Number of undo entries.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    /// Estimated memory held by the undo entries.
    pub fn undo_bytes(&self) -> usize {
        self.undo_bytes
    }
}

/// Estimated memory of `old` that is not shared with `new`.
fn unshared_bytes(old: &Project, new: &Project) -> usize {
    use std::mem::size_of;
    let channels: HashSet<*const Channel> = new.channels.iter().map(CowArc::as_ptr).collect();
    let patterns: HashSet<*const Pattern> = new.patterns.iter().map(CowArc::as_ptr).collect();
    let mut bytes = size_of::<Project>()
        + old.channels.len() * size_of::<CowArc<Channel>>()
        + old.patterns.len() * size_of::<CowArc<Pattern>>()
        + old.arrangement.clips.len() * size_of::<Clip>();
    for c in old.channels.iter().filter(|c| !channels.contains(&CowArc::as_ptr(c))) {
        bytes += size_of::<Channel>() + c.name.len();
    }
    for p in old.patterns.iter().filter(|p| !patterns.contains(&CowArc::as_ptr(p))) {
        bytes += size_of::<Pattern>() + p.name.len();
        for notes in p.notes.values() {
            // Rough cost of a map entry + the notes.
            bytes += 64 + notes.capacity() * size_of::<Note>();
        }
    }
    bytes
}

// ---------------------------------------------------------------------------
// Step sequencer
// ---------------------------------------------------------------------------

/// Number of steps shown for a pattern.
pub fn step_count(pattern: &Pattern) -> usize {
    pattern.length.div_ceil(STEP) as usize
}

/// Rule: a cell is lit if a note of the channel **starts** in this step,
/// whatever its key.
pub fn step_is_on(pattern: &Pattern, channel: ChannelId, step: usize) -> bool {
    let (a, b) = step_range(step);
    pattern
        .notes
        .get(&channel)
        .is_some_and(|ns| ns.iter().any(|n| n.start >= a && n.start < b))
}

fn step_range(step: usize) -> (Tick, Tick) {
    let a = step as Tick * STEP;
    (a, a + STEP)
}

/// Turns a step on/off. Turning on adds a one-step note on `default_key`;
/// turning off removes **all** notes that start in this step.
pub fn set_step(project: &mut Project, pattern: PatternId, channel: ChannelId, step: usize, on: bool) {
    let key = project.channel(channel).map(|c| c.default_key).unwrap_or(60);
    let Some(pat) = project.pattern_mut(pattern) else { return };
    let (a, b) = step_range(step);
    if a >= pat.length {
        return;
    }
    let currently = step_is_on(pat, channel, step);
    if on == currently {
        return;
    }
    let notes = pat.notes.entry(channel).or_default();
    if on {
        notes.push(Note { start: a, len: STEP, key, velocity: 100 });
    } else {
        notes.retain(|n| !(n.start >= a && n.start < b));
    }
    if notes.is_empty() {
        pat.notes.remove(&channel);
    }
}

// ---------------------------------------------------------------------------
// Channels & patterns
// ---------------------------------------------------------------------------

pub fn remove_channel(project: &mut Project, channel: ChannelId) {
    project.channels.retain(|c| c.id != channel);
    // Only the patterns that use the channel are copied.
    for p in project.patterns.iter_mut().filter(|p| p.notes.contains_key(&channel)) {
        p.make_mut().notes.remove(&channel);
    }
}

pub fn duplicate_channel(project: &mut Project, channel: ChannelId) -> Option<ChannelId> {
    let src = project.channel(channel)?.clone();
    let id = project.add_channel(&format!("{} (2)", src.name), src.kind.clone())?;
    let pos = project.channels.iter().position(|c| c.id == channel)?;
    let mut ch = project.channels.pop()?;
    let c = ch.make_mut();
    c.volume = src.volume;
    c.pan = src.pan;
    c.default_key = src.default_key;
    c.choke = src.choke;
    project.channels.insert(pos + 1, ch);
    Some(id)
}

/// Replaces a channel's instrument. Its notes (in every pattern), mixer settings, choke
/// group and arpeggiator are kept. The name follows the instrument unless the user chose
/// it; the step key too, unless it was changed (a sample always starts at its original
/// pitch). The synth / MIDI out settings being left are remembered for [`Channel::recall`].
/// Returns `false` if the channel does not exist or already has this instrument.
pub fn replace_kind(project: &mut Project, channel: ChannelId, mut kind: ChannelKind) -> bool {
    if let ChannelKind::MidiOut { out, .. } = &mut kind
        && *out as usize >= project.midi_outs.len()
    {
        // Remembered output removed since.
        *out = 0;
    }
    if project.channel(channel).is_none_or(|c| c.kind == kind) {
        return false;
    }
    let ch = project.channel_mut(channel).unwrap();
    let old = std::mem::replace(&mut ch.kind, kind.clone());
    if old.is_default_name(&ch.name) {
        ch.name = kind.default_name();
    }
    let to_sample = matches!(kind, ChannelKind::Sampler { .. }) && !matches!(old, ChannelKind::Sampler { .. });
    if to_sample || ch.default_key == old.default_key() {
        ch.default_key = kind.default_key();
    }
    ch.previous_kinds.retain(|k| !k.same_kind(&old) && !k.same_kind(&kind));
    if !matches!(old, ChannelKind::Sampler { .. }) {
        ch.previous_kinds.push(old);
    }
    true
}

pub fn move_channel(project: &mut Project, channel: ChannelId, up: bool) {
    let Some(i) = project.channels.iter().position(|c| c.id == channel) else { return };
    let j = if up { i.checked_sub(1) } else { (i + 1 < project.channels.len()).then_some(i + 1) };
    if let Some(j) = j {
        project.channels.swap(i, j);
    }
}

pub fn new_pattern(project: &mut Project) -> PatternId {
    let n = project.patterns.len() + 1;
    let len = project.bar_len();
    project.add_pattern(&format!("Pattern {n}"), len)
}

pub fn clone_pattern(project: &mut Project, pattern: PatternId) -> Option<PatternId> {
    let src = project.pattern(pattern)?.clone();
    let id = project.add_pattern(&seq_i18n::trf!("{} (copie)", "{} (copy)", src.name), src.length);
    project.pattern_mut(id)?.notes = src.notes;
    Some(id)
}

/// Deletes a pattern and its clips. Refuses to delete the last one.
pub fn delete_pattern(project: &mut Project, pattern: PatternId) -> bool {
    if project.patterns.len() <= 1 {
        return false;
    }
    project.patterns.retain(|p| p.id != pattern);
    project.arrangement.clips.retain(|c| c.pattern != pattern);
    true
}

/// Changes a pattern's length and removes the notes that start past it.
pub fn set_pattern_length(project: &mut Project, pattern: PatternId, length: Tick) {
    let Some(p) = project.pattern_mut(pattern) else { return };
    p.length = length.max(STEP);
    let len = p.length;
    for notes in p.notes.values_mut() {
        notes.retain(|n| n.start < len);
    }
    p.notes.retain(|_, v| !v.is_empty());
}

// ---------------------------------------------------------------------------
// Note selection (piano roll). `sel` = indices into `notes`.
// ---------------------------------------------------------------------------

/// (start of the first note, end of the last) of the selection.
pub fn selection_span(notes: &[Note], sel: &[usize]) -> Option<(Tick, Tick)> {
    let it = sel.iter().filter_map(|&i| notes.get(i));
    let start = it.clone().map(|n| n.start).min()?;
    let end = it.map(|n| n.start + n.len).max()?;
    Some((start, end))
}

/// Copies the selection, shifted to start at 0.
pub fn copy_notes(notes: &[Note], sel: &[usize]) -> Vec<Note> {
    let Some((start, _)) = selection_span(notes, sel) else { return Vec::new() };
    sel.iter()
        .filter_map(|&i| notes.get(i))
        .map(|n| Note { start: n.start - start, ..*n })
        .collect()
}

/// Pastes `clip` starting at `at`. Notes that would start after the end of the pattern
/// are dropped; lengths are truncated at the end of the pattern.
/// Returns the indices of the added notes (new selection).
pub fn paste_notes(notes: &mut Vec<Note>, clip: &[Note], at: Tick, pattern_len: Tick) -> Vec<usize> {
    let mut added = Vec::new();
    for n in clip {
        let start = at + n.start;
        if start >= pattern_len {
            continue;
        }
        notes.push(Note { start, len: n.len.min(pattern_len - start).max(1), ..*n });
        added.push(notes.len() - 1);
    }
    added
}

/// Duplicates the selection right after itself (offset = selection length rounded
/// up to the snap). Returns the new selection (the copies).
pub fn duplicate_notes(notes: &mut Vec<Note>, sel: &[usize], snap: Tick, pattern_len: Tick) -> Vec<usize> {
    let Some((start, end)) = selection_span(notes, sel) else { return Vec::new() };
    let snap = snap.max(1);
    let offset = (end - start).div_ceil(snap) * snap;
    let clip = copy_notes(notes, sel);
    paste_notes(notes, &clip, start + offset.max(snap), pattern_len)
}

/// Clamps an offset (time, key) so that all notes stay inside the pattern
/// and within 0..=127.
pub fn clamp_shift(orig: &[Note], dt: i64, dk: i32, pattern_len: Tick) -> (i64, i32) {
    if orig.is_empty() {
        return (0, 0);
    }
    let min_start = orig.iter().map(|n| n.start).min().unwrap() as i64;
    let max_start = orig.iter().map(|n| n.start).max().unwrap() as i64;
    let min_key = orig.iter().map(|n| n.key).min().unwrap() as i32;
    let max_key = orig.iter().map(|n| n.key).max().unwrap() as i32;
    let last = pattern_len.max(1) as i64 - 1;
    let dt = dt.clamp(-min_start, (last - max_start).max(-min_start));
    let dk = dk.clamp(-min_key, 127 - max_key);
    (dt, dk)
}

/// Shifts the selection (clamped). Returns `false` if nothing moved.
pub fn shift_notes(notes: &mut [Note], sel: &[usize], dt: i64, dk: i32, pattern_len: Tick) -> bool {
    let orig: Vec<Note> = sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
    let (dt, dk) = clamp_shift(&orig, dt, dk, pattern_len);
    if dt == 0 && dk == 0 {
        return false;
    }
    for &i in sel {
        if let Some(n) = notes.get_mut(i) {
            n.start = (n.start as i64 + dt) as Tick;
            n.key = (n.key as i32 + dk) as u8;
        }
    }
    true
}

/// "Bakes" the arpeggio: replaces the selected notes with the notes the arpeggiator
/// would play (same rules as the engine). Returns the indices of the new notes.
/// The notes held at a given moment form the arpeggiated chord; the arpeggio restarts
/// from the beginning after each rest.
pub fn bake_arp(notes: &mut Vec<Note>, sel: &[usize], p: &ArpParams, pattern_len: Tick) -> Vec<usize> {
    let mut src: Vec<Note> = sel.iter().filter_map(|&i| notes.get(i).copied()).collect();
    if src.is_empty() {
        return Vec::new();
    }
    src.sort_by_key(|n| (n.start, n.key));
    let rate = p.rate.max(1);
    let gate_len = ((rate as f32 * p.gate.clamp(0.05, 1.0)) as Tick).max(1);
    let start = src.iter().map(|n| n.start).min().unwrap();
    let end = src.iter().map(|n| n.start + n.len).max().unwrap().min(pattern_len);

    let mut out = Vec::new();
    let mut pool = seq_model::arp::ArpPool::default();
    let mut step: u64 = 0;
    let mut rng: u32 = 0x2545_f491;
    let mut rand = 0;
    let mut t = start;
    while t < end {
        let held: Vec<(u8, u8)> = src
            .iter()
            .filter(|n| n.start <= t && t < n.start + n.len)
            .map(|n| (n.key, n.velocity))
            .collect();
        if held.is_empty() {
            step = 0;
            t += rate;
            continue;
        }
        pool.build(&held, p);
        if p.direction == ArpDirection::Random && step % p.repeat.max(1) as u64 == 0 {
            rand = seq_model::arp::xorshift(&mut rng);
        }
        let picked: Vec<(u8, u8)> = match pool.index(step, p, rand) {
            Some(i) => vec![pool.keys[i]],
            None => pool.keys[..pool.n].to_vec(),
        };
        for (key, velocity) in picked {
            out.push(Note { start: t, len: gate_len.min(pattern_len - t), key, velocity });
        }
        step += 1;
        t += rate;
    }

    delete_notes(notes, sel);
    let first = notes.len();
    notes.extend(out);
    (first..notes.len()).collect()
}

pub fn delete_notes(notes: &mut Vec<Note>, sel: &[usize]) {
    let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < notes.len()).collect();
    s.sort_unstable();
    s.dedup();
    for i in s.into_iter().rev() {
        notes.remove(i);
    }
}

// ---------------------------------------------------------------------------
// Clip selection (playlist). `sel` = indices into `clips`.
// ---------------------------------------------------------------------------

/// (start, end, topmost track) of the selection.
pub fn clips_span(clips: &[Clip], sel: &[usize]) -> Option<(Tick, Tick, u16)> {
    let it = sel.iter().filter_map(|&i| clips.get(i));
    let start = it.clone().map(|c| c.start).min()?;
    let end = it.clone().map(|c| c.start + c.len).max()?;
    let track = it.map(|c| c.track).min()?;
    Some((start, end, track))
}

/// Copy: starts relative to the first clip, tracks relative to the topmost one.
pub fn copy_clips(clips: &[Clip], sel: &[usize]) -> Vec<Clip> {
    let Some((start, _, track)) = clips_span(clips, sel) else { return Vec::new() };
    sel.iter()
        .filter_map(|&i| clips.get(i))
        .map(|c| Clip { start: c.start - start, track: c.track - track, ..*c })
        .collect()
}

/// Pastes at `at` on track `track` (tracks limited to `tracks`). Returns the new indices.
pub fn paste_clips(clips: &mut Vec<Clip>, src: &[Clip], at: Tick, track: u16, tracks: u16) -> Vec<usize> {
    let mut added = Vec::new();
    for c in src {
        let t = (track as u32 + c.track as u32).min(tracks.max(1) as u32 - 1) as u16;
        clips.push(Clip { start: at + c.start, track: t, ..*c });
        added.push(clips.len() - 1);
    }
    added
}

/// Duplicates the selection right after itself, on the same tracks.
pub fn duplicate_clips(clips: &mut Vec<Clip>, sel: &[usize], snap: Tick, tracks: u16) -> Vec<usize> {
    let Some((start, end, track)) = clips_span(clips, sel) else { return Vec::new() };
    let snap = snap.max(1);
    let offset = ((end - start).div_ceil(snap) * snap).max(snap);
    let src = copy_clips(clips, sel);
    paste_clips(clips, &src, start + offset, track, tracks)
}

/// Clamps a move (time, tracks) so it stays inside the playlist.
pub fn clamp_clip_shift(orig: &[Clip], dt: i64, dtr: i32, tracks: u16) -> (i64, i32) {
    if orig.is_empty() {
        return (0, 0);
    }
    let min_start = orig.iter().map(|c| c.start).min().unwrap() as i64;
    let min_tr = orig.iter().map(|c| c.track).min().unwrap() as i32;
    let max_tr = orig.iter().map(|c| c.track).max().unwrap() as i32;
    (dt.max(-min_start), dtr.clamp(-min_tr, (tracks.max(1) as i32 - 1 - max_tr).max(-min_tr)))
}

pub fn shift_clips(clips: &mut [Clip], sel: &[usize], dt: i64, dtr: i32, tracks: u16) -> bool {
    let orig: Vec<Clip> = sel.iter().filter_map(|&i| clips.get(i).copied()).collect();
    let (dt, dtr) = clamp_clip_shift(&orig, dt, dtr, tracks);
    if dt == 0 && dtr == 0 {
        return false;
    }
    for &i in sel {
        if let Some(c) = clips.get_mut(i) {
            c.start = (c.start as i64 + dt) as Tick;
            c.track = (c.track as i32 + dtr) as u16;
        }
    }
    true
}

pub fn delete_clips(clips: &mut Vec<Clip>, sel: &[usize]) {
    let mut s: Vec<usize> = sel.iter().copied().filter(|&i| i < clips.len()).collect();
    s.sort_unstable();
    s.dedup();
    for i in s.into_iter().rev() {
        clips.remove(i);
    }
}

// ---------------------------------------------------------------------------
// Brush: back-to-back copies of a note / clip
// ---------------------------------------------------------------------------

/// Brush slot under position `t`: copies of length `len` starting at `origin`
/// (negative left of the origin).
pub fn paint_slot(t: Tick, origin: Tick, len: Tick) -> i64 {
    (t as i64 - origin as i64).div_euclid(len.max(1) as i64)
}

/// Paints copies of `brush` (key `key`) into slots `from..=to` (in any order),
/// slot 0 starting at `brush.start`. A slot is skipped if it falls outside the pattern or
/// overlaps a note on the same key; the last copy is shortened at the end of the pattern.
/// Returns the indices of the added notes.
pub fn paint_notes(notes: &mut Vec<Note>, brush: Note, key: u8, from: i64, to: i64, pattern_len: Tick) -> Vec<usize> {
    let len = brush.len.max(1) as i64;
    let mut added = Vec::new();
    for slot in from.min(to)..=from.max(to) {
        let start = brush.start as i64 + slot * len;
        if start < 0 || start >= pattern_len as i64 {
            continue;
        }
        let (start, len) = (start as Tick, (len as Tick).min(pattern_len - start as Tick));
        if notes.iter().any(|n| n.key == key && n.start < start + len && start < n.start + n.len) {
            continue;
        }
        notes.push(Note { start, len, key, velocity: brush.velocity });
        added.push(notes.len() - 1);
    }
    added
}

/// Paints copies of `brush` on its track into slots `from..=to`. A slot is skipped if it
/// starts before 0 or overlaps a clip on the same track. Returns the added indices.
pub fn paint_clips(clips: &mut Vec<Clip>, brush: Clip, from: i64, to: i64) -> Vec<usize> {
    let len = brush.len.max(1) as i64;
    let mut added = Vec::new();
    for slot in from.min(to)..=from.max(to) {
        let start = brush.start as i64 + slot * len;
        if start < 0 {
            continue;
        }
        let start = start as Tick;
        if clips.iter().any(|c| c.track == brush.track && c.start < start + brush.len && start < c.start + c.len) {
            continue;
        }
        clips.push(Clip { start, ..brush });
        added.push(clips.len() - 1);
    }
    added
}

// ---------------------------------------------------------------------------
// Snap
// ---------------------------------------------------------------------------

pub fn snap_floor(t: Tick, unit: Tick) -> Tick {
    if unit <= 1 { t } else { t / unit * unit }
}

pub fn snap_round(t: Tick, unit: Tick) -> Tick {
    if unit <= 1 { t } else { (t + unit / 2) / unit * unit }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (Project, PatternId, ChannelId) {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ch = p
            .add_channel("k", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) })
            .unwrap();
        (p, pat, ch)
    }

    #[test]
    fn paint_notes_tiles_and_skips_occupied() {
        let brush = Note { start: STEP, len: 2 * STEP, key: 60, velocity: 90 };
        let mut notes = vec![brush, Note { start: 5 * STEP, len: STEP, key: 60, velocity: 100 }];
        // Slots -1..=3: start -STEP (outside pattern), STEP (occupied), 3, 5 (occupied), 7.
        let added = paint_notes(&mut notes, brush, 60, 3, -1, 16 * STEP);
        let starts: Vec<Tick> = added.iter().map(|&i| notes[i].start).collect();
        assert_eq!(starts, vec![3 * STEP, 7 * STEP]);
        assert!(added.iter().all(|&i| notes[i].velocity == 90 && notes[i].len == 2 * STEP));
        // Different key: no collision.
        assert_eq!(paint_notes(&mut notes, brush, 62, 0, 0, 16 * STEP).len(), 1);
        // Last copy shortened at the end of the pattern.
        let mut n2 = Vec::new();
        let b = Note { start: 0, len: 3 * STEP, key: 60, velocity: 100 };
        paint_notes(&mut n2, b, 60, 0, 5, 4 * STEP);
        assert_eq!(n2.iter().map(|n| (n.start, n.len)).collect::<Vec<_>>(), vec![(0, 3 * STEP), (3 * STEP, STEP)]);
    }

    #[test]
    fn paint_clips_same_track_only() {
        let bar = 4 * PPQ as Tick;
        let brush = Clip { pattern: PatternId(1), track: 0, start: bar, len: bar };
        let mut clips = vec![brush, Clip { pattern: PatternId(2), track: 1, start: 2 * bar, len: bar }];
        let added = paint_clips(&mut clips, brush, -2, 2);
        let starts: Vec<Tick> = added.iter().map(|&i| clips[i].start).collect();
        assert_eq!(starts, vec![0, 2 * bar, 3 * bar]);
        assert!(added.iter().all(|&i| clips[i].track == 0 && clips[i].pattern == PatternId(1)));
        assert_eq!(paint_slot(bar - 1, bar, bar), -1);
        assert_eq!(paint_slot(2 * bar + 5, bar, bar), 1);
    }

    #[test]
    fn toggle_step() {
        let (mut p, pat, ch) = setup();
        set_step(&mut p, pat, ch, 3, true);
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 3));
        assert_eq!(p.pattern(pat).unwrap().notes[&ch][0].start, 3 * STEP);
        set_step(&mut p, pat, ch, 3, false);
        assert!(!step_is_on(p.pattern(pat).unwrap(), ch, 3));
        assert!(p.pattern(pat).unwrap().notes.get(&ch).is_none());
    }

    #[test]
    fn step_off_removes_piano_roll_notes_in_step() {
        let (mut p, pat, ch) = setup();
        p.pattern_mut(pat).unwrap().notes.insert(
            ch,
            vec![
                Note { start: STEP + 10, len: 500, key: 40, velocity: 90 },
                Note { start: STEP * 2, len: 500, key: 40, velocity: 90 },
            ],
        );
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 1));
        set_step(&mut p, pat, ch, 1, false);
        let notes = &p.pattern(pat).unwrap().notes[&ch];
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].start, STEP * 2);
    }

    #[test]
    fn undo_redo_coalesces_gestures() {
        let (mut p, pat, ch) = setup();
        let mut h = History::new(&p, 100);
        // Gesture in progress: several edits, a single entry.
        set_step(&mut p, pat, ch, 0, true);
        assert!(!h.commit_if_changed(&p, true));
        set_step(&mut p, pat, ch, 1, true);
        assert!(h.commit_if_changed(&p, false));
        set_step(&mut p, pat, ch, 2, true);
        h.commit_if_changed(&p, false);

        assert!(h.undo(&mut p));
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 1));
        assert!(!step_is_on(p.pattern(pat).unwrap(), ch, 2));
        assert!(h.undo(&mut p));
        assert!(!step_is_on(p.pattern(pat).unwrap(), ch, 0));
        assert!(!h.undo(&mut p));
        assert!(h.redo(&mut p));
        assert!(h.redo(&mut p));
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 2));
        assert!(!h.redo(&mut p));
    }

    #[test]
    fn undo_commits_pending_change_first() {
        let (mut p, pat, ch) = setup();
        let mut h = History::new(&p, 100);
        set_step(&mut p, pat, ch, 0, true);
        assert!(h.undo(&mut p));
        assert!(!step_is_on(p.pattern(pat).unwrap(), ch, 0));
        assert!(h.redo(&mut p));
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 0));
    }

    /// `n` patterns of `notes` notes each, on one channel.
    fn big_project(n: usize, notes: u64) -> (Project, ChannelId) {
        let (mut p, _, ch) = setup();
        p.patterns.clear();
        for i in 0..n {
            let id = p.add_pattern(&format!("P{i}"), notes * STEP);
            let ns = (0..notes).map(|t| Note { start: t * STEP, len: STEP, key: 60, velocity: 100 }).collect();
            p.pattern_mut(id).unwrap().notes.insert(ch, ns);
        }
        (p, ch)
    }

    fn nudge(p: &mut Project, pattern: usize, ch: ChannelId) {
        let pat = p.patterns[pattern].make_mut();
        pat.notes.get_mut(&ch).unwrap()[0].key += 1;
    }

    #[test]
    fn snapshots_share_unchanged_patterns() {
        let (mut p, ch) = big_project(32, 1000);
        let mut h = History::new(&p, 200);
        for i in 0..200 {
            nudge(&mut p, i % 32, ch);
            assert!(h.commit_if_changed(&p, false));
        }
        assert_eq!(h.undo_len(), 200);
        // Each entry holds about one pattern (1000 notes), not the 32.
        let pattern = 1000 * std::mem::size_of::<Note>();
        assert!(h.undo_bytes() < 200 * pattern * 3 / 2, "{}", h.undo_bytes());
        // Undo restores the right notes and shares the patterns it did not touch.
        let before = p.clone();
        nudge(&mut p, 5, ch);
        h.commit_if_changed(&p, false);
        assert!(h.undo(&mut p));
        assert_eq!(p, before);
        assert!((0..32).filter(|&i| i != 5).all(|i| CowArc::ptr_eq(&p.patterns[i], &before.patterns[i])));
    }

    #[test]
    fn history_memory_stays_bounded() {
        // One large pattern: every entry holds a full copy of it.
        let (mut p, ch) = big_project(1, 20_000);
        let budget = 4 << 20;
        let mut h = History::new(&p, 200).with_budget(budget);
        let mut states = vec![p.clone()];
        for _ in 0..100 {
            nudge(&mut p, 0, ch);
            h.commit_if_changed(&p, false);
            states.push(p.clone());
        }
        let kept = h.undo_len();
        assert!(kept > 1 && kept < 100, "{kept}");
        assert!(h.undo_bytes() <= budget);
        // The most recent entries can still be undone, in order.
        for i in 0..kept {
            assert!(h.undo(&mut p));
            assert_eq!(p, states[states.len() - 2 - i]);
        }
        assert!(!h.undo(&mut p));
        // And redone.
        while h.redo(&mut p) {}
        assert_eq!(p, *states.last().unwrap());
    }

    #[test]
    fn history_limit_drops_oldest() {
        let (mut p, pat, ch) = setup();
        let mut h = History::new(&p, 3);
        for step in 0..5 {
            set_step(&mut p, pat, ch, step, true);
            h.commit_if_changed(&p, false);
        }
        assert_eq!(h.undo_len(), 3);
        while h.undo(&mut p) {}
        // Steps 0 and 1 are older than the limit: they stay.
        assert!(step_is_on(p.pattern(pat).unwrap(), ch, 1));
        assert!(!step_is_on(p.pattern(pat).unwrap(), ch, 2));
    }

    #[test]
    fn remove_channel_cleans_notes() {
        let (mut p, pat, ch) = setup();
        set_step(&mut p, pat, ch, 0, true);
        remove_channel(&mut p, ch);
        assert!(p.channels.is_empty());
        assert!(p.pattern(pat).unwrap().notes.is_empty());
    }

    #[test]
    fn replace_kind_keeps_notes_and_settings() {
        let (mut p, pat, ch) = setup();
        let pat2 = new_pattern(&mut p);
        set_step(&mut p, pat, ch, 0, true);
        set_step(&mut p, pat2, ch, 3, true);
        let c = p.channel_mut(ch).unwrap();
        (c.volume, c.pan, c.mute, c.solo, c.choke) = (0.3, -0.5, true, true, 2);
        c.arp.enabled = true;
        let before = p.clone();
        let mut h = History::new(&p, 100);

        let synth = ChannelKind::Synth(SynthParams::default());
        assert!(replace_kind(&mut p, ch, synth.clone()));
        assert!(!replace_kind(&mut p, ch, synth.clone()));
        assert!(h.commit_if_changed(&p, false));
        let c = p.channel(ch).unwrap();
        assert_eq!(c.kind, synth);
        assert_eq!((c.volume, c.pan, c.mute, c.solo, c.choke, c.arp), (0.3, -0.5, true, true, 2, before.channels[0].arp));
        for id in [pat, pat2] {
            assert_eq!(p.pattern(id).unwrap().notes, before.pattern(id).unwrap().notes);
        }
        assert!(h.undo(&mut p));
        assert_eq!(p, before);
    }

    #[test]
    fn replace_kind_renames_only_default_names() {
        let (mut p, _, ch) = setup();
        p.channel_mut(ch).unwrap().name = "Kick".into();
        replace_kind(&mut p, ch, ChannelKind::Synth(SynthParams::default()));
        assert_eq!(p.channel(ch).unwrap().name, "Synth");
        replace_kind(&mut p, ch, ChannelKind::MidiOut { channel: 0, out: 0 });
        assert_eq!(p.channel(ch).unwrap().name, "MIDI out");
        replace_kind(&mut p, ch, ChannelKind::Sampler { source: SampleSource::File("loops/break.wav".into()) });
        assert_eq!(p.channel(ch).unwrap().name, "break");
        // Drum → drum (names that are the same in every language: the language is global).
        p.channel_mut(ch).unwrap().name = "Clap".into();
        p.channel_mut(ch).unwrap().kind = ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Clap) };
        replace_kind(&mut p, ch, ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Rim) });
        assert_eq!(p.channel(ch).unwrap().name, "Rim");
        // Name chosen by the user: kept.
        p.channel_mut(ch).unwrap().name = "Lead".into();
        replace_kind(&mut p, ch, ChannelKind::Synth(SynthParams::default()));
        assert_eq!(p.channel(ch).unwrap().name, "Lead");
    }

    #[test]
    fn replace_kind_default_key() {
        let (mut p, _, ch) = setup();
        let synth = ChannelKind::Synth(SynthParams::default());
        let midi = ChannelKind::MidiOut { channel: 0, out: 0 };
        let drum = ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Snare) };
        let key = |p: &Project| p.channel(ch).unwrap().default_key;
        // Default keys follow the instrument.
        replace_kind(&mut p, ch, synth.clone());
        assert_eq!(key(&p), 48);
        replace_kind(&mut p, ch, midi.clone());
        assert_eq!(key(&p), 60);
        // A key chosen by the user is kept…
        p.channel_mut(ch).unwrap().default_key = 36;
        replace_kind(&mut p, ch, synth);
        assert_eq!(key(&p), 36);
        // …except for a sample, which plays at its original pitch.
        replace_kind(&mut p, ch, drum);
        assert_eq!(key(&p), 60);
    }

    #[test]
    fn replace_kind_remembers_synth_and_midi_settings() {
        let (mut p, _, ch) = setup();
        p.midi_outs.push(None);
        let sp = SynthParams { cutoff: 1234.0, ..Default::default() };
        replace_kind(&mut p, ch, ChannelKind::Synth(sp));
        replace_kind(&mut p, ch, ChannelKind::MidiOut { channel: 9, out: 1 });
        let c = p.channel(ch).unwrap();
        assert_eq!(c.recall(ChannelKind::Synth(SynthParams::default())), ChannelKind::Synth(sp));
        // Not a MIDI out before: defaults.
        assert_eq!(c.recall(ChannelKind::MidiOut { channel: 0, out: 0 }), ChannelKind::MidiOut { channel: 0, out: 0 });
        replace_kind(&mut p, ch, ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Clap) });
        let c = p.channel(ch).unwrap();
        assert_eq!(c.recall(ChannelKind::MidiOut { channel: 0, out: 0 }), ChannelKind::MidiOut { channel: 9, out: 1 });
        assert_eq!(c.recall(ChannelKind::Synth(SynthParams::default())), ChannelKind::Synth(sp));
        assert_eq!(c.previous_kinds.len(), 2);
        // Output removed in the meantime: back to OUT 1.
        p.midi_outs.pop();
        let k = p.channel(ch).unwrap().recall(ChannelKind::MidiOut { channel: 0, out: 0 });
        replace_kind(&mut p, ch, k);
        assert_eq!(p.channel(ch).unwrap().kind, ChannelKind::MidiOut { channel: 9, out: 0 });
        assert_eq!(p.channel(ch).unwrap().previous_kinds, vec![ChannelKind::Synth(sp)]);
    }

    #[test]
    fn delete_pattern_removes_clips() {
        let (mut p, pat, _) = setup();
        let p2 = new_pattern(&mut p);
        p.arrangement.clips.push(Clip { pattern: p2, track: 0, start: 0, len: 100 });
        assert!(delete_pattern(&mut p, p2));
        assert!(p.arrangement.clips.is_empty());
        assert!(!delete_pattern(&mut p, pat));
    }

    fn n(start: Tick, len: Tick, key: u8) -> Note {
        Note { start, len, key, velocity: 100 }
    }

    #[test]
    fn copy_paste_keeps_spacing() {
        let mut notes = vec![n(480, 240, 60), n(960, 240, 64), n(0, 100, 50)];
        let clip = copy_notes(&notes, &[0, 1]);
        assert_eq!(clip, vec![n(0, 240, 60), n(480, 240, 64)]);
        let added = paste_notes(&mut notes, &clip, 1920, 3840);
        assert_eq!(added, vec![3, 4]);
        assert_eq!(notes[3], n(1920, 240, 60));
        assert_eq!(notes[4], n(2400, 240, 64));
        // Past the end of the pattern: dropped / truncated.
        let added = paste_notes(&mut notes, &clip, 3700, 3840);
        assert_eq!(added.len(), 1);
        assert_eq!(notes.last().unwrap().len, 140);
    }

    #[test]
    fn duplicate_after_selection() {
        let mut notes = vec![n(0, 240, 60), n(480, 240, 62)];
        let new_sel = duplicate_notes(&mut notes, &[0, 1], 240, 3840);
        // Selection 0..720 → offset 720 (multiple of 240).
        assert_eq!(new_sel, vec![2, 3]);
        assert_eq!(notes[2].start, 720);
        assert_eq!(notes[3].start, 1200);
    }

    #[test]
    fn shift_is_clamped() {
        let mut notes = vec![n(0, 240, 120), n(480, 240, 60)];
        assert!(!shift_notes(&mut notes, &[0, 1], -100, 0, 3840));
        assert!(shift_notes(&mut notes, &[0, 1], 0, 12, 3840));
        // The highest note hits 127: offset clamped to +7.
        assert_eq!(notes[0].key, 127);
        assert_eq!(notes[1].key, 67);
        assert!(shift_notes(&mut notes, &[1], 10_000, 0, 3840));
        assert_eq!(notes[1].start, 3839);
    }

    #[test]
    fn bake_arp_replaces_chord() {
        // C-E-G chord on a quarter note, then a rest, then A alone on an eighth note.
        let mut notes = vec![n(0, 960, 64), n(0, 960, 60), n(0, 960, 67), n(1920, 480, 69), n(5000, 10, 10)];
        let p = ArpParams { rate: 240, gate: 0.5, direction: ArpDirection::UpDown, ..Default::default() };
        let new_sel = bake_arp(&mut notes, &[0, 1, 2, 3], &p, 3840);
        assert_eq!(notes[0], n(5000, 10, 10), "the unselected note is kept");
        let baked: Vec<(Tick, u8)> = new_sel.iter().map(|&i| (notes[i].start, notes[i].key)).collect();
        assert_eq!(baked, vec![(0, 60), (240, 64), (480, 67), (720, 64), (1920, 69), (2160, 69)]);
        assert!(new_sel.iter().all(|&i| notes[i].len == 120));
    }

    #[test]
    fn clips_copy_paste_duplicate_shift() {
        let pid = PatternId(1);
        let c = |start, track, len| Clip { pattern: pid, track, start, len };
        let mut clips = vec![c(3840, 2, 3840), c(7680, 3, 1920), c(0, 0, 100)];
        let src = copy_clips(&clips, &[0, 1]);
        assert_eq!(src, vec![c(0, 0, 3840), c(3840, 1, 1920)]);
        // Pasted on the last track: relative track +1 is pulled back into the playlist.
        let added = paste_clips(&mut clips, &src, 0, 7, 8);
        assert_eq!((clips[added[0]].track, clips[added[1]].track), (7, 7));
        // Duplicate: selection 3840..9600 (5760) → offset 7680 with bar snap.
        let d = duplicate_clips(&mut clips, &[0, 1], 3840, 8);
        assert_eq!((clips[d[0]].start, clips[d[0]].track), (3840 + 7680, 2));
        assert_eq!((clips[d[1]].start, clips[d[1]].track), (7680 + 7680, 3));
        // Clamped offset: track 0 at minimum, start 0 at minimum.
        assert!(shift_clips(&mut clips, &[0, 1], -100_000, -10, 8));
        assert_eq!((clips[0].start, clips[0].track), (0, 0));
        assert_eq!((clips[1].start, clips[1].track), (3840, 1));
        delete_clips(&mut clips, &[0, 1]);
        assert_eq!(clips.len(), 5);
    }

    #[test]
    fn delete_many() {
        let mut notes = vec![n(0, 1, 1), n(1, 1, 2), n(2, 1, 3), n(3, 1, 4)];
        delete_notes(&mut notes, &[3, 1, 1, 9]);
        assert_eq!(notes.iter().map(|x| x.key).collect::<Vec<_>>(), vec![1, 3]);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_floor(250, 240), 240);
        assert_eq!(snap_round(370, 240), 480);
        assert_eq!(snap_round(370, 1), 370);
    }
}
