//! The Ruleset Manager: create and edit rulesets and their rules in forms,
//! keep a personal library, and run checks.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{
    BUILTIN_RULESETS, Between, PropertyFilter, Rule, RuleKind, Ruleset, RulesetInfo, Selector,
    Severity,
};
use opencheck_core::{Discipline, Federation};
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::checking_ui::{dot, rule_summary, severity_color};
use crate::library::{self, LibraryRuleset};
use crate::theme;

/// Classes offered while typing; any other IFC class name works too.
const COMMON_CLASSES: &[&str] = &[
    "IfcBuildingElement",
    "IfcBuildingElementProxy",
    "IfcBeam",
    "IfcColumn",
    "IfcSlab",
    "IfcWall",
    "IfcRoof",
    "IfcFooting",
    "IfcPile",
    "IfcMember",
    "IfcPlate",
    "IfcStair",
    "IfcStairFlight",
    "IfcRamp",
    "IfcRailing",
    "IfcDoor",
    "IfcWindow",
    "IfcCovering",
    "IfcCurtainWall",
    "IfcChimney",
    "IfcFurnishingElement",
    "IfcDistributionElement",
    "IfcDistributionFlowElement",
    "IfcFlowSegment",
    "IfcFlowFitting",
    "IfcFlowTerminal",
    "IfcFlowController",
    "IfcEnergyConversionDevice",
    "IfcFlowMovingDevice",
    "IfcDuctSegment",
    "IfcDuctFitting",
    "IfcAirTerminal",
    "IfcPipeSegment",
    "IfcPipeFitting",
    "IfcCableCarrierSegment",
    "IfcCableCarrierFitting",
    "IfcCableSegment",
    "IfcLightFixture",
    "IfcSanitaryTerminal",
    "IfcSpace",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Item {
    #[default]
    None,
    Project(usize),
    Builtin(usize),
    Library(usize),
}

#[derive(Default)]
pub struct ManagerState {
    pub open: bool,
    pub item: Item,
    pub rule: Option<usize>,
    library: Vec<LibraryRuleset>,
    class_input: [String; 2],
    exclude_input: [String; 2],
    /// Matching element counts per selector, so they are not recounted every frame.
    counts: FxHashMap<String, (usize, usize)>,
}

impl ManagerState {
    pub fn open(&mut self) {
        self.open = true;
        self.library = library::list();
        self.counts.clear();
    }
}

pub fn show(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.manager.open {
        return;
    }
    let mut open = true;
    egui::Window::new(RichText::new(format!("{}  Ruleset Manager", icon::LIST_CHECKS)).strong())
        .id(egui::Id::new("ruleset_manager"))
        .open(&mut open)
        .default_size([1120.0, 740.0])
        .min_size([820.0, 480.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            footer(app, ui);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_width(250.0);
                    item_list(app, ui);
                });
                ui.separator();
                ui.vertical(|ui| match app.manager.item {
                    Item::Project(i) if i < app.checking.rulesets.len() => {
                        project_editor(app, ui, i)
                    }
                    Item::Builtin(i) => {
                        let ruleset = Ruleset::from_toml(BUILTIN_RULESETS[i].toml).ok();
                        read_only(app, ui, ruleset, None);
                    }
                    Item::Library(i) if i < app.manager.library.len() => {
                        let ruleset = app.manager.library[i].ruleset.clone();
                        match ruleset {
                            Ok(r) => read_only(app, ui, Some(r), Some(i)),
                            Err(e) => {
                                ui.label(
                                    RichText::new(format!(
                                        "{}  This file is not a valid ruleset",
                                        icon::WARNING
                                    ))
                                    .color(theme::DANGER),
                                );
                                ui.label(e);
                            }
                        }
                    }
                    _ => {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Select a ruleset on the left, or create a new one.")
                                    .color(theme::TEXT_MUTED),
                            );
                        });
                    }
                });
            });
        });
    app.manager.open = open;
}

