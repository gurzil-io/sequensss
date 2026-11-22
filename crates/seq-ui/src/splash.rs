//! Launch splash, drawn once above the UI while the app starts behind it: the snake fades in
//! from the dark, its eyes light up and breathe, the pins of its MIDI connector light up one by
//! one and the title types itself, then everything fades into the app (sooner on a tap or key).
//! The snake hisses as its eyes light up: the app plays the sound while [`Splash::hissing`].
//!
//! The image and the eye / pin positions come from `assets/icon/make-icons.py`
//! (`splash_geom.rs`), so the glows sit exactly on the drawing.

use crate::splash_geom::{ASPECT, EYES, PINS};
use crate::theme::{self, pal};
use eframe::egui::{self, Color32, Mesh, Painter, Pos2, Rect, Sense, TextureHandle, pos2, vec2};
use std::f32::consts::TAU;

const IMAGE: &[u8] = include_bytes!("../assets/splash-snake.png");
const TITLE: &str = "SEQUENSSS";

/// Seconds from the first frame to the end of the fade out.
pub const DURATION: f32 = 2.6;
/// Fade into the app at the end.
const FADE_OUT: f32 = 0.3;
/// Faster fade after a tap or a key.
const SKIP_FADE: f32 = 0.18;
/// The eyes start lighting up (and the snake hisses).
const EYES_AT: f32 = 0.35;
/// Longest step of the splash clock: slow first frames (texture upload, app start) don't
/// swallow the fade in.
const MAX_STEP: f32 = 1.0 / 20.0;

/// What the splash looks like at a given time (all values 0..1 unless stated).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Whole splash, background included (fade out).
    pub opacity: f32,
    /// Snake fading in.
    pub snake: f32,
    /// Scale of the snake (slight zoom in).
    pub zoom: f32,
    /// Eye glow: rises, then breathes.
    pub eyes: f32,
    /// Each connector pin, lit one after the other.
    pub pins: [f32; 5],
    /// Title letters shown; the fraction is the next letter fading in (0..=9).
    pub letters: f32,
    /// Version line under the title.
    pub subtitle: f32,
    /// Over: the app is fully visible.
    pub done: bool,
}

fn ramp(t: f32, start: f32, len: f32) -> f32 {
    ((t - start) / len).clamp(0.0, 1.0)
}

fn smooth(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

/// Timing curve: `t` seconds since the splash appeared, `skipped_at` when it was tapped away.
pub fn frame(t: f32, skipped_at: Option<f32>) -> Frame {
    let snake = ease_out(ramp(t, 0.0, 0.4));
    // Eyes light up over 0.5 s, then breathe gently (continuous at the hand-over).
    let lit_at = 0.85;
    let breath = if t > lit_at { 0.85 + 0.15 * (TAU * (t - lit_at) / 1.1).cos() } else { 1.0 };
    let eyes = smooth(ramp(t, EYES_AT, 0.5)) * breath;
    let pins = std::array::from_fn(|i| smooth(ramp(t, 0.55 + 0.09 * i as f32, 0.06)));
    let letters = ramp(t, 0.45, 0.06 * TITLE.len() as f32) * TITLE.len() as f32;
    let natural = DURATION - FADE_OUT;
    let (fade_at, fade_len) = match skipped_at {
        Some(s) if s < natural => (s, SKIP_FADE),
        _ => (natural, FADE_OUT),
    };
    let out = ramp(t, fade_at, fade_len);
    // (Tolerance: `DURATION - FADE_OUT + FADE_OUT` is not exactly `DURATION` in f32.)
    let done = out > 0.999;
    Frame {
        opacity: if done { 0.0 } else { 1.0 - smooth(out) },
        snake,
        zoom: 0.92 + 0.08 * snake,
        eyes,
        pins,
        letters,
        subtitle: smooth(ramp(t, 1.0, 0.25)),
        done,
    }
}

/// The running splash (owned by the app, dropped when [`Splash::show`] returns `false`).
pub struct Splash {
    version: String,
    texture: Option<TextureHandle>,
    t: f32,
    last: Option<f64>,
    skipped_at: Option<f32>,
    done: bool,
}

impl Splash {
    /// `version`: shown under the title.
    pub fn new(version: &str) -> Self {
        Splash { version: version.to_owned(), texture: None, t: 0.0, last: None, skipped_at: None, done: false }
    }

    /// Draws one frame above everything and swallows the input; `false` once it is over.
    /// Call it before the UI so that the key which skips it doesn't also reach the app.
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let now = ctx.input(|i| i.time);
        if let Some(last) = self.last {
            self.t += ((now - last) as f32).clamp(0.0, MAX_STEP);
        }
        self.last = Some(now);
        let skip = ctx.input(|i| {
            i.pointer.any_pressed() || i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
        });
        if skip && self.skipped_at.is_none() {
            self.skipped_at = Some(self.t);
        }
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_))));

        let f = frame(self.t, self.skipped_at);
        if f.done {
            self.done = true;
            return false;
        }
        let texture = self.texture.get_or_insert_with(|| load(ctx)).clone();
        let screen = ctx.screen_rect();
        let subtitle = format!("v{}", self.version);
        // Covers the screen: taps land here, not on the UI behind.
        egui::Area::new(egui::Id::new("splash_input")).order(egui::Order::Debug).fixed_pos(screen.min).show(ctx, |ui| {
            ui.allocate_rect(screen, Sense::click());
        });
        // Painted on a plain layer: a new Area stays invisible during its first frame.
        let mut p = ctx.layer_painter(egui::LayerId::new(egui::Order::Debug, egui::Id::new("splash")));
        p.set_opacity(f.opacity);
        paint(&p, screen, &f, &texture, &subtitle);
        ctx.request_repaint();
        true
    }

    /// The hiss should be sounding: from the eyes lighting up until the splash is skipped or
    /// over (the app then fades it out).
    pub fn hissing(&self) -> bool {
        self.t >= EYES_AT && self.skipped_at.is_none() && !self.done
    }
}

