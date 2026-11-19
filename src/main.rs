// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod edit;
mod folder;
mod print;
mod selection;
mod theme;
mod view;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let start_file = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("img-view")
            .with_inner_size([1024.0, 768.0])
            .with_drag_and_drop(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        "img-view",
        options,
        Box::new(move |cc| {
            let has_icons = theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::ViewerApp::new(start_file, has_icons)))
        }),
    )
}

/// The eye icon for the title bar and taskbar. Without this eframe shows its own
/// default icon on the window, even though the .exe file carries the eye.
fn window_icon() -> egui::IconData {
    let rgba = image::load_from_memory(include_bytes!("../assets/eye.png"))
        .expect("the bundled icon is a valid PNG")
        .into_rgba8();
    let (width, height) = rgba.dimensions();
    egui::IconData { rgba: rgba.into_raw(), width, height }
}