fn footer(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("manager_footer").show(ui, |ui| {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if app.checking.is_running() {
                ui.add(
                    egui::ProgressBar::new(app.checking.progress_fraction())
                        .show_percentage()
                        .desired_width(300.0)
                        .text("Checking…"),
                );
                return;
            }
            let jobs = app.checking.jobs(&app.federation.ids());
            let can_run = !jobs.is_empty() && !app.is_busy();
            let run = egui::Button::new(
                RichText::new(format!("{}  Run check", icon::PLAY))
                    .strong()
                    .color(Color32::WHITE),
            )
            .fill(if can_run {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            });
            if ui
                .add_enabled(can_run, run)
                .on_hover_text("Runs every enabled ruleset in the project")
                .clicked()
            {
                app.run_check(ui.ctx(), None);
            }
            let rules: usize = jobs.iter().map(|j| j.ruleset.rules.len()).sum();
            ui.label(
                RichText::new(if app.federation.is_empty() {
                    "Load models to run a check".to_owned()
                } else {
                    format!("{} enabled rulesets · {rules} rules", jobs.len())
                })
                .small()
                .color(theme::TEXT_MUTED),
            );
            if let Some(report) = &app.checking.report {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("Last run: {} findings", report.finding_count()))
                            .small()
                            .color(theme::TEXT_MUTED),
                    );
                });
            }
        });
    });
}

fn list_button(ui: &mut egui::Ui, selected: bool, text: RichText) -> egui::Response {
    ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false))
}

fn item_list(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .id_salt("manager_items")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            ui.label(RichText::new("IN THIS PROJECT").small().strong().color(theme::TEXT_MUTED));
            for i in 0..app.checking.rulesets.len() {
                let name = app.checking.rulesets[i].ruleset.ruleset.name.clone();
                let selected = app.manager.item == Item::Project(i);
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut app.checking.rulesets[i].enabled, "").on_hover_text("Run this ruleset").changed() {
                        app.checking.stale = true;
                        app.session.dirty = true;
                    }
                    if list_button(ui, selected, RichText::new(&name)).clicked() {
                        app.manager.item = Item::Project(i);
                        app.manager.rule = Some(0);
                    }
                });
            }
            if ui.button(format!("{}  New ruleset", icon::PLUS)).clicked() {
                new_ruleset(app);
            }
            ui.add_space(12.0);
            ui.label(RichText::new("BUILT-IN").small().strong().color(theme::TEXT_MUTED));
            for (i, builtin) in BUILTIN_RULESETS.iter().enumerate() {
                let name = Ruleset::from_toml(builtin.toml)
                    .map(|r| r.ruleset.name)
                    .unwrap_or_else(|_| builtin.file_name.to_owned());
                let selected = app.manager.item == Item::Builtin(i);
                if list_button(ui, selected, RichText::new(format!("{}  {name}", icon::LOCK_SIMPLE))).clicked() {
                    app.manager.item = Item::Builtin(i);
                }
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("MY LIBRARY").small().strong().color(theme::TEXT_MUTED));
                if ui
                    .add(egui::Button::new(RichText::new(icon::FOLDER_OPEN).small()).frame(false))
                    .on_hover_text(library::folder().display().to_string())
                    .clicked()
                {
                    let _ = std::fs::create_dir_all(library::folder());
                    let _ = std::process::Command::new("explorer").arg(library::folder()).spawn();
                }
                if ui
                    .add(egui::Button::new(RichText::new(icon::ARROW_CLOCKWISE).small()).frame(false))
                    .on_hover_text("Reload the library folder")
                    .clicked()
                {
                    app.manager.library = library::list();
                }
            });
            if app.manager.library.is_empty() {
                ui.label(
                    RichText::new("Empty. Use \"Save to library\" on a project ruleset to reuse it in other projects.")
                        .small()
                        .color(theme::TEXT_MUTED),
                );
            }
            for i in 0..app.manager.library.len() {
                let entry = &app.manager.library[i];
                let (name, ok) = match &entry.ruleset {
                    Ok(r) => (r.ruleset.name.clone(), true),
                    Err(_) => (
                        entry.path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                        false,
                    ),
                };
                let text = if ok { RichText::new(name) } else { RichText::new(name).color(theme::DANGER) };
                let selected = app.manager.item == Item::Library(i);
                if list_button(ui, selected, text).clicked() {
                    app.manager.item = Item::Library(i);
                }
            }
        });
}

fn new_rule(n: usize) -> Rule {
    Rule {
        id: format!("rule-{n}"),
        name: "New clash rule".to_owned(),
        description: String::new(),
        kind: RuleKind::Clash,
        severity: Severity::Moderate,
        tolerance_mm: 10.0,
        clearance_mm: 0.0,
        between: Between::DifferentModels,
        a: Selector::default(),
        b: Some(Selector::default()),
    }
}

