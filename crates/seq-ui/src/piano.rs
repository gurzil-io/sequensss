//! Piano roll: keyboard, grid, notes, tools (draw / select / erase / brush),
//! multi-selection, copy/paste, velocity lane, "Arpeggiate".
//!
//! Brush: press on a note then drag = end-to-end copies of that note; on empty
//! space = notes of the last used length. Pitch follows the finger.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::widgets::{Pointer, hline, vline};
use crate::{EditCmd, Editor, PanelId, PlaybackInfo, SeqUi, UiAction};
use crate::icons::Icon;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use seq_edit::{snap_floor, snap_round};
use seq_i18n::{tr, trf};
use seq_model::*;

const KB_W: f32 = 72.0;
const VEL_H: f32 = 56.0;
const EDGE_PX: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Draw,
    Select,
    Erase,
    Paint,
}

#[derive(Clone, PartialEq)]
enum Drag {
    /// Brush: `brush` = copied note (cell 0 = `brush.start`), `last` = last painted cell,
    /// `before` = notes before the gesture (restored if the gesture becomes a two-finger gesture).
    Paint { brush: Note, last: i64, before: Vec<Note> },
    /// Moving the selection; `anchor` = grabbed note, `orig` = initial state,
    /// `fresh` = note created by this press (removed if the gesture becomes a two-finger gesture).
    Move { anchor: usize, grab: i64, key: u8, orig: Vec<(usize, Note)>, fresh: bool },
    /// Resizing the selection, dragged by the right edge of `anchor`.
    Resize { anchor: usize, orig: Vec<(usize, Note)> },
    /// Selection rectangle.
    Marquee { from: Pos2, additive: bool, base: Vec<usize> },
    Velocity,
    Erase,
    /// Press on the keyboard: plays `key`; beyond a few points of vertical drag,
    /// scrolls the octaves (`y0`, `sy0` = starting position and scroll).
    Keyboard { key: u8, y0: f32, sy0: f32, scrolling: bool },
}

/// Vertical drag (points) beyond which a keyboard press becomes a scroll.
const KB_SCROLL_SLOP: f32 = 8.0;

pub struct PianoState {
    scroll_x: f32,
    scroll_y: f32,
    /// pixels per tick
    zoom: f32,
    key_h: f32,
    snap: Tick,
    pub tool: Tool,
    last_len: Tick,
    drag: Option<Drag>,
    /// Sorted indices of the selected notes.
    pub(crate) selected: Vec<usize>,
    /// Notes seen at the end of the previous frame: if they change elsewhere
    /// (undo, step seq, recording), the selection is cleared.
    seen: Option<(PatternId, ChannelId, Vec<Note>)>,
    clipboard: Vec<Note>,
    centered_for: Option<ChannelId>,
    /// Pattern length when the current gesture began: notes placed past the end grow the
    /// pattern, and dragging them back shrinks it again, never below this length.
    grow_base: Option<(PatternId, Tick)>,
    /// Note created by a tap on empty space, and when: the second tap of a double tap
    /// there must not delete it.
    fresh: Option<(usize, f64)>,
}

impl Default for PianoState {
    fn default() -> Self {
        PianoState {
            scroll_x: 0.0,
            scroll_y: 0.0,
            zoom: 0.16,
            key_h: 22.0,
            snap: STEP,
            tool: Tool::Draw,
            last_len: STEP,
            drag: None,
            selected: Vec::new(),
            seen: None,
            clipboard: Vec::new(),
            centered_for: None,
            grow_base: None,
            fresh: None,
        }
    }
}

fn set_selection(sel: &mut Vec<usize>, mut v: Vec<usize>) {
    v.sort_unstable();
    v.dedup();
    *sel = v;
}

/// Light / dark shade of a channel color (note gradient).
fn note_colors(c: Color32) -> (Color32, Color32, Color32) {
    (mix(c, Color32::WHITE, 0.3), mix(c, Color32::BLACK, 0.08), mix(c, Color32::BLACK, 0.55))
}

