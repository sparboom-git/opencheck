//! The Visual Check tab: rules that colour elements in the 3D view, with
//! their settings on the left and what they found on the right.
//!
//! The first rule is the alignment check of structural walls and columns:
//! each group of elements in line with each other gets a colour of its own,
//! elements out of line with the one above or below are red, and elements
//! with nothing above or below are grey.

use std::collections::BTreeSet;

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use glam::DVec3;
use opencheck_core::Discipline;
use opencheck_core::visual::{
    AlignElement, AlignMode, AlignmentRule, VisualKind, VisualRule, align, member_kind,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::app::{OpenCheckApp, Tab};
use crate::checking_ui::{panel_frame, section_title};
use crate::scene::ElementRef;
use crate::theme;

/// Out of line with the element above or below.
const OFFSET: [u8; 3] = [214, 39, 40];
/// Nothing above or below to line up with.
const ALONE: [u8; 3] = [196, 192, 184];

/// State of the tab; the rules themselves are kept with the project.
#[derive(Default)]
pub struct VisualUi {
    /// Index into `checking.visual`.
    pub selected: Option<usize>,
    pub result: Option<VisualResult>,
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
    /// Groups of two or more elements in line, largest first.
    pub groups: Vec<Group>,
    pub offset: Vec<ElementRef>,
    pub alone: Vec<ElementRef>,
    /// Elements without geometry to measure.
    pub skipped: usize,
    pub colors: FxHashMap<ElementRef, [u8; 3]>,
}

pub struct Group {
    pub color: [u8; 3],
    pub elements: Vec<ElementRef>,
}

impl VisualResult {
    fn analysed(&self) -> usize {
        self.groups.iter().map(|g| g.elements.len()).sum::<usize>()
            + self.offset.len()
            + self.alone.len()
    }
}

/// Shows or removes the colours, after the tab, the result or the options
/// changed.
pub fn apply_paint(app: &mut OpenCheckApp) {
    let state = &app.visual;
    let show = state.result.is_some() && (app.tab == Tab::Visual || state.keep);
    app.view.paint = if show {
        state.result.as_ref().map(|r| r.colors.clone())
    } else {
        None
    };
    app.view.paint_ghost_rest = !state.show_rest;
    app.view.modes_dirty = true;
}

/// A colour per group: hues spread by the golden ratio, leaving out the
/// reds used for elements out of line.
fn group_color(index: usize) -> [u8; 3] {
    let mut hue = (0.13 + index as f32 * 0.618_034).fract();
    if !(0.05..=0.92).contains(&hue) {
        hue = (hue + 0.15).fract();
    }
    let value = if index.is_multiple_of(2) { 0.86 } else { 0.68 };
    let saturation = if index % 3 == 2 { 0.45 } else { 0.62 };
    hsv(hue, saturation, value)
}

fn hsv(h: f32, s: f32, v: f32) -> [u8; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    let (r, g, b) = match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|c| (c * 255.0).round() as u8)
}

