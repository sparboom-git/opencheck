//! BCF Live in the UI: the server window (sign-in, project, responsible
//! people per discipline), turning issues into topics and applying what the
//! server answers.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use glam::DVec3;
use opencheck_core::Discipline;
use opencheck_core::bcf::{
    self, ClippingPlane, ComponentRef, Components, Extensions, Point, Topic, TopicRequest,
    ViewpointRequest, Visibility,
};
use opencheck_core::check::Severity;
use opencheck_core::project;

use crate::app::OpenCheckApp;
use crate::bcf_live::{Message, Outgoing};
use crate::issues::{BcfProjectLink, BcfTopicLink, DisciplineAssignee, Issue, IssueStatus};
use crate::theme;

/// Status names servers use for each of OpenCheck's statuses, best first.
fn status_names(status: IssueStatus) -> &'static [&'static str] {
    match status {
        IssueStatus::Open => &["Open", "Åben", "Aaben", "Aktiv", "New", "Ny", "Active"],
        IssueStatus::InProgress => &[
            "In progress",
            "InProgress",
            "In Progress",
            "Igangværende",
            "I gang",
            "Afventer",
            "Active",
            "Aktiv",
        ],
        IssueStatus::Resolved => &[
            "Resolved",
            "Løst",
            "Fixed",
            "Done",
            "Udført",
            "Afventer godkendelse",
        ],
        IssueStatus::Closed => &[
            "Closed",
            "Lukket",
            "Lukkede",
            "Afsluttet",
            "Done",
            "Godkendt",
        ],
    }
}

/// The server's status for an OpenCheck status, if it has a matching one.
pub fn remote_status(status: IssueStatus, offered: &[String]) -> Option<String> {
    bcf::pick(offered, status_names(status)).cloned()
}

/// OpenCheck's status for a status name from the server.
pub fn local_status(remote: &str) -> Option<IssueStatus> {
    // Most specific first: "Done" counts as resolved before closed.
    [
        IssueStatus::Open,
        IssueStatus::InProgress,
        IssueStatus::Resolved,
        IssueStatus::Closed,
    ]
    .into_iter()
    .find(|s| status_names(*s).iter().any(|n| bcf::same_name(n, remote)))
}

/// The server value a status gets without a mapping: the first offered
/// value with a known name.
pub fn auto_status(status: IssueStatus, ext: &Extensions) -> Option<String> {
    remote_status(status, &ext.topic_status)
}

pub fn auto_priority(priority: Severity, ext: &Extensions) -> Option<String> {
    bcf::pick(&ext.priority, priority_names(priority)).cloned()
}

pub fn auto_topic_type(ext: &Extensions) -> Option<String> {
    bcf::pick(
        &ext.topic_type,
        &[
            crate::c402::ISSUE_TYPE,
            "K&K",
            "Clash",
            "Kollision",
            "Issue",
            "Fejl",
            "Error",
            "Problem",
        ],
    )
    .or(ext.topic_type.first())
    .cloned()
}

/// A server person for initials or a name without a mapping: an exact
/// match, or an e-mail address that starts with the initials
/// ("ABC" -> "abc@example.com | Role").
pub fn auto_person(person: &str, users: &[String]) -> Option<String> {
    let person = person.trim();
    if person.is_empty() {
        return None;
    }
    let lower = person.to_lowercase();
    users
        .iter()
        .find(|u| u.eq_ignore_ascii_case(person))
        .or_else(|| {
            users
                .iter()
                .find(|u| u.to_lowercase().starts_with(&format!("{lower}@")))
        })
        .cloned()
}

/// OpenCheck's criticality for a server priority: by the mapping, or by name.
pub fn local_priority(remote: &str, mapping: &crate::issues::BcfMapping) -> Option<Severity> {
    mapping
        .priority
        .iter()
        .find(|m| m.to == remote)
        .map(|m| m.from)
        .or_else(|| {
            crate::c402::LEVELS
                .into_iter()
                .find(|s| priority_names(*s).iter().any(|n| bcf::same_name(n, remote)))
        })
}

fn priority_names(severity: Severity) -> &'static [&'static str] {
    match severity {
        Severity::Critical => &["C3", "Critical", "Kritisk", "High", "Høj"],
        Severity::Moderate => &["C2", "Moderate", "Normal", "Medium", "Middel"],
        Severity::Low => &["C1", "Low", "Lav", "Minor"],
    }
}

