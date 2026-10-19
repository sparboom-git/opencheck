//! Right panel of the Checking tab: results grouped by rule, category and
//! component, and the list of selected issues.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{Finding, FindingElement, FindingKind, RuleResult, Severity};

use crate::app::OpenCheckApp;
use crate::checking::{Decision, ResultsTab, group_findings};
use crate::checking_ui::{
    ACCEPTED, FOCUS_A, FOCUS_B, dot, panel_frame, rule_summary, severity_color,
};
use crate::{issues_ui, theme};

pub fn element_label(e: &FindingElement) -> &str {
    if e.name.is_empty() {
        &e.ifc_class
    } else {
        &e.name
    }
}

pub fn class_label(class: &str) -> &str {
    class.strip_prefix("Ifc").unwrap_or(class)
}

fn kind_icon(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::Clash => icon::INTERSECT,
        FindingKind::Inside => icon::SELECTION_INVERSE,
        FindingKind::Clearance => icon::ARROWS_IN_LINE_HORIZONTAL,
        FindingKind::Duplicate => icon::COPY,
    }
}

pub fn magnitude_text(f: &Finding) -> String {
    let mm = f.magnitude * 1000.0;
    match f.kind {
        FindingKind::Clash | FindingKind::Inside => format!("depth {mm:.0} mm"),
        FindingKind::Clearance => format!("distance {mm:.0} mm"),
        FindingKind::Duplicate => format!("offset {mm:.0} mm"),
    }
}

/// Changes requested while drawing, applied afterwards.
#[derive(Default)]
struct Actions {
    click: Option<(String, bool, bool)>,
    mark_all: Option<Vec<String>>,
    decide: Option<(Vec<String>, Option<Decision>)>,
    add_issues: Option<Vec<String>>,
    step: Option<isize>,
}

pub fn results_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::right("check_results")
        .resizable(true)
        .default_size(420.0)
        .min_size(320.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let open = app
                    .checking
                    .report
                    .as_ref()
                    .map(|r| {
                        r.rulesets
                            .iter()
                            .flat_map(|s| &s.rules)
                            .flat_map(|r| &r.findings)
                            .filter(|f| !app.checking.decisions.contains_key(&f.id))
                            .count()
                    })
                    .unwrap_or(0);
                for (tab, label) in [
                    (
                        ResultsTab::Results,
                        format!("{}  Results  {open}", icon::LIST_CHECKS),
                    ),
                    (
                        ResultsTab::Issues,
                        format!("{}  Issues  {}", icon::FLAG, app.issues.list.len()),
                    ),
                ] {
                    let selected = app.checking.tab == tab;
                    let text = RichText::new(label);
                    let text = if selected {
                        text.strong().color(theme::ACCENT)
                    } else {
                        text
                    };
                    if ui.add(egui::Button::selectable(selected, text)).clicked() {
                        app.checking.tab = tab;
                    }
                }
            });
            ui.separator();
            match app.checking.tab {
                ResultsTab::Results => results(app, ui),
                ResultsTab::Issues => issues_ui::issues(app, ui),
            }
        });
}

