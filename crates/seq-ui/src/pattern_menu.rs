//! Pattern menu, the same from the timeline's pattern list (long press, right-click) and
//! from the channel rack (⋯ button next to the pattern selector): clone, edit (name and
//! color, see [`crate::edit_dialog`]), move up / down, delete (after confirmation). Each
//! action is one undo step.

use crate::SeqUi;
use crate::edit_dialog::{self, EditTarget, dialog_title, modal, ok_cancel};
use crate::skin::{self, Kind};
use crate::theme::*;
use eframe::egui::{self, Key, Rect, Sense, Ui, vec2};
use seq_i18n::{tr, trf};
use seq_model::*;

const MENU_W: f32 = 290.0;

/// Menu open on a pattern, under (or above) `anchor`.
pub(crate) struct PatternMenu {
    pattern: PatternId,
    anchor: Rect,
}

fn menu_id() -> egui::Id {
    egui::Id::new("pattern_menu")
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
                edit_dialog::open(st, project, EditTarget::Pattern(pat));
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

/// Delete confirmation, if open (call once a frame, after the panels).
pub(crate) fn delete_dialog(st: &mut SeqUi, ctx: &egui::Context, project: &mut Project) {
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
    use eframe::egui::pos2;

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