fn load(ctx: &egui::Context) -> TextureHandle {
    let img = image::load_from_memory_with_format(IMAGE, image::ImageFormat::Png).expect("splash PNG").to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    // Mipmaps: the image is drawn smaller than its pixels on most screens.
    let options = egui::TextureOptions { mipmap_mode: Some(egui::TextureFilter::Linear), ..egui::TextureOptions::LINEAR };
    ctx.load_texture("splash", color, options)
}

/// Premultiplied color with zero alpha: adds light to what is below (additive blending).
fn light(r: u8, g: u8, b: u8, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    Color32::from_rgba_premultiplied((r as f32 * k) as u8, (g as f32 * k) as u8, (b as f32 * k) as u8, 0)
}

/// Additive radial glow (soft, roughly gaussian falloff) as one small mesh.
fn glow(p: &Painter, c: Pos2, radius: f32, (r, g, b): (u8, u8, u8), k: f32) {
    const SEGMENTS: u32 = 32;
    const RINGS: [(f32, f32); 5] = [(0.2, 0.85), (0.4, 0.55), (0.6, 0.27), (0.8, 0.09), (1.0, 0.0)];
    if k <= 0.0 || radius <= 0.0 {
        return;
    }
    let mut mesh = Mesh::default();
    mesh.colored_vertex(c, light(r, g, b, k));
    for (ring, &(d, fall)) in RINGS.iter().enumerate() {
        for i in 0..SEGMENTS {
            let a = TAU * i as f32 / SEGMENTS as f32;
            mesh.colored_vertex(c + radius * d * vec2(a.cos(), a.sin()), light(r, g, b, k * fall));
            let (j, ring) = ((i + 1) % SEGMENTS, ring as u32);
            let at = |ring: u32, i: u32| 1 + ring * SEGMENTS + i;
            if ring == 0 {
                mesh.add_triangle(0, at(0, i), at(0, j));
            } else {
                mesh.add_triangle(at(ring - 1, i), at(ring, i), at(ring, j));
                mesh.add_triangle(at(ring - 1, i), at(ring, j), at(ring - 1, j));
            }
        }
    }
    p.add(mesh);
}

/// Title with one text section per letter, so letters fade in without moving the others.
fn title_job(size: f32, letters: f32) -> egui::text::LayoutJob {
    let font = theme::title_font(size);
    let spacing = if theme::screen() { size * 0.18 } else { size * 0.22 };
    let mut job = egui::text::LayoutJob::default();
    for (i, ch) in TITLE.chars().enumerate() {
        let a = (letters - i as f32).clamp(0.0, 1.0);
        let format = egui::TextFormat { font_id: font.clone(), color: pal().orange.gamma_multiply(a), extra_letter_spacing: spacing, ..Default::default() };
        job.append(&ch.to_string(), 0.0, format);
    }
    job
}

