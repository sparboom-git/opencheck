//! The Ruleset Manager: create and edit rulesets and their rules in forms,
//! keep a personal library, and run checks.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{
    BUILTIN_RULESETS, Between, PropertyFilter, Rule, RuleKind, Ruleset, RulesetInfo, RunDirection,
    SCRIPT_API, Selector, Severity, Zone, ZoneSide, compile_script,
};
use opencheck_core::{Discipline, Element, Federation, IfcModel};
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::checking_ui::{dot, rule_summary, severity_color};
use crate::library::{self, LibraryRuleset};
use crate::pickers::{self, Catalogue};
use crate::scene::{ElementRef, Focus};
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
    /// Classes and properties of the loaded models, for the pickers.
    catalogue: Catalogue,
    /// Filter text of the open picker menu.
    picker_filter: String,
    /// Script rules: the language model's answer, pasted to be used.
    ai_answer: String,
    /// Script rules: name of the parameter being added.
    new_param: String,
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
    let dialog = crate::dialog::Dialog {
        id: "ruleset_manager",
        icon: icon::LIST_CHECKS,
        title: "Ruleset Manager",
        size: egui::vec2(1120.0, 740.0),
        min: egui::vec2(700.0, 420.0),
        scroll: crate::dialog::Scroll::Own,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        footer(app, ui);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(250.0);
                item_list(app, ui);
            });
            ui.separator();
            ui.vertical(|ui| match app.manager.item {
                Item::Project(i) if i < app.checking.rulesets.len() => project_editor(app, ui, i),
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
                                .color(theme::danger()),
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
                                .color(theme::text_muted()),
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
                    .color(theme::on_accent()),
            )
            .fill(if can_run {
                theme::accent()
            } else {
                theme::text_muted()
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
                .color(theme::text_muted()),
            );
            if let Some(report) = &app.checking.report {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("Last run: {} findings", report.finding_count()))
                            .small()
                            .color(theme::text_muted()),
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
            ui.label(RichText::new("IN THIS PROJECT").small().strong().color(theme::text_muted()));
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
            ui.label(RichText::new("BUILT-IN").small().strong().color(theme::text_muted()));
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
                ui.label(RichText::new("MY LIBRARY").small().strong().color(theme::text_muted()));
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
                if ui
                    .add(egui::Button::new(RichText::new(icon::DOWNLOAD_SIMPLE).small()).frame(false))
                    .on_hover_text("Import ruleset files (.toml) into the library")
                    .clicked()
                    && let Some(paths) = crate::platform::pick_files(
                        "Import rulesets into the library",
                        ("OpenCheck ruleset", &["toml"]),
                    )
                {
                    let (imported, errors) = library::import(&paths);
                    app.errors.extend(errors);
                    if imported > 0 {
                        app.notify(format!("{imported} rulesets imported into the library."));
                    }
                    app.manager.library = library::list();
                }
            });
            if app.manager.library.is_empty() {
                ui.label(
                    RichText::new("Empty. Use \"Save to library\" on a project ruleset to reuse it in other projects.")
                        .small()
                        .color(theme::text_muted()),
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
                let text = if ok { RichText::new(name) } else { RichText::new(name).color(theme::danger()) };
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
        standard: String::new(),
        zone: None,
        script: String::new(),
        params: Default::default(),
        presort: Default::default(),
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
        ui.label(RichText::new(&ruleset.ruleset.description).color(theme::text_muted()));
    }
    ui.add_space(6.0);
    let in_project = app.checking.has_ruleset(&ruleset.ruleset.id);
    ui.horizontal(|ui| {
        let label = if in_project { "Update the project copy" } else { "Add to project" };
        let button = egui::Button::new(RichText::new(format!("{}  {label}", icon::PLUS)).strong().color(theme::on_accent()))
            .fill(theme::accent());
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
                    .color(theme::text_muted()),
            );
        }
        if let Some(i) = library_index
            && ui.button(RichText::new(format!("{}  Delete from library", icon::TRASH)).color(theme::danger())).clicked()
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
                .color(theme::text_muted()),
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
    let mut duplicate = false;

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
                    .button(RichText::new(icon::TRASH).color(theme::danger()))
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
                    .button(format!("{}  Duplicate", icon::COPY))
                    .on_hover_text(
                        "Make an editable copy with its own id, e.g. to vary a built-in ruleset",
                    )
                    .clicked()
                {
                    duplicate = true;
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
            ui.label(RichText::new("Description").color(theme::text_muted()));
            changed |= ui.add(egui::TextEdit::singleline(&mut info.description).desired_width(520.0)).changed();
            ui.end_row();
            ui.label(RichText::new("Author").color(theme::text_muted()));
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut info.author).desired_width(240.0)).changed();
                ui.label(RichText::new("Version").color(theme::text_muted()));
                changed |= ui.add(egui::TextEdit::singleline(&mut info.version).desired_width(60.0)).changed();
                ui.label(RichText::new("Id").color(theme::text_muted()));
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut info.id).desired_width(200.0))
                    .on_hover_text("Stable identifier. Changing it disconnects earlier decisions on findings.")
                    .changed();
            });
            ui.end_row();
        });
        if let Err(e) = entry.ruleset.check() {
            ui.label(RichText::new(format!("{}  {e}", icon::WARNING)).color(theme::danger()));
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
                    ui.label(RichText::new("Select a rule to edit it.").color(theme::text_muted()));
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
        && let Some(path) = crate::platform::save_file(
            "Export ruleset",
            ("OpenCheck ruleset", &["toml"]),
            &format!("{}.toml", ruleset.ruleset.name),
        )
        && let Err(e) = crate::platform::write(&path, ruleset.to_toml().as_bytes())
    {
        app.errors.push(format!("Could not export: {e}"));
    }
    if duplicate {
        let mut copy = ruleset.clone();
        let token: String = crate::issues::new_guid("ruleset").chars().take(8).collect();
        copy.ruleset.id = format!("project.{token}");
        copy.ruleset.name.push_str(" (copy)");
        if app.checking.add_ruleset(&copy.to_toml(), None).is_ok() {
            app.manager.item = Item::Project(app.checking.rulesets.len() - 1);
            app.manager.rule = Some(0);
            app.session.dirty = true;
        }
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
            .color(theme::text_muted()),
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
                egui::Button::new(RichText::new(icon::TRASH).color(theme::danger())),
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
    // Wraps on narrow windows instead of running off the edge.
    ui.horizontal_wrapped(|ui| {
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
    app.manager.catalogue.refresh(&app.federation);
    // How the rule sees the element selected in the 3D view.
    let tried = app.view.selection.first().and_then(|e| {
        let model = app.federation.get(e.model)?;
        let element = model.element(e.express_id)?;
        let rule = &app.checking.rulesets[index].ruleset.rules[r];
        let name = if element.name.is_empty() {
            element.ifc_class.clone()
        } else {
            element.name.clone()
        };
        Some((
            format!("{name} ({}, {})", element.ifc_class, model.short_name),
            explain(&rule.a, model, element),
            rule.b.as_ref().map(|b| explain(b, model, element)),
        ))
    });
    let mut show_matches = false;
    let manager = &mut app.manager;
    let rule = &mut app.checking.rulesets[index].ruleset.rules[r];

    egui::Grid::new(("rule_fields", index, r))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Name").color(theme::text_muted()));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut rule.name).desired_width(460.0))
                .changed();
            ui.end_row();

            ui.label(RichText::new("Description").color(theme::text_muted()));
            changed |= ui
                .add(
                    egui::TextEdit::multiline(&mut rule.description)
                        .desired_rows(2)
                        .desired_width(460.0),
                )
                .changed();
            ui.end_row();

            ui.label(RichText::new("Check").color(theme::text_muted()));
            changed |= segmented(
                ui,
                &mut rule.kind,
                &[
                    (RuleKind::Clash, "Clash"),
                    (RuleKind::Clearance, "Clearance"),
                    (RuleKind::Duplicates, "Duplicates"),
                    (RuleKind::Information, "Information"),
                    (RuleKind::FreeZone, "Free zone"),
                    (RuleKind::Script, "Script"),
                ],
            );
            if rule.kind == RuleKind::Information && rule.standard.is_empty() {
                rule.standard = crate::checking::INFORMATION_STANDARD.to_owned();
                changed = true;
            }
            if rule.kind == RuleKind::FreeZone && rule.zone.is_none() {
                rule.zone = Some(Zone::default());
                // A zone must be free of everything unless narrowed down.
                rule.b = None;
                changed = true;
            }
            if rule.kind == RuleKind::Script && rule.script.trim().is_empty() {
                rule.script = SCRIPT_TEMPLATE.to_owned();
                rule.params.entry("depth_mm".to_owned()).or_insert(200.0);
                rule.params.entry("max_blocked_mm".to_owned()).or_insert(1000.0);
                rule.b = None;
                changed = true;
            }
            ui.end_row();

            ui.label(RichText::new("Severity").color(theme::text_muted()));
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
                RuleKind::FreeZone => {
                    let zone = rule.zone.get_or_insert_with(Zone::default);
                    changed |= zone_fields(ui, zone);
                }
                RuleKind::Script => {
                    ui.label(RichText::new("Script").color(theme::text_muted()));
                    ui.label(
                        RichText::new("Parameters and code below; a language model can write the code.")
                            .small()
                            .color(theme::text_muted()),
                    );
                }
                RuleKind::Clearance => {
                    ui.label(RichText::new("Clearance").color(theme::text_muted()));
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
                                .color(theme::text_muted()),
                        );
                    });
                }
                RuleKind::Information => {
                    ui.label(RichText::new("Standard").color(theme::text_muted()));
                    let name = opencheck_core::check::builtin_standard(&rule.standard)
                        .map_or(rule.standard.as_str(), |s| s.standard.name.as_str());
                    ui.horizontal(|ui| {
                        ui.label(name).on_hover_text(
                            "Group A limits which elements are checked. The LOD level is chosen for the project on the ruleset card, and per model in the model list.",
                        );
                        if opencheck_core::check::builtin_standard(&rule.standard).is_some()
                            && ui
                                .button(format!("{}  Where to look…", icon::SLIDERS_HORIZONTAL))
                                .on_hover_text(
                                    "Choose which properties, attributes and other sources each item is read from, for all building parts or for one",
                                )
                                .clicked()
                        {
                            crate::standard_editor::request_open(ui.ctx());
                        }
                    });
                }
                RuleKind::Clash | RuleKind::Duplicates => {
                    ui.label(RichText::new("Tolerance").color(theme::text_muted()));
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
                        ui.label(RichText::new(hint).small().color(theme::text_muted()));
                    });
                    if rule.kind == RuleKind::Clash {
                        ui.end_row();
                        ui.label(RichText::new("Pre-sorting").color(theme::text_muted()));
                        ui.vertical(|ui| presort_settings(ui, &mut rule.presort, &mut changed));
                    }
                }
            }
            ui.end_row();

            // Free zones and scripts look at each element of group A on its own.
            if matches!(rule.kind, RuleKind::FreeZone | RuleKind::Script) {
                return;
            }
            ui.label(RichText::new("Compare").color(theme::text_muted()));
            ui.vertical(|ui| {
                if segmented(
                    ui,
                    &mut rule.between,
                    &[
                        (Between::DifferentModels, "Between models"),
                        (Between::SameModel, "Within a model"),
                        (Between::Any, "Any pair"),
                    ],
                ) {
                    changed = true;
                    // Within one model both groups come from the same model:
                    // one discipline for group A, none of its own for group B.
                    if rule.between == Between::SameModel {
                        rule.a.disciplines.truncate(1);
                        if let Some(b) = &mut rule.b {
                            b.disciplines.clear();
                        }
                    }
                }
                if rule.between == Between::SameModel {
                    ui.label(
                        RichText::new("Pairs are only formed inside one model; each model is checked on its own.")
                            .small()
                            .color(theme::text_muted()),
                    );
                }
            });
            ui.end_row();
        });

    ui.add_space(8.0);
    // Free zones and scripts name their groups after what they do.
    let (title_a, title_b, separate_text) = match rule.kind {
        RuleKind::FreeZone => (
            "Checked (group A)",
            "In the way (group B)",
            "Only these can block (otherwise everything)",
        ),
        RuleKind::Script => (
            "subjects() (group A)",
            "others() (group B)",
            "Only these (otherwise everything)",
        ),
        _ => (
            "Group A",
            "Group B",
            "Separate group (otherwise group A against itself)",
        ),
    };
    let everything = matches!(rule.kind, RuleKind::FreeZone | RuleKind::Script);
    ui.columns(2, |columns| {
        group_header(
            &mut columns[0],
            title_a,
            crate::checking_ui::FOCUS_A,
            count_a,
        );
        let within = rule.between == Between::SameModel;
        let (mode_a, mode_b) = if within {
            (Disciplines::One, Disciplines::SameAsA)
        } else {
            (Disciplines::Any, Disciplines::Any)
        };
        changed |= selector_editor(
            &mut columns[0],
            ("a", index, r),
            &mut rule.a,
            manager,
            0,
            mode_a,
        );

        let mut separate = rule.b.is_some();
        match count_b {
            Some(count) => {
                group_header(&mut columns[1], title_b, crate::checking_ui::FOCUS_B, count)
            }
            None if everything => {
                columns[1].horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(rect, 3.0, crate::checking_ui::FOCUS_B);
                    ui.label(RichText::new(title_b).strong());
                    ui.label(
                        RichText::new("everything in the models")
                            .small()
                            .color(theme::text_muted()),
                    );
                });
            }
            None => group_header(
                &mut columns[1],
                title_b,
                crate::checking_ui::FOCUS_B,
                count_a,
            ),
        }
        if columns[1].checkbox(&mut separate, separate_text).changed() {
            rule.b = separate.then(Selector::default);
            changed = true;
        }
        if let Some(b) = &mut rule.b {
            changed |= selector_editor(&mut columns[1], ("b", index, r), b, manager, 1, mode_b);
        }
    });

    if rule.kind == RuleKind::Script {
        ui.add_space(10.0);
        changed |= script_editor(ui, rule, manager);
    }

    ui.add_space(8.0);
    try_panel(ui, tried.as_ref(), &mut show_matches);

    ui.add_space(8.0);
    ui.collapsing(RichText::new("Advanced").small(), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Rule id").color(theme::text_muted()));
            changed |= ui
                .add(egui::TextEdit::singleline(&mut rule.id).desired_width(240.0))
                .on_hover_text("Keep stable: decisions on findings are linked to it")
                .changed();
        });
    });
    if show_matches {
        let rule = app.checking.rulesets[index].ruleset.rules[r].clone();
        show_rule_matches(app, &rule);
    }
    changed
}

