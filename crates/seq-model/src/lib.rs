//! Project data. Pure: serde, no I/O, no threads.

use seq_i18n::tr;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub mod arp;
mod cow;
mod kits;
pub use arp::{ARP_RATES, ArpChord, ArpDirection, ArpParams, arp_rate_name};
pub use cow::CowArc;
pub use kits::{KITS, Kit};

/// Ticks per quarter note.
pub const PPQ: u32 = 960;
/// Length of one step sequencer step (sixteenth note).
pub const STEP: Tick = (PPQ / 4) as Tick;
/// Maximum number of channels the engine handles.
pub const MAX_CHANNELS: usize = 64;
/// File format version.
pub const FORMAT_VERSION: u32 = 1;

pub type Tick = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PatternId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub version: u32,
    pub tempo_bpm: f64,
    pub time_sig: (u8, u8),
    /// 0.0..=1.0
    pub swing: f32,
    /// Channels and patterns are shared between snapshots (undo, engine) until modified.
    pub channels: Vec<CowArc<Channel>>,
    pub patterns: Vec<CowArc<Pattern>>,
    pub arrangement: Arrangement,
    /// Next free id (shared by channels / patterns). Never reused.
    pub next_id: u32,
    /// Project MIDI outputs (OUT 1, OUT 2…): port name, or `None` if not connected.
    #[serde(default = "default_midi_outs")]
    pub midi_outs: Vec<Option<String>>,
}

fn default_midi_outs() -> Vec<Option<String>> {
    vec![None]
}

/// Maximum number of MIDI outputs per project.
pub const MAX_MIDI_OUTS: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub kind: ChannelKind,
    pub volume: f32,
    /// -1.0 (left) ..= 1.0 (right)
    pub pan: f32,
    pub mute: bool,
    pub solo: bool,
    /// Key used by the step sequencer.
    pub default_key: u8,
    /// Choke group (0 = none). A note on a channel cuts the others in the same group.
    pub choke: u8,
    /// Real-time arpeggiator (missing from older files → disabled).
    #[serde(default)]
    pub arp: ArpParams,
    /// Settings of the synth / MIDI out the channel had before its instrument was
    /// replaced (at most one of each), restored when switching back.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub previous_kinds: Vec<ChannelKind>,
}

