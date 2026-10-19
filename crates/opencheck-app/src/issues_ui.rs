//! The Issues list in the Checking tab: findings picked out for the report.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::Severity;

use crate::app::OpenCheckApp;
use crate::checking_ui::{FOCUS_A, FOCUS_B, dot, severity_color};
use crate::issues::{ComponentRole, IssueStatus};
use crate::results_ui::class_label;
use crate::theme;

pub fn issues(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    if app.issues.list.is_empty() {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(icon::FLAG).size(36.0).color(theme::BORDER));
            ui.label(RichText::new("No issues yet").strong());
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Select findings under Results and press \"Add to issues\". \
                     Issues are the findings that go into the report.",
                )
                .color(theme::TEXT_MUTED),
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
        .color(theme::TEXT_MUTED),
    );
    ui.add_space(4.0);

    let mut show = None;
    let detail_height = if app.issues.selected.is_some() {
        330.0
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
                    dot(ui, severity_color(issue.priority));
                    ui.label(
                        RichText::new(format!("{}", index + 1))
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                    let mut title = RichText::new(&issue.title);
                    if matches!(issue.status, IssueStatus::Resolved | IssueStatus::Closed) {
                        title = title.color(theme::TEXT_MUTED);
                    }
                    if selected {
                        title = title.strong().color(theme::ACCENT);
                    }
                    let response = ui
                        .add(egui::Button::selectable(selected, title).frame_when_inactive(false));
                    ui.label(
                        RichText::new(issue.status.label())
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                    if response.clicked() {
                        show = Some(issue.guid.clone());
                    }
                    if selected {
                        response.scroll_to_me(None);
                    }
                });
            }
        });
    if let Some(guid) = show {
        app.show_issue(&guid);
    }

    if let Some(guid) = app.issues.selected.clone() {
        ui.separator();
        issue_detail(app, ui, &guid);
    }
}

fn issue_detail(app: &mut OpenCheckApp, ui: &mut egui::Ui, guid: &str) {
    let mut remove = false;
    let mut changed = false;
    let Some(issue) = app.issues.get_mut(guid) else {
        return;
    };
    ui.horizontal(|ui| {
        ui.label(RichText::new(icon::FLAG).color(theme::ACCENT));
        changed |= ui
            .add(egui::TextEdit::singleline(&mut issue.title).desired_width(f32::INFINITY))
            .changed();
    });
    egui::Grid::new("issue_fields")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Status").color(theme::TEXT_MUTED));
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
            ui.label(RichText::new("Priority").color(theme::TEXT_MUTED));
            egui::ComboBox::from_id_salt("issue_priority")
                .selected_text(issue.priority.label())
                .show_ui(ui, |ui| {
                    for p in [Severity::Critical, Severity::Moderate, Severity::Low] {
                        changed |= ui
                            .selectable_value(&mut issue.priority, p, p.label())
                            .changed();
                    }
                });
            ui.end_row();
            if let Some(source) = &issue.source {
                ui.label(RichText::new("Rule").color(theme::TEXT_MUTED));
                ui.label(&source.rule).on_hover_text(&source.ruleset);
                ui.end_row();
                ui.label(RichText::new(source.kind.as_str()).color(theme::TEXT_MUTED));
                ui.label(format!("{:.0} mm", source.magnitude_mm));
                ui.end_row();
            }
        });
    ui.label(
        RichText::new("Description")
            .small()
            .color(theme::TEXT_MUTED),
    );
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut issue.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY)
                .hint_text("What must be done, and by whom"),
        )
        .changed();
    for component in &issue.components {
        ui.horizontal(|ui| {
            let color = match component.role {
                ComponentRole::A => FOCUS_A,
                ComponentRole::B => FOCUS_B,
            };
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, color);
            let name = if component.name.is_empty() {
                &component.ifc_class
            } else {
                &component.name
            };
            ui.label(RichText::new(name).strong());
            ui.label(
                RichText::new(format!(
                    "{} · {}",
                    class_label(&component.ifc_class),
                    component.model
                ))
                .small()
                .color(theme::TEXT_MUTED),
            )
            .on_hover_text(&component.global_id);
        });
    }
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "Created {}",
                issue.created.replace('T', " ").trim_end_matches('Z')
            ))
            .small()
            .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(RichText::new(format!("{}  Remove", icon::TRASH)).color(theme::DANGER))
                .clicked()
            {
                remove = true;
            }
        });
    });
    if changed {
        issue.touch();
        app.session.dirty = true;
    }
    if remove {
        app.issues.remove(guid);
        app.clear_finding_focus();
        app.session.dirty = true;
    }
}