/// A new script rule starts from this: a headroom check to adapt.
const SCRIPT_TEMPLATE: &str = r#"// Example: the zone above each element of group A must be free of
// group B, apart from interruptions up to max_blocked_mm.
for e in subjects() {
    if !e.run.horizontal { continue; }
    for s in e.blocked("above", param("depth_mm")) {
        if s.length > param("max_blocked_mm") {
            report(s, `${param("depth_mm")} mm above is blocked over ${s.length} mm`);
        }
    }
}
"#;

/// Fields of a free zone rule.
fn zone_fields(ui: &mut egui::Ui, zone: &mut Zone) -> bool {
    let mut changed = false;
    let hint = |ui: &mut egui::Ui, text: &str| {
        ui.label(RichText::new(text).small().color(theme::text_muted()));
    };
    ui.label(RichText::new("Zone").color(theme::text_muted()));
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("zone_side")
            .selected_text(zone.side.label())
            .show_ui(ui, |ui| {
                for side in ZoneSide::ALL {
                    changed |= ui
                        .selectable_value(&mut zone.side, side, side.label())
                        .changed();
                }
            });
        changed |= ui
            .add(
                egui::DragValue::new(&mut zone.depth_mm)
                    .range(1.0..=10_000.0)
                    .speed(5.0)
                    .suffix(" mm"),
            )
            .changed();
        hint(ui, "deep, along the whole element");
    });
    ui.end_row();

    ui.label(RichText::new("Interruptions").color(theme::text_muted()));
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut zone.max_blocked_mm)
                    .range(0.0..=100_000.0)
                    .speed(10.0)
                    .suffix(" mm"),
            )
            .changed();
        hint(
            ui,
            "the zone may be blocked over at most this length (0: never)",
        );
    });
    ui.end_row();

    ui.label(RichText::new("Always free").color(theme::text_muted()));
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut zone.min_free_mm)
                    .range(0.0..=(zone.depth_mm - 1.0).max(0.0))
                    .speed(1.0)
                    .suffix(" mm"),
            )
            .changed();
        hint(
            ui,
            "even where blocked, at least this much stays free (0: no minimum)",
        );
    });
    ui.end_row();

    ui.label(RichText::new("Elements").color(theme::text_muted()));
    ui.horizontal(|ui| {
        for direction in [
            RunDirection::Any,
            RunDirection::Horizontal,
            RunDirection::Vertical,
        ] {
            changed |= ui
                .selectable_value(&mut zone.direction, direction, direction.label())
                .changed();
        }
        hint(ui, "by the direction they run in");
    });
    ui.end_row();
    changed
}

