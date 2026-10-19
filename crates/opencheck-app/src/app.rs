//! Application state and top-level layout.

use std::path::{Path, PathBuf};

use eframe::egui::{self, KeyboardShortcut, Modifiers, RichText};
use eframe::egui_wgpu::RenderState;
use egui_phosphor::regular as icon;
use glam::Vec3;
use opencheck_core::check::CheckModel;
use opencheck_core::project::{
    self, ModelEntry, ModelSource, PROJECT_EXTENSION, ProjectDocuments, ProjectModel,
};
use opencheck_core::{Aabb, Federation, IfcModel, ModelId};

use crate::camera::Camera;
use crate::checking::{Checking, Decision};
use crate::issues::{
    ComponentRole, Issue, IssueCamera, IssueComponent, IssueSource, IssueStatus, Issues, new_guid,
};
use crate::jobs::{JobResult, Jobs, SaveRequest};
use crate::measure::MeasureTool;
use crate::render::SceneGpu;
use crate::ruleset_manager::{self, ManagerState};
use crate::scene::{ElementRef, SectionPlane, ViewState};
use crate::session::{self, ModelRecord, Session};
use crate::{checking_ui, panels, results_ui, theme, viewport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Model,
    Checking,
}

/// Navigation state carried between frames.
#[derive(Default)]
pub struct Navigation {
    /// Point the current orbit turns around.
    pub pivot: Option<Vec3>,
    /// Distance to the grabbed point while panning.
    pub pan_depth: Option<f32>,
    pub last_pointer: Option<egui::Pos2>,
    /// Aspect ratio of the last drawn viewport, used by keyboard commands.
    pub aspect: f32,
}

/// Something that would discard unsaved changes, waiting for the user's answer.
#[derive(Debug, Clone, PartialEq)]
pub enum PendingAction {
    NewProject,
    /// None: ask for the file.
    OpenProject(Option<PathBuf>),
    Close,
}

const SHORTCUT_NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::N);
const SHORTCUT_OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::O);
const SHORTCUT_SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::S);
const SHORTCUT_SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), egui::Key::S);

pub struct OpenCheckApp {
    pub federation: Federation,
    pub view: ViewState,
    pub camera: Camera,
    pub nav: Navigation,
    pub measure: MeasureTool,
    pub checking: Checking,
    pub manager: ManagerState,
    pub issues: Issues,
    /// Point of the finding or issue under review, for the marker in the 3D view.
    pub focus_marker: Option<Vec3>,
    pub tab: Tab,
    pub jobs: Jobs,
    pub session: Session,
    pub errors: Vec<String>,
    pub tree_filter: String,
    pub property_filter: String,
    render_state: Option<RenderState>,
    /// Bounds of all loaded geometry in the federation frame.
    pub scene_bounds: Aabb,
    confirm: Option<PendingAction>,
    after_save: Option<PendingAction>,
    allow_close: bool,
    window_title: String,
}

