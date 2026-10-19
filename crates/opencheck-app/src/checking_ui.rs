//! Panels of the Checking tab: rulesets on the left, results on the right.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{BUILTIN_RULESETS, RuleKind, Ruleset, Severity};
use opencheck_core::{Discipline, ModelId};

use crate::app::OpenCheckApp;
use crate::checking::RULESET_GUIDE;
use crate::theme;

pub fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Critical => Color32::from_rgb(214, 69, 65),
        Severity::Moderate => Color32::from_rgb(232, 140, 40),
        Severity::Low => Color32::from_rgb(200, 170, 30),
    }
}

pub const FOCUS_A: Color32 = Color32::from_rgb(237, 92, 41);
pub const FOCUS_B: Color32 = Color32::from_rgb(51, 115, 242);
pub const ACCEPTED: Color32 = Color32::from_rgb(46, 160, 98);

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::PANEL)
        .inner_margin(egui::Margin::same(12))
        .stroke(egui::Stroke::new(1.0, theme::BORDER))
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .small()
            .strong()
            .color(theme::TEXT_MUTED),
    );
}

pub fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.5, color);
}

pub fn rule_summary(rule: &opencheck_core::check::Rule) -> String {
    let kind = match rule.kind {
        RuleKind::Clash => format!("Clash, tolerance {} mm", rule.tolerance_mm),
        RuleKind::Clearance => format!("Clearance {} mm", rule.clearance_mm),
        RuleKind::Duplicates => format!("Duplicates within {} mm", rule.tolerance_mm),
    };
    let sides = |d: &[Discipline]| {
        if d.is_empty() {
            "any".to_owned()
        } else {
            d.iter().map(|d| d.code()).collect::<Vec<_>>().join("/")
        }
    };
    match &rule.b {
        Some(b) => format!(
            "{kind} · {} vs {}",
            sides(&rule.a.disciplines),
            sides(&b.disciplines)
        ),
        None => format!("{kind} · {}", sides(&rule.a.disciplines)),
    }
}

// ---- Left: rulesets ------------------------------------------------------------

pub fn rulesets_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::left("check_rulesets")
        .resizable(true)
        .default_size(340.0)
        .min_size(280.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Rulesets");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    add_menu(app, ui);
                });
            });
            let manager = egui::Button::new(
                RichText::new(format!("{}  Ruleset Manager", icon::SLIDERS_HORIZONTAL)).strong(),
            )
            .min_size(egui::vec2(ui.available_width(), 30.0));
            if ui
                .add(manager)
                .on_hover_text("Create and edit rulesets and rules, keep a library, run checks")
                .clicked()
            {
                app.manager.open();
            }
            ui.add_space(6.0);
            let run_area = 92.0;
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - run_area).max(80.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if app.checking.rulesets.is_empty() {
                        library(app, ui);
                    } else {
                        ruleset_cards(app, ui);
                    }
                });
            ui.separator();
            run_area_ui(app, ui);
        });
}

fn builtin_list() -> Vec<(usize, Ruleset)> {
    BUILTIN_RULESETS
        .iter()
        .enumerate()
        .filter_map(|(i, b)| Ruleset::from_toml(b.toml).ok().map(|r| (i, r)))
        .collect()
}

fn add_menu(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.menu_button(format!("{}  Add", icon::PLUS), |ui| {
        ui.set_min_width(300.0);
        ui.label(
            RichText::new("Built-in rulesets")
                .small()
                .color(theme::TEXT_MUTED),
        );
        for (index, ruleset) in builtin_list() {
            let added = app.checking.has_ruleset(&ruleset.ruleset.id);
            let button = egui::Button::new(&ruleset.ruleset.name);
            let response = ui
                .add_enabled(!added, button)
                .on_hover_text(&ruleset.ruleset.description);
            if response.clicked() {
                app.checking.add_builtin(index);
                app.session.dirty = true;
                ui.close();
            }
        }
        ui.separator();
        if ui
            .button(format!("{}  Import ruleset file…", icon::FILE_ARROW_UP))
            .clicked()
        {
            ui.close();
            import_ruleset(app);
        }
        if ui
            .button(format!(
                "{}  Download ruleset guide for AI…",
                icon::FILE_ARROW_DOWN
            ))
            .on_hover_text(
                "A Markdown description of the ruleset format. Give it to a language model \
                 together with your requirements and import the TOML file it writes.",
            )
            .clicked()
        {
            ui.close();
            download_guide(app);
        }
    });
}

fn import_ruleset(app: &mut OpenCheckApp) {
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import ruleset")
        .add_filter("OpenCheck ruleset", &["toml"])
        .pick_file()
    else {
        return;
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
    let result = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| app.checking.add_ruleset(&text, name.as_deref()));
    match result {
        Ok(()) => app.session.dirty = true,
        Err(e) => app.errors.push(format!(
            "{} is not a valid ruleset: {e}",
            name.unwrap_or_default()
        )),
    }
}

fn download_guide(app: &mut OpenCheckApp) {
    if let Some(path) = rfd::FileDialog::new()
        .set_title("Save ruleset guide")
        .add_filter("Markdown", &["md"])
        .set_file_name("opencheck-ruleset-guide.md")
        .save_file()
        && let Err(e) = std::fs::write(&path, RULESET_GUIDE)
    {
        app.errors.push(format!("Could not save the guide: {e}"));
    }
}

