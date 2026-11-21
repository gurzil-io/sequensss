//! "Retro hardware" components: domed cream buttons, LCD, knobs, switches,
//! segmented selectors, dropdown menus, panels with a wooden header.

use crate::icons::{self, Icon};
use crate::theme::*;
use eframe::egui::{
    self, Align, Align2, Color32, FontId, Layout, Mesh, Painter, Pos2, Rect, Response, Rounding, Sense, Shape, Stroke, Ui,
    UiBuilder, Vec2, pos2, vec2,
};

// ---------------------------------------------------------------------------
// Drawing primitives
// ---------------------------------------------------------------------------

/// Outline of a rounded rectangle (convex polygon).
fn rrect_points(rect: Rect, r: f32) -> Vec<Pos2> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    let mut pts = Vec::with_capacity(28);
    let corners = [
        (pos2(rect.right() - r, rect.top() + r), -90.0f32),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 90.0),
        (pos2(rect.left() + r, rect.top() + r), 180.0),
    ];
    for (c, a0) in corners {
        for i in 0..=6 {
            let a = (a0 + 90.0 * i as f32 / 6.0).to_radians();
            pts.push(pos2(c.x + r * a.cos(), c.y + r * a.sin()));
        }
    }
    pts
}

/// Rounded rectangle filled with a vertical gradient.
pub fn vgradient(p: &Painter, rect: Rect, r: f32, top: Color32, bot: Color32) {
    if top == bot {
        p.rect_filled(rect, r, top);
        return;
    }
    let pts = rrect_points(rect, r);
    let col = |y: f32| mix(top, bot, ((y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0));
    let mut m = Mesh::default();
    m.colored_vertex(rect.center(), col(rect.center().y));
    for q in &pts {
        m.colored_vertex(*q, col(q.y));
    }
    let n = pts.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

/// Rectangle (not rounded) with a multi-stop horizontal gradient.
pub fn hgradient(p: &Painter, rect: Rect, stops: &[(f32, Color32)]) {
    let mut m = Mesh::default();
    for (i, w) in stops.windows(2).enumerate() {
        let (x0, c0) = (rect.left() + w[0].0 * rect.width(), w[0].1);
        let (x1, c1) = (rect.left() + w[1].0 * rect.width(), w[1].1);
        let b = (i * 4) as u32;
        m.colored_vertex(pos2(x0, rect.top()), c0);
        m.colored_vertex(pos2(x1, rect.top()), c1);
        m.colored_vertex(pos2(x1, rect.bottom()), c1);
        m.colored_vertex(pos2(x0, rect.bottom()), c0);
        m.add_triangle(b, b + 1, b + 2);
        m.add_triangle(b, b + 2, b + 3);
    }
    p.add(Shape::mesh(m));
}

/// "Domed" volume: gradient + border + highlight at the top.
pub fn bevel(p: &Painter, rect: Rect, r: f32, top: Color32, bot: Color32, edge: Color32, hl: Option<Color32>) {
    vgradient(p, rect, r, top, bot);
    if let Some(hl) = hl {
        p.line_segment([pos2(rect.left() + r * 0.7, rect.top() + 1.5), pos2(rect.right() - r * 0.7, rect.top() + 1.5)], Stroke::new(1.0_f32, hl));
    }
    p.rect_stroke(rect, r, Stroke::new(1.0_f32, edge));
}

/// Luminous halo (glow) around a shape.
pub fn glow(p: &Painter, rect: Rect, r: f32, color: Color32, spread: f32) {
    for i in (1..=4).rev() {
        let e = spread * i as f32 / 4.0;
        p.rect_filled(rect.expand(e), r + e, with_alpha(color, (28 / i) as u8 + 4));
    }
}

/// Round indicator light.
pub fn led(p: &Painter, center: Pos2, r: f32, color: Color32, lit: bool) {
    if lit {
        for i in (1..=3).rev() {
            p.circle_filled(center, r + i as f32 * 1.6, with_alpha(color, 22));
        }
    }
    p.circle_filled(center, r, color);
}

/// Small rounded-square indicator light (panel headers).
pub fn led_square(p: &Painter, center: Pos2, size: f32, color: Color32) {
    let r = Rect::from_center_size(center, vec2(size, size));
    glow(p, r, 3.0, color, 6.0);
    p.rect_filled(r, 3.0, color);
}

/// LCD screen background (recessed).
pub fn lcd_bg(p: &Painter, rect: Rect, r: f32) {
    p.rect_filled(rect, r, pal().lcd_bg);
    if screen() {
        p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal().edge));
        return;
    }
    for i in 0..3 {
        let y = rect.top() + 1.0 + i as f32;
        p.line_segment([pos2(rect.left() + r * 0.5, y), pos2(rect.right() - r * 0.5, y)], Stroke::new(1.0_f32, Color32::from_black_alpha(120 - i * 35)));
    }
    p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal().lcd_edge));
}

