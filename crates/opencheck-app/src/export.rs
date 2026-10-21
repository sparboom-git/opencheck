//! Exports of issues: a BCF file and an illustrated report.
//!
//! Both need a picture per issue. The picture is what the 3D view shows for
//! the issue's viewpoint: each issue is shown in turn, the window is
//! captured a few frames later and the 3D view is cut out of the capture.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use glam::Vec3;
use opencheck_core::bcf_file::{self, FileTopic, FileViewpoint};
use opencheck_core::project;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::app::OpenCheckApp;
use crate::bcf_ui;
use crate::c402;
use crate::camera::Camera;
use crate::issues::new_guid;
use crate::scene::{ElementRef, Focus, SectionPlane};

#[derive(Debug, Clone, PartialEq)]
pub enum ExportKind {
    Bcf(PathBuf),
    Report(PathBuf),
    /// Pictures for new topics, then these issues are sent to the BCF server.
    Live(Vec<String>),
}

/// Frames to let the view settle after showing an issue (focus colours and
/// geometry uploads take effect on the next frames).
const SETTLE_FRAMES: u8 = 3;

enum Stage {
    Show,
    Settle(u8),
    /// Waiting for the window capture, counting frames.
    Requested(u16),
}

/// Widest picture stored, in pixels; larger views are scaled down.
const MAX_PICTURE_WIDTH: usize = 960;

/// Frames to wait for a capture before going on without a picture.
const CAPTURE_TIMEOUT_FRAMES: u16 = 120;

/// What the 3D view showed before the capture, restored afterwards.
struct SavedView {
    camera: Camera,
    focus: Option<Focus>,
    focus_marker: Option<Vec3>,
    selection: Vec<ElementRef>,
    section: Option<SectionPlane>,
    show_context: bool,
    isolated: Option<FxHashSet<ElementRef>>,
    hidden: FxHashSet<ElementRef>,
    issue: Option<String>,
    finding: Option<String>,
}

pub struct Capture {
    kind: ExportKind,
    issues: Vec<String>,
    next: usize,
    stage: Stage,
    images: FxHashMap<String, Vec<u8>>,
    saved: SavedView,
}

impl Capture {
    pub fn progress(&self) -> (usize, usize) {
        (self.next.min(self.issues.len()), self.issues.len())
    }
}

