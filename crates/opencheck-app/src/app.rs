//! Application state and top-level layout.

use std::path::{Path, PathBuf};

use eframe::egui::{self, KeyboardShortcut, Modifiers, RichText};
use eframe::egui_wgpu::RenderState;
use egui_phosphor::regular as icon;
use glam::Vec3;
use opencheck_core::check::{CheckModel, FindingElement, Severity};
use opencheck_core::project::{
    self, ModelEntry, ModelSource, PROJECT_EXTENSION, ProjectDocuments, ProjectModel,
};
use opencheck_core::{Aabb, Federation, IfcModel, ModelId};

use crate::camera::Camera;
use crate::checking::{Checking, Decision};
use crate::issues::{
    ComponentRole, Issue, IssueCamera, IssueComponent, IssueElement, IssueSection, IssueSource,
    IssueStatus, Issues, ModelRevision, RunContext, new_guid,
};
use crate::jobs::{JobResult, Jobs, SaveRequest};
use crate::measure::MeasureTool;
use crate::platform;
use crate::render::SceneGpu;
use crate::ruleset_manager::{self, ManagerState};
use crate::scene::{ElementRef, Focus, SectionPlane, ViewState};
use crate::session::{self, ModelRecord, Session};
use crate::{checking_ui, panels, results_ui, theme, viewport};

/// The floating properties window: open, closed by the user (then it no
/// longer opens by itself), and the element it last saw selected.
#[derive(Default)]
pub struct PropertiesWindow {
    pub open: bool,
    pub closed: bool,
    pub seen: Option<ElementRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Model,
    Checking,
    /// Visual checking: rules that colour elements.
    Visual,
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
    pub walk: Walk,
}

/// First-person walking: stand on a floor and move around at eye height.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    #[default]
    Off,
    /// Waiting for a click on the floor to stand on.
    Placing,
    Walking,
}

/// Where findings go when they become issues.
#[derive(Debug, Clone, PartialEq)]
pub enum IssueTarget {
    /// One issue holding all of them.
    Bundle,
    /// One issue per finding. A single finding takes what is on screen:
    /// the reviewer is looking at it.
    Separate,
    /// One issue per finding, each framed on its finding also when there
    /// is only one: for approved suggestions, which nobody is looking at.
    EachFramed,
    /// Added to an issue that already exists.
    Existing(String),
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
    #[cfg(not(target_arch = "wasm32"))]
    pub autorun: crate::autorun_ui::AutorunUi,
    #[cfg(not(target_arch = "wasm32"))]
    pub claude: crate::claude_link::Link,
    pub visual: crate::visual_ui::VisualUi,
    pub issues: Issues,
    /// Connection to a BCF server (Dalux) for sharing issues.
    pub bcf: crate::bcf_live::BcfLive,
    /// Pictures being taken for a BCF file or report.
    pub capture: Option<crate::export::Capture>,
    /// Where the 3D view was drawn last frame, for pictures.
    pub viewport_rect: egui::Rect,
    /// The report settings window is open.
    pub report_window: bool,
    /// Issues about to be created, waiting for their fields in the dialog.
    pub issue_draft: Option<crate::issue_dialog::IssueDraft>,
    /// "Edit view": the working area being drawn in a top view.
    pub area_edit: Option<crate::area_edit::AreaEdit>,
    /// F12: frame time and display scale in the status bar, for diagnosing
    /// slow frames (e.g. when moving between screens with different scaling).
    frame_stats: bool,
    /// CPU time of the last frame in milliseconds, slowest of the last second.
    frame_ms: (f32, f32, f64),
    /// Point of the finding or issue under review, for the marker in the 3D view.
    pub focus_marker: Option<Vec3>,
    pub tab: Tab,
    pub jobs: Jobs,
    pub session: Session,
    pub errors: Vec<String>,
    /// Short messages that fade by themselves; the time is set when first shown.
    pub notices: Vec<(String, Option<f64>)>,
    /// A model was updated: run the check again once loading has finished.
    pub recheck_after_update: bool,
    pub tree_filter: String,
    pub property_filter: String,
    /// The floating properties window of the Check and Visual Check tabs.
    pub properties: PropertiesWindow,
    render_state: Option<RenderState>,
    /// Bounds of all loaded geometry in the federation frame.
    pub scene_bounds: Aabb,
    confirm: Option<PendingAction>,
    after_save: Option<PendingAction>,
    allow_close: bool,
    window_title: String,
    /// The browser version's welcome note is showing.
    welcome: bool,
    welcome_never_again: bool,
}

/// Storage key: the welcome note was turned off.
const WELCOME_OFF: &str = "welcome_off";

