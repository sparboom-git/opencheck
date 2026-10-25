//! The Visual Check tab: rules that colour elements in the 3D view, with
//! their settings on the left and what they found on the right: a list of
//! problems to fix and the colour legend (groups, systems, storeys).
//!
//! The rules themselves live in `opencheck_core::visual`; this module
//! gathers the models as the view shows them, runs a rule and paints the
//! result.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;
use opencheck_core::visual::{
    self, AlignMode, AlignmentRule, DimensionRule, ElementKey, SlopeLimit, SlopeRule, StoreyRule,
    VisualKind, VisualModel, VisualOutcome, VisualRule,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::app::{OpenCheckApp, Tab};
use crate::checking_ui::{panel_frame, section_title};
use crate::scene::ElementRef;
use crate::theme;

/// State of the tab; the rules themselves are kept with the project.
#[derive(Default)]
pub struct VisualUi {
    /// Index into `checking.visual`.
    pub selected: Option<usize>,
    /// What each rule found, by index into `checking.visual`.
    pub results: FxHashMap<usize, VisualResult>,
    /// Keep the colours on the other tabs.
    pub keep: bool,
    /// Draw the elements the rule does not colour normally, not ghosted.
    pub show_rest: bool,
    /// Analyse only the selected elements.
    pub only_selection: bool,
}

/// What an analysis found.
pub struct VisualResult {
    /// The rule as it was analysed, to tell when its settings changed.
    pub rule: VisualRule,
    pub outcome: VisualOutcome,
    pub colors: FxHashMap<ElementRef, [u8; 3]>,
}

impl VisualUi {
    /// The result shown: the selected rule's.
    fn shown(&self) -> Option<&VisualResult> {
        self.results.get(&self.selected?)
    }
}

fn element_ref(key: &ElementKey) -> ElementRef {
    ElementRef {
        model: key.model,
        express_id: key.express_id,
    }
}

/// Shows or removes the colours, after the tab, the result or the options
/// changed.
pub fn apply_paint(app: &mut OpenCheckApp) {
    let state = &app.visual;
    let shown = state.shown();
    let visible = shown.is_some() && (app.tab == Tab::Visual || state.keep);
    app.view.paint = shown.filter(|_| visible).map(|r| r.colors.clone());
    app.view.paint_ghost_rest = !state.show_rest;
    app.view.modes_dirty = true;
}

fn color32([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn swatch(ui: &mut egui::Ui, color: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, color32(color));
}

fn kind_icon(kind: &VisualKind) -> &'static str {
    match kind {
        VisualKind::Alignment(_) => icon::ALIGN_CENTER_VERTICAL,
        VisualKind::Dimensions(_) => icon::ARROWS_OUT_LINE_HORIZONTAL,
        VisualKind::DrainSlope(_) => icon::DROP,
        VisualKind::Storeys(_) => icon::STACK,
    }
}

// ---- Left: rules and settings --------------------------------------------

pub fn rules_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::left("visual_rules")
        .resizable(true)
        .default_size(360.0)
        .min_size(280.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Visual checks");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(format!("{}  Add", icon::PLUS), |ui| {
                        ui.set_min_width(280.0);
                        for rule in VisualRule::all_kinds() {
                            let text = format!("{}  {}", kind_icon(&rule.kind), rule.name);
                            if ui
                                .button(text)
                                .on_hover_text(rule.kind.description())
                                .clicked()
                            {
                                add_rule(app, rule);
                                ui.close();
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
            if app.checking.visual.is_empty() {
                empty(app, ui);
                return;
            }
            rule_list(app, ui);
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| settings(app, ui));
        });
}

fn add_rule(app: &mut OpenCheckApp, rule: VisualRule) {
    app.checking.visual.push(rule);
    app.visual.selected = Some(app.checking.visual.len() - 1);
    app.session.dirty = true;
    apply_paint(app);
}