/// Parameters, code and AI help of a script rule.
fn script_editor(ui: &mut egui::Ui, rule: &mut Rule, manager: &mut ManagerState) -> bool {
    let mut changed = false;
    ui.label(RichText::new("Parameters").strong());
    ui.label(
        RichText::new("Numbers the script reads with param(\"name\"); lengths in mm.")
            .small()
            .color(theme::text_muted()),
    );
    let mut remove = None;
    egui::Grid::new("script_params")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            for (name, value) in rule.params.iter_mut() {
                ui.label(RichText::new(name).monospace());
                changed |= ui.add(egui::DragValue::new(value).speed(1.0)).changed();
                if ui.small_button(icon::X).on_hover_text("Remove").clicked() {
                    remove = Some(name.clone());
                }
                ui.end_row();
            }
        });
    if let Some(name) = remove {
        rule.params.remove(&name);
        changed = true;
    }
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut manager.new_param)
                .hint_text("new_parameter_mm")
                .desired_width(180.0),
        );
        let name = manager.new_param.trim().to_owned();
        let valid = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !rule.params.contains_key(&name);
        if ui
            .add_enabled(valid, egui::Button::new(format!("{}  Add", icon::PLUS)))
            .clicked()
        {
            rule.params.insert(name, 0.0);
            manager.new_param.clear();
            changed = true;
        }
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Code (Rhai)").strong());
    let editor = egui::TextEdit::multiline(&mut rule.script)
        .code_editor()
        .desired_rows(14)
        .desired_width(f32::INFINITY);
    changed |= ui.add(editor).changed();
    match compile_script(&rule.script) {
        Ok(()) => {
            ui.label(
                RichText::new(format!("{}  The code reads correctly", icon::CHECK))
                    .small()
                    .color(theme::solved()),
            );
        }
        Err(e) => {
            ui.label(
                RichText::new(format!("{}  {e}", icon::WARNING))
                    .small()
                    .color(theme::danger()),
            );
        }
    }

    ui.add_space(8.0);
    ui.label(RichText::new(format!("{}  Write the code with AI", icon::SPARKLE)).strong());
    ui.label(
        RichText::new(
            "1. Describe the rule in the description above. 2. Copy the prompt and give it to \
             a language model such as Claude. 3. Paste its answer below and use it: the code \
             and the parameters are filled in.",
        )
        .small()
        .color(theme::text_muted()),
    );
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  Copy prompt", icon::COPY))
            .on_hover_text("The description, the groups, the parameters, the current code and the script reference")
            .clicked()
        {
            ui.ctx().copy_text(ai_prompt(rule));
        }
        if ui
            .button(format!("{}  Copy script reference", icon::BOOK_OPEN))
            .on_hover_text("Only the reference of the script functions")
            .clicked()
        {
            ui.ctx().copy_text(SCRIPT_API.to_owned());
        }
    });
    ui.add(
        egui::TextEdit::multiline(&mut manager.ai_answer)
            .hint_text("Paste the answer here")
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    let answer = parse_ai_answer(&manager.ai_answer);
    ui.horizontal(|ui| {
        let usable = answer.script.is_some() || !answer.params.is_empty();
        if ui
            .add_enabled(
                usable,
                egui::Button::new(format!("{}  Use the answer", icon::ARROW_DOWN)),
            )
            .clicked()
        {
            if let Some(script) = answer.script.clone() {
                rule.script = script;
            }
            for (name, value) in &answer.params {
                rule.params.insert(name.clone(), *value);
            }
            manager.ai_answer.clear();
            changed = true;
        }
        if !manager.ai_answer.trim().is_empty() {
            let found = match (&answer.script, answer.params.len()) {
                (Some(_), 0) => "code found".to_owned(),
                (Some(_), n) => format!("code and {n} parameters found"),
                (None, 0) => "no ```rhai code block found".to_owned(),
                (None, n) => format!("{n} parameters found"),
            };
            ui.label(RichText::new(found).small().color(theme::text_muted()));
        }
    });
    changed
}

