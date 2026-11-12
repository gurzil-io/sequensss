//! "Sequens Retro" theme: palette, fonts, egui visuals.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, Rounding, Stroke, Vec2};

pub const fn hex(c: u32) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

// Background & wood
pub const BG: Color32 = hex(0x1e1a17);
pub const WOOD_DARK: Color32 = hex(0x4e2c16);
pub const WOOD_MID: Color32 = hex(0x7a4a2a);
pub const WOOD: Color32 = hex(0x5a341c);
// Top bar
pub const BAR_TOP: Color32 = hex(0x4a3f36);
pub const BAR_BOT: Color32 = hex(0x342c26);
pub const BAR_HL: Color32 = hex(0x6b5d50);
pub const EDGE: Color32 = hex(0x141110);
pub const GROOVE: Color32 = hex(0x191512);
// Cream buttons
pub const CREAM_TOP: Color32 = hex(0xefe6d0);
pub const CREAM_BOT: Color32 = hex(0xcdbf9f);
pub const CREAM_HL: Color32 = hex(0xfff8e6);
pub const INK: Color32 = hex(0x2a211a);
// Orange
pub const ORANGE: Color32 = hex(0xf28c28);
pub const ORANGE_TOP: Color32 = hex(0xff9a3a);
pub const ORANGE_BOT: Color32 = hex(0xe2711a);
pub const ORANGE_EDGE: Color32 = hex(0x7a4200);
pub const ORANGE_HL: Color32 = hex(0xffd7a6);
pub const ORANGE_LIGHT: Color32 = hex(0xffc98a);
pub const ON_ORANGE: Color32 = hex(0x1b1f22);
// LCD
pub const LCD_BG: Color32 = hex(0x0a1209);
pub const LCD_EDGE: Color32 = hex(0x050805);
pub const LCD_GREEN: Color32 = hex(0x9cff6b);
pub const LCD_LABEL: Color32 = hex(0x5e8a4a);
// Tiles & segments
pub const DARK_TILE: Color32 = hex(0x1a1613);
pub const SEG_BG: Color32 = hex(0x141110);
pub const SEG_ON: Color32 = hex(0x6b5d50);
pub const BTN_DARK: Color32 = hex(0x4a4038);
pub const TILE: Color32 = hex(0x3a322b);
pub const TILE_SEL: Color32 = hex(0x4f443a);
// Text
pub const TEXT: Color32 = hex(0xefe6d2);
pub const TEXT_DIM: Color32 = hex(0xa89a85);
pub const TEXT_DIMMER: Color32 = hex(0x968871);
pub const TEXT_MUTED: Color32 = hex(0xb3a58c);
// Panels
pub const PANEL: Color32 = hex(0x2f2a25);
pub const PANEL_HL: Color32 = hex(0x4a4037);
pub const HEADER_TOP: Color32 = hex(0x80502e);
pub const HEADER_BOT: Color32 = hex(0x5a341c);
pub const HEADER_TEXT: Color32 = hex(0xf6ecd6);
pub const INNER: Color32 = hex(0x211c18);
pub const INNER2: Color32 = hex(0x26201b);
pub const INNER_ALT: Color32 = hex(0x241f1b);
pub const RULER: Color32 = hex(0x2a2420);
pub const RULER_LINE: Color32 = hex(0x4a4037);
pub const RULER_TEXT: Color32 = hex(0xc9bb9f);
pub const GRIDLINE: Color32 = hex(0x2e2823);
pub const DIVIDER: Color32 = hex(0x2b2520);
// Step seq steps
pub const STEP_OFF_A: Color32 = hex(0x463e36);
pub const STEP_OFF_B: Color32 = hex(0x5a4230);
// Accent colors
pub const RED: Color32 = hex(0xe8402a);
pub const YELLOW: Color32 = hex(0xf2c230);
pub const CREAM_C: Color32 = hex(0xefe3c2);
pub const REC_RED: Color32 = hex(0xd8321f);
pub const DANGER: Color32 = hex(0xff6b6b);
pub const METER_GREEN: Color32 = hex(0x7fdc4a);
pub const METER_YELLOW: Color32 = hex(0xe6c84a);
pub const METER_BG: Color32 = hex(0x1b2125);
pub const POPUP: Color32 = hex(0x353d44);
// Piano roll
pub const KEYBOARD_BG: Color32 = hex(0x141719);
pub const KEY_WHITE: Color32 = hex(0xefe3c2);
pub const KEY_BLACK: Color32 = hex(0x1a1512);
pub const KEY_LINE: Color32 = hex(0xbfae8a);
pub const KEY_LABEL: Color32 = hex(0x6b5d50);
pub const ROW_WHITE: Color32 = hex(0x332c26);
pub const ROW_BLACK: Color32 = hex(0x28221d);
pub const ROW_LINE: Color32 = hex(0x241f1b);
pub const GRID_BG: Color32 = hex(0x2b2520);

/// Channel and pattern colors (assigned by id).
pub const PALETTE: [Color32; 6] = [RED, ORANGE, YELLOW, CREAM_C, hex(0x8fd0c4), hex(0xd98a6c)];

pub fn id_color(id: u32) -> Color32 {
    PALETTE[id as usize % PALETTE.len()]
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
pub fn cond(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("cond".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}
pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("medium".into()))
}
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
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

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (egui::TextStyle::Small, body(12.0)),
        (egui::TextStyle::Body, body(15.0)),
        (egui::TextStyle::Button, body(15.0)),
        (egui::TextStyle::Heading, michroma(16.0)),
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
    v.override_text_color = Some(TEXT);
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, EDGE);
    v.window_rounding = Rounding::same(10.0);
    v.menu_rounding = Rounding::same(10.0);
    v.extreme_bg_color = DARK_TILE;
    v.faint_bg_color = INNER2;
    v.selection.bg_fill = ORANGE;
    v.selection.stroke = Stroke::new(1.0_f32, ON_ORANGE);
    v.hyperlink_color = ORANGE;
    v.popup_shadow.color = Color32::from_black_alpha(140);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, EDGE);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for (s, bg) in [(&mut w.inactive, BTN_DARK), (&mut w.hovered, hex(0x5a4e43)), (&mut w.active, SEG_ON), (&mut w.open, TILE_SEL)] {
        s.bg_fill = bg;
        s.weak_bg_fill = bg;
        s.bg_stroke = Stroke::new(1.0_f32, EDGE);
        s.fg_stroke = Stroke::new(1.5_f32, TEXT);
        s.rounding = Rounding::same(7.0);
    }
    w.hovered.bg_stroke = Stroke::new(1.0_f32, ORANGE);
    ctx.set_style(style);
}
