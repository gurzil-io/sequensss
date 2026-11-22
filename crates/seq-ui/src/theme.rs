//! Themes ("Retro" and "Display"): palettes, fonts, egui visuals.
//!
//! Colors are read through [`pal`] so the theme can change at runtime; the two palettes share
//! every field. The values mirror the "Sequensss Retro" design system (tokens.json).

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, Vec2};
use seq_model::{Channel, Pattern, Rgb};
use std::sync::atomic::{AtomicU8, Ordering};

pub const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

/// The app's looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Theme {
    /// Domed cream keys, wood, warm metal.
    Retro,
    /// Black backlit screen with glowing amber and green outlines.
    Display,
}

impl Theme {
    pub const ALL: [Theme; 2] = [Theme::Retro, Theme::Display];

    pub fn code(self) -> &'static str {
        match self {
            Theme::Retro => "retro",
            Theme::Display => "display",
        }
    }

    pub fn from_code(code: &str) -> Option<Theme> {
        Theme::ALL.into_iter().find(|t| t.code() == code.trim())
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Retro => seq_i18n::tr!("Rétro", "Retro"),
            Theme::Display => seq_i18n::tr!("Écran", "Display"),
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn get() -> Theme {
    if CURRENT.load(Ordering::Relaxed) == 1 { Theme::Display } else { Theme::Retro }
}

/// Switches the theme; call [`apply`] afterwards so egui's own widgets follow.
pub fn set(theme: Theme) {
    CURRENT.store(if theme == Theme::Display { 1 } else { 0 }, Ordering::Relaxed);
}

/// Colors of the current theme.
pub fn pal() -> &'static Palette {
    palette(get())
}

/// Colors of `theme`.
pub fn palette(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Retro => &RETRO,
        Theme::Display => &DISPLAY,
    }
}

/// `true` with the Display theme: flat fills, glowing outlines, pixel titles.
pub fn screen() -> bool {
    pal().screen
}

