//! OpenCheck application: a desktop program, and the same code compiled to
//! WebAssembly for the browser.

mod app;
mod area_edit;
#[cfg(not(target_arch = "wasm32"))]
mod autorun_ui;
mod bcf_live;
mod bcf_ui;
mod c402;
mod camera;
mod checking;
mod checking_ui;
#[cfg(not(target_arch = "wasm32"))]
mod claude_link;
mod dialog;
mod export;
mod issue_dialog;
mod issues;
mod issues_ui;
mod jobs;
mod library;
mod measure;
mod panels;
mod pickers;
mod platform;
mod render;
mod report;
mod results_ui;
mod ruleset_manager;
mod scene;
mod session;
mod snap;
mod standard_editor;
mod theme;
mod triage;
mod viewport;
mod visual_ui;

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
use eframe::egui;

#[cfg(not(target_arch = "wasm32"))]
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

/// In the browser the app draws on the page's `<canvas id="opencheck">`.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;

    // Errors and panics go to the browser console.
    let _ = eframe::WebLogger::init(log::LevelFilter::Warn);

    wasm_bindgen_futures::spawn_local(async {
        let canvas = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("opencheck"))
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok())
            .expect("the page has no <canvas id=\"opencheck\">");
        let options = eframe::WebOptions {
            renderer: eframe::Renderer::Wgpu,
            depth_buffer: 32,
            ..Default::default()
        };
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| Ok(Box::new(app::OpenCheckApp::new(cc, Vec::new())))),
            )
            .await;
        // The page's loading message goes, or tells why it could not start.
        let Some(loading) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("loading"))
        else {
            return;
        };
        match started {
            Ok(()) => loading.remove(),
            Err(e) => loading.set_inner_html(&format!(
                "OpenCheck could not start: {e:?}<br>It needs a browser with WebGPU,                  such as a recent Chrome or Edge."
            )),
        }
    });
}