fn empty(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Visual checks colour the model instead of only listing findings, so a pattern, \
             or a break in it, shows at a glance. Each check is added on its own.",
        )
        .color(theme::text_muted()),
    );
    ui.add_space(8.0);
    for rule in VisualRule::all_kinds() {
        let button = egui::Button::new(
            RichText::new(format!("{}  {}", kind_icon(&rule.kind), rule.name)).strong(),
        )
        .min_size(egui::vec2(ui.available_width(), 30.0));
        if ui
            .add(button)
            .on_hover_text(rule.kind.description())
            .clicked()
        {
            add_rule(app, rule);
        }
    }
}

fn rule_list(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let mut remove = None;
    let mut select = None;
    let mut changed = false;
    for (i, rule) in app.checking.visual.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .checkbox(&mut rule.enabled, "")
                .on_hover_text("Run with autorun")
                .changed();
            let selected = app.visual.selected == Some(i);
            let text = RichText::new(format!("{}  {}", kind_icon(&rule.kind), rule.name));
            if ui
                .add(egui::Button::selectable(selected, text).truncate())
                .clicked()
            {
                select = Some(i);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icon::TRASH)
                    .on_hover_text("Remove the check")
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if changed {
        app.session.dirty = true;
    }
    if let Some(i) = select {
        app.visual.selected = Some(i);
        apply_paint(app);
    }
    if let Some(i) = remove {
        app.checking.visual.remove(i);
        app.session.dirty = true;
        let state = &mut app.visual;
        state.results = std::mem::take(&mut state.results)
            .into_iter()
            .filter(|(k, _)| *k != i)
            .map(|(k, r)| (if k > i { k - 1 } else { k }, r))
            .collect();
        state.selected = match state.selected {
            Some(s) if s == i => None,
            Some(s) if s > i => Some(s - 1),
            other => other,
        };
        apply_paint(app);
    }
    if app.visual.selected.is_none() && !app.checking.visual.is_empty() {
        app.visual.selected = Some(0);
        apply_paint(app);
    }
}

fn settings(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some(index) = app.visual.selected else {
        return;
    };
    let models: Vec<(Discipline, String)> = app
        .federation
        .iter()
        .map(|(_, m)| (m.discipline, m.short_name.clone()))
        .collect();
    // The pipe systems, for the drain check's list.
    let systems = match app.checking.visual.get(index).map(|r| &r.kind) {
        Some(VisualKind::DrainSlope(rule)) => {
            let owned = view_models(app, false);
            visual::drain_systems(rule, &models_of(app, &owned))
        }
        _ => Vec::new(),
    };
    let selection = app.view.selection.len();
    let Some(rule) = app.checking.visual.get_mut(index) else {
        return;
    };
    let before = rule.clone();
    ui.add_space(4.0);
    ui.label(
        RichText::new(rule.kind.description())
            .small()
            .color(theme::text_muted()),
    );
    ui.add_space(4.0);
    ui.label(RichText::new("Name").small().color(theme::text_muted()));
    ui.add(egui::TextEdit::singleline(&mut rule.name).desired_width(f32::INFINITY));
    match &mut rule.kind {
        VisualKind::Alignment(r) => alignment_settings(ui, r, &models),
        VisualKind::Dimensions(r) => dimension_settings(ui, r, &models),
        VisualKind::DrainSlope(r) => slope_settings(ui, r, &models, &systems),
        VisualKind::Storeys(r) => storey_settings(ui, r, &models),
    }

    ui.add_space(10.0);
    ui.label(RichText::new("Selection").strong());
    let state = &mut app.visual;
    ui.radio_value(
        &mut state.only_selection,
        false,
        "All elements in these models",
    );
    ui.radio_value(
        &mut state.only_selection,
        true,
        format!("Selected elements only ({selection} selected)"),
    );
    if *rule != before {
        app.session.dirty = true;
    }

    ui.add_space(12.0);
    ui.horizontal(|ui| {
        let analyse = egui::Button::new(
            RichText::new(format!("{}  Analyse", icon::PLAY))
                .strong()
                .color(theme::on_accent()),
        )
        .fill(theme::accent())
        .min_size(egui::vec2(110.0, 30.0));
        if ui
            .add(analyse)
            .on_hover_text("Runs the check and colours the model")
            .clicked()
        {
            analyse_rule(app, index);
        }
        if ui
            .add_enabled(
                app.visual.results.contains_key(&index),
                egui::Button::new(format!("{}  Clear colours", icon::ERASER))
                    .min_size(egui::vec2(0.0, 30.0)),
            )
            .clicked()
        {
            app.visual.results.remove(&index);
            apply_paint(app);
        }
    });
    ui.add_space(6.0);
    let state = &mut app.visual;
    let keep = ui
        .checkbox(&mut state.keep, "Keep the colours on the other tabs")
        .changed();
    let rest = ui
        .checkbox(&mut state.show_rest, "Show the other elements normally")
        .on_hover_text("Otherwise they are ghosted, so the colours stand out")
        .changed();
    if keep || rest {
        apply_paint(app);
    }
    if let Some(result) = app.visual.results.get(&index) {
        ui.add_space(8.0);
        let stale = app.checking.visual.get(index) != Some(&result.rule);
        egui::Frame::new()
            .fill(theme::surface())
            .stroke(egui::Stroke::new(1.0, theme::border()))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(format!(
                    "{}  Analysed: {}",
                    icon::INFO,
                    result.outcome.summary
                ));
                if stale {
                    ui.label(
                        RichText::new("The settings changed: analyse again.")
                            .small()
                            .color(theme::danger()),
                    );
                }
            });
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(text).strong());
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().color(theme::text_muted()));
}