pub struct Palette {
    /// Display look: skin draws outlines and glows instead of gradients and wood.
    pub screen: bool,
    /// App background (egui panel_fill). Ground for text, text-dim and text-dimmer.
    pub bg: Color32,
    /// Darkest wood tone, side cheeks of the top bar.
    pub wood_dark: Color32,
    /// Lighter wood grain highlight.
    pub wood_mid: Color32,
    /// Wood base, behind the panels.
    pub wood: Color32,
    /// Top of the transport bar gradient.
    pub bar_top: Color32,
    /// Bottom of the transport bar gradient.
    pub bar_bot: Color32,
    /// 1px highlight along the top edge of the transport bar.
    pub bar_hl: Color32,
    /// 1px outline of every control, panel and window.
    pub edge: Color32,
    /// Recessed separators between groups in the top bar; metal slider outline.
    pub groove: Color32,
    /// Top of the domed cream button gradient.
    pub cream_top: Color32,
    /// Bottom of the cream button gradient; also its pressed fill.
    pub cream_bot: Color32,
    /// Highlight line on a cream button; on-state knob of a menu toggle.
    pub cream_hl: Color32,
    /// Text and icons on cream buttons (12.7:1 on cream-top, 8.7:1 on cream-bot).
    pub ink: Color32,
    /// THE accent: selection, active segment, primary action, knob and slider fill, links. As text, on bg (7:1) and panel (5.8:1).
    pub orange: Color32,
    /// Top of the primary button gradient; BPM value on the LCD.
    pub orange_top: Color32,
    /// Bottom of the primary button gradient; its pressed fill.
    pub orange_bot: Color32,
    /// Outline of an orange button, and of a selected cream button.
    pub orange_edge: Color32,
    /// Highlight line on an orange button.
    pub orange_hl: Color32,
    /// Light orange for notes and highlights on dark grounds.
    pub orange_light: Color32,
    /// Text and icons on any orange fill (6.8:1 on orange, 5.2:1 on orange-bot). Never white.
    pub on_orange: Color32,
    /// LCD screen background (transport position, BPM).
    pub lcd_bg: Color32,
    /// LCD screen outline.
    pub lcd_edge: Color32,
    /// LCD digits (15:1 on lcd-bg).
    pub lcd_green: Color32,
    /// Small uppercase label above LCD digits (4.7:1 on lcd-bg).
    pub lcd_label: Color32,
    /// Dark tile button and dropdown trigger fill; egui extreme_bg.
    pub dark_tile: Color32,
    /// Track behind a segmented selector.
    pub seg_bg: Color32,
    /// Active segment (grey style), off knob of switches, ghost button dashes.
    pub seg_on: Color32,
    /// Default egui widget fill, unselected tile button, knob cap.
    pub btn_dark: Color32,
    /// Channel tile in the rack.
    pub tile: Color32,
    /// Selected channel tile; selected menu row.
    pub tile_sel: Color32,
    /// Primary text, on bg, panel, inner and btn-dark (8:1 or more).
    pub text: Color32,
    /// Secondary text and inactive segments, on bg, panel and seg-bg (5.2:1 on panel). Only 3.7:1 on btn-dark: unselected tile labels there are bold/condensed 15px+.
    pub text_dim: Color32,
    /// Hints, disabled menu rows, OFF switch label. 5:1 on bg but 4.1:1 on panel and 3.2:1 on popup: below AA at small sizes there, as the source has it.
    pub text_dimmer: Color32,
    /// Ghost (add) button label, on panel (5.9:1).
    pub text_muted: Color32,
    /// Panel body and window fill.
    pub panel: Color32,
    /// Highlight edge inside a panel.
    pub panel_hl: Color32,
    /// Top of the wooden panel header gradient.
    pub header_top: Color32,
    /// Bottom of the wooden panel header gradient.
    pub header_bot: Color32,
    /// Panel and window titles, on the wooden header (5.8:1 on header-top, 9.2:1 on header-bot).
    pub header_text: Color32,
    /// Recessed area inside a panel (lists, browser rows).
    pub inner: Color32,
    /// Alternate recessed fill; egui faint_bg.
    pub inner2: Color32,
    /// Zebra row inside a recessed list.
    pub inner_alt: Color32,
    /// Timeline ruler background.
    pub ruler: Color32,
    /// Ruler tick lines.
    pub ruler_line: Color32,
    /// Bar numbers on the ruler (8:1 on ruler).
    pub ruler_text: Color32,
    /// Beat lines in the playlist and piano roll.
    pub gridline: Color32,
    /// Menu separators and list dividers.
    pub divider: Color32,
    /// Unlit step pad, beats 1 and 3 of each group of four.
    pub step_off_a: Color32,
    /// Unlit step pad, alternate groups (warmer, helps count 16 steps).
    pub step_off_b: Color32,
    /// Channel palette 1; meter clip.
    pub red: Color32,
    /// Channel palette 3.
    pub yellow: Color32,
    /// Channel palette 4.
    pub cream_c: Color32,
    /// Record button and count-in digits. Only 4:1 on lcd-bg: used at 30px+.
    pub rec_red: Color32,
    /// Destructive actions and error text, on panel (5.1:1).
    pub danger: Color32,
    /// VU meter, lower 62%.
    pub meter_green: Color32,
    /// VU meter, upper part below clip.
    pub meter_yellow: Color32,
    /// VU meter track.
    pub meter_bg: Color32,
    /// Popup and tooltip fill (cool grey, stands off the warm panels).
    pub popup: Color32,
    /// Behind the piano keyboard.
    pub keyboard_bg: Color32,
    /// White piano key.
    pub key_white: Color32,
    /// Black piano key.
    pub key_black: Color32,
    /// Separator between white keys.
    pub key_line: Color32,
    /// Octave label on a white key (5:1 on key-white).
    pub key_label: Color32,
    /// Piano roll row of a white key.
    pub row_white: Color32,
    /// Piano roll row of a black key.
    pub row_black: Color32,
    /// Line between piano roll rows.
    pub row_line: Color32,
    /// Piano roll and playlist grid ground.
    pub grid_bg: Color32,
    /// Bottom of a pressed cream button (Retro: mix(cream-bot, ink, 0.08)).
    pub cream_press: Color32,
    /// Text on the grey active segment (6.4:1 on seg-on).
    pub white: Color32,
    /// On-state knob of a pill switch (on the orange track).
    pub switch_knob: Color32,
    /// Scroll bar handle (rack overview, egui scroll areas) and its outline.
    pub thumb: Color32,
    pub thumb_edge: Color32,
    /// Strokes of egui's own widgets: check marks, combo arrows, scroll bar handles.
    pub widget_fg: Color32,
    /// Channel palette 5. Literal in theme::PALETTE.
    pub teal: Color32,
    /// Channel palette 6. Literal in theme::PALETTE.
    pub terracotta: Color32,
    /// Hovered egui widget fill. Literal in theme::install.
    pub hover_tile: Color32,
    /// Hovered menu row. Literal in skin::menu_item.
    pub hover_row: Color32,
    /// Hovered dropdown trigger. Literal in skin::dropdown.
    pub hover_dropdown: Color32,
    /// Empty track of knobs, sliders and switches. Literal in skin.rs.
    pub track: Color32,
    /// Top of the metal slider handle gradient.
    pub metal_top: Color32,
    /// Bottom of the metal slider handle gradient.
    pub metal_bot: Color32,
    /// Bar lines in the playlist (stronger than gridline). Literal in playlist.rs.
    pub bar_line: Color32,
    /// Hovered channel tile in the rack.
    pub hover_channel: Color32,
}

