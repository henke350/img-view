// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod edit;
mod folder;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let start_file = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("img-view")
            .with_inner_size([1024.0, 768.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "img-view",
        options,
        Box::new(move |_cc| Ok(Box::new(app::ViewerApp::new(start_file)))),
    )
}