/// Text that glows in the Display theme (plain text in Retro).
pub fn gtext(p: &Painter, pos: Pos2, align: Align2, text: impl ToString, font: FontId, color: Color32) -> Rect {
    let text = text.to_string();
    // Small text keeps a crisp edge: the halo would smear its pixels.
    if screen() && font.size >= 24.0 {
        let g = with_alpha(color, (color.a() as f32 * 0.12) as u8);
        for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
            p.text(pos + d, align, &text, font.clone(), g);
        }
    }
    p.text(pos, align, text, font, color)
}

/// LCD text with a glowing afterimage.
pub fn lcd_text(p: &Painter, pos: Pos2, align: Align2, text: &str, font: FontId, color: Color32) {
    let g = with_alpha(color, 50);
    for d in [vec2(-1.0, 0.0), vec2(1.0, 0.0), vec2(0.0, -1.0), vec2(0.0, 1.0)] {
        p.text(pos + d, align, text, font.clone(), g);
    }
    p.text(pos, align, text, font, color);
}

/// Small section label (UPPERCASE).
pub fn caption(p: &Painter, pos: Pos2, align: Align2, text: &str, color: Color32) {
    gtext(&p, pos, align, text, medium(11.5), color);
}

/// Dashed outline.
pub fn dashed_rect(p: &Painter, rect: Rect, color: Color32) {
    let s = Stroke::new(1.0_f32, color);
    let c = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
    for w in c.windows(2) {
        p.extend(Shape::dashed_line(&[w[0], w[1]], s, 5.0, 3.0));
    }
}

/// Vertical level bar (VU meter).
pub fn meter_bar(p: &Painter, rect: Rect, level: f32) {
    p.rect_filled(rect, 2.0, pal().meter_bg);
    let db = 20.0 * level.max(1e-6).log10();
    let frac = ((db + 48.0) / 48.0).clamp(0.0, 1.0);
    if frac <= 0.0 {
        return;
    }
    let h = rect.height() * frac;
    let green_h = rect.height() * 0.62;
    let filled = Rect::from_min_max(pos2(rect.left(), rect.bottom() - h), rect.right_bottom());
    let g = filled.intersect(Rect::from_min_max(pos2(rect.left(), rect.bottom() - green_h), rect.right_bottom()));
    p.rect_filled(g, 2.0, pal().meter_green);
    if h > green_h {
        let y = Rect::from_min_max(filled.left_top(), pos2(rect.right(), rect.bottom() - green_h));
        p.rect_filled(y, 2.0, if level >= 0.99 { pal().red } else { pal().meter_yellow });
    }
}

// ---------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Domed cream (default).
    Cream,
    /// Glowing orange (primary action).
    Orange,
    /// Dark tile.
    Dark,
    /// Small gray button (mute, solo, choke…); orange when selected.
    Tile,
    /// Dashed outline (add).
    Ghost,
}

