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

    let lead = p.channels.iter().find(|c| c.name == "Lead").unwrap();
    assert!(lead.arp.enabled);
    assert!(matches!(p.channels[5].kind, ChannelKind::MidiOut { channel: 9, out: 1 }));
    let verse = &p.patterns[1];
    assert_eq!(verse.name, "Verse");
    assert_eq!(verse.notes[&lead.id][1], Note { start: 500, len: 37, key: 75, velocity: 64 });
}

#[test]
fn saves_byte_identical_file() {
    let p: Project = ron::from_str(V1).unwrap();
    assert_eq!(pretty(&p), V1);
}