fn new_ruleset(app: &mut OpenCheckApp) {
    let token: String = crate::issues::new_guid("ruleset").chars().take(8).collect();
    let ruleset = Ruleset {
        ruleset: RulesetInfo {
            id: format!("project.{token}"),
            name: "New ruleset".to_owned(),
            description: String::new(),
            version: "1".to_owned(),
            author: String::new(),
        },
        rules: vec![new_rule(1)],
    };
    if app.checking.add_ruleset(&ruleset.to_toml(), None).is_ok() {
        app.manager.item = Item::Project(app.checking.rulesets.len() - 1);
        app.manager.rule = Some(0);
        app.session.dirty = true;
    }
}

/// A built-in or library ruleset: shown as it is, with ways to use it.
fn read_only(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    ruleset: Option<Ruleset>,
    library_index: Option<usize>,
) {
    let Some(ruleset) = ruleset else { return };
    ui.label(RichText::new(&ruleset.ruleset.name).heading().strong());
    if !ruleset.ruleset.description.is_empty() {
        ui.label(RichText::new(&ruleset.ruleset.description).color(theme::TEXT_MUTED));
    }
    ui.add_space(6.0);
    let in_project = app.checking.has_ruleset(&ruleset.ruleset.id);
    ui.horizontal(|ui| {
        let label = if in_project { "Update the project copy" } else { "Add to project" };
        let button = egui::Button::new(RichText::new(format!("{}  {label}", icon::PLUS)).strong().color(Color32::WHITE))
            .fill(theme::ACCENT);
        if ui.add(button).clicked() {
            let _ = app.checking.add_ruleset(&ruleset.to_toml(), None);
            app.session.dirty = true;
            if let Some(i) = app.checking.rulesets.iter().position(|r| r.ruleset.ruleset.id == ruleset.ruleset.id) {
                app.manager.item = Item::Project(i);
                app.manager.rule = Some(0);
            }
        }
        if library_index.is_none() {
            ui.label(
                RichText::new(format!("{}  Built-in rulesets cannot be changed. Add one to the project to edit a copy.", icon::LOCK_SIMPLE))
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        }
        if let Some(i) = library_index
            && ui.button(RichText::new(format!("{}  Delete from library", icon::TRASH)).color(theme::DANGER)).clicked()
        {
            let path = app.manager.library[i].path.clone();
            match library::delete(&path) {
                Ok(()) => {
                    app.manager.library = library::list();
                    app.manager.item = Item::None;
                }
                Err(e) => app.errors.push(format!("Could not delete {}: {e}", path.display())),
            }
        }
    });
    ui.add_space(8.0);
    ui.separator();
    for rule in &ruleset.rules {
        ui.horizontal(|ui| {
            dot(ui, severity_color(rule.severity));
            ui.label(RichText::new(&rule.name).strong());
        });
        ui.label(
            RichText::new(rule_summary(rule))
                .small()
                .color(theme::TEXT_MUTED),
        );
        if !rule.description.is_empty() {
            ui.label(RichText::new(&rule.description).small());
        }
        ui.add_space(6.0);
    }
}

fn project_editor(app: &mut OpenCheckApp, ui: &mut egui::Ui, index: usize) {
    let mut changed = false;
    let mut remove_ruleset = false;
    let mut run_this = false;
    let mut export = false;
    let mut save_library = false;

    {
        let entry = &mut app.checking.rulesets[index];
        let info = &mut entry.ruleset.ruleset;
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut info.name)
                        .font(egui::TextStyle::Heading)
                        .desired_width(420.0),
                )
                .changed();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(RichText::new(icon::TRASH).color(theme::DANGER))
                    .on_hover_text("Remove from project")
                    .clicked()
                {
                    remove_ruleset = true;
                }
                if ui
                    .button(format!("{}  Export…", icon::EXPORT))
                    .on_hover_text("Save as a .toml file")
                    .clicked()
                {
                    export = true;
                }
                if ui
                    .button(format!("{}  Save to library", icon::BOOKMARK_SIMPLE))
                    .on_hover_text("Reuse in other projects")
                    .clicked()
                {
                    save_library = true;
                }
                if ui
                    .button(format!("{}  Run only this", icon::PLAY))
                    .clicked()
                {
                    run_this = true;
                }
            });
        });
        egui::Grid::new("ruleset_info").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("Description").color(theme::TEXT_MUTED));
            changed |= ui.add(egui::TextEdit::singleline(&mut info.description).desired_width(520.0)).changed();
            ui.end_row();
            ui.label(RichText::new("Author").color(theme::TEXT_MUTED));
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut info.author).desired_width(240.0)).changed();
                ui.label(RichText::new("Version").color(theme::TEXT_MUTED));
                changed |= ui.add(egui::TextEdit::singleline(&mut info.version).desired_width(60.0)).changed();
                ui.label(RichText::new("Id").color(theme::TEXT_MUTED));
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut info.id).desired_width(200.0))
                    .on_hover_text("Stable identifier. Changing it disconnects earlier decisions on findings.")
                    .changed();
            });
            ui.end_row();
        });
        if let Err(e) = entry.ruleset.check() {
            ui.label(RichText::new(format!("{}  {e}", icon::WARNING)).color(theme::DANGER));
        }
    }
    ui.separator();

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(240.0);
            changed |= rule_list(app, ui, index);
        });
        ui.separator();
        ui.vertical(|ui| {
            let rules = app.checking.rulesets[index].ruleset.rules.len();
            match app.manager.rule.filter(|r| *r < rules) {
                Some(r) => {
                    egui::ScrollArea::vertical()
                        .id_salt("rule_form")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            changed |= rule_form(app, ui, index, r);
                        });
                }
                None => {
                    ui.label(RichText::new("Select a rule to edit it.").color(theme::TEXT_MUTED));
                }
            }
        });
    });

    if changed {
        let entry = &mut app.checking.rulesets[index];
        entry.text = entry.ruleset.to_toml();
        app.checking.stale = true;
        app.session.dirty = true;
    }
    let ruleset = app.checking.rulesets[index].ruleset.clone();
    if save_library {
        match library::save(&ruleset) {
            Ok(_) => app.manager.library = library::list(),
            Err(e) => app
                .errors
                .push(format!("Could not save to the library: {e}")),
        }
    }
    if export
        && let Some(path) = rfd::FileDialog::new()
            .set_title("Export ruleset")
            .add_filter("OpenCheck ruleset", &["toml"])
            .set_file_name(format!("{}.toml", ruleset.ruleset.name))
            .save_file()
        && let Err(e) = std::fs::write(&path, ruleset.to_toml())
    {
        app.errors.push(format!("Could not export: {e}"));
    }
    if run_this {
        let id = ruleset.ruleset.id.clone();
        app.run_check(ui.ctx(), Some(&id));
    }
    if remove_ruleset {
        app.checking.remove_ruleset(index);
        app.manager.item = Item::None;
        app.session.dirty = true;
    }
}