/// Shown while the project has no rulesets: the built-in ones as cards.
fn library(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Add the rulesets you want to run. They are saved with the project.")
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(6.0);
    for (index, ruleset) in builtin_list() {
        egui::Frame::new()
            .fill(theme::SURFACE)
            .corner_radius(8)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&ruleset.ruleset.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(format!("{} Add", icon::PLUS)).clicked() {
                            app.checking.add_builtin(index);
                            app.session.dirty = true;
                        }
                    });
                });
                ui.label(
                    RichText::new(&ruleset.ruleset.description)
                        .small()
                        .color(theme::TEXT_MUTED),
                );
            });
        ui.add_space(4.0);
    }
}

fn scope_label(app: &OpenCheckApp, models: &Option<Vec<ModelId>>) -> (String, bool) {
    let total = app.federation.len();
    match models {
        None => ("All models".to_owned(), true),
        Some(list) if list.is_empty() => ("No models".to_owned(), false),
        Some(list) if list.len() == 1 => (
            app.federation
                .get(list[0])
                .map(|m| m.short_name.clone())
                .unwrap_or_default(),
            true,
        ),
        Some(list) => (format!("{} of {} models", list.len(), total), true),
    }
}

fn ruleset_cards(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let mut remove = None;
    let mut changed = false;
    let model_ids = app.federation.ids();
    for index in 0..app.checking.rulesets.len() {
        let (scope_text, scope_ok) = scope_label(app, &app.checking.rulesets[index].models);
        let model_names: Vec<(ModelId, String, &'static str)> = model_ids
            .iter()
            .filter_map(|id| {
                app.federation
                    .get(*id)
                    .map(|m| (*id, m.short_name.clone(), m.discipline.code()))
            })
            .collect();
        let entry = &mut app.checking.rulesets[index];
        egui::Frame::new()
            .fill(if entry.enabled {
                theme::SURFACE
            } else {
                theme::PANEL
            })
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(8)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut entry.enabled, "").changed();
                    ui.label(RichText::new(&entry.ruleset.ruleset.name).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(icon::X).color(theme::TEXT_MUTED))
                                    .frame(false),
                            )
                            .on_hover_text("Remove from project")
                            .clicked()
                        {
                            remove = Some(index);
                        }
                    });
                });
                if !entry.ruleset.ruleset.description.is_empty() {
                    ui.label(
                        RichText::new(&entry.ruleset.ruleset.description)
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Runs on", icon::STACK)).small());
                    let text = RichText::new(scope_text).small();
                    let text = if scope_ok {
                        text
                    } else {
                        text.color(theme::DANGER)
                    };
                    ui.menu_button(text, |ui| {
                        ui.set_min_width(220.0);
                        let mut all = entry.models.is_none();
                        if ui.checkbox(&mut all, "All models").changed() {
                            entry.models = if all { None } else { Some(model_ids.clone()) };
                            changed = true;
                        }
                        ui.separator();
                        for (id, name, code) in &model_names {
                            let mut on = entry.models.as_ref().is_none_or(|l| l.contains(id));
                            if ui.checkbox(&mut on, format!("{name}  ({code})")).changed() {
                                let mut list =
                                    entry.models.clone().unwrap_or_else(|| model_ids.clone());
                                if on {
                                    list.push(*id);
                                } else {
                                    list.retain(|m| m != id);
                                }
                                // Keep "all" when every model is ticked again.
                                entry.models = (list.len() != model_ids.len()).then_some(list);
                                changed = true;
                            }
                        }
                    });
                });
                egui::CollapsingHeader::new(
                    RichText::new(format!("{} rules", entry.ruleset.rules.len())).small(),
                )
                .id_salt(("rules", &entry.path))
                .show(ui, |ui| {
                    for rule in &entry.ruleset.rules {
                        ui.horizontal(|ui| {
                            dot(ui, severity_color(rule.severity));
                            ui.label(RichText::new(&rule.name).small());
                        });
                        ui.label(
                            RichText::new(rule_summary(rule))
                                .small()
                                .color(theme::TEXT_MUTED),
                        );
                        ui.add_space(2.0);
                    }
                });
            });
        ui.add_space(6.0);
    }
    if let Some(index) = remove {
        app.checking.remove_ruleset(index);
        changed = true;
    }
    if changed {
        app.checking.stale = true;
        app.session.dirty = true;
    }
}

fn run_area_ui(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let ids = app.federation.ids();
    let jobs = app.checking.jobs(&ids);
    let rules: usize = jobs.iter().map(|j| j.ruleset.rules.len()).sum();
    if app.checking.is_running() {
        ui.add_space(6.0);
        ui.add(
            egui::ProgressBar::new(app.checking.progress_fraction())
                .show_percentage()
                .text("Checking…"),
        );
        return;
    }
    let can_run = !jobs.is_empty() && !app.is_busy();
    let button = egui::Button::new(
        RichText::new(format!("{}  Run check", icon::PLAY))
            .size(15.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(if can_run {
        theme::ACCENT
    } else {
        theme::TEXT_MUTED
    })
    .min_size(egui::vec2(ui.available_width(), 36.0));
    if ui.add_enabled(can_run, button).clicked() {
        app.run_check(ui.ctx(), None);
    }
    let hint = if app.federation.is_empty() {
        "Load models first".to_owned()
    } else if jobs.is_empty() {
        "Enable a ruleset to run".to_owned()
    } else {
        format!("{} rulesets · {} rules", jobs.len(), rules)
    };
    ui.label(RichText::new(hint).small().color(theme::TEXT_MUTED));
}
