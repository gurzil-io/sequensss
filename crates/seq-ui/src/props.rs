//! Secondary windows: synth settings, arpeggiator settings, renaming.

use crate::SeqUi;
use crate::skin::{self, Kind};
use crate::theme::*;
use eframe::egui::{self, vec2};
use seq_model::*;
use seq_i18n::{tr, trf};

pub fn windows(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    synth_window(st, ctx, project);
    arp_window(st, ctx, project);
    rename_windows(st, ctx, project);
}

fn window<'a>(title: String, id: &str) -> egui::Window<'a> {
    egui::Window::new(egui::RichText::new(title).font(michroma(13.0)).color(HEADER_TEXT))
        .id(egui::Id::new(id))
        .resizable(false)
        .collapsible(false)
        .vscroll(true)
        .default_width(380.0)
}

fn synth_window(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    let Some(id) = st.synth_open else { return };
    let mut open = true;
    let name = project.channel(id).map(|c| c.name.clone()).unwrap_or_default();
    window(trf!("SYNTHÉ · {}", "SYNTH · {}", name.to_uppercase()), "synth_win").open(&mut open).show(ctx, |ui| {
        let Some(ch) = project.channel_mut(id) else { return };
        if let ChannelKind::Synth(p) = &mut ch.kind {
            synth_params(ui, p);
        } else {
            ui.label(tr!("Ce channel n'est pas un synthé.", "This channel is not a synth."));
        }
    });
    if !open {
        st.synth_open = None;
    }
}

fn arp_window(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    let Some(id) = st.arp_open else { return };
    let mut open = true;
    let name = project.channel(id).map(|c| c.name.clone()).unwrap_or_default();
    window(trf!("ARPÉGIATEUR · {}", "ARPEGGIATOR · {}", name.to_uppercase()), "arp_win").open(&mut open).show(ctx, |ui| {
        if let Some(ch) = project.channel_mut(id) {
            arp_params(ui, &mut ch.arp);
        }
    });
    if !open {
        st.arp_open = None;
    }
}

fn rename_windows(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    if let Some((pid, mut name)) = st.renaming_pattern.take() {
        let mut keep = true;
        window(tr!("RENOMMER LE PATTERN", "RENAME PATTERN").into(), "rename_pat").show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0).font(body(17.0)));
            r.request_focus();
            ui.horizontal(|ui| {
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if skin::text_button(ui, vec2(110.0, 40.0), Kind::Orange, "OK", body(15.0)).clicked() || enter {
                    if let (Some(p), false) = (project.pattern_mut(pid), name.trim().is_empty()) {
                        p.name = name.trim().to_string();
                    }
                    keep = false;
                }
                if skin::text_button(ui, vec2(110.0, 40.0), Kind::Cream, tr!("Annuler", "Cancel"), body(15.0)).clicked() {
                    keep = false;
                }
            });
        });
        if keep {
            st.renaming_pattern = Some((pid, name));
        }
    }
    if let Some((cid, mut name)) = st.renaming_channel.take() {
        let mut keep = true;
        window(tr!("RENOMMER LE CHANNEL", "RENAME CHANNEL").into(), "rename_ch").show(ctx, |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0).font(body(17.0)));
            r.request_focus();
            ui.horizontal(|ui| {
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if skin::text_button(ui, vec2(110.0, 40.0), Kind::Orange, "OK", body(15.0)).clicked() || enter {
                    if let (Some(c), false) = (project.channel_mut(cid), name.trim().is_empty()) {
                        c.name = name.trim().to_string();
                    }
                    keep = false;
                }
                if skin::text_button(ui, vec2(110.0, 40.0), Kind::Cream, tr!("Annuler", "Cancel"), body(15.0)).clicked() {
                    keep = false;
                }
            });
        });
        if keep {
            st.renaming_channel = Some((cid, name));
        }
    }
}

