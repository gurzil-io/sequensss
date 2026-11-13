//! Sound settings of the selected channel: sound, volume, pan, step seq note,
//! choke group, arpeggiator.

use crate::icons::{self, Icon};
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{PanelId, PlaybackInfo, SeqUi, UiAction};
use eframe::egui::{Align2, Rect, Sense, Stroke, Ui, UiBuilder, Vec2, pos2, vec2};
use seq_model::*;
use seq_i18n::{tr, trf};

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project, _info: &PlaybackInfo) {
    let ch_id = st.channel.filter(|c| project.channel(*c).is_some());
    let chip = ch_id.and_then(|c| project.channel(c)).map(|c| (c.name.clone(), c.kind.label(), id_color(c.id.0)));
    let res = skin::panel(ui, rect, tr!("RÉGLAGES DU SON", "SOUND SETTINGS"), ORANGE, |ui| {
        if let Some((name, kind, color)) = &chip {
            let font = body(15.0);
            let w1 = ui.fonts(|f| f.layout_no_wrap(name.clone(), font.clone(), TEXT).size().x);
            let w2 = ui.fonts(|f| f.layout_no_wrap(format!("· {kind}"), medium(13.0), TEXT).size().x);
            let (r, _) = ui.allocate_exact_size(vec2(w1 + w2 + 44.0, 38.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 7.0, DARK_TILE);
            skin::led(p, pos2(r.left() + 17.0, r.center().y), 5.0, *color, true);
            p.text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, name, font, TEXT);
            p.text(pos2(r.left() + 36.0 + w1, r.center().y), Align2::LEFT_CENTER, format!("· {kind}"), medium(13.0), TEXT_DIMMER);
        }
    });
    if res.expand {
        st.toggle_focus(PanelId::Sound);
    }
    let area = res.body.shrink(10.0);
    let Some(id) = ch_id else {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, tr!("Aucun channel sélectionné", "No channel selected"), body(16.0), TEXT_DIMMER);
        return;
    };

    // Columns (mockup proportions).
    let gap = 8.0;
    let unit_of = |w: &[f32]| (area.width() - gap * (w.len() - 1) as f32) / w.iter().sum::<f32>();
    // Narrow columns (tablet, medium window): the SOUND column (display + buttons)
    // takes width from the knobs.
    let narrow = unit_of(&[1.9, 1.0, 1.0, 1.3, 1.5, 1.1]) * 1.3 < 125.0;
    let weights: [f32; 6] = if narrow { [2.8, 0.85, 0.85, 1.05, 1.35, 0.95] } else { [1.9, 1.0, 1.0, 1.3, 1.5, 1.1] };
    let unit = unit_of(&weights);
    let mut x = area.left();
    let mut cols = Vec::new();
    for w in weights {
        let r = Rect::from_min_size(pos2(x, area.top()), vec2(w * unit, area.height()));
        cols.push(r);
        x += w * unit + gap;
    }
    // Narrow columns: short titles.
    let titles = if narrow {
        [tr!("SON", "SOUND"), "VOL", "PAN", "NOTE", "CHOKE", "ARP"]
    } else {
        [
            tr!("SON", "SOUND"),
            "VOLUME",
            "PAN",
            tr!("NOTE DU STEP SEQ", "STEP SEQ NOTE"),
            tr!("GROUPE DE CHOKE", "CHOKE GROUP"),
            tr!("ARPÉGIATEUR", "ARPEGGIATOR"),
        ]
    };
    let p = ui.painter().clone();
    for (r, t) in cols.iter().zip(titles) {
        p.rect_filled(*r, 8.0, INNER2);
        p.rect_stroke(*r, 8.0, Stroke::new(1.0_f32, EDGE));
        p.with_clip_rect(*r).text(pos2(r.left() + 10.0, r.top() + 15.0), Align2::LEFT_CENTER, t, medium(11.5), TEXT_DIMMER);
    }
    let content = |r: Rect| Rect::from_min_max(pos2(r.left() + 10.0, r.top() + 28.0), pos2(r.right() - 10.0, r.bottom() - 10.0));
    let row_h = ((area.height() - 38.0 - 8.0) / 2.0).clamp(30.0, 52.0);

    // 1. Sound
    son(st, ui, content(cols[0]), row_h, project, id);

    let Some(ch) = project.channel_mut(id) else { return };

    // 2. Volume
    {
        let c = content(cols[1]);
        let d = (c.height() - 24.0).min(c.width()).min(92.0).max(30.0);
        let mut u = ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(pos2(c.center().x, c.top() + d / 2.0), Vec2::splat(d))));
        skin::knob(&mut u, &mut ch.volume, 0.0, 1.25, 0.8, d, false, Color::WHITE).on_hover_text(tr!("Glisser verticalement · double-clic : 80 %", "Drag vertically · double-click: 80 %"));
        p.text(pos2(c.center().x, c.bottom() - 8.0), Align2::CENTER_CENTER, format!("{:.0} %", ch.volume * 100.0), mono(17.0), TEXT);
    }
    // 3. Pan
    {
        let c = content(cols[2]);
        let d = (c.height() - 24.0).min(c.width()).min(92.0).max(30.0);
        let mut u = ui.new_child(UiBuilder::new().max_rect(Rect::from_center_size(pos2(c.center().x, c.top() + d / 2.0), Vec2::splat(d))));
        skin::knob(&mut u, &mut ch.pan, -1.0, 1.0, 0.0, d, true, LCD_GREEN).on_hover_text(tr!("Glisser verticalement · double-clic : centre", "Drag vertically · double-click: center"));
        let label = if ch.pan.abs() < 0.005 {
            "C".to_string()
        } else if ch.pan < 0.0 {
            trf!("G {:.0}", "L {:.0}", -ch.pan * 100.0)
        } else {
            trf!("D {:.0}", "R {:.0}", ch.pan * 100.0)
        };
        p.text(pos2(c.center().x, c.bottom() - 8.0), Align2::CENTER_CENTER, label, mono(17.0), TEXT);
    }
    // 4. Step seq note
    {
        let c = content(cols[3]);
        let lcd = Rect::from_min_max(c.min, pos2(c.right(), c.bottom() - row_h - 6.0));
        skin::lcd_bg(&p, lcd, 7.0);
        skin::lcd_text(&p, lcd.center(), Align2::CENTER_CENTER, &key_name(ch.default_key), mono((lcd.height() * 0.62).clamp(18.0, 34.0)), ORANGE_TOP);
        let bw = (c.width() - 4.0) / 2.0;
        let mut u = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(pos2(c.left(), c.bottom() - row_h), c.max)).layout(eframe::egui::Layout::left_to_right(eframe::egui::Align::Center)));
        u.spacing_mut().item_spacing.x = 4.0;
        if skin::text_button(&mut u, vec2(bw, row_h), Kind::Cream, "−", body(24.0)).clicked() {
            ch.default_key = ch.default_key.saturating_sub(1);
        }
        if skin::text_button(&mut u, vec2(bw, row_h), Kind::Cream, "+", body(24.0)).clicked() {
            ch.default_key = (ch.default_key + 1).min(127);
        }
    }
    // 5. Choke group
    {
        let c = content(cols[4]);
        let (cw, chh) = ((c.width() - 8.0) / 3.0, (c.height() - 4.0) / 2.0);
        for g in 0..6u8 {
            let r = Rect::from_min_size(pos2(c.left() + (g % 3) as f32 * (cw + 4.0), c.top() + (g / 3) as f32 * (chh + 4.0)), vec2(cw, chh));
            let resp = ui.interact(r, ui.id().with(("choke", g)), Sense::click());
            let fg = skin::paint_button(&p, r, 7.0, Kind::Tile, ch.choke == g, resp.is_pointer_button_down_on(), resp.hovered(), true);
            let fg = if ch.choke == g { fg } else { TEXT };
            p.text(r.center(), Align2::CENTER_CENTER, if g == 0 { "—".to_string() } else { g.to_string() }, bold(if g == 0 { 20.0 } else { 17.0 }), fg);
            if resp.on_hover_text(tr!(
                "Une note coupe les autres channels du même groupe (hi-hat ouvert / fermé)",
                "A note cuts the other channels of the same group (open / closed hi-hat)"
            )).clicked() {
                ch.choke = g;
            }
        }
    }
    // 6. Arpeggiator
    {
        let c = content(cols[5]);
        let mut u = ui.new_child(UiBuilder::new().max_rect(c));
        u.spacing_mut().item_spacing.y = 6.0;
        skin::pill_switch(&mut u, &mut ch.arp.enabled, vec2(c.width(), row_h.min(48.0)), Some(("ON", "OFF")));
        let sub = if ch.arp.enabled {
            let rate = arp_rate_name(ch.arp.rate).unwrap_or("?");
            format!("{} · {}", ch.arp.direction.name(), rate)
        } else {
            String::new()
        };
        let mut b = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(pos2(c.left(), c.bottom() - row_h), c.max)));
        let full = tr!("Réglages…", "Settings…");
        let tw = ui.fonts(|f| f.layout_no_wrap(full.to_string(), body(14.0), TEXT).size().x);
        let label = if c.width() >= tw + 12.0 { full } else { "…" };
        if skin::text_button(&mut b, vec2(c.width(), row_h), Kind::Cream, label, body(14.0))
            .on_hover_text(tr!("Réglages de l'arpégiateur", "Arpeggiator settings"))
            .clicked()
        {
            st.arp_open = Some(id);
        }
        if !sub.is_empty() {
            let y = (c.top() + row_h.min(48.0) + c.bottom() - row_h) / 2.0;
            p.with_clip_rect(c).text(pos2(c.center().x, y), Align2::CENTER_CENTER, sub, medium(12.0), TEXT_DIM);
        }
    }
}