/// The prompt for a language model: what the rule should do and how scripts work.
fn ai_prompt(rule: &Rule) -> String {
    let group = |s: Option<&Selector>| match s {
        Some(s) => toml::to_string(s).unwrap_or_default(),
        None => "everything in the models\n".to_owned(),
    };
    let params: String = rule
        .params
        .iter()
        .map(|(k, v)| format!("{k} = {v}\n"))
        .collect();
    format!(
        "Write a script rule for OpenCheck, an IFC model checker. The script reference \
         follows below.\n\n\
         ## What the rule must check\n{}\n\n\
         ## Rule name\n{}\n\n\
         ## Group A, subjects()\n{}\n\
         ## Group B, others()\n{}\n\
         ## Current parameters\n{}\n\
         ## Current code\n```rhai\n{}\n```\n\n\
         ## How to answer\n\
         Answer with exactly two code blocks: first the parameters as\n\
         ```toml\n[rule.params]\nname = value\n```\n\
         with every number the rule needs (lengths in mm, short snake_case names), then the \
         complete script as\n```rhai\n...\n```\n\
         Read every number with param(\"name\") instead of writing it in the code. Keep \
         the code short and explain anything unclear in one or two sentences after the \
         blocks.\n\n\
         ---\n\n{}",
        if rule.description.trim().is_empty() {
            "(not described yet: describe it here)"
        } else {
            rule.description.trim()
        },
        rule.name,
        group(Some(&rule.a)),
        group(rule.b.as_ref()),
        if params.is_empty() {
            "none\n".to_owned()
        } else {
            params
        },
        rule.script.trim(),
        SCRIPT_API
    )
}

