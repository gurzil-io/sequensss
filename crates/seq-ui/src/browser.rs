//! Projects kept in the app folder (Android, saved there by earlier versions), with
//! "Open a file…" for the system picker. List supplied by the app (`SeqUi::browser`),
//! actions returned via `UiAction::Browser*`.

use crate::icons::Icon;
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{SeqUi, UiAction};
use eframe::egui::{self, Align2, Rect, Sense, Stroke, UiBuilder, pos2, vec2};
use seq_i18n::tr;

/// State of the open browser.
#[derive(Default, Clone)]
pub struct Browser {
    /// Project names (without extension), sorted.
    pub projects: Vec<String>,
    /// The open project, if it is one of them.
    current: Option<String>,
    /// Project whose deletion awaits confirmation (second press).
    confirm_delete: Option<String>,
}

impl Browser {
    pub fn new(projects: Vec<String>, current: Option<String>) -> Self {
        Browser { projects, current, confirm_delete: None }
    }
}

const ROW_H: f32 = 52.0;

impl SeqUi {
    pub(crate) fn browser_window(&mut self, ctx: &egui::Context) {
        let Some(mut b) = self.browser.take() else { return };
        let screen = ctx.screen_rect();
        let size = vec2((screen.width() - 80.0).min(720.0), (screen.height() - 60.0).min(620.0));
        let mut open = true;
        let mut actions = Vec::new();
        egui::Window::new(tr!("PROJETS DANS L'APP", "PROJECTS IN THE APP"))
            .id(egui::Id::new("project_browser"))
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                if skin::auto_button(ui, 44.0, Kind::Orange, tr!("Ouvrir un fichier…", "Open a file…")).clicked() {
                    actions.push(UiAction::BrowserPickFile);
                }
                ui.add_space(6.0);
                let hint = tr!(
                    "Projets enregistrés dans l'app par les versions précédentes. « Enregistrer sous… » les range où vous voulez.",
                    "Projects saved inside the app by earlier versions. “Save as…” puts them where you like."
                );
                ui.label(egui::RichText::new(hint).font(body(14.0)).color(pal().text_dimmer));
                ui.add_space(8.0);
                if b.projects.is_empty() {
                    ui.label(egui::RichText::new(tr!("Aucun projet dans l'app.", "No project in the app.")).font(body(15.0)).color(pal().text_dimmer));
                }
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for name in b.projects.clone() {
                        let w = ui.available_width();
                        let (row, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
                        let is_current = b.current.as_deref() == Some(name.as_str());
                        let p = ui.painter();
                        p.rect_filled(row, 8.0, if resp.is_pointer_button_down_on() { pal().tile } else { pal().inner });
                        p.rect_stroke(row, 8.0, if is_current { Stroke::new(2.0_f32, pal().orange) } else { Stroke::new(1.0_f32, pal().edge) });
                        // Buttons on the right: share, delete.
                        let del = Rect::from_min_size(pos2(row.right() - 6.0 - 44.0, row.top() + 4.0), vec2(44.0, 44.0));
                        let confirming = b.confirm_delete.as_deref() == Some(name.as_str());
                        let del = if confirming { Rect::from_min_max(pos2(del.right() - 120.0, del.top()), del.max) } else { del };
                        let share = Rect::from_min_size(pos2(del.left() - 6.0 - 110.0, row.top() + 4.0), vec2(110.0, 44.0));
                        let text_r = Rect::from_min_max(pos2(row.left() + 14.0, row.top()), pos2(share.left() - 8.0, row.bottom()));
                        p.with_clip_rect(text_r).text(text_r.left_center(), Align2::LEFT_CENTER, &name, body(17.0), pal().text);

                        let mut su = ui.new_child(UiBuilder::new().max_rect(share));
                        let share_clicked = skin::text_button(&mut su, share.size(), Kind::Cream, tr!("Partager", "Share"), body(14.0)).clicked();
                        let mut du = ui.new_child(UiBuilder::new().max_rect(del));
                        let del_clicked = if confirming {
                            skin::text_button(&mut du, del.size(), Kind::Orange, tr!("Supprimer ?", "Delete?"), body(14.0)).clicked()
                        } else {
                            skin::icon_button(&mut du, del.size(), Kind::Tile, Icon::Trash, 18.0).clicked()
                        };

                        if share_clicked {
                            actions.push(UiAction::BrowserShare(name.clone()));
                        } else if del_clicked {
                            if confirming {
                                actions.push(UiAction::BrowserDelete(name.clone()));
                                b.confirm_delete = None;
                            } else {
                                b.confirm_delete = Some(name.clone());
                            }
                        } else if resp.clicked() {
                            actions.push(UiAction::BrowserOpen(name.clone()));
                        }
                    }
                });
            });
        let opened = actions.iter().any(|a| matches!(a, UiAction::BrowserOpen(_) | UiAction::BrowserPickFile));
        self.actions.extend(actions);
        if open && !opened {
            self.browser = Some(b);
        }
    }
}