type Color = eframe::egui::Color32;

fn son(st: &mut SeqUi, ui: &mut Ui, c: Rect, row_h: f32, project: &mut Project, id: ChannelId) {
    let p = ui.painter().clone();
    let Some(ch) = project.channel(id).cloned() else { return };
    let top = Rect::from_min_max(c.min, pos2(c.right(), c.bottom() - row_h - 6.0));
    let arrow_w = (c.width() * 0.2).clamp(30.0, 44.0);
    let prev_r = Rect::from_min_size(top.min, vec2(arrow_w, top.height()));
    let next_r = Rect::from_min_max(pos2(top.right() - arrow_w, top.top()), top.max);
    let lcd = Rect::from_min_max(pos2(prev_r.right() + 4.0, top.top()), pos2(next_r.left() - 4.0, top.bottom()));

    let (title, sub) = match &ch.kind {
        ChannelKind::Sampler { source: SampleSource::Builtin(d) } => {
            let i = Drum::ALL.iter().position(|x| x == d).unwrap_or(0);
            (d.name().to_string(), trf!("Intégré · {} / {}", "Built-in · {} / {}", i + 1, Drum::ALL.len()))
        }
        ChannelKind::Sampler { source: SampleSource::File(path) } => {
            (path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), tr!("Fichier WAV", "WAV file").to_string())
        }
        ChannelKind::Synth(sp) => (tr!("Synthé", "Synth").to_string(), format!("{} + {}", sp.osc1.name(), sp.osc2.name())),
        ChannelKind::MidiOut { channel, out } => {
            let port = project.midi_outs.get(*out as usize).cloned().flatten().unwrap_or(tr!("non connectée", "not connected").into());
            (trf!("Canal MIDI {}", "MIDI channel {}", channel + 1), format!("OUT {} · {}", out + 1, port))
        }
    };
    skin::lcd_bg(&p, lcd, 7.0);
    let lp = p.with_clip_rect(lcd.shrink(2.0));
    lp.text(pos2(lcd.left() + 10.0, lcd.center().y - 7.0), Align2::LEFT_CENTER, &title, bold(17.0), ORANGE_TOP);
    lp.text(pos2(lcd.left() + 10.0, lcd.center().y + 11.0), Align2::LEFT_CENTER, &sub, medium(11.5), LCD_LABEL);

    let can_cycle = !matches!(ch.kind, ChannelKind::Synth(_));
    for (r, icon, dir) in [(prev_r, Icon::Prev, -1i32), (next_r, Icon::Next, 1)] {
        let resp = ui.interact(r, ui.id().with(("son_cycle", dir)), if can_cycle { Sense::click() } else { Sense::hover() });
        let fg = skin::paint_button(&p, r, 7.0, Kind::Cream, false, resp.is_pointer_button_down_on(), resp.hovered(), can_cycle);
        icons::draw(&p, r, 16.0, icon, fg, 1.0);
        if resp.clicked() {
            cycle(project, id, dir);
        }
    }

    // Preview (held) + secondary action.
    let bw = (c.width() - 4.0) / 2.0;
    let listen = Rect::from_min_size(pos2(c.left(), c.bottom() - row_h), vec2(bw, row_h));
    let second = Rect::from_min_size(pos2(listen.right() + 4.0, listen.top()), vec2(bw, row_h));
    let lresp = ui.interact(listen, ui.id().with("son_listen"), Sense::click_and_drag());
    let fg = skin::paint_button(&p, listen, 7.0, Kind::Orange, false, lresp.is_pointer_button_down_on(), lresp.hovered(), true);
    // Narrow button: icon only (the text would spill onto the neighboring button).
    let label = tr!("Écouter", "Preview");
    let tw = p.layout_no_wrap(label.to_string(), bold(15.0), fg).size().x;
    if bw >= tw + 14.0 + 8.0 + 10.0 {
        let x0 = listen.center().x - (14.0 + 8.0 + tw) / 2.0;
        icons::draw(&p, Rect::from_min_size(pos2(x0, listen.center().y - 7.0), vec2(14.0, 14.0)), 14.0, Icon::Play, fg, 1.0);
        p.text(pos2(x0 + 22.0, listen.center().y), Align2::LEFT_CENTER, label, bold(15.0), fg);
    } else {
        icons::draw(&p, Rect::from_center_size(listen.center(), vec2(16.0, 16.0)), 16.0, Icon::Play, fg, 1.0);
    }
    let down = lresp.is_pointer_button_down_on();
    match (down, st.audition) {
        (true, None) => {
            st.audition = Some((id, ch.default_key));
            st.actions.push(UiAction::Preview { channel: id, key: ch.default_key, vel: 110, on: true });
        }
        (false, Some((cid, key))) => {
            st.audition = None;
            st.actions.push(UiAction::Preview { channel: cid, key, vel: 0, on: false });
        }
        _ => {}
    }

    let sresp = ui.interact(second, ui.id().with("son_second"), Sense::click());
    let fg = skin::paint_button(&p, second, 7.0, Kind::Cream, false, sresp.is_pointer_button_down_on(), sresp.hovered(), true);
    let label = match &ch.kind {
        ChannelKind::Sampler { .. } => "WAV…".to_string(),
        ChannelKind::Synth(_) => tr!("Synthé…", "Synth…").to_string(),
        ChannelKind::MidiOut { out, .. } => format!("OUT {} ▸", out + 1),
    };
    p.text(second.center(), Align2::CENTER_CENTER, &label, body(14.0), fg);
    if sresp.clicked() {
        match ch.kind {
            ChannelKind::Sampler { .. } => st.actions.push(UiAction::LoadSample(Some(id))),
            ChannelKind::Synth(_) => st.synth_open = Some(id),
            ChannelKind::MidiOut { channel, out } => {
                let n = project.midi_outs.len().max(1) as u8;
                if let Some(c) = project.channel_mut(id) {
                    c.kind = ChannelKind::MidiOut { channel, out: (out + 1) % n };
                }
            }
        }
    }
    let hover = match ch.kind {
        ChannelKind::Sampler { .. } => tr!("Charger un fichier WAV", "Load a WAV file"),
        ChannelKind::Synth(_) => tr!("Réglages du synthé", "Synth settings"),
        ChannelKind::MidiOut { .. } => tr!("Sortie MIDI suivante (voir MIDI & Audio)", "Next MIDI output (see MIDI & Audio)"),
    };
    sresp.on_hover_text(hover);
}

/// Previous / next sound (built-in sounds), or previous / next MIDI channel.
fn cycle(project: &mut Project, id: ChannelId, dir: i32) {
    let Some(ch) = project.channel_mut(id) else { return };
    let old_name_auto = matches!(ch.kind, ChannelKind::Sampler { source: SampleSource::Builtin(_) }) && ch.kind.is_default_name(&ch.name);
    match &mut ch.kind {
        ChannelKind::Sampler { source } => {
            let n = Drum::ALL.len() as i32;
            let i = match source {
                SampleSource::Builtin(d) => Drum::ALL.iter().position(|x| x == d).unwrap_or(0) as i32,
                SampleSource::File(_) => if dir > 0 { -1 } else { n },
            };
            let d = Drum::ALL[(i + dir).rem_euclid(n) as usize];
            *source = SampleSource::Builtin(d);
            // The name follows the sound as long as it has not been renamed.
            if old_name_auto {
                ch.name = d.name().to_string();
            }
        }
        ChannelKind::MidiOut { channel, .. } => {
            *channel = (*channel as i32 + dir).rem_euclid(16) as u8;
        }
        ChannelKind::Synth(_) => {}
    }
}
