//! Panels of the Checking tab: rulesets on the left, results on the right.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{
    BUILTIN_RULESETS, RuleKind, Ruleset, Severity, builtin_standard, ids_path, standard_to_ids,
};
use opencheck_core::{Discipline, ModelId};

use crate::app::OpenCheckApp;
use crate::checking::{self, RULESET_GUIDE};
use crate::theme;

pub fn severity_color(severity: Severity) -> Color32 {
    match severity {
        Severity::Critical => theme::c3(),
        Severity::Moderate => theme::c2(),
        Severity::Low => theme::c1(),
    }
}

pub const FOCUS_A: Color32 = theme::CLASH_A;
pub const FOCUS_B: Color32 = theme::CLASH_B;
/// Accepted findings, as solved ones.
pub fn accepted() -> Color32 {
    theme::solved()
}

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(theme::panel())
        .inner_margin(egui::Margin::same(12))
        .stroke(egui::Stroke::new(1.0, theme::border()))
}

pub fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .small()
            .strong()
            .color(theme::text_muted()),
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
        RuleKind::Information => match ids_path(&rule.standard) {
            Some(path) => format!("IDS {}", path.trim_start_matches("requirements/")),
            None => "Information per LOD level".to_owned(),
        },
        RuleKind::FreeZone => match &rule.zone {
            Some(z) => format!(
                "Free zone {} {:.0} mm",
                z.side.label().to_lowercase(),
                z.depth_mm
            ),
            None => "Free zone".to_owned(),
        },
        RuleKind::Script => format!("Script, {} parameters", rule.params.len()),
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
                .color(theme::text_muted()),
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
            import_ruleset(app, ui.ctx());
        }
        if ui
            .button(format!("{}  Import IDS…", icon::FILE_ARROW_UP))
            .on_hover_text(
                "A buildingSMART Information Delivery Specification (IDS 1.0). It is stored in \
                 the project and checked as an information rule.",
            )
            .clicked()
        {
            ui.close();
            import_ids(app, ui.ctx());
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
        if ui
            .button(format!(
                "{}  Export LOD requirements as IDS…",
                icon::FILE_ARROW_DOWN
            ))
            .on_hover_text(
                "The built-in information requirements at the project's level, as an IDS 1.0 \
                 file for other tools. Alternatives between kinds of information are reduced to \
                 one, see the documentation.",
            )
            .clicked()
        {
            ui.close();
            export_ids(app);
        }
    });
}

fn import_ids(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let filter: crate::platform::Filter = ("Information Delivery Specification", &["ids", "xml"]);
    if crate::platform::IS_WEB {
        crate::platform::pick_in_browser(crate::platform::Pick::ImportIds, filter, false, ctx);
        return;
    }
    let Some(path) = crate::platform::pick_file("Import IDS", filter) else {
        return;
    };
    let name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let result = std::fs::read_to_string(&path)
        .map_err(|e| e.to_string())
        .and_then(|text| app.checking.add_ids(&text, &name));
    match result {
        Ok(()) => app.session.dirty = true,
        Err(e) => app
            .errors
            .push(format!("{name} is not a valid IDS file: {e}")),
    }
}

fn export_ids(app: &mut OpenCheckApp) {
    let Some(standard) = builtin_standard(checking::INFORMATION_STANDARD) else {
        return;
    };
    let level = app.checking.project_level();
    let Some(path) = crate::platform::save_file(
        "Export information requirements as IDS",
        ("Information Delivery Specification", &["ids"]),
        &format!("information-lod-{level}.ids"),
    ) else {
        return;
    };
    let export = standard_to_ids(standard, std::slice::from_ref(&level));
    if let Err(e) = crate::platform::write(&path, export.ids.to_xml().as_bytes()) {
        app.errors.push(format!("Could not save the IDS file: {e}"));
    }
}

