//! Arpeggiator: parameters + pure sequencing logic, shared between the engine
//! (real time, allocation-free: fixed arrays) and the piano roll ("baking" the arpeggio).

use crate::{PPQ, Tick};
use seq_i18n::tr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArpDirection {
    Up,
    Down,
    /// Up then down, without repeating the end notes.
    UpDown,
    DownUp,
    /// Up then down, repeating the end notes.
    UpDownInclusive,
    /// In the order the notes were played.
    AsPlayed,
    Random,
    /// All notes together on every step (rhythmic chords).
    Chord,
}

impl ArpDirection {
    pub const ALL: [ArpDirection; 8] = [
        ArpDirection::Up,
        ArpDirection::Down,
        ArpDirection::UpDown,
        ArpDirection::DownUp,
        ArpDirection::UpDownInclusive,
        ArpDirection::AsPlayed,
        ArpDirection::Random,
        ArpDirection::Chord,
    ];
    pub fn name(self) -> &'static str {
        match self {
            ArpDirection::Up => tr!("Montée", "Up"),
            ArpDirection::Down => tr!("Descente", "Down"),
            ArpDirection::UpDown => tr!("Montée-descente", "Up-down"),
            ArpDirection::DownUp => tr!("Descente-montée", "Down-up"),
            ArpDirection::UpDownInclusive => tr!("Montée-descente (bouts répétés)", "Up-down (repeat ends)"),
            ArpDirection::AsPlayed => tr!("Ordre joué", "As played"),
            ArpDirection::Random => tr!("Aléatoire", "Random"),
            ArpDirection::Chord => tr!("Accord", "Chord"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArpChord {
    /// Only the played notes.
    None,
    Major,
    Minor,
    Sus2,
    Sus4,
    Power,
    Major7,
    Minor7,
    Dominant7,
    Octave,
}

impl ArpChord {
    pub const ALL: [ArpChord; 10] = [
        ArpChord::None,
        ArpChord::Major,
        ArpChord::Minor,
        ArpChord::Sus2,
        ArpChord::Sus4,
        ArpChord::Power,
        ArpChord::Major7,
        ArpChord::Minor7,
        ArpChord::Dominant7,
        ArpChord::Octave,
    ];
    pub fn name(self) -> &'static str {
        match self {
            ArpChord::None => tr!("Aucun (notes jouées)", "None (played notes)"),
            ArpChord::Major => tr!("Majeur", "Major"),
            ArpChord::Minor => tr!("Mineur", "Minor"),
            ArpChord::Sus2 => "Sus2",
            ArpChord::Sus4 => "Sus4",
            ArpChord::Power => tr!("Quinte (power)", "Fifth (power)"),
            ArpChord::Major7 => "Maj7",
            ArpChord::Minor7 => "Min7",
            ArpChord::Dominant7 => tr!("7 (dominante)", "7 (dominant)"),
            ArpChord::Octave => "Octave",
        }
    }
    /// Intervals in semitones from each played note.
    pub fn intervals(self) -> &'static [u8] {
        match self {
            ArpChord::None => &[0],
            ArpChord::Major => &[0, 4, 7],
            ArpChord::Minor => &[0, 3, 7],
            ArpChord::Sus2 => &[0, 2, 7],
            ArpChord::Sus4 => &[0, 5, 7],
            ArpChord::Power => &[0, 7],
            ArpChord::Major7 => &[0, 4, 7, 11],
            ArpChord::Minor7 => &[0, 3, 7, 10],
            ArpChord::Dominant7 => &[0, 4, 7, 10],
            ArpChord::Octave => &[0, 12],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArpParams {
    pub enabled: bool,
    pub direction: ArpDirection,
    /// Range in octaves (1 = only the played notes).
    pub octaves: u8,
    /// Length of one arpeggio step.
    pub rate: Tick,
    /// Length of each note, as a fraction of the step (0.05..=1.0).
    pub gate: f32,
    /// Number of times each note is repeated.
    pub repeat: u8,
    pub chord: ArpChord,
    /// Steps locked to the song grid during playback (otherwise: start immediately).
    pub sync: bool,
    /// Keeps notes after release until the next chord (live playing).
    pub hold: bool,
}

impl Default for ArpParams {
    fn default() -> Self {
        ArpParams {
            enabled: false,
            direction: ArpDirection::Up,
            octaves: 1,
            rate: PPQ as Tick / 4,
            gate: 0.5,
            repeat: 1,
            chord: ArpChord::None,
            sync: true,
            hold: false,
        }
    }
}

pub const ARP_RATES: [(&str, Tick); 8] = [
    ("1/4", PPQ as Tick),
    ("1/8", PPQ as Tick / 2),
    ("1/8 triolet", PPQ as Tick / 3),
    ("1/16", PPQ as Tick / 4),
    ("1/16 triolet", PPQ as Tick / 6),
    ("1/32", PPQ as Tick / 8),
    ("1/2", PPQ as Tick * 2),
    ("1/1", PPQ as Tick * 4),
];

/// Translated name of an [`ARP_RATES`] duration (the table labels stay in French:
/// a `const` cannot follow the current language).
pub fn arp_rate_name(rate: Tick) -> Option<&'static str> {
    let fr = ARP_RATES.iter().find(|r| r.1 == rate)?.0;
    Some(match fr {
        "1/8 triolet" => tr!("1/8 triolet", "1/8 triplet"),
        "1/16 triolet" => tr!("1/16 triolet", "1/16 triplet"),
        other => other,
    })
}

pub const ARP_MAX_HELD: usize = 32;
pub const ARP_POOL: usize = 256;

/// Held notes → pool of notes to arpeggiate. Fixed arrays: usable on the audio thread.
#[derive(Clone, Copy)]
pub struct ArpPool {
    pub keys: [(u8, u8); ARP_POOL],
    pub n: usize,
}

impl Default for ArpPool {
    fn default() -> Self {
        ArpPool { keys: [(0, 0); ARP_POOL], n: 0 }
    }
}

impl ArpPool {
    /// `held` = (key, velocity) in press order.
    pub fn build(&mut self, held: &[(u8, u8)], p: &ArpParams) {
        self.n = 0;
        let octaves = p.octaves.clamp(1, 4);
        for oct in 0..octaves {
            for &(key, vel) in held {
                for &iv in p.chord.intervals() {
                    let k = key as u16 + iv as u16 + 12 * oct as u16;
                    if k <= 127 && self.n < ARP_POOL {
                        self.keys[self.n] = (k as u8, vel);
                        self.n += 1;
                    }
                }
            }
        }
        if p.direction != ArpDirection::AsPlayed {
            // Sort by key (insertion sort: no allocation), then dedup.
            let s = &mut self.keys[..self.n];
            for i in 1..s.len() {
                let mut j = i;
                while j > 0 && s[j - 1].0 > s[j].0 {
                    s.swap(j - 1, j);
                    j -= 1;
                }
            }
            let mut w = 0;
            for r in 0..self.n {
                if w == 0 || self.keys[w - 1].0 != self.keys[r].0 {
                    self.keys[w] = self.keys[r];
                    w += 1;
                }
            }
            self.n = w;
        }
    }

    /// Index into the pool for step `step`. `rand` is used for the random direction.
    /// `None` for the Chord direction (all notes) or if the pool is empty.
    pub fn index(&self, step: u64, p: &ArpParams, rand: u32) -> Option<usize> {
        let n = self.n as u64;
        if n == 0 || p.direction == ArpDirection::Chord {
            return None;
        }
        let i = step / p.repeat.max(1) as u64;
        let idx = match p.direction {
            ArpDirection::Up | ArpDirection::AsPlayed => i % n,
            ArpDirection::Down => n - 1 - i % n,
            ArpDirection::UpDown | ArpDirection::DownUp => {
                let j = if n < 2 {
                    0
                } else {
                    let period = 2 * n - 2;
                    let j = i % period;
                    if j < n { j } else { period - j }
                };
                if p.direction == ArpDirection::DownUp { n - 1 - j } else { j }
            }
            ArpDirection::UpDownInclusive => {
                let period = 2 * n;
                let j = i % period;
                if j < n { j } else { period - 1 - j }
            }
            ArpDirection::Random => rand as u64 % n,
            ArpDirection::Chord => unreachable!(),
        };
        Some(idx as usize)
    }
}

/// Deterministic pseudo-random generator (xorshift32).
pub fn xorshift(state: &mut u32) -> u32 {
    let mut x = (*state).max(1);
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    *state = x;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seq(held: &[(u8, u8)], p: &ArpParams, steps: u64) -> Vec<u8> {
        let mut pool = ArpPool::default();
        pool.build(held, p);
        (0..steps).map(|s| pool.keys[pool.index(s, p, 0).unwrap()].0).collect()
    }

    #[test]
    fn directions() {
        let held = [(64, 100), (60, 100), (67, 100)];
        let mut p = ArpParams::default();
        assert_eq!(seq(&held, &p, 6), vec![60, 64, 67, 60, 64, 67]);
        p.direction = ArpDirection::Down;
        assert_eq!(seq(&held, &p, 4), vec![67, 64, 60, 67]);
        p.direction = ArpDirection::UpDown;
        assert_eq!(seq(&held, &p, 6), vec![60, 64, 67, 64, 60, 64]);
        p.direction = ArpDirection::DownUp;
        assert_eq!(seq(&held, &p, 5), vec![67, 64, 60, 64, 67]);
        p.direction = ArpDirection::UpDownInclusive;
        assert_eq!(seq(&held, &p, 7), vec![60, 64, 67, 67, 64, 60, 60]);
        p.direction = ArpDirection::AsPlayed;
        assert_eq!(seq(&held, &p, 4), vec![64, 60, 67, 64]);
    }

    #[test]
    fn octaves_chord_repeat() {
        let mut p = ArpParams { octaves: 2, chord: ArpChord::Major, repeat: 2, ..Default::default() };
        let held = [(60, 90)];
        assert_eq!(seq(&held, &p, 8), vec![60, 60, 64, 64, 67, 67, 72, 72]);
        p.repeat = 1;
        // Duplicates removed (60+12 = 72 already present via the octave).
        let held = [(60, 90), (72, 90)];
        p.chord = ArpChord::None;
        assert_eq!(seq(&held, &p, 4), vec![60, 72, 84, 60]);
    }

    #[test]
    fn chord_direction_has_no_index() {
        let p = ArpParams { direction: ArpDirection::Chord, ..Default::default() };
        let mut pool = ArpPool::default();
        pool.build(&[(60, 100)], &p);
        assert_eq!(pool.index(0, &p, 0), None);
        assert_eq!(pool.n, 1);
    }
}