/// The topic description: the issue's own text, responsible disciplines and
/// findings.
pub fn topic_description(issue: &Issue) -> String {
    let mut description = issue.description.trim().to_owned();
    let mut add = |line: String| {
        if !description.is_empty() {
            description.push_str(
                "

",
            );
        }
        description.push_str(&line);
    };
    if !issue.responsible.is_empty() {
        add(format!(
            "Responsible: {}",
            issue
                .responsible
                .iter()
                .map(|d| format!("{} ({})", d.label(), d.code()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let findings: Vec<String> = issue
        .findings
        .iter()
        .map(|f| {
            if f.details.is_empty() {
                format!("- {} {:.0} mm: {}", f.kind, f.magnitude_mm, f.rule)
            } else {
                format!("- Missing: {}", f.details.join(", "))
            }
        })
        .collect();
    if !findings.is_empty() {
        add(format!(
            "Findings:
{}",
            findings.join(
                "
"
            )
        ));
    }
    add("Created with OpenCheck.".to_owned());
    description
}

/// The issue's viewpoint in BCF terms: camera, section plane, selected and
/// visible components.
pub fn issue_viewpoint(issue: &Issue, fov_y: f32) -> Option<ViewpointRequest> {
    let ids = |list: &mut dyn Iterator<Item = &String>| -> Vec<ComponentRef> {
        list.map(|g| ComponentRef {
            ifc_guid: g.clone(),
        })
        .collect()
    };
    let selection = ids(&mut issue.components.iter().map(|c| &c.global_id));
    let visibility = if issue.show_context {
        Visibility {
            default_visibility: true,
            exceptions: ids(&mut issue.hidden.iter().map(|h| &h.global_id)),
        }
    } else {
        Visibility {
            default_visibility: false,
            exceptions: selection.clone(),
        }
    };
    issue.camera.map(|c| ViewpointRequest {
        perspective_camera: bcf::perspective_camera(
            DVec3::from_array(c.eye),
            c.yaw as f64,
            c.pitch as f64,
            fov_y as f64,
        ),
        components: Components {
            selection,
            visibility,
        },
        clipping_planes: issue
            .section
            .map(|s| ClippingPlane {
                location: Point {
                    x: s.anchor[0],
                    y: s.anchor[1],
                    z: s.height,
                },
                direction: Point {
                    x: 0.0,
                    y: 0.0,
                    z: 1.0,
                },
            })
            .into_iter()
            .collect(),
        snapshot: None,
    })
}

/// BCF status names for files: the English names BCF tools know.
pub fn file_status(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "Open",
        IssueStatus::InProgress => "In Progress",
        IssueStatus::Resolved => "Resolved",
        IssueStatus::Closed => "Closed",
    }
}

/// The topic and viewpoint for an issue.
pub fn outgoing(
    issue: &Issue,
    link: &BcfProjectLink,
    extensions: Option<&Extensions>,
    fov_y: f32,
) -> Outgoing {
    let empty = Extensions::default();
    let ext = extensions.unwrap_or(&empty);
    let mapping = &link.mapping;
    // Initials or a name in OpenCheck, then the server's person for it.
    let assignee = issue.effective_assignee(Some(link)).and_then(|person| {
        let mapped = mapping
            .person_for(person)
            .map(str::to_owned)
            .or_else(|| auto_person(person, &ext.user_id_type))
            .unwrap_or_else(|| person.to_owned());
        (ext.user_id_type.is_empty() || ext.user_id_type.contains(&mapped)).then_some(mapped)
    });
    let topic = TopicRequest {
        title: issue.title.clone(),
        topic_type: Some(mapping.topic_type.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| auto_topic_type(ext)),
        topic_status: mapping
            .status_for(issue.status)
            .map(str::to_owned)
            .or_else(|| auto_status(issue.status, ext)),
        priority: mapping
            .priority_for(issue.priority)
            .map(str::to_owned)
            .or_else(|| auto_priority(issue.priority, ext)),
        stage: Some(mapping.stage.clone()).filter(|s| !s.is_empty()),
        // Discipline labels, where the server offers them.
        labels: issue
            .responsible
            .iter()
            .filter_map(|d| crate::c402::discipline_label(*d))
            .filter_map(|l| bcf::pick(&ext.topic_label, &[l]).cloned())
            .collect(),
        assigned_to: assignee,
        description: Some(topic_description(issue)),
    };
    Outgoing {
        issue: issue.guid.clone(),
        topic_guid: issue.bcf.as_ref().map(|b| b.topic_guid.clone()),
        topic,
        viewpoint: issue_viewpoint(issue, fov_y),
        version: issue.modified.clone(),
    }
}

impl OpenCheckApp {
    /// Applies answers from the server and keeps the project's lists loaded.
    pub fn handle_bcf(&mut self, ctx: &egui::Context) {
        for message in self.bcf.poll() {
            match message {
                Message::Ready => self.bcf.load_projects(ctx),
                Message::Sent {
                    issue,
                    version,
                    topic,
                } => {
                    let Some(link) = &self.issues.bcf else {
                        continue;
                    };
                    let project_id = link.project_id.clone();
                    if let Some(issue) = self.issues.get_mut(&issue) {
                        issue.bcf = Some(BcfTopicLink {
                            project_id,
                            topic_guid: topic.guid.clone(),
                            synced: project::now_rfc3339(),
                            remote_status: topic.topic_status.clone().unwrap_or_default(),
                            remote_assigned_to: topic.assigned_to.clone().unwrap_or_default(),
                            sent_version: version,
                        });
                        self.session.dirty = true;
                    }
                }
                Message::Topics { project, topics } => {
                    let changed = self.apply_topics(&project, &topics);
                    self.bcf.status = if changed > 0 {
                        format!(
                            "Synced: {changed} issues were changed on the server and updated here."
                        )
                    } else {
                        "Synced: the issues are up to date with the server.".to_owned()
                    };
                }
                Message::Error(text) => self.errors.push(text),
                _ => {}
            }
        }
        // Load what the linked project accepts once signed in.
        if let Some(link) = &self.issues.bcf
            && self.bcf.is_signed_in()
            && !self.bcf.is_busy()
            && self.bcf.extensions_for != link.project_id
            && self.bcf.account().server == link.server
        {
            let project = link.project_id.clone();
            self.bcf.extensions_for = project.clone();
            self.bcf.load_extensions(&project, ctx);
        }
        // As in Solibri: once the project's values are known, ask the user
        // to pair them with OpenCheck's, the first time.
        if let Some(link) = &self.issues.bcf
            && !link.mapping.confirmed
            && self.bcf.extensions.is_some()
            && self.bcf.extensions_for == link.project_id
            && self.bcf.mapping_offered != link.project_id
        {
            self.bcf.mapping_offered = link.project_id.clone();
            self.bcf.window_open = true;
        }
    }

    /// Takes status and assignee changes made on the server. Returns how
    /// many issues changed.
    fn apply_topics(&mut self, project: &str, topics: &[Topic]) -> usize {
        let mut changed = 0;
        let mapping = self
            .issues
            .bcf
            .as_ref()
            .map(|l| l.mapping.clone())
            .unwrap_or_default();
        for issue in &mut self.issues.list {
            let Some(link) = &mut issue.bcf else { continue };
            if link.project_id != project {
                continue;
            }
            let Some(topic) = topics.iter().find(|t| t.guid == link.topic_guid) else {
                continue;
            };
            let status = topic.topic_status.clone().unwrap_or_default();
            let assigned = topic.assigned_to.clone().unwrap_or_default();
            let mut touched = false;
            // Title and priority: taken from the server unless they were
            // changed here and not sent yet.
            let unsent = link.sent_version != issue.modified;
            if !unsent {
                if !topic.title.trim().is_empty() && topic.title != issue.title {
                    issue.title = topic.title.clone();
                    touched = true;
                }
                if let Some(local) = topic
                    .priority
                    .as_deref()
                    .and_then(|p| local_priority(p, &mapping))
                    && local != issue.priority
                {
                    issue.priority = local;
                    touched = true;
                }
            }
            if status != link.remote_status {
                link.remote_status = status.clone();
                if let Some(local) = mapping
                    .status_from(&status)
                    .or_else(|| local_status(&status))
                    && local != issue.status
                {
                    issue.status = local;
                    touched = true;
                }
            }
            if assigned != link.remote_assigned_to {
                link.remote_assigned_to = assigned.clone();
                // Back to the initials the project uses, when mapped.
                let local = mapping
                    .person_from(&assigned)
                    .map(str::to_owned)
                    .unwrap_or(assigned);
                if local != issue.assigned_to {
                    issue.assigned_to = local;
                    touched = true;
                }
            }
            link.synced = project::now_rfc3339();
            if touched {
                issue.touch();
                // The server's state is what was changed; nothing to send back.
                if let Some(link) = &mut issue.bcf {
                    link.sent_version = issue.modified.clone();
                }
                changed += 1;
            }
        }
        if changed > 0 {
            self.session.dirty = true;
        }
        changed
    }

    /// Sends the given issues (new or changed ones) to the linked project.
    /// Sends the given issues. New topics get a picture of their viewpoint:
    /// the pictures are taken first, then the issues are sent.
    pub fn send_issues(&mut self, guids: &[String], ctx: &egui::Context) {
        if self.issues.bcf.is_none() {
            self.bcf.window_open = true;
            return;
        }
        let new: Vec<String> = guids
            .iter()
            .filter(|g| self.issues.get(g).is_some_and(|i| i.bcf.is_none()))
            .cloned()
            .collect();
        if !new.is_empty() && !self.federation.is_empty() && !self.is_capturing() {
            self.bcf.status = format!("Taking pictures for {} new issues…", new.len());
            self.start_capture(crate::export::ExportKind::Live(guids.to_vec()), new, ctx);
            return;
        }
        self.send_issues_with(guids, &Default::default(), ctx);
    }

    /// Sends the issues; `images` are PNG pictures for new topics.
    pub fn send_issues_with(
        &mut self,
        guids: &[String],
        images: &rustc_hash::FxHashMap<String, Vec<u8>>,
        ctx: &egui::Context,
    ) {
        let Some(link) = self.issues.bcf.clone() else {
            self.bcf.window_open = true;
            return;
        };
        let extensions = (self.bcf.extensions_for == link.project_id)
            .then_some(self.bcf.extensions.as_ref())
            .flatten();
        let outgoing: Vec<Outgoing> = guids
            .iter()
            .filter_map(|g| self.issues.get(g))
            .filter(|i| {
                i.bcf
                    .as_ref()
                    .is_none_or(|b| b.project_id == link.project_id)
            })
            .map(|i| outgoing(i, &link, extensions, self.camera.fov_y))
            .collect();
        if outgoing.is_empty() {
            self.bcf.status = "Nothing to send.".to_owned();
            return;
        }
        // The picture goes with the viewpoint of a new topic.
        let mut outgoing = outgoing;
        for item in &mut outgoing {
            if item.topic_guid.is_none()
                && let (Some(viewpoint), Some(png)) = (&mut item.viewpoint, images.get(&item.issue))
            {
                viewpoint.snapshot = Some(bcf::Snapshot {
                    snapshot_type: "png".to_owned(),
                    snapshot_data: bcf::base64(png),
                });
            }
        }
        // Dalux lists the user's rights in the extensions: without
        // createTopic, new issues cannot be sent, existing ones still updated.
        if let Some(ext) = extensions
            && !ext.may_create_topics()
            && outgoing.iter().any(|o| o.topic_guid.is_none())
        {
            let new = outgoing.iter().filter(|o| o.topic_guid.is_none()).count();
            outgoing.retain(|o| o.topic_guid.is_some());
            let name = if link.project_name.is_empty() {
                &link.project_id
            } else {
                &link.project_name
            };
            self.errors.push(format!(
                "Your user may not create topics in {name} (your rights there: {}). {new} new issues were not sent. Ask the project administrator in Dalux for the right to create issues; issues already on the server can still be updated.",
                ext.project_actions.join(", ")
            ));
            if outgoing.is_empty() {
                self.bcf.status = "No rights to create topics in this project.".to_owned();
                return;
            }
        }
        if let Some(project) = self
            .bcf
            .projects
            .iter()
            .find(|p| p.project_id == link.project_id)
            && !project.may_create_topics()
        {
            let rights = project
                .authorization
                .as_ref()
                .map(|a| a.project_actions.join(", "))
                .unwrap_or_default();
            self.errors.push(format!(
                "The server does not let you create topics in {} (your rights there: {rights}). Ask the project administrator for BCF rights.",
                project.name
            ));
            return;
        }
        self.bcf.status = format!("Sending {} issues…", outgoing.len());
        self.bcf.send(&link.project_id, outgoing, ctx);
    }

    /// Brings the latest from the BCF server: the project's statuses and
    /// people, and every change made to the topics there.
    pub fn sync_bcf(&mut self, ctx: &egui::Context) {
        let Some(link) = &self.issues.bcf else {
            return;
        };
        let project = link.project_id.clone();
        self.bcf.status = "Syncing with the server…".to_owned();
        self.bcf.extensions_for = project.clone();
        self.bcf.load_extensions(&project, ctx);
        self.bcf.fetch_topics(&project, ctx);
        // Comments of the issue that is open.
        if let Some(topic) = self
            .issues
            .selected
            .as_ref()
            .and_then(|g| self.issues.get(g))
            .and_then(|i| i.bcf.as_ref())
            .map(|b| b.topic_guid.clone())
        {
            self.bcf.fetch_comments(&project, &topic, ctx);
        }
    }
}

/// The BCF Live window: server, sign-in, project and responsible people.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.bcf.window_open {
        return;
    }
    let mut open = true;
    // Never taller than the app window; the contents scroll instead, and the
    // window can be made smaller.
    let screen = ctx.content_rect();
    egui::Window::new(RichText::new(format!("{}  BCF Live", icon::CLOUD)).strong())
        .open(&mut open)
        .default_width(480.0)
        .default_height((screen.height() * 0.75).min(720.0))
        .max_height(screen.height() - 40.0)
        .default_pos(screen.center_top() + egui::vec2(-240.0, 40.0))
        .constrain(true)
        .resizable(true)
        .collapsible(false)
        .vscroll(true)
        .show(ctx, |ui| contents(app, ui));
    app.bcf.window_open = open;
}

fn contents(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    ui.label(
        RichText::new("SERVER")
            .small()
            .strong()
            .color(theme::text_muted()),
    );
    let mut account_changed = false;
    egui::Grid::new("bcf_server")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let account = app.bcf.account_mut();
            ui.label("Name");
            account_changed |= ui.text_edit_singleline(&mut account.name).changed();
            ui.end_row();
            ui.label("Server");
            ui.label("Dalux")
                .on_hover_text("More BCF servers can follow; Dalux comes first.");
            ui.end_row();
            ui.label("Url");
            account_changed |= ui
                .add(egui::TextEdit::singleline(&mut account.server).desired_width(320.0))
                .on_hover_text(
                    "The BCF address Dalux gives you, e.g. https://node1.field.dalux.com/service/",
                )
                .changed();
            ui.end_row();
        });
    egui::CollapsingHeader::new(RichText::new("Advanced").small())
        .id_salt("bcf_advanced")
        .show(ui, |ui| {
            let account = app.bcf.account_mut();
            egui::Grid::new("bcf_advanced_grid")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Client id");
                    account_changed |= ui.text_edit_singleline(&mut account.client_id).changed();
                    ui.end_row();
                    ui.label("Client secret");
                    account_changed |= ui
                        .add(egui::TextEdit::singleline(&mut account.client_secret).password(true))
                        .changed();
                    ui.end_row();
                    ui.label("Login port");
                    account_changed |= ui
                        .add(egui::DragValue::new(&mut account.redirect_port).range(1024..=65535))
                        .on_hover_text("The browser returns to http://localhost:<port>/callback after the login")
                        .changed();
                    ui.end_row();
                    ui.label("PKCE");
                    account_changed |= ui
                        .checkbox(&mut account.pkce, "Send a code challenge")
                        .on_hover_text("Only for servers that require PKCE; Dalux does not accept it")
                        .changed();
                    ui.end_row();
                });
            ui.label(
                RichText::new(
                    "Only change these if the server owner gave you a client id for OpenCheck.",
                )
                .small()
                .color(theme::text_muted()),
            );
        });
    if account_changed {
        app.bcf.save_accounts();
    }

    ui.horizontal(|ui| {
        let signed_in = app.bcf.is_signed_in();
        if app.bcf.signing_in {
            ui.spinner();
            ui.label("Log in in the browser window that opened…");
        } else if ui
            .button(format!("{}  Authenticate", icon::SIGN_IN))
            .on_hover_text("Opens the server's login page in your browser")
            .clicked()
        {
            app.bcf.sign_in(&ctx);
        }
        if signed_in {
            let user = app.bcf.account().user.clone();
            ui.label(
                RichText::new(if user.is_empty() {
                    "Signed in".to_owned()
                } else {
                    format!("Signed in as {user}")
                })
                .color(crate::theme::solved()),
            );
            if ui.small_button("Sign out").clicked() {
                app.bcf.sign_out();
            }
        }
    });
    if !app.bcf.status.is_empty() {
        ui.label(
            RichText::new(&app.bcf.status)
                .small()
                .color(theme::text_muted()),
        );
    }
    let log = crate::bcf_live::log_file();
    if log.exists()
        && ui
            .small_button(format!("{}  Open connection log", icon::FILE_TEXT))
            .on_hover_text(format!(
                "Every request to the server and its answer, without passwords or tokens:\n{}",
                log.display()
            ))
            .clicked()
    {
        let _ = webbrowser::open(&log.to_string_lossy());
    }

    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("PROJECTS")
                .small()
                .strong()
                .color(theme::text_muted()),
        );
        if app.bcf.is_signed_in()
            && ui
                .add_enabled(
                    !app.bcf.is_busy(),
                    egui::Button::new(icon::ARROWS_CLOCKWISE).small(),
                )
                .on_hover_text("Load the project list")
                .clicked()
        {
            app.bcf.load_projects(&ctx);
        }
        if app.bcf.is_busy() {
            ui.spinner();
        }
    });
    let linked = app.issues.bcf.as_ref().map(|l| l.project_id.clone());
    let mut choose = None;
    egui::ScrollArea::vertical()
        .id_salt("bcf_projects")
        .max_height(160.0)
        .show(ui, |ui| {
            if app.bcf.projects.is_empty() {
                let text = if app.bcf.is_signed_in() {
                    "Press ↻ to load your projects."
                } else {
                    "Authenticate to see your projects."
                };
                ui.label(RichText::new(text).color(theme::text_muted()));
            }
            for project in &app.bcf.projects {
                let selected = linked.as_deref() == Some(project.project_id.as_str());
                let name = if project.name.is_empty() {
                    &project.project_id
                } else {
                    &project.name
                };
                if ui
                    .add(egui::Button::selectable(selected, name.as_str()))
                    .on_hover_text(match &project.authorization {
                        Some(a) if !a.project_actions.is_empty() => format!(
                            "Share this project's issues through this BCF project
Your rights: {}",
                            a.project_actions.join(", ")
                        ),
                        _ => "Share this project's issues through this BCF project".to_owned(),
                    })
                    .clicked()
                {
                    choose = Some(project.clone());
                }
            }
        });
    if let Some(project) = choose {
        let server = app.bcf.account().server.clone();
        let assignees = app
            .issues
            .bcf
            .as_ref()
            .filter(|l| l.project_id == project.project_id)
            .map(|l| l.assignees.clone())
            .unwrap_or_default();
        let mapping = app
            .issues
            .bcf
            .as_ref()
            .filter(|l| l.project_id == project.project_id)
            .map(|l| l.mapping.clone())
            .unwrap_or_default();
        app.issues.bcf = Some(BcfProjectLink {
            server,
            project_id: project.project_id.clone(),
            project_name: project.name.clone(),
            assignees,
            mapping,
        });
        app.bcf.extensions = None;
        app.bcf.extensions_for = project.project_id.clone();
        app.bcf.load_extensions(&project.project_id, &ctx);
        app.session.dirty = true;
    }

    let Some(link) = app.issues.bcf.clone() else {
        return;
    };
    ui.separator();
    // The user's rights in the linked project, as the server lists them.
    if app.bcf.extensions_for == link.project_id
        && let Some(ext) = &app.bcf.extensions
        && !ext.project_actions.is_empty()
    {
        if ext.may_create_topics() {
            ui.label(
                RichText::new(format!(
                    "{}  You may create topics here",
                    icon::CHECK_CIRCLE
                ))
                .small()
                .color(theme::solved()),
            );
        } else {
            ui.label(
                RichText::new(format!(
                    "{}  Your user may not create topics in this project (rights: {}). Ask the project administrator in Dalux.",
                    icon::WARNING,
                    ext.project_actions.join(", ")
                ))
                .small()
                .color(theme::danger()),
            );
        }
    }
    mapping_editor(app, ui, &link);
    ui.separator();
    ui.label(
        RichText::new("RESPONSIBLE PER DISCIPLINE")
            .small()
            .strong()
            .color(theme::text_muted()),
    );
    ui.label(
        RichText::new(
            "Issues without their own assignee go to the person of their first responsible discipline.",
        )
        .small()
        .color(theme::text_muted()),
    );
    let users: Vec<String> = app.bcf.users().to_vec();
    let mut disciplines: Vec<Discipline> =
        app.federation.iter().map(|(_, m)| m.discipline).collect();
    disciplines.extend(link.assignees.iter().map(|a| a.discipline));
    disciplines.sort_by_key(|d| *d as u8);
    disciplines.dedup();
    let mut changed = None;
    egui::Grid::new("bcf_assignees")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for d in disciplines {
                ui.label(format!("{} ({})", d.label(), d.code()));
                let current = link.assignee_for(d).unwrap_or("").to_owned();
                let mut value = current.clone();
                user_picker(ui, ("assignee", d as u8), &mut value, &users);
                ui.end_row();
                if value != current {
                    changed = Some((d, value));
                }
            }
        });
    if let Some((discipline, user)) = changed
        && let Some(link) = &mut app.issues.bcf
    {
        link.assignees.retain(|a| a.discipline != discipline);
        if !user.is_empty() {
            link.assignees.push(DisciplineAssignee { discipline, user });
        }
        app.session.dirty = true;
    }
    if users.is_empty() && app.bcf.is_signed_in() {
        ui.label(
            RichText::new("The project's people are loading, or the server lists none.")
                .small()
                .color(theme::text_muted()),
        );
    }
    ui.add_space(4.0);
    if ui
        .add_enabled(
            app.bcf.is_signed_in() && !app.bcf.is_busy(),
            egui::Button::new(format!("{}  Sync with the server", icon::ARROWS_CLOCKWISE)),
        )
        .on_hover_text(
            "Bring over the latest statuses, assignees, titles, priorities, people and comments",
        )
        .clicked()
    {
        app.sync_bcf(&ctx);
    }
    if ui
        .small_button(format!(
            "{}  Stop sharing with this project",
            icon::LINK_BREAK
        ))
        .clicked()
    {
        app.issues.bcf = None;
        app.session.dirty = true;
    }
}