pub const RETRO: Palette = Palette {
    screen: false,
    bg: hex(0x1e1a17),
    wood_dark: hex(0x4e2c16),
    wood_mid: hex(0x7a4a2a),
    wood: hex(0x5a341c),
    bar_top: hex(0x4a3f36),
    bar_bot: hex(0x342c26),
    bar_hl: hex(0x6b5d50),
    edge: hex(0x141110),
    groove: hex(0x191512),
    cream_top: hex(0xefe6d0),
    cream_bot: hex(0xcdbf9f),
    cream_hl: hex(0xfff8e6),
    ink: hex(0x2a211a),
    orange: hex(0xf28c28),
    orange_top: hex(0xff9a3a),
    orange_bot: hex(0xe2711a),
    orange_edge: hex(0x7a4200),
    orange_hl: hex(0xffd7a6),
    orange_light: hex(0xffc98a),
    on_orange: hex(0x1b1f22),
    lcd_bg: hex(0x0a1209),
    lcd_edge: hex(0x050805),
    lcd_green: hex(0x9cff6b),
    lcd_label: hex(0x5e8a4a),
    dark_tile: hex(0x1a1613),
    seg_bg: hex(0x141110),
    seg_on: hex(0x6b5d50),
    btn_dark: hex(0x4a4038),
    tile: hex(0x3a322b),
    tile_sel: hex(0x4f443a),
    text: hex(0xefe6d2),
    text_dim: hex(0xa89a85),
    text_dimmer: hex(0x968871),
    text_muted: hex(0xb3a58c),
    panel: hex(0x2f2a25),
    panel_hl: hex(0x4a4037),
    header_top: hex(0x80502e),
    header_bot: hex(0x5a341c),
    header_text: hex(0xf6ecd6),
    inner: hex(0x211c18),
    inner2: hex(0x26201b),
    inner_alt: hex(0x241f1b),
    ruler: hex(0x2a2420),
    ruler_line: hex(0x4a4037),
    ruler_text: hex(0xc9bb9f),
    gridline: hex(0x2e2823),
    divider: hex(0x2b2520),
    step_off_a: hex(0x463e36),
    step_off_b: hex(0x5a4230),
    red: hex(0xe8402a),
    yellow: hex(0xf2c230),
    cream_c: hex(0xefe3c2),
    rec_red: hex(0xd8321f),
    danger: hex(0xff6b6b),
    meter_green: hex(0x7fdc4a),
    meter_yellow: hex(0xe6c84a),
    meter_bg: hex(0x1b2125),
    popup: hex(0x353d44),
    keyboard_bg: hex(0x141719),
    key_white: hex(0xefe3c2),
    key_black: hex(0x1a1512),
    key_line: hex(0xbfae8a),
    key_label: hex(0x6b5d50),
    row_white: hex(0x332c26),
    row_black: hex(0x28221d),
    row_line: hex(0x241f1b),
    grid_bg: hex(0x2b2520),
    cream_press: hex(0xc0b294),
    white: hex(0xffffff),
    switch_knob: hex(0xffffff),
    // Color32::from_white_alpha(40) and (110), premultiplied (egui's gamma-aware values).
    thumb: Color32::from_rgba_premultiplied(110, 110, 110, 40),
    thumb_edge: Color32::from_rgba_premultiplied(175, 175, 175, 110),
    widget_fg: hex(0xefe6d2),
    teal: hex(0x8fd0c4),
    terracotta: hex(0xd98a6c),
    hover_tile: hex(0x5a4e43),
    hover_row: hex(0x433a32),
    hover_dropdown: hex(0x221d19),
    track: hex(0x161210),
    metal_top: hex(0xeef1f3),
    metal_bot: hex(0xb9c1c7),
    bar_line: hex(0x3a332c),
    hover_channel: hex(0x40372f),
};

