//! Channel rack: one row per channel (mute, solo, tile, step sequencer steps).

use crate::icons::Icon;
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::widgets::Pointer;
use crate::{Editor, PanelId, PlaybackInfo, SeqUi, UiAction};
use eframe::egui::{self, Align2, Color32, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use seq_edit::step_count;
use seq_i18n::{tr, trf};
use seq_model::*;

const ROW_H: f32 = 44.0;
/// Step width bounds: a bar fits the width when it can (as with the fixed lengths),
/// otherwise the steps keep this minimum size and the rack scrolls. Longer patterns
/// never squeeze the steps.
const MIN_CW: f32 = 12.0;
const MAX_CW: f32 = 56.0;
/// Gaps between steps, groups of 4 steps, bars (added to the group gap).
const GAP: f32 = 4.0;
const GGAP: f32 = 6.0;
const BGAP: f32 = 8.0;
/// Horizontal scroll bar under the rows.
const HBAR_H: f32 = 16.0;
/// Left of the steps: mute, solo, channel tile.
const STEPS_X: f32 = 265.0;
/// After a manual scroll, the view stops following the playhead for this long (s).
const FOLLOW_PAUSE: f64 = 2.0;

/// Horizontal scroll of the step grid.
#[derive(Default)]
pub struct RackState {
    scroll_x: f32,
    /// Time of the last manual scroll.
    scrolled_at: f64,
    /// Pattern before the step paint in progress (restored if it becomes a two-finger gesture).
    before: Option<CowArc<Pattern>>,
}

/// Horizontal layout of the step columns, shared by all rows: the pattern's steps,
/// then empty bars (at least one) where painting a step grows the pattern.
#[derive(Clone, Copy)]
struct Cols {
    /// Step width.
    cw: f32,
    /// Steps per bar.
    spb: usize,
    /// Steps of the pattern.
    steps: usize,
    /// Columns drawn (pattern + empty bars).
    total: usize,
    /// Visible width and scroll.
    view: f32,
    sx: f32,
}

impl Cols {
    fn new(view: f32, spb: usize, steps: usize) -> Self {
        let groups = spb.div_ceil(4);
        let cw = ((view - (spb - 1) as f32 * GAP - (groups - 1) as f32 * GGAP) / spb as f32).clamp(MIN_CW, MAX_CW);
        let bars = steps.div_ceil(spb) + 1;
        let fill = ((view / (cw + GAP)) as usize).div_ceil(spb);
        Cols { cw, spb, steps, total: bars.max(fill) * spb, view, sx: 0.0 }
    }

    /// Left of column `i`, from the start of the grid (unscrolled).
    fn offset(&self, i: usize) -> f32 {
        i as f32 * (self.cw + GAP) + (i / 4) as f32 * GGAP + (i / self.spb) as f32 * BGAP
    }

    fn content(&self) -> f32 {
        self.offset(self.total - 1) + self.cw
    }

    fn max_scroll(&self) -> f32 {
        (self.content() - self.view).max(0.0)
    }
}

enum RowOp {
    Delete(ChannelId),
    Duplicate(ChannelId),
    Move(ChannelId, bool),
}

/// Highest velocity among the channel's notes that start within the step (None = step off).
fn step_vel(pat: &Pattern, ch: ChannelId, step: usize) -> Option<u8> {
    let (a, b) = (step as Tick * STEP, (step as Tick + 1) * STEP);
    pat.notes.get(&ch)?.iter().filter(|n| n.start >= a && n.start < b).map(|n| n.velocity).max()
}

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
    let bar = project.bar_len();
    let pat_id = st.pattern;

    let res = skin::panel(ui, rect, "CHANNEL RACK", ORANGE, |ui| {
        // Current pattern + pattern management.
        let (name, color) = project.pattern(pat_id).map(|p| (p.name.clone(), id_color(p.id.0))).unwrap_or_default();
        skin::dropdown(ui, "rack_pat", vec2(170.0, 38.0), Some((color, false)), &name, None, |ui| {
            skin::menu_caption(ui, "PATTERNS");
            let mut pick = None;
            for p in &project.patterns {
                if skin::menu_item(ui, &p.name, TEXT, p.id == st.pattern).clicked() {
                    pick = Some(p.id);
                }
            }
            if let Some(p) = pick {
                st.pattern = p;
            }
            skin::menu_separator(ui);
            if skin::menu_item(ui, tr!("+ Nouveau pattern", "+ New pattern"), TEXT, false).clicked() {
                st.pattern = seq_edit::new_pattern(project);
            }
            if skin::menu_item(ui, tr!("Cloner", "Clone"), TEXT, false).clicked()
                && let Some(id) = seq_edit::clone_pattern(project, st.pattern)
            {
                st.pattern = id;
            }
            if skin::menu_item(ui, tr!("Renommer…", "Rename…"), TEXT, false).clicked() {
                st.renaming_pattern = Some((st.pattern, name.clone()));
            }
            if skin::menu_item(ui, tr!("Supprimer", "Delete"), DANGER, false).clicked() && !seq_edit::delete_pattern(project, st.pattern) {
                st.status = tr!("Impossible de supprimer le dernier pattern", "Can't delete the last pattern").into();
            }
        });
        // Length: − / + one bar. Painting past the end also grows the pattern.
        if let Some(pat) = project.pattern(pat_id) {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (len, shorter, longer) = (pat.length, seq_edit::shorter_length(pat, bar), seq_edit::longer_length(pat, bar));
            let tip = tr!("Retirer la dernière mesure (si elle est vide)", "Remove the last bar (if it is empty)");
            let minus = ui
                .add_enabled_ui(shorter.is_some(), |ui| skin::text_button(ui, vec2(32.0, 38.0), Kind::Dark, "−", body(20.0)))
                .inner
                .on_hover_text(tip)
                .on_disabled_hover_text(tip);
            if minus.clicked()
                && let Some(l) = shorter
            {
                seq_edit::set_pattern_length(project, pat_id, l);
            }
            let label = match len / bar {
                1 if len == bar => tr!("1 mes.", "1 bar").to_string(),
                n if len % bar == 0 => trf!("{} mes.", "{} bars", n),
                _ => trf!("{} pas", "{} steps", len.div_ceil(STEP)),
            };
            let font = body(14.0);
            let w = ui.fonts(|f| f.layout_no_wrap(label.clone(), font.clone(), TEXT).size().x).max(40.0) + 4.0;
            let (r, _) = ui.allocate_exact_size(vec2(w, 38.0), Sense::hover());
            ui.painter().text(r.center(), Align2::CENTER_CENTER, label, font, TEXT);
            if skin::text_button(ui, vec2(32.0, 38.0), Kind::Dark, "+", body(20.0))
                .on_hover_text(tr!("Ajouter une mesure (ou peindre après la fin du pattern)", "Add a bar (or paint past the end of the pattern)"))
                .clicked()
            {
                seq_edit::set_pattern_length(project, pat_id, longer);
            }
        }
        if ui.available_width() > 140.0 {
            skin::spacer_right(ui, 130.0);
            skin::hint(ui, tr!("Appui long = menu", "Long press = menu"));
        }
    });
    if res.expand {
        st.toggle_focus(PanelId::Rack);
    }

    if !ui.ctx().is_context_menu_open() {
        st.replacing = None;
    }

    let inner = res.body.shrink2(vec2(12.0, 10.0));
    let rows = Rect::from_min_max(inner.min, pos2(inner.right(), inner.bottom() - HBAR_H - 6.0));
    let hbar = Rect::from_min_max(pos2(inner.left(), rows.bottom() + 6.0), inner.max);
    let mut bu = ui.new_child(UiBuilder::new().max_rect(rows));
    bu.set_clip_rect(rows);
    let pointer = Pointer::read(&bu);
    let now = ui.input(|i| i.time);
    let playing_step = (info.playing && !st.song_mode).then(|| (info.playhead / STEP as f64) as usize);
    let spb = (bar / STEP).max(1) as usize;
    let mut op = None;
    let mut cols = None;

    // Two fingers: the step paint started by the first finger is cancelled.
    if pointer.multi
        && st.step_paint.take().is_some()
        && let Some(before) = st.rack.before.take()
        && let Some(p) = project.patterns.iter_mut().find(|p| p.id == before.id)
    {
        *p = before;
    }
    if !pointer.any_down() {
        st.rack.before = None;
    }
    // Horizontal scroll: wheel (Shift+wheel, trackpad) or two fingers.
    if pointer.pos.is_some_and(|p| inner.contains(p)) && pointer.scroll.x != 0.0 {
        st.rack.scroll_x -= pointer.scroll.x;
        st.rack.scrolled_at = now;
    }

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut bu, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        let width = ui.available_width();
        let steps = project.pattern(pat_id).map(step_count).unwrap_or(0);
        let mut c = Cols::new(width - STEPS_X, spb, steps);
        // Follows the playhead page by page, unless the user just scrolled or is painting.
        let rs = &mut st.rack;
        if let Some(s) = playing_step.filter(|_| steps > 0).map(|s| s % steps)
            && now - rs.scrolled_at > FOLLOW_PAUSE
            && st.step_paint.is_none()
            && (c.offset(s) < rs.scroll_x || c.offset(s) + c.cw > rs.scroll_x + c.view)
        {
            rs.scroll_x = c.offset(s / spb * spb);
        }
        rs.scroll_x = rs.scroll_x.clamp(0.0, c.max_scroll());
        c.sx = rs.scroll_x;
        cols = Some(c);
        for i in 0..project.channels.len() {
            let ch = project.channels[i].clone();
            let (row, _) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::hover());
            if !ui.is_rect_visible(row) {
                continue;
            }
            let p = ui.painter().clone();
            let color = id_color(ch.id.0);

            // Mute / Solo
            let m_rect = Rect::from_min_size(row.min, vec2(44.0, 44.0));
            let s_rect = m_rect.translate(vec2(49.0, 0.0));
            for (r, label, on, tag) in [(m_rect, "M", ch.mute, 0u8), (s_rect, "S", ch.solo, 1u8)] {
                let resp = ui.interact(r, ui.id().with(("ms", ch.id.0, tag)), Sense::click());
                let fg = skin::paint_button(&p, r, 7.0, Kind::Tile, on, resp.is_pointer_button_down_on(), resp.hovered(), true);
                p.text(r.center(), Align2::CENTER_CENTER, label, cond(17.0), fg);
                if resp.clicked() {
                    let c = project.channels[i].make_mut();
                    if tag == 0 {
                        c.mute = !c.mute;
                    } else {
                        c.solo = !c.solo;
                    }
                }
                resp.on_hover_text(if tag == 0 { "Mute" } else { "Solo" });
            }

            // Channel tile
            let tile = Rect::from_min_size(row.min + vec2(98.0, 0.0), vec2(156.0, 44.0));
            let selected = st.channel == Some(ch.id);
            let tresp = ui.interact(tile, ui.id().with(("tile", ch.id.0)), Sense::click());
            p.rect_filled(tile, 7.0, if selected { TILE_SEL } else if tresp.hovered() { hex(0x40372f) } else { TILE });
            p.rect_stroke(tile, 7.0, if selected { Stroke::new(2.0_f32, ORANGE) } else { Stroke::new(1.0_f32, EDGE) });
            // Sample loading: blinking LED, dimmed name.
            let loading = st.loading.contains(&ch.id);
            let blink = loading && (ui.input(|i| i.time) * 4.0) as i64 % 2 == 1;
            skin::led(&p, pos2(tile.left() + 16.0, tile.center().y), 6.0, if blink { with_alpha(color, 60) } else { color }, !blink);
            if loading {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(125));
            }
            let name_color = if ch.mute || loading { with_alpha(TEXT, 115) } else { TEXT };
            p.with_clip_rect(tile.shrink(2.0)).text(pos2(tile.left() + 31.0, tile.center().y), Align2::LEFT_CENTER, &ch.name, body(16.0), name_color);
            let lvl = info.levels.get(&ch.id).map(|l| l.0.max(l.1)).unwrap_or(0.0);
            let frac = ((20.0 * lvl.max(1e-6).log10() + 48.0) / 48.0).clamp(0.0, 1.0);
            if frac > 0.0 {
                let lr = Rect::from_min_size(pos2(tile.left() + 1.0, tile.bottom() - 4.0), vec2((tile.width() - 2.0) * frac, 3.0));
                skin::hgradient(&p, lr, &[(0.0, METER_GREEN), (1.0, METER_YELLOW)]);
            }
            if tresp.clicked() {
                st.channel = Some(ch.id);
            }
            if tresp.double_clicked() {
                st.channel = Some(ch.id);
                st.active_editor = Editor::Piano;
                st.show_panel(PanelId::Piano);
            }
            let mut hover = trf!("{} — {}\nDouble-clic : piano roll · clic droit / appui long : menu", "{} — {}\nDouble-click: piano roll · right-click / long press: menu", ch.name, ch.kind.label());
            if loading {
                hover.push_str(tr!("\nChargement du sample…", "\nLoading sample…"));
            }
            let tresp = tresp.on_hover_text(hover);
            tresp.context_menu(|ui| {
                ui.set_min_width(290.0);
                skin::menu_caption(ui, &format!("{} · {}", ch.name.to_uppercase(), ch.kind.label().to_uppercase()));
                if st.replacing == Some(ch.id) {
                    replace_items(st, ui, project, &ch);
                    return;
                }
                if skin::menu_item(ui, tr!("Ouvrir dans le piano roll", "Open in piano roll"), TEXT, false).clicked() {
                    st.channel = Some(ch.id);
                    st.active_editor = Editor::Piano;
                    st.show_panel(PanelId::Piano);
                    ui.close_menu();
                }
                if skin::menu_item(ui, tr!("Renommer…", "Rename…"), TEXT, false).clicked() {
                    st.renaming_channel = Some((ch.id, ch.name.clone()));
                    ui.close_menu();
                }
                if skin::menu_item(ui, tr!("Dupliquer", "Duplicate"), TEXT, false).clicked() {
                    op = Some(RowOp::Duplicate(ch.id));
                    ui.close_menu();
                }
                if skin::menu_item(ui, tr!("Remplacer par…", "Replace with…"), TEXT, false).clicked() {
                    st.replacing = Some(ch.id);
                }
                note_clipboard_items(st, ui, project, pat_id, ch.id);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let w = (ui.available_width() - 4.0) / 2.0;
                    if skin::text_button(ui, vec2(w, 44.0), Kind::Dark, tr!("↑ Monter", "↑ Move up"), body(15.0)).clicked() {
                        op = Some(RowOp::Move(ch.id, true));
                        ui.close_menu();
                    }
                    if skin::text_button(ui, vec2(w, 44.0), Kind::Dark, tr!("↓ Descendre", "↓ Move down"), body(15.0)).clicked() {
                        op = Some(RowOp::Move(ch.id, false));
                        ui.close_menu();
                    }
                });
                skin::menu_separator(ui);
                if skin::menu_item(ui, tr!("Supprimer", "Delete"), DANGER, false).clicked() {
                    op = Some(RowOp::Delete(ch.id));
                    ui.close_menu();
                }
            });

            // Steps
            let steps = Rect::from_min_max(pos2(row.left() + STEPS_X, row.top()), row.max);
            let melodic = project.pattern(pat_id).and_then(|p| p.notes.get(&ch.id)).is_some_and(|n| !seq_edit::fits_steps(n));
            if melodic {
                note_preview(st, ui, &p, steps, project, pat_id, ch.id, color, playing_step);
            } else {
                steps_row(st, ui, &p, steps, project, pat_id, ch.id, color, playing_step, &pointer, &c);
            }
        }

        ui.add_space(4.0);
        let add = skin::button(ui, vec2(254.0, 40.0), Kind::Ghost, false, |p, r, c| {
            crate::icons::draw(p, Rect::from_center_size(pos2(r.center().x - 70.0, r.center().y), vec2(18.0, 18.0)), 18.0, Icon::PlusCircle, c, 2.0);
            p.text(pos2(r.center().x - 54.0, r.center().y), Align2::LEFT_CENTER, tr!("Ajouter un channel", "Add channel"), body(14.5), c);
        });
        let add_id = ui.make_persistent_id("add_channel_menu");
        if add.clicked() {
            ui.memory_mut(|m| m.toggle_popup(add_id));
        }
        egui::popup_above_or_below_widget(ui, add_id, &add, egui::AboveOrBelow::Above, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
            ui.set_min_width(310.0);
            st.add_channel_items(ui, project);
        });
    });
    if let Some(c) = cols {
        hscroll(st, ui, hbar, &c, &pointer, now);
    }

    match op {
        Some(RowOp::Delete(id)) => seq_edit::remove_channel(project, id),
        Some(RowOp::Duplicate(id)) => {
            if let Some(n) = seq_edit::duplicate_channel(project, id) {
                st.channel = Some(n);
            }
        }
        Some(RowOp::Move(id, up)) => seq_edit::move_channel(project, id, up),
        None => {}
    }
}

