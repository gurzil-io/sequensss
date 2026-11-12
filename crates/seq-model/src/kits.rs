//! Default kits: a ready-to-play project with a demo beat.

use crate::{ChannelKind, Drum, Note, Project, STEP, SampleSource, SynthParams, Wave};

pub struct Kit {
    pub name: &'static str,
    /// (sound, choke group, demo steps: 'x' = accent, 'o' = note, anything else = empty)
    pub drums: &'static [(Drum, u8, &'static str)],
    pub with_synths: bool,
}

pub const KITS: [Kit; 4] = [
    Kit {
        name: "Basic",
        drums: &[
            (Drum::Kick, 0, "x...x...x...x..."),
            (Drum::Snare, 0, "....x.......x..."),
            (Drum::HatClosed, 1, "o.o.o.o.o.o.o..."),
            (Drum::HatOpen, 1, "..............x."),
            (Drum::Clap, 0, "................"),
        ],
        with_synths: false,
    },
    Kit {
        name: "808",
        drums: &[
            (Drum::Kick808, 0, "x.....x...x....."),
            (Drum::Clap, 0, "....x.......x..."),
            (Drum::Rim, 0, "..o.....o..o...."),
            (Drum::HatClosed, 1, "oooooooooooooo.."),
            (Drum::HatOpen, 1, "..............x."),
            (Drum::Cowbell, 0, "................"),
        ],
        with_synths: false,
    },
    Kit {
        name: "Hard",
        drums: &[
            (Drum::KickHard, 0, "x..x..x...x..x.."),
            (Drum::SnareTight, 0, "....x.......x..o"),
            (Drum::Clap, 0, "....x.......x..."),
            (Drum::HatClosed, 0, "o.oxo.oxo.oxo.ox"),
            (Drum::TomLow, 0, "................"),
            (Drum::TomHigh, 0, "................"),
        ],
        with_synths: false,
    },
    Kit {
        name: "Synth starter",
        drums: &[
            (Drum::Kick, 0, "x...x...x...x..."),
            (Drum::Snare, 0, "....x.......x..."),
            (Drum::HatClosed, 0, "..o...o...o...o."),
        ],
        with_synths: true,
    },
];

impl Project {
    /// New project from a kit (index into [`KITS`]).
    pub fn from_kit(index: usize) -> Project {
        let kit = &KITS[index.min(KITS.len() - 1)];
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        for &(drum, choke, steps) in kit.drums {
            let id = p
                .add_channel(
                    drum.name(),
                    ChannelKind::Sampler { source: SampleSource::Builtin(drum) },
                )
                .expect("kit too large");
            p.channel_mut(id).unwrap().choke = choke;
            let notes: Vec<Note> = steps
                .chars()
                .enumerate()
                .filter_map(|(i, c)| {
                    let velocity = match c {
                        'x' => 115,
                        'o' => 85,
                        _ => return None,
                    };
                    Some(Note { start: i as u64 * STEP, len: STEP, key: 60, velocity })
                })
                .collect();
            if !notes.is_empty() {
                p.pattern_mut(pat).unwrap().notes.insert(id, notes);
            }
        }
        if kit.with_synths {
            let bass = p
                .add_channel(seq_i18n::tr!("Basse", "Bass"), ChannelKind::Synth(SynthParams::default()))
                .unwrap();
            p.channel_mut(bass).unwrap().default_key = 36;
            let line = [(0, 36, 3), (4, 36, 2), (7, 48, 1), (8, 39, 3), (12, 41, 2), (14, 43, 2)];
            let notes = line
                .iter()
                .map(|&(s, k, l)| Note { start: s * STEP, len: l * STEP, key: k, velocity: 100 })
                .collect();
            p.pattern_mut(pat).unwrap().notes.insert(bass, notes);

            let lead = p
                .add_channel(
                    "Lead",
                    ChannelKind::Synth(SynthParams {
                        osc1: Wave::Square,
                        osc2: Wave::Saw,
                        osc2_detune: 12.0,
                        osc_mix: 0.3,
                        cutoff: 2500.0,
                        resonance: 0.2,
                        filter_env: 0.2,
                        attack: 0.01,
                        decay: 0.3,
                        sustain: 0.5,
                        release: 0.3,
                        gain: 0.35,
                    }),
                )
                .unwrap();
            p.channel_mut(lead).unwrap().default_key = 72;
        }
        p
    }
}
