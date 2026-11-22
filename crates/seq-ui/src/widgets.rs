use crate::theme::{pal, screen, with_alpha};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use seq_model::Tick;

/// Pointer info for the frame.
#[derive(Clone, Copy)]
pub struct Pointer {
    pub pos: Option<egui::Pos2>,
    pub primary_pressed: bool,
    pub secondary_pressed: bool,
    pub primary_down: bool,
    pub secondary_down: bool,
    pub double_clicked: bool,
    /// Wheel, or two-finger move (the content follows the fingers).
    pub scroll: Vec2,
    /// Ctrl+wheel, or pinch.
    pub zoom: f32,
    /// Two-finger gesture in progress: no editing, only scroll / zoom.
    pub multi: bool,
    pub shift: bool,
    /// Ctrl (Windows/Linux) or Cmd (macOS).
    pub command: bool,
    /// Input time (seconds).
    pub time: f64,
}

impl Pointer {
    pub fn read(ui: &egui::Ui) -> Self {
        ui.input(|i| {
            let touch = i.multi_touch();
            Pointer {
                pos: i.pointer.interact_pos().or(i.pointer.hover_pos()),
                primary_pressed: i.pointer.primary_pressed() && touch.is_none(),
                secondary_pressed: i.pointer.secondary_pressed(),
                primary_down: i.pointer.primary_down(),
                secondary_down: i.pointer.secondary_down(),
                double_clicked: i.pointer.button_double_clicked(egui::PointerButton::Primary),
                scroll: i.smooth_scroll_delta + touch.map(|t| t.translation_delta).unwrap_or_default(),
                zoom: i.zoom_delta(),
                multi: touch.is_some(),
                shift: i.modifiers.shift,
                command: i.modifiers.command,
                time: i.time,
            }
        })
    }

    pub fn any_down(&self) -> bool {
        self.primary_down || self.secondary_down
    }
}

pub fn vline(p: &egui::Painter, x: f32, y0: f32, y1: f32, color: Color32, w: f32) {
    p.line_segment([pos2(x, y0), pos2(x, y1)], Stroke::new(w, color));
}

pub fn hline(p: &egui::Painter, y: f32, x0: f32, x1: f32, color: Color32, w: f32) {
    p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(w, color));
}

/// How far a horizontally scrolled editor reaches (ticks): `end` plus one screen (`view`
/// ticks) of empty room, rounded up to a bar. Something placed in that room moves the end,
/// and the reach with it, so the editor never runs out.
pub fn reach(end: Tick, view: Tick, bar: Tick) -> Tick {
    let bar = bar.max(1);
    (end + view).div_ceil(bar) * bar
}

/// Largest horizontal scroll (points): the end of `reach` at the right edge of a `view_w`
/// wide view. During a gesture the previous limit (`held`) is kept if it was larger, so
/// dragging something back from the far end does not pull the view under the finger.
pub fn scroll_limit(reach: Tick, zoom: f32, view_w: f32, held: Option<f32>) -> f32 {
    let max = (reach as f32 * zoom - view_w).max(0.0);
    held.map_or(max, |h| max.max(h))
}

/// Scroll bar thumb: (offset from the start of the bar, length) for a `bar` long bar
/// showing a `view` long window on `content`, scrolled by `scroll`. The thumb is at least
/// `min` long, so a finger can still grab it with a lot of content.
pub fn thumb_span(bar: f32, view: f32, content: f32, scroll: f32, min: f32) -> (f32, f32) {
    if content <= view || bar <= 0.0 {
        return (0.0, bar.max(0.0));
    }
    let len = (bar * view / content).clamp(min.min(bar), bar);
    let off = (scroll / (content - view)).clamp(0.0, 1.0) * (bar - len);
    (off, len)
}

/// Inverse of [`thumb_span`]: the scroll that puts the thumb at `offset` (clamped).
pub fn scroll_for_thumb(bar: f32, view: f32, content: f32, offset: f32, min: f32) -> f32 {
    let (_, len) = thumb_span(bar, view, content, 0.0, min);
    let free = bar - len;
    if free <= 0.0 || content <= view { 0.0 } else { (offset / free).clamp(0.0, 1.0) * (content - view) }
}

/// Scroll that brings the item at `top`..`top + len` (in content coordinates) into a
/// `view` long window scrolled by `scroll`, moving as little as possible.
pub fn reveal(scroll: f32, top: f32, len: f32, view: f32) -> f32 {
    if top < scroll {
        top
    } else if top + len > scroll + view {
        (top + len - view).min(top)
    } else {
        scroll
    }
}

