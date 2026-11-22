//! Pattern menu, the same from the timeline's pattern list (long press, right-click) and
//! from the channel rack (⋯ button next to the pattern selector): clone, edit (name and
//! color), move up / down, delete (after confirmation). Each action is one undo step.

use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{SeqUi, UiAction};
use eframe::egui::{self, Align2, Color32, Key, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use seq_i18n::{tr, trf};
use seq_model::*;

/// Hues offered as swatches in the edit dialog (the hue slider gives any other).
const SWATCHES: usize = 16;
const SWATCH_COLS: usize = 8;
const SWATCH: f32 = 40.0;
const MENU_W: f32 = 290.0;

/// Menu open on a pattern, under (or above) `anchor`.
pub(crate) struct PatternMenu {
    pattern: PatternId,
    anchor: Rect,
}

/// Edit dialog in progress: changes are applied together on OK (one undo step).
pub(crate) struct PatternEdit {
    pattern: PatternId,
    name: String,
    /// `None` until a color is picked (a pattern without a color of its own keeps the theme's).
    color: Option<Rgb>,
}

fn menu_id() -> egui::Id {
    egui::Id::new("pattern_menu")
}

/// Color a pattern is drawn with: its own, or one of the theme's from its id.
pub fn pattern_rgb(p: &Pattern) -> Color32 {
    p.color.map_or_else(|| id_color(p.id.0), |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// Opens the menu of `pattern` at `anchor` (screen rectangle of the row or button). Opening
/// it again from the same place closes it (the rack button toggles).
pub(crate) fn toggle(st: &mut SeqUi, ctx: &egui::Context, pattern: PatternId, anchor: Rect) {
    let open = ctx.memory(|m| m.is_popup_open(menu_id()));
    if open && st.pattern_menu.as_ref().is_some_and(|m| m.pattern == pattern && m.anchor == anchor) {
        ctx.memory_mut(|m| m.close_popup());
        st.pattern_menu = None;
    } else {
        st.pattern_menu = Some(PatternMenu { pattern, anchor });
        ctx.memory_mut(|m| m.open_popup(menu_id()));
    }
}

/// Opens the menu (long press, right-click): unlike [`toggle`], never closes it.
pub(crate) fn open(st: &mut SeqUi, ctx: &egui::Context, pattern: PatternId, anchor: Rect) {
    st.pattern_menu = Some(PatternMenu { pattern, anchor });
    ctx.memory_mut(|m| m.open_popup(menu_id()));
}

/// Whether the menu is open for `pattern` (its row or button stays highlighted).
pub(crate) fn is_open_for(st: &SeqUi, ctx: &egui::Context, pattern: PatternId) -> bool {
    st.pattern_menu.as_ref().is_some_and(|m| m.pattern == pattern) && ctx.memory(|m| m.is_popup_open(menu_id()))
}

/// Height of the menu: caption, 4 rows of 44 points, a separator, frame margins.
fn menu_height() -> f32 {
    28.0 + 4.0 * 44.0 + 9.0 + 20.0
}

/// Where a menu opens: below the anchor if it fits on screen, otherwise on the side with the
/// most room (egui then keeps it inside the screen). The anchor is narrowed so a wide one
/// (a long clip) does not stretch the menu.
pub(crate) fn menu_anchor(anchor: Rect, menu_h: f32, screen: Rect) -> (Rect, egui::AboveOrBelow) {
    let anchor = Rect::from_min_size(anchor.min, vec2(anchor.width().min(260.0), anchor.height()));
    let below = screen.bottom() - anchor.bottom();
    let above = anchor.top() - screen.top();
    let side = if below >= menu_h || below >= above { egui::AboveOrBelow::Below } else { egui::AboveOrBelow::Above };
    (anchor, side)
}

/// Label of the delete entry: what goes with the pattern. The last pattern stays, but its
/// clips can go; `None` when there is nothing to delete.
fn delete_label(last: bool, clips: usize) -> Option<String> {
    match (last, clips) {
        (false, 0) => Some(tr!("Supprimer le pattern…", "Delete pattern…").to_string()),
        (false, 1) => Some(tr!("Supprimer le pattern et son clip…", "Delete pattern and its clip…").to_string()),
        (false, n) => Some(trf!("Supprimer le pattern et ses {} clips…", "Delete pattern and its {} clips…", n)),
        (true, 0) => None,
        (true, 1) => Some(tr!("Supprimer son clip…", "Delete its clip…").to_string()),
        (true, n) => Some(trf!("Supprimer ses {} clips…", "Delete its {} clips…", n)),
    }
}

/// The menu, if open (call once a frame, after the panels).
pub(crate) fn show(st: &mut SeqUi, ui: &mut Ui, project: &mut Project) {
    let id = menu_id();
    let Some(m) = st.pattern_menu.as_ref() else { return };
    let (pat, anchor) = (m.pattern, m.anchor);
    let open = ui.memory(|m| m.is_popup_open(id));
    let Some(name) = project.pattern(pat).map(|p| p.name.clone()).filter(|_| open) else {
        // Closed (tap outside, another menu) or pattern gone (undo, deletion elsewhere).
        st.pattern_menu = None;
        if open {
            ui.memory_mut(|m| m.close_popup());
        }
        return;
    };
    let clips = project.arrangement.clips.iter().filter(|c| c.pattern == pat).count();
    let last = project.patterns.len() <= 1;
    let (anchor, side) = menu_anchor(anchor, menu_height(), ui.ctx().screen_rect());
    let anchor = ui.interact(anchor, id.with("anchor"), Sense::hover());
    let mut close = false;
    egui::popup_above_or_below_widget(ui, id, &anchor, side, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        // Small landscape phone: the menu scrolls rather than run off the screen.
        let max_h = ui.ctx().screen_rect().height() - 24.0;
        egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
            ui.set_min_width(MENU_W);
            skin::menu_caption(ui, &name.to_uppercase());
            if skin::menu_item(ui, tr!("Modifier (nom, couleur)…", "Edit (name, color)…"), pal().text, false).clicked() {
                st.pattern_edit = project.pattern(pat).map(|p| PatternEdit { pattern: pat, name: p.name.clone(), color: p.color });
                close = true;
            }
            if skin::menu_item(ui, tr!("Cloner", "Clone"), pal().text, false).clicked()
                && let Some(copy) = seq_edit::clone_pattern(project, pat)
            {
                st.pattern = copy;
                close = true;
            }
            // Move up and down share a row; the menu stays open to move again.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let w = (ui.available_width() - 4.0) / 2.0;
                for (up, label) in [(true, tr!("Monter", "Move up")), (false, tr!("Descendre", "Move down"))] {
                    let can = seq_edit::can_move_pattern(project, pat, up);
                    let b = ui.add_enabled_ui(can, |ui| skin::text_button(ui, vec2(w, 44.0), Kind::Dark, label, body(15.0))).inner;
                    if b.clicked() {
                        seq_edit::move_pattern(project, pat, up);
                    }
                }
            });
            skin::menu_separator(ui);
            match delete_label(last, clips) {
                Some(label) => {
                    if skin::menu_item(ui, &label, pal().danger, false).clicked() {
                        st.pattern_delete = Some(pat);
                        close = true;
                    }
                }
                None => skin::menu_disabled(ui, tr!("Dernier pattern, impossible à supprimer", "Last pattern, can't be deleted")),
            }
        });
    });
    if close {
        st.pattern_menu = None;
        ui.memory_mut(|m| m.close_popup());
    }
}