/// Arpeggio presets (all editable afterwards).
fn arp_presets() -> Vec<(&'static str, ArpParams)> {
    let base = ArpParams { enabled: true, ..Default::default() };
    let s16 = STEP;
    vec![
        (tr!("Montée 1/16", "Up 1/16"), ArpParams { rate: s16, gate: 0.5, ..base }),
        (tr!("Trance 1/16, 2 octaves", "Trance 1/16, 2 octaves"), ArpParams { rate: s16, gate: 0.35, octaves: 2, direction: ArpDirection::UpDown, ..base }),
        (tr!("Rebond 1/8", "Bounce 1/8"), ArpParams { rate: 2 * s16, gate: 0.7, direction: ArpDirection::UpDownInclusive, ..base }),
        (tr!("Descente 1/16 triolet", "Down 1/16 triplet"), ArpParams { rate: PPQ as Tick / 6, gate: 0.5, direction: ArpDirection::Down, ..base }),
        (tr!("Accords rythmés 1/8", "Rhythmic chords 1/8"), ArpParams { rate: 2 * s16, gate: 0.3, direction: ArpDirection::Chord, ..base }),
        (tr!("Stabs majeurs (1 note)", "Major stabs (1 note)"), ArpParams { rate: 2 * s16, gate: 0.25, direction: ArpDirection::Chord, chord: ArpChord::Major, ..base }),
        (tr!("Mineur 3 octaves (1 note)", "Minor 3 octaves (1 note)"), ArpParams { rate: s16, gate: 0.5, octaves: 3, chord: ArpChord::Minor, ..base }),
        (tr!("Aléatoire 1/32", "Random 1/32"), ArpParams { rate: PPQ as Tick / 8, gate: 0.6, octaves: 2, direction: ArpDirection::Random, ..base }),
        (tr!("Double croche répétée ×2", "Repeated 16th ×2"), ArpParams { rate: s16 / 2, gate: 0.5, repeat: 2, ..base }),
        (tr!("Ordre joué, hold", "As played, hold"), ArpParams { rate: s16, gate: 0.6, direction: ArpDirection::AsPlayed, hold: true, ..base }),
    ]
}

fn arp_params(ui: &mut egui::Ui, p: &mut ArpParams) {
    ui.horizontal(|ui| {
        skin::pill_switch(ui, &mut p.enabled, vec2(110.0, 40.0), Some(("ON", "OFF")));
        egui::ComboBox::new("arp_preset", "").selected_text(tr!("Préréglages…", "Presets…")).width(200.0).show_ui(ui, |ui| {
            for (name, preset) in arp_presets() {
                if ui.selectable_label(false, name).clicked() {
                    *p = preset;
                }
            }
        });
    });
    ui.add_space(6.0);
    ui.add_enabled_ui(p.enabled, |ui| {
        egui::Grid::new("arp_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label("Direction");
            egui::ComboBox::new("arp_dir", "").width(220.0).selected_text(p.direction.name()).show_ui(ui, |ui| {
                for d in ArpDirection::ALL {
                    ui.selectable_value(&mut p.direction, d, d.name());
                }
            });
            ui.end_row();

            ui.label(tr!("Vitesse", "Rate"));
            let rate_name = arp_rate_name(p.rate).map(str::to_string).unwrap_or(format!("{} ticks", p.rate));
            egui::ComboBox::new("arp_rate", "").width(140.0).selected_text(rate_name).show_ui(ui, |ui| {
                for (n, v) in ARP_RATES {
                    ui.selectable_value(&mut p.rate, v, arp_rate_name(v).unwrap_or(n));
                }
            });
            ui.end_row();

            ui.label("Octaves");
            ui.add(egui::DragValue::new(&mut p.octaves).range(1..=4));
            ui.end_row();

            ui.label("Gate");
            let mut pct = p.gate * 100.0;
            if ui.add(egui::Slider::new(&mut pct, 5.0..=100.0).suffix(" %")).changed() {
                p.gate = pct / 100.0;
            }
            ui.end_row();

            ui.label(tr!("Répétition", "Repeat"));
            ui.add(egui::DragValue::new(&mut p.repeat).range(1..=8).suffix("×"));
            ui.end_row();

            ui.label(tr!("Accord", "Chord"));
            egui::ComboBox::new("arp_chord", "").width(200.0).selected_text(p.chord.name()).show_ui(ui, |ui| {
                for c in ArpChord::ALL {
                    ui.selectable_value(&mut p.chord, c, c.name());
                }
            });
            ui.end_row();

            ui.label("Options");
            ui.horizontal(|ui| {
                ui.checkbox(&mut p.sync, tr!("Calé sur la grille", "Sync to grid"))
                    .on_hover_text(tr!(
                        "Pendant la lecture, les pas tombent sur la grille du morceau. Sinon l'arpège part dès la première note.",
                        "During playback, steps land on the song grid. Otherwise the arpeggio starts on the first note."
                    ));
                ui.checkbox(&mut p.hold, "Hold")
                    .on_hover_text(tr!(
                        "Les notes restent tenues après relâchement, jusqu'au prochain accord (jeu au clavier MIDI).",
                        "Notes keep sounding after release, until the next chord (MIDI keyboard playing)."
                    ));
            });
            ui.end_row();
        });
    });
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(tr!(
            "S'applique en direct à tout ce que joue le channel : pattern, clavier MIDI, clavier du piano roll.",
            "Applies live to everything the channel plays: pattern, MIDI keyboard, piano roll keyboard."
        ))
            .font(medium(13.0))
            .color(TEXT_DIMMER),
    );
}

