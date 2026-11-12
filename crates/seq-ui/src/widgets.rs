use eframe::egui::{self, Color32, Stroke, Vec2, pos2};

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
