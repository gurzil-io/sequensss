//! WAV export (offline render) and MIDI export (`.mid`, one track per channel).

use crate::{IoError, Result};
use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use seq_engine::graph::swing_offset;
use seq_i18n::trf;
use seq_model::*;
use std::path::Path;

/// Writes interleaved stereo audio as 16-bit WAV.
pub fn write_wav(path: &Path, interleaved: &[f32], sample_rate: u32) -> Result<()> {
    let spec = hound::WavSpec { channels: 2, sample_rate, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec)?;
    for &x in interleaved {
        w.write_sample((x.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16)?;
    }
    w.finalize()?;
    Ok(())
}

/// Absolute note (song ticks).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AbsNote {
    pub start: Tick,
    pub end: Tick,
    pub key: u8,
    pub vel: u8,
}

/// A channel's notes over the whole song (same rules as the engine: clip repetition and
/// truncation, swing). With no clips, the `fallback` pattern is played once.
pub fn flatten_channel(p: &Project, channel: ChannelId, fallback: PatternId) -> Vec<AbsNote> {
    let mut out = Vec::new();
    let mut emit = |pat: &Pattern, at: Tick, clip_len: Tick| {
        let Some(notes) = pat.notes.get(&channel) else { return };
        let plen = pat.length.max(1);
        let mut rep = 0;
        while rep * plen < clip_len {
            for n in notes {
                if n.start >= plen || n.velocity == 0 {
                    continue;
                }
                let local = rep * plen + (n.start + swing_offset(n.start, p.swing)).min(plen - 1);
                if local >= clip_len {
                    continue;
                }
                let end = (local + n.len.max(1)).min(rep * plen + plen).min(clip_len);
                out.push(AbsNote { start: at + local, end: at + end, key: n.key.min(127), vel: n.velocity.min(127) });
            }
            rep += 1;
        }
    };
    if p.arrangement.clips.is_empty() {
        if let Some(pat) = p.pattern(fallback) {
            emit(pat, 0, pat.length);
        }
    } else {
        for c in &p.arrangement.clips {
            if let Some(pat) = p.pattern(c.pattern) {
                emit(pat, c.start, c.len);
            }
        }
    }
    out.sort_by_key(|n| (n.start, n.key));
    out
}

pub fn write_midi(path: &Path, p: &Project, fallback: PatternId) -> Result<()> {
    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ as u16))));

    // Track 0: tempo + time signature.
    let us_per_q = (60_000_000.0 / p.tempo_bpm.max(1.0)).round() as u32;
    let den_pow = (p.time_sig.1.max(1) as f32).log2().round() as u8;
    smf.tracks.push(vec![
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us_per_q))) },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(p.time_sig.0, den_pow, 24, 8)),
        },
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ]);

    let mut next_melodic = 0u8;
    for ch in &p.channels {
        let notes = flatten_channel(p, ch.id, fallback);
        if notes.is_empty() {
            continue;
        }
        // MIDI channel: the MIDI out channel's own; drums on 10; synths on the following ones.
        let midi_ch = match ch.kind {
            ChannelKind::MidiOut { channel, .. } => channel.min(15),
            ChannelKind::Sampler { .. } => 9,
            ChannelKind::Synth(_) => {
                let c = next_melodic;
                next_melodic = (next_melodic + 1) % 16;
                if next_melodic == 9 {
                    next_melodic = 10;
                }
                c
            }
        };
        // (tick, 0 = off / 1 = on, key, vel): at equal ticks, offs sort before ons.
        let mut evs: Vec<(Tick, u8, u8, u8)> = Vec::with_capacity(notes.len() * 2);
        for n in &notes {
            evs.push((n.start, 1, n.key, n.vel.max(1)));
            evs.push((n.end, 0, n.key, 0));
        }
        evs.sort();
        let mut track = vec![TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(ch.name.as_bytes())),
        }];
        let mut last = 0;
        for (t, on, key, vel) in evs {
            let message = if on == 1 {
                MidiMessage::NoteOn { key: u7::new(key), vel: u7::new(vel) }
            } else {
                MidiMessage::NoteOff { key: u7::new(key), vel: u7::new(0) }
            };
            let delta = (t - last).min(u28::max_value().as_int() as u64) as u32;
            track.push(TrackEvent { delta: u28::new(delta), kind: TrackEventKind::Midi { channel: u4::new(midi_ch), message } });
            last = t;
        }
        track.push(TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) });
        smf.tracks.push(track);
    }
    smf.save(path).map_err(|e| IoError::Other(trf!("MIDI : {e}", "MIDI: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_repeats_and_truncates() {
        let mut p = Project::from_kit(0);
        let pat = p.patterns[0].id;
        let kick = p.channels[0].id;
        let bar = p.bar_len();
        // 1.5-bar clip: the kick (4 beats) plays 4 + 2 times.
        p.arrangement.clips.push(Clip { pattern: pat, track: 0, start: bar, len: bar + bar / 2 });
        let n = flatten_channel(&p, kick, pat);
        assert_eq!(n.len(), 6);
        assert_eq!(n[0].start, bar);
        assert!(n.iter().all(|x| x.end <= bar + bar + bar / 2));
    }

    #[test]
    fn midi_file_parses_back() {
        let p = Project::from_kit(3);
        let dir = std::env::temp_dir().join("sequensss-test-mid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.mid");
        write_midi(&path, &p, p.patterns[0].id).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        // Tempo track + one track per non-empty channel (kick, snare, hat, bass).
        assert_eq!(smf.tracks.len(), 5);
        let note_ons = smf.tracks[1]
            .iter()
            .filter(|e| matches!(e.kind, TrackEventKind::Midi { message: MidiMessage::NoteOn { .. }, .. }))
            .count();
        assert_eq!(note_ons, 4);
    }

    #[test]
    fn wav_written() {
        let dir = std::env::temp_dir().join("sequensss-test-wavout");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("o.wav");
        write_wav(&path, &[0.0, 0.5, -0.5, 1.0], 48_000).unwrap();
        let r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec().channels, 2);
        assert_eq!(r.len(), 4);
    }
}
