//! `.ron` format stability: a project saved by an earlier version.

use seq_model::*;

/// Saved before channels and patterns became shared (`CowArc`).
const V1: &str = include_str!("fixtures/v1_project.ron");

fn pretty(p: &Project) -> String {
    ron::ser::to_string_pretty(p, ron::ser::PrettyConfig::default()).unwrap()
}

#[test]
fn loads_project_saved_by_earlier_version() {
    let p: Project = ron::from_str(V1).unwrap();
    assert_eq!(p.version, 1);
    assert_eq!(p.swing, 0.25);
    assert_eq!(p.channels.len(), 7);
    assert_eq!(p.patterns.len(), 2);
    assert_eq!(p.arrangement.clips.len(), 3);
    assert_eq!(p.midi_outs, vec![Some("IAC Bus 1".to_string()), None]);
    // MIDI clock did not exist yet: off on every output.
    assert!(p.midi_clock.is_empty());
    assert_eq!(p.midi_clock_mask(), 0);
    // Track mute did not exist yet: every track plays.
    assert!(p.arrangement.muted.is_empty());
    assert!((0..p.arrangement.tracks).all(|t| !p.arrangement.is_muted(t)));

    let lead = p.channels.iter().find(|c| c.name == "Lead").unwrap();
    assert!(lead.arp.enabled);
    assert!(matches!(p.channels[5].kind, ChannelKind::MidiOut { channel: 9, out: 1 }));
    let verse = &p.patterns[1];
    assert_eq!(verse.name, "Verse");
    assert_eq!(verse.notes[&lead.id][1], Note { start: 500, len: 37, key: 75, velocity: 64, pan: 0 });
}

#[test]
fn saves_byte_identical_file() {
    let p: Project = ron::from_str(V1).unwrap();
    assert_eq!(pretty(&p), V1);
}

#[test]
fn muted_tracks_round_trip() {
    let mut p: Project = ron::from_str(V1).unwrap();
    p.arrangement.set_muted(2, true);
    let text = pretty(&p);
    assert!(text.contains("muted"));
    let back: Project = ron::from_str(&text).unwrap();
    assert_eq!(back, p);
    assert!(back.arrangement.is_muted(2));
    // Unmuted again: the file is the old one, byte for byte.
    p.arrangement.set_muted(2, false);
    assert_eq!(pretty(&p), V1);
}

#[test]
fn note_pan_round_trip() {
    let mut p: Project = ron::from_str(V1).unwrap();
    // Note pan did not exist yet: every note is centred.
    assert!(p.patterns.iter().all(|pt| pt.notes.values().flatten().all(|n| n.pan == 0)));
    let verse = p.patterns[1].id;
    let lead = p.channels.iter().find(|c| c.name == "Lead").unwrap().id;
    p.pattern_mut(verse).unwrap().notes.get_mut(&lead).unwrap()[1].pan = PAN_MIN;
    let text = pretty(&p);
    assert_eq!(text.matches("pan:").count(), V1.matches("pan:").count() + 1);
    let back: Project = ron::from_str(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(back.pattern(verse).unwrap().notes[&lead][1].pan, -64);
    // Centred again: the file is the old one, byte for byte.
    p.pattern_mut(verse).unwrap().notes.get_mut(&lead).unwrap()[1].pan = 0;
    assert_eq!(pretty(&p), V1);
}
