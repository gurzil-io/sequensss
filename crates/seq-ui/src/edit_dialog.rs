//! Edit dialog for a pattern or a channel: name and color (swatches, hue slider). OK applies
//! both at once (one undo step). Also the building blocks of the app's modal dialogs.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{SeqUi, UiAction};
use eframe::egui::{self, Align2, Color32, Key, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use seq_i18n::tr;
use seq_model::*;

/// Hues offered as swatches (the hue slider gives any other).
const SWATCHES: usize = 16;
const SWATCH_COLS: usize = 8;
const SWATCH: f32 = 40.0;

/// What is being edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    Pattern(PatternId),
    Channel(ChannelId),
}

/// Edit dialog in progress: changes are applied together on OK (one undo step).
pub(crate) struct EditDialog {
    target: EditTarget,
    name: String,
    /// `None` until a color is picked (without a color of its own, the theme's is kept).
    color: Option<Rgb>,
}

/// Opens the dialog on `target` with its current name and color (nothing if it is gone).
pub(crate) fn open(st: &mut SeqUi, project: &Project, target: EditTarget) {
    st.edit_dialog = match target {
        EditTarget::Pattern(id) => project.pattern(id).map(|p| (p.name.clone(), p.color)),
        EditTarget::Channel(id) => project.channel(id).map(|c| (c.name.clone(), c.color)),
    }
    .map(|(name, color)| EditDialog { target, name, color });
}

/// Name entered in the system's text dialog (Android) for what is being edited.
pub(crate) fn set_name(st: &mut SeqUi, name: String) {
    if let Some(e) = st.edit_dialog.as_mut()
        && !name.trim().is_empty()
    {
        e.name = name.trim().to_string();
    }
}

/// Dimmed backdrop and a centred box of `size`; returns whether the backdrop was tapped.
pub(crate) fn modal(ctx: &egui::Context, id: &str, size: egui::Vec2, content: impl FnOnce(&mut Ui, Rect)) -> bool {
    let screen = ctx.screen_rect();
    let mut outside = false;
    egui::Area::new(egui::Id::new((id, "backdrop"))).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (r, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
        ui.painter().rect_filled(r, 0.0, Color32::from_black_alpha(170));
        outside = resp.clicked();
    });
    let size = vec2(size.x.min(screen.width() - 16.0), size.y.min(screen.height() - 16.0));
    let rect = Rect::from_center_size(screen.center(), size);
    egui::Area::new(egui::Id::new(id)).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        // Drawn above the backdrop (both Foreground: the last shown is on top).
        ui.ctx().move_to_top(ui.layer_id());
        let (rect, _) = ui.allocate_exact_size(size, Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 12.0, pal().bg);
        p.rect_stroke(rect, 12.0, Stroke::new(1.0_f32, pal().edge));
        let mut inner = ui.new_child(UiBuilder::new().max_rect(rect.shrink(16.0)).layout(egui::Layout::top_down(egui::Align::Min)));
        content(&mut inner, rect.shrink(16.0));
    });
    outside
}

pub(crate) fn dialog_title(ui: &mut Ui, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    skin::gtext(ui.painter(), r.left_center(), Align2::LEFT_CENTER, text, title_font(14.0), pal().header_text);
}

/// OK / Cancel row at the bottom right; returns (ok, cancel).
pub(crate) fn ok_cancel(ui: &mut Ui, ok: &str, danger: bool) -> (bool, bool) {
    let mut res = (false, false);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let ok_resp = if danger {
            skin::button(ui, vec2(140.0, 44.0), Kind::Dark, false, |p, r, _| {
                p.rect_stroke(r.shrink(1.0), 8.0, Stroke::new(1.0_f32, pal().danger));
                skin::gtext(p, r.center(), Align2::CENTER_CENTER, ok, body(15.5), pal().danger);
            })
        } else {
            skin::text_button(ui, vec2(140.0, 44.0), Kind::Orange, ok, body(15.5))
        };
        res.0 = ok_resp.clicked();
        res.1 = skin::text_button(ui, vec2(120.0, 44.0), Kind::Cream, tr!("Annuler", "Cancel"), body(15.5)).clicked();
    });
    res
}

