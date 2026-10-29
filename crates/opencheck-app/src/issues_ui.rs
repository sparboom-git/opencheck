//! The Issues list in the Check tab: findings picked out for the report.
//! An issue can bundle several findings; issues can be merged.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;

use crate::app::OpenCheckApp;
use crate::c402;
use crate::checking_ui::{FOCUS_A, FOCUS_B, dot, severity_color};
use crate::issues::{ComponentRole, IssueStatus};
use crate::results_ui::class_label;
use crate::theme;

pub fn issues(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    bcf_bar(app, ui);
    if !app.issues.list.is_empty() {
        export_bar(app, ui);
    }
    if app.issues.list.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(icon::FLAG).size(36.0).color(theme::border()));
            ui.label(RichText::new("No issues yet").strong());
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Tick findings under Results and press \"Add as one issue\". \
                     Issues are the findings that go into the report.",
                )
                .color(theme::text_muted()),
            );
        });
        return;
    }

    let mut counts = [0usize; 4];
    for issue in &app.issues.list {
        counts[IssueStatus::ALL
            .iter()
            .position(|s| *s == issue.status)
            .unwrap_or(0)] += 1;
    }
    ui.label(
        RichText::new(
            IssueStatus::ALL
                .iter()
                .zip(counts)
                .map(|(s, n)| format!("{n} {}", s.label().to_lowercase()))
                .collect::<Vec<_>>()
                .join(" · "),
        )
        .small()
        .color(theme::text_muted()),
    );
    let solved = app
        .issues
        .list
        .iter()
        .filter(|i| i.solved_by_update)
        .count();
    if solved > 0 {
        ui.label(
            RichText::new(format!(
                "{} {solved} solved by model updates",
                icon::CHECK_CIRCLE
            ))
            .small()
            .color(theme::solved()),
        );
    }

    // Merging: tick two or more issues.
    let mut merge = false;
    if app.issues.marked.len() >= 2 {
        egui::Frame::new()
            .fill(theme::accent_soft())
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("{} selected", app.issues.marked.len())).strong());
                    let button = egui::Button::new(
                        RichText::new(format!("{}  Merge into one issue", icon::STACK))
                            .strong()
                            .color(theme::on_accent()),
                    )
                    .fill(theme::accent());
                    if ui
                        .add(button)
                        .on_hover_text("The first ticked issue keeps its title; findings and components are combined")
                        .clicked()
                    {
                        merge = true;
                    }
                    if ui.button("Clear").clicked() {
                        app.issues.marked.clear();
                    }
                });
            });
    } else {
        ui.label(
            RichText::new("Tick two or more issues to merge them")
                .small()
                .color(theme::text_muted()),
        );
    }
    ui.add_space(4.0);

    let mut show = None;
    let mut toggle = None;
    let detail_height = if app.issues.selected.is_some() {
        360.0
    } else {
        0.0
    };
    egui::ScrollArea::vertical()
        .id_salt("issue_list")
        .max_height((ui.available_height() - detail_height).max(100.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for (index, issue) in app.issues.list.iter().enumerate() {
                let selected = app.issues.selected.as_deref() == Some(issue.guid.as_str());
                ui.horizontal(|ui| {
                    let mut ticked = app.issues.marked.contains(&issue.guid);
                    if ui.checkbox(&mut ticked, "").changed() {
                        toggle = Some((issue.guid.clone(), ticked));
                    }
                    dot(ui, severity_color(issue.priority));
                    ui.label(
                        RichText::new(c402::code(issue.priority))
                            .small()
                            .strong()
                            .color(severity_color(issue.priority)),
                    )
                    .on_hover_text(c402::label(issue.priority));
                    ui.label(
                        RichText::new(format!("{}", index + 1))
                            .small()
                            .color(theme::text_muted()),
                    );
                    let mut title = RichText::new(&issue.title);
                    if matches!(issue.status, IssueStatus::Resolved | IssueStatus::Closed) {
                        title = title.color(theme::text_muted());
                    }
                    if selected {
                        title = title.family(theme::bold());
                    }
                    // Leave room for the badges after the title; a long title is cut.
                    let response = crate::results_ui::bounded(ui, 150.0, |ui| {
                        ui.add(
                            egui::Button::selectable(selected, title)
                                .frame_when_inactive(false)
                                .truncate(),
                        )
                    })
                    .on_hover_text(&issue.title);
                    if let Some(link) = &issue.bcf {
                        let (glyph, tip) = if issue.needs_sending() {
                            (
                                icon::CLOUD_ARROW_UP,
                                "Changed since it was sent to the BCF server",
                            )
                        } else {
                            (icon::CLOUD_CHECK, "Up to date on the BCF server")
                        };
                        ui.label(RichText::new(glyph).small().color(theme::accent()))
                            .on_hover_text(format!("{tip}\nStatus there: {}", link.remote_status));
                    }
                    if !issue.responsible.is_empty() {
                        ui.label(
                            RichText::new(
                                issue
                                    .responsible
                                    .iter()
                                    .map(|d| d.code())
                                    .collect::<Vec<_>>()
                                    .join(" "),
                            )
                            .small()
                            .color(theme::text_muted()),
                        )
                        .on_hover_text("Responsible disciplines");
                    }
                    if issue.findings.len() > 1 {
                        ui.label(
                            RichText::new(format!("{} {}", icon::STACK, issue.findings.len()))
                                .small()
                                .color(theme::text_muted()),
                        )
                        .on_hover_text("Findings bundled in this issue");
                    }
                    if issue.solved_by_update {
                        solved_badge(ui);
                    } else {
                        let partly = issue.solved_count();
                        if partly > 0 {
                            ui.label(
                                RichText::new(format!(
                                    "{} {partly}/{}",
                                    icon::CHECK_CIRCLE,
                                    issue.findings.len()
                                ))
                                .small()
                                .color(theme::solved()),
                            )
                            .on_hover_text("Findings solved by model updates");
                        }
                        ui.label(
                            RichText::new(issue.status.label())
                                .small()
                                .color(theme::text_muted()),
                        );
                    }
                    if response.clicked() {
                        show = Some(issue.guid.clone());
                    }
                    if selected {
                        crate::results_ui::scroll_once(ui, &response, "issues", &issue.guid);
                    }
                });
            }
        });
    if let Some((guid, on)) = toggle {
        if on {
            app.issues.marked.push(guid);
        } else {
            app.issues.marked.retain(|m| *m != guid);
        }
    }
    if merge {
        let marked = app.issues.marked.clone();
        if let Some(guid) = app.issues.merge(&marked) {
            app.session.dirty = true;
            app.show_issue(&guid);
        }
    }
    if let Some(guid) = show {
        app.show_issue(&guid);
    }

    if let Some(guid) = app.issues.selected.clone() {
        ui.separator();
        issue_detail(app, ui, &guid);
    }
}