/// The value mapping: OpenCheck's statuses, criticality, type, phase and
/// people paired with the values the server offers.
fn mapping_editor(app: &mut OpenCheckApp, ui: &mut egui::Ui, link: &BcfProjectLink) {
    let Some(ext) = app
        .bcf
        .extensions
        .clone()
        .filter(|_| app.bcf.extensions_for == link.project_id)
    else {
        ui.label(
            RichText::new("VALUE MAPPING")
                .small()
                .strong()
                .color(theme::text_muted()),
        );
        ui.label(
            RichText::new("The project's values are loaded after signing in.")
                .small()
                .color(theme::text_muted()),
        );
        return;
    };
    let mut mapping = link.mapping.clone();
    let id = ui.make_persistent_id(("bcf_mapping", &link.project_id));
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        !mapping.confirmed,
    );
    if !mapping.confirmed {
        state.set_open(true);
    }
    state
        .show_header(ui, |ui| {
            ui.label(
                RichText::new("VALUE MAPPING")
                    .small()
                    .strong()
                    .color(theme::text_muted()),
            );
            if mapping.confirmed {
                ui.label(RichText::new(icon::CHECK).small().color(theme::solved()))
                    .on_hover_text("Saved");
            } else {
                ui.label(
                    RichText::new("pair the values before sending")
                        .small()
                        .color(crate::checking_ui::severity_color(Severity::Moderate)),
                );
            }
        })
        .body(|ui| {
            ui.label(
                RichText::new(
                    "Each server names statuses, priorities and people its own way. Choose the \
                     server's value for each of OpenCheck's; Automatic matches by name.",
                )
                .small()
                .color(theme::text_muted()),
            );
            egui::Grid::new(("mapping_grid", &link.project_id))
                .num_columns(2)
                .spacing([12.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("OpenCheck").small().strong());
                    ui.label(RichText::new("Server").small().strong());
                    ui.end_row();

                    mapping_section(ui, "Status");
                    for status in IssueStatus::ALL {
                        ui.label(status.label());
                        let current = mapping.status_for(status).unwrap_or("").to_owned();
                        let auto = auto_status(status, &ext);
                        if let Some(value) = value_combo(
                            ui,
                            ("map_status", status.label()),
                            &current,
                            auto.as_deref(),
                            &ext.topic_status,
                        ) {
                            mapping.set_status(status, value);
                        }
                        ui.end_row();
                    }

                    mapping_section(ui, "Criticality");
                    for priority in crate::c402::LEVELS {
                        ui.label(crate::c402::label(priority));
                        let current = mapping.priority_for(priority).unwrap_or("").to_owned();
                        let auto = auto_priority(priority, &ext);
                        if let Some(value) = value_combo(
                            ui,
                            ("map_priority", crate::c402::code(priority)),
                            &current,
                            auto.as_deref(),
                            &ext.priority,
                        ) {
                            mapping.set_priority(priority, value);
                        }
                        ui.end_row();
                    }

                    mapping_section(ui, "Topic");
                    ui.label("Type");
                    let auto = auto_topic_type(&ext);
                    if let Some(value) = value_combo(
                        ui,
                        "map_type",
                        &mapping.topic_type,
                        auto.as_deref(),
                        &ext.topic_type,
                    ) {
                        mapping.topic_type = value;
                    }
                    ui.end_row();
                    if !ext.stage.is_empty() {
                        ui.label("Phase / stage");
                        if let Some(value) =
                            value_combo(ui, "map_stage", &mapping.stage, None, &ext.stage)
                        {
                            mapping.stage = value;
                        }
                        ui.end_row();
                    }

                    let mut people: Vec<String> = app.issues.people.clone();
                    for m in &mapping.people {
                        if !people.iter().any(|p| p.eq_ignore_ascii_case(&m.from)) {
                            people.push(m.from.clone());
                        }
                    }
                    if !people.is_empty() && !ext.user_id_type.is_empty() {
                        mapping_section(ui, "People");
                        for person in people {
                            ui.label(&person);
                            let current = mapping.person_for(&person).unwrap_or("").to_owned();
                            let auto = auto_person(&person, &ext.user_id_type);
                            if let Some(value) = value_combo(
                                ui,
                                ("map_person", &person),
                                &current,
                                auto.as_deref(),
                                &ext.user_id_type,
                            ) {
                                mapping.set_person(person.clone(), value);
                            }
                            ui.end_row();
                        }
                    }
                });
            ui.horizontal(|ui| {
                let label = if mapping.confirmed {
                    "Mapping saved"
                } else {
                    "Save mapping"
                };
                if ui
                    .add_enabled(
                        !mapping.confirmed,
                        egui::Button::new(format!("{}  {label}", icon::CHECK)),
                    )
                    .clicked()
                {
                    mapping.confirmed = true;
                }
                if ui
                    .small_button("Reset to automatic")
                    .on_hover_text("Forget the pairing; values are matched by name again")
                    .clicked()
                {
                    mapping = Default::default();
                }
            });
        });
    if mapping != link.mapping
        && let Some(stored) = &mut app.issues.bcf
    {
        // A changed pairing must be saved again, so it is looked at.
        if link.mapping.confirmed && mapping.confirmed {
            mapping.confirmed = false;
        }
        stored.mapping = mapping;
        app.session.dirty = true;
    }
}

