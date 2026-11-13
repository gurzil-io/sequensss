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
    if !p.has_song() {
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

/// Notes sorted by start. A note still sounding when the same key starts again ends there
/// (a MIDI synth would cut it anyway, and on/on/off/off would cut the second one short).
/// Two notes starting together on the same key keep the longest.
fn without_overlaps(notes: Vec<AbsNote>) -> Vec<AbsNote> {
    let mut out: Vec<AbsNote> = Vec::with_capacity(notes.len());
    let mut last: [Option<usize>; 128] = [None; 128];
    for n in notes {
        let k = n.key as usize;
        if let Some(i) = last[k] {
            let prev = &mut out[i];
            if prev.start == n.start {
                prev.end = prev.end.max(n.end);
                prev.vel = prev.vel.max(n.vel);
                continue;
            }
            prev.end = prev.end.min(n.start);
        }
        last[k] = Some(out.len());
        out.push(n);
    }
    out
}

pub fn write_midi(path: &Path, p: &Project, fallback: PatternId) -> Result<()> {
    let mut smf = Smf::new(Header::new(Format::Parallel, Timing::Metrical(u15::new(PPQ as u16))));

    // Track 0: tempo + time signature.
    // Same range as the engine (`compile`), so the file plays at the exported tempo and
    // always fits in 24 bits.
    let us_per_q = (60_000_000.0 / p.tempo_bpm.clamp(10.0, 999.0)).round() as u32;
    let den_pow = (p.time_sig.1.max(1) as f32).log2().round() as u8;
    smf.tracks.push(vec![
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us_per_q))) },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TimeSignature(p.time_sig.0, den_pow, 24, 8)),
        },
        TrackEvent { delta: u28::new(0), kind: TrackEventKind::Meta(MetaMessage::EndOfTrack) },
    ]);

    // What is heard: muted channels, and channels silenced by a solo, are left out.
    let any_solo = p.channels.iter().any(|c| c.solo);
    let tracks: Vec<(&Channel, Vec<AbsNote>)> = p
        .channels
        .iter()
        .filter(|ch| !ch.mute && (!any_solo || ch.solo))
        .map(|ch| (&**ch, without_overlaps(flatten_channel(p, ch.id, fallback))))
        .filter(|(_, notes)| !notes.is_empty())
        .collect();
    // Synths take the channels left free by the drums (10) and the MIDI out tracks, in
    // order, cycling when there are more synths than free channels.
    let mut taken = [false; 16];
    taken[9] = true;
    for (ch, _) in &tracks {
        if let ChannelKind::MidiOut { channel, .. } = ch.kind {
            taken[channel.min(15) as usize] = true;
        }
    }
    let mut free: Vec<u8> = (0..16).filter(|&c| !taken[c as usize]).collect();
    if free.is_empty() {
        free = (0..16).filter(|&c| c != 9).collect();
    }
    let mut synths = 0;

    for (ch, notes) in tracks {
        // MIDI channel: the MIDI out channel's own; drums on 10; synths on the free ones.
        let midi_ch = match ch.kind {
            ChannelKind::MidiOut { channel, .. } => channel.min(15),
            ChannelKind::Sampler { .. } => 9,
            ChannelKind::Synth(_) => {
                synths += 1;
                free[(synths - 1) % free.len()]
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

    fn tmp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sequensss-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Renders and writes like the app does (`export_wav_to`), then reads the file back.
    fn export_and_read(p: &Project, mode: seq_engine::PlayMode, path: &Path) -> (hound::WavSpec, Vec<i16>) {
        let sr = 48_000;
        let cache = crate::instruments::SampleCache::default();
        let slots = p.channels.iter().map(|c| (c.id, crate::instruments::build_slot(c, sr, &cache).unwrap())).collect();
        let audio = seq_engine::offline::render(seq_engine::graph::compile(p), slots, sr, mode, 2.0);
        write_wav(path, &audio, sr).unwrap();
        let mut r = hound::WavReader::open(path).unwrap();
        let spec = r.spec();
        (spec, r.samples::<i16>().map(|s| s.unwrap()).collect())
    }

    /// RMS (both channels) over [from, to) seconds.
    fn rms(samples: &[i16], sr: u32, from: f64, to: f64) -> f64 {
        let a = (from * sr as f64) as usize * 2;
        let b = (to * sr as f64) as usize * 2;
        let w = &samples[a..b];
        (w.iter().map(|&x| (x as f64 / 32768.0).powi(2)).sum::<f64>() / w.len() as f64).sqrt()
    }

    fn one_kick_project() -> (Project, PatternId) {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ch = p.add_channel("Kick", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) }).unwrap();
        // A single kick on beat 2 (0.5 s at 120 BPM).
        p.pattern_mut(pat).unwrap().notes.insert(ch, vec![Note { start: PPQ as Tick, len: STEP, key: 60, velocity: 127 }]);
        (p, pat)
    }

    #[test]
    fn wav_export_pattern_end_to_end() {
        let (p, pat) = one_kick_project();
        let path = tmp_dir("wav-e2e-pat").join("out.wav");
        let (spec, s) = export_and_read(&p, seq_engine::PlayMode::Pattern(pat), &path);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.channels, 2);
        assert_eq!(spec.bits_per_sample, 16);
        // 1 bar at 120 BPM = 2 s, + 2 s of tail.
        assert_eq!(s.len(), 4 * 48_000 * 2);
        assert!(rms(&s, 48_000, 0.0, 0.49) < 1e-4, "sound before the note");
        assert!(rms(&s, 48_000, 0.5, 0.6) > 0.05, "no sound on the note");
        assert!(rms(&s, 48_000, 3.5, 4.0) < 1e-4, "sound at the end of the tail");
        // The hit starts on time (within 2 ms).
        let first = s.iter().position(|x| x.unsigned_abs() > 300).unwrap() / 2;
        assert!((24_000..24_096).contains(&first), "first sound at frame {first}");
    }

    #[test]
    fn wav_export_song_end_to_end() {
        let (mut p, pat) = one_kick_project();
        let bar = p.bar_len();
        // Clip in bar 2 only: the song lasts 2 bars, the first one silent.
        p.arrangement.clips.push(Clip { pattern: pat, track: 3, start: bar, len: bar });
        let path = tmp_dir("wav-e2e-song").join("out.wav");
        let (spec, s) = export_and_read(&p, seq_engine::PlayMode::Song, &path);
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(s.len(), 6 * 48_000 * 2);
        assert!(rms(&s, 48_000, 0.0, 2.49) < 1e-4, "sound before the clip's note");
        assert!(rms(&s, 48_000, 2.5, 2.6) > 0.05, "no sound on the note");
        assert!(rms(&s, 48_000, 5.5, 6.0) < 1e-4, "sound at the end of the tail");
    }

    #[test]
    fn wav_16bit_conversion_clips() {
        let path = tmp_dir("wav-clip").join("o.wav");
        write_wav(&path, &[-2.0, 2.0, 1.0, -1.0, 0.5, -0.5, 1e9, -1e9], 44_100).unwrap();
        let mut r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec().sample_rate, 44_100);
        assert_eq!(r.spec().bits_per_sample, 16);
        let v: Vec<i16> = r.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(v, vec![-32767, 32767, 32767, -32767, 16384, -16384, 32767, -32767]);
    }

    /// (absolute tick, MIDI channel, is note on, key, velocity) for the MIDI events of a track.
    fn midi_events(track: &[TrackEvent]) -> Vec<(u64, u8, bool, u8, u8)> {
        let mut t = 0u64;
        let mut out = Vec::new();
        for e in track {
            t += e.delta.as_int() as u64;
            if let TrackEventKind::Midi { channel, message } = e.kind {
                match message {
                    MidiMessage::NoteOn { key, vel } => out.push((t, channel.as_int(), true, key.as_int(), vel.as_int())),
                    MidiMessage::NoteOff { key, vel } => out.push((t, channel.as_int(), false, key.as_int(), vel.as_int())),
                    _ => {}
                }
            }
        }
        out
    }

    fn track_name<'a>(track: &'a [TrackEvent<'a>]) -> &'a [u8] {
        track
            .iter()
            .find_map(|e| match e.kind {
                TrackEventKind::Meta(MetaMessage::TrackName(n)) => Some(n),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn midi_header_tempo_and_time_signature() {
        let mut p = Project::from_kit(0);
        p.tempo_bpm = 90.0;
        p.time_sig = (6, 8);
        let path = tmp_dir("mid-header").join("t.mid");
        write_midi(&path, &p, p.patterns[0].id).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.header.format, Format::Parallel);
        assert_eq!(smf.header.timing, Timing::Metrical(u15::new(960)));
        let metas: Vec<_> = smf.tracks[0].iter().map(|e| e.kind).collect();
        assert!(metas.contains(&TrackEventKind::Meta(MetaMessage::Tempo(u24::new(666_667)))));
        assert!(metas.contains(&TrackEventKind::Meta(MetaMessage::TimeSignature(6, 3, 24, 8))));
        assert!(smf.tracks[0].iter().all(|e| e.delta.as_int() == 0));
    }

    #[test]
    fn midi_tempo_never_wraps() {
        // A hand-edited / older file may hold any tempo; the engine plays it clamped to 10..=999 BPM.
        let mut p = Project::from_kit(0);
        p.tempo_bpm = 2.0;
        let path = tmp_dir("mid-slow").join("t.mid");
        write_midi(&path, &p, p.patterns[0].id).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let tempo = smf.tracks[0]
            .iter()
            .find_map(|e| match e.kind {
                TrackEventKind::Meta(MetaMessage::Tempo(t)) => Some(t.as_int()),
                _ => None,
            })
            .unwrap();
        // 60e6 / 2 = 30_000_000 does not fit in 24 bits and is masked to 13_222_784 (~4.5 BPM).
        // Expected: the tempo the engine actually plays (10 BPM).
        assert_eq!(tempo, 6_000_000);
    }

    #[test]
    fn midi_channel_per_kind() {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let drum = p.add_channel("Drum", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Snare) }).unwrap();
        let s1 = p.add_channel("Lead", ChannelKind::Synth(SynthParams::default())).unwrap();
        let _empty = p.add_channel("Empty", ChannelKind::Synth(SynthParams::default())).unwrap();
        let ext = p.add_channel("Ext", ChannelKind::MidiOut { channel: 5, out: 1 }).unwrap();
        let s2 = p.add_channel("Bass", ChannelKind::Synth(SynthParams::default())).unwrap();
        let hi = p.add_channel("Hi", ChannelKind::MidiOut { channel: 200, out: 0 }).unwrap();
        let note = Note { start: 0, len: STEP, key: 64, velocity: 100 };
        for ch in [drum, s1, ext, s2, hi] {
            p.pattern_mut(pat).unwrap().notes.insert(ch, vec![note]);
        }
        let path = tmp_dir("mid-chan").join("t.mid");
        write_midi(&path, &p, pat).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        // Tempo track + 5 non-empty channels (the empty synth has no track).
        assert_eq!(smf.tracks.len(), 6);
        let got: Vec<(&[u8], u8)> = smf.tracks[1..]
            .iter()
            .map(|t| {
                let ev = midi_events(t);
                assert_eq!(ev.len(), 2);
                assert!(ev.iter().all(|e| e.1 == ev[0].1));
                (track_name(t), ev[0].1)
            })
            .collect();
        // Drums on 10 (index 9), synths 0, 1 (the empty one takes no channel), MIDI out on its own (clamped to 15).
        assert_eq!(got, vec![(&b"Drum"[..], 9), (&b"Lead"[..], 0), (&b"Ext"[..], 5), (&b"Bass"[..], 1), (&b"Hi"[..], 15)]);
    }

    #[test]
    fn midi_synth_channels_skip_drum_channel() {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        for i in 0..18 {
            let ch = p.add_channel(&format!("S{i}"), ChannelKind::Synth(SynthParams::default())).unwrap();
            p.pattern_mut(pat).unwrap().notes.insert(ch, vec![Note { start: 0, len: STEP, key: 60, velocity: 90 }]);
        }
        let path = tmp_dir("mid-synths").join("t.mid");
        write_midi(&path, &p, pat).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let chans: Vec<u8> = smf.tracks[1..].iter().map(|t| midi_events(t)[0].1).collect();
        // 0..=8, 10..=15, then wraps around (still skipping 9).
        assert_eq!(chans, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15, 0, 1, 2]);
    }

    #[test]
    fn midi_synth_does_not_reuse_midi_out_channel() {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ext = p.add_channel("Ext", ChannelKind::MidiOut { channel: 0, out: 0 }).unwrap();
        let syn = p.add_channel("Lead", ChannelKind::Synth(SynthParams::default())).unwrap();
        for ch in [ext, syn] {
            p.pattern_mut(pat).unwrap().notes.insert(ch, vec![Note { start: 0, len: STEP, key: 60, velocity: 90 }]);
        }
        let path = tmp_dir("mid-collide").join("t.mid");
        write_midi(&path, &p, pat).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let ext_ch = midi_events(&smf.tracks[1])[0].1;
        let syn_ch = midi_events(&smf.tracks[2])[0].1;
        assert_eq!(ext_ch, 0);
        // Both instruments end up on MIDI channel 1 and merge in a DAW.
        assert_ne!(syn_ch, ext_ch);
    }

    #[test]
    fn midi_note_lengths_and_velocities() {
        let mut p = Project::empty();
        p.swing = 0.5;
        let pat = p.patterns[0].id;
        let bar = p.bar_len();
        let ch = p.add_channel("Lead", ChannelKind::Synth(SynthParams::default())).unwrap();
        p.pattern_mut(pat).unwrap().notes.insert(
            ch,
            vec![
                Note { start: 0, len: PPQ as Tick, key: 60, velocity: 1 },
                // Starts right where the previous one ends, same key: the off must come first.
                Note { start: PPQ as Tick, len: STEP, key: 60, velocity: 127 },
                // Odd step: swing delays it by STEP / 4, the length is kept.
                Note { start: STEP * 5, len: 100, key: 62, velocity: 64 },
                // Last step (odd, so swung too), runs past the end of the pattern: truncated.
                Note { start: bar - STEP, len: 10 * PPQ as Tick, key: 70, velocity: 90 },
                // Velocity 0: skipped; out-of-range velocity and key: clamped.
                Note { start: STEP * 8, len: STEP, key: 61, velocity: 0 },
                Note { start: STEP * 10, len: STEP, key: 200, velocity: 255 },
            ],
        );
        let path = tmp_dir("mid-notes").join("t.mid");
        write_midi(&path, &p, pat).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let ev = midi_events(&smf.tracks[1]);
        let q = PPQ as u64;
        let swung = STEP * 5 + STEP / 4;
        assert_eq!(
            ev,
            vec![
                (0, 0, true, 60, 1),
                (q, 0, false, 60, 0),
                (q, 0, true, 60, 127),
                (q + STEP, 0, false, 60, 0),
                (swung, 0, true, 62, 64),
                (swung + 100, 0, false, 62, 0),
                (STEP * 10, 0, true, 127, 127),
                (STEP * 11, 0, false, 127, 0),
                (bar - STEP + STEP / 4, 0, true, 70, 90),
                (bar, 0, false, 70, 0),
            ]
        );
    }

    #[test]
    fn midi_export_falls_back_to_pattern_when_clips_play_nothing() {
        let mut p = Project::from_kit(0);
        let pat = p.patterns[0].id;
        let kick = p.channels[0].id;
        let pattern_only = flatten_channel(&p, kick, pat);
        assert!(!pattern_only.is_empty());
        // A clip pointing to a missing pattern, and a zero-length clip.
        p.arrangement.clips.push(Clip { pattern: PatternId(999), track: 0, start: 0, len: p.bar_len() });
        p.arrangement.clips.push(Clip { pattern: pat, track: 1, start: 0, len: 0 });
        assert_eq!(flatten_channel(&p, kick, pat), pattern_only);
    }

    fn notes_of(p: &Project, path: &Path) -> Vec<Vec<(u64, bool, u8)>> {
        write_midi(path, p, p.patterns[0].id).unwrap();
        let bytes = std::fs::read(path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        smf.tracks[1..]
            .iter()
            .map(|t| {
                let mut at = 0u64;
                let mut v = Vec::new();
                for e in t {
                    at += e.delta.as_int() as u64;
                    match e.kind {
                        TrackEventKind::Midi { message: MidiMessage::NoteOn { key, .. }, .. } => v.push((at, true, key.as_int())),
                        TrackEventKind::Midi { message: MidiMessage::NoteOff { key, .. }, .. } => v.push((at, false, key.as_int())),
                        _ => {}
                    }
                }
                v
            })
            .collect()
    }

    #[test]
    fn midi_overlapping_notes_on_same_key_do_not_cut_each_other() {
        let dir = std::env::temp_dir().join("sequensss-test-mid-overlap");
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ch = p.add_channel("s", ChannelKind::Synth(SynthParams::default())).unwrap();
        let n = |start, len| Note { start, len, key: 60, velocity: 100 };
        // 0..960 overlaps 480..1440; two identical starts at 1920 (lengths 240 and 480).
        p.pattern_mut(pat).unwrap().notes.insert(ch, vec![n(0, 960), n(480, 960), n(1920, 240), n(1920, 480)]);
        let tracks = notes_of(&p, &dir.join("o.mid"));
        assert_eq!(
            tracks[0],
            vec![(0, true, 60), (480, false, 60), (480, true, 60), (1440, false, 60), (1920, true, 60), (2400, false, 60)]
        );
    }

    #[test]
    fn midi_export_leaves_out_muted_and_unsoloed_channels() {
        let dir = std::env::temp_dir().join("sequensss-test-mid-mute");
        std::fs::create_dir_all(&dir).unwrap();
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ids: Vec<ChannelId> = (0..3).map(|_| p.add_channel("s", ChannelKind::Synth(SynthParams::default())).unwrap()).collect();
        for (i, &id) in ids.iter().enumerate() {
            let key = 60 + i as u8;
            p.pattern_mut(pat).unwrap().notes.insert(id, vec![Note { start: 0, len: 480, key, velocity: 100 }]);
        }
        let keys = |p: &Project, name: &str| -> Vec<u8> {
            notes_of(p, &dir.join(name)).iter().map(|t| t[0].2).collect()
        };
        assert_eq!(keys(&p, "all.mid"), vec![60, 61, 62]);
        p.channel_mut(ids[0]).unwrap().mute = true;
        assert_eq!(keys(&p, "mute.mid"), vec![61, 62]);
        p.channel_mut(ids[2]).unwrap().solo = true;
        assert_eq!(keys(&p, "solo.mid"), vec![62]);
    }

    #[test]
    fn midi_song_mode_several_clips_and_tracks() {
        let mut p = Project::empty();
        let bar = p.bar_len();
        let a = p.patterns[0].id;
        let b = p.add_pattern("B", bar / 2);
        let kick = p.add_channel("Kick", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) }).unwrap();
        let bass = p.add_channel("Bass", ChannelKind::Synth(SynthParams::default())).unwrap();
        p.pattern_mut(a).unwrap().notes.insert(kick, vec![Note { start: 0, len: STEP, key: 36, velocity: 100 }]);
        p.pattern_mut(b).unwrap().notes.insert(
            bass,
            vec![Note { start: 0, len: PPQ as Tick, key: 40, velocity: 80 }, Note { start: PPQ as Tick, len: 4 * PPQ as Tick, key: 43, velocity: 70 }],
        );
        // Kick: bars 0 and 2 (track 0). Bass: from bar 1, 1.5 bars long (track 1) -> pattern B (half a bar) plays 3 times.
        p.arrangement.clips.push(Clip { pattern: a, track: 0, start: 2 * bar, len: bar });
        p.arrangement.clips.push(Clip { pattern: a, track: 0, start: 0, len: bar });
        p.arrangement.clips.push(Clip { pattern: b, track: 1, start: bar, len: bar + bar / 2 - PPQ as Tick / 2 });
        let path = tmp_dir("mid-song").join("t.mid");
        // The fallback pattern is ignored in song mode.
        write_midi(&path, &p, b).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        assert_eq!(smf.tracks.len(), 3);
        let k = midi_events(&smf.tracks[1]);
        let starts: Vec<u64> = k.iter().filter(|e| e.2).map(|e| e.0).collect();
        assert_eq!(starts, vec![0, 2 * bar]);
        assert!(k.iter().all(|e| e.1 == 9));

        let q = PPQ as u64;
        let half = bar / 2;
        let clip_end = bar + bar + half - q / 2;
        let b_ev = midi_events(&smf.tracks[2]);
        let notes: Vec<(u64, u64, u8)> = b_ev
            .iter()
            .filter(|e| e.2)
            .map(|on| {
                let off = b_ev.iter().find(|e| !e.2 && e.3 == on.3 && e.0 > on.0).unwrap();
                (on.0, off.0, on.3)
            })
            .collect();
        assert_eq!(
            notes,
            vec![
                (bar, bar + q, 40),
                // The 4-beat note is cut at the end of the half-bar pattern.
                (bar + q, bar + half, 43),
                (bar + half, bar + half + q, 40),
                (bar + half + q, bar + 2 * half, 43),
                (bar + 2 * half, bar + 2 * half + q, 40),
                // Last repetition: cut by the clip end.
                (bar + 2 * half + q, clip_end, 43),
            ]
        );
        assert!(b_ev.iter().all(|e| e.1 == 0));
    }
}
