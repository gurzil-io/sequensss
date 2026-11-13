//! MIDI & Audio: audio output, MIDI input, project MIDI outputs, quantize.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{PanelId, SeqUi, UiAction};
use crate::icons::Icon;
use eframe::egui::{self, Align2, Rect, Sense, Ui, UiBuilder, pos2, vec2};
use seq_model::*;
use seq_i18n::{tr, trf};

fn fmt_hz(sr: u32) -> String {
    let s = sr.to_string();
    if s.len() > 3 { format!("{} {}", &s[..s.len() - 3], &s[s.len() - 3..]) } else { s }
}

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project) {
    let res = skin::panel(ui, rect, "MIDI & AUDIO", RED, |ui| {
        if ui.available_width() > 150.0 {
            skin::spacer_right(ui, 140.0);
        }
        ui.add_enabled_ui(project.midi_outs.len() < MAX_MIDI_OUTS, |ui| {
            if skin::auto_button(ui, 44.0, Kind::Cream, tr!("+ Sortie MIDI", "+ MIDI output")).clicked() {
                project.midi_outs.push(None);
            }
        });
    });
    if res.expand {
        st.toggle_focus(PanelId::Midi);
    }
    let area = res.body.shrink(10.0);
    let lw = (area.width() - 10.0) / 2.25;
    let left = Rect::from_min_size(area.min, vec2(lw, area.height()));
    let right = Rect::from_min_max(pos2(left.right() + 10.0, area.top()), area.max);
    let d = st.devices.clone();

    // --- Left column ---
    let mut lu = ui.new_child(UiBuilder::new().max_rect(left));
    lu.set_clip_rect(left);
    egui::ScrollArea::vertical().id_salt("midi_left").auto_shrink([false, false]).show(&mut lu, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let w = ui.available_width();
        let sr = d.sample_rate.map(|s| format!(" · {} Hz", fmt_hz(s))).unwrap_or_default();
        label(ui, &trf!("SORTIE AUDIO{sr}", "AUDIO OUTPUT{sr}"));
        let audio_led = if d.audio_ok { METER_GREEN } else { RED };
        let cur = d.current_audio.clone().unwrap_or_else(|| tr!("Par défaut", "Default").into());
        skin::dropdown(ui, "dd_audio", vec2(w, 44.0), Some((audio_led, true)), &cur, None, |ui| {
            if skin::menu_item(ui, tr!("↻ Rafraîchir la liste", "↻ Refresh list"), TEXT_DIM, false).clicked() {
                st.actions.push(UiAction::RefreshDevices);
            }
            skin::menu_separator(ui);
            if skin::menu_item(ui, tr!("Par défaut", "Default"), TEXT, d.current_audio.is_none()).clicked() {
                st.actions.push(UiAction::SelectAudio(None));
            }
            for n in &d.audio {
                if skin::menu_item(ui, n, TEXT, d.current_audio.as_ref() == Some(n)).clicked() {
                    st.actions.push(UiAction::SelectAudio(Some(n.clone())));
                }
            }
        });
        ui.add_space(6.0);
        label(ui, tr!("ENTRÉE MIDI · JOUE LE CHANNEL SÉLECTIONNÉ", "MIDI INPUT · PLAYS SELECTED CHANNEL"));
        let in_led = if d.current_in.is_some() { METER_GREEN } else { SEG_ON };
        let cur = d.current_in.clone().unwrap_or_else(|| tr!("Aucune", "None").into());
        skin::dropdown(ui, "dd_midi_in", vec2(w, 44.0), Some((in_led, true)), &cur, None, |ui| {
            if skin::menu_item(ui, tr!("↻ Rafraîchir la liste", "↻ Refresh list"), TEXT_DIM, false).clicked() {
                st.actions.push(UiAction::RefreshDevices);
            }
            skin::menu_separator(ui);
            if skin::menu_item(ui, tr!("Aucune", "None"), TEXT, d.current_in.is_none()).clicked() {
                st.actions.push(UiAction::SelectMidiIn(None));
            }
            if d.midi_in.is_empty() {
                skin::menu_caption(ui, tr!("AUCUN PORT (loopMIDI ?)", "NO PORT (loopMIDI?)"));
            }
            for n in &d.midi_in {
                if skin::menu_item(ui, n, TEXT, d.current_in.as_ref() == Some(n)).clicked() {
                    st.actions.push(UiAction::SelectMidiIn(Some(n.clone())));
                }
            }
        });
        ui.add_space(6.0);
        let (r, _) = ui.allocate_exact_size(vec2(w, 44.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 7.0, INNER2);
        p.rect_stroke(r, 7.0, egui::Stroke::new(1.0_f32, EDGE));
        p.with_clip_rect(r).text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, tr!("Quantifier l'enreg. 1/16", "Quantize rec. 1/16"), body(15.0), TEXT);
        let mut su = ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(pos2(r.right() - 38.0, r.center().y), vec2(64.0, 34.0))));
        skin::pill_switch(&mut su, &mut st.quantize, vec2(64.0, 34.0), None);
    });

    // --- Right column: MIDI outputs ---
    let mut ru = ui.new_child(UiBuilder::new().max_rect(right));
    ru.set_clip_rect(right);
    let mut remove = None;
    egui::ScrollArea::vertical().id_salt("midi_right").auto_shrink([false, false]).show(&mut ru, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        label(ui, tr!("SORTIES MIDI", "MIDI OUTPUTS"));
        let w = ui.available_width();
        for i in 0..project.midi_outs.len() {
            let port = project.midi_outs[i].clone();
            let users = project.channels.iter().filter(|c| matches!(c.kind, ChannelKind::MidiOut { out, .. } if out as usize == i)).count();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let connected = port.as_ref().is_some_and(|p| d.open_outs.contains(p));
                let badge_bg = if port.is_some() { PALETTE[i % 4] } else { BTN_DARK };
                let (b, _) = ui.allocate_exact_size(vec2(64.0, 44.0), Sense::hover());
                ui.painter().rect_filled(b, 7.0, badge_bg);
                ui.painter().text(b.center(), Align2::CENTER_CENTER, format!("OUT {}", i + 1), cond(17.0), if port.is_some() { ON_ORANGE } else { TEXT_DIM });
                let led = match (&port, connected) {
                    (None, _) => SEG_ON,
                    (Some(_), true) => METER_GREEN,
                    (Some(_), false) => RED,
                };
                let text = port.clone().unwrap_or_else(|| tr!("Aucun port", "No port").into());
                let dw = (w - 64.0 - 44.0 - 12.0).max(80.0);
                skin::dropdown(ui, ("dd_out", i), vec2(dw, 44.0), Some((led, true)), &text, None, |ui| {
                    if skin::menu_item(ui, tr!("↻ Rafraîchir la liste", "↻ Refresh list"), TEXT_DIM, false).clicked() {
                        st.actions.push(UiAction::RefreshDevices);
                    }
                    skin::menu_separator(ui);
                    if skin::menu_item(ui, tr!("Aucun port", "No port"), TEXT, port.is_none()).clicked() {
                        project.midi_outs[i] = None;
                    }
                    if d.midi_out.is_empty() {
                        skin::menu_caption(ui, tr!("AUCUN PORT (loopMIDI ?)", "NO PORT (loopMIDI?)"));
                    }
                    for n in &d.midi_out {
                        if skin::menu_item(ui, n, TEXT, port.as_ref() == Some(n)).clicked() {
                            project.midi_outs[i] = Some(n.clone());
                        }
                    }
                })
                .on_hover_text(trf!(
                    "{users} channel(s) MIDI out sur cette sortie{}",
                    "{users} MIDI out channel(s) on this output{}",
                    if port.is_some() && !connected {
                        tr!("\nPort non ouvert : déjà utilisé par une autre appli ?", "\nPort not open: already used by another app?")
                    } else {
                        ""
                    }
                ));
                ui.add_enabled_ui(project.midi_outs.len() > 1, |ui| {
                    if skin::icon_button(ui, vec2(44.0, 44.0), Kind::Tile, Icon::Close, 16.0).on_hover_text(tr!("Supprimer cette sortie", "Remove this output")).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        if project.channels.iter().all(|c| !matches!(c.kind, ChannelKind::MidiOut { .. })) {
            ui.label(egui::RichText::new(tr!(
                "Ajoute un channel « MIDI out » dans le rack pour piloter un synthé externe.",
                "Add a “MIDI out” channel in the rack to drive an external synth."
            )).font(medium(13.0)).color(TEXT_DIMMER));
        }
    });

    // Removing an output: channels that targeted it move to OUT 1,
    // those on the following outputs are shifted down.
    if let Some(i) = remove {
        project.midi_outs.remove(i);
        for c in project.channels.iter_mut().filter(|c| matches!(c.kind, ChannelKind::MidiOut { out, .. } if out as usize >= i)) {
            if let ChannelKind::MidiOut { out, .. } = &mut c.make_mut().kind {
                let o = *out as usize;
                if o == i {
                    *out = 0;
                } else if o > i {
                    *out -= 1;
                }
            }
        }
    }
}

fn label(ui: &mut Ui, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 16.0), Sense::hover());
    skin::caption(ui.painter(), r.left_center(), Align2::LEFT_CENTER, text, TEXT_DIMMER);
}