#[derive(Default, PartialEq, Debug)]
struct AiAnswer {
    script: Option<String>,
    params: std::collections::BTreeMap<String, f64>,
}

/// The code and parameters in a language model's answer: a ```rhai block and
/// a ```toml block with `[rule.params]`.
fn parse_ai_answer(text: &str) -> AiAnswer {
    let mut answer = AiAnswer::default();
    let mut rest = text;
    while let Some(start) = rest.find("```") {
        let after = &rest[start + 3..];
        let Some(line_end) = after.find('\n') else {
            break;
        };
        let lang = after[..line_end].trim().to_ascii_lowercase();
        let body_start = &after[line_end + 1..];
        let Some(end) = body_start.find("```") else {
            break;
        };
        let body = &body_start[..end];
        rest = &body_start[end + 3..];
        if lang == "toml" || body.contains("[rule.params]") {
            let table = body.replace("[rule.params]", "");
            if let Ok(values) = toml::from_str::<toml::Table>(&table) {
                for (k, v) in values {
                    let number = v.as_float().or_else(|| v.as_integer().map(|i| i as f64));
                    if let Some(n) = number {
                        answer.params.insert(k, n);
                    }
                }
            }
        } else if matches!(lang.as_str(), "rhai" | "rust" | "js" | "javascript" | "")
            && answer.script.is_none()
        {
            answer.script = Some(body.trim_end().to_owned() + "\n");
        }
    }
    answer
}

