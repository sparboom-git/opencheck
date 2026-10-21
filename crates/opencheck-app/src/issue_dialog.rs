//! The dialog that opens when findings become issues: the issue's fields
//! are set right away instead of afterwards.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;
use opencheck_core::check::Severity;

use crate::app::{IssueTarget, OpenCheckApp};
use crate::c402;
use crate::issues::{IssueStatus, remember_person};
use crate::theme;

/// Fields for issues that are about to be created.
pub struct IssueDraft {
    pub findings: Vec<String>,
    pub target: IssueTarget,
    /// Empty: the automatic title.
    pub title: String,
    pub priority: Severity,
    pub status: IssueStatus,
    pub responsible: Vec<Discipline>,
    pub assigned_to: String,
    pub description: String,
}

impl OpenCheckApp {
    /// Opens the dialog for new issues from findings. Adding to an existing
    /// issue needs no new fields and happens at once.
    pub fn request_issues(&mut self, findings: Vec<String>, target: IssueTarget) {
        if matches!(target, IssueTarget::Existing(_)) {
            self.add_issues(&findings, target);
            return;
        }
        let mut priority = Severity::Low;
        let mut responsible = Vec::new();
        for id in &findings {
            if let Some((_, rule, f)) = self.checking.find(id) {
                priority = priority.min(rule.rule.severity);
                for model in [f.a.model, f.b.model] {
                    if let Some(m) = self.federation.get(model)
                        && !responsible.contains(&m.discipline)
                    {
                        responsible.push(m.discipline);
                    }
                }
            }
        }
        self.issue_draft = Some(IssueDraft {
            findings,
            target,
            title: String::new(),
            priority,
            status: IssueStatus::Open,
            responsible,
            assigned_to: String::new(),
            description: String::new(),
        });
    }

    /// Creates the issues of the draft with its fields.
    fn create_from_draft(&mut self, draft: IssueDraft) {
        let before: Vec<String> = self.issues.list.iter().map(|i| i.guid.clone()).collect();
        let Some(last) = self.add_issues(&draft.findings, draft.target.clone()) else {
            return;
        };
        let created: Vec<String> = self
            .issues
            .list
            .iter()
            .map(|i| i.guid.clone())
            .filter(|g| !before.contains(g))
            .collect();
        let one = created.len() == 1;
        for guid in &created {
            let Some(issue) = self.issues.get_mut(guid) else {
                continue;
            };
            // A typed title names a single issue; separate issues keep their own.
            if one && !draft.title.trim().is_empty() {
                issue.title = draft.title.trim().to_owned();
            }
            issue.priority = draft.priority;
            issue.status = draft.status;
            issue.responsible = draft.responsible.clone();
            issue.assigned_to = draft.assigned_to.trim().to_owned();
            if !draft.description.trim().is_empty() {
                issue.description = draft.description.trim().to_owned();
            }
            issue.touch();
        }
        remember_person(&mut self.issues.people, &draft.assigned_to);
        self.session.dirty = true;
        self.show_issue(&last);
    }
}