/// Edit and delete dialogs, if open (call once a frame, after the panels).
pub(crate) fn dialogs(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    edit_dialog(st, ctx, project);
    delete_dialog(st, ctx, project);
}

/// Name entered in the system's text dialog (Android) for the pattern being edited.
pub(crate) fn set_edit_name(st: &mut SeqUi, name: String) {
    if let Some(e) = st.pattern_edit.as_mut()
        && !name.trim().is_empty()
    {
        e.name = name.trim().to_string();
    }
}

/// Dimmed backdrop and a centred box of `size`; returns whether the backdrop was tapped.
fn modal(ctx: &egui::Context, id: &str, size: egui::Vec2, content: impl FnOnce(&mut Ui, Rect)) -> bool {
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

fn dialog_title(ui: &mut Ui, text: &str) {
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
    skin::gtext(ui.painter(), r.left_center(), Align2::LEFT_CENTER, text, title_font(14.0), pal().header_text);
}

/// OK / Cancel row at the bottom right; returns (ok, cancel).
fn ok_cancel(ui: &mut Ui, ok: &str, danger: bool) -> (bool, bool) {
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

fn edit_dialog(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    let Some(mut e) = st.pattern_edit.take() else { return };
    let Some(pat) = project.pattern(e.pattern) else { return };
    let current = e.color.map_or_else(|| pattern_rgb(pat), |[r, g, b]| Color32::from_rgb(r, g, b));
    let native = st.native_text_input;
    let (mut ok, mut cancel) = (false, false);
    let w = SWATCH_COLS as f32 * (SWATCH + 6.0) - 6.0 + 32.0;
    let outside = modal(ctx, "pattern_edit", vec2(w, 345.0), |ui, rect| {
        ui.spacing_mut().item_spacing.y = 8.0;
        dialog_title(ui, tr!("MODIFIER LE PATTERN", "EDIT PATTERN"));

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
                    st.actions.push(UiAction::AskPatternName(e.name.clone()));
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
        seq_edit::rename_pattern(project, e.pattern, &e.name);
        if let Some(c) = e.color {
            seq_edit::set_pattern_color(project, e.pattern, c);
        }
    } else if !cancel {
        st.pattern_edit = Some(e);
    }
}

fn delete_dialog(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
    let Some(pat) = st.pattern_delete else { return };
    let Some(name) = project.pattern(pat).map(|p| p.name.clone()) else {
        st.pattern_delete = None;
        return;
    };
    let clips = project.arrangement.clips.iter().filter(|c| c.pattern == pat).count();
    let last = project.patterns.len() <= 1;
    let (title, text, button) = if last {
        (
            tr!("SUPPRIMER LES CLIPS", "DELETE CLIPS"),
            trf!("Supprimer les {} clips de « {} » dans la timeline ? Le pattern reste (c'est le dernier).", "Delete the {} clips of “{}” in the timeline? The pattern stays (it is the last one).", clips, name),
            tr!("Supprimer les clips", "Delete clips"),
        )
    } else {
        let text = match clips {
            0 => trf!("Supprimer le pattern « {} » ?", "Delete pattern “{}”?", name),
            n => trf!("Supprimer le pattern « {} » et ses {} clips dans la timeline ?", "Delete pattern “{}” and its {} clips in the timeline?", name, n),
        };
        (tr!("SUPPRIMER LE PATTERN", "DELETE PATTERN"), text, tr!("Supprimer", "Delete"))
    };
    let (mut ok, mut cancel) = (false, false);
    let outside = modal(ctx, "pattern_delete", vec2(460.0, 190.0), |ui, rect| {
        dialog_title(ui, title);
        ui.add_space(6.0);
        ui.add(egui::Label::new(egui::RichText::new(text).font(body(16.0)).color(pal().text)).wrap());
        let used = ui.min_rect().height();
        ui.add_space((rect.height() - used - 48.0).max(8.0));
        (ok, cancel) = ok_cancel(ui, button, true);
    });
    cancel |= ctx.input(|i| i.key_pressed(Key::Escape)) || outside;
    if ok {
        if last {
            seq_edit::delete_pattern_clips(project, pat);
        } else {
            seq_edit::delete_pattern(project, pat);
        }
        st.playlist_deselect();
    }
    if ok || cancel {
        st.pattern_delete = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_stays_on_a_phone_screen() {
        // 2400 × 1080 px landscape phone at 2.625 px per point.
        let screen = Rect::from_min_size(egui::Pos2::ZERO, vec2(914.0, 411.0));
        let h = menu_height();
        assert!(h < screen.height());
        let side = |top: f32| menu_anchor(Rect::from_min_size(pos2(300.0, top), vec2(900.0, 40.0)), h, screen);
        // Room below: below. Row near the bottom: above, and fits.
        assert!(matches!(side(40.0).1, egui::AboveOrBelow::Below));
        let (r, s) = side(330.0);
        assert!(matches!(s, egui::AboveOrBelow::Above));
        assert!(r.top() - h >= screen.top());
        // A wide anchor does not stretch the menu.
        assert_eq!(r.width(), 260.0);
    }

    #[test]
    fn delete_entry_says_what_goes() {
        seq_i18n::with_lang(seq_i18n::Lang::En, || {
            assert_eq!(delete_label(false, 0).unwrap(), "Delete pattern…");
            assert_eq!(delete_label(false, 1).unwrap(), "Delete pattern and its clip…");
            assert_eq!(delete_label(false, 3).unwrap(), "Delete pattern and its 3 clips…");
            assert_eq!(delete_label(true, 2).unwrap(), "Delete its 2 clips…");
            // The last pattern without clips: nothing to delete.
            assert!(delete_label(true, 0).is_none());
        });
    }
}