/// The dialog, if open (call once a frame, after the panels).
pub(crate) fn show(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    let Some(mut e) = st.edit_dialog.take() else { return };
    // Gone (undo, deletion elsewhere): the dialog closes.
    let (title, theme_color) = match e.target {
        EditTarget::Pattern(id) => match project.pattern(id) {
            Some(p) => (tr!("MODIFIER LE PATTERN", "EDIT PATTERN"), pattern_rgb(p)),
            None => return,
        },
        EditTarget::Channel(id) => match project.channel(id) {
            Some(c) => (tr!("MODIFIER LE CHANNEL", "EDIT CHANNEL"), channel_rgb(c)),
            None => return,
        },
    };
    let current = e.color.map_or(theme_color, |[r, g, b]| Color32::from_rgb(r, g, b));
    let native = st.native_text_input;
    let (mut ok, mut cancel) = (false, false);
    let w = SWATCH_COLS as f32 * (SWATCH + 6.0) - 6.0 + 32.0;
    let outside = modal(ctx, "edit_dialog", vec2(w, 345.0), |ui, rect| {
        ui.spacing_mut().item_spacing.y = 8.0;
        dialog_title(ui, title);

        // Name, with the chosen color in front.
        ui.horizontal(|ui| {
            let (sw, _) = ui.allocate_exact_size(vec2(44.0, 44.0), Sense::hover());
            ui.painter().rect_filled(sw.shrink(6.0), 6.0, current);
            let fw = ui.available_width();
            if native {
                // The soft keyboard cannot type into the app: the system's text dialog.
                let b = skin::button(ui, vec2(fw, 44.0), Kind::Dark, false, |p, r, c| {
                    p.with_clip_rect(r.shrink(4.0)).text(pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, &e.name, body(17.0), pal().text);
                    crate::icons::draw(p, Rect::from_center_size(pos2(r.right() - 22.0, r.center().y), vec2(18.0, 18.0)), 18.0, crate::icons::Icon::Pencil, c, 1.9);
                });
                if b.clicked() {
                    st.actions.push(UiAction::AskName(e.target, e.name.clone()));
                }
            } else {
                let r = ui.add_sized(vec2(fw, 44.0), egui::TextEdit::singleline(&mut e.name).font(body(17.0)).vertical_align(egui::Align::Center));
                ok |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            }
        });

        skin::caption(ui.painter(), ui.cursor().min + vec2(0.0, 8.0), Align2::LEFT_CENTER, tr!("COULEUR", "COLOR"), pal().text_dim);
        ui.add_space(14.0);
        // Swatches: hues around the wheel.
        let grid = Rect::from_min_size(ui.cursor().min, vec2(rect.width(), 2.0 * SWATCH + 6.0));
        ui.allocate_rect(grid, Sense::hover());
        for i in 0..SWATCHES {
            let c = pattern_color(i as f32 / SWATCHES as f32);
            let (col, row) = (i % SWATCH_COLS, i / SWATCH_COLS);
            let r = Rect::from_min_size(grid.min + vec2(col as f32 * (SWATCH + 6.0), row as f32 * (SWATCH + 6.0)), vec2(SWATCH, SWATCH));
            let resp = ui.interact(r, ui.id().with(("swatch", i)), Sense::click());
            let p = ui.painter();
            p.rect_filled(r.shrink(if resp.hovered() { 0.0 } else { 2.0 }), 7.0, Color32::from_rgb(c[0], c[1], c[2]));
            if e.color == Some(c) {
                p.rect_stroke(r.expand(1.0), 8.0, Stroke::new(2.5_f32, pal().text));
            }
            if resp.clicked() {
                e.color = Some(c);
            }
        }
        // Hue slider: any hue, at the same saturation and brightness.
        let (bar, resp) = ui.allocate_exact_size(vec2(rect.width(), 40.0), Sense::click_and_drag());
        let p = ui.painter();
        let slices = 48;
        let sw = bar.width() / slices as f32;
        for i in 0..slices {
            let [r, g, b] = pattern_color((i as f32 + 0.5) / slices as f32);
            let s = Rect::from_min_size(pos2(bar.left() + i as f32 * sw, bar.top() + 6.0), vec2(sw + 0.5, bar.height() - 12.0));
            p.rect_filled(s, 0.0, Color32::from_rgb(r, g, b));
        }
        p.rect_stroke(bar.shrink2(vec2(0.0, 6.0)), 4.0, Stroke::new(1.0_f32, pal().edge));
        if let Some(c) = e.color {
            let x = bar.left() + rgb_hue(c) * bar.width();
            let m = Rect::from_center_size(pos2(x, bar.center().y), vec2(10.0, bar.height()));
            p.rect_filled(m, 4.0, Color32::from_rgb(c[0], c[1], c[2]));
            p.rect_stroke(m, 4.0, Stroke::new(2.0_f32, pal().text));
        }
        if (resp.clicked() || resp.dragged())
            && let Some(pos) = resp.interact_pointer_pos()
        {
            e.color = Some(pattern_color(((pos.x - bar.left()) / bar.width()).clamp(0.0, 0.999)));
        }

        ui.add_space(4.0);
        let (o, c) = ok_cancel(ui, "OK", false);
        ok |= o;
        cancel |= c;
    });
    cancel |= ctx.input(|i| i.key_pressed(Key::Escape)) || outside;
    if ok {
        apply(project, &e);
    } else if !cancel {
        st.edit_dialog = Some(e);
    }
}

