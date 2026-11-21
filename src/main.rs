//! rustburn — native Rust laser design & control app (LightBurn-like) for GRBL diode
//! lasers such as the Elegoo Phecda. Linux-native, no license required.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// Scaffolding for upcoming phases keeps some helpers unused for now.
#![allow(dead_code)]

mod app;
mod device;
mod selftest;
mod gcode;
mod gcode_dither;
mod geometry;
mod geom_ops;
mod grbl_settings;
mod homography;
mod import;
mod materials;
mod model;
mod project;
mod shapes;
mod text;
mod trace;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,rustburn=info"))
        .init();

    // Headless self-test: build a sample document and dump G-code stats, no GUI.
    if std::env::args().any(|a| a == "--selftest") {
        selftest::run();
        return Ok(());
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1360.0, 860.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("rustburn — control láser (GRBL)"),
        ..Default::default()
    };

    eframe::run_native(
        "rustburn",
        native_options,
        Box::new(|cc| Ok(Box::new(app::RustburnApp::new(cc)))),
    )
}