/// Overview strip under a horizontally scrolled editor (timeline, piano roll): the whole
/// reach with the content up to `end` tinted, what `marks` draws (given the tick to x
/// mapping), and the part in view. Tap = jump there, drag = scroll (one finger, while the
/// editor above edits). `scroll_x` is in points at `zoom`, at most `max_x`. Returns true
/// when it scrolled.
#[allow(clippy::too_many_arguments)]
pub fn overview_strip(
    ui: &mut Ui,
    rect: Rect,
    id: &str,
    reach: Tick,
    end: Tick,
    scroll_x: &mut f32,
    zoom: f32,
    view_w: f32,
    max_x: f32,
    marks: impl FnOnce(&egui::Painter, &dyn Fn(f32) -> f32),
) -> bool {
    if rect.width() < 20.0 || rect.height() < 4.0 || zoom <= 0.0 {
        return false;
    }
    let p = ui.painter();
    // The limit held during a gesture can go past the reach: the strip covers it too.
    let span = (reach as f32).max((max_x + view_w) / zoom).max(1.0);
    let k = rect.width() / span;
    let x_of = |t: f32| rect.left() + t * k;
    p.rect_filled(rect, 5.0, pal().inner);
    if end > 0 {
        // Display: fainter, so the thumb outline stands out (as in the rack).
        let a = if screen() { 6 } else { 45 };
        p.rect_filled(Rect::from_min_max(rect.min, pos2(x_of(end as f32).min(rect.right()), rect.bottom())), 5.0, with_alpha(pal().orange, a));
    }
    marks(p, &x_of);
    let view = view_w / zoom;
    let thumb = Rect::from_min_max(pos2(x_of(*scroll_x / zoom), rect.top()), pos2(x_of(*scroll_x / zoom + view).min(rect.right()), rect.bottom()));
    p.rect_filled(thumb, 5.0, pal().thumb);
    p.rect_stroke(thumb, 5.0, Stroke::new(1.0_f32, pal().thumb_edge));

    let resp = ui.interact(rect.expand2(vec2(0.0, 4.0)), ui.id().with(id), Sense::click_and_drag());
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let before = *scroll_x;
    if resp.hovered()
        && pressed
        && let Some(pos) = resp.interact_pointer_pos().or(resp.hover_pos()).filter(|p| !thumb.x_range().contains(p.x))
    {
        *scroll_x = ((pos.x - rect.left()) / k - view / 2.0) * zoom;
    }
    if resp.dragged() {
        *scroll_x += resp.drag_delta().x / k * zoom;
    }
    *scroll_x = scroll_x.clamp(0.0, max_x);
    *scroll_x != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reach_is_a_screen_past_the_end() {
        let bar = 3840;
        assert_eq!(reach(4 * bar, 8 * bar, bar), 12 * bar);
        // Rounded up to a bar.
        assert_eq!(reach(4 * bar + 10, 8 * bar, bar), 13 * bar);
        assert_eq!(reach(0, 0, bar), 0);
    }

    #[test]
    fn thumb_follows_the_scroll_and_back() {
        // Everything fits: the thumb fills the bar.
        assert_eq!(thumb_span(100.0, 200.0, 150.0, 0.0, 20.0), (0.0, 100.0));
        assert_eq!(scroll_for_thumb(100.0, 200.0, 150.0, 30.0, 20.0), 0.0);
        // Half the content in view: half the bar, from the top to the bottom.
        assert_eq!(thumb_span(100.0, 200.0, 400.0, 0.0, 20.0), (0.0, 50.0));
        assert_eq!(thumb_span(100.0, 200.0, 400.0, 200.0, 20.0), (50.0, 50.0));
        assert_eq!(thumb_span(100.0, 200.0, 400.0, 100.0, 20.0), (25.0, 50.0));
        // Dragging the thumb gives the scroll back, clamped at both ends.
        assert_eq!(scroll_for_thumb(100.0, 200.0, 400.0, 25.0, 20.0), 100.0);
        assert_eq!(scroll_for_thumb(100.0, 200.0, 400.0, -10.0, 20.0), 0.0);
        assert_eq!(scroll_for_thumb(100.0, 200.0, 400.0, 90.0, 20.0), 200.0);
        // Lots of content: the thumb keeps its minimum length, and still reaches the end.
        let (off, len) = thumb_span(100.0, 100.0, 10_000.0, 9_900.0, 24.0);
        assert_eq!((off, len), (76.0, 24.0));
        assert_eq!(scroll_for_thumb(100.0, 100.0, 10_000.0, 76.0, 24.0), 9_900.0);
    }

    #[test]
    fn reveal_scrolls_just_enough() {
        // Already in view: no change.
        assert_eq!(reveal(100.0, 120.0, 40.0, 200.0), 100.0);
        // Above: its top at the top. Below: its bottom at the bottom.
        assert_eq!(reveal(100.0, 50.0, 40.0, 200.0), 50.0);
        assert_eq!(reveal(100.0, 300.0, 40.0, 200.0), 140.0);
        // Taller than the view: its top.
        assert_eq!(reveal(0.0, 300.0, 400.0, 200.0), 300.0);
    }

    #[test]
    fn scroll_limit_holds_during_a_gesture() {
        assert_eq!(scroll_limit(1000, 1.0, 400.0, None), 600.0);
        assert_eq!(scroll_limit(10, 1.0, 400.0, None), 0.0);
        // The content got shorter under the finger: the limit stays until the gesture ends.
        assert_eq!(scroll_limit(800, 1.0, 400.0, Some(600.0)), 600.0);
        assert_eq!(scroll_limit(1200, 1.0, 400.0, Some(600.0)), 800.0);
    }
}