/// BCF file, Excel and report export of the ticked issues, or all.
fn export_bar(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let count = app.export_scope().len();
    let scope = if app.issues.marked.is_empty() {
        format!("all {count} issues")
    } else {
        format!("{count} ticked issues")
    };
    ui.horizontal(|ui| {
        let busy = app.is_capturing();
        ui.add_enabled_ui(!busy, |ui| {
            ui.menu_button(format!("{}  Export", icon::EXPORT), |ui| {
                ui.set_min_width(240.0);
                ui.label(RichText::new(format!("Exports {scope}")).small().color(theme::text_muted()));
                if ui
                    .button(format!("{}  BCF file (.bcfzip)…", icon::FILE_ZIP))
                    .on_hover_text("For Solibri, BIMcollab, Dalux and other BCF tools; with pictures and viewpoints")
                    .clicked()
                {
                    ui.close();
                    app.export_bcf_dialog(&ctx);
                }
                if ui
                    .button(format!("{}  Excel (.xlsx)…", icon::FILE_XLS))
                    .on_hover_text(
                        "One row per issue with its fields, and the elements with their \
                         Revit IDs and IFC GlobalIds",
                    )
                    .clicked()
                {
                    ui.close();
                    app.export_excel_dialog(&ctx);
                }
                if ui
                    .button(format!("{}  Report…", icon::FILE_TEXT))
                    .on_hover_text("An illustrated report (HTML, printable to PDF)")
                    .clicked()
                {
                    ui.close();
                    app.report_window = true;
                }
            });
        });
        if busy {
            ui.spinner();
            ui.label(RichText::new("Taking pictures…").small().color(theme::text_muted()));
        } else {
            ui.label(RichText::new(scope).small().color(theme::text_muted()));
        }
    });
}