pub fn show(st: &mut SeqUi, ui: &mut Ui, rect: Rect, project: &mut Project, info: &PlaybackInfo) {
    let mut bake = false;
    let res = skin::panel(ui, rect, "PIANO ROLL", YELLOW, |ui| {
        let (cur, color) = st
            .channel
            .and_then(|c| project.channel(c))
            .map(|c| (c.name.clone(), id_color(c.id.0)))
            .unwrap_or(("—".into(), SEG_ON));
        skin::dropdown(ui, "pr_channel", vec2(150.0, 38.0), Some((color, true)), &cur, None, |ui| {
            skin::menu_caption(ui, "CHANNEL");
            for c in &project.channels {
                if skin::menu_item(ui, &c.name, TEXT, st.channel == Some(c.id)).clicked() {
                    st.channel = Some(c.id);
                }
            }
        });
        // Small screen: icon-only tools (the brush would not fit otherwise).
        let narrow = ui.available_width() < 1000.0;
        let tools = [
            (Tool::Draw, Icon::Pencil, tr!("Dessiner", "Draw")),
            (Tool::Select, Icon::Select, tr!("Sélection", "Select")),
            (Tool::Erase, Icon::Eraser, tr!("Gomme", "Erase")),
            (Tool::Paint, Icon::Brush, tr!("Pinceau", "Paint")),
        ];
        let opts: Vec<(Tool, Option<Icon>, &str)> = tools.iter().map(|&(t, i, l)| (t, Some(i), if narrow { "" } else { l })).collect();
        skin::segmented(ui, &mut st.piano.tool, &opts, 38.0, if narrow { 46.0 } else { 0.0 }, skin::SegStyle::Orange, body(14.0));
        skin::caption(ui.painter(), ui.cursor().left_center() + vec2(2.0, 0.0), Align2::LEFT_CENTER, "SNAP", TEXT_DIMMER);
        ui.add_space(38.0);
        skin::segmented(
            ui,
            &mut st.piano.snap,
            &[(PPQ as Tick / 2, None, "1/8"), (STEP, None, "1/16"), (PPQ as Tick / 8, None, "1/32"), (PPQ as Tick / 6, None, tr!("Triol.", "Triplet")), (1, None, tr!("Libre", "Free"))],
            38.0,
            44.0,
            skin::SegStyle::Grey,
            body(14.0),
        );
        let label = if st.piano.selected.is_empty() { tr!("Arpéger", "Arpeggiate") } else { tr!("Arpéger la sél.", "Arpeggiate sel.") };
        if ui.available_width() > 120.0 {
            skin::spacer_right(ui, 130.0);
        }
        if skin::auto_button(ui, 44.0, Kind::Cream, label)
            .on_hover_text(tr!(
                "Remplace les notes (la sélection, sinon tout) par l'arpège qu'elles produisent, avec les réglages ARP du channel.\n\
                 L'ARP en direct du channel est ensuite désactivé pour ne pas arpéger deux fois.",
                "Replaces the notes (the selection, otherwise all) with the arpeggio they produce, using the channel's ARP settings.\n\
                 The channel's live ARP is then turned off so it doesn't arpeggiate twice.",
            ))
            .clicked()
        {
            bake = true;
        }
    });
    if res.expand {
        st.toggle_focus(PanelId::Piano);
    }

    let area = res.body.shrink(10.0);
    let Some(ch) = st.channel.filter(|c| project.channel(*c).is_some()) else {
        ui.painter().text(area.center(), Align2::CENTER_CENTER, tr!("Aucun channel", "No channel"), body(16.0), TEXT_DIMMER);
        return;
    };
    let pat_id = st.pattern;
    let Some(pat_len) = project.pattern(pat_id).map(|p| p.length) else { return };
    let default_key = project.channel(ch).map(|c| c.default_key).unwrap_or(60);
    let bar = project.bar_len();
    let beat = project.beat_len();
    let color = id_color(ch.0);
    let (note_top, note_bot, note_edge) = note_colors(color);

    let roll = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - VEL_H - 6.0));
    let kb = Rect::from_min_max(roll.min, pos2(roll.left() + KB_W, roll.bottom()));
    let grid = Rect::from_min_max(pos2(kb.right(), roll.top()), roll.max);
    let vel_full = Rect::from_min_max(pos2(area.left(), roll.bottom() + 6.0), area.max);
    let vel = Rect::from_min_max(pos2(vel_full.left() + KB_W, vel_full.top()), vel_full.max);
    if grid.width() < 20.0 || grid.height() < 20.0 {
        return;
    }
    let resp = ui.interact(Rect::from_min_max(area.min, area.max), ui.id().with("piano_area"), Sense::click_and_drag());
    let pointer = Pointer::read(ui);
    if resp.hovered() && (pointer.primary_pressed || pointer.secondary_pressed) {
        st.active_editor = Editor::Piano;
    }
    let edit = st.take_edit(Editor::Piano);
    let ps = &mut st.piano;
    let base_len = match ps.grow_base {
        Some((p, len)) if p == pat_id => len,
        _ => pat_len,
    };
    // Room after the end of the pattern: one bar, where notes can be placed (the pattern grows).
    let reach = (base_len + bar).max(pat_len);

    // --- Scroll / zoom ---
    if ps.centered_for != Some(ch) {
        ps.centered_for = Some(ch);
        ps.scroll_y = (127.0 - default_key as f32) * ps.key_h - grid.height() / 2.0;
    }
    if resp.hovered() || pointer.multi {
        if pointer.zoom != 1.0 {
            let anchor_tick = pointer.pos.map(|p| (p.x - grid.left() + ps.scroll_x) / ps.zoom).unwrap_or(0.0);
            ps.zoom = (ps.zoom * pointer.zoom).clamp(0.01, 2.0);
            if let Some(p) = pointer.pos {
                ps.scroll_x = anchor_tick * ps.zoom - (p.x - grid.left());
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
    // Limited zoom-out: the pattern and the bar after it (at least 4 bars) fill the width.
    let fit = grid.width() / reach.max(4 * bar) as f32;
    ps.zoom = ps.zoom.max(fit);
    ps.scroll_y = ps.scroll_y.clamp(0.0, (128.0 * ps.key_h - grid.height()).max(0.0));
    ps.scroll_x = ps.scroll_x.clamp(0.0, (reach as f32 * ps.zoom - grid.width()).max(0.0));
    // Notes can be placed anywhere in sight, and at least one bar past the end.
    let limit = reach.max((((ps.scroll_x + grid.width()) / ps.zoom) as Tick).div_ceil(bar) * bar);
    // Paste / duplicate: as far as needed.
    let far = limit.max(1024 * bar);

    let (zoom, sx, sy, kh) = (ps.zoom, ps.scroll_x, ps.scroll_y, ps.key_h);
    let x_of = |t: f64| grid.left() + t as f32 * zoom - sx;
    let tick_of = |x: f32| ((x - grid.left() + sx) / zoom).max(0.0) as f64;
    let y_of = |key: u8| grid.top() + (127 - key) as f32 * kh - sy;
    let key_of = |y: f32| (127 - ((y - grid.top() + sy) / kh).floor().clamp(0.0, 127.0) as i32) as u8;
    let note_rect = |n: &Note| {
        Rect::from_min_max(pos2(x_of(n.start as f64), y_of(n.key) + 1.5), pos2(x_of((n.start + n.len) as f64) - 2.0, y_of(n.key) + kh - 1.5))
    };

    let snap = ps.snap.max(1);
    let notes_snapshot: Vec<Note> = project.pattern(pat_id).and_then(|p| p.notes.get(&ch)).cloned().unwrap_or_default();
    let mut actions = Vec::new();

    // Change from elsewhere (undo, other channel/pattern, step seq, rec): selection cleared.
    let unchanged = matches!(&ps.seen, Some((p, c, n)) if *p == pat_id && *c == ch && *n == notes_snapshot);
    if !unchanged {
        ps.selected.clear();
        match ps.drag.take() {
            Some(Drag::Keyboard { key, .. } | Drag::Move { key, .. } | Drag::Paint { brush: Note { key, .. }, .. }) => {
                actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false })
            }
            other => ps.drag = other.filter(|d| matches!(d, Drag::Velocity | Drag::Erase)),
        }
    }
    let mut notes = notes_snapshot.clone();
    ps.selected.retain(|&i| i < notes.len());

    // "Bake" the arpeggio.
    let mut baked = false;
    if bake {
        let sel: Vec<usize> = if ps.selected.is_empty() { (0..notes.len()).collect() } else { ps.selected.clone() };
        if !sel.is_empty() {
            let params = project.channel(ch).map(|c| c.arp).unwrap_or_default();
            let added = seq_edit::bake_arp(&mut notes, &sel, &params, pat_len);
            set_selection(&mut ps.selected, added);
            baked = true;
        }
    }

    // --- Edit commands (keyboard, top bar) ---
    match edit {
        Some(EditCmd::SelectAll) => ps.selected = (0..notes.len()).collect(),
        Some(EditCmd::Deselect) => ps.selected.clear(),
        Some(EditCmd::Copy) if !ps.selected.is_empty() => {
            ps.clipboard = seq_edit::copy_notes(&notes, &ps.selected);
        }
        Some(EditCmd::Paste) if !ps.clipboard.is_empty() => {
            // After the selection, otherwise under the mouse, otherwise at the start of the pattern.
            let at = match seq_edit::selection_span(&notes, &ps.selected) {
                Some((_, end)) => end.div_ceil(snap) * snap,
                None => pointer.pos.filter(|p| grid.contains(*p)).map(|p| snap_floor(tick_of(p.x) as Tick, snap)).unwrap_or(0),
            };
            let added = seq_edit::paste_notes(&mut notes, &ps.clipboard, at, far);
            set_selection(&mut ps.selected, added);
        }
        Some(EditCmd::Duplicate) if !ps.selected.is_empty() => {
            let added = seq_edit::duplicate_notes(&mut notes, &ps.selected, snap, far);
            set_selection(&mut ps.selected, added);
        }
        Some(EditCmd::Delete) if !ps.selected.is_empty() => {
            seq_edit::delete_notes(&mut notes, &ps.selected);
            ps.selected.clear();
        }
        Some(EditCmd::Arrow { dx, dy, big }) if !ps.selected.is_empty() => {
            let dk = -dy * if big { 12 } else { 1 };
            seq_edit::shift_notes(&mut notes, &ps.selected, dx as i64 * snap as i64, dk, limit);
        }
        _ => {}
    }

    // --- Two fingers: the gesture started by the first finger is cancelled ---
    if pointer.multi
        && let Some(drag) = ps.drag.take()
    {
        match drag {
            Drag::Move { anchor, key, fresh, .. } => {
                actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
                if fresh && anchor < notes.len() {
                    notes.remove(anchor);
                    ps.selected.clear();
                }
            }
            Drag::Keyboard { key, .. } => actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false }),
            Drag::Paint { brush, before, .. } => {
                actions.push(UiAction::Preview { channel: ch, key: brush.key, vel: 0, on: false });
                notes = before;
                ps.selected.clear();
            }
            _ => {}
        }
    }

    // --- Mouse: press ---
    let tool = ps.tool;
    let hit = |notes: &[Note], pos: Pos2| notes.iter().rposition(|n| note_rect(n).expand2(vec2(0.0, 1.5)).contains(pos));
    let snapshot_of = |notes: &[Note], sel: &[usize]| -> Vec<(usize, Note)> { sel.iter().filter_map(|&i| notes.get(i).map(|n| (i, *n))).collect() };
    if let Some(pos) = pointer.pos.filter(|_| resp.hovered()) {
        if pointer.primary_pressed {
            if kb.contains(pos) {
                let key = key_of(pos.y);
                actions.push(UiAction::Preview { channel: ch, key, vel: 100, on: true });
                ps.drag = Some(Drag::Keyboard { key, y0: pos.y, sy0: ps.scroll_y, scrolling: false });
            } else if vel.contains(pos) {
                ps.drag = Some(Drag::Velocity);
            } else if grid.contains(pos) {
                let t = tick_of(pos.x);
                match (tool, hit(&notes, pos)) {
                    (Tool::Erase, _) => ps.drag = Some(Drag::Erase),
                    // Brush: copies the touched note, otherwise places a note of the last used length.
                    (Tool::Paint, h) if !pointer.shift && !pointer.command => {
                        let before = notes.clone();
                        ps.selected.clear();
                        let brush = match h {
                            Some(idx) => notes[idx],
                            None => {
                                let start = snap_floor(t as Tick, snap);
                                Note { start, len: ps.last_len.max(1), key: key_of(pos.y), velocity: 100 }
                            }
                        };
                        if h.is_none() && brush.start < limit {
                            seq_edit::paint_notes(&mut notes, brush, brush.key, 0, 0, limit);
                        }
                        actions.push(UiAction::Preview { channel: ch, key: brush.key, vel: brush.velocity, on: true });
                        ps.drag = Some(Drag::Paint { brush, last: 0, before });
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
                    (_, Some(idx)) => {
                        if !ps.selected.contains(&idx) {
                            ps.selected = vec![idx];
                        }
                        let n = notes[idx];
                        let orig = snapshot_of(&notes, &ps.selected);
                        if pos.x >= note_rect(&n).right() - EDGE_PX {
                            ps.drag = Some(Drag::Resize { anchor: idx, orig });
                        } else {
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - n.start as i64, key: n.key, orig, fresh: false });
                            actions.push(UiAction::Preview { channel: ch, key: n.key, vel: n.velocity, on: true });
                        }
                    }
                    // Select tool, or Ctrl/Shift + drag: rectangle.
                    (Tool::Select, None) | (Tool::Draw, None) if tool == Tool::Select || pointer.command || pointer.shift => {
                        let base = if pointer.shift { ps.selected.clone() } else { Vec::new() };
                        ps.drag = Some(Drag::Marquee { from: pos, additive: pointer.shift, base });
                    }
                    (_, None) => {
                        ps.selected.clear();
                        if (t as Tick) < limit {
                            let key = key_of(pos.y);
                            let start = snap_floor(t as Tick, snap);
                            let len = ps.last_len.min(limit - start).max(1);
                            notes.push(Note { start, len, key, velocity: 100 });
                            let idx = notes.len() - 1;
                            ps.selected = vec![idx];
                            let orig = snapshot_of(&notes, &ps.selected);
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - start as i64, key, orig, fresh: true });
                            ps.fresh = Some((idx, pointer.time));
                            actions.push(UiAction::Preview { channel: ch, key, vel: 100, on: true });
                        }
                    }
                }
            }
        } else if pointer.secondary_pressed && grid.contains(pos) {
            ps.drag = Some(Drag::Erase);
        }
    }

    // --- Double tap on a note: delete it (quick delete on touch, no tool change) ---
    if pointer.double_clicked
        && tool != Tool::Erase
        && !pointer.shift
        && !pointer.command
        && let Some(pos) = pointer.pos.filter(|p| resp.hovered() && grid.contains(*p))
        && let Some(idx) = hit(&notes, pos)
        && !ps.fresh.is_some_and(|(i, t)| i == idx && pointer.time - t < 1.0)
    {
        // The second tap started a move or a resize of that note: dropped with it.
        if let Some(Drag::Move { key, .. }) = ps.drag.take() {
            actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
        }
        notes.remove(idx);
        ps.selected.clear();
        // The notes were drawn before this: show the deletion now, not at the next input.
        ui.ctx().request_repaint();
    }

    // --- Mouse: drag / release ---
    let mut marquee: Option<Rect> = None;
    if let (Some(drag), Some(pos)) = (ps.drag.clone(), pointer.pos) {
        if pointer.any_down() {
            match drag {
                Drag::Paint { brush, last, before } => {
                    let slot = seq_edit::paint_slot(tick_of(pos.x) as Tick, brush.start, brush.len);
                    if slot != last {
                        // Every cell crossed since the last frame (fast drag).
                        let from = if slot > last { last + 1 } else { last - 1 };
                        seq_edit::paint_notes(&mut notes, brush, key_of(pos.y), from, slot, limit);
                        ps.drag = Some(Drag::Paint { brush, last: slot, before });
                    }
                }
                Drag::Move { anchor, grab, key, orig, fresh } => {
                    if let Some(&(_, a)) = orig.iter().find(|(i, _)| *i == anchor) {
                        let raw = (tick_of(pos.x) as i64 - grab).max(0) as Tick;
                        let new_start = snap_round(raw, snap) as i64;
                        let new_key = key_of(pos.y) as i32;
                        let base: Vec<Note> = orig.iter().map(|(_, n)| *n).collect();
                        let (dt, dk) = seq_edit::clamp_shift(&base, new_start - a.start as i64, new_key - a.key as i32, limit);
                        for &(i, n) in &orig {
                            if let Some(m) = notes.get_mut(i) {
                                m.start = (n.start as i64 + dt) as Tick;
                                m.key = (n.key as i32 + dk) as u8;
                            }
                        }
                        let k = (a.key as i32 + dk) as u8;
                        if k != key {
                            actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
                            actions.push(UiAction::Preview { channel: ch, key: k, vel: a.velocity, on: true });
                            ps.drag = Some(Drag::Move { anchor, grab, key: k, orig, fresh });
                        }
                    }
                }
                Drag::Resize { anchor, orig } => {
                    if let Some(&(_, a)) = orig.iter().find(|(i, _)| *i == anchor) {
                        let min_len = snap.clamp(1, STEP / 4);
                        let end = snap_round(tick_of(pos.x) as Tick, snap).min(limit).max(a.start + min_len);
                        let delta = (end - a.start) as i64 - a.len as i64;
                        for &(i, n) in &orig {
                            if let Some(m) = notes.get_mut(i) {
                                m.len = (n.len as i64 + delta).max(min_len as i64) as Tick;
                            }
                        }
                    }
                }
                Drag::Marquee { from, additive, base } => {
                    let r = Rect::from_two_pos(from, pos);
                    marquee = Some(r);
                    let mut v: Vec<usize> = notes.iter().enumerate().filter(|(_, n)| note_rect(n).intersects(r)).map(|(i, _)| i).collect();
                    if additive {
                        v.extend(base);
                    }
                    set_selection(&mut ps.selected, v);
                }
                Drag::Velocity => {
                    let v = (((vel.bottom() - 4.0 - pos.y) / (vel.height() - 8.0)) * 127.0).clamp(1.0, 127.0) as u8;
                    if ps.selected.is_empty() {
                        for n in notes.iter_mut().filter(|n| (x_of(n.start as f64) + 7.0 - pos.x).abs() <= 8.0) {
                            n.velocity = v;
                        }
                    } else {
                        for &i in &ps.selected {
                            if let Some(n) = notes.get_mut(i) {
                                n.velocity = v;
                            }
                        }
                    }
                }
                Drag::Erase => {
                    if let Some(i) = hit(&notes, pos) {
                        notes.remove(i);
                        ps.selected.clear();
                    }
                }
                Drag::Keyboard { key, y0, sy0, scrolling } => {
                    let dy = pos.y - y0;
                    if scrolling || dy.abs() > KB_SCROLL_SLOP {
                        if !scrolling {
                            actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
                            ps.drag = Some(Drag::Keyboard { key, y0, sy0, scrolling: true });
                        }
                        ps.scroll_y = sy0 - dy;
                    }
                }
            }
        } else {
            match drag {
                Drag::Move { anchor, key, .. } => {
                    actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
                    if let (Some(n), 1) = (notes.get(anchor), ps.selected.len()) {
                        ps.last_len = n.len;
                    }
                }
                Drag::Resize { anchor, .. } => {
                    if let Some(n) = notes.get(anchor) {
                        ps.last_len = n.len;
                    }
                }
                Drag::Keyboard { key, .. } | Drag::Paint { brush: Note { key, .. }, .. } => {
                    actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false })
                }
                _ => {}
            }
            ps.drag = None;
        }
    } else if !pointer.any_down()
        && let Some(Drag::Keyboard { key, .. } | Drag::Move { key, .. } | Drag::Paint { brush: Note { key, .. }, .. }) = ps.drag.take()
    {
        actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
    }

    // Notes past the end grow the pattern to the end of their bar (same undo step).
    let new_len = seq_edit::length_for_notes(base_len, &notes, bar);
    let regrow = ps.grow_base.is_some() && new_len != pat_len;
    if (notes != notes_snapshot || regrow)
        && let Some(p) = project.pattern_mut(pat_id)
    {
        if notes.is_empty() {
            p.notes.remove(&ch);
        } else {
            p.notes.insert(ch, notes.clone());
        }
        p.length = new_len;
    }
    let pat_len = new_len;
    ps.grow_base = ps.drag.is_some().then_some((pat_id, base_len));
    if baked {
        if let Some(c) = project.channel_mut(ch) {
            c.arp.enabled = false;
        }
        st.status = trf!("Arpège gravé : {} notes (ARP en direct du channel désactivé)", "Arpeggio baked: {} notes (channel live ARP turned off)", ps.selected.len());
    }
    ps.seen = Some((pat_id, ch, notes.clone()));
    let selected = ps.selected.clone();
    let pressed_key = match &ps.drag {
        Some(Drag::Keyboard { key, scrolling: false, .. }) => Some(*key),
        _ => None,
    };
    let is_sel = |i: usize| selected.binary_search(&i).is_ok();

    // --- Drawing ---
    let p = ui.painter();
    p.rect_filled(roll, 8.0, GRID_BG);
    let gp = ui.painter_at(grid);
    for key in 0..=127u8 {
        let y = y_of(key);
        if y > grid.bottom() || y + kh < grid.top() {
            continue;
        }
        let row = Rect::from_min_max(pos2(grid.left(), y), pos2(grid.right(), y + kh));
        gp.rect_filled(row, 0.0, if is_black_key(key) { ROW_BLACK } else { ROW_WHITE });
        hline(&gp, y + kh - 0.5, grid.left(), grid.right(), ROW_LINE, 1.0);
    }
    let t0 = tick_of(grid.left()) as Tick;
    let t1 = tick_of(grid.right()) as Tick + 1;
    let line_unit = if (snap as f32 * zoom) >= 6.0 { snap } else if (STEP as f32 * zoom) >= 6.0 { STEP } else { beat };
    let mut t = snap_floor(t0, line_unit);
    while t <= t1 {
        let x = x_of(t as f64);
        if t.is_multiple_of(bar) {
            vline(&gp, x, grid.top(), grid.bottom(), Color32::from_black_alpha(170), 2.5);
        } else if t.is_multiple_of(beat) {
            vline(&gp, x, grid.top(), grid.bottom(), Color32::from_black_alpha(140), 2.0);
        } else {
            vline(&gp, x, grid.top(), grid.bottom(), Color32::from_black_alpha(64), 1.0);
        }
        t += line_unit.max(1);
    }
    // Past the end of the pattern: dimmed, notes can still be placed there.
    let end_x = x_of(pat_len as f64);
    if end_x < grid.right() {
        gp.rect_filled(Rect::from_min_max(pos2(end_x.max(grid.left()), grid.top()), grid.right_bottom()), 0.0, Color32::from_black_alpha(110));
        vline(&gp, end_x, grid.top(), grid.bottom(), with_alpha(ORANGE, 160), 2.0);
    }
    for (i, n) in notes.iter().enumerate() {
        let r = note_rect(n);
        if !r.intersects(grid) {
            continue;
        }
        let k = 0.55 + 0.45 * (n.velocity as f32 / 127.0);
        let (top, bot) = (mix(GRID_BG, note_top, k), mix(GRID_BG, note_bot, k));
        skin::vgradient(&gp, r, 4.0, top, bot);
        gp.line_segment([pos2(r.left() + 3.0, r.top() + 1.0), pos2(r.right() - 3.0, r.top() + 1.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(100)));
        if r.width() > 14.0 {
            vline(&gp, r.right() - 8.0, r.top() + 2.0, r.bottom() - 2.0, with_alpha(note_edge, 110), 1.0);
        }
        let sel = is_sel(i);
        gp.rect_stroke(r, 4.0, if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::new(1.0_f32, note_edge) });
        if r.width() > 34.0 && kh >= 12.0 {
            gp.text(pos2(r.left() + 6.0, r.center().y), Align2::LEFT_CENTER, key_name(n.key), bold(11.0), note_edge);
        }
    }
    if let Some(r) = marquee {
        gp.rect_filled(r, 0.0, with_alpha(ORANGE, 30));
        gp.rect_stroke(r, 0.0, Stroke::new(1.0_f32, ORANGE));
    }
    if info.playing && !st.song_mode {
        let x = x_of(info.playhead);
        if x >= grid.left() {
            gp.rect_filled(Rect::from_center_size(pos2(x, grid.center().y), vec2(6.0, grid.height())), 0.0, with_alpha(LCD_GREEN, 30));
            vline(&gp, x, grid.top(), grid.bottom(), LCD_GREEN, 2.0);
        }
    }

    // Keyboard
    let kp = ui.painter_at(kb);
    kp.rect_filled(kb, 0.0, KEYBOARD_BG);
    for key in 0..=127u8 {
        let y = y_of(key);
        if y > kb.bottom() || y + kh < kb.top() {
            continue;
        }
        let r = Rect::from_min_max(pos2(kb.left(), y), pos2(kb.right(), y + kh));
        let pressed = pressed_key == Some(key);
        if is_black_key(key) {
            kp.rect_filled(r, 0.0, if pressed { ORANGE_LIGHT } else { KEY_WHITE });
            let black = Rect::from_min_max(r.min, pos2(r.left() + r.width() * 0.62, r.bottom()));
            kp.rect_filled(black, 0.0, if pressed { ORANGE } else { KEY_BLACK });
        } else {
            kp.rect_filled(r, 0.0, if pressed { ORANGE_LIGHT } else { KEY_WHITE });
        }
        hline(&kp, r.bottom() - 0.5, r.left(), r.right(), KEY_LINE, 1.0);
        if key % 12 == 0 || key == default_key {
            let label = if key == default_key && key % 12 != 0 { format!("{} ●", key_name(key)) } else { key_name(key) };
            kp.text(pos2(r.right() - 6.0, r.center().y), Align2::RIGHT_CENTER, label, bold(11.0), KEY_LABEL);
        }
    }

    // Velocity lane
    let vlabel = Rect::from_min_max(vel_full.min, pos2(vel.left(), vel_full.bottom()));
    p.rect_filled(vel_full, 8.0, INNER);
    p.rect_stroke(vel_full, 8.0, Stroke::new(1.0_f32, EDGE));
    p.rect_stroke(roll, 8.0, Stroke::new(1.0_f32, EDGE));
    skin::caption(p, vlabel.center(), Align2::CENTER_CENTER, if selected.is_empty() { tr!("VÉLO", "VEL") } else { tr!("VÉLO ·SÉL", "VEL ·SEL") }, TEXT_DIMMER);
    let vp = ui.painter_at(vel);
    let mut t = snap_floor(t0, STEP);
    while t <= t1 {
        vline(&vp, x_of(t as f64), vel.top(), vel.bottom(), Color32::from_white_alpha(10), 1.0);
        t += STEP;
    }
    for (i, n) in notes.iter().enumerate() {
        let x = x_of(n.start as f64) + 2.0;
        let h = (n.velocity as f32 / 127.0) * (vel.height() - 8.0);
        let r = Rect::from_min_max(pos2(x, vel.bottom() - 4.0 - h), pos2(x + 10.0, vel.bottom() - 4.0));
        let c = if is_sel(i) { Color32::WHITE } else { color };
        vp.rect_filled(r, egui::Rounding { nw: 3.0, ne: 3.0, sw: 0.0, se: 0.0 }, c);
        vp.rect_filled(Rect::from_min_size(r.min, vec2(10.0, 3.0)), 2.0, mix(c, Color32::WHITE, 0.6));
    }

    st.actions.extend(actions);
}