pub const DISPLAY: Palette = Palette {
    screen: true,
    bg: hex(0x0a0d0c),
    wood_dark: hex(0x141616),
    wood_mid: hex(0x1f2223),
    wood: hex(0x191b1c),
    bar_top: hex(0x0d1110),
    bar_bot: hex(0x0d1110),
    bar_hl: hex(0x1a2220),
    edge: hex(0x25302b),
    groove: hex(0x1d2522),
    cream_top: hex(0x111614),
    cream_bot: hex(0x111614),
    cream_hl: hex(0x111614),
    ink: hex(0xffa63d),
    orange: hex(0xffa23a),
    orange_top: hex(0xffb04f),
    orange_bot: hex(0xff9526),
    orange_edge: hex(0xffc06a),
    orange_hl: hex(0xffd08a),
    orange_light: hex(0xffc98a),
    on_orange: hex(0x1a0f02),
    lcd_bg: hex(0x0a0d0c),
    lcd_edge: hex(0x0a0d0c),
    lcd_green: hex(0x7dff8c),
    lcd_label: hex(0x4fae5f),
    dark_tile: hex(0x101513),
    seg_bg: hex(0x0d1110),
    seg_on: hex(0x2b3530),
    btn_dark: hex(0x131917),
    tile: hex(0x101513),
    tile_sel: hex(0x1a231f),
    text: hex(0xffe6c2),
    text_dim: hex(0xa7ad9f),
    text_dimmer: hex(0x848c83),
    text_muted: hex(0xb3baae),
    panel: hex(0x0c100f),
    panel_hl: hex(0x1a2220),
    header_top: hex(0x0c100f),
    header_bot: hex(0x0c100f),
    header_text: hex(0x8dff9c),
    inner: hex(0x0e1312),
    inner2: hex(0x101514),
    inner_alt: hex(0x0c1110),
    ruler: hex(0x0e1312),
    ruler_line: hex(0x3d2c14),
    ruler_text: hex(0xffb760),
    gridline: hex(0x2a1f10),
    divider: hex(0x1c2522),
    step_off_a: hex(0x2a1d0c),
    step_off_b: hex(0x21170a),
    red: hex(0xff5a4a),
    yellow: hex(0xffe04a),
    cream_c: hex(0xffe9c4),
    rec_red: hex(0xff4d3d),
    danger: hex(0xff6b5a),
    meter_green: hex(0x7dff8c),
    meter_yellow: hex(0xffe04a),
    meter_bg: hex(0x101513),
    popup: hex(0x121816),
    keyboard_bg: hex(0x0a0d0c),
    key_white: hex(0x3a2810),
    key_black: hex(0x120d06),
    key_line: hex(0x0a0d0c),
    key_label: hex(0x7dff8c),
    row_white: hex(0x0e1211),
    row_black: hex(0x0b0e0d),
    row_line: hex(0x2a1f10),
    grid_bg: hex(0x0a0d0c),
    cream_press: hex(0x1d1609),
    white: hex(0xfff3dc),
    switch_knob: hex(0x1a0f02),
    // A faint amber tint, and with_alpha(#ffc06a, 230), premultiplied.
    thumb: Color32::from_rgba_premultiplied(40, 25, 8, 20),
    thumb_edge: Color32::from_rgba_premultiplied(244, 183, 101, 230),
    widget_fg: hex(0xffa63d),
    teal: hex(0x6dffc8),
    terracotta: hex(0xff8a62),
    hover_tile: hex(0x1a2220),
    hover_row: hex(0x1a2220),
    hover_dropdown: hex(0x151b19),
    track: hex(0x1c2320),
    metal_top: hex(0xffb04f),
    metal_bot: hex(0xff9526),
    bar_line: hex(0x4a3417),
    hover_channel: hex(0x1a2220),
};