/// Applies the dialog's name and color (one undo step: the app commits once per frame).
fn apply(project: &mut Project, e: &EditDialog) {
    match e.target {
        EditTarget::Pattern(id) => {
            seq_edit::rename_pattern(project, id, &e.name);
            if let Some(c) = e.color {
                seq_edit::set_pattern_color(project, id, c);
            }
        }
        EditTarget::Channel(id) => {
            seq_edit::rename_channel(project, id, &e.name);
            if let Some(c) = e.color {
                seq_edit::set_channel_color(project, id, c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_edits_a_channel_or_a_pattern() {
        let mut p = Project::empty();
        let pat = p.patterns[0].id;
        let ch = p.add_channel("Kick", ChannelKind::Sampler { source: SampleSource::Builtin(Drum::Kick) }).unwrap();
        let mut st = SeqUi::new(&p);
        open(&mut st, &p, EditTarget::Channel(ch));
        set_name(&mut st, "  Bass drum ".into());
        set_name(&mut st, "   ".into());
        let e = st.edit_dialog.as_mut().unwrap();
        assert_eq!(e.name, "Bass drum");
        assert_eq!(e.color, None);
        e.color = Some([1, 2, 3]);
        apply(&mut p, st.edit_dialog.as_ref().unwrap());
        let c = p.channel(ch).unwrap();
        assert_eq!((c.name.as_str(), c.color), ("Bass drum", Some([1, 2, 3])));
        assert_eq!(channel_rgb(c), Color32::from_rgb(1, 2, 3));
        // The pattern is untouched; its dialog starts from its own name.
        assert_eq!(p.pattern(pat).unwrap().color, None);
        open(&mut st, &p, EditTarget::Pattern(pat));
        assert_eq!(st.edit_dialog.as_ref().unwrap().name, p.pattern(pat).unwrap().name);
        // Nothing to edit: no dialog.
        open(&mut st, &p, EditTarget::Channel(ChannelId(999)));
        assert!(st.edit_dialog.is_none());
    }
}
