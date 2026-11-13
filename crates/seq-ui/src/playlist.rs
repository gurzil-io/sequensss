//! Timeline (playlist): pattern palette, tracks × time, clips, multi-selection,
//! selection loop.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::widgets::{Pointer, vline};
use crate::{EditCmd, Editor, PanelId, PlaybackInfo, SeqUi, UiAction};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use seq_edit::{snap_floor, snap_round};
use seq_i18n::{tr, trf};
use seq_model::*;
use std::collections::HashMap;

const RULER_H: f32 = 28.0;
const EDGE_PX: f32 = 8.0;
const PALETTE_W: f32 = 230.0;

#[derive(Clone, PartialEq)]
enum Drag {
    /// `fresh` = clip created by this press (removed if the gesture becomes a two-finger gesture).
    Move { anchor: usize, grab: i64, orig: Vec<(usize, Clip)>, fresh: bool },
    Resize { anchor: usize, orig: Vec<(usize, Clip)> },
    Marquee { from: Pos2, additive: bool, base: Vec<usize> },
    Erase,
    Loop { anchor: Tick, moved: bool },
    /// Brush: `brush` = copied clip (cell 0 = `brush.start`, fixed track), `last` = last
    /// painted cell, `before` = clips before the gesture (restored on two fingers).
    Paint { brush: Clip, last: i64, before: Vec<Clip> },
}

/// Menu opened by a double tap: on a palette pattern, or on a clip (index).
#[derive(Clone, Copy, PartialEq)]
enum TapMenu {
    Pattern(PatternId),
    Clip(usize),
}

pub struct PlaylistState {
    scroll_x: f32,
    scroll_y: f32,
    zoom: f32,
    /// 0 = bar
    snap: Tick,
    drag: Option<Drag>,
    /// Sorted indices of the selected clips.
    selected: Vec<usize>,
    /// Clips seen in the previous frame (changed elsewhere → selection cleared).
    seen: Option<Vec<Clip>>,
    clipboard: Vec<Clip>,
    /// Brush tool: press on a clip then drag = end-to-end copies; on empty space =
    /// clips of the selected pattern.
    paint: bool,
    menu: Option<TapMenu>,
    /// Clip created by a tap on empty space, and when: the second tap of a double tap
    /// there must not open its menu.
    fresh: Option<(usize, f64)>,
}

impl Default for PlaylistState {
    fn default() -> Self {
        PlaylistState {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 0.0,
            snap: 0,
            drag: None,
            selected: Vec::new(),
            seen: None,
            clipboard: Vec::new(),
            paint: false,
            menu: None,
            fresh: None,
        }
    }
}