fn results(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some(report) = app.checking.report.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(icon::LIST_CHECKS)
                    .size(36.0)
                    .color(theme::BORDER),
            );
            ui.label(RichText::new("Run a check to see results here.").color(theme::TEXT_MUTED));
        });
        return;
    };

    // Open findings per severity.
    let mut open = [0usize; 3];
    let (mut accepted, mut rejected) = (0, 0);
    for rule in report.rulesets.iter().flat_map(|r| &r.rules) {
        for f in &rule.findings {
            match app.checking.decisions.get(&f.id) {
                Some(Decision::Accepted) => accepted += 1,
                Some(Decision::Rejected) => rejected += 1,
                None => open[rule.rule.severity as usize] += 1,
            }
        }
    }
    ui.horizontal_wrapped(|ui| {
        for (severity, count) in [Severity::Critical, Severity::Moderate, Severity::Low]
            .into_iter()
            .zip(open)
        {
            dot(ui, severity_color(severity));
            ui.label(
                RichText::new(format!("{count} {}", severity.label().to_lowercase())).strong(),
            );
            ui.add_space(6.0);
        }
    });
    ui.label(
        RichText::new(format!(
            "{accepted} accepted · {rejected} rejected · checked in {:.1} s",
            report.duration.as_secs_f32()
        ))
        .small()
        .color(theme::TEXT_MUTED),
    );
    ui.checkbox(&mut app.checking.show_decided, "Show accepted and rejected");
    if app.checking.stale {
        ui.label(
            RichText::new(format!(
                "{}  Models or rulesets changed since this run",
                icon::WARNING
            ))
            .small()
            .color(severity_color(Severity::Moderate)),
        );
    }

    let mut actions = Actions::default();
    selection_bar(app, ui, &mut actions);
    ui.separator();

    let detail_height = if app.checking.selected.is_some() {
        230.0
    } else {
        0.0
    };
    egui::ScrollArea::vertical()
        .max_height((ui.available_height() - detail_height).max(100.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for (ri, ruleset) in report.rulesets.iter().enumerate() {
                let total: usize = ruleset.rules.iter().map(|r| r.findings.len()).sum();
                egui::CollapsingHeader::new(
                    RichText::new(format!("{}   {total}", ruleset.info.name)).strong(),
                )
                .id_salt(("result_ruleset", ri))
                .default_open(true)
                .show(ui, |ui| {
                    for (rj, rule) in ruleset.rules.iter().enumerate() {
                        rule_group(app, ui, (ri, rj), rule, &mut actions);
                    }
                });
            }
        });

    if let Some(id) = app.checking.selected.clone() {
        ui.separator();
        finding_detail(app, ui, &id, &mut actions);
    }
    apply(app, actions);
}

/// Bulk actions for the picked findings.
fn selection_bar(app: &OpenCheckApp, ui: &mut egui::Ui, actions: &mut Actions) {
    let marked = &app.checking.marked;
    if marked.len() < 2 {
        ui.label(
            RichText::new("Ctrl+click or Shift+click to select several findings")
                .small()
                .color(theme::TEXT_MUTED),
        );
        if let [only] = marked.as_slice() {
            ui.horizontal(|ui| {
                add_issue_button(app, ui, actions, std::slice::from_ref(only));
            });
        }
        return;
    }
    egui::Frame::new()
        .fill(theme::ACCENT_SOFT)
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} selected", marked.len())).strong());
                add_issue_button(app, ui, actions, marked);
                if ui
                    .button(RichText::new(format!("{}  Accept", icon::CHECK)).color(ACCEPTED))
                    .clicked()
                {
                    actions.decide = Some((marked.clone(), Some(Decision::Accepted)));
                }
                if ui.button(format!("{}  Reject", icon::X)).clicked() {
                    actions.decide = Some((marked.clone(), Some(Decision::Rejected)));
                }
                if ui.button("Clear").clicked() {
                    actions.mark_all = Some(Vec::new());
                }
            });
        });
}

fn add_issue_button(app: &OpenCheckApp, ui: &mut egui::Ui, actions: &mut Actions, ids: &[String]) {
    let new: Vec<String> = ids
        .iter()
        .filter(|id| app.issues.for_finding(id).is_none())
        .cloned()
        .collect();
    let label = if new.is_empty() {
        "Already an issue".to_owned()
    } else if ids.len() == 1 {
        "Add to issues".to_owned()
    } else {
        format!("Add {} to issues", new.len())
    };
    let button = egui::Button::new(
        RichText::new(format!("{}  {label}", icon::FLAG))
            .strong()
            .color(egui::Color32::WHITE),
    )
    .fill(theme::ACCENT);
    if ui
        .add_enabled(!new.is_empty(), button)
        .on_hover_text("Selected issues are the ones that go into the report")
        .clicked()
    {
        actions.add_issues = Some(new);
    }
}

fn header_menu(response: &egui::Response, ids: Vec<String>, actions: &mut Actions) {
    response.context_menu(|ui| {
        if ui
            .button(format!("{}  Select all {}", icon::CHECK_SQUARE, ids.len()))
            .clicked()
        {
            actions.mark_all = Some(ids.clone());
            ui.close();
        }
    });
}