fn rule_list(app: &mut OpenCheckApp, ui: &mut egui::Ui, index: usize) -> bool {
    let mut changed = false;
    let rules = &mut app.checking.rulesets[index].ruleset.rules;
    ui.label(
        RichText::new(format!("RULES  {}", rules.len()))
            .small()
            .strong()
            .color(theme::TEXT_MUTED),
    );
    egui::ScrollArea::vertical()
        .id_salt("rule_list")
        .max_height((ui.available_height() - 40.0).max(120.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            for (i, rule) in rules.iter().enumerate() {
                ui.horizontal(|ui| {
                    dot(ui, severity_color(rule.severity));
                    let selected = app.manager.rule == Some(i);
                    if list_button(ui, selected, RichText::new(&rule.name)).clicked() {
                        app.manager.rule = Some(i);
                    }
                });
            }
        });
    ui.horizontal(|ui| {
        if ui.button(icon::PLUS).on_hover_text("New rule").clicked() {
            let n = rules.len() + 1;
            let mut rule = new_rule(n);
            while rules.iter().any(|r| r.id == rule.id) {
                rule.id.push('b');
            }
            rules.push(rule);
            app.manager.rule = Some(rules.len() - 1);
            changed = true;
        }
        let selected = app.manager.rule.filter(|r| *r < rules.len());
        if ui
            .add_enabled(selected.is_some(), egui::Button::new(icon::COPY))
            .on_hover_text("Duplicate rule")
            .clicked()
            && let Some(r) = selected
        {
            let mut copy = rules[r].clone();
            copy.name.push_str(" (copy)");
            copy.id.push_str("-copy");
            while rules.iter().any(|x| x.id == copy.id) {
                copy.id.push('2');
            }
            rules.insert(r + 1, copy);
            app.manager.rule = Some(r + 1);
            changed = true;
        }
        if ui
            .add_enabled(
                selected.is_some_and(|r| r > 0),
                egui::Button::new(icon::ARROW_UP),
            )
            .clicked()
            && let Some(r) = selected
        {
            rules.swap(r, r - 1);
            app.manager.rule = Some(r - 1);
            changed = true;
        }
        if ui
            .add_enabled(
                selected.is_some_and(|r| r + 1 < rules.len()),
                egui::Button::new(icon::ARROW_DOWN),
            )
            .clicked()
            && let Some(r) = selected
        {
            rules.swap(r, r + 1);
            app.manager.rule = Some(r + 1);
            changed = true;
        }
        if ui
            .add_enabled(
                selected.is_some() && rules.len() > 1,
                egui::Button::new(RichText::new(icon::TRASH).color(theme::DANGER)),
            )
            .on_hover_text("Delete rule (a ruleset needs at least one)")
            .clicked()
            && let Some(r) = selected
        {
            rules.remove(r);
            app.manager.rule = Some(r.saturating_sub(1));
            changed = true;
        }
    });
    changed
}

