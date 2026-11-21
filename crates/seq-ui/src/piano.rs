//! Piano roll: keyboard, grid, notes, tools (draw / select / erase / brush),
//! multi-selection, copy/paste, velocity / pan lane, "Arpeggiate".
//!
//! Active note parameter (velocity, length, pan): chosen in the side panel when the piano
//! roll is full screen, otherwise by tapping the lane label, which cycles through them.
//!
//! Brush: press on a note then drag = end-to-end copies of that note; on empty
//! space = notes of the last used length. Pitch follows the finger.
//!
//! Touch: two fingers scroll and zoom. One finger holding a note still while another one
//! slides up or down changes the active parameter of that note.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::widgets::{Pointer, hline, vline};
use crate::{EditCmd, Editor, PanelId, PlaybackInfo, SeqUi, UiAction};
use crate::icons::Icon;
use eframe::egui::{self, Align2, Color32, Event, Pos2, Rect, Sense, Stroke, TouchId, TouchPhase, Ui, pos2, vec2};
use seq_edit::{NoteParam, snap_floor, snap_round};
use seq_i18n::{tr, trf};
use seq_model::*;

const KB_W: f32 = 72.0;
const VEL_H: f32 = 56.0;
/// Side panel (full screen): width, height of the parameter buttons; the panel only
/// appears when the piano roll is at least `SIDE_MIN_W` wide.
const SIDE_W: f32 = 176.0;
const PARAM_BTN_H: f32 = 60.0;
const SIDE_MIN_W: f32 = 640.0;
const EDGE_PX: f32 = 8.0;
/// Touch: the resize handle reaches this far inside a note (at most 40% of its width)...
const EDGE_TOUCH_IN: f32 = 22.0;
/// ...and this far past its right edge, over empty space.
const EDGE_TOUCH_OUT: f32 = 12.0;

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
    /// Drawing in the lane under the grid (velocity or pan).
    Lane(NoteParam),
    Erase,
    /// Press on the keyboard: plays `key`; beyond a few points of vertical drag,
    /// scrolls the octaves (`y0`, `sy0` = starting position and scroll).
    Keyboard { key: u8, y0: f32, sy0: f32, scrolling: bool },
}

/// Vertical drag (points) beyond which a keyboard press becomes a scroll.
const KB_SCROLL_SLOP: f32 = 8.0;
/// Parameter gesture: the finger on the note must not have moved more than this (points)
/// and must have been down this long (s) when the second finger lands; otherwise the two
/// fingers scroll and zoom (two fingers landing together are a pinch).
const ANCHOR_SLOP: f32 = 16.0;
const ANCHOR_HOLD: f64 = 0.1;
/// Parameter gesture sensitivity: vertical slide (points) per velocity unit, per pan unit
/// (the whole pan range in about 380 points) and per length step.
const VEL_PX: f32 = 3.0;
const PAN_PX: f32 = 3.0;
const LEN_PX: f32 = 24.0;
/// After a manual scroll or zoom, the view stops following the playhead for this long (s).
const FOLLOW_PAUSE: f64 = 2.0;

pub struct PianoState {
    scroll_x: f32,
    scroll_y: f32,
    /// pixels per tick
    zoom: f32,
    key_h: f32,
    snap: Tick,
    pub tool: Tool,
    /// Active note parameter (side panel, lane, two-finger gesture).
    pub(crate) param: NoteParam,
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
    /// Time of the last manual scroll or zoom.
    scrolled_at: f64,
    /// Selection and note count when the current press began: restored if the press
    /// becomes a two-finger gesture (so the selection survives the start of a parameter one).
    press_sel: Option<(Vec<usize>, usize)>,
    /// The current press landed on an existing note: index, position, time. It may become
    /// the anchor of a parameter gesture.
    press_note: Option<(usize, Pos2, f64)>,
    fingers: Fingers,
    multi: Multi,
}

/// What the fingers do once a second one is down, until they are all lifted.
#[derive(Clone, Debug, Default, PartialEq)]
enum Multi {
    /// At most one finger.
    #[default]
    Off,
    /// Scroll and zoom.
    View,
    /// Parameter gesture: `anchor` holds note `note`, `mover` slides up or down. `orig` =
    /// the edited notes before the gesture, taken once the first finger's edit is undone.
    Param { anchor: TouchId, mover: TouchId, note: usize, orig: Option<Vec<(usize, Note)>> },
    /// Parameter gesture over, the note still held: another finger can start a new one.
    Held { anchor: TouchId, note: usize },
}

/// Touches currently down, tracked one by one (egui's `multi_touch` only gives aggregates):
/// id, position at the start of the gesture, current position.
#[derive(Default)]
struct Fingers(Vec<(TouchId, Pos2, Pos2)>);

impl Fingers {
    fn apply(&mut self, id: TouchId, phase: TouchPhase, pos: Pos2) {
        let i = self.0.iter().position(|f| f.0 == id);
        match (phase, i) {
            (TouchPhase::Start, Some(i)) => self.0[i] = (id, pos, pos),
            (TouchPhase::Start, None) => self.0.push((id, pos, pos)),
            (TouchPhase::Move, Some(i)) => self.0[i].2 = pos,
            // A move for a touch whose start was missed (panel hidden): tracked from here.
            (TouchPhase::Move, None) => self.0.push((id, pos, pos)),
            (TouchPhase::End | TouchPhase::Cancel, Some(i)) => {
                self.0.remove(i);
            }
            (TouchPhase::End | TouchPhase::Cancel, None) => {}
        }
    }