/// Draws a button's background and returns the content color.
pub fn paint_button(p: &Painter, rect: Rect, r: f32, kind: Kind, selected: bool, pressed: bool, hovered: bool, enabled: bool) -> Color32 {
    if screen() {
        return paint_screen_button(p, rect, r, kind, selected, pressed, hovered, enabled);
    }
    let lift = |c: Color32| if hovered && enabled && !pressed { mix(c, Color32::WHITE, 0.07) } else { c };
    let fg = match kind {
        Kind::Cream => {
            let (t, b) = if pressed { (pal().cream_bot, mix(pal().cream_bot, pal().ink, 0.08)) } else { (lift(pal().cream_top), lift(pal().cream_bot)) };
            if selected {
                glow(p, rect, r, pal().orange, 6.0);
            }
            bevel(p, rect, r, t, b, if selected { pal().orange_edge } else { pal().edge }, (!pressed).then_some(pal().cream_hl));
            pal().ink
        }
        Kind::Orange => {
            glow(p, rect, r, pal().orange, if pressed { 4.0 } else { 9.0 });
            let (t, b) = if pressed { (pal().orange_bot, pal().orange_bot) } else { (lift(pal().orange_top), lift(pal().orange_bot)) };
            bevel(p, rect, r, t, b, pal().orange_edge, (!pressed).then_some(pal().orange_hl));
            pal().on_orange
        }
        Kind::Dark => {
            p.rect_filled(rect, r, lift(pal().dark_tile));
            p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal().edge));
            pal().text
        }
        Kind::Tile => {
            if selected {
                p.rect_filled(rect, r, lift(pal().orange));
                p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal().edge));
                pal().on_orange
            } else {
                p.rect_filled(rect, r, lift(pal().btn_dark));
                p.rect_stroke(rect, r, Stroke::new(1.0_f32, pal().edge));
                pal().text_dim
            }
        }
        Kind::Ghost => {
            if hovered {
                p.rect_filled(rect, r, Color32::from_white_alpha(8));
            }
            dashed_rect(p, rect, pal().seg_on);
            pal().text_muted
        }
    };
    if enabled { fg } else { with_alpha(fg, 90) }
}

/// Display theme: flat dark fills, outlines in the label color, glow when lit.
#[allow(clippy::too_many_arguments)]
fn paint_screen_button(p: &Painter, rect: Rect, r: f32, kind: Kind, selected: bool, pressed: bool, hovered: bool, enabled: bool) -> Color32 {
    let c = pal();
    let fill = |base: Color32| {
        if pressed {
            mix(base, c.bg, 0.35)
        } else if hovered && enabled {
            mix(base, Color32::WHITE, 0.05)
        } else {
            base
        }
    };
    let outlined = |ink: Color32, alpha: u8| {
        p.rect_stroke(rect, r, Stroke::new(1.5_f32, with_alpha(ink, alpha)));
    };
    let fg = match kind {
        Kind::Cream => {
            let ink = if selected { c.orange } else { c.ink };
            if selected && !pressed {
                glow(p, rect, r, ink, 6.0);
            }
            p.rect_filled(rect, r, fill(c.cream_top));
            outlined(ink, if selected { 255 } else { 70 });
            ink
        }
        Kind::Orange => {
            glow(p, rect, r, c.orange, if pressed { 4.0 } else { 9.0 });
            p.rect_filled(rect, r, if pressed { c.orange_bot } else { fill(c.orange) });
            outlined(c.orange_edge, 255);
            c.on_orange
        }
        Kind::Dark => {
            p.rect_filled(rect, r, fill(c.dark_tile));
            outlined(c.ink, 55);
            c.ink
        }
        Kind::Tile => {
            if selected {
                glow(p, rect, r, c.orange, 5.0);
                p.rect_filled(rect, r, fill(c.orange));
                c.on_orange
            } else {
                p.rect_filled(rect, r, fill(c.btn_dark));
                outlined(c.ink, 55);
                c.ink
            }
        }
        Kind::Ghost => {
            if hovered {
                p.rect_filled(rect, r, Color32::from_white_alpha(6));
            }
            dashed_rect(p, rect, with_alpha(c.ink, 120));
            c.ink
        }
    };
    if enabled { fg } else { with_alpha(fg, 90) }
}