/// One line of an explanation: whether the condition holds, and how.
type Reason = (bool, String);

/// Whether a selector takes an element, condition by condition.
fn explain(selector: &Selector, model: &IfcModel, element: &Element) -> (bool, Vec<Reason>) {
    let mut reasons = Vec::new();
    let only = |s: Selector| s.matches(model, element);
    if !selector.disciplines.is_empty() {
        let ok = selector.disciplines.contains(&model.discipline);
        reasons.push((
            ok,
            if ok {
                format!("Discipline {} is in the group", model.discipline.label())
            } else {
                format!(
                    "Discipline {} is not one of {}",
                    model.discipline.label(),
                    selector
                        .disciplines
                        .iter()
                        .map(|d| d.label())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            },
        ));
    }
    if !selector.classes.is_empty() {
        let hit = selector.classes.iter().find(|c| {
            only(Selector {
                classes: vec![(*c).clone()],
                ..Default::default()
            })
        });
        reasons.push(match hit {
            Some(c) => (true, format!("{} counts as {c}", element.ifc_class)),
            None => (
                false,
                format!(
                    "{} is none of {}",
                    element.ifc_class,
                    selector.classes.join(", ")
                ),
            ),
        });
    }
    for c in &selector.exclude_classes {
        if only(Selector {
            classes: vec![c.clone()],
            ..Default::default()
        }) {
            reasons.push((
                false,
                format!("Left out: {} counts as {c}", element.ifc_class),
            ));
        }
    }
    for filter in &selector.properties {
        let ok = only(Selector {
            properties: vec![filter.clone()],
            ..Default::default()
        });
        let what = match &filter.equals {
            Some(v) => format!("{}.{} = {v}", filter.pset, filter.name),
            None => format!("{}.{} exists", filter.pset, filter.name),
        };
        let what = what.trim_start_matches('.').to_owned();
        reasons.push((ok, if ok { what } else { format!("Not: {what}") }));
    }
    if !selector.names.is_empty() {
        let ok = only(Selector {
            names: selector.names.clone(),
            ..Default::default()
        });
        let verb = if ok { "matches" } else { "does not match" };
        reasons.push((ok, format!("Name {verb} {}", selector.names.join(" | "))));
    }
    if !selector.materials.is_empty() {
        let ok = only(Selector {
            materials: selector.materials.clone(),
            ..Default::default()
        });
        let verb = if ok { "matches" } else { "does not match" };
        reasons.push((
            ok,
            format!("Material {verb} {}", selector.materials.join(" | ")),
        ));
    }
    if reasons.is_empty() {
        reasons.push((true, "The group takes every element".to_owned()));
    }
    (selector.matches(model, element), reasons)
}

/// The selected element's label and how groups A and B see it.
type Tried = (String, (bool, Vec<Reason>), Option<(bool, Vec<Reason>)>);

/// Explains the rule for the selected element and offers to show all matches.
fn try_panel(ui: &mut egui::Ui, tried: Option<&Tried>, show_matches: &mut bool) {
    egui::Frame::new()
        .fill(theme::surface())
        .corner_radius(8)
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}  TRY THE RULE", icon::CURSOR_CLICK))
                        .small()
                        .strong()
                        .color(theme::text_muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(format!("{}  Show matches in 3D", icon::CUBE_FOCUS))
                        .on_hover_text(
                            "Highlights every element of group A (orange) and group B (blue) \
                             in the 3D view behind this window",
                        )
                        .clicked()
                    {
                        *show_matches = true;
                    }
                });
            });
            let Some((label, a, b)) = tried else {
                ui.label(
                    RichText::new(
                        "Select an element in the 3D view to see whether this rule includes it, and why.",
                    )
                    .small()
                    .color(theme::text_muted()),
                );
                return;
            };
            ui.label(RichText::new(label).strong());
            side_verdict(ui, "group A", crate::checking_ui::FOCUS_A, a);
            match b {
                Some(b) => side_verdict(ui, "group B", crate::checking_ui::FOCUS_B, b),
                None => {
                    ui.label(
                        RichText::new("Group A is checked against itself.")
                            .small()
                            .color(theme::text_muted()),
                    );
                }
            }
        });
}