    fn count(&self) -> usize {
        self.0.len()
    }

    /// Start and current position of a finger.
    fn get(&self, id: TouchId) -> Option<(Pos2, Pos2)> {
        self.0.iter().find(|f| f.0 == id).map(|f| (f.1, f.2))
    }

    fn has(&self, id: TouchId) -> bool {
        self.0.iter().any(|f| f.0 == id)
    }

    /// The finger that landed last.
    fn newest(&self) -> Option<(TouchId, Pos2, Pos2)> {
        self.0.last().copied()
    }

    /// The finger that started closest to `at`.
    fn started_near(&self, at: Pos2) -> Option<(TouchId, Pos2, Pos2)> {
        self.0.iter().copied().min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))
    }
}

/// Mode when a second finger lands: a parameter gesture if the first finger pressed an
/// existing note (`press`: note, position, time) at least `ANCHOR_HOLD` ago, is still
/// within `ANCHOR_SLOP` of where it landed, and the new finger is in the grid. Otherwise
/// (empty space, finger moved, both landing together): scroll and zoom.
fn start_multi(fingers: &Fingers, press: Option<(usize, Pos2, f64)>, now: f64, grid: Rect) -> Multi {
    let (Some((note, at, t)), Some(mover)) = (press, fingers.newest()) else { return Multi::View };
    let Some((anchor, start, cur)) = fingers.started_near(at) else { return Multi::View };
    let still = start.distance(at) <= ANCHOR_SLOP && cur.distance(at) <= ANCHOR_SLOP;
    if anchor != mover.0 && still && now - t >= ANCHOR_HOLD && grid.contains(mover.1) {
        Multi::Param { anchor, mover: mover.0, note, orig: None }
    } else {
        Multi::View
    }
}

/// Change of `param` for a vertical slide of `dy` points (down > 0, as on screen): up =
/// more. Length changes by whole grid steps (a 1/64 note when the snap is free).
fn gesture_delta(param: NoteParam, dy: f32, snap: Tick) -> i64 {
    let up = -dy;
    match param {
        NoteParam::Velocity => (up / VEL_PX).round() as i64,
        NoteParam::Pan => (up / PAN_PX).round() as i64,
        NoteParam::Length => (up / LEN_PX).round() as i64 * if snap > 1 { snap } else { STEP / 4 } as i64,
    }
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
            param: NoteParam::Velocity,
            last_len: STEP,
            drag: None,
            selected: Vec::new(),
            seen: None,
            clipboard: Vec::new(),
            centered_for: None,
            grow_base: None,
            fresh: None,
            scrolled_at: f64::NEG_INFINITY,
            press_sel: None,
            press_note: None,
            fingers: Fingers::default(),
            multi: Multi::Off,
        }
    }
}

/// Puts back the notes saved at the start of a gesture.
fn restore(notes: &mut [Note], orig: &[(usize, Note)]) {
    for &(i, n) in orig {
        if let Some(m) = notes.get_mut(i) {
            *m = n;
        }
    }
}