/// Generic button: `content` draws inside the rect with the given color.
pub fn button(ui: &mut Ui, size: Vec2, kind: Kind, selected: bool, content: impl FnOnce(&Painter, Rect, Color32)) -> Response {
    let enabled = ui.is_enabled();
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fg = paint_button(p, rect, 8.0, kind, selected, resp.is_pointer_button_down_on(), resp.hovered(), enabled);
        content(p, rect, fg);
    }
    resp
}

pub fn icon_button(ui: &mut Ui, size: Vec2, kind: Kind, icon: Icon, icon_size: f32) -> Response {
    button(ui, size, kind, false, |p, r, c| icons::draw(p, r, icon_size, icon, c, 1.9))
}

pub fn text_button(ui: &mut Ui, size: Vec2, kind: Kind, text: &str, font: FontId) -> Response {
    button(ui, size, kind, false, |p, r, c| {
        gtext(&p, r.center(), Align2::CENTER_CENTER, text, font, c);
    })
}

/// Icon above a small label (edit bar).
pub fn icon_caption_button(ui: &mut Ui, size: Vec2, icon: Icon, text: &str) -> Response {
    button(ui, size, Kind::Cream, false, |p, r, c| {
        icons::draw(p, Rect::from_center_size(r.center() - vec2(0.0, 8.0), vec2(20.0, 20.0)), 20.0, icon, c, 1.9);
        gtext(&p, r.center() + vec2(0.0, 14.0), Align2::CENTER_CENTER, text, body(11.5), c);
    })
}

/// Icon + text side by side.
pub fn icon_text_button(ui: &mut Ui, h: f32, kind: Kind, icon: Icon, text: &str) -> Response {
    let font = body(15.0);
    let tw = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), pal().text).size().x);
    let w = 14.0 + 20.0 + 8.0 + tw + 14.0;
    button(ui, vec2(w, h), kind, false, |p, r, c| {
        icons::draw(p, Rect::from_min_size(pos2(r.left() + 14.0, r.center().y - 10.0), vec2(20.0, 20.0)), 20.0, icon, c, 2.0);
        gtext(&p, pos2(r.left() + 42.0, r.center().y), Align2::LEFT_CENTER, text, font, c);
    })
}

/// Text button, auto width.
pub fn auto_button(ui: &mut Ui, h: f32, kind: Kind, text: &str) -> Response {
    let font = body(14.5);
    let tw = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), pal().text).size().x);
    text_button(ui, vec2(tw + 28.0, h), kind, text, font)
}

// ---------------------------------------------------------------------------
// Segmented selector
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SegStyle {
    /// Light gray active segment.
    Grey,
    /// Orange active segment.
    Orange,
}

/// Returns `true` if the value changed.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    value: &mut T,
    opts: &[(T, Option<Icon>, &str)],
    h: f32,
    min_w: f32,
    style: SegStyle,
    font: FontId,
) -> bool {
    let pad = 3.0;
    let widths: Vec<f32> = opts
        .iter()
        .map(|(_, icon, text)| {
            let tw = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), pal().text).size().x);
            (tw + if icon.is_some() { 22.0 } else { 0.0 } + 24.0).max(min_w)
        })
        .collect();
    let total = widths.iter().sum::<f32>() + 2.0 * (opts.len().saturating_sub(1)) as f32 + 2.0 * pad;
    let (rect, _) = ui.allocate_exact_size(vec2(total, h + 2.0 * pad), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 8.0, pal().seg_bg);
    if screen() {
        p.rect_stroke(rect, 8.0, Stroke::new(1.0_f32, pal().edge));
    }
    let mut x = rect.left() + pad;
    let mut changed = false;
    for (i, ((v, icon, text), w)) in opts.iter().zip(widths).enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + pad), vec2(w, h));
        let resp = ui.interact(r, ui.id().with(("seg", rect.min.x as i32, rect.min.y as i32, i)), Sense::click());
        if resp.clicked() && *value != *v {
            *value = *v;
            changed = true;
        }
        let on = *value == *v;
        let fg = if on {
            match style {
                SegStyle::Orange => {
                    if screen() {
                        glow(&p, r, 6.0, pal().orange, 5.0);
                    }
                    p.rect_filled(r, 6.0, pal().orange);
                    pal().on_orange
                }
                SegStyle::Grey => {
                    if screen() {
                        p.rect_stroke(r, 6.0, Stroke::new(1.5_f32, pal().ink));
                    } else {
                        p.rect_filled(r, 6.0, pal().seg_on);
                    }
                    pal().white
                }
            }
        } else {
            if resp.hovered() {
                p.rect_filled(r, 6.0, Color32::from_white_alpha(10));
            }
            if screen() { pal().ink } else { pal().text_dim }
        };
        match icon {
            Some(ic) => {
                let tw = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), pal().text).size().x);
                let x0 = r.center().x - (tw + 22.0) / 2.0;
                icons::draw(&p, Rect::from_min_size(pos2(x0, r.center().y - 8.0), vec2(16.0, 16.0)), 16.0, *ic, fg, 2.0);
                gtext(&p, pos2(x0 + 22.0, r.center().y), Align2::LEFT_CENTER, *text, font.clone(), fg);
            }
            None => {
                gtext(&p, r.center(), Align2::CENTER_CENTER, *text, font.clone(), fg);
            }
        }
        x += w + 2.0;
    }
    changed
}