fn segmented<T: PartialEq + Copy>(ui: &mut egui::Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (option, label) in options {
            if ui
                .add(egui::Button::selectable(*value == *option, *label))
                .clicked()
                && *value != *option
            {
                *value = *option;
                changed = true;
            }
        }
    });
    changed
}

fn rule_form(app: &mut OpenCheckApp, ui: &mut egui::Ui, index: usize, r: usize) -> bool {
    let mut changed = false;
    // Counts first: they need the federation while the rule is borrowed below.
    let (count_a, count_b) = {
        let rule = &app.checking.rulesets[index].ruleset.rules[r];
        let a = cached_count(&mut app.manager.counts, &app.federation, &rule.a);
        let b = rule
            .b
            .as_ref()
            .map(|b| cached_count(&mut app.manager.counts, &app.federation, b));
        (a, b)
    };
    let manager = &mut app.manager;
    let rule = &mut app.checking.rulesets[index].ruleset.rules[r];

    egui::Grid::new(("rule_fields", index, r))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme::TEXT_MUTED));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut rule.name).desired_width(460.0))
                .changed();
            ui.end_row();

            ui.label(RichText::new("Description").color(theme::TEXT_MUTED));
            changed |= ui
                .add(
                    egui::TextEdit::multiline(&mut rule.description)
                        .desired_rows(2)
                        .desired_width(460.0),
                )
                .changed();
            ui.end_row();

            ui.label(RichText::new("Check").color(theme::TEXT_MUTED));
            changed |= segmented(
                ui,
                &mut rule.kind,
                &[
                    (RuleKind::Clash, "Clash"),
                    (RuleKind::Clearance, "Clearance"),
                    (RuleKind::Duplicates, "Duplicates"),
                ],
            );
            ui.end_row();

            ui.label(RichText::new("Severity").color(theme::TEXT_MUTED));
            changed |= segmented(
                ui,
                &mut rule.severity,
                &[
                    (Severity::Critical, "Critical"),
                    (Severity::Moderate, "Moderate"),
                    (Severity::Low, "Low"),
                ],
            );
            ui.end_row();

            match rule.kind {
                RuleKind::Clearance => {
                    ui.label(RichText::new("Clearance").color(theme::TEXT_MUTED));
                    ui.horizontal(|ui| {
                        if rule.clearance_mm <= 0.0 {
                            rule.clearance_mm = 50.0;
                            changed = true;
                        }
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut rule.clearance_mm)
                                    .range(1.0..=5000.0)
                                    .speed(1.0)
                                    .suffix(" mm"),
                            )
                            .changed();
                        ui.label(
                            RichText::new("Report pairs closer than this")
                                .small()
                                .color(theme::TEXT_MUTED),
                        );
                    });
                }
                RuleKind::Clash | RuleKind::Duplicates => {
                    ui.label(RichText::new("Tolerance").color(theme::TEXT_MUTED));
                    ui.horizontal(|ui| {
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut rule.tolerance_mm)
                                    .range(0.0..=1000.0)
                                    .speed(0.5)
                                    .suffix(" mm"),
                            )
                            .changed();
                        let hint = if rule.kind == RuleKind::Clash {
                            "Ignore intersections up to this depth"
                        } else {
                            "Largest offset between two copies"
                        };
                        ui.label(RichText::new(hint).small().color(theme::TEXT_MUTED));
                    });
                }
            }
            ui.end_row();

            ui.label(RichText::new("Compare").color(theme::TEXT_MUTED));
            changed |= segmented(
                ui,
                &mut rule.between,
                &[
                    (Between::DifferentModels, "Between models"),
                    (Between::SameModel, "Within a model"),
                    (Between::Any, "Any pair"),
                ],
            );
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.columns(2, |columns| {
        group_header(
            &mut columns[0],
            "Group A",
            crate::checking_ui::FOCUS_A,
            count_a,
        );
        changed |= selector_editor(&mut columns[0], ("a", index, r), &mut rule.a, manager, 0);

        let mut separate = rule.b.is_some();
        match count_b {
            Some(count) => group_header(
                &mut columns[1],
                "Group B",
                crate::checking_ui::FOCUS_B,
                count,
            ),
            None => group_header(
                &mut columns[1],
                "Group B",
                crate::checking_ui::FOCUS_B,
                count_a,
            ),
        }
        if columns[1]
            .checkbox(
                &mut separate,
                "Separate group (otherwise group A against itself)",
            )
            .changed()
        {
            rule.b = separate.then(Selector::default);
            changed = true;
        }
        if let Some(b) = &mut rule.b {
            changed |= selector_editor(&mut columns[1], ("b", index, r), b, manager, 1);
        }
    });

    ui.add_space(8.0);
    ui.collapsing(RichText::new("Advanced").small(), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Rule id").color(theme::TEXT_MUTED));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut rule.id).desired_width(240.0))
                .on_hover_text("Keep stable: decisions on findings are linked to it")
                .changed();
        });
    });
    changed
}