/// Checkboxes per discipline, and the models they take in.
fn disciplines(ui: &mut egui::Ui, chosen: &mut Vec<Discipline>, models: &[(Discipline, String)]) {
    heading(ui, "Models");
    ui.horizontal_wrapped(|ui| {
        for d in Discipline::ALL {
            let mut on = chosen.contains(&d);
            if ui.checkbox(&mut on, d.label()).changed() {
                if on {
                    chosen.push(d);
                } else {
                    chosen.retain(|x| *x != d);
                }
            }
        }
    });
    let names: Vec<&str> = models
        .iter()
        .filter(|(d, _)| chosen.contains(d))
        .map(|(_, n)| n.as_str())
        .collect();
    if names.is_empty() {
        ui.label(
            RichText::new(
                "No model of these disciplines is loaded. Set a model's discipline in the Model tab.",
            )
            .small()
            .color(theme::danger()),
        );
    } else {
        hint(ui, &names.join(", "));
    }
}

fn number(
    ui: &mut egui::Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    text: &str,
) {
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(value)
                .range(range)
                .speed(0.5)
                .max_decimals(1)
                .suffix(suffix),
        );
        hint(ui, text);
    });
}

/// A comma separated list in a text field.
fn word_list(ui: &mut egui::Ui, list: &mut Vec<String>, id: &str) {
    let mut text = list.join(", ");
    if ui
        .add(
            egui::TextEdit::multiline(&mut text)
                .id_salt(id)
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        )
        .changed()
    {
        *list = text
            .split(',')
            .map(|w| w.trim().to_owned())
            .filter(|w| !w.is_empty())
            .collect();
    }
}

fn alignment_settings(
    ui: &mut egui::Ui,
    rule: &mut AlignmentRule,
    models: &[(Discipline, String)],
) {
    disciplines(ui, &mut rule.disciplines, models);
    heading(ui, "Elements");
    ui.horizontal(|ui| {
        ui.checkbox(&mut rule.walls, "Walls");
        ui.checkbox(&mut rule.columns, "Columns");
    });
    heading(ui, "Check alignment");
    ui.radio_value(
        &mut rule.mode,
        AlignMode::Centerline,
        "Centerline alignment",
    )
    .on_hover_text("Walls by their centre lines, columns by their centre points");
    ui.radio_value(&mut rule.mode, AlignMode::Face, "Face alignment")
        .on_hover_text(
            "Walls by their faces, so a thinner wall flush with the one below counts as in line. \
         Columns are still compared by their centre points.",
        );
    heading(ui, "Tolerance");
    number(
        ui,
        &mut rule.tolerance_mm,
        0.0..=500.0,
        " mm",
        "largest offset that is still in line",
    );
}

