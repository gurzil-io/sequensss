//! What the app sends to the engine: instrument slot changes, export region.

use seq_model::*;
use std::collections::HashMap;

/// Instrument slot change for a channel, from the kinds already sent to the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum SlotUpdate {
    /// Channel deleted.
    Remove(ChannelId),
    /// Synth → synth: only the parameters changed (the voices keep playing).
    SynthParams(ChannelId, SynthParams),
    /// New channel or kind/source changed: the slot is rebuilt.
    Rebuild(ChannelId),
}

/// Slot changes between `sent` (kinds last sent to the engine) and `channels`: removals
/// first, then one update per new or changed channel, in channel order (at most
/// `MAX_CHANNELS` channels). Volume, pan, mute… only change the graph, not the slots.
pub fn slot_updates(sent: &HashMap<ChannelId, ChannelKind>, channels: &[Channel]) -> Vec<SlotUpdate> {
    let mut out: Vec<SlotUpdate> = sent
        .keys()
        .filter(|id| !channels.iter().any(|c| c.id == **id))
        .map(|&id| SlotUpdate::Remove(id))
        .collect();
    for ch in channels.iter().take(MAX_CHANNELS) {
        let prev = sent.get(&ch.id);
        if prev == Some(&ch.kind) {
            continue;
        }
        out.push(match (prev, &ch.kind) {
            (Some(ChannelKind::Synth(_)), ChannelKind::Synth(p)) => SlotUpdate::SynthParams(ch.id, *p),
            _ => SlotUpdate::Rebuild(ch.id),
        });
    }
    out
}

/// WAV/MIDI export: the whole song if the playlist has a clip that plays something
/// (`Project::has_song`), otherwise the current pattern.
pub fn exports_song(project: &Project) -> bool {
    project.has_song()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (Project, ChannelId, ChannelId) {
        let mut p = Project::empty();
        let a = p.add_channel("kick", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) }).unwrap();
        let b = p.add_channel("bass", ChannelKind::Synth(SynthParams::default())).unwrap();
        (p, a, b)
    }

    fn sent(p: &Project) -> HashMap<ChannelId, ChannelKind> {
        p.channels.iter().map(|c| (c.id, c.kind.clone())).collect()
    }

    #[test]
    fn first_sync_rebuilds_every_channel() {
        let (p, a, b) = project();
        assert_eq!(slot_updates(&HashMap::new(), &p.channels), vec![SlotUpdate::Rebuild(a), SlotUpdate::Rebuild(b)]);
        assert!(slot_updates(&sent(&p), &p.channels).is_empty());
    }

    #[test]
    fn synth_params_only() {
        let (mut p, _, b) = project();
        let before = sent(&p);
        let ChannelKind::Synth(sp) = &mut p.channel_mut(b).unwrap().kind else { unreachable!() };
        sp.cutoff = 2000.0;
        let sp = *sp;
        assert_eq!(slot_updates(&before, &p.channels), vec![SlotUpdate::SynthParams(b, sp)]);
    }

    #[test]
    fn sample_source_change_rebuilds() {
        let (mut p, a, _) = project();
        let before = sent(&p);
        p.channel_mut(a).unwrap().kind = ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Snare) };
        assert_eq!(slot_updates(&before, &p.channels), vec![SlotUpdate::Rebuild(a)]);
        p.channel_mut(a).unwrap().kind = ChannelKind::Sampler { source: SampleSource::File("x.wav".into()) };
        assert_eq!(slot_updates(&before, &p.channels), vec![SlotUpdate::Rebuild(a)]);
    }

    #[test]
    fn removed_channel() {
        let (mut p, a, _) = project();
        let before = sent(&p);
        crate::remove_channel(&mut p, a);
        assert_eq!(slot_updates(&before, &p.channels), vec![SlotUpdate::Remove(a)]);
    }

    #[test]
    fn removals_come_first() {
        let (mut p, a, _) = project();
        let before = sent(&p);
        crate::remove_channel(&mut p, a);
        let c = p.add_channel("c", ChannelKind::MidiOut { channel: 0, out: 0 }).unwrap();
        assert_eq!(slot_updates(&before, &p.channels), vec![SlotUpdate::Remove(a), SlotUpdate::Rebuild(c)]);
    }

    #[test]
    fn kind_changes_rebuild() {
        let (mut p, a, _) = project();
        let mut s = sent(&p);
        let synth = ChannelKind::Synth(SynthParams::default());
        let midi = ChannelKind::MidiOut { channel: 3, out: 0 };
        // Sampler → synth → MIDI out → MIDI out on another channel/output → sampler.
        for kind in [synth, midi, ChannelKind::MidiOut { channel: 4, out: 0 }, ChannelKind::MidiOut { channel: 4, out: 1 }] {
            p.channel_mut(a).unwrap().kind = kind.clone();
            assert_eq!(slot_updates(&s, &p.channels), vec![SlotUpdate::Rebuild(a)], "{kind:?}");
            s.insert(a, kind);
        }
        p.channel_mut(a).unwrap().kind = ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) };
        assert_eq!(slot_updates(&s, &p.channels), vec![SlotUpdate::Rebuild(a)]);
    }

    #[test]
    fn mixer_changes_do_not_touch_slots() {
        let (mut p, a, b) = project();
        let before = sent(&p);
        let ch = p.channel_mut(a).unwrap();
        ch.volume = 0.1;
        ch.pan = -1.0;
        ch.mute = true;
        ch.solo = true;
        ch.choke = 2;
        ch.name = "x".into();
        p.channel_mut(b).unwrap().arp.enabled = true;
        crate::move_channel(&mut p, b, true);
        assert!(slot_updates(&before, &p.channels).is_empty());
    }

    #[test]
    fn export_region() {
        let (mut p, _, _) = project();
        assert!(!exports_song(&p));
        let pat = p.patterns[0].id;
        p.arrangement.clips.push(Clip { pattern: pat, track: 0, start: 0, len: 3840 });
        assert!(exports_song(&p));
        // Deleting the pattern removes its clips.
        let p2 = crate::new_pattern(&mut p);
        assert!(crate::delete_pattern(&mut p, pat));
        assert!(!exports_song(&p));
        p.arrangement.clips.push(Clip { pattern: p2, track: 3, start: 7680, len: 100 });
        assert!(exports_song(&p));
    }

    #[test]
    fn export_ignores_clips_that_play_nothing() {
        // E.g. a project file whose clips point to a pattern that no longer exists: the song
        // compiles to length 0, so the WAV contains only the 2 s tail.
        let (mut p, _, _) = project();
        p.arrangement.clips.push(Clip { pattern: PatternId(999), track: 0, start: 0, len: 3840 });
        assert!(!exports_song(&p));
        p.arrangement.clips = vec![Clip { pattern: p.patterns[0].id, track: 0, start: 0, len: 0 }];
        assert!(!exports_song(&p));
    }
}