/// Cuts `rect` (points) out of a window capture and encodes it as PNG.
fn crop_png(image: &egui::ColorImage, rect: egui::Rect, pixels_per_point: f32) -> Option<Vec<u8>> {
    let [width, height] = image.size;
    let x0 = ((rect.min.x * pixels_per_point).round().max(0.0) as usize).min(width);
    let y0 = ((rect.min.y * pixels_per_point).round().max(0.0) as usize).min(height);
    let x1 = ((rect.max.x * pixels_per_point).round().max(0.0) as usize).min(width);
    let y1 = ((rect.max.y * pixels_per_point).round().max(0.0) as usize).min(height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    // Pictures are for orientation: shrink large views by averaging blocks.
    let step = (x1 - x0).div_ceil(MAX_PICTURE_WIDTH).max(1);
    let (w, h) = ((x1 - x0) / step, (y1 - y0) / step);
    if w == 0 || h == 0 {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for by in 0..h {
        for bx in 0..w {
            let mut sum = [0u32; 3];
            for y in y0 + by * step..y0 + (by + 1) * step {
                for p in &image.pixels[y * width + x0 + bx * step..y * width + x0 + (bx + 1) * step]
                {
                    let [r, g, b, _] = p.to_srgba_unmultiplied();
                    sum[0] += r as u32;
                    sum[1] += g as u32;
                    sum[2] += b as u32;
                }
            }
            let n = (step * step) as u32;
            rgba.extend_from_slice(&[
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                255,
            ]);
        }
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w as u32, h as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&rgba).ok()?;
    }
    Some(out)
}

impl OpenCheckApp {
    /// The issues an export covers: the ticked ones, or else all.
    pub fn export_scope(&self) -> Vec<String> {
        if self.issues.marked.is_empty() {
            self.issues.list.iter().map(|i| i.guid.clone()).collect()
        } else {
            // In list order, not tick order.
            self.issues
                .list
                .iter()
                .filter(|i| self.issues.marked.contains(&i.guid))
                .map(|i| i.guid.clone())
                .collect()
        }
    }

    pub fn is_capturing(&self) -> bool {
        self.capture.is_some()
    }

    /// Starts taking pictures of the issues; the export is written when done.
    pub fn start_export(&mut self, kind: ExportKind, ctx: &egui::Context) {
        let issues = self.export_scope();
        self.start_capture(kind, issues, ctx);
    }

    /// Takes pictures of `issues`, then finishes the export `kind`.
    pub fn start_capture(&mut self, kind: ExportKind, issues: Vec<String>, ctx: &egui::Context) {
        if issues.is_empty() || self.capture.is_some() {
            return;
        }
        let saved = SavedView {
            camera: self.camera,
            focus: self.view.focus.clone(),
            focus_marker: self.focus_marker,
            selection: self.view.selection.clone(),
            section: self.view.section,
            show_context: self.view.show_context,
            isolated: self.view.isolated.clone(),
            hidden: self.view.hidden.clone(),
            issue: self.issues.selected.clone(),
            finding: self.checking.selected.clone(),
        };
        self.capture = Some(Capture {
            kind,
            issues,
            next: 0,
            stage: Stage::Show,
            images: FxHashMap::default(),
            saved,
        });
        ctx.request_repaint();
    }

    /// One step of the capture; called every frame.
    pub fn step_capture(&mut self, ctx: &egui::Context) {
        let Some(capture) = &mut self.capture else {
            return;
        };
        ctx.request_repaint();
        match capture.stage {
            Stage::Show => {
                if capture.next >= capture.issues.len() {
                    self.finish_export(ctx);
                    return;
                }
                let guid = capture.issues[capture.next].clone();
                capture.stage = Stage::Settle(0);
                self.show_issue(&guid);
                // Without a stored camera, frame the issue's components.
                if self.issues.get(&guid).is_some_and(|i| i.camera.is_none())
                    && let Some(focus) = &self.view.focus
                {
                    let elements: Vec<ElementRef> =
                        focus.a.iter().chain(focus.b.iter()).copied().collect();
                    let bounds = self.view.bounds_of(&elements);
                    self.camera.fit(&bounds, self.nav.aspect);
                }
            }
            Stage::Settle(n) if n < SETTLE_FRAMES => capture.stage = Stage::Settle(n + 1),
            Stage::Settle(_) => {
                capture.stage = Stage::Requested(0);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            Stage::Requested(waited) => {
                let image: Option<Arc<egui::ColorImage>> = ctx.input(|i| {
                    i.raw.events.iter().find_map(|e| match e {
                        egui::Event::Screenshot { image, .. } => Some(image.clone()),
                        _ => None,
                    })
                });
                if let Some(image) = image {
                    let guid = capture.issues[capture.next].clone();
                    if let Some(png) = crop_png(&image, self.viewport_rect, ctx.pixels_per_point())
                    {
                        capture.images.insert(guid, png);
                    }
                    capture.next += 1;
                    capture.stage = Stage::Show;
                } else if waited >= CAPTURE_TIMEOUT_FRAMES {
                    // No picture for this issue; the export goes on without it.
                    capture.next += 1;
                    capture.stage = Stage::Show;
                } else {
                    capture.stage = Stage::Requested(waited + 1);
                }
            }
        }
    }

    fn finish_export(&mut self, ctx: &egui::Context) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        let saved = capture.saved;
        self.camera = saved.camera;
        self.view.focus = saved.focus;
        self.focus_marker = saved.focus_marker;
        self.view.selection = saved.selection;
        self.view.section = saved.section;
        self.view.show_context = saved.show_context;
        self.view.isolated = saved.isolated;
        self.view.hidden = saved.hidden;
        self.view.modes_dirty = true;
        self.issues.selected = saved.issue;
        self.checking.selected = saved.finding;

        let result = match &capture.kind {
            ExportKind::Live(guids) => {
                self.send_issues_with(guids, &capture.images, ctx);
                return;
            }
            ExportKind::Bcf(path) => self
                .bcf_file(&capture.issues, &capture.images)
                .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()))
                .map(|()| {
                    (
                        path.clone(),
                        format!("{} issues written as BCF", capture.issues.len()),
                    )
                }),
            ExportKind::Report(path) => {
                let html = crate::report::build(self, &capture.issues, &capture.images);
                std::fs::write(path, html)
                    .map_err(|e| e.to_string())
                    .map(|()| {
                        (
                            path.clone(),
                            format!("Report with {} issues written", capture.issues.len()),
                        )
                    })
            }
        };
        match result {
            Ok((path, message)) => {
                self.notify(format!(
                    "{message}: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                if matches!(capture.kind, ExportKind::Report(_)) {
                    let _ = webbrowser::open(&path.to_string_lossy());
                }
            }
            Err(e) => self.errors.push(format!("Could not export: {e}")),
        }
    }

    /// The issues as a BCF 2.1 archive, pictures included.
    fn bcf_file(
        &self,
        guids: &[String],
        images: &FxHashMap<String, Vec<u8>>,
    ) -> Result<Vec<u8>, String> {
        let link = self.issues.bcf.as_ref();
        let author = if self.issues.report.author.trim().is_empty() {
            "OpenCheck".to_owned()
        } else {
            self.issues.report.author.trim().to_owned()
        };
        let topics: Vec<FileTopic> = guids
            .iter()
            .filter_map(|guid| {
                let index = self.issues.list.iter().position(|i| i.guid == *guid)?;
                let issue = &self.issues.list[index];
                Some(FileTopic {
                    guid: issue.guid.clone(),
                    title: issue.title.clone(),
                    topic_type: c402::ISSUE_TYPE.to_owned(),
                    topic_status: bcf_ui::file_status(issue.status).to_owned(),
                    priority: c402::code(issue.priority).to_owned(),
                    index: Some(index as u32 + 1),
                    labels: issue
                        .responsible
                        .iter()
                        .filter_map(|d| c402::discipline_label(*d).map(str::to_owned))
                        .collect(),
                    creation_date: issue.created.clone(),
                    creation_author: author.clone(),
                    modified_date: issue.modified.clone(),
                    assigned_to: issue
                        .effective_assignee(link)
                        .unwrap_or_default()
                        .to_owned(),
                    description: bcf_ui::topic_description(issue),
                    viewpoint: bcf_ui::issue_viewpoint(issue, self.camera.fov_y).map(|view| {
                        FileViewpoint {
                            guid: new_guid(&format!("viewpoint {}", issue.guid)),
                            view,
                            snapshot: images.get(&issue.guid).cloned(),
                        }
                    }),
                })
            })
            .collect();
        let name = if self.issues.report.project.trim().is_empty() {
            self.session
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            self.issues.report.project.clone()
        };
        bcf_file::write_bcfzip(&name, &topics).map_err(|e| e.to_string())
    }

    /// Asks for a file name and exports the issues in scope as BCF.
    pub fn export_bcf_dialog(&mut self, ctx: &egui::Context) {
        let name = format!("{}-issues.bcfzip", self.export_base_name());
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export issues as BCF")
            .add_filter("BCF file", &["bcfzip", "bcf"])
            .set_file_name(name)
            .save_file()
        {
            self.start_export(ExportKind::Bcf(path), ctx);
        }
    }

    pub fn export_report_dialog(&mut self, ctx: &egui::Context) {
        let name = format!(
            "{}-K&K-rapport-{}.html",
            self.export_base_name(),
            &project::now_rfc3339()[..10]
        );
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Export issue report")
            .add_filter("Report (HTML)", &["html"])
            .set_file_name(name)
            .save_file()
        {
            self.start_export(ExportKind::Report(path), ctx);
        }
    }

    fn export_base_name(&self) -> String {
        let name = if self.issues.report.project.trim().is_empty() {
            self.session
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "OpenCheck".to_owned())
        } else {
            self.issues.report.project.trim().to_owned()
        };
        name.chars()
            .map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crops_the_viewport_out_of_a_capture() {
        let mut image = egui::ColorImage::filled([4, 4], egui::Color32::WHITE);
        image.pixels[5] = egui::Color32::RED; // (1, 1)
        let rect = egui::Rect::from_min_max(egui::pos2(0.5, 0.5), egui::pos2(1.5, 1.5));
        let png = crop_png(&image, rect, 2.0).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 2));
        assert!(crop_png(&image, egui::Rect::NOTHING, 1.0).is_none());

        // A wide view is scaled down to at most MAX_PICTURE_WIDTH.
        let wide = egui::ColorImage::filled([2000, 100], egui::Color32::WHITE);
        let all = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(2000.0, 100.0));
        let png = crop_png(&wide, all, 1.0).unwrap();
        let reader = png::Decoder::new(std::io::Cursor::new(png))
            .read_info()
            .unwrap();
        assert!(reader.info().width <= MAX_PICTURE_WIDTH as u32);
    }
}
