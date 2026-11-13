//! "New version available" notice: small floating banner at the bottom right
//! (above the toast and the compact tabs), never blocking.

use crate::icons::Icon;
use crate::skin::{self, Kind};
use crate::theme::*;
use crate::{SeqUi, UiAction};
use eframe::egui::{self, Align2, Sense, Stroke, vec2};
use seq_i18n::{tr, trf};

/// Release found by the app's update check.
#[derive(Debug, Clone, PartialEq)]
pub struct UpdateNotice {
    /// `x.y.z`
    pub version: String,
    /// Release page (notes and downloads).
    pub url: String,
}

const H: f32 = 36.0;

impl SeqUi {
    pub(crate) fn update_notice(&mut self, ctx: &egui::Context) {
        let Some(n) = self.update.clone() else { return };
        // Clears the toast (bottom center) and, in full screen, the floating transport.
        let offset = if self.compact { vec2(-20.0, -118.0) } else { vec2(-30.0, -72.0) };
        let (mut view, mut skip, mut close) = (false, false, false);
        egui::Area::new(egui::Id::new("update_notice"))
            .order(egui::Order::Foreground)
            .anchor(Align2::RIGHT_BOTTOM, offset)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(POPUP)
                    .rounding(10.0)
                    .stroke(Stroke::new(1.0_f32, ORANGE))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .shadow(egui::Shadow { offset: vec2(0.0, 6.0), blur: 12.0, spread: 0.0, color: egui::Color32::from_black_alpha(110) })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (led, _) = ui.allocate_exact_size(vec2(12.0, H), Sense::hover());
                            skin::led(ui.painter(), led.center(), 4.0, ORANGE, true);
                            let text = trf!("Version {} disponible", "Version {} available", n.version);
                            ui.label(egui::RichText::new(text).font(body(15.0)).color(TEXT));
                            ui.add_space(4.0);
                            view = skin::auto_button(ui, H, Kind::Orange, tr!("Voir", "View")).on_hover_text(&n.url).clicked();
                            skip = skin::auto_button(ui, H, Kind::Cream, tr!("Ignorer cette version", "Skip this version")).clicked();
                            close = skin::icon_button(ui, vec2(H, H), Kind::Tile, Icon::Close, 14.0).on_hover_text(tr!("Fermer", "Close")).clicked();
                        });
                    });
            });
        if view {
            self.act(UiAction::OpenUrl(n.url));
        } else if skip {
            self.act(UiAction::SkipVersion(n.version));
        }
        if view || skip || close {
            self.update = None;
        }
    }
}
