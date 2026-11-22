//! Timeline (playlist): pattern palette, tracks × time, clips, tools (draw / select / erase
//! / brush, as in the piano roll), multi-selection, selection loop, track mute.
//!
//! Double tap on a clip: deletes it (not with the eraser, which already does). Double tap
//! on a palette pattern: its menu (delete the pattern and its clips).
//!
//! Touch: two fingers scroll and zoom. With one finger, the strip under the lanes scrolls
//! horizontally and a drag on the mute column scrolls the tracks. The timeline always
//! reaches one screen past the end of the song, so there is room for new clips.

use crate::icons::Icon;
use crate::piano::{EDGE_PX, EDGE_TOUCH_IN, EDGE_TOUCH_OUT, Tool};
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::widgets::{self, Pointer, scroll_limit, vline};
use crate::{EditCmd, Editor, PanelId, PlaybackInfo, SeqUi, UiAction};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use seq_edit::{snap_floor, snap_round};
use seq_i18n::{tr, trf};
use seq_model::*;
use std::collections::HashMap;

const RULER_H: f32 = 28.0;
const PALETTE_W: f32 = 230.0;
/// Column of mute buttons, left of the tracks.
const LANE_W: f32 = 40.0;
/// Overview strip under the lanes (one-finger horizontal scroll), and the gap above it.
const HBAR_H: f32 = 16.0;
const HBAR_GAP: f32 = 6.0;
/// Even an empty song spans this many bars.
const MIN_BARS: Tick = 16;
/// Vertical drag (points) on the mute column before it scrolls rather than taps.
const SCROLL_SLOP: f32 = 8.0;

#[derive(Clone, PartialEq)]
enum Drag {
    /// `fresh` = clip created by this press (removed if the gesture becomes a two-finger gesture).
    Move { anchor: usize, grab: i64, orig: Vec<(usize, Clip)>, fresh: bool },
    Resize { anchor: usize, orig: Vec<(usize, Clip)> },
    Marquee { from: Pos2, additive: bool, base: Vec<usize> },
    /// Eraser (or right button): `before` = clips before the gesture (restored on two fingers).
    Erase { before: Vec<Clip> },
    Loop { anchor: Tick, moved: bool },
    /// Brush: `brush` = copied clip (cell 0 = `brush.start`, fixed track), `last` = last
    /// painted cell, `before` = clips before the gesture (restored on two fingers).
    Paint { brush: Clip, last: i64, before: Vec<Clip> },
    /// Press on the mute column: past a few points of vertical drag, scrolls the tracks
    /// (`y0`, `sy0` = starting position and scroll).
    ScrollY { y0: f32, sy0: f32, scrolling: bool },
}

pub struct PlaylistState {
    scroll_x: f32,
    scroll_y: f32,
    /// Horizontal scroll limit of the last frame (kept during a gesture, see [`scroll_limit`]).
    max_x: f32,
    zoom: f32,
    /// 0 = bar
    snap: Tick,
    drag: Option<Drag>,
    /// Sorted indices of the selected clips.
    selected: Vec<usize>,
    /// Clips seen in the previous frame (changed elsewhere → selection cleared).
    seen: Option<Vec<Clip>>,
    clipboard: Vec<Clip>,
    /// Draw / select / erase / brush. Brush: press on a clip then drag = end-to-end copies;
    /// on empty space = clips of the selected pattern.
    pub tool: Tool,
    /// Palette pattern whose menu is open (double tap on its tile).
    menu: Option<PatternId>,
    /// Clip created by a tap on empty space, and when: the second tap of a double tap
    /// there must not delete it.
    fresh: Option<(usize, f64)>,
    /// Selection when the current press began: restored if it becomes a two-finger gesture.
    press_sel: Option<Vec<usize>>,
}

impl Default for PlaylistState {
    fn default() -> Self {
        PlaylistState {
            scroll_x: 0.0,
            scroll_y: 0.0,
            max_x: 0.0,
            zoom: 0.0,
            snap: 0,
            drag: None,
            selected: Vec::new(),
            seen: None,
            clipboard: Vec::new(),
            tool: Tool::Draw,
            menu: None,
            fresh: None,
            press_sel: None,
        }
    }
}