fn side_verdict(
    ui: &mut egui::Ui,
    title: &str,
    color: Color32,
    (ok, reasons): &(bool, Vec<Reason>),
) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, color);
        let verdict = if *ok {
            RichText::new(format!("{}  In {title}", icon::CHECK)).color(theme::solved())
        } else {
            RichText::new(format!("{}  Not in {title}", icon::X)).color(theme::danger())
        };
        ui.label(verdict.strong());
    });
    for (ok, text) in reasons {
        let (mark, color) = if *ok {
            (icon::CHECK, theme::text_muted())
        } else {
            (icon::X, theme::danger())
        };
        ui.label(
            RichText::new(format!("      {mark}  {text}"))
                .small()
                .color(color),
        );
    }
}

/// Highlights the elements the groups of a rule take, and frames them.
fn show_rule_matches(app: &mut OpenCheckApp, rule: &Rule) {
    let mut focus = Focus::default();
    for (id, model) in app.federation.iter() {
        for element in model.elements.iter().filter(|e| e.has_geometry()) {
            let element_ref = ElementRef {
                model: id,
                express_id: element.express_id,
            };
            if rule.a.matches(model, element) {
                focus.a.insert(element_ref);
            }
            if rule.b.as_ref().is_some_and(|b| b.matches(model, element)) {
                focus.b.insert(element_ref);
            }
        }
    }
    if focus.a.is_empty() && focus.b.is_empty() {
        app.errors
            .push("No element in the loaded models matches the groups of this rule.".to_owned());
        return;
    }
    let all: Vec<ElementRef> = focus.a.iter().chain(&focus.b).copied().collect();
    let bounds = app.view.bounds_of(&all);
    app.checking.selected = None;
    app.issues.selected = None;
    app.focus_marker = None;
    app.view.selection.clear();
    app.view.show_context = false;
    app.view.focus = Some(focus);
    app.view.modes_dirty = true;
    if !bounds.is_empty() {
        app.camera.fit(&bounds, app.nav.aspect);
    }
}