/// Channel and pattern colors by id, for those without a color of their own.
pub fn channel_palette() -> [Color32; 6] {
    let p = pal();
    [p.red, p.orange, p.yellow, p.cream_c, p.teal, p.terracotta]
}

pub fn id_color(id: u32) -> Color32 {
    let palette = channel_palette();
    palette[id as usize % palette.len()]
}

/// Color chosen by the user, or one of the theme's from the id.
fn own_color(color: Option<Rgb>, id: u32) -> Color32 {
    color.map_or_else(|| id_color(id), |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Color a pattern is drawn with: its own, or one of the theme's from its id.
pub fn pattern_rgb(p: &Pattern) -> Color32 {
    own_color(p.color, p.id.0)
}

/// Color a channel is drawn with (card, dots, notes): its own, or one of the theme's from its id.
pub fn channel_rgb(c: &Channel) -> Color32 {
    own_color(c.color, c.id.0)
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

// --- Fonts ---

pub fn michroma(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("michroma".into()))
}
pub fn lcd(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("vt323".into()))
}
/// Display theme: every text face becomes the VT323 screen font, a size up so it reads
/// as large as the Barlow it replaces.
fn screen_face(size: f32) -> Option<FontId> {
    screen().then(|| FontId::new((size * 1.3).round(), FontFamily::Name("vt323".into())))
}
pub fn cond(size: f32) -> FontId {
    screen_face(size).unwrap_or_else(|| FontId::new(size, FontFamily::Name("cond".into())))
}
pub fn mono(size: f32) -> FontId {
    screen_face(size).unwrap_or_else(|| FontId::new(size, FontFamily::Monospace))
}
pub fn body(size: f32) -> FontId {
    screen_face(size).unwrap_or_else(|| FontId::new(size, FontFamily::Proportional))
}
pub fn medium(size: f32) -> FontId {
    screen_face(size).unwrap_or_else(|| FontId::new(size, FontFamily::Name("medium".into())))
}
pub fn bold(size: f32) -> FontId {
    screen_face(size).unwrap_or_else(|| FontId::new(size, FontFamily::Name("bold".into())))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fallback = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut add = |key: &str, bytes: &'static [u8], family: FontFamily| {
        fonts.font_data.insert(key.to_owned(), FontData::from_static(bytes));
        let list = fonts.families.entry(family).or_default();
        list.insert(0, key.to_owned());
    };
    add("barlow", include_bytes!("../assets/fonts/Barlow-SemiBold.ttf"), FontFamily::Proportional);
    add("jbmono", include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"), FontFamily::Monospace);
    add("michroma", include_bytes!("../assets/fonts/Michroma-Regular.ttf"), FontFamily::Name("michroma".into()));
    add("vt323", include_bytes!("../assets/fonts/VT323-Regular.ttf"), FontFamily::Name("vt323".into()));
    add("cond", include_bytes!("../assets/fonts/BarlowCondensed-Bold.ttf"), FontFamily::Name("cond".into()));
    add("medium", include_bytes!("../assets/fonts/Barlow-Medium.ttf"), FontFamily::Name("medium".into()));
    add("bold", include_bytes!("../assets/fonts/Barlow-Bold.ttf"), FontFamily::Name("bold".into()));
    // Fall back to egui fonts (symbols, emoji) for the named families.
    for fam in ["michroma", "vt323", "cond", "medium", "bold"] {
        if let Some(list) = fonts.families.get_mut(&FontFamily::Name(fam.into())) {
            list.extend(fallback.iter().cloned());
        }
    }
    ctx.set_fonts(fonts);
    apply(ctx);
}

/// Titles: Michroma in Retro, the larger VT323 pixel face in Display (`size` is the Michroma size).
pub fn title_font(size: f32) -> FontId {
    if screen() { lcd((size * 1.75).round()) } else { michroma(size) }
}

/// egui style of the current theme (call again after [`set`]).
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Small, body(12.0)),
        (egui::TextStyle::Body, body(15.0)),
        (egui::TextStyle::Button, body(15.0)),
        (egui::TextStyle::Heading, title_font(16.0)),
        (egui::TextStyle::Monospace, mono(14.0)),
    ]
    .into();
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(12.0, 7.0);
    style.spacing.interact_size = Vec2::new(40.0, 30.0);
    style.spacing.slider_width = 180.0;
    style.spacing.menu_margin = egui::Margin::same(6.0);

    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.override_text_color = Some(pal().text);
    v.panel_fill = pal().bg;
    v.window_fill = pal().panel;
    v.window_stroke = Stroke::new(1.0_f32, pal().edge);
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(10.0);
    v.extreme_bg_color = pal().dark_tile;
    v.faint_bg_color = pal().inner2;
    v.selection.bg_fill = pal().orange;
    v.selection.stroke = Stroke::new(1.0_f32, pal().on_orange);
    v.hyperlink_color = pal().orange;
    v.popup_shadow.color = Color32::from_black_alpha(140);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = pal().panel;
    w.noninteractive.weak_bg_fill = pal().panel;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, pal().edge);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, pal().text);
    for (s, bg) in [(&mut w.inactive, pal().btn_dark), (&mut w.hovered, pal().hover_tile), (&mut w.active, pal().seg_on), (&mut w.open, pal().tile_sel)] {
        s.bg_fill = bg;
        s.weak_bg_fill = bg;
        s.bg_stroke = Stroke::new(1.0_f32, pal().edge);
        s.fg_stroke = Stroke::new(1.5_f32, pal().widget_fg);
        s.rounding = Rounding::same(7.0);
    }
    w.hovered.bg_stroke = Stroke::new(1.0_f32, pal().orange);
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip() {
        for t in Theme::ALL {
            assert_eq!(Theme::from_code(t.code()), Some(t));
        }
        assert_eq!(Theme::from_code("neon"), None);
    }

    // Pure: the current theme is global and tests run in parallel, so they don't switch it.
    #[test]
    fn each_theme_has_its_palette() {
        assert!(palette(Theme::Display).screen);
        assert_eq!(palette(Theme::Display).bg, DISPLAY.bg);
        assert!(!palette(Theme::Retro).screen);
        assert_eq!(palette(Theme::Retro).bg, RETRO.bg);
        assert_ne!(RETRO.bg, DISPLAY.bg);
    }

    // Retro colors that used to be literals in skin.rs and rack.rs, so it looks the same.
    #[test]
    fn retro_keeps_its_former_literals() {
        assert_eq!(RETRO.cream_press, mix(RETRO.cream_bot, RETRO.ink, 0.08));
        assert_eq!(RETRO.thumb, Color32::from_white_alpha(40));
        assert_eq!(RETRO.thumb_edge, Color32::from_white_alpha(110));
        assert_eq!(RETRO.switch_knob, Color32::WHITE);
        assert_eq!(RETRO.widget_fg, RETRO.text);
        assert_eq!(DISPLAY.thumb_edge, with_alpha(hex(0xffc06a), 230));
    }
}