/// The dialog; draws nothing when no issues are being created.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let Some(mut draft) = app.issue_draft.take() else {
        return;
    };
    let mut disciplines: Vec<Discipline> =
        app.federation.iter().map(|(_, m)| m.discipline).collect();
    disciplines.extend(draft.responsible.iter().copied());
    disciplines.sort_by_key(|d| *d as u8);
    disciplines.dedup();
    let mut suggestions = app.issues.people.clone();
    for user in app.bcf.users() {
        if !suggestions.iter().any(|s| s.eq_ignore_ascii_case(user)) {
            suggestions.push(user.clone());
        }
    }
    let count = draft.findings.len();
    let heading = match (&draft.target, count) {
        (IssueTarget::Separate, n) if n > 1 => format!("New issues ({n})"),
        (IssueTarget::Bundle, n) if n > 1 => format!("New issue with {n} findings"),
        _ => "New issue".to_owned(),
    };

    let mut create = false;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("issue_draft")).show(ctx, |ui| {
        ui.set_width(440.0);
        ui.heading(RichText::new(format!("{}  {heading}", icon::FLAG)).strong());
        ui.add_space(6.0);
        egui::Grid::new("draft_fields")
            .num_columns(2)
            .spacing([10.0, 8.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Title").color(theme::text_muted()));
                let separate = matches!(draft.target, IssueTarget::Separate) && count > 1;
                ui.add_enabled(
                    !separate,
                    egui::TextEdit::singleline(&mut draft.title)
                        .hint_text(if separate {
                            "Each issue gets its own title"
                        } else {
                            "Automatic, e.g. \"Duct vs Beam\""
                        })
                        .desired_width(300.0),
                );
                ui.end_row();

                ui.label(RichText::new("Criticality").color(theme::text_muted()));
                ui.horizontal(|ui| {
                    for p in c402::LEVELS {
                        let on = draft.priority == p;
                        let text = RichText::new(c402::code(p)).strong();
                        let text = if on {
                            text.color(theme::on_accent())
                        } else {
                            text
                        };
                        let mut button = egui::Button::new(text);
                        if on {
                            button = button.fill(crate::checking_ui::severity_color(p));
                        }
                        if ui
                            .add(button)
                            .on_hover_text(format!("{}\n{}", c402::label(p), c402::explanation(p)))
                            .clicked()
                        {
                            draft.priority = p;
                        }
                    }
                    ui.label(
                        RichText::new(
                            c402::label(draft.priority)
                                .split('–')
                                .nth(1)
                                .unwrap_or("")
                                .trim(),
                        )
                        .small()
                        .color(theme::text_muted()),
                    );
                });
                ui.end_row();

                ui.label(RichText::new("Status").color(theme::text_muted()));
                egui::ComboBox::from_id_salt("draft_status")
                    .selected_text(draft.status.label())
                    .show_ui(ui, |ui| {
                        for s in IssueStatus::ALL {
                            ui.selectable_value(&mut draft.status, s, s.label());
                        }
                    });
                ui.end_row();

                ui.label(RichText::new("Responsible").color(theme::text_muted()));
                ui.horizontal_wrapped(|ui| {
                    for d in &disciplines {
                        let on = draft.responsible.contains(d);
                        let text = RichText::new(d.code()).color(if on {
                            theme::discipline_color(*d)
                        } else {
                            theme::text_muted()
                        });
                        let text = if on { text.strong() } else { text };
                        if ui
                            .add(egui::Button::selectable(on, text))
                            .on_hover_text(d.label())
                            .clicked()
                        {
                            if on {
                                draft.responsible.retain(|x| x != d);
                            } else {
                                draft.responsible.push(*d);
                            }
                        }
                    }
                });
                ui.end_row();

                ui.label(RichText::new("Assigned to").color(theme::text_muted()));
                ui.horizontal(|ui| {
                    crate::issues_ui::person_picker(
                        ui,
                        "draft_assigned",
                        &mut draft.assigned_to,
                        &suggestions,
                    );
                });
                ui.end_row();
            });
        ui.label(RichText::new("Description").color(theme::text_muted()));
        ui.add(
            egui::TextEdit::multiline(&mut draft.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("What must be done, and by whom"),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = match (&draft.target, count) {
                (IssueTarget::Separate, n) if n > 1 => format!("Create {n} issues"),
                _ => "Create issue".to_owned(),
            };
            let button = egui::Button::new(
                RichText::new(format!("{}  {label}", icon::CHECK))
                    .strong()
                    .color(theme::on_accent()),
            )
            .fill(theme::accent());
            if ui.add(button).clicked() {
                create = true;
            }
            if ui.button("Cancel").clicked() {
                cancel = true;
            }
        });
    });
    if modal.should_close() {
        cancel = true;
    }
    if create {
        app.create_from_draft(draft);
    } else if !cancel {
        app.issue_draft = Some(draft);
    }
}
