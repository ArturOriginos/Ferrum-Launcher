// In release builds on Windows we don't show a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod avatar;
mod config;
mod custom;
mod export;
mod import;
mod java;
mod launch;
mod loaders;
mod minecraft;
mod mods;
mod net;
mod perf;
mod skin;
mod terms;
mod theme;
mod util;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Ferrum Launcher")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([920.0, 580.0])
            .with_icon(eframe::egui::IconData {
                rgba: include_bytes!("../assets/icon_256.rgba").to_vec(),
                width: 256,
                height: 256,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "Ferrum Launcher",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