impl OpenCheckApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx, theme::saved_dark());
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
            #[cfg(not(target_arch = "wasm32"))]
            autorun: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            claude: Default::default(),
            visual: Default::default(),
            issues: Issues::default(),
            bcf: crate::bcf_live::BcfLive::default(),
            capture: None,
            viewport_rect: egui::Rect::NOTHING,
            report_window: false,
            issue_draft: None,
            area_edit: None,
            frame_stats: false,
            frame_ms: (0.0, 0.0, 0.0),
            focus_marker: None,
            tab: Tab::Model,
            jobs: Jobs::default(),
            session: Session::default(),
            errors: Vec::new(),
            notices: Vec::new(),
            recheck_after_update: false,
            tree_filter: String::new(),
            property_filter: String::new(),
            properties: Default::default(),
            render_state,
            scene_bounds: Aabb::EMPTY,
            confirm: None,
            after_save: None,
            allow_close: false,
            window_title: String::new(),
            welcome: platform::IS_WEB
                && !cc
                    .storage
                    .and_then(|s| eframe::get_value::<bool>(s, WELCOME_OFF))
                    .unwrap_or(false),
            welcome_never_again: false,
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
            "ifc" => self.jobs.load_file(path, None, ctx),
            PROJECT_EXTENSION => self.request(PendingAction::OpenProject(Some(path)), ctx),
            _ => self.errors.push(format!(
                "{} is not an IFC file or OpenCheck project",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }

    pub fn open_file_dialog(&mut self, ctx: &egui::Context) {
        if platform::IS_WEB {
            platform::pick_in_browser(
                platform::Pick::AddModels,
                ("IFC model", &["ifc", "IFC"]),
                true,
                ctx,
            );
            return;
        }
        if let Some(paths) = platform::pick_files("Add IFC models", ("IFC model", &["ifc", "IFC"]))
        {
            for path in paths {
                self.jobs.load_file(path, None, ctx);
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
                // In the browser the file arrives later; see handle_picked_files.
                if platform::IS_WEB {
                    platform::pick_in_browser(
                        platform::Pick::OpenProject,
                        ("OpenCheck project", &[PROJECT_EXTENSION]),
                        false,
                        ctx,
                    );
                    return;
                }
                let path = path.or_else(|| {
                    platform::pick_file("Open project", ("OpenCheck project", &[PROJECT_EXTENSION]))
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
        self.visual = Default::default();
        self.focus_marker = None;
        self.scene_bounds = Aabb::EMPTY;
        self.recheck_after_update = false;
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
        if let Some(path) = platform::save_file(
            "Save project",
            ("OpenCheck project", &[PROJECT_EXTENSION]),
            &name,
        ) {
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
                requirements: self.checking.requirement_files(),
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
                    replaces,
                    result,
                } => match result {
                    Ok(model) => match replaces {
                        Some(old) => self.replace_model(old, *model, source),
                        None => self.add_model(*model, source, entry),
                    },
                    Err(message) => self.errors.push(format!(
                        "Could not open {}: {message}",
                        source_label(&source)
                    )),
                },
                JobResult::ProjectOpened { path, result } => match result {
                    Ok(contents) => {
                        self.session.path = Some(path);
                        self.session.created = Some(contents.manifest.created);
                        let errors = self.checking.load(
                            contents.rulesets,
                            contents.requirements,
                            contents.documents.checking,
                        );
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
                    self.apply_area();
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
                                // In the browser the bytes stay in memory; the
                                // download is not a file that can be read again.
                                if !platform::IS_WEB {
                                    record.source = ModelSource::Project {
                                        path: path.clone(),
                                        entry: entry.file.clone(),
                                    };
                                }
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
                    self.evaluate_issues();
                    self.presort(false);
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
                        ModelSource::Project { .. } | ModelSource::Memory { .. } => None,
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
        if self.checking.area.is_some() {
            self.apply_area();
        }
        if first {
            self.camera.home(&self.scene_bounds, self.nav.aspect);
        }
    }

    pub fn notify(&mut self, message: String) {
        self.notices.push((message, None));
    }

    /// Asks for a newer version of a model's IFC file and loads it in its place.
    pub fn update_model_dialog(&mut self, id: ModelId, ctx: &egui::Context) {
        let name = self
            .federation
            .get(id)
            .map(|m| m.short_name.clone())
            .unwrap_or_default();
        if platform::IS_WEB {
            self.errors.push(
                "Updating a model is not in the browser version yet: remove it and add the new file."
                    .to_owned(),
            );
            return;
        }
        if let Some(path) = platform::pick_file(
            &format!("Update {name}: choose the new IFC file"),
            ("IFC model", &["ifc", "IFC"]),
        ) {
            self.jobs.load_file(path, Some(id), ctx);
        }
    }

    /// Swaps in a new version of a model. Name, discipline, visibility and
    /// the user's picks (by GlobalId) carry over; the revision goes up.
    fn replace_model(&mut self, id: ModelId, mut model: IfcModel, source: ModelSource) {
        let (Some(record), Some(old)) = (
            self.session.records.get(&id).cloned(),
            self.federation.get(id),
        ) else {
            self.add_model(model, source, None);
            return;
        };
        if record.entry.sha256 == model.content_hash {
            let message = format!(
                "{} is unchanged: the new file is identical to the loaded version.",
                old.short_name
            );
            self.notify(message);
            return;
        }
        model.short_name = old.short_name.clone();
        model.discipline = old.discipline;

        // Picks on the old model, by GlobalId, to find them again in the new one.
        let global_ids = |refs: Vec<ElementRef>| -> Vec<String> {
            refs.into_iter()
                .filter(|r| r.model == id)
                .filter_map(|r| old.element(r.express_id).map(|e| e.global_id.clone()))
                .collect()
        };
        let hidden = global_ids(self.view.hidden.iter().copied().collect());
        let isolated = self
            .view
            .isolated
            .as_ref()
            .map(|set| global_ids(set.iter().copied().collect()));
        let selection = global_ids(self.view.selection.clone());
        self.view.hidden.retain(|r| r.model != id);
        if let Some(set) = &mut self.view.isolated {
            set.retain(|r| r.model != id);
        }
        self.view.selection.retain(|r| r.model != id);
        let visible = self.view.model(id).is_none_or(|m| m.visible);
        if self
            .view
            .focus
            .as_ref()
            .is_some_and(|f| f.involves_model(id))
        {
            self.clear_finding_focus();
        }

        let revision = record.entry.revision + 1;
        let name = model.short_name.clone();
        self.federation.replace(id, model);
        self.measure.edges.forget_model(id);
        self.view.remove_model(id);
        let transform = self
            .federation
            .model_transform(id)
            .expect("model was just replaced");
        let model = self.federation.get(id).expect("model was just replaced");
        let chunks = self.view.add_model(id, model, transform);
        if let Some(rs) = &self.render_state {
            SceneGpu::upload_model(rs, id, &chunks);
        }
        let find = |gid: &String| {
            model.element_by_global_id(gid).map(|e| ElementRef {
                model: id,
                express_id: e.express_id,
            })
        };
        self.view.hidden.extend(hidden.iter().filter_map(find));
        if let (Some(set), Some(ids)) = (&mut self.view.isolated, isolated) {
            set.extend(ids.iter().filter_map(find));
        }
        self.view
            .selection
            .extend(selection.iter().filter_map(find));
        let content_hash = model.content_hash.clone();
        if !visible {
            self.set_model_visible(id, false);
        }

        let entry = self
            .session
            .records
            .get_mut(&id)
            .map(|r| &mut r.entry)
            .expect("record checked above");
        entry.revision = revision;
        entry.sha256 = content_hash;
        entry.updated = project::now_rfc3339();
        if let ModelSource::File(path) = &source {
            entry.source_path = Some(path.display().to_string());
        }
        if let Some(r) = self.session.records.get_mut(&id) {
            r.source = source;
        }
        self.scene_bounds = self.federation.bounds();
        if self.checking.area.is_some() {
            self.apply_area();
        }
        self.session.dirty = true;
        self.checking.stale = true;
        // Re-check so issues learn what the update solved.
        self.recheck_after_update = self.checking.report.is_some();
        self.notify(format!("{name} updated to revision {revision}."));
    }

    /// Marks issue findings that a model update made disappear as solved,
    /// and reopens them if they come back.
    fn evaluate_issues(&mut self) {
        let Some(report) = self.checking.report.as_ref() else {
            return;
        };
        let model_id = |name: &str| {
            self.federation
                .iter()
                .find(|(_, m)| m.short_name == name)
                .map(|(id, _)| id)
        };
        let revision = |name: &str| {
            model_id(name)
                .and_then(|id| self.session.records.get(&id))
                .map(|r| r.entry.revision)
        };
        // Was this rule run on every model the finding involved?
        let checked = |source: &IssueSource| {
            report.rulesets.iter().any(|rs| {
                let same_ruleset = if source.ruleset_id.is_empty() {
                    rs.info.name == source.ruleset
                } else {
                    rs.info.id == source.ruleset_id
                };
                same_ruleset
                    && rs.rules.iter().any(|r| {
                        if source.rule_id.is_empty() {
                            r.rule.name == source.rule
                        } else {
                            r.rule.id == source.rule_id
                        }
                    })
                    && !source.revisions.is_empty()
                    && source
                        .revisions
                        .iter()
                        .all(|m| model_id(&m.model).is_some_and(|id| rs.models.contains(&id)))
            })
        };
        let present = |finding: &str| self.checking.find(finding).is_some();
        let outcome = self.issues.apply_run(
            &RunContext {
                present: &present,
                checked: &checked,
                revision: &revision,
            },
            &project::now_rfc3339(),
        );
        if outcome.solved > 0 {
            self.notify(format!(
                "{} issue findings were solved by model updates.",
                outcome.solved
            ));
        }
        if outcome.reopened > 0 {
            self.notify(format!(
                "{} issue findings came back and were reopened.",
                outcome.reopened
            ));
        }
        if outcome.solved + outcome.reopened > 0 {
            self.session.dirty = true;
        }
    }

    pub fn remove_model(&mut self, id: ModelId) {
        self.federation.remove(id);
        self.measure.edges.forget_model(id);
        self.checking.forget_model(id);
        if self
            .view
            .focus
            .as_ref()
            .is_some_and(|f| f.involves_model(id))
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
        if self.checking.area.is_some() {
            self.apply_area();
        }
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
        // While reviewing a clash, "everything" includes the model around it.
        if self.view.focus.is_some() {
            self.view.show_context = true;
        }
        self.view.hidden.clear();
        self.view.isolated = None;
        self.view.storey_filter = None;
        self.view.modes_dirty = true;
    }

    pub fn toggle_xray(&mut self) {
        if self.view.focus.is_some() {
            self.view.show_context = !self.view.show_context;
        } else {
            self.view.xray = !self.view.xray;
        }
        self.view.modes_dirty = true;
    }

    /// Cuts the model just above the top of the selection, or through the
    /// middle of the view when nothing is selected.
    pub fn section_at_selection(&mut self) {
        // Reviewing a clash: cut a little above it, so it can be seen from above.
        if self.view.focus.is_some()
            && let Some(point) = self.focus_marker
        {
            self.view.section = Some(SectionPlane {
                height: point.z + 0.25,
                anchor: point,
            });
            return;
        }
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
        crate::visual_ui::apply_paint(self);
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
                .map(|r| self.checking.job(r, &ids))
                .collect(),
            None => self.checking.jobs(&ids),
        };
        if jobs.is_empty() {
            return;
        }
        let origin = self.federation.origin();
        let area = self.checking.area;
        let models: Vec<CheckModel> = ids
            .iter()
            .filter_map(|id| {
                let model = self.federation.shared(*id)?;
                let transform = self.federation.model_transform(*id)?;
                // Only the working area takes part.
                let mask = area.map(|a| std::sync::Arc::new(a.mask(&model, &transform, origin)));
                Some(CheckModel {
                    id: *id,
                    model,
                    transform,
                    mask,
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

    /// The a- and b-side elements of some findings, and the region they cover.
    fn focus_for<'a>(&self, ids: impl IntoIterator<Item = &'a String>) -> (Focus, Aabb) {
        let mut focus = Focus::default();
        let mut region = Aabb::EMPTY;
        for id in ids {
            if let Some((_, _, f)) = self.checking.find(id) {
                focus.a.insert(ElementRef {
                    model: f.a.model,
                    express_id: f.a.express_id,
                });
                focus.b.insert(ElementRef {
                    model: f.b.model,
                    express_id: f.b.express_id,
                });
                region = region.union(&f.region);
            }
        }
        (focus, region)
    }

    /// Highlights every ticked finding and the one under review, without
    /// moving the camera.
    pub fn refresh_finding_focus(&mut self) {
        let mut ids = self.checking.marked.clone();
        if let Some(selected) = &self.checking.selected
            && !ids.contains(selected)
        {
            ids.push(selected.clone());
        }
        let (focus, _) = self.focus_for(&ids);
        self.view.focus = (!focus.a.is_empty()).then_some(focus);
        self.focus_marker = self
            .checking
            .selected
            .as_ref()
            .and_then(|id| self.checking.find(id))
            .map(|(_, _, f)| f.point);
        self.view.modes_dirty = true;
    }

    /// Highlights the given findings and frames them together.
    pub fn show_group(&mut self, ids: &[String]) {
        let (focus, region) = self.focus_for(ids);
        if focus.a.is_empty() {
            return;
        }
        self.checking.selected = None;
        self.issues.selected = None;
        self.view.focus = Some(focus);
        self.focus_marker = None;
        self.view.selection.clear();
        self.view.modes_dirty = true;
        self.camera.fit(&finding_frame(&region), self.nav.aspect);
    }

    pub fn show_finding(&mut self, id: &str, move_camera: bool) {
        let Some((_, _, finding)) = self.checking.find(id) else {
            self.clear_finding_focus();
            return;
        };
        let frame = finding_frame(&finding.region);
        self.checking.selected = Some(id.to_owned());
        self.issues.selected = None;
        self.view.selection.clear();
        self.refresh_finding_focus();
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
        self.view.focus.as_ref()?;
        self.focus_marker
    }

    /// Boxes of the blocked zone of the finding under review (free zone and
    /// script findings), for drawing in the 3D view.
    pub fn focus_zones(&self) -> Vec<[Vec3; 8]> {
        if self.view.focus.is_none() {
            return Vec::new();
        }
        self.checking
            .selected
            .as_ref()
            .and_then(|id| self.checking.find(id))
            .map(|(_, _, f)| f.zones.clone())
            .unwrap_or_default()
    }

    /// Turns findings into issues and marks them accepted: an issue is a
    /// problem someone has to handle. Returns the issue they went into.
    pub fn add_issues(&mut self, finding_ids: &[String], target: IssueTarget) -> Option<String> {
        let origin = self.federation.origin();
        let model_name = |id| {
            self.federation
                .get(id)
                .map(|m| m.short_name.clone())
                .unwrap_or_default()
        };
        // The model versions a finding was seen in.
        let model_revisions = |ids: &[ModelId]| {
            let mut revisions: Vec<ModelRevision> = Vec::new();
            for id in ids {
                let model = model_name(*id);
                if revisions.iter().all(|r| r.model != model) {
                    revisions.push(ModelRevision {
                        model,
                        revision: self.session.records.get(id).map_or(1, |r| r.entry.revision),
                    });
                }
            }
            revisions
        };
        let component = |e: &FindingElement, role| IssueComponent {
            global_id: e.global_id.clone(),
            name: e.name.clone(),
            ifc_class: e.ifc_class.clone(),
            model: model_name(e.model),
            role,
            tag: self
                .federation
                .get(e.model)
                .and_then(|m| m.element(e.express_id))
                .map(crate::issues::element_id)
                .unwrap_or_default(),
        };
        struct Entry {
            source: IssueSource,
            components: Vec<IssueComponent>,
            region: Aabb,
            severity: Severity,
            classes: (String, String),
            title: String,
            /// Missing information about one element, not a pair.
            single: bool,
            /// Disciplines of the models the elements come from.
            disciplines: Vec<opencheck_core::Discipline>,
        }
        let entries: Vec<Entry> = finding_ids
            .iter()
            .filter(|id| self.issues.for_finding(id).is_none())
            .filter_map(|id| self.checking.find(id))
            .map(|(ruleset, rule, f)| Entry {
                source: IssueSource {
                    finding: f.id.clone(),
                    ruleset: ruleset.info.name.clone(),
                    rule: rule.rule.name.clone(),
                    kind: f.kind.label().to_owned(),
                    magnitude_mm: f.magnitude * 1000.0,
                    ruleset_id: ruleset.info.id.clone(),
                    rule_id: rule.rule.id.clone(),
                    revisions: model_revisions(&[f.a.model, f.b.model]),
                    solved_by_update: None,
                    details: f.details.clone(),
                },
                components: if f.is_single() {
                    vec![component(&f.a, ComponentRole::A)]
                } else {
                    vec![
                        component(&f.a, ComponentRole::A),
                        component(&f.b, ComponentRole::B),
                    ]
                },
                region: f.region,
                severity: rule.rule.severity,
                classes: if f.is_single() {
                    (f.category.clone(), f.category.clone())
                } else {
                    (f.a.ifc_class.clone(), f.b.ifc_class.clone())
                },
                title: if f.is_single() {
                    format!(
                        "{}: missing {}",
                        results_ui::element_label(&f.a),
                        f.details.join(", ")
                    )
                } else {
                    format!(
                        "{} vs {}",
                        results_ui::element_label(&f.a),
                        results_ui::element_label(&f.b)
                    )
                },
                single: f.is_single(),
                disciplines: {
                    let mut list = Vec::new();
                    for model in [f.a.model, f.b.model] {
                        if let Some(m) = self.federation.get(model)
                            && !list.contains(&m.discipline)
                        {
                            list.push(m.discipline);
                        }
                    }
                    list
                },
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        for e in &entries {
            self.checking
                .decide(&e.source.finding, Some(Decision::Accepted));
        }
        self.session.dirty = true;

        // The viewpoint frames every finding in the issue.
        let viewpoint = |entries: &[&Entry]| {
            let region = entries
                .iter()
                .fold(Aabb::EMPTY, |acc, e| acc.union(&e.region));
            let mut camera = self.camera;
            camera.fit(&finding_frame(&region), self.nav.aspect);
            (
                (origin + region.center().as_dvec3()).to_array(),
                IssueCamera {
                    eye: (origin + camera.eye.as_dvec3()).to_array(),
                    yaw: camera.yaw,
                    pitch: camera.pitch,
                },
            )
        };
        // What the reviewer is looking at right now: camera, section, context.
        let current = self.capture_issue_view();
        let make = |entries: &[&Entry], use_current: bool| {
            let (point, framed) = viewpoint(entries);
            let first = entries[0];
            let same_classes = entries.iter().all(|e| e.classes == first.classes);
            let title = match (entries.len(), same_classes) {
                (1, _) => first.title.clone(),
                (n, true) if first.single => {
                    format!("{n} elements: {} missing information", first.classes.0)
                }
                (n, true) => format!(
                    "{n} findings: {} vs {}",
                    results_ui::component_kind(&first.classes.0),
                    results_ui::component_kind(&first.classes.1)
                ),
                (n, false) => format!("{n} findings: {} and others", first.title),
            };
            let mut labels: Vec<String> = entries.iter().map(|e| e.source.kind.clone()).collect();
            labels.sort();
            labels.dedup();
            let now = project::now_rfc3339();
            let mut issue = Issue {
                guid: new_guid(&first.source.finding),
                title,
                description: String::new(),
                status: IssueStatus::Open,
                priority: entries
                    .iter()
                    .map(|e| e.severity)
                    .min()
                    .unwrap_or(first.severity),
                created: now.clone(),
                modified: now,
                labels,
                findings: entries.iter().map(|e| e.source.clone()).collect(),
                source: None,
                components: Vec::new(),
                point,
                camera: Some(framed),
                section: None,
                show_context: false,
                hidden: Vec::new(),
                solved_by_update: false,
                responsible: Vec::new(),
                assigned_to: String::new(),
                bcf: None,
            };
            for e in entries {
                issue.add_responsible(e.disciplines.iter().copied());
            }
            if use_current {
                current.apply_to(&mut issue);
            }
            issue.add_components(entries.iter().flat_map(|e| e.components.iter().cloned()));
            issue
        };

        let refs: Vec<&Entry> = entries.iter().collect();
        let guid = match target {
            IssueTarget::Bundle => {
                let issue = make(&refs, true);
                let guid = issue.guid.clone();
                self.issues.list.push(issue);
                guid
            }
            IssueTarget::Separate | IssueTarget::EachFramed => {
                let mut last = String::new();
                // One issue per finding: each gets its own framing, unless there is
                // only one and it is what is on screen.
                let on_screen = target == IssueTarget::Separate && refs.len() == 1;
                for e in &refs {
                    let issue = make(std::slice::from_ref(e), on_screen);
                    last = issue.guid.clone();
                    self.issues.list.push(issue);
                }
                last
            }
            IssueTarget::Existing(guid) => {
                let issue = self.issues.get_mut(&guid)?;
                for e in &refs {
                    issue.findings.push(e.source.clone());
                    issue.add_components(e.components.iter().cloned());
                    issue.add_responsible(e.disciplines.iter().copied());
                    issue.priority = issue.priority.min(e.severity);
                }
                if issue.solved_by_update {
                    // New, unsolved findings: the issue is open again.
                    issue.solved_by_update = false;
                    issue.status = IssueStatus::Open;
                }
                issue.touch();
                guid
            }
        };
        // The ticked findings are dealt with now.
        self.checking.marked.clear();
        self.refresh_finding_focus();
        Some(guid)
    }

    /// Restores an issue's viewpoint and shows all of its components.
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
        let mut focus = Focus::default();
        for c in &issue.components {
            if let Some(element) = resolve(c) {
                match c.role {
                    ComponentRole::A => focus.a.insert(element),
                    ComponentRole::B => focus.b.insert(element),
                };
            }
        }
        self.issues.selected = Some(guid.to_owned());
        self.checking.selected = None;
        self.view.focus = (!focus.a.is_empty() || !focus.b.is_empty()).then_some(focus);
        self.focus_marker = Some((glam::DVec3::from_array(issue.point) - origin).as_vec3());
        self.view.selection.clear();
        self.view.modes_dirty = true;
        if let Some(c) = issue.camera {
            self.camera.eye = (glam::DVec3::from_array(c.eye) - origin).as_vec3();
            self.camera.yaw = c.yaw;
            self.camera.pitch = c.pitch;
        }
        // The rest of the viewpoint: section, context and hidden elements.
        self.view.section = issue.section.map(|s| SectionPlane {
            height: (s.height - origin.z) as f32,
            anchor: (glam::DVec3::from_array(s.anchor) - origin).as_vec3(),
        });
        self.view.show_context = issue.show_context;
        self.view.isolated = None;
        self.view.hidden = issue
            .hidden
            .iter()
            .filter_map(|h| {
                resolve(&IssueComponent {
                    global_id: h.global_id.clone(),
                    name: String::new(),
                    ifc_class: String::new(),
                    model: h.model.clone(),
                    role: ComponentRole::A,
                    tag: String::new(),
                })
            })
            .collect();
    }

    /// The current view as an issue viewpoint.
    fn capture_issue_view(&self) -> IssueView {
        let origin = self.federation.origin();
        IssueView {
            camera: IssueCamera {
                eye: (origin + self.camera.eye.as_dvec3()).to_array(),
                yaw: self.camera.yaw,
                pitch: self.camera.pitch,
            },
            section: self.view.section.map(|s| IssueSection {
                height: origin.z + s.height as f64,
                anchor: (origin + s.anchor.as_dvec3()).to_array(),
            }),
            show_context: self.view.show_context,
            hidden: self
                .view
                .hidden
                .iter()
                .filter_map(|e| {
                    let model = self.federation.get(e.model)?;
                    Some(IssueElement {
                        global_id: model.element(e.express_id)?.global_id.clone(),
                        model: model.short_name.clone(),
                    })
                })
                .collect(),
        }
    }

    /// Stores what is on screen now as the issue's viewpoint.
    pub fn update_issue_view(&mut self, guid: &str) {
        let view = self.capture_issue_view();
        if let Some(issue) = self.issues.get_mut(guid) {
            view.apply_to(issue);
            issue.touch();
            self.session.dirty = true;
        }
    }

    pub fn is_busy(&self) -> bool {
        self.jobs.is_busy() || platform::reading_files()
    }

    /// What is being worked on, for the status bar and the browser's
    /// loading message.
    pub fn busy_text(&self) -> String {
        let labels: Vec<&str> = self.jobs.labels().collect();
        if labels.is_empty() {
            "Reading files…".to_owned()
        } else {
            labels.join(", ")
        }
    }

    /// Files picked in the browser arrive as bytes, a frame or more later.
    fn handle_picked_files(&mut self, ctx: &egui::Context) {
        for (kind, files) in platform::take_picked() {
            for file in files {
                match kind {
                    platform::Pick::OpenProject => {
                        self.reset();
                        self.jobs.open_project_bytes(file.name, file.bytes, ctx);
                    }
                    // A project dropped on the page opens, unless that would
                    // discard unsaved work.
                    platform::Pick::AddModels
                        if file
                            .name
                            .to_ascii_lowercase()
                            .ends_with(&format!(".{PROJECT_EXTENSION}")) =>
                    {
                        if self.session.dirty {
                            self.errors.push(format!(
                                "Save the current project (or start a new one) before opening {}",
                                file.name
                            ));
                        } else {
                            self.reset();
                            self.jobs.open_project_bytes(file.name, file.bytes, ctx);
                        }
                    }
                    platform::Pick::AddModels => {
                        if file.name.to_ascii_lowercase().ends_with(".ifc") {
                            self.jobs.load_bytes(file.name, file.bytes, ctx);
                        } else {
                            self.errors.push(format!(
                                "{} is not an IFC file; in the browser only IFC files can be opened",
                                file.name
                            ));
                        }
                    }
                    platform::Pick::ImportRuleset => {
                        let text = String::from_utf8_lossy(&file.bytes);
                        match self.checking.add_ruleset(&text, Some(&file.name)) {
                            Ok(()) => self.session.dirty = true,
                            Err(e) => self
                                .errors
                                .push(format!("{} is not a valid ruleset: {e}", file.name)),
                        }
                    }
                    platform::Pick::ImportIds => {
                        let text = String::from_utf8_lossy(&file.bytes);
                        let stem = file
                            .name
                            .rsplit_once('.')
                            .map_or(file.name.as_str(), |(s, _)| s);
                        match self.checking.add_ids(&text, stem) {
                            Ok(()) => self.session.dirty = true,
                            Err(e) => self
                                .errors
                                .push(format!("{} is not a valid IDS file: {e}", file.name)),
                        }
                    }
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // In the browser dropped files have no path; their bytes arrive later
        // like picked files.
        if platform::IS_WEB {
            let files = ctx.input(|i| i.raw.dropped_files.clone());
            platform::read_dropped(files, ctx);
            return;
        }
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
            // Scheduled runs need the desktop: files on a drive, Task Scheduler.
            #[cfg(not(target_arch = "wasm32"))]
            if item(ui, icon::CLOCK_COUNTER_CLOCKWISE, "Autorun…", None) {
                self.autorun.open();
                ui.close();
            }
            #[cfg(not(target_arch = "wasm32"))]
            if item(ui, icon::SPARKLE, "Claude connection…", None) {
                self.claude.open = true;
                ui.close();
            }
            ui.separator();
            ui.menu_button(format!("{}  Theme", icon::PALETTE), |ui| {
                ui.set_min_width(180.0);
                let dark = theme::is_dark();
                for (label, value) in [("Light (Grafit)", false), ("Dark", true)] {
                    if ui.selectable_label(dark == value, label).clicked() {
                        theme::set_dark(&ctx, value);
                        theme::save_dark(value);
                        ui.close();
                    }
                }
            });
            // A browser tab is closed by the browser.
            if !platform::IS_WEB {
                ui.separator();
                if item(ui, icon::SIGN_OUT, "Exit", None) {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    ui.close();
                }
            }
        });
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top_bar")
            .frame(
                egui::Frame::new()
                    .fill(theme::panel())
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .stroke(egui::Stroke::new(1.0, theme::border())),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} OpenCheck", icon::CUBE_FOCUS))
                            .size(15.0)
                            .family(theme::bold()),
                    );
                    ui.add_space(12.0);
                    self.file_menu(ui);
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(12.0);
                    for (tab, label) in [
                        (Tab::Model, "Model"),
                        (Tab::Checking, "Check"),
                        (Tab::Visual, "Visual Check"),
                    ] {
                        let selected = self.tab == tab;
                        let text = RichText::new(label).size(14.5);
                        let text = if selected {
                            text.family(theme::bold())
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
                                .color(theme::on_accent())
                                .strong(),
                        )
                        .fill(theme::accent());
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
                    .fill(theme::surface())
                    .inner_margin(egui::Margin::symmetric(12, 4))
                    .stroke(egui::Stroke::new(1.0, theme::border())),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.is_busy() {
                        ui.spinner();
                        ui.label(self.busy_text());
                    } else {
                        let file = self
                            .session
                            .path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "Not saved yet".to_owned());
                        ui.label(RichText::new(file).color(theme::text_muted()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.frame_stats {
                            let size = ui.ctx().content_rect().size() * ui.ctx().pixels_per_point();
                            ui.label(
                                RichText::new(format!(
                                    "frame {:.1} ms (slowest {:.0} ms) · scale {:.2} · {:.0}×{:.0} px",
                                    self.frame_ms.0,
                                    self.frame_ms.1,
                                    ui.ctx().pixels_per_point(),
                                    size.x,
                                    size.y
                                ))
                                .monospace()
                                .small()
                                .color(theme::accent()),
                            )
                            .on_hover_text("CPU time per frame. F12 hides this.");
                            ui.separator();
                        }
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
                            .color(theme::text_muted()),
                        );
                        if !self.view.selection.is_empty() {
                            ui.separator();
                            ui.label(format!("{} selected", self.view.selection.len()));
                        }
                    });
                });
            });
    }

    /// What the browser version is, shown when it opens.
    fn welcome_dialog(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.welcome {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("welcome")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.label(
                RichText::new(format!("{}  OpenCheck in the browser", icon::GLOBE))
                    .heading()
                    .strong(),
            );
            ui.add_space(8.0);
            ui.label(
                "This is the web version of the OpenCheck desktop app, meant mostly for \
                 testing.",
            );
            ui.add_space(6.0);
            ui.label(
                "Your files stay on your computer. Nothing is uploaded or sent anywhere: \
                 the browser only reads the files you open and shows them.",
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Large models load slower here, and BCF Live is only in the desktop app.",
                )
                .color(theme::text_muted()),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Desktop app:").color(theme::text_muted()));
                ui.hyperlink_to(
                    "github.com/sparboom-git/opencheck/releases",
                    "https://github.com/sparboom-git/opencheck/releases",
                );
            });
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let start =
                    egui::Button::new(RichText::new("Start").strong().color(theme::on_accent()))
                        .fill(theme::accent());
                if ui.add(start).clicked() {
                    close = true;
                }
                ui.checkbox(&mut self.welcome_never_again, "Don't show this again");
            });
        });
        if close || modal.should_close() {
            self.welcome = false;
            if self.welcome_never_again
                && let Some(storage) = frame.storage_mut()
            {
                eframe::set_value(storage, WELCOME_OFF, &true);
                storage.flush();
            }
        }
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
                    .color(theme::text_muted()),
            );
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let save =
                    egui::Button::new(RichText::new("Save").strong().color(theme::on_accent()))
                        .fill(theme::accent());
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
        ModelSource::Memory { name, .. } => Path::new(name),
    };
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl eframe::App for OpenCheckApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if ctx.input(|i| i.key_pressed(egui::Key::F12)) {
            self.frame_stats = !self.frame_stats;
        }
        if self.frame_stats {
            let ms = frame.info().cpu_usage.unwrap_or(0.0) * 1000.0;
            let now = ctx.input(|i| i.time);
            let (_, slowest, since) = self.frame_ms;
            let slowest = if now - since > 1.0 {
                ms
            } else {
                slowest.max(ms)
            };
            let since = if now - since > 1.0 { now } else { since };
            self.frame_ms = (ms, slowest, since);
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        self.receive_jobs(&ctx);
        self.handle_picked_files(&ctx);
        self.handle_bcf(&ctx);
        self.step_capture(&ctx);
        self.handle_dropped_files(&ctx);
        self.handle_shortcuts(&ctx);
        self.handle_close_request(&ctx);
        self.update_title(&ctx);
        if self.jobs.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else if self.recheck_after_update && !self.checking.is_running() {
            // All updated models are in: check again so issues see what was solved.
            self.recheck_after_update = false;
            self.run_check(&ctx, None);
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
            Tab::Visual => {
                crate::visual_ui::rules_panel(self, ui);
                crate::visual_ui::results_panel(self, ui);
            }
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| viewport::show(self, ui));

        // Windows stay out of the pictures.
        if !self.is_capturing() {
            ruleset_manager::show(self, &ctx);
            crate::standard_editor::window(self, &ctx);
            crate::bcf_ui::window(self, &ctx);
            crate::issues_ui::report_window(self, &ctx);
            crate::issue_dialog::window(self, &ctx);
            #[cfg(not(target_arch = "wasm32"))]
            crate::autorun_ui::window(self, &ctx);
            #[cfg(not(target_arch = "wasm32"))]
            crate::claude_link::window(self, &ctx);
            panels::properties_window(self, &ctx);
        }
        self.confirm_dialog(&ctx);
        self.welcome_dialog(&ctx, frame);
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

/// What an issue remembers of the view it was created from.
struct IssueView {
    camera: IssueCamera,
    section: Option<IssueSection>,
    show_context: bool,
    hidden: Vec<IssueElement>,
}

impl IssueView {
    fn apply_to(&self, issue: &mut Issue) {
        issue.camera = Some(self.camera);
        issue.section = self.section;
        issue.show_context = self.show_context;
        issue.hidden = self.hidden.clone();
    }
}
