//! Compact mode (phone): condensed top bar and panel tabs at the bottom.
//!
//! What does not fit in the bar goes into two menus: ≡ (files + swing / time signature)
//! and ✎ (edit commands, for when there is no physical keyboard).

use crate::icons::{self, Icon};
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{EditCmd, PanelId, PlaybackInfo, SeqUi, UiAction};
use eframe::egui::{self, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use seq_i18n::{tr, trf};
use seq_model::*;

const H: f32 = 48.0;

impl SeqUi {
    pub(crate) fn compact_top_bar(&mut self, ui: &mut egui::Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
        let p = ui.painter().clone();
        skin::bevel(&p, rect, 10.0, BAR_TOP, BAR_BOT, EDGE, Some(BAR_HL));
        let mut row = ui.new_child(
            UiBuilder::new().max_rect(rect.shrink2(vec2(8.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let ui = &mut row;
        ui.spacing_mut().item_spacing.x = 5.0;

        // Main menu
        let menu_btn = skin::icon_button(ui, vec2(52.0, H), Kind::Cream, Icon::Menu, 22.0);
        self.compact_menu(ui, &menu_btn, project);
        if self.dirty {
            skin::led(ui.painter(), pos2(menu_btn.rect.right() - 7.0, menu_btn.rect.top() + 7.0), 3.5, ORANGE, false);
        }

        // Transport
        let play_icon = if info.playing { Icon::Pause } else { Icon::Play };
        if skin::icon_button(ui, vec2(64.0, H), Kind::Orange, play_icon, 24.0).clicked() {
            self.act(UiAction::TogglePlay);
        }
        if skin::icon_button(ui, vec2(52.0, H), Kind::Cream, Icon::Stop, 18.0).clicked() {
            self.act(UiAction::Stop);
        }
        let rec = self.recording;
        if skin::button(ui, vec2(52.0, H), Kind::Cream, rec, |p, r, _| {
            if rec {
                skin::led(p, r.center(), 8.0, REC_RED, true);
            } else {
                icons::draw(p, r, 18.0, Icon::Record, REC_RED, 1.0);
            }
        })
        .clicked()
        {
            self.recording = !self.recording;
        }
        let metro = self.metronome;
        if skin::button(ui, vec2(52.0, H), Kind::Dark, false, |p, r, _| {
            let c = if metro { LCD_GREEN } else { TEXT_DIMMER };
            if metro {
                p.rect_stroke(r.shrink(1.0), 8.0, Stroke::new(1.0_f32, with_alpha(LCD_GREEN, 90)));
            }
            icons::draw(p, Rect::from_center_size(r.center(), vec2(20.0, 20.0)), 20.0, Icon::Metronome, c, 1.8);
        })
        .clicked()
        {
            self.metronome = !self.metronome;
        }

        skin::segmented(ui, &mut self.song_mode, &[(false, None, "PAT"), (true, None, "SONG")], H - 6.0, 58.0, skin::SegStyle::Orange, cond(17.0));

        // BPM
        ui.spacing_mut().item_spacing.x = 3.0;
        if skin::text_button(ui, vec2(38.0, H), Kind::Cream, "−", body(22.0)).clicked() {
            project.tempo_bpm = (project.tempo_bpm - 1.0).round().max(20.0);
        }
        let bpm = skin::lcd(ui, vec2(90.0, H), "BPM", &format!("{:.1}", project.tempo_bpm), ORANGE_TOP, lcd(32.0), Sense::click_and_drag());
        if bpm.dragged() {
            project.tempo_bpm = ((project.tempo_bpm - bpm.drag_delta().y as f64 * 0.25) * 10.0).round() / 10.0;
        }
        if skin::text_button(ui, vec2(38.0, H), Kind::Cream, "+", body(22.0)).clicked() {
            project.tempo_bpm = (project.tempo_bpm + 1.0).round().min(300.0);
        }
        project.tempo_bpm = project.tempo_bpm.clamp(20.0, 300.0);
        ui.spacing_mut().item_spacing.x = 5.0;

        // Position (tablet, medium window: if there is room left)
        let right_w = 3.0 * 52.0 + 2.0 * 5.0 + 5.0 + 22.0 + 5.0;
        if ui.available_width() > right_w + 180.0 + 20.0 {
            ui.add_space(6.0);
            let bar = project.bar_len().max(1) as f64;
            let beat = project.beat_len().max(1) as f64;
            let t = info.playhead.max(0.0);
            let b = (t / bar).floor();
            let be = ((t - b * bar) / beat).floor();
            let pos = format!("{:03}:{}:{:03}", b as u64 + 1, be as u64 + 1, (t - b * bar - be * beat) as u64);
            skin::lcd(ui, vec2(180.0, H), "POSITION", &pos, LCD_GREEN, lcd(32.0), Sense::hover());
        }

        // Edit, right-aligned
        skin::spacer_right(ui, right_w);
        let edit_btn = skin::icon_button(ui, vec2(52.0, H), Kind::Cream, Icon::Pencil, 20.0);
        self.edit_menu(ui, &edit_btn);
        ui.add_enabled_ui(self.can_undo, |ui| {
            if skin::icon_button(ui, vec2(52.0, H), Kind::Cream, Icon::Undo, 20.0).clicked() {
                self.act(UiAction::Undo);
            }
        });
        ui.add_enabled_ui(self.can_redo, |ui| {
            if skin::icon_button(ui, vec2(52.0, H), Kind::Cream, Icon::Redo, 20.0).clicked() {
                self.act(UiAction::Redo);
            }
        });

        // Master VU meter
        let (mr, _) = ui.allocate_exact_size(vec2(22.0, H), Sense::hover());
        let p = ui.painter();
        p.rect_filled(mr, 5.0, LCD_BG);
        p.rect_stroke(mr, 5.0, Stroke::new(1.0_f32, LCD_EDGE));
        let inner = mr.shrink(3.0);
        let bw = (inner.width() - 2.0) / 2.0;
        skin::meter_bar(p, Rect::from_min_size(inner.min, vec2(bw, inner.height())), info.master.0);
        skin::meter_bar(p, Rect::from_min_size(inner.min + vec2(bw + 2.0, 0.0), vec2(bw, inner.height())), info.master.1);
    }

    /// ≡ menu: files, then project settings that have no room in the bar.
    fn compact_menu(&mut self, ui: &mut egui::Ui, btn: &egui::Response, project: &mut Project) {
        let id = ui.make_persistent_id("compact_menu");
        if btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        // Close on outside click only: the swing slider is dragged with a finger.
        let max_h = ui.ctx().screen_rect().height() - btn.rect.bottom() - 16.0;
        egui::popup_below_widget(ui, id, btn, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(300.0);
            egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| self.compact_menu_items(ui, project));
        });
    }

    fn compact_menu_items(&mut self, ui: &mut egui::Ui, project: &mut Project) {
        {
            let mut chosen = None;
            skin::menu_caption(ui, &trf!("PROJET · {}", "PROJECT · {}", self.file_label.to_uppercase()));
            for (label, a) in [(tr!("Enregistrer", "Save"), UiAction::Save), (tr!("Ouvrir…", "Open…"), UiAction::Open), (tr!("Enregistrer sous…", "Save as…"), UiAction::SaveAs)] {
                if skin::menu_item(ui, label, TEXT, false).clicked() {
                    chosen = Some(a);
                }
            }
            skin::menu_separator(ui);
            for (label, a) in [(tr!("Exporter en WAV", "Export WAV"), UiAction::ExportWav), (tr!("Exporter en MIDI", "Export MIDI"), UiAction::ExportMidi)] {
                if skin::menu_item(ui, label, TEXT, false).clicked() {
                    chosen = Some(a);
                }
            }
            skin::menu_separator(ui);
            skin::menu_caption(ui, &format!("SWING · {:.0} %", project.swing * 100.0));
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                skin::metal_slider(ui, &mut project.swing, 260.0);
            });
            skin::menu_caption(ui, &trf!("MESURE · {}/{}", "TIME SIG. · {}/{}", project.time_sig.0, project.time_sig.1));
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.spacing_mut().item_spacing.x = 4.0;
                if skin::text_button(ui, vec2(44.0, 40.0), Kind::Cream, "−", body(20.0)).clicked() {
                    project.time_sig.0 = project.time_sig.0.saturating_sub(1).max(1);
                }
                if skin::text_button(ui, vec2(44.0, 40.0), Kind::Cream, "+", body(20.0)).clicked() {
                    project.time_sig.0 = (project.time_sig.0 + 1).min(16);
                }
                ui.add_space(8.0);
                skin::segmented(
                    ui,
                    &mut project.time_sig.1,
                    &[(2u8, None, "/2"), (4, None, "/4"), (8, None, "/8"), (16, None, "/16")],
                    34.0,
                    36.0,
                    skin::SegStyle::Grey,
                    cond(15.0),
                );
            });
            skin::menu_separator(ui);
            skin::menu_caption(ui, tr!("NOUVEAU PROJET", "NEW PROJECT"));
            for (i, kit) in KITS.iter().enumerate() {
                if skin::menu_item(ui, &format!("Kit {}", kit.name), TEXT, false).clicked() {
                    chosen = Some(UiAction::NewProject(i));
                }
            }
            skin::menu_separator(ui);
            if let Some(l) = crate::language_items(ui) {
                chosen = Some(UiAction::SetLanguage(l));
            }
            skin::menu_separator(ui);
            if let Some(on) = crate::update_toggle(ui, self.check_updates) {
                chosen = Some(UiAction::SetCheckUpdates(on));
            }
            ui.add_space(6.0);
            if let Some(a) = chosen {
                self.act(a);
                ui.memory_mut(|m| m.close_popup());
            }
        }
    }

    /// ✎ menu: edit commands for the piano roll or the timeline.
    fn edit_menu(&mut self, ui: &mut egui::Ui, btn: &egui::Response) {
        let id = ui.make_persistent_id("compact_edit_menu");
        if btn.clicked() {
            ui.memory_mut(|m| m.toggle_popup(id));
        }
        let editor = self.editor_for_tab();
        egui::popup_below_widget(ui, id, btn, egui::PopupCloseBehavior::CloseOnClick, |ui| {
            ui.set_min_width(240.0);
            let target = match editor {
                crate::Editor::Piano => "PIANO ROLL",
                crate::Editor::Timeline => "TIMELINE",
            };
            skin::menu_caption(ui, &trf!("SÉLECTION · {target}", "SELECTION · {target}"));
            for (label, cmd) in [
                (tr!("Copier", "Copy"), EditCmd::Copy),
                (tr!("Coller", "Paste"), EditCmd::Paste),
                (tr!("Dupliquer", "Duplicate"), EditCmd::Duplicate),
                (tr!("Supprimer", "Delete"), EditCmd::Delete),
                (tr!("Tout sélectionner", "Select all"), EditCmd::SelectAll),
                (tr!("Désélectionner", "Deselect"), EditCmd::Deselect),
            ] {
                if skin::menu_item(ui, label, TEXT, false).clicked() {
                    self.edit_cmd = Some((editor, cmd));
                    self.active_editor = editor;
                }
            }
        });
    }

    /// Full screen: floating Undo/Redo and Play/Stop at the bottom right (the top bar is hidden).
    pub(crate) fn floating_transport(&mut self, ctx: &egui::Context, info: &PlaybackInfo) {
        egui::Area::new(egui::Id::new("floating_transport"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-22.0, -14.0))
            .show(ctx, |ui| {
                let gap = 10.0;
                let (bg, _) = ui.allocate_exact_size(vec2(4.0 * 52.0 + 2.0 * 4.0 + gap + 12.0, 52.0 + 12.0), Sense::hover());
                ui.painter().rect_filled(bg.translate(vec2(0.0, 4.0)), 12.0, egui::Color32::from_black_alpha(90));
                ui.painter().rect_filled(bg, 12.0, with_alpha(BAR_BOT, 235));
                let mut row = ui.new_child(UiBuilder::new().max_rect(bg.shrink(6.0)).layout(egui::Layout::left_to_right(egui::Align::Center)));
                row.spacing_mut().item_spacing.x = 4.0;
                row.add_enabled_ui(self.can_undo, |ui| {
                    if skin::icon_button(ui, vec2(52.0, 52.0), Kind::Cream, Icon::Undo, 20.0).on_hover_text("Ctrl+Z").clicked() {
                        self.act(UiAction::Undo);
                    }
                });
                row.add_enabled_ui(self.can_redo, |ui| {
                    if skin::icon_button(ui, vec2(52.0, 52.0), Kind::Cream, Icon::Redo, 20.0).on_hover_text("Ctrl+Y").clicked() {
                        self.act(UiAction::Redo);
                    }
                });
                row.add_space(gap - 4.0);
                let play_icon = if info.playing { Icon::Pause } else { Icon::Play };
                if skin::icon_button(&mut row, vec2(52.0, 52.0), Kind::Orange, play_icon, 22.0).clicked() {
                    self.act(UiAction::TogglePlay);
                }
                if skin::icon_button(&mut row, vec2(52.0, 52.0), Kind::Cream, Icon::Stop, 18.0).clicked() {
                    self.act(UiAction::Stop);
                }
            });
    }

    /// Bottom tabs: one panel at a time.
    pub(crate) fn tab_bar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter().clone();
        skin::bevel(&p, rect, 10.0, BAR_TOP, BAR_BOT, EDGE, Some(BAR_HL));
        let tabs = [
            (PanelId::Timeline, "TIMELINE", LCD_GREEN),
            (PanelId::Rack, "RACK", ORANGE),
            (PanelId::Piano, "PIANO ROLL", YELLOW),
            (PanelId::Sound, tr!("SON", "SOUND"), ORANGE),
            (PanelId::Midi, "MIDI & AUDIO", RED),
        ];
        let inner = rect.shrink(4.0);
        let gap = 4.0;
        let w = (inner.width() - gap * (tabs.len() - 1) as f32) / tabs.len() as f32;
        for (i, (id, label, led)) in tabs.into_iter().enumerate() {
            let r = Rect::from_min_size(pos2(inner.left() + i as f32 * (w + gap), inner.top()), vec2(w, inner.height()));
            let resp = ui.interact(r, ui.id().with(("tab", i)), Sense::click());
            let on = self.tab == id;
            if on {
                p.rect_filled(r, 7.0, ORANGE);
            } else if resp.is_pointer_button_down_on() {
                p.rect_filled(r, 7.0, egui::Color32::from_white_alpha(14));
            }
            let fg = if on { ON_ORANGE } else { TEXT_DIM };
            let g = p.layout_no_wrap(label.to_string(), cond(16.0), fg);
            let x0 = r.center().x - (g.size().x + 16.0) / 2.0;
            skin::led(&p, pos2(x0 + 4.0, r.center().y), 4.0, if on { ON_ORANGE } else { led }, !on);
            p.galley(pos2(x0 + 16.0, r.center().y - g.size().y / 2.0), g, fg);
            if resp.clicked() {
                // Applied at the end of the frame: otherwise the new panel, drawn right after,
                // would handle the same touch (note or clip added by mistake).
                self.pending_tab = Some(id);
            }
        }
    }
}