/// Channel menu: copy / cut / paste the channel's notes in the current pattern, on one
/// row without a caption (the menu must fit a phone in landscape). Paste replaces the channel's notes
/// (one undo step), at the copied positions.
fn note_clipboard_items(st: &mut SeqUi, ui: &mut Ui, project: &mut Project, pat: PatternId, ch: ChannelId) {
    let has_notes = project.pattern(pat).is_some_and(|p| p.notes.get(&ch).is_some_and(|n| !n.is_empty()));
    let n_clip = st.channel_clipboard.len();
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let w = (ui.available_width() - 8.0) / 3.0;
        let items = [(0, tr!("Copier notes", "Copy notes"), has_notes), (1, tr!("Couper notes", "Cut notes"), has_notes), (2, tr!("Coller notes", "Paste notes"), n_clip > 0)];
        for (i, label, enabled) in items {
            ui.add_enabled_ui(enabled, |ui| {
                let resp = skin::text_button(ui, vec2(w, 44.0), Kind::Dark, label, body(14.0));
                let resp = if i == 2 && enabled { resp.on_hover_text(trf!("Remplace les notes du channel par les {} notes copiées", "Replaces the channel's notes with the {} copied notes", n_clip)) } else { resp };
                if resp.clicked() {
                    chosen = Some(i);
                }
            });
        }
    });
    match chosen {
        Some(0) => {
            st.channel_clipboard = seq_edit::channel_notes(project, pat, ch);
            st.status = trf!("{} notes copiées", "{} notes copied", st.channel_clipboard.len());
        }
        Some(1) => {
            st.channel_clipboard = seq_edit::cut_channel_notes(project, pat, ch);
            st.status = trf!("{} notes coupées", "{} notes cut", st.channel_clipboard.len());
        }
        Some(_) => seq_edit::replace_channel_notes(project, pat, ch, &st.channel_clipboard),
        None => return,
    }
    ui.close_menu();
}