fn dimension_settings(
    ui: &mut egui::Ui,
    rule: &mut DimensionRule,
    models: &[(Discipline, String)],
) {
    disciplines(ui, &mut rule.disciplines, models);
    heading(ui, "Elements");
    ui.horizontal(|ui| {
        ui.checkbox(&mut rule.pipes, "Pipes");
        ui.checkbox(&mut rule.ducts, "Ducts");
    });
    heading(ui, "Tolerance");
    number(
        ui,
        &mut rule.tolerance_percent,
        0.0..=100.0,
        " %",
        "larger than this counts as a jump",
    );
    hint(
        ui,
        "Sizes are measured from the geometry; rectangular ducts by their area, so 400×200 to \
         300×300 is no jump.",
    );
    heading(ui, "Connections");
    number(
        ui,
        &mut rule.connect_mm,
        0.0..=200.0,
        " mm",
        "loose elements meeting within this are connected",
    );
    hint(
        ui,
        "Elements with connected ports are connected by their ports.",
    );
}

fn slope_settings(
    ui: &mut egui::Ui,
    rule: &mut SlopeRule,
    models: &[(Discipline, String)],
    systems: &[visual::DrainSystem],
) {
    disciplines(ui, &mut rule.disciplines, models);
    heading(ui, "Least fall");
    number(
        ui,
        &mut rule.min_permille,
        0.0..=100.0,
        " ‰",
        "for all sizes, unless a limit below fits",
    );
    let mut remove = None;
    for (i, limit) in rule.limits.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.label("Up to");
            ui.add(
                egui::DragValue::new(&mut limit.up_to_mm)
                    .range(0.0..=2000.0)
                    .speed(1.0)
                    .suffix(" mm"),
            );
            ui.label("at least");
            ui.add(
                egui::DragValue::new(&mut limit.min_permille)
                    .range(0.0..=100.0)
                    .speed(0.5)
                    .max_decimals(1)
                    .suffix(" ‰"),
            );
            if ui.small_button(icon::X).on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        rule.limits.remove(i);
    }
    if ui
        .small_button(format!("{}  Limit by pipe size", icon::PLUS))
        .on_hover_text("A different least fall for pipes up to an outer diameter")
        .clicked()
    {
        rule.limits.push(SlopeLimit {
            up_to_mm: 110.0,
            min_permille: rule.min_permille,
        });
    }
    hint(
        ui,
        "Set the values to the project's rules (DS 432 and your own practice).",
    );
    heading(ui, "Tolerance");
    number(
        ui,
        &mut rule.tolerance_permille,
        0.0..=10.0,
        " ‰",
        "less fall than this is flat",
    );

    heading(ui, "Drain systems");
    hint(
        ui,
        "Systems whose name or kind holds one of these words are drains:",
    );
    word_list(ui, &mut rule.words, "drain_words");
    if systems.is_empty() {
        hint(ui, "No pipe systems in the models of these disciplines.");
    } else {
        hint(ui, "Tick or untick a system to set it by hand.");
        egui::ScrollArea::vertical()
            .id_salt("drain_systems")
            .min_scrolled_height(160.0)
            .max_height(220.0)
            .show(ui, |ui| {
                for s in systems {
                    let mut on = s.drain;
                    let label = if s.kind.is_empty() {
                        s.name.clone()
                    } else {
                        format!("{}  ·  {}", s.name, s.kind)
                    };
                    let text = if s.by_hand {
                        RichText::new(format!("{label}  (set by hand)")).strong()
                    } else {
                        RichText::new(label)
                    };
                    if ui
                        .checkbox(&mut on, text)
                        .on_hover_text(format!("{} pipes", s.pipes))
                        .changed()
                    {
                        rule.drains.retain(|n| *n != s.name);
                        rule.not_drains.retain(|n| *n != s.name);
                        // Only a choice against the words is kept.
                        if rule.classify(&s.name, &s.kind).0 != on {
                            if on {
                                rule.drains.push(s.name.clone());
                            } else {
                                rule.not_drains.push(s.name.clone());
                            }
                        }
                    }
                }
            });
    }
}