impl Channel {
    /// `kind`, or the settings this channel last had for the same kind of instrument.
    pub fn recall(&self, kind: ChannelKind) -> ChannelKind {
        self.previous_kinds.iter().find(|k| k.same_kind(&kind)).cloned().unwrap_or(kind)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ChannelKind {
    Sampler { source: SampleSource },
    Synth(SynthParams),
    /// MIDI channel (0..=15) on output `out` (index into `Project::midi_outs`).
    MidiOut {
        channel: u8,
        #[serde(default)]
        out: u8,
    },
}

impl ChannelKind {
    pub fn label(&self) -> &'static str {
        match self {
            ChannelKind::Sampler { .. } => "Sampler",
            ChannelKind::Synth(_) => "Synth",
            ChannelKind::MidiOut { .. } => "MIDI out",
        }
    }

    /// Same kind of instrument (sampler, synth or MIDI out), whatever the settings.
    pub fn same_kind(&self, other: &ChannelKind) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// Name given to a new channel of this kind.
    pub fn default_name(&self) -> String {
        match self {
            ChannelKind::Sampler { source: SampleSource::Builtin(d) } => d.name().to_string(),
            ChannelKind::Sampler { source: SampleSource::File(path) } => {
                path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or("Sample".into())
            }
            ChannelKind::Synth(_) => "Synth".into(),
            ChannelKind::MidiOut { .. } => "MIDI out".into(),
        }
    }

    /// `name` is the default name of this kind, in any language (project created
    /// before a language change).
    pub fn is_default_name(&self, name: &str) -> bool {
        seq_i18n::Lang::ALL.into_iter().any(|l| seq_i18n::with_lang(l, || self.default_name()) == name)
    }

    /// Step sequencer key of a new channel of this kind (samples: original pitch).
    pub fn default_key(&self) -> u8 {
        match self {
            ChannelKind::Synth(_) => 48,
            _ => 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SampleSource {
    /// Drum sound synthesized at startup (no file needed).
    Builtin(Drum),
    File(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Drum {
    Kick,
    Kick808,
    KickHard,
    Snare,
    SnareTight,
    Clap,
    HatClosed,
    HatOpen,
    Rim,
    TomLow,
    TomHigh,
    Cowbell,
}

impl Drum {
    pub const ALL: [Drum; 12] = [
        Drum::Kick,
        Drum::Kick808,
        Drum::KickHard,
        Drum::Snare,
        Drum::SnareTight,
        Drum::Clap,
        Drum::HatClosed,
        Drum::HatOpen,
        Drum::Rim,
        Drum::TomLow,
        Drum::TomHigh,
        Drum::Cowbell,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Drum::Kick => "Kick",
            Drum::Kick808 => "Kick 808",
            Drum::KickHard => "Kick Hard",
            Drum::Snare => "Snare",
            Drum::SnareTight => "Snare Tight",
            Drum::Clap => "Clap",
            Drum::HatClosed => tr!("Hat fermé", "Closed hat"),
            Drum::HatOpen => tr!("Hat ouvert", "Open hat"),
            Drum::Rim => "Rim",
            Drum::TomLow => tr!("Tom grave", "Low tom"),
            Drum::TomHigh => tr!("Tom aigu", "High tom"),
            Drum::Cowbell => "Cowbell",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wave {
    Saw,
    Square,
    Sine,
    Triangle,
}

impl Wave {
    pub const ALL: [Wave; 4] = [Wave::Saw, Wave::Square, Wave::Sine, Wave::Triangle];
    pub fn name(self) -> &'static str {
        match self {
            Wave::Saw => "Saw",
            Wave::Square => "Square",
            Wave::Sine => "Sine",
            Wave::Triangle => "Triangle",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SynthParams {
    pub osc1: Wave,
    pub osc2: Wave,
    /// Oscillator 2 detune, in semitones.
    pub osc2_detune: f32,
    /// 0.0 = osc1 only, 1.0 = osc2 only.
    pub osc_mix: f32,
    /// Hz
    pub cutoff: f32,
    /// 0.0..=1.0
    pub resonance: f32,
    /// Envelope modulation of the cutoff, 0.0..=1.0 (up to +4 octaves).
    pub filter_env: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub gain: f32,
}

impl Default for SynthParams {
    fn default() -> Self {
        SynthParams {
            osc1: Wave::Saw,
            osc2: Wave::Square,
            osc2_detune: 0.07,
            osc_mix: 0.35,
            cutoff: 900.0,
            resonance: 0.3,
            filter_env: 0.4,
            attack: 0.005,
            decay: 0.25,
            sustain: 0.6,
            release: 0.15,
            gain: 0.6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    pub length: Tick,
    /// Notes per channel. The step sequencer and piano roll edit the same notes.
    pub notes: BTreeMap<ChannelId, Vec<Note>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub start: Tick,
    pub len: Tick,
    pub key: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Arrangement {
    pub tracks: u16,
    pub clips: Vec<Clip>,
}

/// A clip plays its pattern from the start; if `len` exceeds the pattern length,
/// the pattern loops; otherwise it is truncated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Clip {
    pub pattern: PatternId,
    pub track: u16,
    pub start: Tick,
    pub len: Tick,
}

impl Default for Project {
    fn default() -> Self {
        Self::empty()
    }
}

impl Project {
    /// Empty project with a one-bar pattern.
    pub fn empty() -> Self {
        let mut p = Project {
            version: FORMAT_VERSION,
            tempo_bpm: 120.0,
            time_sig: (4, 4),
            swing: 0.0,
            channels: Vec::new(),
            patterns: Vec::new(),
            arrangement: Arrangement { tracks: 8, clips: Vec::new() },
            next_id: 1,
            midi_outs: default_midi_outs(),
        };
        let len = p.bar_len();
        p.add_pattern("Pattern 1", len);
        p
    }

    pub fn alloc_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Length of a bar in ticks.
    pub fn bar_len(&self) -> Tick {
        let (num, den) = self.time_sig;
        PPQ as Tick * 4 * num.max(1) as Tick / den.max(1) as Tick
    }

    /// Length of a beat in ticks.
    pub fn beat_len(&self) -> Tick {
        PPQ as Tick * 4 / self.time_sig.1.max(1) as Tick
    }

    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id).map(|c| &**c)
    }

    pub fn channel_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id).map(CowArc::make_mut)
    }

    /// Whether the playlist has a clip that plays something: a non-zero length and an
    /// existing pattern. Otherwise playback and export fall back to the current pattern.
    pub fn has_song(&self) -> bool {
        self.arrangement.clips.iter().any(|c| c.len > 0 && self.pattern(c.pattern).is_some())
    }

    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id).map(|p| &**p)
    }

    pub fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id).map(CowArc::make_mut)
    }

    pub fn add_pattern(&mut self, name: &str, length: Tick) -> PatternId {
        let id = PatternId(self.alloc_id());
        self.patterns.push(CowArc::new(Pattern {
            id,
            name: name.to_string(),
            length,
            notes: BTreeMap::new(),
        }));
        id
    }

    pub fn add_channel(&mut self, name: &str, kind: ChannelKind) -> Option<ChannelId> {
        if self.channels.len() >= MAX_CHANNELS {
            return None;
        }
        let id = ChannelId(self.alloc_id());
        self.channels.push(CowArc::new(Channel {
            id,
            name: name.to_string(),
            kind,
            volume: 0.8,
            pan: 0.0,
            mute: false,
            solo: false,
            default_key: 60,
            choke: 0,
            arp: ArpParams::default(),
            previous_kinds: Vec::new(),
        }));
        Some(id)
    }

    /// End of the last clip in the playlist.
    pub fn song_length(&self) -> Tick {
        self.arrangement
            .clips
            .iter()
            .map(|c| c.start + c.len)
            .max()
            .unwrap_or(0)
    }
}

/// MIDI note name (60 = C4).
pub fn key_name(key: u8) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let octave = key as i32 / 12 - 1;
    format!("{}{}", NAMES[key as usize % 12], octave)
}

pub fn is_black_key(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ron_round_trip() {
        for kit in 0..KITS.len() {
            let p = Project::from_kit(kit);
            let s = ron::ser::to_string_pretty(&p, ron::ser::PrettyConfig::default()).unwrap();
            let back: Project = ron::from_str(&s).unwrap();
            assert_eq!(p, back);
        }
    }

    #[test]
    fn old_files_without_arp_still_load() {
        let p = Project::from_kit(0);
        let s = ron::ser::to_string_pretty(&p, ron::ser::PrettyConfig::default()).unwrap();
        // Simulate a v1 file without the `arp` field.
        let mut old = String::new();
        let mut skip = 0;
        for line in s.lines() {
            if line.trim_start().starts_with("arp:") {
                skip = 1;
                continue;
            }
            if skip > 0 {
                if line.trim_start().starts_with("),") {
                    skip = 0;
                }
                continue;
            }
            old.push_str(line);
            old.push('\n');
        }
        assert!(!old.contains("arp:"));
        let back: Project = ron::from_str(&old).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn bar_len_4_4() {
        let p = Project::empty();
        assert_eq!(p.bar_len(), 3840);
        assert_eq!(p.beat_len(), 960);
    }

    #[test]
    fn ids_unique() {
        let mut p = Project::empty();
        let a = p.add_channel("a", ChannelKind::MidiOut { channel: 0, out: 0 }).unwrap();
        let b = p.add_pattern("b", 100);
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name(60), "C4");
        assert_eq!(key_name(69), "A4");
        assert!(is_black_key(61));
    }
}