/// Channel menu, "Replace with" page: another instrument for the channel, which keeps
/// its notes and settings (the current one is highlighted).
fn replace_items(st: &mut SeqUi, ui: &mut Ui, project: &mut Project, ch: &Channel) {
    if skin::menu_item(ui, tr!("‹ Retour", "‹ Back"), TEXT_DIM, false).clicked() {
        st.replacing = None;
    }
    skin::menu_caption(ui, tr!("REMPLACER PAR", "REPLACE WITH"));
    let current_drum = match ch.kind {
        ChannelKind::Sampler { source: SampleSource::Builtin(d) } => Some(d),
        _ => None,
    };
    let mut kind = None;
    // Room for the whole page on a phone in landscape.
    let max_h = (ui.ctx().screen_rect().height() - 150.0).max(160.0);
    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
        if let Some(d) = crate::drum_grid(ui, current_drum) {
            kind = Some(ChannelKind::Sampler { source: SampleSource::Builtin(d) });
        }
        skin::menu_separator(ui);
        let is_file = matches!(ch.kind, ChannelKind::Sampler { source: SampleSource::File(_) });
        if skin::menu_item(ui, tr!("Fichier WAV…", "WAV file…"), TEXT, is_file).clicked() {
            st.act(UiAction::LoadSample(Some(ch.id)));
            st.replacing = None;
            ui.close_menu();
        }
        // Synth / MIDI out: the settings this channel had last time, if any.
        if skin::menu_item(ui, tr!("Synthé", "Synth"), TEXT, matches!(ch.kind, ChannelKind::Synth(_))).clicked() {
            kind = Some(ch.recall(ChannelKind::Synth(SynthParams::default())));
        }
        if skin::menu_item(ui, "MIDI out", TEXT, matches!(ch.kind, ChannelKind::MidiOut { .. })).clicked() {
            kind = Some(ch.recall(ChannelKind::MidiOut { channel: 0, out: 0 }));
        }
    });
    if let Some(kind) = kind {
        seq_edit::replace_kind(project, ch.id, kind);
        st.replacing = None;
        ui.close_menu();
    }
}