fn set_selection(sel: &mut Vec<usize>, mut v: Vec<usize>) {
    v.sort_unstable();
    v.dedup();
    *sel = v;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SnapSel {
    Bar,
    Beat,
    Step,
}

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
    let bar = project.bar_len();
    let beat = project.beat_len();

    let res = skin::panel(ui, rect, "TIMELINE", LCD_GREEN, |ui| {
        let mut s = match st.playlist.snap {
            0 => SnapSel::Bar,
            x if x == STEP => SnapSel::Step,
            _ => SnapSel::Beat,
        };
        if skin::segmented(ui, &mut s, &[(SnapSel::Bar, None, tr!("Mesure", "Bar")), (SnapSel::Beat, None, tr!("Temps", "Beat")), (SnapSel::Step, None, tr!("Pas", "Step"))], 38.0, 0.0, skin::SegStyle::Grey, body(14.0)) {
            st.playlist.snap = match s {
                SnapSel::Bar => 0,
                SnapSel::Beat => beat,
                SnapSel::Step => STEP,
            };
        }
        let paint = st.playlist.paint;
        if skin::button(ui, vec2(46.0, 38.0), Kind::Dark, paint, |p, r, c| {
            crate::icons::draw(p, r, 18.0, crate::icons::Icon::Brush, if paint { ORANGE } else { c }, 1.9);
        })
        .on_hover_text(tr!("Pinceau : glisser depuis un clip pour le copier bout à bout", "Paint: drag from a clip to copy it end to end"))
        .clicked()
        {
            st.playlist.paint = !paint;
        }
        if let Some((a, b)) = st.loop_region {
            let text = trf!("Boucle {} → {}", "Loop {} → {}", fmt_bar(a, bar), fmt_bar(b, bar));
            let font = body(14.0);
            let tw = ui.fonts(|f| f.layout_no_wrap(text.clone(), font.clone(), TEXT).size().x);
            let (r, resp) = ui.allocate_exact_size(vec2(tw + 46.0, 38.0), Sense::click());
            let p = ui.painter();
            p.rect_filled(r, 7.0, with_alpha(ORANGE, 36));
            p.rect_stroke(r, 7.0, Stroke::new(1.0_f32, with_alpha(ORANGE, 128)));
            p.text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, text, font, ORANGE_LIGHT);
            p.text(pos2(r.right() - 14.0, r.center().y), Align2::CENTER_CENTER, "×", body(20.0), ORANGE);
            if resp.on_hover_text(tr!("Supprimer la boucle", "Remove loop")).clicked() {
                st.loop_region = None;
            }
        }
        if skin::auto_button(ui, 38.0, Kind::Dark, tr!("+ piste", "+ track")).clicked() && project.arrangement.tracks < 64 {
            project.arrangement.tracks += 1;
        }
        if skin::auto_button(ui, 38.0, Kind::Dark, tr!("− piste", "− track")).clicked() && project.arrangement.tracks > 1 {
            let t = project.arrangement.tracks - 1;
            project.arrangement.clips.retain(|c| c.track < t);
            project.arrangement.tracks = t;
        }
        if !st.song_mode {
            ui.label(egui::RichText::new(tr!("Mode PAT : passe en SONG pour écouter", "PAT mode: switch to SONG to listen")).font(medium(13.0)).color(ORANGE_LIGHT));
        }
        if ui.available_width() > 330.0 {
            skin::spacer_right(ui, 320.0);
            skin::hint(ui, tr!("Glisser sur la règle = boucle · Toucher = position", "Drag on ruler = loop · Tap = position"));
        }
    });
    if res.expand {
        st.toggle_focus(PanelId::Timeline);
    }

    let area = res.body.shrink(10.0);
    let palette = Rect::from_min_size(area.min, vec2(PALETTE_W, area.height()));
    let arr = Rect::from_min_max(pos2(palette.right() + 10.0, area.top()), area.max);

    // Pattern info (copied: avoids a double borrow of `project`).
    let pat_info: HashMap<PatternId, (String, Tick, Vec<Note>)> = project
        .patterns
        .iter()
        .map(|p| (p.id, (p.name.clone(), p.length.max(1), p.notes.values().flatten().copied().collect())))
        .collect();

    // --- Palette ---
    let menu_id = ui.make_persistent_id("timeline_tap_menu");
    let mut tiles: Vec<(PatternId, Rect)> = Vec::new();
    {
        let p = ui.painter();
        skin::caption(p, palette.left_top() + vec2(0.0, 7.0), Align2::LEFT_CENTER, "PATTERNS", TEXT_DIMMER);
        let tile_w = (PALETTE_W - 6.0) / 2.0;
        let tile_h = 40.0;
        let rows_avail = ((palette.height() - 18.0 - tile_h - 6.0) / (tile_h + 6.0)).floor().max(1.0) as usize;
        let mut y = palette.top() + 18.0;
        let n = project.patterns.len();
        let shown = n.min(rows_avail * 2);
        for (i, pat) in project.patterns.iter().take(shown).enumerate() {
            let r = Rect::from_min_size(pos2(palette.left() + (i % 2) as f32 * (tile_w + 6.0), y), vec2(tile_w, tile_h));
            let resp = ui.interact(r, ui.id().with(("pal", pat.id.0)), Sense::click());
            let sel = st.pattern == pat.id;
            p.rect_filled(r, 7.0, if resp.hovered() { hex(0x2a2420) } else { INNER });
            p.rect_stroke(r, 7.0, if sel { Stroke::new(2.0_f32, ORANGE) } else { Stroke::new(1.0_f32, EDGE) });
            p.rect_filled(Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(12.0, 12.0)), 3.0, id_color(pat.id.0));
            p.with_clip_rect(r.shrink(2.0)).text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, &pat.name, body(15.0), TEXT);
            let resp = resp.on_hover_text(tr!(
                "Sélectionner, puis cliquer dans la timeline pour le poser · double-clic : supprimer…",
                "Select, then click in the timeline to place it · double-click: delete…"
            ));
            if resp.clicked() {
                st.pattern = pat.id;
            }
            if resp.double_clicked() {
                st.playlist.menu = Some(TapMenu::Pattern(pat.id));
                ui.memory_mut(|m| m.open_popup(menu_id));
            }
            tiles.push((pat.id, r));
            if i % 2 == 1 {
                y += tile_h + 6.0;
            }
        }
        if shown % 2 == 1 {
            y += tile_h + 6.0;
        }
        if shown < n {
            p.text(pos2(palette.left(), y - 2.0), Align2::LEFT_TOP, trf!("+ {} autres (menu du rack)", "+ {} more (rack menu)", n - shown), medium(12.0), TEXT_DIMMER);
            y += 16.0;
        }
        let add = Rect::from_min_size(pos2(palette.left(), y), vec2(PALETTE_W, tile_h));
        let mut au = ui.new_child(egui::UiBuilder::new().max_rect(add));
        if skin::text_button(&mut au, add.size(), Kind::Ghost, tr!("+ Nouveau pattern", "+ New pattern"), body(14.0)).clicked() {
            st.pattern = seq_edit::new_pattern(project);
        }
    }

    // --- Arrangement ---
    let p = ui.painter().clone();
    p.rect_filled(arr, 8.0, INNER);
    let ruler = Rect::from_min_size(arr.min, vec2(arr.width(), RULER_H));
    let grid = Rect::from_min_max(pos2(arr.left(), ruler.bottom() + 1.0), arr.max);
    let resp = ui.interact(arr, ui.id().with("timeline_area"), Sense::click_and_drag());
    let pointer = Pointer::read(ui);
    if resp.hovered() && (pointer.primary_pressed || pointer.secondary_pressed) {
        st.active_editor = Editor::Timeline;
    }
    let edit = st.take_edit(Editor::Timeline);
    let tracks = project.arrangement.tracks.max(1);
    let ps = &mut st.playlist;
    let track_h = ((grid.height() - 4.0) / tracks.min(4) as f32).clamp(26.0, 64.0);

    // Scroll / zoom (initial zoom: 16 bars across the width).
    if ps.zoom <= 0.0 {
        ps.zoom = (grid.width() / (16 * bar) as f32).max(0.004);
    }
    if resp.hovered() || pointer.multi {
        if pointer.zoom != 1.0 {
            let anchor = pointer.pos.map(|p| (p.x - grid.left() + ps.scroll_x) / ps.zoom).unwrap_or(0.0);
            ps.zoom = (ps.zoom * pointer.zoom).clamp(0.004, 0.5);
            if let Some(p) = pointer.pos {
                ps.scroll_x = anchor * ps.zoom - (p.x - grid.left());
            }
        }
        let (mut dx, mut dy) = (pointer.scroll.x, pointer.scroll.y);
        if pointer.shift && dx == 0.0 {
            dx = dy;
            dy = 0.0;
        }
        ps.scroll_x -= dx;
        ps.scroll_y -= dy;
    }
    let song_end = project.song_length().max(16 * bar);
    ps.scroll_x = ps.scroll_x.clamp(0.0, (song_end as f32 * ps.zoom - grid.width() * 0.5).max(0.0));
    ps.scroll_y = ps.scroll_y.clamp(0.0, (tracks as f32 * (track_h + 2.0) - grid.height()).max(0.0));

    let (zoom, sx, sy, th) = (ps.zoom, ps.scroll_x, ps.scroll_y, track_h);
    let x_of = |t: f64| grid.left() + t as f32 * zoom - sx;
    let tick_of = |x: f32| ((x - grid.left() + sx) / zoom).max(0.0) as Tick;
    let y_of = |track: u16| grid.top() + 2.0 + track as f32 * (th + 2.0) - sy;
    let track_of = |y: f32| (((y - grid.top() - 2.0 + sy) / (th + 2.0)).floor().max(0.0) as u16).min(tracks - 1);
    let snap = if ps.snap == 0 { bar } else { ps.snap };
    let clip_rect = |c: &Clip| {
        Rect::from_min_max(pos2(x_of(c.start as f64) + 1.0, y_of(c.track) + 2.0), pos2(x_of((c.start + c.len) as f64) - 1.0, y_of(c.track) + th - 2.0))
    };

    let clips = &mut project.arrangement.clips;
    if ps.seen.as_ref() != Some(&*clips) {
        ps.selected.clear();
        if matches!(ps.menu, Some(TapMenu::Clip(_))) {
            ps.menu = None;
        }
        if matches!(ps.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Marquee { .. })) {
            ps.drag = None;
        }
    }
    ps.selected.retain(|&i| i < clips.len());
    let mut actions = Vec::new();

    // --- Edit commands ---
    match edit {
        Some(EditCmd::SelectAll) => ps.selected = (0..clips.len()).collect(),
        Some(EditCmd::Deselect) => ps.selected.clear(),
        Some(EditCmd::Copy) if !ps.selected.is_empty() => ps.clipboard = seq_edit::copy_clips(clips, &ps.selected),
        Some(EditCmd::Paste) if !ps.clipboard.is_empty() => {
            let (at, track) = match pointer.pos.filter(|p| grid.contains(*p)) {
                Some(p) => (snap_floor(tick_of(p.x), snap), track_of(p.y)),
                None => match seq_edit::clips_span(clips, &ps.selected) {
                    Some((_, end, tr)) => (end.div_ceil(snap) * snap, tr),
                    None => (0, 0),
                },
            };
            let added = seq_edit::paste_clips(clips, &ps.clipboard, at, track, tracks);
            set_selection(&mut ps.selected, added);
        }
        Some(EditCmd::Duplicate) if !ps.selected.is_empty() => {
            let added = seq_edit::duplicate_clips(clips, &ps.selected, snap, tracks);
            set_selection(&mut ps.selected, added);
        }
        Some(EditCmd::Delete) if !ps.selected.is_empty() => {
            seq_edit::delete_clips(clips, &ps.selected);
            ps.selected.clear();
        }
        Some(EditCmd::Arrow { dx, dy, .. }) if !ps.selected.is_empty() => {
            seq_edit::shift_clips(clips, &ps.selected, dx as i64 * snap as i64, dy, tracks);
        }
        _ => {}
    }

    // --- Two fingers: the gesture started by the first finger is cancelled ---
    if pointer.multi
        && let Some(drag) = ps.drag.take()
    {
        match drag {
            Drag::Move { anchor, fresh: true, .. } if anchor < clips.len() => {
                clips.remove(anchor);
                ps.selected.clear();
            }
            Drag::Paint { before, .. } => {
                *clips = before;
                ps.selected.clear();
            }
            _ => {}
        }
    }

    // --- Mouse: press ---
    let hit = |clips: &[Clip], pos: Pos2| clips.iter().rposition(|c| clip_rect(c).contains(pos));
    let snapshot_of = |clips: &[Clip], sel: &[usize]| -> Vec<(usize, Clip)> { sel.iter().filter_map(|&i| clips.get(i).map(|c| (i, *c))).collect() };
    if let Some(pos) = pointer.pos.filter(|_| resp.hovered()) {
        if pointer.primary_pressed {
            if ruler.contains(pos) {
                ps.drag = Some(Drag::Loop { anchor: snap_round(tick_of(pos.x), snap), moved: false });
            } else if grid.contains(pos) {
                let t = tick_of(pos.x);
                match hit(clips, pos) {
                    // Brush: copies the touched clip, otherwise places the selected pattern.
                    h if ps.paint && !pointer.shift && !pointer.command => {
                        let before = clips.clone();
                        ps.selected.clear();
                        let brush = match h {
                            Some(idx) => Some(clips[idx]),
                            None => pat_info
                                .get(&st.pattern)
                                .map(|(_, len, _)| Clip { pattern: st.pattern, track: track_of(pos.y), start: snap_floor(t, snap), len: *len }),
                        };
                        if let Some(brush) = brush {
                            if h.is_none() {
                                seq_edit::paint_clips(clips, brush, 0, 0);
                            }
                            ps.drag = Some(Drag::Paint { brush, last: 0, before });
                        }
                    }
                    Some(idx) if pointer.shift => {
                        if let Some(p) = ps.selected.iter().position(|&i| i == idx) {
                            ps.selected.remove(p);
                        } else {
                            let mut v = ps.selected.clone();
                            v.push(idx);
                            set_selection(&mut ps.selected, v);
                        }
                    }
                    Some(idx) => {
                        if !ps.selected.contains(&idx) {
                            ps.selected = vec![idx];
                        }
                        if pointer.double_clicked {
                            st.pattern = clips[idx].pattern;
                        }
                        let orig = snapshot_of(clips, &ps.selected);
                        if pos.x >= clip_rect(&clips[idx]).right() - EDGE_PX {
                            ps.drag = Some(Drag::Resize { anchor: idx, orig });
                        } else {
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - clips[idx].start as i64, orig, fresh: false });
                        }
                    }
                    None if pointer.command || pointer.shift => {
                        let base = if pointer.shift { ps.selected.clone() } else { Vec::new() };
                        ps.drag = Some(Drag::Marquee { from: pos, additive: pointer.shift, base });
                    }
                    None => {
                        ps.selected.clear();
                        if let Some((_, len, _)) = pat_info.get(&st.pattern) {
                            let start = snap_floor(t, snap);
                            clips.push(Clip { pattern: st.pattern, track: track_of(pos.y), start, len: *len });
                            let idx = clips.len() - 1;
                            ps.fresh = Some((idx, pointer.time));
                            ps.selected = vec![idx];
                            let orig = snapshot_of(clips, &ps.selected);
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - start as i64, orig, fresh: true });
                        }
                    }
                }
            }
        } else if pointer.secondary_pressed {
            if ruler.contains(pos) {
                st.loop_region = None;
            } else if grid.contains(pos) {
                ps.drag = Some(Drag::Erase);
            }
        }
    }

    // --- Double tap on a clip: its menu (not on a clip just created by the first tap) ---
    if pointer.double_clicked
        && !ps.paint
        && !pointer.shift
        && !pointer.command
        && let Some(pos) = pointer.pos.filter(|p| resp.hovered() && grid.contains(*p))
        && let Some(idx) = hit(clips, pos)
        && !ps.fresh.is_some_and(|(i, t)| i == idx && pointer.time - t < 1.0)
    {
        st.pattern = clips[idx].pattern;
        ps.menu = Some(TapMenu::Clip(idx));
        ui.memory_mut(|m| m.open_popup(menu_id));
    }

    // --- Mouse: drag / release ---
    let mut marquee: Option<Rect> = None;
    if let (Some(drag), Some(pos)) = (ps.drag.clone(), pointer.pos) {
        if pointer.any_down() {
            match drag {
                Drag::Paint { brush, last, before } => {
                    let slot = seq_edit::paint_slot(tick_of(pos.x), brush.start, brush.len);
                    if slot != last {
                        let from = if slot > last { last + 1 } else { last - 1 };
                        seq_edit::paint_clips(clips, brush, from, slot);
                        ps.drag = Some(Drag::Paint { brush, last: slot, before });
                    }
                }
                Drag::Move { anchor, grab, orig, .. } => {
                    if let Some(&(_, a)) = orig.iter().find(|(i, _)| *i == anchor) {
                        let raw = (tick_of(pos.x) as i64 - grab).max(0) as Tick;
                        let dt = snap_round(raw, snap) as i64 - a.start as i64;
                        let dtr = track_of(pos.y) as i32 - a.track as i32;
                        let base: Vec<Clip> = orig.iter().map(|(_, c)| *c).collect();
                        let (dt, dtr) = seq_edit::clamp_clip_shift(&base, dt, dtr, tracks);
                        for &(i, c) in &orig {
                            if let Some(m) = clips.get_mut(i) {
                                m.start = (c.start as i64 + dt) as Tick;
                                m.track = (c.track as i32 + dtr) as u16;
                            }
                        }
                    }
                }
                Drag::Resize { anchor, orig } => {
                    if let Some(&(_, a)) = orig.iter().find(|(i, _)| *i == anchor) {
                        let end = snap_round(tick_of(pos.x), snap).max(a.start + snap);
                        let delta = (end - a.start) as i64 - a.len as i64;
                        for &(i, c) in &orig {
                            if let Some(m) = clips.get_mut(i) {
                                m.len = (c.len as i64 + delta).max(snap as i64) as Tick;
                            }
                        }
                    }
                }
                Drag::Marquee { from, additive, base } => {
                    let r = Rect::from_two_pos(from, pos);
                    marquee = Some(r);
                    let mut v: Vec<usize> = clips.iter().enumerate().filter(|(_, c)| clip_rect(c).intersects(r)).map(|(i, _)| i).collect();
                    if additive {
                        v.extend(base);
                    }
                    set_selection(&mut ps.selected, v);
                }
                Drag::Erase => {
                    if let Some(i) = hit(clips, pos) {
                        clips.remove(i);
                        ps.selected.clear();
                    }
                }
                Drag::Loop { anchor, moved } => {
                    let cur = snap_round(tick_of(pos.x), snap);
                    if cur != anchor || moved {
                        ps.drag = Some(Drag::Loop { anchor, moved: true });
                        let (a, b) = (anchor.min(cur), anchor.max(cur));
                        st.loop_region = (b > a).then_some((a, b));
                    }
                }
            }
        } else {
            if let Drag::Loop { moved: false, .. } = drag {
                actions.push(UiAction::Seek(snap_floor(tick_of(pos.x), STEP)));
            }
            ps.drag = None;
        }
    } else if !pointer.any_down() {
        ps.drag = None;
    }
    ps.seen = Some(clips.clone());
    let selected = ps.selected.clone();
    let is_sel = |i: usize| selected.binary_search(&i).is_ok();

    // --- Drawing ---
    let gp = p.with_clip_rect(grid.intersect(p.clip_rect()));
    for tr in 0..tracks {
        let y = y_of(tr);
        gp.rect_filled(Rect::from_min_max(pos2(grid.left(), y), pos2(grid.right(), y + th)), 0.0, if tr % 2 == 1 { INNER_ALT } else { INNER });
    }
    let t0 = tick_of(grid.left());
    let t1 = tick_of(grid.right()) + 1;
    let unit = if (beat as f32 * zoom) >= 10.0 { beat } else { bar };
    let mut t = snap_floor(t0, unit);
    while t <= t1 {
        let c = if t.is_multiple_of(bar) { hex(0x3a332c) } else { GRIDLINE };
        vline(&gp, x_of(t as f64), grid.top(), grid.bottom(), c, 1.0);
        t += unit;
    }
    if let Some((a, b)) = st.loop_region {
        gp.rect_filled(Rect::from_min_max(pos2(x_of(a as f64), grid.top()), pos2(x_of(b as f64), grid.bottom())), 0.0, with_alpha(ORANGE, 14));
    }

    for (i, c) in clips.iter().enumerate() {
        let r = clip_rect(c);
        if !r.intersects(grid) {
            continue;
        }
        let color = id_color(c.pattern.0);
        let Some((name, plen, notes)) = pat_info.get(&c.pattern) else { continue };
        let sel = is_sel(i);
        gp.rect_filled(r, 5.0, mix(INNER, color, if sel { 0.32 } else { 0.18 }));
        let head = Rect::from_min_size(r.min, vec2(r.width(), 17.0));
        gp.rect_filled(head, egui::Rounding { nw: 5.0, ne: 5.0, sw: 0.0, se: 0.0 }, color);
        gp.with_clip_rect(head.intersect(grid)).text(pos2(head.left() + 6.0, head.center().y), Align2::LEFT_CENTER, name, bold(12.0), hex(0x161210));
        // Note preview, repeated if the clip is longer than the pattern.
        let inner = Rect::from_min_max(pos2(r.left() + 4.0, head.bottom() + 3.0), pos2(r.right() - 4.0, r.bottom() - 3.0));
        if !notes.is_empty() && inner.height() > 6.0 {
            let (kmin, kmax) = notes.iter().fold((127u8, 0u8), |(a, b), n| (a.min(n.key), b.max(n.key)));
            let span = (kmax - kmin) as f32 + 1.0;
            let nh = (inner.height() / span).clamp(2.0, 4.0);
            let mut rep = 0;
            while rep * plen < c.len {
                for n in notes {
                    let s = rep * plen + n.start;
                    if s >= c.len {
                        continue;
                    }
                    let e = (s + n.len).min(c.len).min(rep * plen + plen);
                    let y = inner.bottom() - ((n.key - kmin) as f32 + 0.5) / span * (inner.height() - nh) - nh;
                    let x0 = x_of((c.start + s) as f64);
                    let nr = Rect::from_min_max(pos2(x0, y), pos2(x_of((c.start + e) as f64).max(x0 + 2.0), y + nh));
                    gp.rect_filled(nr.intersect(inner), 0.0, color);
                }
                rep += 1;
                if rep * plen < c.len {
                    vline(&gp, x_of((c.start + rep * plen) as f64), head.bottom(), r.bottom(), Color32::from_black_alpha(130), 1.0);
                }
            }
        }
        gp.rect_stroke(r, 5.0, if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::new(1.0_f32, color) });
    }
    if let Some(r) = marquee {
        gp.rect_filled(r, 0.0, with_alpha(ORANGE, 30));
        gp.rect_stroke(r, 0.0, Stroke::new(1.0_f32, ORANGE));
    }

    // Ruler
    let rp = p.with_clip_rect(ruler.intersect(p.clip_rect()));
    rp.rect_filled(ruler, egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 }, RULER);
    p.line_segment([pos2(ruler.left(), ruler.bottom() + 0.5), pos2(ruler.right(), ruler.bottom() + 0.5)], Stroke::new(1.0_f32, EDGE));
    if let Some((a, b)) = st.loop_region {
        let lr = Rect::from_min_max(pos2(x_of(a as f64).max(ruler.left()), ruler.top()), pos2(x_of(b as f64).min(ruler.right()), ruler.bottom()));
        if lr.width() > 0.0 {
            rp.rect_filled(lr, 0.0, with_alpha(ORANGE, 46));
            rp.rect_filled(Rect::from_min_max(pos2(lr.left(), lr.bottom() - 3.0), lr.max), 0.0, ORANGE);
        }
    }
    let label_every = ((40.0 / (bar as f32 * zoom)).ceil() as u64).max(1);
    let mut b = snap_floor(t0, bar);
    while b <= t1 {
        let x = x_of(b as f64);
        let n = b / bar;
        vline(&rp, x, ruler.top(), ruler.bottom(), RULER_LINE, 1.0);
        if n.is_multiple_of(label_every) {
            rp.text(pos2(x + 6.0, ruler.center().y), Align2::LEFT_CENTER, format!("{}", n + 1), mono(12.0), RULER_TEXT);
        }
        b += bar;
    }

    // Playhead (Song mode)
    if info.playing && st.song_mode {
        let x = x_of(info.playhead);
        if x >= grid.left() && x <= grid.right() {
            gp.rect_filled(Rect::from_center_size(pos2(x, grid.center().y), vec2(6.0, grid.height())), 0.0, with_alpha(LCD_GREEN, 30));
            vline(&gp, x, grid.top(), grid.bottom(), LCD_GREEN, 2.0);
        }
    }
    p.rect_stroke(arr, 8.0, Stroke::new(1.0_f32, EDGE));

    let anchor = match st.playlist.menu {
        Some(TapMenu::Pattern(id)) => tiles.iter().find(|(t, _)| *t == id).map(|(_, r)| *r),
        Some(TapMenu::Clip(i)) => project.arrangement.clips.get(i).map(|c| clip_rect(c).intersect(grid)),
        None => None,
    };
    match anchor {
        Some(r) if ui.memory(|m| m.is_popup_open(menu_id)) => tap_menu(st, ui, menu_id, r, project),
        Some(_) => st.playlist.menu = None,
        None => {
            // Pattern or clip gone (undo, deletion elsewhere): the menu goes too.
            if st.playlist.menu.take().is_some() && ui.memory(|m| m.is_popup_open(menu_id)) {
                ui.memory_mut(|m| m.close_popup());
            }
        }
    }

    st.actions.extend(actions);
}