/// Header fields of the report and the button that writes it.
pub fn report_window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.report_window {
        return;
    }
    let mut open = true;
    let mut export = false;
    let count = app.export_scope().len();
    let dialog = crate::dialog::Dialog {
        id: "issue_report",
        icon: icon::FILE_TEXT,
        title: "Issue report",
        size: egui::vec2(480.0, 640.0),
        min: egui::vec2(340.0, 260.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        ui.label(
                RichText::new(
                    "Documentation of the consistency and clash check: introduction, models and software, then each issue with picture, criticality (C1–C3), responsible disciplines and the GlobalIds of its objects. The report is in Danish.",
                )
                .small()
                .color(theme::text_muted()),
            );
        ui.add_space(6.0);
        let r = &mut app.issues.report;
        let before = r.clone();
        egui::Grid::new("report_fields")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for (label, value, hint) in [
                    ("Project", &mut r.project, "Projektnavn"),
                    ("Prepared by", &mut r.author, "Name or initials"),
                    ("Company", &mut r.company, "Firmanavn"),
                    ("Revision", &mut r.revision, "e.g. A, 01"),
                    ("Disciplines", &mut r.disciplines, "Own discipline(s)"),
                    ("Phase", &mut r.phase, "e.g. Projektforslag"),
                ] {
                    ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(value)
                            .hint_text(hint)
                            .desired_width(280.0),
                    );
                    ui.end_row();
                }
            });
        ui.label("How the check was done (besides OpenCheck)");
        ui.add(
            egui::TextEdit::multiline(&mut r.method)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
                .hint_text("e.g. visual review of the combined model"),
        );
        ui.label("General remarks");
        ui.add(
            egui::TextEdit::multiline(&mut r.remarks)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("Project conditions, tolerances, areas left out …"),
        );
        if *r != before {
            app.session.dirty = true;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let scope = if app.issues.marked.is_empty() {
                format!("All {count} issues")
            } else {
                format!("{count} ticked issues")
            };
            ui.label(RichText::new(scope).color(theme::text_muted()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = egui::Button::new(
                    RichText::new(format!("{}  Export report…", icon::EXPORT))
                        .strong()
                        .color(theme::on_accent()),
                )
                .fill(theme::accent());
                if ui.add_enabled(count > 0, button).clicked() {
                    export = true;
                }
            });
        });
    });
    app.report_window = open && !export;
    if export {
        app.export_report_dialog(ctx);
    }
}

/// Connection state and the send / fetch buttons for the BCF server.
fn bcf_bar(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    egui::Frame::new()
        .fill(theme::surface())
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let Some(link) = app.issues.bcf.clone() else {
                    ui.label(RichText::new(icon::CLOUD).color(theme::text_muted()));
                    if ui
                        .button("Connect to Dalux…")
                        .on_hover_text("Share issues with a BCF server so the responsible people get them there")
                        .clicked()
                    {
                        app.bcf.window_open = true;
                    }
                    return;
                };
                let server = app.bcf.account().name.clone();
                if ui
                    .add(
                        egui::Button::new(RichText::new(format!(
                            "{}  {server}: {}",
                            icon::CLOUD,
                            if link.project_name.is_empty() {
                                &link.project_id
                            } else {
                                &link.project_name
                            }
                        )))
                        .frame(false)
                        .truncate(),
                    )
                    .on_hover_text("BCF Live settings")
                    .clicked()
                {
                    app.bcf.window_open = true;
                }
                if !app.bcf.is_signed_in() {
                    if ui.button("Authenticate").clicked() {
                        app.bcf.window_open = true;
                        app.bcf.sign_in(&ctx);
                    }
                    return;
                }
                // Ticked issues, or else every issue that is new or changed.
                let targets: Vec<String> = if app.issues.marked.is_empty() {
                    app.issues
                        .list
                        .iter()
                        .filter(|i| i.needs_sending())
                        .map(|i| i.guid.clone())
                        .collect()
                } else {
                    app.issues.marked.clone()
                };
                let busy = app.bcf.is_busy();
                let label = if app.issues.marked.is_empty() {
                    format!("{}  Send changes ({})", icon::CLOUD_ARROW_UP, targets.len())
                } else {
                    format!("{}  Send ticked ({})", icon::CLOUD_ARROW_UP, targets.len())
                };
                if ui
                    .add_enabled(!busy && !targets.is_empty(), egui::Button::new(label))
                    .on_hover_text("Create or update the topics on the server")
                    .clicked()
                {
                    app.send_issues(&targets, &ctx);
                }
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(format!("{}  Sync", icon::ARROWS_CLOCKWISE)),
                    )
                    .on_hover_text(
                        "Bring over the latest from the server: status, assignee, title and priority of every sent issue, the project's people and statuses, and the comments of the open issue",
                    )
                    .clicked()
                {
                    app.sync_bcf(&ctx);
                }
                if busy {
                    ui.spinner();
                }
            });
            if !app.bcf.status.is_empty() {
                ui.label(RichText::new(&app.bcf.status).small().color(theme::text_muted()));
            }
        });
    ui.add_space(4.0);
}

