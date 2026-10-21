//! OpenCheck desktop application.

mod app;
mod area_edit;
mod bcf_live;
mod bcf_ui;
mod c402;
mod camera;
mod checking;
mod checking_ui;
mod export;
mod issue_dialog;
mod issues;
mod issues_ui;
mod jobs;
mod library;
mod measure;
mod panels;
mod pickers;
mod render;
mod report;
mod results_ui;
mod ruleset_manager;
mod scene;
mod session;
mod snap;
mod standard_editor;
mod theme;
mod viewport;

use std::path::PathBuf;

use eframe::egui;

fn main() -> eframe::Result {
    let files: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("OpenCheck")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([900.0, 560.0])
            .with_drag_and_drop(true),
        renderer: eframe::Renderer::Wgpu,
        depth_buffer: 32,
        multisampling: render::MSAA_SAMPLES as u16,
        ..Default::default()
    };

    eframe::run_native(
        "OpenCheck",
        options,
        Box::new(|cc| Ok(Box::new(app::OpenCheckApp::new(cc, files)))),
    )
}