fn storey_settings(ui: &mut egui::Ui, rule: &mut StoreyRule, models: &[(Discipline, String)]) {
    disciplines(ui, &mut rule.disciplines, models);
    heading(ui, "Tolerance");
    number(
        ui,
        &mut rule.tolerance_mm,
        0.0..=3000.0,
        " mm",
        "how far an element may reach below its storey",
    );
    hint(
        ui,
        "An element stands on the storey its underside is on. Slabs and beams under the deck \
         reach a little below their storey.",
    );
    heading(ui, "May stand anywhere");
    hint(
        ui,
        "IFC classes not judged, e.g. IfcBeam for beams modelled on the storey above:",
    );
    word_list(ui, &mut rule.exempt, "storey_exempt");
}

/// The models as the checks see them: visible ones, without hidden
/// elements and elements outside the working area, and with only the
/// selection when asked. Element sets are owned here; see [`models_of`].
fn view_models(
    app: &OpenCheckApp,
    only_selection: bool,
) -> Vec<(opencheck_core::ModelId, glam::DMat4, Option<FxHashSet<u32>>)> {
    let selection: Option<FxHashSet<ElementRef>> =
        only_selection.then(|| app.view.selection.iter().copied().collect());
    let mut out = Vec::new();
    for (id, model) in app.federation.iter() {
        let Some(view_model) = app.view.model(id).filter(|m| m.visible) else {
            continue;
        };
        let left_out = |express_id: u32| {
            let e = ElementRef {
                model: id,
                express_id,
            };
            app.view.hidden.contains(&e)
                || app.view.outside.contains(&e)
                || selection.as_ref().is_some_and(|s| !s.contains(&e))
        };
        let filtered = selection.is_some()
            || app.view.hidden.iter().any(|e| e.model == id)
            || app.view.outside.iter().any(|e| e.model == id);
        let include = filtered.then(|| {
            model
                .elements
                .iter()
                .map(|e| e.express_id)
                .filter(|&x| !left_out(x))
                .collect()
        });
        out.push((id, view_model.transform.as_dmat4(), include));
    }
    out
}

fn models_of<'a>(
    app: &'a OpenCheckApp,
    owned: &'a [(opencheck_core::ModelId, glam::DMat4, Option<FxHashSet<u32>>)],
) -> Vec<VisualModel<'a>> {
    owned
        .iter()
        .filter_map(|(id, transform, include)| {
            Some(VisualModel {
                id: *id,
                model: app.federation.get(*id)?,
                transform: *transform,
                include: include.as_ref(),
            })
        })
        .collect()
}

/// Runs the rule and colours the model.
fn analyse_rule(app: &mut OpenCheckApp, index: usize) {
    let Some(rule) = app.checking.visual.get(index).cloned() else {
        return;
    };
    let owned = view_models(app, app.visual.only_selection);
    let outcome = visual::run(&rule, &models_of(app, &owned));
    let colors = outcome
        .colors()
        .into_iter()
        .map(|(k, c)| (element_ref(&k), c))
        .collect();
    app.visual.results.insert(
        index,
        VisualResult {
            rule,
            outcome,
            colors,
        },
    );
    apply_paint(app);
}

// ---- Right: what the analysis found --------------------------------------

/// Height of one row in the lists.
const ROW: f32 = 22.0;
/// A problem takes two lines: what is wrong, and the element.
const PROBLEM_ROW: f32 = 36.0;