fn group_header(ui: &mut egui::Ui, title: &str, color: Color32, count: (usize, usize)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 3.0, color);
        ui.label(RichText::new(title).strong());
        let (elements, models) = count;
        let plural = |n: usize, word: &str| {
            if n == 1 {
                format!("{n} {word}")
            } else {
                format!("{n} {word}s")
            }
        };
        let text = format!(
            "matches {} in {}",
            plural(elements, "element"),
            plural(models, "model")
        );
        let text = RichText::new(text).small();
        ui.label(if elements == 0 {
            text.color(theme::danger())
        } else {
            text.color(theme::text_muted())
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

/// How a group's disciplines can be chosen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Disciplines {
    /// Any number (none = any).
    Any,
    /// At most one: "within a model" checks one model at a time.
    One,
    /// None of its own: the same model as group A.
    SameAsA,
}

fn selector_editor(
    ui: &mut egui::Ui,
    id: (&str, usize, usize),
    selector: &mut Selector,
    manager: &mut ManagerState,
    side: usize,
    mode: Disciplines,
) -> bool {
    let mut changed = false;
    let title = match mode {
        Disciplines::Any => "Disciplines (none = any)",
        Disciplines::One => "Discipline (one; none = any model)",
        Disciplines::SameAsA => "Discipline: the same model as group A",
    };
    ui.label(RichText::new(title).small().color(theme::text_muted()));
    if mode != Disciplines::SameAsA {
        ui.horizontal_wrapped(|ui| {
            for d in Discipline::ALL
                .iter()
                .filter(|d| **d != Discipline::Unknown)
            {
                let mut on = selector.disciplines.contains(d);
                if ui.toggle_value(&mut on, d.label()).changed() {
                    if on {
                        if mode == Disciplines::One {
                            selector.disciplines.clear();
                        }
                        selector.disciplines.push(*d);
                    } else {
                        selector.disciplines.retain(|x| x != d);
                    }
                    changed = true;
                }
            }
        });
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new("IFC classes, subtypes included (none = any)")
            .small()
            .color(theme::text_muted()),
    );
    let disciplines = selector.disciplines.clone();
    changed |= class_list(
        ui,
        (id.0, id.1, id.2, "classes"),
        &mut selector.classes,
        &mut manager.class_input[side],
        &manager.catalogue,
        &disciplines,
        &mut manager.picker_filter,
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Except classes")
            .small()
            .color(theme::text_muted()),
    );
    changed |= class_list(
        ui,
        (id.0, id.1, id.2, "exclude"),
        &mut selector.exclude_classes,
        &mut manager.exclude_input[side],
        &manager.catalogue,
        &disciplines,
        &mut manager.picker_filter,
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new("Property conditions (all must hold)")
            .small()
            .color(theme::text_muted()),
    );
    let mut remove = None;
    let catalogue = &manager.catalogue;
    let picker_filter = &mut manager.picker_filter;
    for (i, filter) in selector.properties.iter_mut().enumerate() {
        let salt = (id.0, id.1 * 1000 + id.2 * 100 + i);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut filter.pset)
                        .hint_text("Property set")
                        .desired_width(100.0),
                )
                .changed();
            if let Some(pset) = pickers::pset_menu(ui, salt, catalogue, picker_filter) {
                filter.pset = pset;
                picker_filter.clear();
                changed = true;
            }
            ui.add_space(4.0);
            changed |= ui
                .add(
                    egui::TextEdit::singleline(&mut filter.name)
                        .hint_text("Property")
                        .desired_width(90.0),
                )
                .changed();
            if let Some(name) =
                pickers::property_menu(ui, salt, catalogue, &filter.pset, picker_filter)
            {
                filter.name = name;
                picker_filter.clear();
                changed = true;
            }
            ui.add_space(4.0);
            let mut value = filter.equals.clone().unwrap_or_default();
            if ui
                .add(
                    egui::TextEdit::singleline(&mut value)
                        .hint_text("= value (empty: exists)")
                        .desired_width(100.0),
                )
                .changed()
            {
                filter.equals = (!value.trim().is_empty()).then_some(value);
                changed = true;
            }
            if let Some(value) = pickers::value_menu(
                ui,
                salt,
                catalogue,
                &filter.pset,
                &filter.name,
                picker_filter,
            ) {
                filter.equals = Some(value);
                picker_filter.clear();
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
    catalogue: &Catalogue,
    disciplines: &[Discipline],
    picker_filter: &mut String,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        let mut remove = None;
        for (i, class) in classes.iter().enumerate() {
            let chip = egui::Button::new(
                RichText::new(format!("{}  {}", class.trim_start_matches("Ifc"), icon::X)).small(),
            )
            .fill(theme::accent_soft());
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
    let response = ui
        .horizontal(|ui| {
            changed |= pickers::class_menu(ui, classes, catalogue, disciplines, picker_filter);
            ui.add(
                egui::TextEdit::singleline(input)
                    .id(edit_id)
                    .hint_text("or type a class, e.g. Beam, and press Enter")
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    let typed = input.trim().to_owned();
    let mut add = None;
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !typed.is_empty() {
        add = Some(normalise_class(&typed));
    }
    if !typed.is_empty() && (response.has_focus() || response.lost_focus()) {
        // Classes in the loaded models first, then common ones.
        let needle = typed.to_lowercase();
        let mut suggestions: Vec<&str> = Vec::new();
        for c in catalogue.suggest(&typed).chain(
            COMMON_CLASSES
                .iter()
                .copied()
                .filter(|c| c.to_lowercase().contains(&needle)),
        ) {
            if suggestions.len() < 8 && !suggestions.contains(&c) && !classes.iter().any(|x| x == c)
            {
                suggestions.push(c);
            }
        }
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

#[cfg(test)]
mod ai_tests {
    use super::*;

    #[test]
    fn reads_code_and_parameters_from_an_answer() {
        let text = "Here you go:\n```toml\n[rule.params]\nservice_mm = 500\nfactor = 1.5\n```\n\
                    ```rhai\nfor e in subjects() { }\n```\nDone.";
        let answer = parse_ai_answer(text);
        assert_eq!(answer.script.as_deref(), Some("for e in subjects() { }\n"));
        assert_eq!(answer.params.get("service_mm"), Some(&500.0));
        assert_eq!(answer.params.get("factor"), Some(&1.5));
        assert_eq!(parse_ai_answer("no code"), AiAnswer::default());
    }

    #[test]
    fn the_template_compiles() {
        compile_script(SCRIPT_TEMPLATE).unwrap();
    }
}

/// The fixed rules that suggest known noise as C1 after each check.
fn presort_settings(
    ui: &mut egui::Ui,
    presort: &mut opencheck_core::triage::Presort,
    changed: &mut bool,
) {
    *changed |= ui
        .checkbox(
            &mut presort.enabled,
            "Suggest known noise as C1 after each check",
        )
        .on_hover_text(
            "Off: the results are raw. The Pre-sort button in the results runs it on demand.",
        )
        .changed();
    ui.add_enabled_ui(presort.enabled, |ui| {
        ui.horizontal(|ui| {
            *changed |= ui
                .add(
                    egui::DragValue::new(&mut presort.depth_mm)
                        .range(0.0..=200.0)
                        .speed(0.5)
                        .suffix(" mm"),
                )
                .changed();
            ui.label(
                RichText::new("Shallower overlaps are noise (0: off)")
                    .small()
                    .color(theme::text_muted()),
            );
        });
        *changed |= ui
            .checkbox(&mut presort.insulation, "Insulation around its own pipe or duct")
            .changed();
        *changed |= ui
            .checkbox(&mut presort.same_element, "The same element in two models")
            .changed();
        *changed |= ui
            .checkbox(&mut presort.openings, "Penetrations at a planned opening")
            .on_hover_text("IfcOpeningElement, or elements named provision for void, opening, udsparing or hulafsætning")
            .changed();
    });
}