fn import_ruleset(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let filter: crate::platform::Filter = ("OpenCheck ruleset", &["toml"]);
    if crate::platform::IS_WEB {
        crate::platform::pick_in_browser(crate::platform::Pick::ImportRuleset, filter, false, ctx);
        return;
    }
    let Some(path) = crate::platform::pick_file("Import ruleset", filter) else {
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
    if let Some(path) = crate::platform::save_file(
        "Save ruleset guide",
        ("Markdown", &["md"]),
        "opencheck-ruleset-guide.md",
    ) && let Err(e) = crate::platform::write(&path, RULESET_GUIDE.as_bytes())
    {
        app.errors.push(format!("Could not save the guide: {e}"));
    }
}

/// Shown while the project has no rulesets: the built-in ones as cards.
fn library(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new("Add the rulesets you want to run. They are saved with the project.")
            .color(theme::text_muted()),
    );
    ui.add_space(6.0);
    for (index, ruleset) in builtin_list() {
        egui::Frame::new()
            .fill(theme::surface())
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
                        .color(theme::text_muted()),
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
    let mut new_level = None;
    let mut adapt = false;
    let adapted = app.checking.adapted.is_some();
    let model_ids = app.federation.ids();
    let project_level = app.checking.project_level();
    let levels = checking::information_levels();
    let overrides: Vec<String> = app
        .checking
        .model_levels
        .iter()
        .filter_map(|(id, level)| {
            let name = app.federation.get(*id)?.short_name.clone();
            Some(format!("{name}: {}", checking::level_label(level)))
        })
        .collect();
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
                theme::surface()
            } else {
                theme::panel()
            })
            .stroke(egui::Stroke::new(1.0, theme::border()))
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
                                egui::Button::new(RichText::new(icon::X).color(theme::text_muted()))
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
                            .color(theme::text_muted()),
                    );
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  Runs on", icon::STACK)).small());
                    let text = RichText::new(scope_text).small();
                    let text = if scope_ok {
                        text
                    } else {
                        text.color(theme::danger())
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
                let information = entry.ruleset.rules.iter().any(checking::uses_levels);
                if information {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}  Level", icon::STAIRS)).small());
                        egui::ComboBox::from_id_salt(("project_level", &entry.path))
                            .selected_text(
                                RichText::new(checking::level_label(&project_level)).small(),
                            )
                            .show_ui(ui, |ui| {
                                for level in &levels {
                                    if ui
                                        .selectable_label(
                                            *level == project_level,
                                            checking::level_label(level),
                                        )
                                        .clicked()
                                    {
                                        new_level = Some(level.clone());
                                    }
                                }
                            })
                            .response
                            .on_hover_text(
                                "The level of development the whole project is checked at. A model can use another level: open its menu in the model list.",
                            );
                        if !overrides.is_empty() {
                            ui.label(
                                RichText::new(format!("{} models differ", overrides.len()))
                                    .small()
                                    .color(theme::text_muted()),
                            )
                            .on_hover_text(overrides.join("
"));
                        }
                    });
                    ui.horizontal(|ui| {
                        let text = if adapted {
                            format!("{}  Where to look (adapted)…", icon::SLIDERS_HORIZONTAL)
                        } else {
                            format!("{}  Where to look…", icon::SLIDERS_HORIZONTAL)
                        };
                        if ui
                            .small_button(text)
                            .on_hover_text(
                                "Choose which properties, attributes and other sources each item is read from, for all building parts or for one",
                            )
                            .clicked()
                        {
                            adapt = true;
                        }
                    });
                }
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
                                .color(theme::text_muted()),
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
    if adapt {
        crate::standard_editor::open(app, None, None);
    }
    if let Some(level) = new_level {
        app.checking.set_project_level(level);
        app.session.dirty = true;
    }
    if changed {
        app.checking.stale = true;
        app.session.dirty = true;
    }
}

fn run_area_ui(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    if app.checking.area.is_some() {
        let (inside, total) = app.area_counts();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{}  Working area: {inside} of {total} elements are checked",
                    icon::CROP
                ))
                .small()
                .color(theme::accent()),
            )
            .on_hover_text("Set in the Model tab with Edit view");
            if ui.small_button("Reset").clicked() {
                app.reset_area();
            }
        });
    }
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
            .color(theme::on_accent()),
    )
    .fill(if can_run {
        theme::accent()
    } else {
        theme::text_muted()
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
    ui.label(RichText::new(hint).small().color(theme::text_muted()));
}