/// Double-tap menu (palette pattern or clip). Deleting goes through a menu, never
/// straight from the double tap: a stray double touch must not remove anything.
fn tap_menu(st: &mut SeqUi, ui: &mut Ui, id: egui::Id, anchor: Rect, project: &mut Project) {
    let Some(menu) = st.playlist.menu else { return };
    let pat = match menu {
        TapMenu::Pattern(p) => p,
        TapMenu::Clip(i) => project.arrangement.clips[i].pattern,
    };
    let Some(name) = project.pattern(pat).map(|p| p.name.clone()) else { return };
    let uses = project.arrangement.clips.iter().filter(|c| c.pattern == pat).count();
    let anchor = ui.interact(anchor, id.with("anchor"), Sense::hover());
    let mut done = false;
    egui::popup_below_widget(ui, id, &anchor, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(260.0);
        skin::menu_caption(ui, &name.to_uppercase());
        if let TapMenu::Clip(i) = menu
            && skin::menu_item(ui, tr!("Supprimer ce clip", "Delete this clip"), DANGER, false).clicked()
        {
            seq_edit::delete_clips(&mut project.arrangement.clips, &[i]);
            done = true;
        }
        let label = match uses {
            0 => tr!("Supprimer le pattern", "Delete pattern").to_string(),
            1 => tr!("Supprimer le pattern et son clip", "Delete pattern and its clip").to_string(),
            n => trf!("Supprimer le pattern et ses {} clips", "Delete pattern and its {} clips", n),
        };
        if skin::menu_item(ui, &label, DANGER, false).clicked() {
            if !seq_edit::delete_pattern(project, pat) {
                st.status = tr!("Impossible de supprimer le dernier pattern", "Can't delete the last pattern").into();
            }
            done = true;
        }
    });
    if done {
        st.playlist.menu = None;
        st.playlist.selected.clear();
        ui.memory_mut(|m| m.close_popup());
    }
}

fn fmt_bar(t: Tick, bar: Tick) -> String {
    if t.is_multiple_of(bar) { format!("{}", t / bar + 1) } else { format!("{:.2}", t as f64 / bar as f64 + 1.0) }
}