pub fn results_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::right("visual_results")
        .resizable(true)
        .default_size(380.0)
        .min_size(280.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            section_title(ui, "Result");
            ui.add_space(4.0);
            let Some(result) = app.visual.shown() else {
                ui.label(
                    RichText::new("Choose a check and Analyse to colour the model.")
                        .color(theme::text_muted()),
                );
                return;
            };
            let outcome = &result.outcome;
            ui.label(RichText::new(&outcome.summary).strong());
            if outcome.skipped > 0 {
                hint(
                    ui,
                    &format!("{} without geometry, left out", outcome.skipped),
                );
            }
            for note in &outcome.notes {
                ui.label(RichText::new(note).small().color(theme::text_muted()))
                    .on_hover_text(note);
            }
            ui.add_space(6.0);

            let selection: FxHashSet<ElementRef> = app.view.selection.iter().copied().collect();
            let mut pick: Option<Vec<ElementRef>> = None;
            let label = |key: &ElementKey| {
                app.federation
                    .get(key.model)
                    .and_then(|m| m.element(key.express_id))
                    .map_or_else(|| format!("#{}", key.express_id), |e| e.label().to_owned())
            };
            let storeys = |elements: &[ElementKey]| {
                let names: BTreeSet<&str> = elements
                    .iter()
                    .filter_map(|e| app.view.storey_of(element_ref(e)))
                    .collect();
                match names.len() {
                    0 => String::new(),
                    1 => names.into_iter().next().unwrap_or_default().to_owned(),
                    n => format!("{n} storeys"),
                }
            };
            let row = |ui: &mut egui::Ui,
                       color: [u8; 3],
                       text: String,
                       detail: String,
                       elements: &[ElementKey],
                       hover: &str|
             -> Option<Vec<ElementRef>> {
                let refs: Vec<ElementRef> = elements.iter().map(element_ref).collect();
                let on = !selection.is_empty() && refs.iter().all(|e| selection.contains(e));
                let response = ui
                    .horizontal(|ui| {
                        ui.set_height(ROW);
                        swatch(ui, color);
                        let text = RichText::new(text);
                        ui.add(
                            egui::Button::selectable(on, if on { text.strong() } else { text })
                                .truncate(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(detail).small().color(theme::text_muted()));
                        });
                    })
                    .response;
                ui.interact(response.rect, response.id.with("row"), egui::Sense::click())
                    .on_hover_text(hover)
                    .clicked()
                    .then_some(refs)
            };

            ui.label(
                RichText::new(format!(
                    "{}  ·  {}",
                    outcome.problem_title,
                    outcome.problems.len()
                ))
                .strong(),
            );
            hint(ui, &outcome.problem_hint);
            if outcome.problems.is_empty() {
                ui.label(RichText::new("None").color(theme::text_muted()));
            } else {
                let height = (ui.available_height() * 0.55).max(120.0);
                egui::ScrollArea::vertical()
                    .id_salt("visual_problems")
                    .max_height(height)
                    .auto_shrink([false, true])
                    .show_rows(
                        ui,
                        PROBLEM_ROW + ui.spacing().item_spacing.y,
                        outcome.problems.len(),
                        |ui, rows| {
                            for p in &outcome.problems[rows] {
                                let Some(first) = p.elements.first() else {
                                    continue;
                                };
                                let refs = vec![element_ref(first)];
                                let on = refs.iter().all(|e| selection.contains(e));
                                // What is wrong first, the element under it.
                                let response = ui
                                    .horizontal(|ui| {
                                        ui.set_height(PROBLEM_ROW);
                                        swatch(ui, p.color);
                                        ui.vertical(|ui| {
                                            let text = RichText::new(&p.text);
                                            ui.add(
                                                egui::Button::selectable(
                                                    on,
                                                    if on { text.strong() } else { text },
                                                )
                                                .truncate(),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}  ·  {}",
                                                    label(first),
                                                    storeys(std::slice::from_ref(first))
                                                ))
                                                .small()
                                                .color(theme::text_muted()),
                                            );
                                        });
                                    })
                                    .response;
                                if ui
                                    .interact(
                                        response.rect,
                                        response.id.with("problem"),
                                        egui::Sense::click(),
                                    )
                                    .on_hover_text(format!(
                                        "{}
{}",
                                        p.text,
                                        label(first)
                                    ))
                                    .clicked()
                                {
                                    pick = Some(refs);
                                }
                            }
                        },
                    );
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Colours").strong());
            egui::ScrollArea::vertical()
                .id_salt("visual_categories")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for c in &outcome.categories {
                        let detail = match storeys(&c.elements) {
                            s if s.is_empty() || s == c.label => c.detail.clone(),
                            s => format!("{}  ·  {s}", c.detail),
                        };
                        if let Some(refs) = row(
                            ui,
                            c.color,
                            c.label.clone(),
                            detail,
                            &c.elements,
                            "Select and show them",
                        ) {
                            pick = Some(refs);
                        }
                    }
                });
            if let Some(elements) = pick {
                app.view.selection = elements;
                app.view.modes_dirty = true;
                app.fit_selection();
            }
        });
}