fn mapping_section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).small().family(theme::bold()));
    ui.label("");
    ui.end_row();
}

/// A choice among the server's values; empty means automatic. Returns the
/// new value when it changed.
fn value_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    auto: Option<&str>,
    options: &[String],
) -> Option<String> {
    let automatic = match auto {
        Some(a) => format!("Automatic ({a})"),
        None => "Not sent".to_owned(),
    };
    let shown = if current.is_empty() {
        automatic.clone()
    } else {
        current.to_owned()
    };
    let mut chosen = current.to_owned();
    egui::ComboBox::from_id_salt(id)
        .selected_text(RichText::new(shown).small())
        .width(240.0)
        .truncate()
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, String::new(), automatic);
            for option in options {
                ui.selectable_value(&mut chosen, option.clone(), option);
            }
        });
    (chosen != current).then_some(chosen)
}

/// A person from the server's list, or typed in when the list is empty.
pub fn user_picker(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    users: &[String],
) -> bool {
    let before = value.clone();
    if users.is_empty() {
        ui.add(
            egui::TextEdit::singleline(value)
                .hint_text("e-mail")
                .desired_width(220.0),
        );
    } else {
        let shown = if value.is_empty() {
            "Nobody"
        } else {
            value.as_str()
        };
        egui::ComboBox::from_id_salt(id)
            .selected_text(shown.to_owned())
            .width(220.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(value, String::new(), "Nobody");
                for user in users {
                    ui.selectable_value(value, user.clone(), user);
                }
            });
    }
    *value != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_both_ways() {
        let offered = vec![
            "Åben".to_owned(),
            "Igangværende".to_owned(),
            "Lukket".to_owned(),
        ];
        assert_eq!(
            remote_status(IssueStatus::Open, &offered).as_deref(),
            Some("Åben")
        );
        assert_eq!(
            remote_status(IssueStatus::Closed, &offered).as_deref(),
            Some("Lukket")
        );
        assert_eq!(remote_status(IssueStatus::Resolved, &offered), None);
        assert_eq!(local_status("closed"), Some(IssueStatus::Closed));
        assert_eq!(local_status("Igangværende"), Some(IssueStatus::InProgress));
        assert_eq!(local_status("Something else"), None);
    }

    #[test]
    fn initials_find_their_server_person_and_the_mapping_wins() {
        let users = vec![
            "TEST IKT-leder".to_owned(),
            "xyz@example.com | BIM manager".to_owned(),
            "abc@example.com | Example".to_owned(),
        ];
        assert_eq!(
            auto_person("ABC", &users).as_deref(),
            Some("abc@example.com | Example")
        );
        assert_eq!(auto_person("QQQ", &users), None);

        let mut mapping = crate::issues::BcfMapping::default();
        mapping.set_status(IssueStatus::Closed, "Lukkede".into());
        mapping.set_person("XYZ".into(), users[1].clone());
        assert_eq!(mapping.status_for(IssueStatus::Closed), Some("Lukkede"));
        assert_eq!(mapping.status_from("Lukkede"), Some(IssueStatus::Closed));
        assert_eq!(mapping.person_for("xyz"), Some(users[1].as_str()));
        assert_eq!(mapping.person_from(&users[1]), Some("XYZ"));
        // An empty choice means automatic again.
        mapping.set_status(IssueStatus::Closed, String::new());
        assert_eq!(mapping.status_for(IssueStatus::Closed), None);
    }
}