impl OpenCheckApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let render_state = cc.wgpu_render_state.clone();
        if let Some(rs) = &render_state {
            SceneGpu::install(rs);
        }
        let mut app = Self {
            federation: Federation::new(),
            view: ViewState::default(),
            camera: Camera::default(),
            nav: Navigation {
                aspect: 1.6,
                ..Default::default()
            },
            measure: MeasureTool::default(),
            checking: Checking::default(),
            manager: ManagerState::default(),
            issues: Issues::default(),
            focus_marker: None,
            tab: Tab::Model,
            jobs: Jobs::default(),
            session: Session::default(),
            errors: Vec::new(),
            tree_filter: String::new(),
            property_filter: String::new(),
            render_state,
            scene_bounds: Aabb::EMPTY,
            confirm: None,
            after_save: None,
            allow_close: false,
            window_title: String::new(),
        };
        for file in files {
            app.open_path(file, &cc.egui_ctx);
        }
        app
    }

    fn open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        match extension.as_str() {
            "ifc" => self.jobs.load_file(path, ctx),
            PROJECT_EXTENSION => self.request(PendingAction::OpenProject(Some(path)), ctx),
            _ => self.errors.push(format!(
                "{} is not an IFC file or OpenCheck project",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }

    pub fn open_file_dialog(&mut self, ctx: &egui::Context) {
        if let Some(paths) = rfd::FileDialog::new()
            .set_title("Add IFC models")
            .add_filter("IFC model", &["ifc", "IFC"])
            .pick_files()
        {
            for path in paths {
                self.jobs.load_file(path, ctx);
            }
        }
    }

    // ---- Project commands ----------------------------------------------

    /// Runs an action that could discard work, asking first when there are unsaved changes.
    pub fn request(&mut self, action: PendingAction, ctx: &egui::Context) {
        if self.session.dirty {
            self.confirm = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    fn perform(&mut self, action: PendingAction, ctx: &egui::Context) {
        match action {
            PendingAction::NewProject => self.reset(),
            PendingAction::OpenProject(path) => {
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .set_title("Open project")
                        .add_filter("OpenCheck project", &[PROJECT_EXTENSION])
                        .pick_file()
                });
                if let Some(path) = path {
                    self.reset();
                    self.jobs.open_project(path, ctx);
                }
            }
            PendingAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    /// Empties the session: no models, no project file.
    fn reset(&mut self) {
        self.jobs.new_session();
        for id in self.federation.ids() {
            if let Some(rs) = &self.render_state {
                SceneGpu::remove_model(rs, id);
            }
        }
        self.federation = Federation::new();
        self.view = ViewState::default();
        self.view.modes_dirty = true;
        self.camera = Camera::default();
        self.measure = MeasureTool::default();
        self.session = Session::default();
        self.checking = Checking::default();
        self.issues = Issues::default();
        self.focus_marker = None;
        self.scene_bounds = Aabb::EMPTY;
    }

    /// Saves to the current file, or asks for one. `then` runs after a successful save.
    pub fn save(&mut self, then: Option<PendingAction>, ctx: &egui::Context) {
        match self.session.path.clone() {
            Some(path) => self.save_to(path, then, ctx),
            None => self.save_as(then, ctx),
        }
    }

    pub fn save_as(&mut self, then: Option<PendingAction>, ctx: &egui::Context) {
        let name = self
            .session
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("Project.{PROJECT_EXTENSION}"));
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save project")
            .add_filter("OpenCheck project", &[PROJECT_EXTENSION])
            .set_file_name(name)
            .save_file()
        {
            self.save_to(with_extension(path), then, ctx);
        }
    }

    fn save_to(&mut self, path: PathBuf, then: Option<PendingAction>, ctx: &egui::Context) {
        if self.jobs.is_busy() {
            self.errors
                .push("Wait until loading has finished before saving.".to_owned());
            return;
        }
        let ids = self.federation.ids();
        let models: Vec<ProjectModel> = ids
            .iter()
            .filter_map(|id| {
                let model = self.federation.get(*id)?;
                let record = self.session.records.get(id)?;
                Some(ProjectModel {
                    entry: ModelEntry {
                        name: model.short_name.clone(),
                        discipline: model.discipline,
                        visible: self.view.model(*id).is_none_or(|m| m.visible),
                        ..record.entry.clone()
                    },
                    source: record.source.clone(),
                })
            })
            .collect();
        let documents = ProjectDocuments {
            view: Some(session::capture_view(
                &self.federation,
                &self.view,
                &self.camera,
                &self.measure.pinned,
            )),
            checking: Some(self.checking.to_json(&ids)),
            issues: Some(self.issues.to_json()),
        };
        self.after_save = then;
        self.jobs.save_project(
            SaveRequest {
                path,
                ids,
                models,
                rulesets: self.checking.ruleset_files(),
                documents,
                created: self.session.created.clone(),
            },
            ctx,
        );
    }

    // ---- Background results ---------------------------------------------

    fn receive_jobs(&mut self, ctx: &egui::Context) {
        for result in self.jobs.poll() {
            match result {
                JobResult::ModelLoaded {
                    source,
                    entry,
                    result,
                } => match result {
                    Ok(model) => self.add_model(*model, source, entry),
                    Err(message) => self.errors.push(format!(
                        "Could not open {}: {message}",
                        source_label(&source)
                    )),
                },
                JobResult::ProjectOpened { path, result } => match result {
                    Ok(contents) => {
                        self.session.path = Some(path);
                        self.session.created = Some(contents.manifest.created);
                        let errors = self
                            .checking
                            .load(contents.rulesets, contents.documents.checking);
                        self.errors.extend(errors);
                        self.issues = Issues::load(contents.documents.issues);
                        self.session.pending_view = contents.documents.view;
                        self.session.dirty = false;
                    }
                    Err(message) => self.errors.push(format!(
                        "Could not open {}: {message}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                },
                JobResult::ProjectModelsDone => {
                    self.checking.apply_pending(&self.federation.ids());
                    self.checking.stale = false;
                    if let Some(view) = self.session.pending_view.take() {
                        session::apply_view(
                            view,
                            &self.federation,
                            &mut self.view,
                            &mut self.camera,
                            &mut self.measure.pinned,
                        );
                    }
                }
                JobResult::Saved { path, ids, result } => match result {
                    Ok(manifest) => {
                        for (id, entry) in ids.iter().zip(manifest.models) {
                            if let Some(record) = self.session.records.get_mut(id) {
                                record.source = ModelSource::Project {
                                    path: path.clone(),
                                    entry: entry.file.clone(),
                                };
                                record.entry = entry;
                            }
                        }
                        self.session.path = Some(path);
                        self.session.created = Some(manifest.created);
                        self.session.dirty = false;
                        if let Some(next) = self.after_save.take() {
                            self.perform(next, ctx);
                        }
                    }
                    Err(message) => {
                        self.after_save = None;
                        self.errors.push(format!("Could not save: {message}"));
                    }
                },
                JobResult::CheckDone { report } => {
                    self.checking.finish_run(*report);
                    match self.checking.selected.clone() {
                        Some(id) if self.tab == Tab::Checking => self.show_finding(&id, false),
                        _ => self.clear_finding_focus(),
                    }
                }
            }
        }
    }

    fn add_model(&mut self, mut model: IfcModel, source: ModelSource, entry: Option<ModelEntry>) {
        let entry = match entry {
            Some(entry) => {
                model.short_name = entry.name.clone();
                model.discipline = entry.discipline;
                entry
            }
            None => {
                self.session.dirty = true;
                let now = project::now_rfc3339();
                ModelEntry {
                    file: String::new(),
                    name: model.short_name.clone(),
                    discipline: model.discipline,
                    visible: true,
                    source_path: match &source {
                        ModelSource::File(p) => Some(p.display().to_string()),
                        ModelSource::Project { .. } => None,
                    },
                    sha256: model.content_hash.clone(),
                    revision: 1,
                    added: now.clone(),
                    updated: now,
                }
            }
        };
        let visible = entry.visible;
        let first = self.federation.is_empty();
        self.checking.stale = true;
        let id = self.federation.add(model);
        let transform = self
            .federation
            .model_transform(id)
            .expect("model was just added");
        let model = self.federation.get(id).expect("model was just added");
        let chunks = self.view.add_model(id, model, transform);
        if let Some(rs) = &self.render_state {
            SceneGpu::upload_model(rs, id, &chunks);
        }
        self.session
            .records
            .insert(id, ModelRecord { source, entry });
        if !visible {
            self.set_model_visible(id, false);
        }
        self.scene_bounds = self.federation.bounds();
        if first {
            self.camera.home(&self.scene_bounds, self.nav.aspect);
        }
    }

    pub fn remove_model(&mut self, id: ModelId) {
        self.federation.remove(id);
        self.measure.edges.forget_model(id);
        self.checking.forget_model(id);
        if self
            .view
            .focus
            .is_some_and(|(a, b)| a.model == id || b.model == id)
        {
            self.clear_finding_focus();
        }
        self.view.remove_model(id);
        self.session.records.remove(&id);
        self.session.dirty = true;
        if let Some(rs) = &self.render_state {
            SceneGpu::remove_model(rs, id);
        }
        self.scene_bounds = self.federation.bounds();
    }

    pub fn set_model_visible(&mut self, id: ModelId, visible: bool) {
        if let Some(m) = self.view.model_mut(id) {
            m.visible = visible;
        }
        if let Some(rs) = &self.render_state {
            SceneGpu::set_model_visible(rs, id, visible);
        }
    }

    // ---- View commands, shared by toolbar, keys and menus -----------------

    pub fn select(&mut self, element: Option<ElementRef>, additive: bool) {
        match (element, additive) {
            (Some(e), true) => {
                if let Some(pos) = self.view.selection.iter().position(|s| *s == e) {
                    self.view.selection.remove(pos);
                } else {
                    self.view.selection.push(e);
                }
            }
            (Some(e), false) => self.view.selection = vec![e],
            (None, true) => {}
            (None, false) => self.view.selection.clear(),
        }
        self.view.modes_dirty = true;
    }

    fn visible_or_scene_bounds(&self) -> Aabb {
        let bounds = self.view.visible_bounds();
        if bounds.is_empty() {
            self.scene_bounds
        } else {
            bounds
        }
    }

    pub fn fit_all(&mut self) {
        let bounds = self.visible_or_scene_bounds();
        self.camera.fit(&bounds, self.nav.aspect);
    }

    pub fn fit_selection(&mut self) {
        let bounds = self.view.bounds_of(&self.view.selection);
        if bounds.is_empty() {
            self.fit_all();
        } else {
            self.camera.fit(&bounds, self.nav.aspect);
        }
    }

    pub fn fit_element(&mut self, element: ElementRef) {
        let bounds = self.view.bounds_of(&[element]);
        if !bounds.is_empty() {
            self.camera.fit(&bounds, self.nav.aspect);
        }
    }

    pub fn hide_selection(&mut self) {
        let selected = std::mem::take(&mut self.view.selection);
        self.view.hidden.extend(selected);
        self.view.modes_dirty = true;
    }

    pub fn isolate_selection(&mut self) {
        if self.view.selection.is_empty() {
            return;
        }
        self.view.isolated = Some(self.view.selection.iter().copied().collect());
        self.view.modes_dirty = true;
    }

    pub fn show_all(&mut self) {
        self.view.hidden.clear();
        self.view.isolated = None;
        self.view.storey_filter = None;
        self.view.modes_dirty = true;
    }

    pub fn toggle_xray(&mut self) {
        self.view.xray = !self.view.xray;
        self.view.modes_dirty = true;
    }

    /// Cuts the model just above the top of the selection, or through the
    /// middle of the view when nothing is selected.
    pub fn section_at_selection(&mut self) {
        let selected = self.view.bounds_of(&self.view.selection);
        let (height, anchor) = if selected.is_empty() {
            let bounds = self.visible_or_scene_bounds();
            if bounds.is_empty() {
                return;
            }
            (bounds.center().z, bounds.center())
        } else {
            (selected.max.z + 0.005, selected.center())
        };
        self.view.section = Some(SectionPlane { height, anchor });
    }

    pub fn toggle_section(&mut self) {
        if self.view.section.is_some() {
            self.view.section = None;
        } else {
            self.section_at_selection();
        }
    }

    // ---- Checking -------------------------------------------------------------

    pub fn switch_tab(&mut self, tab: Tab) {
        if tab == self.tab {
            return;
        }
        self.tab = tab;
        match (tab, self.checking.selected.clone()) {
            (Tab::Checking, Some(id)) => self.show_finding(&id, false),
            _ => self.clear_finding_focus(),
        }
    }

    /// Runs the enabled rulesets, or only the one with id `only`.
    pub fn run_check(&mut self, ctx: &egui::Context, only: Option<&str>) {
        if self.checking.is_running() || self.jobs.is_busy() {
            return;
        }
        let ids = self.federation.ids();
        let jobs: Vec<_> = match only {
            // "Run only this" also runs a ruleset that is switched off.
            Some(id) => self
                .checking
                .rulesets
                .iter()
                .filter(|r| r.ruleset.ruleset.id == id)
                .map(|r| opencheck_core::check::CheckJob {
                    ruleset: r.ruleset.clone(),
                    models: r.models.clone().unwrap_or_else(|| ids.clone()),
                })
                .collect(),
            None => self.checking.jobs(&ids),
        };
        if jobs.is_empty() {
            return;
        }
        let models: Vec<CheckModel> = ids
            .iter()
            .filter_map(|id| {
                Some(CheckModel {
                    id: *id,
                    model: self.federation.shared(*id)?,
                    transform: self.federation.model_transform(*id)?,
                })
            })
            .collect();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        self.checking.progress = Some(progress.clone());
        self.jobs.run_check(models, jobs, progress, ctx);
    }

    /// Selects a finding and frames it in the 3D view.
    pub fn focus_finding(&mut self, id: &str) {
        self.show_finding(id, true);
    }

    pub fn show_finding(&mut self, id: &str, move_camera: bool) {
        let Some((_, _, finding)) = self.checking.find(id) else {
            self.clear_finding_focus();
            return;
        };
        let a = ElementRef {
            model: finding.a.model,
            express_id: finding.a.express_id,
        };
        let b = ElementRef {
            model: finding.b.model,
            express_id: finding.b.express_id,
        };
        let point = finding.point;
        let frame = finding_frame(&finding.region);
        self.checking.selected = Some(id.to_owned());
        self.issues.selected = None;
        self.view.focus = Some((a, b));
        self.focus_marker = Some(point);
        self.view.selection.clear();
        self.view.modes_dirty = true;
        if move_camera {
            self.camera.fit(&frame, self.nav.aspect);
        }
    }

    pub fn clear_finding_focus(&mut self) {
        if self.view.focus.take().is_some() {
            self.view.modes_dirty = true;
        }
        self.focus_marker = None;
        if self.tab == Tab::Checking {
            self.checking.selected = None;
            self.issues.selected = None;
        }
    }

    /// The point of the finding or issue under review, for the marker in the 3D view.
    pub fn focus_point(&self) -> Option<Vec3> {
        self.view.focus?;
        self.focus_marker
    }

    /// Turns findings into selected issues, one issue per finding, and marks
    /// them accepted: an issue is a problem someone has to handle.
    pub fn add_issues(&mut self, finding_ids: &[String]) {
        let origin = self.federation.origin();
        let model_name = |id| {
            self.federation
                .get(id)
                .map(|m| m.short_name.clone())
                .unwrap_or_default()
        };
        let mut added = Vec::new();
        for id in finding_ids {
            if self.issues.for_finding(id).is_some() {
                continue;
            }
            let Some((ruleset, rule, f)) = self.checking.find(id) else {
                continue;
            };
            // The viewpoint: the same framing the finding gets when selected.
            let mut camera = self.camera;
            camera.fit(&finding_frame(&f.region), self.nav.aspect);
            let component = |e: &opencheck_core::check::FindingElement, role| IssueComponent {
                global_id: e.global_id.clone(),
                name: e.name.clone(),
                ifc_class: e.ifc_class.clone(),
                model: model_name(e.model),
                role,
            };
            let now = project::now_rfc3339();
            let label = |e: &opencheck_core::check::FindingElement| {
                if e.name.is_empty() {
                    e.ifc_class.clone()
                } else {
                    e.name.clone()
                }
            };
            added.push(Issue {
                guid: new_guid(id),
                title: format!("{} vs {}", label(&f.a), label(&f.b)),
                description: String::new(),
                status: IssueStatus::Open,
                priority: rule.rule.severity,
                created: now.clone(),
                modified: now,
                labels: vec![f.kind.label().to_owned()],
                source: Some(IssueSource {
                    finding: f.id.clone(),
                    ruleset: ruleset.info.name.clone(),
                    rule: rule.rule.name.clone(),
                    kind: f.kind.label().to_owned(),
                    magnitude_mm: f.magnitude * 1000.0,
                }),
                components: vec![
                    component(&f.a, ComponentRole::A),
                    component(&f.b, ComponentRole::B),
                ],
                point: (origin + f.point.as_dvec3()).to_array(),
                camera: Some(IssueCamera {
                    eye: (origin + camera.eye.as_dvec3()).to_array(),
                    yaw: camera.yaw,
                    pitch: camera.pitch,
                }),
            });
        }
        if added.is_empty() {
            return;
        }
        for issue in &added {
            if let Some(source) = &issue.source {
                self.checking
                    .decide(&source.finding, Some(Decision::Accepted));
            }
        }
        self.issues.list.extend(added);
        self.session.dirty = true;
    }

    /// Restores an issue's viewpoint and shows its components.
    pub fn show_issue(&mut self, guid: &str) {
        let Some(issue) = self.issues.get(guid).cloned() else {
            return;
        };
        let origin = self.federation.origin();
        let resolve = |c: &IssueComponent| -> Option<ElementRef> {
            // GlobalIds are unique per project; prefer the model with the same name.
            let mut fallback = None;
            for (id, model) in self.federation.iter() {
                if let Some(e) = model.element_by_global_id(&c.global_id) {
                    let found = ElementRef {
                        model: id,
                        express_id: e.express_id,
                    };
                    if model.short_name == c.model {
                        return Some(found);
                    }
                    fallback.get_or_insert(found);
                }
            }
            fallback
        };
        let a = issue
            .components
            .iter()
            .find(|c| c.role == ComponentRole::A)
            .and_then(resolve);
        let b = issue
            .components
            .iter()
            .find(|c| c.role == ComponentRole::B)
            .and_then(resolve);
        self.issues.selected = Some(guid.to_owned());
        self.checking.selected = issue.source.as_ref().map(|s| s.finding.clone());
        self.view.focus = match (a, b) {
            (Some(a), Some(b)) => Some((a, b)),
            (Some(x), None) | (None, Some(x)) => Some((x, x)),
            (None, None) => None,
        };
        self.focus_marker = Some((glam::DVec3::from_array(issue.point) - origin).as_vec3());
        self.view.selection.clear();
        self.view.modes_dirty = true;
        if let Some(c) = issue.camera {
            self.camera.eye = (glam::DVec3::from_array(c.eye) - origin).as_vec3();
            self.camera.yaw = c.yaw;
            self.camera.pitch = c.pitch;
        }
    }

    pub fn is_busy(&self) -> bool {
        self.jobs.is_busy()
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for path in dropped {
            self.open_path(path, ctx);
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Save As first: it is Save plus Shift.
        if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_SAVE_AS)) {
            self.save_as(None, ctx);
        } else if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_SAVE)) {
            self.save(None, ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_NEW)) {
            self.request(PendingAction::NewProject, ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT_OPEN)) {
            self.request(PendingAction::OpenProject(None), ctx);
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.session.dirty && !self.allow_close
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(PendingAction::Close);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = self.session.title();
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn sync_gpu(&mut self) {
        if self.view.modes_dirty {
            if let Some(rs) = &self.render_state {
                SceneGpu::write_modes(rs, &self.view.build_modes());
            }
            self.view.modes_dirty = false;
        }
    }

    // ---- Layout -----------------------------------------------------------

    fn file_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let item =
            |ui: &mut egui::Ui, glyph: &str, label: &str, shortcut: Option<&KeyboardShortcut>| {
                let mut button = egui::Button::new(format!("{glyph}  {label}"));
                if let Some(s) = shortcut {
                    button = button.shortcut_text(ui.ctx().format_shortcut(s));
                }
                ui.add(button).clicked()
            };
        ui.menu_button(RichText::new("File").size(14.5), |ui| {
            ui.set_min_width(240.0);
            if item(ui, icon::FILE_PLUS, "New project", Some(&SHORTCUT_NEW)) {
                self.request(PendingAction::NewProject, &ctx);
                ui.close();
            }
            if item(ui, icon::FOLDER_OPEN, "Open project…", Some(&SHORTCUT_OPEN)) {
                self.request(PendingAction::OpenProject(None), &ctx);
                ui.close();
            }
            ui.separator();
            if item(ui, icon::FLOPPY_DISK, "Save", Some(&SHORTCUT_SAVE)) {
                self.save(None, &ctx);
                ui.close();
            }
            if item(
                ui,
                icon::FLOPPY_DISK_BACK,
                "Save as…",
                Some(&SHORTCUT_SAVE_AS),
            ) {
                self.save_as(None, &ctx);
                ui.close();
            }
            ui.separator();
            if item(ui, icon::PLUS, "Add IFC models…", None) {
                self.open_file_dialog(&ctx);
                ui.close();
            }
            ui.separator();
            if item(ui, icon::SIGN_OUT, "Exit", None) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                ui.close();
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} OpenCheck", icon::CUBE_FOCUS))
                            .size(17.0)
                            .strong()
                            .color(theme::ACCENT),
                    );
                    ui.add_space(12.0);
                    self.file_menu(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(12.0);
                    for (tab, label) in [(Tab::Model, "Model"), (Tab::Checking, "Checking")] {
                        let selected = self.tab == tab;
                        let text = RichText::new(label).size(14.5);
                        let text = if selected {
                            text.strong().color(theme::ACCENT)
                        } else {
                            text
                        };
                        if ui.add(egui::Button::selectable(selected, text)).clicked() {
                            self.switch_tab(tab);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let add = egui::Button::new(
                            RichText::new(format!("{}  Add models", icon::PLUS))
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(theme::ACCENT);
                        if ui
                            .add(add)
                            .on_hover_text("Open one or more IFC files")
                            .clicked()
                        {
                            self.open_file_dialog(ui.ctx());
                        }
                    });
                });
            });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.jobs.is_busy() {
                        ui.spinner();
                        let labels: Vec<&str> = self.jobs.labels().collect();
                        ui.label(labels.join(", "));
                    } else {
                        let file = self
                            .session
                            .path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Not saved yet".to_owned());
                        ui.label(RichText::new(file).color(theme::TEXT_MUTED));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let elements: usize =
                            self.federation.iter().map(|(_, m)| m.elements.len()).sum();
                        let triangles: usize = self
                            .federation
                            .iter()
                            .map(|(_, m)| m.triangle_count())
                            .sum();
                        ui.label(
                            RichText::new(format!(
                                "{} models  ·  {} elements  ·  {} triangles",
                                self.federation.len(),
                                elements,
                                triangles
                            ))
                            .color(theme::TEXT_MUTED),
                        );
                        if !self.view.selection.is_empty() {
                            ui.separator();
                            ui.label(format!("{} selected", self.view.selection.len()));
                        }
                    });
                });
            });
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(action) = self.confirm.clone() else {
            return;
        };
        let name = self
            .session
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::new("unsaved_changes")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(
                RichText::new(format!("Save changes to “{name}”?"))
                    .heading()
                    .strong(),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new("Your changes will be lost if you don't save them.")
                    .color(theme::TEXT_MUTED),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let save =
                    egui::Button::new(RichText::new("Save").strong().color(egui::Color32::WHITE))
                        .fill(theme::ACCENT);
                if ui.add(save).clicked() {
                    choice = Some(0);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        if modal.should_close() && choice.is_none() {
            choice = Some(2);
        }
        match choice {
            Some(0) => {
                self.confirm = None;
                self.save(Some(action), ctx);
            }
            Some(1) => {
                self.confirm = None;
                self.perform(action, ctx);
            }
            Some(_) => self.confirm = None,
            None => {}
        }
    }
}