fn group_header(ui: &mut egui::Ui, title: &str, color: Color32, count: (usize, usize)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, color);
        ui.label(RichText::new(title).strong());
        let (elements, models) = count;
        let text = format!("matches {elements} elements in {models} models");
        let text = RichText::new(text).small();
        ui.label(if elements == 0 {
            text.color(theme::DANGER)
        } else {
            text.color(theme::TEXT_MUTED)
        });
    });
}

fn cached_count(
    cache: &mut FxHashMap<String, (usize, usize)>,
    federation: &Federation,
    selector: &Selector,
) -> (usize, usize) {
    let key = format!(
        "{}|{}",
        federation.ids().len(),
        toml::to_string(selector).unwrap_or_default()
    );
    *cache.entry(key).or_insert_with(|| {
        let mut elements = 0;
        let mut models = 0;
        for (_, model) in federation.iter() {
            let n = model
                .elements
                .iter()
                .filter(|e| e.has_geometry() && selector.matches(model, e))
                .count();
            elements += n;
            models += usize::from(n > 0);
        }
        (elements, models)
    })
}

fn selector_editor(
    ui: &mut egui::Ui,
    id: (&str, usize, usize),
    selector: &mut Selector,
    manager: &mut ManagerState,
    side: usize,
) -> bool {
    let mut changed = false;
    ui.label(
        RichText::new("Disciplines (none = any)")
            .small()
            .color(theme::TEXT_MUTED),
    );
    ui.horizontal_wrapped(|ui| {
        for d in Discipline::ALL
            .iter()
            .filter(|d| **d != Discipline::Unknown)
        {
            let mut on = selector.disciplines.contains(d);
            if ui.toggle_value(&mut on, d.label()).changed() {
                if on {
                    selector.disciplines.push(*d);
                } else {
                    selector.disciplines.retain(|x| x != d);
                }
                changed = true;
            }
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("IFC classes, subtypes included (none = any)")
            .small()
            .color(theme::TEXT_MUTED),
    );
    changed |= class_list(
        ui,
        (id.0, id.1, id.2, "classes"),
        &mut selector.classes,
        &mut manager.class_input[side],
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Except classes")
            .small()
            .color(theme::TEXT_MUTED),
    );
    changed |= class_list(
        ui,
        (id.0, id.1, id.2, "exclude"),
        &mut selector.exclude_classes,
        &mut manager.exclude_input[side],
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Property conditions (all must hold)")
            .small()
            .color(theme::TEXT_MUTED),
    );
    let mut remove = None;
    for (i, filter) in selector.properties.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut filter.pset)
                        .hint_text("Property set")
                        .desired_width(110.0),
                )
                .changed();
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut filter.name)
                        .hint_text("Property")
                        .desired_width(100.0),
                )
                .changed();
            let mut value = filter.equals.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut value)
                        .hint_text("= value (empty: exists)")
                        .desired_width(110.0),
                )
                .changed()
            {
                filter.equals = (!value.trim().is_empty()).then_some(value);
                changed = true;
            }
            if ui.small_button(icon::X).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        selector.properties.remove(i);
        changed = true;
    }
    if ui
        .small_button(format!("{} Add condition", icon::PLUS))
        .clicked()
    {
        selector.properties.push(PropertyFilter {
            pset: String::new(),
            name: String::new(),
            equals: None,
        });
        changed = true;
    }
    changed
}