fn rule_group(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    key: (usize, usize),
    rule: &RuleResult,
    actions: &mut Actions,
) {
    let categories = group_findings(rule.findings.iter().filter(|f| app.checking.is_shown(f)));
    let count: usize = categories.iter().map(|c| c.count()).sum();
    let id = ui.make_persistent_id(("result_rule", key));
    let all: Vec<String> = categories.iter().flat_map(|c| c.ids()).collect();
    let header =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, count > 0)
            .show_header(ui, |ui| {
                dot(ui, severity_color(rule.rule.severity));
                let name = ui
                    .label(&rule.rule.name)
                    .on_hover_text(rule_summary(&rule.rule));
                header_menu(&name, all, actions);
                let text = RichText::new(count.to_string()).small();
                ui.label(if count == 0 {
                    text.color(theme::TEXT_MUTED)
                } else {
                    text.strong()
                });
            });
    header.body(|ui| {
        if categories.is_empty() {
            ui.label(
                RichText::new("No findings")
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        }
        for (ci, category) in categories.iter().enumerate() {
            let cid = ui.make_persistent_id(("result_category", key, ci));
            let open = categories.len() == 1 || category.count() <= 20;
            let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                ui.ctx(),
                cid,
                open,
            )
            .show_header(ui, |ui| {
                ui.label(RichText::new(icon::SQUARES_FOUR).color(theme::TEXT_MUTED));
                let name = ui.label(format!(
                    "{} vs {}",
                    class_label(category.a_class),
                    class_label(category.b_class)
                ));
                header_menu(&name, category.ids(), actions);
                ui.label(RichText::new(category.count().to_string()).small().strong());
            });
            header.body(|ui| {
                for (ki, component) in category.components.iter().enumerate() {
                    let kid = ui.make_persistent_id(("result_component", key, ci, ki));
                    let ids: Vec<String> =
                        component.findings.iter().map(|f| f.id.clone()).collect();
                    let marked = ids.iter().all(|id| app.checking.marked.contains(id));
                    let header = egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        kid,
                        component.findings.len() <= 5,
                    )
                    .show_header(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, FOCUS_A);
                        let model = app
                            .federation
                            .get(component.element.model)
                            .map(|m| m.short_name.as_str())
                            .unwrap_or("");
                        let name = RichText::new(element_label(component.element));
                        let response = ui.add(
                            egui::Button::selectable(
                                marked,
                                if marked { name.strong() } else { name },
                            )
                            .frame_when_inactive(false),
                        );
                        if response.clicked() {
                            // Clicking a component picks all its findings.
                            actions.mark_all = Some(ids.clone());
                        }
                        header_menu(&response, ids.clone(), actions);
                        ui.label(RichText::new(model).small().color(theme::TEXT_MUTED));
                        ui.label(RichText::new(component.findings.len().to_string()).small());
                    });
                    header.body(|ui| {
                        for f in &component.findings {
                            finding_row(app, ui, f, actions);
                        }
                    });
                }
            });
        }
    });
}

fn finding_row(app: &OpenCheckApp, ui: &mut egui::Ui, f: &Finding, actions: &mut Actions) {
    let focused = app.checking.selected.as_deref() == Some(f.id.as_str());
    let marked = app.checking.marked.contains(&f.id);
    let decision = app.checking.decisions.get(&f.id).copied();
    let is_issue = app.issues.for_finding(&f.id).is_some();
    ui.horizontal(|ui| {
        let (glyph, color) = if is_issue {
            (icon::FLAG, theme::ACCENT)
        } else {
            match decision {
                Some(Decision::Accepted) => (icon::CHECK_CIRCLE, ACCEPTED),
                Some(Decision::Rejected) => (icon::X_CIRCLE, theme::TEXT_MUTED),
                None => (kind_icon(f.kind), theme::TEXT),
            }
        };
        ui.label(RichText::new(glyph).color(color));
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, FOCUS_B);
        let mut label = RichText::new(element_label(&f.b));
        if decision == Some(Decision::Rejected) {
            label = label.color(theme::TEXT_MUTED);
        }
        if focused {
            label = label.strong().color(theme::ACCENT);
        }
        let response = ui
            .add(egui::Button::selectable(marked || focused, label).frame_when_inactive(false))
            .on_hover_text(format!(
                "{} [{}]\n{} [{}]\n{}",
                element_label(&f.a),
                f.a.ifc_class,
                element_label(&f.b),
                f.b.ifc_class,
                magnitude_text(f)
            ));
        ui.label(
            RichText::new(format!("{:.0} mm", f.magnitude * 1000.0))
                .small()
                .color(theme::TEXT_MUTED),
        );
        if focused {
            response.scroll_to_me(None);
        }
        if response.clicked() {
            let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            actions.click = Some((f.id.clone(), ctrl, shift));
        }
    });
}