fn issue_detail(app: &mut OpenCheckApp, ui: &mut egui::Ui, guid: &str) {
    let mut remove = false;
    let mut changed = false;
    let mut drop_finding = None;
    let mut focus_finding = None;
    let mut update_view = false;
    let mut remember = false;
    let mut load_comments = false;
    let mut post_comment = None;
    let users: Vec<String> = app.bcf.users().to_vec();
    let mut disciplines: Vec<Discipline> =
        app.federation.iter().map(|(_, m)| m.discipline).collect();
    let link = app.issues.bcf.clone();
    let comments = app
        .issues
        .get(guid)
        .and_then(|i| i.bcf.as_ref())
        .and_then(|b| app.bcf.comments.iter().find(|(t, _)| *t == b.topic_guid))
        .map(|(_, c)| c.clone());
    let signed_in = app.bcf.is_signed_in();
    let comment_id = egui::Id::new(("bcf_comment", guid));
    let mut draft: String = ui
        .ctx()
        .data_mut(|d| d.get_temp(comment_id))
        .unwrap_or_default();
    // Initials used in other issues, then the people on the BCF server.
    let mut suggestions = app.issues.people.clone();
    for user in &users {
        if !suggestions.iter().any(|s| s.eq_ignore_ascii_case(user)) {
            suggestions.push(user.clone());
        }
    }
    let Some(issue) = app.issues.get_mut(guid) else {
        return;
    };
    disciplines.extend(issue.responsible.iter().copied());
    disciplines.sort_by_key(|d| *d as u8);
    disciplines.dedup();
    if issue.solved_by_update {
        egui::Frame::new()
            .fill(theme::solved().gamma_multiply(0.12))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!(
                        "{}  Solved by update: no finding of this issue is found after the model update.",
                        icon::CHECK_CIRCLE
                    ))
                    .color(theme::solved()),
                );
            });
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::FLAG).color(theme::accent()));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut issue.title).desired_width(f32::INFINITY))
            .changed();
    });
    egui::Grid::new("issue_fields")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Status").color(theme::text_muted()));
            egui::ComboBox::from_id_salt("issue_status")
                .selected_text(issue.status.label())
                .show_ui(ui, |ui| {
                    for s in IssueStatus::ALL {
                        changed |= ui
                            .selectable_value(&mut issue.status, s, s.label())
                            .changed();
                    }
                });
            ui.end_row();
            ui.label(RichText::new("Criticality").color(theme::text_muted()))
                .on_hover_text("C402 categories: C1 not critical, C2 critical, C3 very critical");
            egui::ComboBox::from_id_salt("issue_priority")
                .selected_text(c402::label(issue.priority))
                .show_ui(ui, |ui| {
                    for p in c402::LEVELS {
                        changed |= ui
                            .selectable_value(&mut issue.priority, p, c402::label(p))
                            .on_hover_text(c402::explanation(p))
                            .changed();
                    }
                });
            ui.end_row();
            ui.label(RichText::new("Responsible").color(theme::text_muted()));
            ui.horizontal_wrapped(|ui| {
                for d in &disciplines {
                    let on = issue.responsible.contains(d);
                    let text = RichText::new(d.code()).color(if on {
                        theme::discipline_color(*d)
                    } else {
                        theme::text_muted()
                    });
                    let text = if on { text.strong() } else { text };
                    if ui
                        .add(egui::Button::selectable(on, text))
                        .on_hover_text(format!(
                            "{}: click to {} as responsible",
                            d.label(),
                            if on { "remove" } else { "add" }
                        ))
                        .clicked()
                    {
                        if on {
                            issue.responsible.retain(|x| x != d);
                        } else {
                            issue.responsible.push(*d);
                        }
                        changed = true;
                    }
                }
            });
            ui.end_row();
            ui.label(RichText::new("Assigned to").color(theme::text_muted()));
            ui.horizontal(|ui| {
                let (edited, done) =
                    person_picker(ui, ("assigned", guid), &mut issue.assigned_to, &suggestions);
                changed |= edited;
                remember |= done;
                if issue.assigned_to.is_empty()
                    && let Some(person) = issue.effective_assignee(link.as_ref())
                {
                    ui.label(
                        RichText::new(format!("→ {person}"))
                            .small()
                            .color(theme::text_muted()),
                    )
                    .on_hover_text("The default person of the first responsible discipline");
                }
            });
            ui.end_row();
        });
    if let Some(bcf) = &issue.bcf {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(icon::CLOUD).color(theme::accent()));
            let mut text = format!("On the server: {}", bcf.remote_status);
            if !bcf.remote_assigned_to.is_empty() {
                text.push_str(&format!(" · {}", bcf.remote_assigned_to));
            }
            text.push_str(&format!(
                " · synced {}",
                bcf.synced.replace('T', " ").trim_end_matches('Z')
            ));
            ui.label(RichText::new(text).small().color(theme::text_muted()));
        });
        egui::CollapsingHeader::new(RichText::new("Comments on the server").small())
            .id_salt(("bcf_comments", guid))
            .show(ui, |ui| {
                match &comments {
                    None => {
                        if ui
                            .add_enabled(signed_in, egui::Button::new("Load comments").small())
                            .clicked()
                        {
                            load_comments = true;
                        }
                    }
                    Some(list) => {
                        if list.is_empty() {
                            ui.label(
                                RichText::new("No comments yet.")
                                    .small()
                                    .color(theme::text_muted()),
                            );
                        }
                        for c in list {
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}",
                                    c.author,
                                    c.date.replace('T', " ").trim_end_matches('Z')
                                ))
                                .small()
                                .strong(),
                            );
                            ui.label(RichText::new(&c.comment).small());
                        }
                    }
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut draft)
                            .hint_text("Write a comment")
                            .desired_width(ui.available_width() - 60.0),
                    );
                    if ui
                        .add_enabled(
                            signed_in && !draft.trim().is_empty(),
                            egui::Button::new("Post"),
                        )
                        .clicked()
                    {
                        post_comment = Some(std::mem::take(&mut draft));
                    }
                });
            });
    }
    ui.ctx().data_mut(|d| d.insert_temp(comment_id, draft));
    ui.label(
        RichText::new("Description")
            .small()
            .color(theme::text_muted()),
    );
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut issue.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("What must be done, and by whom"),
        )
        .changed();

    ui.add_space(4.0);
    ui.label(
        RichText::new(format!("FINDINGS  {}", issue.findings.len()))
            .small()
            .strong()
            .color(theme::text_muted()),
    );
    egui::ScrollArea::vertical()
        .id_salt("issue_findings")
        .max_height(110.0)
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for source in &issue.findings {
                ui.horizontal(|ui| {
                    let text = if source.details.is_empty() {
                        format!(
                            "{} · {:.0} mm · {}",
                            source.kind, source.magnitude_mm, source.rule
                        )
                    } else {
                        format!("Missing: {}", source.details.join(", "))
                    };
                    if let Some(solved) = &source.solved_by_update {
                        let models = solved
                            .models
                            .iter()
                            .map(|m| format!("{} rev {}", m.model, m.revision))
                            .collect::<Vec<_>>()
                            .join(", ");
                        ui.label(
                            RichText::new(format!("{} {text}", icon::CHECK_CIRCLE))
                                .small()
                                .color(theme::solved())
                                .strikethrough(),
                        )
                        .on_hover_text(format!(
                            "Solved by update: gone after {models} ({})",
                            solved.date.replace('T', " ").trim_end_matches('Z')
                        ));
                    } else if ui
                        .add(
                            egui::Button::new(RichText::new(text).small())
                                .frame(false)
                                .truncate(),
                        )
                        .on_hover_text("Show this finding in 3D")
                        .clicked()
                    {
                        focus_finding = Some(source.finding.clone());
                    }
                    if issue.findings.len() > 1
                        && ui
                            .small_button(icon::X)
                            .on_hover_text("Take this finding out of the issue")
                            .clicked()
                    {
                        drop_finding = Some(source.finding.clone());
                    }
                });
            }
        });

    ui.label(
        RichText::new(format!("COMPONENTS  {}", issue.components.len()))
            .small()
            .strong()
            .color(theme::text_muted()),
    );
    egui::ScrollArea::vertical()
        .id_salt("issue_components")
        .max_height(90.0)
        .show(ui, |ui| {
            for component in &issue.components {
                ui.horizontal(|ui| {
                    let color = match component.role {
                        ComponentRole::A => FOCUS_A,
                        ComponentRole::B => FOCUS_B,
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 3.0, color);
                    let name = if component.name.is_empty() {
                        &component.ifc_class
                    } else {
                        &component.name
                    };
                    ui.label(RichText::new(name).strong());
                    ui.label(
                        RichText::new(format!(
                            "{} · {}{}",
                            class_label(&component.ifc_class),
                            component.model,
                            if component.tag.is_empty() {
                                String::new()
                            } else {
                                format!(" · Revit ID {}", component.tag)
                            }
                        ))
                        .small()
                        .color(theme::text_muted()),
                    )
                    .on_hover_text(format!("IFC GlobalId {}", component.global_id));
                });
            }
        });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "Created {}",
                issue.created.replace('T', " ").trim_end_matches('Z')
            ))
            .small()
            .color(theme::text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new(format!("{}  Remove", icon::TRASH)).color(theme::danger()))
                .clicked()
            {
                remove = true;
            }
            if ui
                .button(format!("{}  Update view", icon::CAMERA))
                .on_hover_text("Save what is on screen now (camera, section, hidden elements) as this issue's view")
                .clicked()
            {
                update_view = true;
            }
        });
    });

    if let Some(finding) = drop_finding {
        issue.findings.retain(|f| f.finding != finding);
        changed = true;
    }
    if changed {
        issue.touch();
        app.session.dirty = true;
    }
    if remove {
        app.issues.remove(guid);
        app.clear_finding_focus();
        app.session.dirty = true;
    }
    if let Some(finding) = focus_finding {
        app.focus_finding(&finding);
    }
    if update_view {
        app.update_issue_view(guid);
    }
    if remember && let Some(person) = app.issues.get(guid).map(|i| i.assigned_to.clone()) {
        crate::issues::remember_person(&mut app.issues.people, &person);
    }
    let topic = app
        .issues
        .get(guid)
        .and_then(|i| i.bcf.as_ref())
        .map(|b| (b.project_id.clone(), b.topic_guid.clone()));
    if let Some((project, topic)) = topic {
        let ctx = ui.ctx().clone();
        if load_comments {
            app.bcf.fetch_comments(&project, &topic, &ctx);
        }
        if let Some(text) = post_comment {
            app.bcf.add_comment(&project, &topic, text, &ctx);
        }
    }
}

