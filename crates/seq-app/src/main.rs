#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Desktop binary.

use sequensss::eframe::{self, egui};
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    let path = std::env::args().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1600.0, 900.0])
            .with_min_inner_size([1180.0, 700.0])
            .with_maximized(true)
            .with_title("Sequensss")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icon/sequensss-256.png"))
                    .expect("PNG icon"),
            ),
        ..Default::default()
    };
    sequensss::run(options, path, sequensss::Files::desktop(), sys_locale::get_locale())
}