fn paint(p: &Painter, screen: Rect, f: &Frame, texture: &TextureHandle, subtitle: &str) {
    // Same dark as the Android launch window, in both themes, so the hand-over is seamless.
    p.rect_filled(screen, 0.0, theme::DISPLAY.bg);

    let snake_h = (screen.height() * 0.56).min(screen.width() * 0.6 / ASPECT);
    let title_size = snake_h * 0.1;
    let title = p.layout_job(title_job(title_size, f.letters));
    let sub = p.layout_no_wrap(subtitle.to_owned(), theme::body((snake_h * 0.05).max(11.0)), pal().text_dim.gamma_multiply(f.subtitle));
    let gap = snake_h * 0.07;
    let block = snake_h + gap + title.size().y + gap * 0.3 + sub.size().y;
    let top = screen.center().y - block / 2.0;
    let cx = screen.center().x;

    let full = Rect::from_min_size(pos2(cx - snake_h * ASPECT / 2.0, top), vec2(snake_h * ASPECT, snake_h));
    let img = Rect::from_center_size(full.center(), full.size() * f.zoom);
    let at = |[x, y, _]: [f32; 3]| img.min + vec2(x * img.width(), y * img.height());
    p.image(texture.id(), img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_white_alpha((f.snake * 255.0) as u8));

    let eyes = f.eyes * f.snake;
    let biggest = EYES.iter().map(|e| e[2]).fold(0.0, f32::max);
    for e in EYES {
        // The far eye is half hidden: its halo is a bit larger than its visible part.
        let r = e[2].max(biggest * 0.6) * img.height();
        glow(p, at(e), r * 4.0, (255, 170, 50), 0.7 * eyes);
        glow(p, at(e), r * 1.5, (255, 235, 150), 0.65 * eyes);
    }
    for (pin, &lit) in PINS.iter().zip(&f.pins) {
        let r = pin[2] * img.height();
        glow(p, at(*pin), (r * 7.0).max(6.0), (255, 140, 30), 0.8 * lit * f.snake);
        glow(p, at(*pin), (r * 2.2).max(2.5), (255, 230, 160), lit * f.snake);
    }

    let title_y = full.bottom() + gap;
    let sub_pos = pos2(cx - sub.size().x / 2.0, title_y + title.size().y + gap * 0.3);
    p.galley(pos2(cx - title.size().x / 2.0, title_y), title, Color32::WHITE);
    p.galley(sub_pos, sub, Color32::WHITE);

    if theme::screen() {
        crate::scanlines(p, screen);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_dark_and_ends_in_the_app() {
        let f = frame(0.0, None);
        assert_eq!((f.opacity, f.snake, f.eyes, f.letters, f.subtitle), (1.0, 0.0, 0.0, 0.0, 0.0));
        assert_eq!(f.pins, [0.0; 5]);
        assert!(f.zoom < 1.0 && !f.done);
        assert!(!frame(DURATION - 0.01, None).done);
        assert!(frame(DURATION, None).done);
        assert_eq!(frame(DURATION, None).opacity, 0.0);
    }

    #[test]
    fn snake_then_eyes_then_title_and_pins() {
        let f = frame(0.4, None);
        assert_eq!((f.snake, f.zoom), (1.0, 1.0));
        assert!(f.eyes < 0.05);
        let f = frame(1.3, None);
        assert_eq!(f.letters, TITLE.len() as f32);
        assert_eq!(f.pins, [1.0; 5]);
        assert_eq!((f.opacity, f.subtitle), (1.0, 1.0));
        assert!(f.eyes > 0.6);
    }

    #[test]
    fn pins_light_up_one_by_one() {
        let f = frame(0.72, None);
        assert!(f.pins[0] == 1.0 && f.pins[1] == 1.0 && f.pins[4] == 0.0, "{:?}", f.pins);
        for t in 0..40 {
            let f = frame(t as f32 * 0.05, None);
            assert!(f.pins.windows(2).all(|w| w[0] >= w[1]), "{t}: {:?}", f.pins);
        }
    }

    #[test]
    fn curves_stay_in_range_and_never_go_back() {
        let mut prev = frame(0.0, None);
        for i in 1..=200 {
            let f = frame(i as f32 * 0.01, None);
            for v in [f.opacity, f.snake, f.eyes, f.letters / TITLE.len() as f32, f.subtitle].into_iter().chain(f.pins) {
                assert!((0.0..=1.0).contains(&v), "{i}: {f:?}");
            }
            assert!(f.snake >= prev.snake && f.letters >= prev.letters && f.subtitle >= prev.subtitle);
            assert!(f.opacity <= prev.opacity);
            // The breathing is gentle and continuous.
            assert!((f.eyes - prev.eyes).abs() < 0.05, "{i}: {} → {}", prev.eyes, f.eyes);
            prev = f;
        }
    }

    #[test]
    fn eyes_breathe_while_visible() {
        let eyes: Vec<f32> = (0..30).map(|i| frame(0.9 + i as f32 * 0.03, None).eyes).collect();
        let (lo, hi) = eyes.iter().fold((f32::MAX, 0.0f32), |(lo, hi), &e| (lo.min(e), hi.max(e)));
        assert!(lo >= 0.69 && hi > 0.95 && hi - lo > 0.2, "{lo}..{hi}");
    }

    #[test]
    fn a_tap_fades_out_quickly() {
        let f = frame(0.4, Some(0.3));
        assert!(f.opacity < 1.0 && !f.done);
        assert!(frame(0.3 + SKIP_FADE, Some(0.3)).done);
        // During the natural fade out a tap changes nothing.
        let late = DURATION - 0.1;
        assert_eq!(frame(late, Some(DURATION - 0.2)), frame(late, None));
    }

    /// Runs one frame at `time` (seconds), with `events`; returns whether the splash is still up
    /// and the events left for the app.
    fn run(ctx: &egui::Context, splash: &mut Splash, time: f64, events: Vec<egui::Event>) -> (bool, usize) {
        let input = egui::RawInput { time: Some(time), events, ..Default::default() };
        let (mut up, mut left) = (false, 0);
        let _ = ctx.run(input, |ctx| {
            up = splash.show(ctx);
            left = ctx.input(|i| i.events.len());
        });
        (up, left)
    }

    #[test]
    fn runs_its_course_and_swallows_the_skipping_key() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut splash = Splash::new("1.2.3");
        let mut t = 0.0;
        while run(&ctx, &mut splash, t, Vec::new()).0 {
            t += 1.0 / 60.0;
            assert!(t < 3.0, "never ends");
        }
        assert!(t >= (DURATION - 0.05) as f64, "ended at {t}");

        let mut splash = Splash::new("1.2.3");
        assert_eq!(run(&ctx, &mut splash, 0.0, Vec::new()), (true, 0));
        let space = egui::Event::Key { key: egui::Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        assert_eq!(run(&ctx, &mut splash, 0.1, vec![space]), (true, 0));
        let mut t = 0.1;
        while run(&ctx, &mut splash, t, Vec::new()).0 {
            t += 1.0 / 60.0;
        }
        assert!(t - 0.1 < (SKIP_FADE + 0.05) as f64, "skipped after {t}");
    }

    #[test]
    fn hisses_with_the_eyes_until_skipped_or_over() {
        let ctx = egui::Context::default();
        theme::install(&ctx);
        let mut splash = Splash::new("1.2.3");
        let mut t = 0.0;
        while run(&ctx, &mut splash, t, Vec::new()).0 {
            assert_eq!(splash.hissing(), splash.t >= EYES_AT, "{t}");
            t += 1.0 / 60.0;
        }
        assert!(!splash.hissing());

        let mut splash = Splash::new("1.2.3");
        for i in 0..30 {
            run(&ctx, &mut splash, i as f64 / 60.0, Vec::new());
        }
        assert!(splash.hissing());
        let click = egui::Event::PointerButton { pos: pos2(10.0, 10.0), button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() };
        assert!(run(&ctx, &mut splash, 0.5, vec![click]).0);
        assert!(!splash.hissing());
    }

    #[test]
    fn geometry_lies_inside_the_image() {
        assert!(ASPECT > 0.5 && ASPECT < 1.5);
        for [x, y, r] in EYES.into_iter().chain(PINS) {
            assert!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) && r > 0.0 && r < 0.1);
        }
        // Eyes at the top right, connector at the bottom left.
        assert!(EYES.iter().all(|e| e[0] > 0.5 && e[1] < 0.3));
        assert!(PINS.iter().all(|p| p[0] < 0.5 && p[1] > 0.8));
    }
}