// ---------------------------------------------------------------------------
// LCD
// ---------------------------------------------------------------------------

/// LCD display: small label + large value. Returns the response (drag = edit).
pub fn lcd(ui: &mut Ui, size: Vec2, label: &str, value: &str, color: Color32, font: FontId, sense: Sense) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let p = ui.painter();
    lcd_bg(p, rect, 9.0);
    let left = rect.left() + 16.0;
    if label.is_empty() {
        lcd_text(p, rect.center(), Align2::CENTER_CENTER, value, font, color);
    } else if size.x < 130.0 {
        caption(p, pos2(rect.center().x, rect.top() + 11.0), Align2::CENTER_CENTER, label, pal().lcd_label);
        lcd_text(p, pos2(rect.center().x, rect.center().y + 7.0), Align2::CENTER_CENTER, value, font, color);
    } else {
        caption(p, pos2(left, rect.top() + 11.0), Align2::LEFT_CENTER, label, pal().lcd_label);
        lcd_text(p, pos2(left, rect.center().y + 7.0), Align2::LEFT_CENTER, value, font, color);
    }
    resp
}

// ---------------------------------------------------------------------------
// Knob, switch, slider
// ---------------------------------------------------------------------------

fn arc_points(c: Pos2, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
    let n = (((a1 - a0).abs() / 0.08) as usize).max(2);
    (0..=n)
        .map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + r * a.sin(), c.y - r * a.cos())
        })
        .collect()
}

/// Rotary knob (drag vertically; double-click = default value).
#[allow(clippy::too_many_arguments)]
pub fn knob(ui: &mut Ui, value: &mut f32, min: f32, max: f32, default: f32, d: f32, bipolar: bool, mark: Color32) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(d, d), Sense::click_and_drag());
    if resp.dragged() {
        let dv = -resp.drag_delta().y / 180.0 * (max - min);
        let fine = ui.input(|i| i.modifiers.shift);
        *value = (*value + if fine { dv * 0.2 } else { dv }).clamp(min, max);
        resp.mark_changed();
    }
    if resp.double_clicked() {
        *value = default;
        resp.mark_changed();
    }
    let p = ui.painter();
    let c = rect.center();
    let ring_w = d * 0.11;
    let r_ring = d / 2.0 - ring_w / 2.0;
    let a0 = -135f32.to_radians();
    let a1 = 135f32.to_radians();
    let t = ((*value - min) / (max - min)).clamp(0.0, 1.0);
    let av = a0 + (a1 - a0) * t;
    p.add(Shape::line(arc_points(c, r_ring, a0, a1), Stroke::new(ring_w, pal().track)));
    let (from, to) = if bipolar { (0.0, av) } else { (a0, av) };
    if (to - from).abs() > 0.01 {
        p.add(Shape::line(arc_points(c, r_ring, from.min(to), from.max(to)), Stroke::new(ring_w, pal().orange)));
    }
    let cap = d * 0.39;
    p.circle_filled(c + vec2(0.0, 3.0), cap, Color32::from_black_alpha(110));
    p.circle_filled(c, cap, pal().btn_dark);
    p.circle_filled(c - vec2(cap * 0.2, cap * 0.35), cap * 0.62, with_alpha(pal().seg_on, 120));
    p.circle_stroke(c, cap, Stroke::new(1.0_f32, pal().edge));
    let dir = vec2(av.sin(), -av.cos());
    p.line_segment([c + dir * (cap - 6.0 - cap * 0.5), c + dir * (cap - 6.0)], Stroke::new(4.0_f32, mark));
    resp
}