#[allow(clippy::too_many_arguments)]
fn steps_row(
    st: &mut SeqUi,
    ui: &mut Ui,
    p: &egui::Painter,
    area: Rect,
    project: &mut Project,
    pat_id: PatternId,
    ch: ChannelId,
    color: Color32,
    playing_step: Option<usize>,
    pointer: &Pointer,
    c: &Cols,
) {
    let Some(pat) = project.pattern(pat_id) else { return };
    let cell = |i: usize| Rect::from_min_size(pos2(area.left() + c.offset(i) - c.sx, area.top()), vec2(c.cw, area.height()));
    let cell_at = |x: f32| {
        (area.x_range().contains(x)).then(|| (0..c.total).find(|&i| cell(i).expand2(vec2(GAP / 2.0 + 1.0, 0.0)).x_range().contains(x))).flatten()
    };
    let resp = ui.interact(area, ui.id().with(("steps", ch.0)), Sense::click_and_drag());
    // The glow may overflow vertically, not onto the channel tile.
    let p = p.with_clip_rect(Rect::from_min_max(pos2(area.left() - 3.0, area.top() - 4.0), pos2(area.right(), area.bottom() + 4.0)).intersect(p.clip_rect()));

    let r = (c.cw * 0.15).clamp(2.0, 6.0);
    let playing = playing_step.filter(|_| c.steps > 0).map(|s| s % c.steps);
    for i in 0..c.total {
        let cr = cell(i);
        if cr.right() < area.left() - 3.0 {
            continue;
        }
        if cr.left() > area.right() {
            break;
        }
        let off_bg = if (i / 4) % 2 == 1 { STEP_OFF_B } else { STEP_OFF_A };
        match step_vel(pat, ch, i) {
            Some(v) => {
                let fill = if v >= 100 { color } else { mix(off_bg, color, 0.72) };
                skin::glow(&p, cr, r, with_alpha(color, 90), 3.0);
                p.rect_filled(cr, r, fill);
                p.rect_filled(Rect::from_min_max(pos2(cr.left(), cr.bottom() - 4.0), cr.max), r, Color32::from_black_alpha(56));
                p.line_segment([pos2(cr.left() + r, cr.top() + 1.0), pos2(cr.right() - r, cr.top() + 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(90)));
            }
            // Past the end of the pattern: painting a step there grows the pattern.
            None if i >= c.steps => {
                p.rect_filled(cr, r, with_alpha(off_bg, 45));
            }
            None => {
                p.rect_filled(cr, r, off_bg);
                p.line_segment([pos2(cr.left() + r, cr.top() + 1.5), pos2(cr.right() - r, cr.top() + 1.5)], Stroke::new(2.0_f32, Color32::from_black_alpha(60)));
            }
        }
        if playing == Some(i) {
            p.rect_stroke(cr.shrink(1.0), r, Stroke::new(2.0_f32, Color32::WHITE));
        }
    }
    // End of the pattern.
    if c.steps > 0 {
        let x = (cell(c.steps - 1).right() + cell(c.steps).left()) / 2.0;
        if area.x_range().contains(x) {
            crate::widgets::vline(&p, x, area.top() - 2.0, area.bottom() + 2.0, with_alpha(ORANGE, 170), 2.0);
        }
    }

    // Click = toggle; drag = paint the same state; right click = clear.
    if resp.hovered() && (pointer.primary_pressed || pointer.secondary_pressed)
        && let Some(i) = pointer.pos.and_then(|q| cell_at(q.x))
    {
        let target = pointer.primary_pressed && step_vel(pat, ch, i).is_none();
        st.rack.before = project.patterns.iter().find(|p| p.id == pat_id).cloned();
        st.step_paint = Some((ch, target));
        st.channel = Some(ch);
    }
    if let (Some((pc, val)), Some(pos)) = (st.step_paint, pointer.pos)
        && pc == ch
        && pointer.any_down()
        && area.expand(2.0).contains(pos)
        && let Some(i) = cell_at(pos.x)
    {
        seq_edit::set_step(project, pat_id, ch, i, val);
    }
}

/// Mini piano roll in place of the steps, for a channel the steps cannot show (several
/// keys, notes off the grid): the whole pattern across, the channel's key range from
/// bottom to top (an octave at least). A tap opens the piano roll.
#[allow(clippy::too_many_arguments)]
fn note_preview(
    st: &mut SeqUi,
    ui: &mut Ui,
    p: &egui::Painter,
    area: Rect,
    project: &Project,
    pat_id: PatternId,
    ch: ChannelId,
    color: Color32,
    playing_step: Option<usize>,
) {
    let Some(pat) = project.pattern(pat_id) else { return };
    let notes = pat.notes.get(&ch).map(Vec::as_slice).unwrap_or_default();
    let len = pat.length.max(1);
    let resp = ui.interact(area, ui.id().with(("preview", ch.0)), Sense::click());
    p.rect_filled(area, 6.0, STEP_OFF_A);
    // Beat bands, like the step groups.
    let beats = len.div_ceil(4 * STEP);
    let x_of = |t: Tick| area.left() + t as f32 / len as f32 * area.width();
    for b in (1..beats).step_by(2) {
        let r = Rect::from_min_max(pos2(x_of(b * 4 * STEP), area.top()), pos2(x_of(((b + 1) * 4 * STEP).min(len)), area.bottom()));
        p.rect_filled(r, 0.0, STEP_OFF_B);
    }
    let inner = area.shrink2(vec2(2.0, 4.0));
    let (lo, hi) = seq_edit::preview_keys(notes, 12);
    let row = inner.height() / (hi - lo + 1) as f32;
    let nh = row.clamp(2.0, 5.0);
    for n in notes {
        let y = inner.bottom() - (n.key - lo) as f32 * row - (row + nh) / 2.0;
        let x0 = x_of(n.start);
        let x1 = x_of((n.start + n.len).min(len)).max(x0 + 2.0);
        p.rect_filled(Rect::from_min_max(pos2(x0, y), pos2(x1 - 1.0, y + nh)).intersect(area), 1.0, color);
    }
    if let Some(s) = playing_step {
        let n = seq_edit::step_count(pat).max(1);
        let x = x_of((s % n) as Tick * STEP);
        p.line_segment([pos2(x, area.top()), pos2(x, area.bottom())], Stroke::new(2.0_f32, Color32::WHITE));
    }
    p.rect_stroke(area, 6.0, Stroke::new(1.0_f32, with_alpha(color, 110)));
    if resp.on_hover_text(tr!("Notes du piano roll · toucher pour l'ouvrir", "Piano roll notes · tap to open it")).clicked() {
        st.channel = Some(ch);
        st.active_editor = Editor::Piano;
        st.show_panel(PanelId::Piano);
    }
}

/// Horizontal scroll bar under the rows: overview of the pattern (bars, end) and of the
/// visible part. Tap = jump there, drag = scroll. On the left: the bars in sight.
fn hscroll(st: &mut SeqUi, ui: &mut Ui, rect: Rect, c: &Cols, pointer: &Pointer, now: f64) {
    let track = Rect::from_min_max(pos2(rect.left() + STEPS_X, rect.top()), pos2(rect.left() + STEPS_X + c.view, rect.bottom()));
    let first = (c.sx / (c.offset(c.spb).max(1.0))) as usize + 1;
    let last = ((c.sx + c.view) / c.offset(c.spb).max(1.0)).ceil().max(1.0) as usize;
    let p = ui.painter();
    let label = if first >= last { trf!("MES. {}", "BAR {}", first) } else { trf!("MES. {}–{}", "BARS {}–{}", first, last) };
    skin::caption(p, pos2(rect.left() + 2.0, rect.center().y), Align2::LEFT_CENTER, &label, TEXT_DIMMER);

    let k = track.width() / c.content().max(1.0);
    let x_of = |off: f32| track.left() + off * k;
    p.rect_filled(track, 5.0, INNER);
    if c.steps > 0 {
        let end = x_of(c.offset(c.steps - 1) + c.cw);
        p.rect_filled(Rect::from_min_max(track.min, pos2(end, track.bottom())), 5.0, with_alpha(ORANGE, 45));
    }
    for b in 1..c.total.div_ceil(c.spb) {
        let x = x_of(c.offset(b * c.spb)) - BGAP * k / 2.0;
        crate::widgets::vline(p, x, track.top() + 3.0, track.bottom() - 3.0, Color32::from_white_alpha(28), 1.0);
    }
    let thumb = Rect::from_min_max(pos2(x_of(c.sx), track.top()), pos2(x_of(c.sx + c.view).min(track.right()), track.bottom()));
    p.rect_filled(thumb, 5.0, Color32::from_white_alpha(40));
    p.rect_stroke(thumb, 5.0, Stroke::new(1.0_f32, Color32::from_white_alpha(110)));

    let resp = ui.interact(track.expand2(vec2(0.0, 4.0)), ui.id().with("rack_hscroll"), Sense::click_and_drag());
    let rs = &mut st.rack;
    if resp.hovered()
        && pointer.primary_pressed
        && let Some(pos) = pointer.pos.filter(|p| !thumb.x_range().contains(p.x))
    {
        rs.scroll_x = (pos.x - track.left()) / k - c.view / 2.0;
        rs.scrolled_at = now;
    }
    if resp.dragged() {
        rs.scroll_x += resp.drag_delta().x / k;
        rs.scrolled_at = now;
    }
    rs.scroll_x = rs.scroll_x.clamp(0.0, c.max_scroll());
}