fn with_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(PROJECT_EXTENSION));
    if has {
        path
    } else {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(PROJECT_EXTENSION);
        path.with_file_name(name)
    }
}

fn source_label(source: &ModelSource) -> String {
    let path: &Path = match source {
        ModelSource::File(p) => p,
        ModelSource::Project { entry, .. } => Path::new(entry),
    };
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl eframe::App for OpenCheckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.receive_jobs(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.update_title(&ctx);
        if self.jobs.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }

        self.top_bar(ui);
        self.status_bar(ui);
        match self.tab {
            Tab::Model => {
                panels::model_panel(self, ui);
                // Always present, so the 3D view does not jump when the selection changes.
                panels::properties_panel(self, ui);
            }
            Tab::Checking => {
                checking_ui::rulesets_panel(self, ui);
                results_ui::results_panel(self, ui);
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| viewport::show(self, ui));

        ruleset_manager::show(self, &ctx);
        self.confirm_dialog(&ctx);
        self.sync_gpu();
    }
}

/// Some context around a problem, but not the whole building.
fn finding_frame(region: &Aabb) -> Aabb {
    let pad = (region.size().length() * 0.75).clamp(1.0, 4.0);
    Aabb {
        min: region.min - Vec3::splat(pad),
        max: region.max + Vec3::splat(pad),
    }
}