/// Pill switch (ON/OFF). Returns the response (click = toggle already applied).
pub fn pill_switch(ui: &mut Ui, on: &mut bool, size: Vec2, labels: Option<(&str, &str)>) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let p = ui.painter();
    let r = rect.height() / 2.0;
    let anim = ui.ctx().animate_bool(resp.id, *on);
    p.rect_filled(rect, r, mix(pal().track, pal().orange, anim));
    p.line_segment([pos2(rect.left() + r, rect.top() + 1.5), pos2(rect.right() - r, rect.top() + 1.5)], Stroke::new(2.0_f32, Color32::from_black_alpha(80)));
    let kr = r - 4.0;
    let kx = rect.left() + r + (rect.width() - 2.0 * r) * anim;
    let kc = pos2(kx, rect.center().y);
    p.circle_filled(kc + vec2(0.0, 1.5), kr, Color32::from_black_alpha(90));
    p.circle_filled(kc, kr, mix(pal().seg_on, Color32::WHITE, anim));
    p.circle_stroke(kc, kr, Stroke::new(1.0_f32, pal().edge));
    if let Some((l_on, l_off)) = labels {
        let (text, pos, align, col) = if *on {
            (l_on, pos2(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, pal().on_orange)
        } else {
            (l_off, pos2(rect.right() - 14.0, rect.center().y), Align2::RIGHT_CENTER, pal().text_dimmer)
        };
        gtext(&p, pos, align, text, bold(15.0), col);
    }
    resp
}

/// Horizontal slider with a metal handle (swing).
pub fn metal_slider(ui: &mut Ui, value: &mut f32, w: f32) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click_and_drag());
    if (resp.dragged() || resp.clicked()) && let Some(pos) = resp.interact_pointer_pos() {
        *value = ((pos.x - rect.left() - 14.0) / (rect.width() - 28.0)).clamp(0.0, 1.0);
        resp.mark_changed();
    }
    if resp.double_clicked() {
        *value = 0.0;
        resp.mark_changed();
    }
    let p = ui.painter();
    let track = Rect::from_center_size(rect.center(), vec2(rect.width(), 8.0));
    p.rect_filled(track, 4.0, pal().track);
    let x = rect.left() + 14.0 + (rect.width() - 28.0) * *value;
    p.rect_filled(Rect::from_min_max(track.left_top(), pos2(x, track.bottom())), 4.0, pal().orange);
    let thumb = Rect::from_center_size(pos2(x, rect.center().y), vec2(28.0, 28.0));
    p.circle_filled(thumb.center() + vec2(0.0, 2.0), 14.0, Color32::from_black_alpha(110));
    vgradient(p, thumb, 14.0, pal().metal_top, pal().metal_bot);
    p.circle_stroke(thumb.center(), 14.0, Stroke::new(1.0_f32, pal().groove));
    resp
}

// ---------------------------------------------------------------------------
// Dropdown menus
// ---------------------------------------------------------------------------