impl PianoState {
    /// Snap to grid, in ticks (1 = free).
    pub fn snap(&self) -> Tick {
        self.snap
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
            &[(PPQ as Tick, None, "1/4"), (PPQ as Tick / 2, None, "1/8"),(STEP, None, "1/16"), (PPQ as Tick / 8, None, "1/32"), (PPQ as Tick / 6, None, tr!("Triol.", "Triplet")), (1, None, tr!("Libre", "Free"))],
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

    let mut area = res.body.shrink(10.0);
    // Full screen: side panel for the active parameter, on the right.
    let side = (st.focus == Some(PanelId::Piano) && area.width() >= SIDE_MIN_W).then(|| {
        let r = Rect::from_min_max(pos2(area.right() - SIDE_W, area.top()), area.max);
        area.max.x = r.left() - 8.0;
        r
    });
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
    // Lane label: shows the active parameter, a tap cycles through them.
    let vlabel = Rect::from_min_max(vel_full.min, pos2(vel.left(), vel_full.bottom()));
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

    // --- Individual touches (parameter gesture) ---
    let (touch_events, any_touches) = ui.input(|i| {
        let ev: Vec<(TouchId, TouchPhase, Pos2)> = i
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Touch { id, phase, pos, .. } => Some((*id, *phase, *pos)),
                _ => None,
            })
            .collect();
        (ev, i.any_touches())
    });
    for (id, phase, pos) in touch_events {
        ps.fingers.apply(id, phase, pos);
    }
    if !any_touches {
        ps.fingers = Fingers::default();
        ps.multi = Multi::Off;
        ps.press_note = None;
    }
    if ps.fingers.count() >= 2 {
        ps.multi = match std::mem::take(&mut ps.multi) {
            Multi::Off => start_multi(&ps.fingers, ps.press_note, pointer.time, grid),
            // Note still held: a new finger starts a new parameter gesture.
            Multi::Held { anchor, note } if ps.fingers.has(anchor) => match ps.fingers.newest() {
                Some((mover, start, _)) if mover != anchor && grid.contains(start) => Multi::Param { anchor, mover, note, orig: None },
                _ => Multi::Held { anchor, note },
            },
            m => m,
        };
    }
    let scroll_ok = matches!(ps.multi, Multi::Off | Multi::View);

    // --- Scroll / zoom ---
    if ps.centered_for != Some(ch) {
        ps.centered_for = Some(ch);
        ps.scroll_y = (127.0 - default_key as f32) * ps.key_h - grid.height() / 2.0;
    }
    if (resp.hovered() || pointer.multi) && scroll_ok {
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
        if dx != 0.0 || pointer.zoom != 1.0 {
            ps.scrolled_at = pointer.time;
        }
    }
    // Limited zoom-out: the pattern and the bar after it (at least 4 bars) fill the width.
    let fit = grid.width() / reach.max(4 * bar) as f32;
    ps.zoom = ps.zoom.max(fit);
    ps.scroll_y = ps.scroll_y.clamp(0.0, (128.0 * ps.key_h - grid.height()).max(0.0));
    // Playhead inside the edited pattern (Song mode: only while one of its clips plays).
    let playhead = info.playing.then(|| if st.song_mode { pattern_playhead(&project.arrangement, pat_id, pat_len, info.playhead) } else { Some(info.playhead) }).flatten();
    // Follows the playhead bar by bar, unless the user just scrolled or is editing.
    if let Some(ph) = playhead
        && ps.drag.is_none()
        && !matches!(ps.multi, Multi::Param { .. } | Multi::Held { .. })
        && pointer.time - ps.scrolled_at > FOLLOW_PAUSE
    {
        let x = ph as f32 * ps.zoom - ps.scroll_x;
        if x < 0.0 || x >= grid.width() {
            ps.scroll_x = snap_floor(ph as Tick, bar) as f32 * ps.zoom;
        }
    }
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
        if matches!(ps.multi, Multi::Param { .. } | Multi::Held { .. }) {
            ps.multi = Multi::View;
        }
        match ps.drag.take() {
            Some(Drag::Keyboard { key, .. } | Drag::Move { key, .. } | Drag::Paint { brush: Note { key, .. }, .. }) => {
                actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false })
            }
            other => ps.drag = other.filter(|d| matches!(d, Drag::Lane(_) | Drag::Erase)),
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
            Drag::Move { anchor, key, fresh, orig, .. } => {
                actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false });
                if fresh && anchor < notes.len() {
                    notes.remove(anchor);
                    ps.selected.clear();
                } else {
                    restore(&mut notes, &orig);
                }
            }
            Drag::Resize { orig, .. } => restore(&mut notes, &orig),
            Drag::Keyboard { key, .. } => actions.push(UiAction::Preview { channel: ch, key, vel: 0, on: false }),
            Drag::Paint { brush, before, .. } => {
                actions.push(UiAction::Preview { channel: ch, key: brush.key, vel: 0, on: false });
                notes = before;
                ps.selected.clear();
            }
            _ => {}
        }
        if let Some((sel, n)) = ps.press_sel.take()
            && n == notes.len()
        {
            ps.selected = sel;
        }
    }

    // --- Parameter gesture: one finger holds a note, another slides up or down ---
    if let Multi::Param { note, orig: orig @ None, .. } = &mut ps.multi {
        if *note < notes.len() {
            ps.selected = vec![*note];
            *orig = Some(ps.selected.iter().map(|&i| (i, notes[i])).collect());
        } else {
            ps.multi = Multi::View;
        }
    }
    let mut gesture_at = None;
    let mut ended = None;
    if let Multi::Param { anchor, mover, note, orig: Some(orig) } = &ps.multi {
        match (ps.fingers.has(*anchor), ps.fingers.get(*mover)) {
            (true, Some((start, cur))) => {
                // Only the active parameter changes: pitch and position stay.
                let delta = gesture_delta(ps.param, cur.y - start.y, snap);
                seq_edit::adjust_param(&mut notes, orig, ps.param, delta, seq_edit::min_note_len(snap), limit);
                gesture_at = Some(cur);
            }
            // A finger lifted: the gesture ends, the values stay.
            (held, _) => ended = Some((held.then_some(*anchor), *note)),
        }
    }
    if let Some((anchor, note)) = ended {
        if ps.param == NoteParam::Length
            && let Some(n) = notes.get(note)
        {
            ps.last_len = n.len;
        }
        ps.multi = match anchor {
            Some(anchor) => Multi::Held { anchor, note },
            None => Multi::View,
        };
    }

    // --- Mouse: press ---
    let tool = ps.tool;
    let hit = |notes: &[Note], pos: Pos2| notes.iter().rposition(|n| note_rect(n).expand2(vec2(0.0, 1.5)).contains(pos));
    let snapshot_of = |notes: &[Note], sel: &[usize]| -> Vec<(usize, Note)> { sel.iter().filter_map(|&i| notes.get(i).map(|n| (i, *n))).collect() };
    // Note whose right-edge handle is under `pos` (`h` = note under `pos`). On touch the
    // handle is wider than it looks, and reaches a little past the end of the note.
    let edge_of = |notes: &[Note], h: Option<usize>, pos: Pos2| -> Option<usize> {
        match h {
            Some(i) => {
                let r = note_rect(&notes[i]);
                let inside = if any_touches { (r.width() * 0.4).clamp(EDGE_PX, EDGE_TOUCH_IN) } else { EDGE_PX };
                (pos.x >= r.right() - inside).then_some(i)
            }
            None if any_touches => notes.iter().rposition(|n| {
                let r = note_rect(n);
                (r.top()..=r.bottom()).contains(&pos.y) && pos.x >= r.right() && pos.x <= r.right() + EDGE_TOUCH_OUT
            }),
            None => None,
        }
    };
    if let Some(pos) = pointer.pos.filter(|_| resp.hovered() && ps.multi == Multi::Off) {
        if pointer.primary_pressed {
            ps.press_sel = Some((ps.selected.clone(), notes.len()));
            ps.press_note = None;
            if kb.contains(pos) {
                let key = key_of(pos.y);
                actions.push(UiAction::Preview { channel: ch, key, vel: 100, on: true });
                ps.drag = Some(Drag::Keyboard { key, y0: pos.y, sy0: ps.scroll_y, scrolling: false });
            } else if vlabel.contains(pos) {
                ps.param = ps.param.next();
            } else if vel.contains(pos) {
                ps.drag = Some(Drag::Lane(lane_param(ps.param)));
            } else if grid.contains(pos) {
                let t = tick_of(pos.x);
                let h = hit(&notes, pos);
                let edge = edge_of(&notes, h, pos);
                if tool != Tool::Erase {
                    ps.press_note = h.map(|i| (i, pos, pointer.time));
                }
                match (tool, h) {
                    (Tool::Erase, _) => ps.drag = Some(Drag::Erase),
                    // Brush: copies the touched note, otherwise places a note of the last used length.
                    (Tool::Paint, h) if !pointer.shift && !pointer.command => {
                        let before = notes.clone();
                        ps.selected.clear();
                        let brush = match h {
                            Some(idx) => notes[idx],
                            None => {
                                let start = snap_floor(t as Tick, snap);
                                Note { start, len: ps.last_len.max(1), key: key_of(pos.y), velocity: 100, pan: 0 }
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
                        if edge == Some(idx) {
                            ps.drag = Some(Drag::Resize { anchor: idx, orig });
                        } else {
                            ps.drag = Some(Drag::Move { anchor: idx, grab: t as i64 - n.start as i64, key: n.key, orig, fresh: false });
                            actions.push(UiAction::Preview { channel: ch, key: n.key, vel: n.velocity, on: true });
                        }
                    }
                    // Touch, just past the end of a note: its resize handle.
                    (_, None) if !pointer.shift && !pointer.command && edge.is_some() => {
                        let idx = edge.unwrap_or_default();
                        if !ps.selected.contains(&idx) {
                            ps.selected = vec![idx];
                        }
                        let orig = snapshot_of(&notes, &ps.selected);
                        ps.drag = Some(Drag::Resize { anchor: idx, orig });
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
                            notes.push(Note { start, len, key, velocity: 100, pan: 0 });
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
        && ps.multi == Multi::Off
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
                Drag::Lane(param) => {
                    let v = (((vel.bottom() - 4.0 - pos.y) / (vel.height() - 8.0)) * 127.0).clamp(1.0, 127.0) as u8;
                    let pan = lane_pan(pos.y, vel.center().y, (vel.height() - 8.0) / 2.0);
                    let set = |n: &mut Note| match param {
                        NoteParam::Pan => n.pan = pan,
                        _ => n.velocity = v,
                    };
                    if ps.selected.is_empty() {
                        notes.iter_mut().filter(|n| (x_of(n.start as f64) + 7.0 - pos.x).abs() <= 8.0).for_each(set);
                    } else {
                        for &i in &ps.selected {
                            if let Some(n) = notes.get_mut(i) {
                                set(n);
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
    ps.grow_base = (ps.drag.is_some() || matches!(ps.multi, Multi::Param { .. })).then_some((pat_id, base_len));
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
    // Playhead, above the notes: a dark outline keeps it visible on any note color.
    if let Some(ph) = playhead {
        let x = x_of(ph);
        if x >= grid.left() && x <= grid.right() {
            gp.rect_filled(Rect::from_center_size(pos2(x, grid.center().y), vec2(10.0, grid.height())), 0.0, with_alpha(LCD_GREEN, 40));
            vline(&gp, x, grid.top(), grid.bottom(), Color32::from_black_alpha(160), 5.0);
            vline(&gp, x, grid.top(), grid.bottom(), LCD_GREEN, 2.5);
            let top = grid.top();
            gp.add(egui::Shape::convex_polygon(vec![pos2(x - 6.0, top), pos2(x + 6.0, top), pos2(x, top + 8.0)], LCD_GREEN, Stroke::new(1.0_f32, Color32::from_black_alpha(160))));
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

    // Lane: velocity, or pan when it is the active parameter.
    p.rect_filled(vel_full, 8.0, INNER);
    p.rect_stroke(vel_full, 8.0, Stroke::new(1.0_f32, EDGE));
    p.rect_stroke(roll, 8.0, Stroke::new(1.0_f32, EDGE));
    let param = ps.param;
    lane_label(p, vlabel, param, !selected.is_empty());
    let vp = ui.painter_at(vel);
    let mut t = snap_floor(t0, STEP);
    while t <= t1 {
        vline(&vp, x_of(t as f64), vel.top(), vel.bottom(), Color32::from_white_alpha(10), 1.0);
        t += STEP;
    }
    let pan_lane = lane_param(param) == NoteParam::Pan;
    let mid = vel.center().y;
    let half = (vel.height() - 8.0) / 2.0;
    if pan_lane {
        hline(&vp, mid, vel.left(), vel.right(), Color32::from_white_alpha(40), 1.0);
    }
    for (i, n) in notes.iter().enumerate() {
        let x = x_of(n.start as f64) + 2.0;
        let c = if is_sel(i) { Color32::WHITE } else { color };
        if pan_lane {
            // From the centre line: up = right, down = left.
            let y = mid - pan_unit(n.pan) * half;
            vp.rect_filled(Rect::from_two_pos(pos2(x, mid), pos2(x + 10.0, y)), 2.0, c);
            vp.rect_filled(Rect::from_center_size(pos2(x + 5.0, y), vec2(10.0, 3.0)), 1.5, mix(c, Color32::WHITE, 0.6));
        } else {
            let h = (n.velocity as f32 / 127.0) * (vel.height() - 8.0);
            let r = Rect::from_min_max(pos2(x, vel.bottom() - 4.0 - h), pos2(x + 10.0, vel.bottom() - 4.0));
            vp.rect_filled(r, egui::Rounding { nw: 3.0, ne: 3.0, sw: 0.0, se: 0.0 }, c);
            vp.rect_filled(Rect::from_min_size(r.min, vec2(10.0, 3.0)), 2.0, mix(c, Color32::WHITE, 0.6));
        }
    }

    // Parameter gesture: the value, next to the moving finger.
    if let Some(at) = gesture_at {
        let value = seq_edit::param_range(&notes, &selected, param).map(|r| range_text(param, r)).unwrap_or_default();
        let fp = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.id().with("param_label")));
        let g = fp.layout_no_wrap(format!("{}  {}", param_short(param), value), bold(22.0), TEXT);
        let size = g.size() + vec2(28.0, 16.0);
        // Above the finger, kept on screen.
        let screen = ui.ctx().screen_rect();
        let c = pos2(
            at.x.clamp(screen.left() + size.x / 2.0, (screen.right() - size.x / 2.0).max(screen.left() + size.x / 2.0)),
            (at.y - 90.0).max(screen.top() + size.y / 2.0),
        );
        let r = Rect::from_center_size(c, size);
        fp.rect_filled(r, 10.0, POPUP);
        fp.rect_stroke(r, 10.0, Stroke::new(2.0_f32, ORANGE));
        fp.galley(r.center() - g.size() / 2.0, g, TEXT);
    }

    if let Some(r) = side {
        side_panel(ui, r, &mut ps.param, &notes, &selected);
    }

    st.actions.extend(actions);
}

/// Full-screen side panel: picks the active parameter, shows its value for the selected
/// notes (or their range) and how many are selected. Never changes the selection.
fn side_panel(ui: &Ui, rect: Rect, param: &mut NoteParam, notes: &[Note], selected: &[usize]) {
    let p = ui.painter();
    p.rect_filled(rect, 8.0, INNER);
    p.rect_stroke(rect, 8.0, Stroke::new(1.0_f32, EDGE));
    let inner = rect.shrink(10.0);
    skin::caption(p, pos2(inner.left() + 2.0, inner.top() + 8.0), Align2::LEFT_CENTER, tr!("PARAMÈTRE", "PARAMETER"), TEXT_DIMMER);
    let mut y = inner.top() + 22.0;
    for (i, &np) in NoteParam::ALL.iter().enumerate() {
        let r = Rect::from_min_size(pos2(inner.left(), y), vec2(inner.width(), PARAM_BTN_H));
        let resp = ui.interact(r, ui.id().with(("note_param", i)), Sense::click());
        if resp.clicked() {
            *param = np;
        }
        let fg = skin::paint_button(p, r, 8.0, Kind::Tile, *param == np, resp.is_pointer_button_down_on(), resp.hovered(), true);
        p.text(r.center(), Align2::CENTER_CENTER, param_name(np), bold(17.0), fg);
        y += PARAM_BTN_H + 8.0;
    }
    // Value of the selected notes, updated live.
    let lcd_r = Rect::from_min_size(pos2(inner.left(), y + 6.0), vec2(inner.width(), 66.0));
    skin::lcd_bg(p, lcd_r, 9.0);
    skin::caption(p, pos2(lcd_r.center().x, lcd_r.top() + 13.0), Align2::CENTER_CENTER, param_short(*param), LCD_LABEL);
    let value = seq_edit::param_range(notes, selected, *param).map(|r| range_text(*param, r)).unwrap_or_else(|| "—".into());
    let size = if value.chars().count() > 7 { 17.0 } else { 24.0 };
    skin::lcd_text(p, pos2(lcd_r.center().x, lcd_r.center().y + 8.0), Align2::CENTER_CENTER, &value, lcd(size), LCD_GREEN);
    let count = match selected.len() {
        0 => tr!("Aucune note sélectionnée", "No note selected").to_string(),
        1 => tr!("1 note sélectionnée", "1 note selected").to_string(),
        n => trf!("{} notes sélectionnées", "{} notes selected", n),
    };
    let g = p.layout(count, body(13.5), TEXT_DIM, inner.width());
    let bottom = lcd_r.bottom() + 10.0 + g.size().y;
    p.galley(pos2(inner.left() + 2.0, lcd_r.bottom() + 10.0), g, TEXT_DIM);
    let hint = tr!("Tenez une note et glissez un autre doigt vers le haut ou le bas.", "Hold a note and slide another finger up or down.");
    let g = p.layout(hint.to_string(), body(12.5), TEXT_DIMMER, inner.width());
    let y = inner.bottom() - g.size().y;
    if y > bottom + 8.0 {
        p.galley(pos2(inner.left() + 2.0, y), g, TEXT_DIMMER);
    }
}

/// Lane label: the active parameter (name and position in the cycle), tappable.
fn lane_label(p: &egui::Painter, r: Rect, param: NoteParam, selection: bool) {
    let r = r.shrink(4.0);
    skin::paint_button(p, r, 6.0, Kind::Dark, false, false, false, true);
    let c = r.center();
    skin::caption(p, c - vec2(0.0, 12.0), Align2::CENTER_CENTER, param_short(param), TEXT);
    let n = NoteParam::ALL.len() as f32;
    for (i, &np) in NoteParam::ALL.iter().enumerate() {
        let x = c.x + (i as f32 - (n - 1.0) / 2.0) * 10.0;
        p.circle_filled(pos2(x, c.y + 4.0), 3.0, if np == param { ORANGE } else { SEG_ON });
    }
    if selection {
        skin::caption(p, c + vec2(0.0, 17.0), Align2::CENTER_CENTER, tr!("SÉL", "SEL"), TEXT_DIMMER);
    }
}

/// What the lane shows and edits: pan when it is active, velocity otherwise.
fn lane_param(param: NoteParam) -> NoteParam {
    if param == NoteParam::Pan { NoteParam::Pan } else { NoteParam::Velocity }
}

/// Pan drawn at height `y` in the lane (centre line `mid`, `half` points to each end):
/// up = right. Within a few points of the centre line, snaps to the centre.
fn lane_pan(y: f32, mid: f32, half: f32) -> i8 {
    if (mid - y).abs() <= 3.0 {
        return 0;
    }
    let u = ((mid - y) / half.max(1.0)).clamp(-1.0, 1.0);
    if u < 0.0 { u * -(PAN_MIN as f32) } else { u * PAN_MAX as f32 }.round() as i8
}

fn param_name(p: NoteParam) -> &'static str {
    match p {
        NoteParam::Velocity => tr!("Vélocité", "Velocity"),
        NoteParam::Length => tr!("Durée", "Length"),
        NoteParam::Pan => tr!("Pan", "Pan"),
    }
}

fn param_short(p: NoteParam) -> &'static str {
    match p {
        NoteParam::Velocity => tr!("VÉLO", "VEL"),
        NoteParam::Length => tr!("DURÉE", "LEN"),
        NoteParam::Pan => tr!("PAN", "PAN"),
    }
}

/// A value of `p` as shown to the user.
fn value_text(p: NoteParam, v: i64) -> String {
    match p {
        NoteParam::Velocity => v.to_string(),
        NoteParam::Length => len_text(v.max(0) as Tick),
        NoteParam::Pan if v < 0 => trf!("G{}", "L{}", -v),
        NoteParam::Pan if v > 0 => trf!("D{}", "R{}", v),
        NoteParam::Pan => tr!("C", "C").to_string(),
    }
}

/// A value, or a range "min–max" when the notes differ.
fn range_text(p: NoteParam, (lo, hi): (i64, i64)) -> String {
    if lo == hi { value_text(p, lo) } else { format!("{}–{}", value_text(p, lo), value_text(p, hi)) }
}

/// Note length as a fraction of a whole note ("3/16"), in ticks when none fits.
fn len_text(len: Tick) -> String {
    let whole = 4 * PPQ as Tick;
    [1, 2, 4, 8, 16, 32, 64, 3, 6, 12, 24, 48, 96]
        .into_iter()
        .find(|&d| (len * d).is_multiple_of(whole))
        .map(|d| format!("{}/{}", len * d / whole, d))
        .unwrap_or_else(|| trf!("{} ticks", "{} ticks", len))
}

/// Playhead position inside `pattern` (length `pat_len`) for the song position `pos`:
/// the clip of that pattern under the playhead, on any track, gives the offset, and the
/// pattern loops when the clip is longer than it (as in the engine). If several clips of
/// the pattern overlap, the one that started last wins. None when none is playing.
pub(crate) fn pattern_playhead(arr: &Arrangement, pattern: PatternId, pat_len: Tick, pos: f64) -> Option<f64> {
    let plen = pat_len.max(1) as f64;
    arr.audible_clips()
        .filter(|c| c.pattern == pattern && c.len > 0 && pos >= c.start as f64 && pos < (c.start + c.len) as f64)
        .max_by_key(|c| c.start)
        .map(|c| (pos - c.start as f64) % plen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(pattern: u32, track: u16, start: Tick, len: Tick) -> Clip {
        Clip { pattern: PatternId(pattern), track, start, len }
    }

    #[test]
    fn playhead_inside_clip() {
        let arr = Arrangement { tracks: 4, clips: vec![clip(1, 0, 1000, 500), clip(2, 2, 0, 4000)], muted: Vec::new() };
        assert_eq!(pattern_playhead(&arr, PatternId(1), 500, 1200.0), Some(200.0));
        assert_eq!(pattern_playhead(&arr, PatternId(2), 4000, 1200.0), Some(1200.0));
    }

    #[test]
    fn playhead_none_outside_clips() {
        let arr = Arrangement { tracks: 4, clips: vec![clip(1, 0, 1000, 500), clip(1, 1, 3000, 0)], muted: Vec::new() };
        assert_eq!(pattern_playhead(&arr, PatternId(1), 500, 999.0), None);
        // The clip end is exclusive.
        assert_eq!(pattern_playhead(&arr, PatternId(1), 500, 1500.0), None);
        // Empty clips are ignored, like in the engine.
        assert_eq!(pattern_playhead(&arr, PatternId(1), 500, 3000.0), None);
        // Another pattern.
        assert_eq!(pattern_playhead(&arr, PatternId(3), 500, 1200.0), None);
    }

    #[test]
    fn playhead_loops_in_long_clip() {
        let arr = Arrangement { tracks: 1, clips: vec![clip(1, 0, 1000, 2500)], muted: Vec::new() };
        assert_eq!(pattern_playhead(&arr, PatternId(1), 1000, 1000.0), Some(0.0));
        assert_eq!(pattern_playhead(&arr, PatternId(1), 1000, 2250.0), Some(250.0));
        assert_eq!(pattern_playhead(&arr, PatternId(1), 1000, 3400.0), Some(400.0));
        // A clip shorter than the pattern truncates it.
        let arr = Arrangement { tracks: 1, clips: vec![clip(1, 0, 0, 300)], muted: Vec::new() };
        assert_eq!(pattern_playhead(&arr, PatternId(1), 1000, 299.0), Some(299.0));
        assert_eq!(pattern_playhead(&arr, PatternId(1), 1000, 300.0), None);
    }

    #[test]
    fn playhead_latest_overlapping_clip_wins() {
        let mut arr = Arrangement { tracks: 2, clips: vec![clip(1, 1, 500, 2000), clip(1, 0, 0, 2000)], muted: Vec::new() };
        assert_eq!(pattern_playhead(&arr, PatternId(1), 4000, 700.0), Some(200.0));
        assert_eq!(pattern_playhead(&arr, PatternId(1), 4000, 300.0), Some(300.0));
        // A muted track doesn't play, so the other clip gives the position.
        arr.set_muted(1, true);
        assert_eq!(pattern_playhead(&arr, PatternId(1), 4000, 700.0), Some(700.0));
    }
}

#[cfg(test)]
mod param_tests {
    use super::*;

    #[test]
    fn lengths_read_as_fractions() {
        assert_eq!(len_text(STEP), "1/16");
        assert_eq!(len_text(3 * STEP), "3/16");
        assert_eq!(len_text(PPQ as Tick), "1/4");
        assert_eq!(len_text(4 * PPQ as Tick), "1/1");
        assert_eq!(len_text(6 * PPQ as Tick), "3/2");
        assert_eq!(len_text(STEP / 4), "1/64");
        assert_eq!(len_text(PPQ as Tick / 6), "1/24");
        assert!(len_text(7).contains('7'));
    }

    #[test]
    fn values_and_ranges() {
        seq_i18n::with_lang(seq_i18n::Lang::En, || {
            assert_eq!(range_text(NoteParam::Velocity, (90, 90)), "90");
            assert_eq!(range_text(NoteParam::Velocity, (40, 100)), "40–100");
            assert_eq!(range_text(NoteParam::Pan, (-64, 63)), "L64–R63");
            assert_eq!(range_text(NoteParam::Pan, (0, 0)), "C");
            assert_eq!(range_text(NoteParam::Length, (STEP as i64, 2 * STEP as i64)), "1/16–1/8");
        });
    }

    #[test]
    fn lane_pan_from_height() {
        assert_eq!(lane_pan(50.0, 50.0, 20.0), 0);
        assert_eq!(lane_pan(51.0, 50.0, 20.0), 0);
        assert_eq!(lane_pan(30.0, 50.0, 20.0), PAN_MAX);
        assert_eq!(lane_pan(0.0, 50.0, 20.0), PAN_MAX);
        assert_eq!(lane_pan(70.0, 50.0, 20.0), PAN_MIN);
        assert_eq!(lane_pan(60.0, 50.0, 20.0), -32);
        assert_eq!(lane_param(NoteParam::Length), NoteParam::Velocity);
        assert_eq!(lane_param(NoteParam::Pan), NoteParam::Pan);
    }
}

#[cfg(test)]
mod gesture_tests {
    use super::*;

    #[test]
    fn fingers_tracked_one_by_one() {
        let mut f = Fingers::default();
        f.apply(TouchId(1), TouchPhase::Start, pos2(10.0, 10.0));
        f.apply(TouchId(2), TouchPhase::Start, pos2(30.0, 10.0));
        f.apply(TouchId(3), TouchPhase::Start, pos2(300.0, 10.0));
        assert_eq!(f.count(), 3);
        f.apply(TouchId(1), TouchPhase::Move, pos2(12.0, 11.0));
        f.apply(TouchId(3), TouchPhase::Move, pos2(260.0, 30.0));
        assert_eq!(f.get(TouchId(3)), Some((pos2(300.0, 10.0), pos2(260.0, 30.0))));
        assert_eq!(f.newest().map(|n| n.0), Some(TouchId(3)));
        assert_eq!(f.started_near(pos2(28.0, 12.0)).map(|n| n.0), Some(TouchId(2)));
        f.apply(TouchId(2), TouchPhase::End, pos2(30.0, 10.0));
        f.apply(TouchId(9), TouchPhase::Cancel, pos2(0.0, 0.0));
        assert_eq!(f.count(), 2);
        assert!(f.has(TouchId(1)) && !f.has(TouchId(2)));
        // A move whose start was missed: tracked from there.
        f.apply(TouchId(4), TouchPhase::Move, pos2(50.0, 50.0));
        assert_eq!(f.get(TouchId(4)), Some((pos2(50.0, 50.0), pos2(50.0, 50.0))));
    }

    const GRID: Rect = Rect { min: Pos2 { x: 100.0, y: 0.0 }, max: Pos2 { x: 1000.0, y: 600.0 } };

    /// First finger pressed note 3 at (200, 100) at t = 1 s and is now at `anchor_now`;
    /// the second lands at `second`.
    fn second_lands(anchor_now: Pos2, second: Pos2, now: f64, on_note: bool) -> Multi {
        let mut f = Fingers::default();
        f.apply(TouchId(1), TouchPhase::Start, pos2(200.0, 100.0));
        f.apply(TouchId(1), TouchPhase::Move, anchor_now);
        f.apply(TouchId(2), TouchPhase::Start, second);
        start_multi(&f, on_note.then_some((3, pos2(200.0, 100.0), 1.0)), now, GRID)
    }

    #[test]
    fn holding_a_note_still_starts_a_parameter_gesture() {
        let param = Multi::Param { anchor: TouchId(1), mover: TouchId(2), note: 3, orig: None };
        assert_eq!(second_lands(pos2(200.0, 100.0), pos2(600.0, 300.0), 1.5, true), param);
        // A little jitter is fine.
        assert_eq!(second_lands(pos2(208.0, 106.0), pos2(600.0, 300.0), 1.5, true), param);
    }

    #[test]
    fn otherwise_two_fingers_scroll_and_zoom() {
        // First finger on empty space.
        assert_eq!(second_lands(pos2(200.0, 100.0), pos2(600.0, 300.0), 1.5, false), Multi::View);
        // First finger moved (it was dragging the note).
        assert_eq!(second_lands(pos2(240.0, 100.0), pos2(600.0, 300.0), 1.5, true), Multi::View);
        // Both fingers landing together: a pinch.
        assert_eq!(second_lands(pos2(200.0, 100.0), pos2(600.0, 300.0), 1.05, true), Multi::View);
        // Second finger outside the grid (keyboard, lane, side panel).
        assert_eq!(second_lands(pos2(200.0, 100.0), pos2(50.0, 300.0), 1.5, true), Multi::View);
        assert_eq!(second_lands(pos2(200.0, 100.0), pos2(600.0, 700.0), 1.5, true), Multi::View);
    }

    #[test]
    fn slide_up_is_more() {
        // Velocity and pan: one unit per 3 points, up = more.
        assert_eq!(gesture_delta(NoteParam::Velocity, 0.0, STEP), 0);
        assert_eq!(gesture_delta(NoteParam::Velocity, -30.0, STEP), 10);
        assert_eq!(gesture_delta(NoteParam::Velocity, 30.0, STEP), -10);
        assert_eq!(gesture_delta(NoteParam::Pan, -381.0, STEP), 127);
        // Length: whole grid steps, one per 24 points.
        assert_eq!(gesture_delta(NoteParam::Length, -11.0, STEP), 0);
        assert_eq!(gesture_delta(NoteParam::Length, -13.0, STEP), STEP as i64);
        assert_eq!(gesture_delta(NoteParam::Length, -50.0, PPQ as Tick / 2), PPQ as i64);
        assert_eq!(gesture_delta(NoteParam::Length, 24.0, STEP), -(STEP as i64));
        // Free snap: 1/64 notes.
        assert_eq!(gesture_delta(NoteParam::Length, -48.0, 1), STEP as i64 / 2);
    }
}