fn color32([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn swatch(ui: &mut egui::Ui, color: [u8; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, color32(color));
}

// ---- Left: rules and settings --------------------------------------------

pub fn rules_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::left("visual_rules")
        .resizable(true)
        .default_size(340.0)
        .min_size(280.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                section_title(ui, "Visual checks");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.menu_button(format!("{}  Add", icon::PLUS), |ui| {
                        ui.set_min_width(260.0);
                        if ui
                            .button(format!("{}  Alignment of walls and columns", icon::ALIGN_CENTER_VERTICAL))
                            .on_hover_text("Colours walls and columns that stand in line with each other, from storey to storey")
                            .clicked()
                        {
                            add_rule(app, VisualRule::alignment());
                            ui.close();
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
}

fn empty(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(
            "Visual checks colour the model instead of listing findings, so a pattern, \
             or a break in it, shows at a glance.",
        )
        .color(theme::text_muted()),
    );
    ui.add_space(8.0);
    let button = egui::Button::new(
        RichText::new(format!(
            "{}  Alignment of walls and columns",
            icon::ALIGN_CENTER_VERTICAL
        ))
        .strong(),
    )
    .min_size(egui::vec2(ui.available_width(), 30.0));
    if ui
        .add(button)
        .on_hover_text("Colours structural walls and columns that stand in line with each other")
        .clicked()
    {
        add_rule(app, VisualRule::alignment());
    }
}

fn rule_list(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let mut remove = None;
    for (i, rule) in app.checking.visual.iter().enumerate() {
        ui.horizontal(|ui| {
            let selected = app.visual.selected == Some(i);
            let text = RichText::new(format!("{}  {}", icon::ALIGN_CENTER_VERTICAL, rule.name));
            if ui
                .add(egui::Button::selectable(selected, text).truncate())
                .clicked()
            {
                app.visual.selected = Some(i);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icon::TRASH)
                    .on_hover_text("Remove the rule")
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        });
    }
    if let Some(i) = remove {
        app.checking.visual.remove(i);
        app.session.dirty = true;
        app.visual.selected = match app.visual.selected {
            Some(s) if s == i => None,
            Some(s) if s > i => Some(s - 1),
            other => other,
        };
        if app
            .visual
            .result
            .as_ref()
            .is_some_and(|r| !app.checking.visual.contains(&r.rule))
        {
            app.visual.result = None;
            apply_paint(app);
        }
    }
    if app.visual.selected.is_none() && !app.checking.visual.is_empty() {
        app.visual.selected = Some(0);
    }
}

fn settings(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some(index) = app.visual.selected else {
        return;
    };
    // Disciplines' models, for the list below.
    let models: Vec<(Discipline, String)> = app
        .federation
        .iter()
        .map(|(_, m)| (m.discipline, m.short_name.clone()))
        .collect();
    let selected = app.view.selection.clone();
    let Some(rule) = app.checking.visual.get_mut(index) else {
        return;
    };
    let before = rule.clone();
    ui.add_space(4.0);
    ui.label(RichText::new("Name").small().color(theme::text_muted()));
    ui.add(egui::TextEdit::singleline(&mut rule.name).desired_width(f32::INFINITY));
    let VisualKind::Alignment(align_rule) = &mut rule.kind;
    alignment_settings(ui, align_rule, &models);

    ui.add_space(10.0);
    ui.label(RichText::new("Selection").strong());
    let state = &mut app.visual;
    ui.radio_value(
        &mut state.only_selection,
        false,
        "All of them in these models",
    );
    let will = selected
        .iter()
        .filter(|e| {
            app.federation
                .get(e.model)
                .is_some_and(|m| align_rule.disciplines.contains(&m.discipline))
                && app
                    .federation
                    .get(e.model)
                    .and_then(|m| m.element(e.express_id))
                    .is_some_and(|el| align_rule.includes_class(&el.ifc_class))
        })
        .count();
    ui.radio_value(
        &mut state.only_selection,
        true,
        format!(
            "Selected elements only ({} selected, {will} will be analysed)",
            selected.len()
        ),
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
            .on_hover_text("Groups the elements and colours them in the 3D view")
            .clicked()
        {
            analyse_rule(app, index);
        }
        if ui
            .add_enabled(
                app.visual.result.is_some(),
                egui::Button::new(format!("{}  Clear colours", icon::ERASER))
                    .min_size(egui::vec2(0.0, 30.0)),
            )
            .clicked()
        {
            app.visual.result = None;
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
    if let Some(result) = &app.visual.result {
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
                    "{}  Analysed: {} groups ({} elements)",
                    icon::INFO,
                    result.groups.len(),
                    result.analysed()
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

fn alignment_settings(
    ui: &mut egui::Ui,
    rule: &mut AlignmentRule,
    models: &[(Discipline, String)],
) {
    ui.add_space(10.0);
    ui.label(RichText::new("Models").strong());
    ui.horizontal_wrapped(|ui| {
        for d in Discipline::ALL {
            let mut on = rule.disciplines.contains(&d);
            if ui.checkbox(&mut on, d.label()).changed() {
                if on {
                    rule.disciplines.push(d);
                } else {
                    rule.disciplines.retain(|x| *x != d);
                }
            }
        }
    });
    let names: Vec<&str> = models
        .iter()
        .filter(|(d, _)| rule.disciplines.contains(d))
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
        ui.label(
            RichText::new(names.join(", "))
                .small()
                .color(theme::text_muted()),
        );
    }

    ui.add_space(10.0);
    ui.label(RichText::new("Elements").strong());
    ui.horizontal(|ui| {
        ui.checkbox(&mut rule.walls, "Walls");
        ui.checkbox(&mut rule.columns, "Columns");
    });

    ui.add_space(10.0);
    ui.label(RichText::new("Check alignment").strong());
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

    ui.add_space(10.0);
    ui.label(RichText::new("Tolerance").strong());
    ui.horizontal(|ui| {
        ui.add(
            egui::DragValue::new(&mut rule.tolerance_mm)
                .range(0.0..=500.0)
                .speed(0.5)
                .max_decimals(1)
                .suffix(" mm"),
        );
        ui.label(
            RichText::new("largest offset that is still in line")
                .small()
                .color(theme::text_muted()),
        );
    });
}

/// Runs the rule and colours the model.
fn analyse_rule(app: &mut OpenCheckApp, index: usize) {
    let Some(rule) = app.checking.visual.get(index).cloned() else {
        return;
    };
    let VisualKind::Alignment(settings) = &rule.kind;
    let only: Option<FxHashSet<ElementRef>> = app
        .visual
        .only_selection
        .then(|| app.view.selection.iter().copied().collect());

    let mut refs = Vec::new();
    let mut elements = Vec::new();
    for (id, model) in app.federation.iter() {
        if !settings.disciplines.contains(&model.discipline) {
            continue;
        }
        let Some(view_model) = app.view.model(id).filter(|m| m.visible) else {
            continue;
        };
        let transform = view_model.transform.as_dmat4();
        let mut slot = FxHashMap::default();
        for e in &model.elements {
            let element = ElementRef {
                model: id,
                express_id: e.express_id,
            };
            let Some(kind) = member_kind(&e.ifc_class) else {
                continue;
            };
            if !settings.includes_class(&e.ifc_class)
                || app.view.hidden.contains(&element)
                || app.view.outside.contains(&element)
                || only.as_ref().is_some_and(|s| !s.contains(&element))
            {
                continue;
            }
            slot.insert(e.express_id, elements.len());
            refs.push(element);
            elements.push(AlignElement {
                kind,
                points: Vec::new(),
            });
        }
        for mesh in &model.meshes {
            if let Some(&i) = slot.get(&mesh.express_id) {
                elements[i].points.extend(
                    mesh.positions
                        .iter()
                        .map(|p| transform.transform_point3(DVec3::from(p.map(f64::from)))),
                );
            }
        }
    }

    let alignment = align(&elements, settings.mode, settings.tolerance_mm / 1000.0);
    let mut members: Vec<Vec<ElementRef>> = vec![Vec::new(); alignment.groups];
    let mut skipped = 0;
    for (i, group) in alignment.group.iter().enumerate() {
        match group {
            Some(g) => members[*g].push(refs[i]),
            None => skipped += 1,
        }
    }
    let mut result = VisualResult {
        rule: rule.clone(),
        groups: Vec::new(),
        offset: Vec::new(),
        alone: Vec::new(),
        skipped,
        colors: FxHashMap::default(),
    };
    for (i, group) in alignment.group.iter().enumerate() {
        if let Some(g) = group
            && members[*g].len() == 1
        {
            let list = if alignment.stacked[i] {
                &mut result.offset
            } else {
                &mut result.alone
            };
            list.push(refs[i]);
        }
    }
    // Groups come largest first, so the singles are at the end.
    for list in members.into_iter().filter(|m| m.len() > 1) {
        let color = group_color(result.groups.len());
        for e in &list {
            result.colors.insert(*e, color);
        }
        result.groups.push(Group {
            color,
            elements: list,
        });
    }
    for e in &result.offset {
        result.colors.insert(*e, OFFSET);
    }
    for e in &result.alone {
        result.colors.insert(*e, ALONE);
    }
    app.visual.result = Some(result);
    apply_paint(app);
}

// ---- Right: what the analysis found --------------------------------------

pub fn results_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::right("visual_results")
        .resizable(true)
        .default_size(360.0)
        .min_size(280.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            section_title(ui, "Result");
            ui.add_space(4.0);
            let Some(result) = &app.visual.result else {
                ui.label(
                    RichText::new("Choose a rule and Analyse to colour the model.")
                        .color(theme::text_muted()),
                );
                return;
            };
            ui.label(
                RichText::new(format!(
                    "{} groups ({} elements)",
                    result.groups.len(),
                    result.analysed()
                ))
                .strong(),
            );
            if result.skipped > 0 {
                ui.label(
                    RichText::new(format!("{} without geometry, left out", result.skipped))
                        .small()
                        .color(theme::text_muted()),
                );
            }
            ui.add_space(6.0);
            let mut pick: Option<Vec<ElementRef>> = None;
            let selection: FxHashSet<ElementRef> = app.view.selection.iter().copied().collect();
            let row = |ui: &mut egui::Ui,
                       color: [u8; 3],
                       text: String,
                       detail: String,
                       elements: &[ElementRef],
                       hover: &str| {
                let on = !selection.is_empty() && elements.iter().all(|e| selection.contains(e));
                let response = ui
                    .horizontal(|ui| {
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
                let clicked = ui
                    .interact(response.rect, response.id.with("row"), egui::Sense::click())
                    .on_hover_text(hover)
                    .clicked();
                clicked.then(|| elements.to_vec())
            };
            let storeys = |elements: &[ElementRef]| {
                let names: BTreeSet<&str> = elements
                    .iter()
                    .filter_map(|e| app.view.storey_of(*e))
                    .collect();
                match names.len() {
                    0 => String::new(),
                    1 => names.into_iter().next().unwrap_or_default().to_owned(),
                    n => format!("{n} storeys"),
                }
            };
            let label = |e: &ElementRef| {
                app.federation
                    .get(e.model)
                    .and_then(|m| m.element(e.express_id))
                    .map_or_else(|| format!("#{}", e.express_id), |el| el.label().to_owned())
            };

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(RichText::new("Out of line").strong());
                    ui.label(
                        RichText::new(
                            "In line with nothing, though a wall or column stands above or below.",
                        )
                        .small()
                        .color(theme::text_muted()),
                    );
                    if result.offset.is_empty() {
                        ui.label(RichText::new("None").color(theme::text_muted()));
                    }
                    for e in &result.offset {
                        if let Some(p) = row(
                            ui,
                            OFFSET,
                            label(e),
                            storeys(std::slice::from_ref(e)),
                            std::slice::from_ref(e),
                            "Select and show it",
                        ) {
                            pick = Some(p);
                        }
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("In line").strong());
                    for (n, group) in result.groups.iter().enumerate() {
                        if let Some(p) = row(
                            ui,
                            group.color,
                            format!("Group {}  ·  {} elements", n + 1, group.elements.len()),
                            storeys(&group.elements),
                            &group.elements,
                            "Select and show the group",
                        ) {
                            pick = Some(p);
                        }
                    }
                    if !result.alone.is_empty() {
                        ui.add_space(10.0);
                        if let Some(p) = row(
                            ui,
                            ALONE,
                            format!("Nothing above or below  ·  {}", result.alone.len()),
                            String::new(),
                            &result.alone,
                            "Select and show them",
                        ) {
                            pick = Some(p);
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