/// Dropdown menu trigger (dark tile + indicator light + chevron).
pub fn dropdown(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash,
    size: Vec2,
    led_color: Option<(Color32, bool)>,
    text: &str,
    suffix: Option<&str>,
    add_contents: impl FnOnce(&mut Ui),
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 7.0, if resp.hovered() { pal().hover_dropdown } else { pal().dark_tile });
    p.rect_stroke(rect, 7.0, Stroke::new(1.0_f32, pal().edge));
    let mut x = rect.left() + 12.0;
    if let Some((c, round)) = led_color {
        if round {
            led(p, pos2(x + 4.0, rect.center().y), 4.0, c, c != pal().seg_on);
        } else {
            p.rect_filled(Rect::from_center_size(pos2(x + 5.0, rect.center().y), vec2(10.0, 10.0)), 3.0, c);
        }
        x += 18.0;
    }
    let clip = p.with_clip_rect(Rect::from_min_max(pos2(x, rect.top()), pos2(rect.right() - 30.0, rect.bottom())));
    let g = clip.layout_no_wrap(text.to_owned(), body(15.0), pal().text);
    let tw = g.size().x;
    clip.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, pal().text);
    if let Some(s) = suffix {
        clip.text(pos2(x + tw + 6.0, rect.center().y), Align2::LEFT_CENTER, s, medium(13.0), pal().text_dimmer);
    }
    icons::draw(p, Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(14.0, 14.0)), 14.0, Icon::Chevron, pal().text_dim, 2.2);

    let popup_id = ui.make_persistent_id(id_salt);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup_below_widget(ui, popup_id, &resp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(rect.width().max(220.0));
        egui::ScrollArea::vertical().max_height(420.0).show(ui, add_contents);
    });
    resp
}

/// Menu row (popups).
pub fn menu_item(ui: &mut Ui, text: &str, color: Color32, selected: bool) -> Response {
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
    let p = ui.painter();
    if selected || resp.hovered() {
        p.rect_filled(rect, 7.0, if selected { pal().tile_sel } else { pal().hover_row });
    }
    if selected {
        p.rect_filled(Rect::from_min_size(rect.left_top() + vec2(0.0, 8.0), vec2(3.0, rect.height() - 16.0)), 1.5, pal().orange);
    }
    gtext(&p, pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, text, body(15.5), color);
    resp
}

/// Greyed menu row that does nothing (an action that is not possible, and why).
pub fn menu_disabled(ui: &mut Ui, text: &str) {
    let w = ui.available_width().max(200.0);
    let (rect, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, text, body(15.5), pal().text_dimmer);
}

/// Menu row with an on/off switch on the right (the click toggles: up to the caller).
pub fn menu_toggle(ui: &mut Ui, text: &str, on: bool) -> Response {
    let w = ui.available_width().max(200.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 40.0), Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 7.0, pal().hover_row);
    }
    gtext(&p, pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, text, body(15.5), pal().text);
    let pill = Rect::from_center_size(pos2(rect.right() - 12.0 - 19.0, rect.center().y), vec2(38.0, 20.0));
    p.rect_filled(pill, 10.0, if on { pal().orange } else { pal().track });
    p.rect_stroke(pill, 10.0, Stroke::new(1.0_f32, pal().edge));
    let kc = pos2(if on { pill.right() - 10.0 } else { pill.left() + 10.0 }, pill.center().y);
    p.circle_filled(kc, 7.0, if on { pal().cream_hl } else { pal().seg_on });
    p.circle_stroke(kc, 7.0, Stroke::new(1.0_f32, pal().edge));
    resp
}

/// Group title inside a menu.
pub fn menu_caption(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 20.0), Sense::hover());
    caption(ui.painter(), pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, text, pal().text_dim);
}

pub fn menu_separator(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(200.0), 9.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.left(), rect.center().y), pos2(rect.right(), rect.center().y)], Stroke::new(1.0_f32, pal().divider));
}

// ---------------------------------------------------------------------------
// Panel with a wooden header
// ---------------------------------------------------------------------------

pub struct PanelResult {
    pub body: Rect,
    pub expand: bool,
}

/// Draws a panel's frame and its header; `header` adds the header controls.
pub fn panel(ui: &mut Ui, rect: Rect, title: &str, led_color: Color32, header: impl FnOnce(&mut Ui)) -> PanelResult {
    panel_with(ui, rect, title, led_color, true, header)
}

/// Panel without the expand button (sections of the Settings dialog).
pub fn panel_bare(ui: &mut Ui, rect: Rect, title: &str, led_color: Color32, header: impl FnOnce(&mut Ui)) -> PanelResult {
    panel_with(ui, rect, title, led_color, false, header)
}