fn finding_detail(app: &OpenCheckApp, ui: &mut egui::Ui, id: &str, actions: &mut Actions) {
    let Some((_, rule, f)) = app.checking.find(id) else {
        return;
    };
    let decision = app.checking.decisions.get(id).copied();
    ui.horizontal(|ui| {
        dot(ui, severity_color(rule.rule.severity));
        ui.label(
            RichText::new(format!(
                "{} · {}",
                f.kind.label(),
                rule.rule.severity.label()
            ))
            .heading()
            .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(icon::CARET_RIGHT).frame(false))
                .on_hover_text("Next finding")
                .clicked()
            {
                actions.step = Some(1);
            }
            if ui
                .add(egui::Button::new(icon::CARET_LEFT).frame(false))
                .on_hover_text("Previous finding")
                .clicked()
            {
                actions.step = Some(-1);
            }
        });
    });
    ui.label(RichText::new(&rule.rule.name).color(theme::TEXT_MUTED));
    ui.label(magnitude_text(f));
    ui.add_space(4.0);
    for (element, color) in [(&f.a, FOCUS_A), (&f.b, FOCUS_B)] {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, color);
            let model = app
                .federation
                .get(element.model)
                .map(|m| m.short_name.clone())
                .unwrap_or_default();
            ui.label(RichText::new(element_label(element)).strong());
            ui.label(
                RichText::new(format!("{} · {model}", element.ifc_class))
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        });
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let one = vec![id.to_owned()];
        let accept = egui::Button::selectable(
            decision == Some(Decision::Accepted),
            RichText::new(format!("{}  Accept", icon::CHECK)).color(ACCEPTED),
        );
        if ui
            .add(accept)
            .on_hover_text("A real problem that must be handled")
            .clicked()
        {
            actions.decide = Some((one.clone(), Some(Decision::Accepted)));
        }
        let reject = egui::Button::selectable(
            decision == Some(Decision::Rejected),
            format!("{}  Reject", icon::X),
        );
        if ui
            .add(reject)
            .on_hover_text("Not a problem; hide it from the open findings")
            .clicked()
        {
            actions.decide = Some((one.clone(), Some(Decision::Rejected)));
        }
        if decision.is_some() && ui.button("Reset").clicked() {
            actions.decide = Some((one, None));
        }
    });
}

fn apply(app: &mut OpenCheckApp, actions: Actions) {
    if let Some(ids) = actions.mark_all {
        if let Some(first) = ids.first().cloned() {
            app.focus_finding(&first);
        }
        app.checking.mark_all(ids);
    }
    if let Some((id, ctrl, shift)) = actions.click {
        app.checking.click(&id, ctrl, shift);
        app.focus_finding(&id);
    }
    if let Some(ids) = actions.add_issues {
        app.add_issues(&ids);
    }
    if let Some((ids, decision)) = actions.decide {
        let current = app.checking.selected.clone();
        let order = app.checking.ordered_shown();
        for id in &ids {
            app.checking.decide(id, decision);
        }
        app.session.dirty = true;
        // Once decided findings disappear from the list, move on to the next open one.
        if decision.is_some() && !app.checking.show_decided {
            app.checking.marked.retain(|m| !ids.contains(m));
            let next = current.and_then(|c| {
                let at = order.iter().position(|o| *o == c)?;
                order[at..]
                    .iter()
                    .chain(order[..at].iter().rev())
                    .find(|o| !app.checking.decisions.contains_key(*o))
                    .cloned()
            });
            match next {
                Some(next) => app.focus_finding(&next),
                None => app.clear_finding_focus(),
            }
        }
    }
    if let Some(offset) = actions.step
        && let Some(current) = app.checking.selected.clone()
    {
        let order = app.checking.ordered_shown();
        if let Some(at) = order.iter().position(|o| *o == current) {
            let next = (at as isize + offset).rem_euclid(order.len() as isize) as usize;
            let id = order[next].clone();
            app.checking.click(&id, false, false);
            app.focus_finding(&id);
        }
    }
}