fn solved_badge(ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(theme::solved().gamma_multiply(0.15))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{} Solved by update", icon::CHECK_CIRCLE))
                    .small()
                    .color(theme::solved()),
            );
        })
        .response
        .on_hover_text("Every finding of this issue disappeared after a model update");
}

/// Initials or an e-mail address, typed in or picked from people used before.
/// Returns (changed, finished): finished when typing ended or a person was
/// picked, so only whole names are remembered.
pub fn person_picker(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    suggestions: &[String],
) -> (bool, bool) {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id_salt(&id)
            .hint_text("Initials, e.g. ABC")
            .desired_width(150.0),
    );
    let mut changed = response.changed();
    let mut finished = response.lost_focus();
    if !suggestions.is_empty() {
        ui.menu_button(icon::CARET_DOWN, |ui| {
            ui.set_min_width(180.0);
            if ui.selectable_label(value.is_empty(), "Nobody").clicked() {
                value.clear();
                changed = true;
                finished = true;
                ui.close();
            }
            for person in suggestions {
                if ui
                    .selectable_label(value.eq_ignore_ascii_case(person), person)
                    .clicked()
                {
                    *value = person.clone();
                    changed = true;
                    finished = true;
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("People used in other issues and on the BCF server");
    }
    (changed, finished)
}