fn panel_with(ui: &mut Ui, rect: Rect, title: &str, led_color: Color32, expandable: bool, header: impl FnOnce(&mut Ui)) -> PanelResult {
    let p = ui.painter_at(rect.expand(8.0));
    p.rect_filled(rect, 10.0, pal().panel);
    let head = Rect::from_min_size(rect.min, vec2(rect.width(), 52.0));
    let screen = screen();
    // Wood: gradient + grain lines (Display: the plain screen).
    vgradient(&p, Rect::from_min_max(head.min, head.max + vec2(0.0, 10.0)), 10.0, pal().header_top, pal().header_bot);
    p.rect_filled(
        Rect::from_min_max(pos2(rect.left(), head.bottom()), rect.max),
        Rounding { nw: 0.0, ne: 0.0, sw: 10.0, se: 10.0 },
        pal().panel,
    );
    let sp = p.with_clip_rect(head.shrink2(vec2(2.0, 1.0)));
    let mut x = if screen { head.right() } else { head.left() - 4.0 };
    while x < head.right() {
        sp.line_segment([pos2(x, head.top()), pos2(x + 2.0, head.bottom())], Stroke::new(3.0_f32, Color32::from_black_alpha(20)));
        x += 11.0;
    }
    p.line_segment([pos2(rect.left(), head.bottom()), pos2(rect.right(), head.bottom())], Stroke::new(1.0_f32, if screen { pal().divider } else { pal().edge }));
    p.line_segment([pos2(rect.left() + 8.0, rect.top() + 1.0), pos2(rect.right() - 8.0, rect.top() + 1.0)], Stroke::new(1.0_f32, with_alpha(pal().header_top, 255)));
    p.rect_stroke(rect, 10.0, Stroke::new(1.0_f32, pal().edge));

    led_square(&p, pos2(head.left() + 19.0, head.center().y), 10.0, led_color);
    let tf = title_font(14.0);
    let shadow = if screen { with_alpha(pal().header_text, 45) } else { Color32::from_black_alpha(130) };
    gtext(&p, pos2(head.left() + 36.0, head.center().y + 1.0), Align2::LEFT_CENTER, title, tf.clone(), shadow);
    let tg = p.layout_no_wrap(title.to_owned(), tf, pal().header_text);
    let tw = tg.size().x;
    p.galley(pos2(head.left() + 36.0, head.center().y - tg.size().y / 2.0), tg, pal().header_text);

    // Header controls.
    let hx0 = head.left() + 36.0 + tw + 14.0;
    let exp_rect = Rect::from_center_size(pos2(head.right() - 6.0 - 22.0, head.center().y), vec2(44.0, 44.0));
    let hrect = Rect::from_min_max(pos2(hx0, head.top()), pos2(exp_rect.left() - 8.0, head.bottom()));
    if hrect.width() > 10.0 {
        let mut child = ui.new_child(UiBuilder::new().max_rect(hrect).layout(Layout::left_to_right(Align::Center)));
        child.set_clip_rect(hrect.intersect(ui.clip_rect()));
        child.spacing_mut().item_spacing.x = 10.0;
        header(&mut child);
    }
    let expand = expandable && {
        let mut eu = ui.new_child(UiBuilder::new().max_rect(exp_rect));
        icon_button(&mut eu, vec2(44.0, 44.0), Kind::Cream, Icon::Expand, 20.0)
            .on_hover_text(seq_i18n::tr!("Agrandir / réduire le panneau", "Expand / collapse panel"))
            .clicked()
    };
    PanelResult { body: Rect::from_min_max(pos2(rect.left(), head.bottom() + 1.0), rect.max), expand }
}

/// Subtle help text (headers).
pub fn hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).font(medium(13.0)).color(pal().text_dimmer));
}

/// Expanding space in a horizontal row (pushes what follows to the right).
pub fn spacer_right(ui: &mut Ui, reserve: f32) {
    let w = (ui.available_width() - reserve).max(0.0);
    ui.add_space(w);
}