/// Chips for a list of classes, with a text field that suggests class names.
fn class_list(
    ui: &mut egui::Ui,
    id: (&str, usize, usize, &str),
    classes: &mut Vec<String>,
    input: &mut String,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (i, class) in classes.iter().enumerate() {
            let chip = egui::Button::new(
                RichText::new(format!("{}  {}", class.trim_start_matches("Ifc"), icon::X)).small(),
            )
            .fill(theme::ACCENT_SOFT);
            if ui
                .add(chip)
                .on_hover_text(format!("{class} — click to remove"))
                .clicked()
            {
                remove = Some(i);
            }
        }
        if let Some(i) = remove {
            classes.remove(i);
            changed = true;
        }
    });
    let edit_id = ui.make_persistent_id(id);
    let response = ui.add(
        egui::TextEdit::singleline(input)
            .id(edit_id)
            .hint_text("Type a class, e.g. Beam, and press Enter")
            .desired_width(f32::INFINITY),
    );
    let typed = input.trim().to_owned();
    let mut add = None;
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !typed.is_empty() {
        add = Some(normalise_class(&typed));
    }
    if !typed.is_empty() && (response.has_focus() || response.lost_focus()) {
        let needle = typed.to_lowercase();
        let suggestions: Vec<&str> = COMMON_CLASSES
            .iter()
            .copied()
            .filter(|c| c.to_lowercase().contains(&needle) && !classes.iter().any(|x| x == c))
            .take(6)
            .collect();
        ui.horizontal_wrapped(|ui| {
            for s in suggestions {
                if ui
                    .small_button(s.trim_start_matches("Ifc"))
                    .on_hover_text(s)
                    .clicked()
                {
                    add = Some(s.to_owned());
                }
            }
        });
    }
    if let Some(class) = add {
        if !classes.contains(&class) {
            classes.push(class);
            changed = true;
        }
        input.clear();
    }
    changed
}

/// `beam` -> `IfcBeam`, `IfcDuctSegment` stays as it is.
fn normalise_class(typed: &str) -> String {
    if let Some(known) = COMMON_CLASSES
        .iter()
        .find(|c| c.eq_ignore_ascii_case(typed) || c[3..].eq_ignore_ascii_case(typed))
    {
        return (*known).to_owned();
    }
    let body = typed
        .strip_prefix("Ifc")
        .or_else(|| typed.strip_prefix("IFC"))
        .unwrap_or(typed);
    let mut chars = body.chars();
    match chars.next() {
        Some(first) => format!("Ifc{}{}", first.to_ascii_uppercase(), chars.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::normalise_class;

    #[test]
    fn class_names_are_normalised() {
        assert_eq!(normalise_class("beam"), "IfcBeam");
        assert_eq!(normalise_class("ductsegment"), "IfcDuctSegment");
        assert_eq!(normalise_class("IfcFooBar"), "IfcFooBar");
        assert_eq!(normalise_class("tendon"), "IfcTendon");
    }
}