/// Puts back the clips saved at the start of a gesture.
fn restore(clips: &mut [Clip], orig: &[(usize, Clip)]) {
    for &(i, c) in orig {
        if let Some(m) = clips.get_mut(i) {
            *m = c;
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

/// Header: tools, snap, tracks, loop. When everything does not fit with the tool names
/// (docked on a tablet, phone), the tools show their icons only and the loop its bars only.
fn header(st: &mut SeqUi, ui: &mut Ui, project: &mut Project, bar: Tick, beat: Tick) {
    let font = body(14.0);
    let text_w = |ui: &Ui, text: &str, font: &egui::FontId| ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), pal().text).size().x);
    let tools = [
        (Tool::Draw, Icon::Pencil, tr!("Dessiner", "Draw")),
        (Tool::Select, Icon::Select, tr!("Sélection", "Select")),
        (Tool::Erase, Icon::Eraser, tr!("Gomme", "Erase")),
        (Tool::Paint, Icon::Brush, tr!("Pinceau", "Paint")),
    ];
    let named: Vec<(Tool, Option<Icon>, &str)> = tools.iter().map(|&(t, i, l)| (t, Some(i), l)).collect();
    let snaps = [(SnapSel::Bar, None, tr!("Mesure", "Bar")), (SnapSel::Beat, None, tr!("Temps", "Beat")), (SnapSel::Step, None, tr!("Pas", "Step"))];
    let (add, remove) = (tr!("+ piste", "+ track"), tr!("− piste", "− track"));
    let loop_text = st.loop_region.map(|(a, b)| trf!("Boucle {} → {}", "Loop {} → {}", fmt_bar(a, bar), fmt_bar(b, bar)));
    let gap = ui.spacing().item_spacing.x;
    let full_w = skin::segmented_width(ui, &named, 0.0, &font)
        + skin::segmented_width(ui, &snaps, 0.0, &font)
        + text_w(ui, add, &body(14.5))
        + text_w(ui, remove, &body(14.5))
        + 56.0
        + loop_text.as_ref().map_or(0.0, |t| text_w(ui, t, &font) + 46.0 + gap)
        + 3.0 * gap;
    let narrow = ui.available_width() < full_w;
    let opts: Vec<(Tool, Option<Icon>, &str)> = named.iter().map(|&(t, i, l)| (t, i, if narrow { "" } else { l })).collect();
    skin::segmented(ui, &mut st.playlist.tool, &opts, 38.0, if narrow { 46.0 } else { 0.0 }, skin::SegStyle::Orange, font.clone());

    let mut s = match st.playlist.snap {
        0 => SnapSel::Bar,
        x if x == STEP => SnapSel::Step,
        _ => SnapSel::Beat,
    };
    if skin::segmented(ui, &mut s, &snaps, 38.0, 0.0, skin::SegStyle::Grey, font.clone()) {
        st.playlist.snap = match s {
            SnapSel::Bar => 0,
            SnapSel::Beat => beat,
            SnapSel::Step => STEP,
        };
    }
    if skin::auto_button(ui, 38.0, Kind::Dark, add).clicked() {
        seq_edit::add_track(project);
    }
    if skin::auto_button(ui, 38.0, Kind::Dark, remove).clicked() {
        seq_edit::remove_last_track(project);
    }
    if let (Some((a, b)), Some(full)) = (st.loop_region, loop_text) {
        let text = if narrow { format!("{} → {}", fmt_bar(a, bar), fmt_bar(b, bar)) } else { full };
        let tw = text_w(ui, &text, &font);
        let (r, resp) = ui.allocate_exact_size(vec2(tw + 46.0, 38.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(r, 7.0, with_alpha(pal().orange, 36));
        p.rect_stroke(r, 7.0, Stroke::new(1.0_f32, with_alpha(pal().orange, 128)));
        skin::gtext(p, pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, text, font.clone(), pal().orange_light);
        skin::gtext(p, pos2(r.right() - 14.0, r.center().y), Align2::CENTER_CENTER, "×", body(20.0), pal().orange);
        if resp.on_hover_text(tr!("Supprimer la boucle", "Remove loop")).clicked() {
            st.loop_region = None;
        }
    }
    if !st.song_mode {
        let long = tr!("Mode PAT : passe en SONG pour écouter", "PAT mode: switch to SONG to listen");
        let text = if ui.available_width() > text_w(ui, long, &medium(13.0)) + 8.0 { long } else { tr!("Mode PAT", "PAT mode") };
        ui.label(egui::RichText::new(text).font(medium(13.0)).color(pal().orange_light)).on_hover_text(long);
    }
    if ui.available_width() > 330.0 {
        skin::spacer_right(ui, 320.0);
        skin::hint(ui, tr!("Glisser sur la règle = boucle · Toucher = position", "Drag on ruler = loop · Tap = position"));
    }
}

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
    let bar = project.bar_len();
    let beat = project.beat_len();

    let res = skin::panel(ui, rect, "TIMELINE", pal().lcd_green, |ui| header(st, ui, project, bar, beat));
    if res.expand {
        st.toggle_focus(PanelId::Timeline);
    }

    let area = res.body.shrink(10.0);
    let palette = Rect::from_min_size(area.min, vec2(PALETTE_W, area.height()));
    let arr = Rect::from_min_max(pos2(palette.right() + 10.0, area.top()), pos2(area.right(), area.bottom() - HBAR_H - HBAR_GAP));
    let strip = Rect::from_min_max(pos2(arr.left() + LANE_W, arr.bottom() + HBAR_GAP), area.max);

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
        skin::caption(p, palette.left_top() + vec2(0.0, 7.0), Align2::LEFT_CENTER, "PATTERNS", pal().text_dimmer);
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
            p.rect_filled(r, 7.0, if resp.hovered() { pal().ruler } else { pal().inner });
            p.rect_stroke(r, 7.0, if sel { Stroke::new(2.0_f32, pal().orange) } else { Stroke::new(1.0_f32, pal().edge) });
            p.rect_filled(Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), vec2(12.0, 12.0)), 3.0, id_color(pat.id.0));
            p.with_clip_rect(r.shrink(2.0)).text(pos2(r.left() + 30.0, r.center().y), Align2::LEFT_CENTER, &pat.name, body(15.0), pal().text);
            let resp = resp.on_hover_text(tr!(
                "Sélectionner, puis cliquer dans la timeline pour le poser · double-clic : supprimer…",
                "Select, then click in the timeline to place it · double-click: delete…"
            ));
            if resp.clicked() {
                st.pattern = pat.id;
            }
            if resp.double_clicked() {
                st.playlist.menu = Some(pat.id);
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
            skin::gtext(&p, pos2(palette.left(), y - 2.0), Align2::LEFT_TOP, trf!("+ {} autres (menu du rack)", "+ {} more (rack menu)", n - shown), medium(12.0), pal().text_dimmer);
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
    p.rect_filled(arr, 8.0, pal().inner);
    let ruler_bg = Rect::from_min_size(arr.min, vec2(arr.width(), RULER_H));
    let lanes = Rect::from_min_max(pos2(arr.left(), ruler_bg.bottom() + 1.0), pos2(arr.left() + LANE_W, arr.bottom()));
    let ruler = Rect::from_min_max(pos2(lanes.right(), arr.top()), ruler_bg.right_bottom());
    let grid = Rect::from_min_max(pos2(lanes.right(), ruler.bottom() + 1.0), arr.max);
    let resp = ui.interact(arr, ui.id().with("timeline_area"), Sense::click_and_drag());
    let pointer = Pointer::read(ui);
    // Two fingers scroll the panel they are on (the first one), not every panel.
    let fingers_here = pointer.multi && pointer.pos.is_some_and(|p| arr.contains(p));
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
    if resp.hovered() || fingers_here {
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
    let song_end = project.song_length();
    let reach = reach(song_end, (grid.width() / ps.zoom) as Tick, bar);
    ps.max_x = scroll_limit(reach, ps.zoom, grid.width(), ps.drag.is_some().then_some(ps.max_x));
    ps.scroll_x = ps.scroll_x.clamp(0.0, ps.max_x);
    let content_h = tracks as f32 * (track_h + 2.0) + 2.0;
    ps.scroll_y = ps.scroll_y.clamp(0.0, (content_h - grid.height()).max(0.0));

    let (zoom, sx, sy, th) = (ps.zoom, ps.scroll_x, ps.scroll_y, track_h);
    let x_of = |t: f64| grid.left() + t as f32 * zoom - sx;
    let tick_of = |x: f32| ((x - grid.left() + sx) / zoom).max(0.0) as Tick;
    let y_of = |track: u16| grid.top() + 2.0 + track as f32 * (th + 2.0) - sy;
    let track_of = |y: f32| (((y - grid.top() - 2.0 + sy) / (th + 2.0)).floor().max(0.0) as u16).min(tracks - 1);
    let snap = if ps.snap == 0 { bar } else { ps.snap };
    let clip_rect = |c: &Clip| {
        Rect::from_min_max(pos2(x_of(c.start as f64) + 1.0, y_of(c.track) + 2.0), pos2(x_of((c.start + c.len) as f64) - 1.0, y_of(c.track) + th - 2.0))
    };

    let muted: Vec<bool> = (0..tracks).map(|t| project.arrangement.is_muted(t)).collect();
    let is_muted = |t: u16| muted.get(t as usize).copied().unwrap_or(false);
    let clips = &mut project.arrangement.clips;
    if ps.seen.as_ref() != Some(&*clips) {
        ps.selected.clear();
        if matches!(ps.drag, Some(Drag::Move { .. } | Drag::Resize { .. } | Drag::Marquee { .. })) {
            ps.drag = None;
        }
    }
    ps.selected.retain(|&i| i < clips.len());
    let mut actions = Vec::new();
    let (t0, t1) = (tick_of(grid.left()), tick_of(grid.right()));

    // --- Edit commands (keyboard, top bar) ---
    match edit {
        Some(EditCmd::SelectAll) => ps.selected = (0..clips.len()).collect(),
        Some(EditCmd::Deselect) => ps.selected.clear(),
        Some(EditCmd::Copy) if !ps.selected.is_empty() => ps.clipboard = seq_edit::copy_clips(clips, &ps.selected),
        Some(EditCmd::Cut) if !ps.selected.is_empty() => {
            ps.clipboard = seq_edit::cut_clips(clips, &ps.selected);
            ps.selected.clear();
        }
        Some(EditCmd::Paste) if !ps.clipboard.is_empty() => {
            // As in the piano roll: after the selection, otherwise under the mouse, otherwise
            // at the playhead when it is in sight (Song mode), otherwise at the left of the view.
            let at = seq_edit::paste_after_clips(clips, &ps.selected, snap)
                .or_else(|| pointer.pos.filter(|p| grid.contains(*p)).map(|p| (snap_floor(tick_of(p.x), snap), track_of(p.y))))
                .unwrap_or_else(|| {
                    let ph = info.playhead.max(0.0) as Tick;
                    let t = if st.song_mode && (t0..t1).contains(&ph) { snap_floor(ph, snap) } else { t0.div_ceil(snap) * snap };
                    (t, track_of(grid.top() + 2.0))
                });
            let added = seq_edit::paste_clips(clips, &ps.clipboard, at.0, at.1, tracks);
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
        // Arrows: by the snap (Shift: by a bar), up / down = track.
        Some(EditCmd::Arrow { dx, dy, big }) if !ps.selected.is_empty() => {
            let step = if big { bar } else { snap };
            seq_edit::shift_clips(clips, &ps.selected, dx as i64 * step as i64, dy, tracks);
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
            }
            Drag::Move { orig, .. } | Drag::Resize { orig, .. } => restore(clips, &orig),
            Drag::Paint { before, .. } | Drag::Erase { before } => *clips = before,
            _ => {}
        }
        if let Some(mut sel) = ps.press_sel.take() {
            sel.retain(|&i| i < clips.len());
            ps.selected = sel;
        }
    }

    // --- Press ---
    let tool = ps.tool;
    let hit = |clips: &[Clip], pos: Pos2| clips.iter().rposition(|c| clip_rect(c).contains(pos));
    let snapshot_of = |clips: &[Clip], sel: &[usize]| -> Vec<(usize, Clip)> { sel.iter().filter_map(|&i| clips.get(i).map(|c| (i, *c))).collect() };
    let any_touches = ui.input(|i| i.any_touches());
    // Clip whose resize handle is under `pos` (`h` = clip under `pos`). On touch the handle
    // is wider than it looks, and reaches a little past the end of the clip.
    let edge_of = |clips: &[Clip], h: Option<usize>, pos: Pos2| -> Option<usize> {
        match h {
            Some(i) => {
                let r = clip_rect(&clips[i]);
                let inside = if any_touches { (r.width() * 0.4).clamp(EDGE_PX, EDGE_TOUCH_IN) } else { EDGE_PX };
                (pos.x >= r.right() - inside).then_some(i)
            }
            None if any_touches => clips.iter().rposition(|c| {
                let r = clip_rect(c);
                (r.top()..=r.bottom()).contains(&pos.y) && pos.x >= r.right() && pos.x <= r.right() + EDGE_TOUCH_OUT
            }),
            None => None,
        }
    };
    let pressed = pointer.primary_pressed || pointer.secondary_pressed;
    if let Some(pos) = pointer.pos.filter(|_| resp.hovered() && !pointer.multi) {
        if pressed {
            ps.press_sel = Some(ps.selected.clone());
        }
        if pointer.primary_pressed {
            if ruler.contains(pos) {
                ps.drag = Some(Drag::Loop { anchor: snap_round(tick_of(pos.x), snap), moved: false });
            } else if lanes.contains(pos) {
                ps.drag = Some(Drag::ScrollY { y0: pos.y, sy0: ps.scroll_y, scrolling: false });
            } else if grid.contains(pos) {
                let t = tick_of(pos.x);
                let h = hit(clips, pos);
                let edge = edge_of(clips, h, pos);
                match (tool, h) {
                    (Tool::Erase, h) => {
                        let before = clips.clone();
                        if let Some(i) = h {
                            seq_edit::delete_clips(clips, &[i]);
                            ps.selected.clear();
                        }
                        ps.drag = Some(Drag::Erase { before });
                    }
                    // Brush: copies the touched clip, otherwise places the selected pattern.
                    (Tool::Paint, h) if !pointer.shift && !pointer.command => {
                        let before = clips.clone();
                        ps.selected.clear();
                        let brush = match h {
                            Some(idx) => Some(clips[idx]),
                            None => pat_info
                                .get(&st.pattern)
                                .map(|(_, len, _)| Clip { pattern: st.pattern, track: track_of(pos.y), start: snap_floor(t, snap), len: *len }),
                        };
                        if let Some(brush) = brush {
                            if h.is_none()
                                && let Some(&i) = seq_edit::paint_clips(clips, brush, 0, 0).first()
                            {
                                ps.fresh = Some((i, pointer.time));
                            }
                            ps.drag = Some(Drag::Paint { brush, last: 0, before });
                        }
                    }
                    // Shift+click: add to / remove from the selection.
                    (_, Some(idx)) if pointer.shift => {
                        if let Some(p) = ps.selected.iter().position(|&i| i == idx) {
                            ps.selected.remove(p);
                        } else {
                            let mut v = ps.selected.clone();
                            v.push(idx);
                            set_selection(&mut ps.selected, v);
                        }
                    }
                    // On a clip: selects it (its pattern becomes the current one), then moves
                    // the selection, or resizes it by the clip's right end.
                    (_, Some(idx)) => {
                        if !ps.selected.contains(&idx) {
                            ps.selected = vec![idx];
                        }
                        st.pattern = clips[idx].pattern;
                        let orig = snapshot_of(clips, &ps.selected);
                        if edge == Some(idx) {
                            ps.drag = Some(Drag::Resize { anchor: idx, orig });
                        } else {
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - clips[idx].start as i64, orig, fresh: false });
                        }
                    }
                    // Touch, just past the end of a clip: its resize handle.
                    (_, None) if !pointer.shift && !pointer.command && edge.is_some() => {
                        let idx = edge.unwrap_or_default();
                        if !ps.selected.contains(&idx) {
                            ps.selected = vec![idx];
                        }
                        let orig = snapshot_of(clips, &ps.selected);
                        ps.drag = Some(Drag::Resize { anchor: idx, orig });
                    }
                    // Select tool, or Ctrl/Shift + drag: rectangle.
                    (Tool::Select, None) | (Tool::Draw, None) if tool == Tool::Select || pointer.command || pointer.shift => {
                        let base = if pointer.shift { ps.selected.clone() } else { Vec::new() };
                        ps.drag = Some(Drag::Marquee { from: pos, additive: pointer.shift, base });
                    }
                    // Draw: a clip of the current pattern, moved while the finger slides.
                    (_, None) => {
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
                let before = clips.clone();
                if let Some(i) = hit(clips, pos) {
                    seq_edit::delete_clips(clips, &[i]);
                    ps.selected.clear();
                }
                ps.drag = Some(Drag::Erase { before });
            }
        }
    }

    // --- Double tap on a clip: delete it (not a clip just created by the first tap) ---
    if pointer.double_clicked
        && !pointer.multi
        && tool != Tool::Erase
        && !pointer.shift
        && !pointer.command
        && let Some(pos) = pointer.pos.filter(|p| resp.hovered() && grid.contains(*p))
        && let Some(idx) = hit(clips, pos)
        && !ps.fresh.is_some_and(|(i, t)| i == idx && pointer.time - t < 1.0)
    {
        // The second tap started a move, a resize or a brush stroke there: dropped with it.
        ps.drag = None;
        seq_edit::delete_clips(clips, &[idx]);
        ps.selected.clear();
        // The clips were drawn before this: show the deletion now, not at the next input.
        ui.ctx().request_repaint();
    }

    // --- Drag / release ---
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
                        seq_edit::move_clips(clips, &orig, dt, dtr, tracks);
                    }
                }
                Drag::Resize { anchor, orig } => {
                    seq_edit::resize_clips(clips, &orig, anchor, snap_round(tick_of(pos.x), snap), snap);
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
                // The press already erased what it landed on.
                Drag::Erase { .. } if pressed => {}
                Drag::Erase { .. } => {
                    if let Some(i) = hit(clips, pos) {
                        seq_edit::delete_clips(clips, &[i]);
                        ps.selected.clear();
                    }
                }
                Drag::ScrollY { y0, sy0, scrolling } => {
                    let dy = pos.y - y0;
                    if scrolling || dy.abs() > SCROLL_SLOP {
                        ps.drag = Some(Drag::ScrollY { y0, sy0, scrolling: true });
                        ps.scroll_y = (sy0 - dy).clamp(0.0, (content_h - grid.height()).max(0.0));
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
    let lp = p.with_clip_rect(lanes.intersect(p.clip_rect()));
    let mut toggle_mute = None;
    for tr in 0..tracks {
        let y = y_of(tr);
        let bg = if tr % 2 == 1 { pal().inner_alt } else { pal().inner };
        let lane = Rect::from_min_max(pos2(grid.left(), y), pos2(grid.right(), y + th));
        if is_muted(tr) && screen() {
            // Display: darkening a near-black lane would not show, so it is hatched.
            gp.rect_filled(lane, 0.0, bg);
            hatch(&gp, lane, with_alpha(pal().ink, 40));
        } else {
            gp.rect_filled(lane, 0.0, if is_muted(tr) { mix(bg, Color32::BLACK, 0.3) } else { bg });
        }
        // Mute button at the start of the lane, like the rack's.
        let row = Rect::from_min_max(pos2(lanes.left(), y), pos2(lanes.right(), y + th));
        if !row.intersects(lanes) {
            continue;
        }
        lp.rect_filled(row, 0.0, bg);
        let r = Rect::from_center_size(row.center(), vec2(LANE_W - 10.0, (th - 6.0).min(32.0)));
        let b = ui.interact(r.intersect(lanes), ui.id().with(("track_mute", tr)), Sense::click());
        let fg = skin::paint_button(&lp, r, 6.0, Kind::Tile, is_muted(tr), b.is_pointer_button_down_on(), b.hovered(), true);
        lp.text(r.center(), Align2::CENTER_CENTER, "M", cond(15.0), fg);
        if b.on_hover_text(trf!("Couper la piste {}", "Mute track {}", tr + 1)).clicked() {
            toggle_mute = Some(tr);
        }
    }
    vline(&p, lanes.right() - 0.5, lanes.top(), lanes.bottom(), pal().edge, 1.0);
    let t0 = tick_of(grid.left());
    let t1 = tick_of(grid.right()) + 1;
    let unit = if (beat as f32 * zoom) >= 10.0 { beat } else { bar };
    let mut t = snap_floor(t0, unit);
    while t <= t1 {
        let c = if t.is_multiple_of(bar) { pal().bar_line } else { pal().gridline };
        vline(&gp, x_of(t as f64), grid.top(), grid.bottom(), c, 1.0);
        t += unit;
    }
    if let Some((a, b)) = st.loop_region {
        gp.rect_filled(Rect::from_min_max(pos2(x_of(a as f64), grid.top()), pos2(x_of(b as f64), grid.bottom())), 0.0, with_alpha(pal().orange, 14));
    }

    for (i, c) in clips.iter().enumerate() {
        let r = clip_rect(c);
        if !r.intersects(grid) {
            continue;
        }
        // Clips on a muted track are dimmed.
        let color = if is_muted(c.track) { mix(pal().inner, id_color(c.pattern.0), 0.4) } else { id_color(c.pattern.0) };
        let Some((name, plen, notes)) = pat_info.get(&c.pattern) else { continue };
        let sel = is_sel(i);
        gp.rect_filled(r, 5.0, mix(pal().inner, color, if sel { 0.32 } else { 0.18 }));
        let head = Rect::from_min_size(r.min, vec2(r.width(), 17.0));
        gp.rect_filled(head, egui::Rounding { nw: 5.0, ne: 5.0, sw: 0.0, se: 0.0 }, color);
        gp.with_clip_rect(head.intersect(grid)).text(pos2(head.left() + 6.0, head.center().y), Align2::LEFT_CENTER, name, bold(12.0), pal().track);
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
        // Grip at the right end: drag it to resize.
        if r.width() > 36.0 && r.bottom() - head.bottom() > 8.0 {
            for dx in [5.0, 9.0] {
                vline(&gp, r.right() - dx, head.bottom() + 3.0, r.bottom() - 3.0, with_alpha(if sel { Color32::WHITE } else { color }, 150), 1.0);
            }
        }
    }
    if let Some(r) = marquee {
        gp.rect_filled(r, 0.0, with_alpha(pal().orange, 30));
        gp.rect_stroke(r, 0.0, Stroke::new(1.0_f32, pal().orange));
    }

    // Ruler
    let rp = p.with_clip_rect(ruler.intersect(p.clip_rect()));
    p.rect_filled(ruler_bg, egui::Rounding { nw: 8.0, ne: 8.0, sw: 0.0, se: 0.0 }, pal().ruler);
    p.line_segment([pos2(ruler_bg.left(), ruler.bottom() + 0.5), pos2(ruler.right(), ruler.bottom() + 0.5)], Stroke::new(1.0_f32, pal().edge));
    if let Some((a, b)) = st.loop_region {
        let lr = Rect::from_min_max(pos2(x_of(a as f64).max(ruler.left()), ruler.top()), pos2(x_of(b as f64).min(ruler.right()), ruler.bottom()));
        if lr.width() > 0.0 {
            rp.rect_filled(lr, 0.0, with_alpha(pal().orange, 46));
            rp.rect_filled(Rect::from_min_max(pos2(lr.left(), lr.bottom() - 3.0), lr.max), 0.0, pal().orange);
        }
    }
    let label_every = ((40.0 / (bar as f32 * zoom)).ceil() as u64).max(1);
    let mut b = snap_floor(t0, bar);
    while b <= t1 {
        let x = x_of(b as f64);
        let n = b / bar;
        vline(&rp, x, ruler.top(), ruler.bottom(), pal().ruler_line, 1.0);
        if n.is_multiple_of(label_every) {
            rp.text(pos2(x + 6.0, ruler.center().y), Align2::LEFT_CENTER, format!("{}", n + 1), mono(12.0), pal().ruler_text);
        }
        b += bar;
    }

    // Playhead (Song mode)
    if info.playing && st.song_mode {
        let x = x_of(info.playhead);
        if x >= grid.left() && x <= grid.right() {
            gp.rect_filled(Rect::from_center_size(pos2(x, grid.center().y), vec2(6.0, grid.height())), 0.0, with_alpha(pal().lcd_green, 30));
            vline(&gp, x, grid.top(), grid.bottom(), pal().lcd_green, 2.0);
        }
    }
    // More tracks than fit: where the view is, at the right edge.
    if content_h > grid.height() {
        let k = grid.height() / content_h;
        let bar_r = Rect::from_min_max(pos2(grid.right() - 6.0, grid.top() + sy * k), pos2(grid.right() - 2.0, grid.top() + (sy + grid.height()) * k));
        gp.rect_filled(bar_r, 2.0, pal().thumb_edge);
    }
    p.rect_stroke(arr, 8.0, Stroke::new(1.0_f32, pal().edge));
    overview(ui, strip, &mut st.playlist, &project.arrangement.clips, tracks, reach, song_end, grid.width());

    if let Some(tr) = toggle_mute {
        seq_edit::toggle_track_mute(project, tr);
    }
    let anchor = st.playlist.menu.and_then(|id| tiles.iter().find(|(t, _)| *t == id).map(|(_, r)| *r));
    match anchor {
        Some(r) if ui.memory(|m| m.is_popup_open(menu_id)) => tap_menu(st, ui, menu_id, r, project),
        Some(_) => st.playlist.menu = None,
        None => {
            // Pattern gone (undo, deletion elsewhere): the menu goes too.
            if st.playlist.menu.take().is_some() && ui.memory(|m| m.is_popup_open(menu_id)) {
                ui.memory_mut(|m| m.close_popup());
            }
        }
    }

    st.actions.extend(actions);
}

/// Double-tap menu of a palette pattern. Deleting a pattern goes through a menu, never
/// straight from the double tap: a stray double touch must not remove all its clips.
fn tap_menu(st: &mut SeqUi, ui: &mut Ui, id: egui::Id, anchor: Rect, project: &mut Project) {
    let Some(pat) = st.playlist.menu else { return };
    let Some(name) = project.pattern(pat).map(|p| p.name.clone()) else { return };
    let uses = project.arrangement.clips.iter().filter(|c| c.pattern == pat).count();
    let last = project.patterns.len() <= 1;
    // The last pattern stays: its clips can still go.
    let pattern_item = match (last, uses) {
        (false, 0) => Some(tr!("Supprimer le pattern", "Delete pattern").to_string()),
        (false, 1) => Some(tr!("Supprimer le pattern et son clip", "Delete pattern and its clip").to_string()),
        (false, n) => Some(trf!("Supprimer le pattern et ses {} clips", "Delete pattern and its {} clips", n)),
        (true, 0) => None,
        (true, 1) => Some(tr!("Supprimer son clip", "Delete its clip").to_string()),
        (true, n) => Some(trf!("Supprimer ses {} clips", "Delete its {} clips", n)),
    };
    let (anchor, side) = menu_anchor(anchor, menu_height(1), ui.ctx().screen_rect());
    let anchor = ui.interact(anchor, id.with("anchor"), Sense::hover());
    let mut done = false;
    egui::popup_above_or_below_widget(ui, id, &anchor, side, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(260.0);
        skin::menu_caption(ui, &name.to_uppercase());
        match &pattern_item {
            Some(label) if skin::menu_item(ui, label, pal().danger, false).clicked() => {
                if last {
                    seq_edit::delete_pattern_clips(project, pat);
                } else {
                    seq_edit::delete_pattern(project, pat);
                }
                done = true;
            }
            Some(_) => {}
            None => skin::menu_disabled(ui, tr!("Dernier pattern, impossible à supprimer", "Last pattern, can't be deleted")),
        }
    });
    if done {
        st.playlist.menu = None;
        st.playlist.selected.clear();
        ui.memory_mut(|m| m.close_popup());
    }
}

/// Height of the tap menu: caption, `rows` items of 40 points, frame margins.
fn menu_height(rows: usize) -> f32 {
    28.0 + rows as f32 * 44.0 + 20.0
}

/// Where the tap menu opens: below the anchor if it fits on screen, otherwise on the side
/// with the most room (egui then keeps it inside the screen). The anchor is narrowed so a
/// long clip does not stretch the menu across the timeline.
fn menu_anchor(anchor: Rect, menu_h: f32, screen: Rect) -> (Rect, egui::AboveOrBelow) {
    let anchor = Rect::from_min_size(anchor.min, vec2(anchor.width().min(260.0), anchor.height()));
    let below = screen.bottom() - anchor.bottom();
    let above = anchor.top() - screen.top();
    let side = if below >= menu_h || below >= above { egui::AboveOrBelow::Below } else { egui::AboveOrBelow::Above };
    (anchor, side)
}

/// How far the timeline reaches (ticks): the end of the song (at least `MIN_BARS` bars) plus
/// one screen (`view` ticks) of empty room, rounded up to a bar (see [`widgets::reach`]).
fn reach(song_end: Tick, view: Tick, bar: Tick) -> Tick {
    widgets::reach(song_end.max(MIN_BARS * bar.max(1)), view, bar)
}

/// Overview strip under the lanes: the whole reach with the song and its clips, and the
/// part in view. Tap = jump there, drag = scroll (one finger, while the lanes edit).
#[allow(clippy::too_many_arguments)]
fn overview(ui: &mut Ui, rect: Rect, ps: &mut PlaylistState, clips: &[Clip], tracks: u16, reach: Tick, song_end: Tick, view_w: f32) {
    let inner = rect.shrink2(vec2(0.0, 3.0));
    // Rows overlap when there are many tracks: a clip stays at least 2 points high.
    let row = inner.height() / tracks.max(1) as f32;
    let marks = |p: &egui::Painter, x_of: &dyn Fn(f32) -> f32| {
        for c in clips {
            let y = inner.top() + c.track as f32 * row;
            let r = Rect::from_min_max(pos2(x_of(c.start as f32), y), pos2(x_of((c.start + c.len) as f32).max(x_of(c.start as f32) + 1.5), y + row.max(2.0)));
            p.rect_filled(r.intersect(rect), 0.0, with_alpha(id_color(c.pattern.0), 190));
        }
    };
    widgets::overview_strip(ui, rect, "timeline_hscroll", reach, song_end, &mut ps.scroll_x, ps.zoom, view_w, ps.max_x, marks);
}

fn fmt_bar(t: Tick, bar: Tick) -> String {
    if t.is_multiple_of(bar) { format!("{}", t / bar + 1) } else { format!("{:.2}", t as f64 / bar as f64 + 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tap_menu_stays_on_a_phone_screen() {
        // 2400 × 1080 px landscape phone at 2.625 px per point.
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(914.0, 411.0));
        let h = menu_height(1);
        let side = |top: f32| menu_anchor(Rect::from_min_size(pos2(300.0, top), vec2(900.0, 60.0)), h, screen);
        // Room below: below. Clip near the bottom: above, and fits.
        assert!(matches!(side(80.0).1, egui::AboveOrBelow::Below));
        let (r, s) = side(300.0);
        assert!(matches!(s, egui::AboveOrBelow::Above));
        assert!(r.top() - h >= screen.top());
        // A long clip does not stretch the menu.
        assert_eq!(r.width(), 260.0);
    }

    #[test]
    fn timeline_reaches_a_screen_past_the_song() {
        let bar = 3840;
        // Empty song: 16 bars, plus a screen of 16 bars.
        assert_eq!(reach(0, 16 * bar, bar), 32 * bar);
        // Long song: its end plus the screen, rounded up to a bar.
        assert_eq!(reach(40 * bar + 10, 8 * bar, bar), 49 * bar);
        // So the view scrolls until the song end meets its left edge: a whole screen free.
        let zoom = 0.01;
        let view_w = 8.0 * bar as f32 * zoom;
        let max = scroll_limit(reach(40 * bar, 8 * bar, bar), zoom, view_w, None);
        assert!((max - 40.0 * bar as f32 * zoom).abs() < 0.01);
        // Placing a clip out there pushes the reach further.
        assert!(reach(47 * bar, 8 * bar, bar) > reach(40 * bar, 8 * bar, bar));
    }
}

/// Diagonal hatching over `rect` (muted lane, Display theme).
fn hatch(p: &egui::Painter, rect: Rect, color: Color32) {
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    if p.clip_rect().is_negative() {
        return;
    }
    let h = rect.height();
    let stroke = Stroke::new(1.0_f32, color);
    let mut x = ((p.clip_rect().left() - h) / 10.0).floor() * 10.0;
    while x < p.clip_rect().right() {
        p.line_segment([pos2(x, rect.bottom()), pos2(x + h, rect.top())], stroke);
        x += 10.0;
    }
}