fn synth_params(ui: &mut egui::Ui, p: &mut SynthParams) {
    egui::Grid::new("synth_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        wave(ui, "Osc 1", "osc1", &mut p.osc1);
        wave(ui, "Osc 2", "osc2", &mut p.osc2);
        ui.label(tr!("Désaccord osc 2", "Osc 2 detune"));
        ui.add(egui::Slider::new(&mut p.osc2_detune, -24.0..=24.0).suffix(" dt"));
        ui.end_row();
        ui.label("Mix osc 1/2");
        ui.add(egui::Slider::new(&mut p.osc_mix, 0.0..=1.0));
        ui.end_row();
        ui.label("Cutoff");
        ui.add(egui::Slider::new(&mut p.cutoff, 40.0..=16_000.0).logarithmic(true).suffix(" Hz"));
        ui.end_row();
        ui.label(tr!("Résonance", "Resonance"));
        ui.add(egui::Slider::new(&mut p.resonance, 0.0..=1.0));
        ui.end_row();
        ui.label(tr!("Env → filtre", "Env → filter"));
        ui.add(egui::Slider::new(&mut p.filter_env, 0.0..=1.0));
        ui.end_row();
        ui.label("Attack");
        ui.add(egui::Slider::new(&mut p.attack, 0.001..=2.0).logarithmic(true).suffix(" s"));
        ui.end_row();
        ui.label("Decay");
        ui.add(egui::Slider::new(&mut p.decay, 0.005..=3.0).logarithmic(true).suffix(" s"));
        ui.end_row();
        ui.label("Sustain");
        ui.add(egui::Slider::new(&mut p.sustain, 0.0..=1.0));
        ui.end_row();
        ui.label("Release");
        ui.add(egui::Slider::new(&mut p.release, 0.005..=4.0).logarithmic(true).suffix(" s"));
        ui.end_row();
        ui.label("Gain");
        ui.add(egui::Slider::new(&mut p.gain, 0.0..=1.0));
        ui.end_row();
    });
}

fn wave(ui: &mut egui::Ui, label: &str, id: &str, w: &mut Wave) {
    ui.label(label);
    egui::ComboBox::new(id, "").selected_text(w.name()).show_ui(ui, |ui| {
        for x in Wave::ALL {
            ui.selectable_value(w, x, x.name());
        }
    });
    ui.end_row();
}
