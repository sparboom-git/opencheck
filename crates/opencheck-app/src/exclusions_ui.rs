//! What a rule leaves out, as a table: each row a discipline's models, a
//! category and a property, chosen in that order. Used for the clash
//! rulesets on the Check tab and for every rule in the Ruleset Manager.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{EXCLUDE_ATTRIBUTES, ExcludeOperator, Exclusion, RuleKind};
use opencheck_core::{Discipline, Federation};
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::pickers::{self, Catalogue};
use crate::theme;

/// Categories offered, by their IFC class (subtypes included).
pub const CATEGORIES: &[(&str, &str)] = &[
    ("Walls", "IfcWall"),
    ("Curtain walls", "IfcCurtainWall"),
    ("Columns", "IfcColumn"),
    ("Beams", "IfcBeam"),
    ("Slabs and floors", "IfcSlab"),
    ("Roofs", "IfcRoof"),
    ("Stairs", "IfcStair"),
    ("Stair flights", "IfcStairFlight"),
    ("Ramps", "IfcRamp"),
    ("Railings", "IfcRailing"),
    ("Doors", "IfcDoor"),
    ("Windows", "IfcWindow"),
    ("Coverings (insulation, ceilings)", "IfcCovering"),
    ("Plates", "IfcPlate"),
    ("Members", "IfcMember"),
    ("Footings", "IfcFooting"),
    ("Piles", "IfcPile"),
    ("Furniture", "IfcFurnishingElement"),
    ("Proxies", "IfcBuildingElementProxy"),
    ("Ducts", "IfcDuctSegment"),
    ("Duct fittings", "IfcDuctFitting"),
    ("Pipes", "IfcPipeSegment"),
    ("Pipe fittings", "IfcPipeFitting"),
    ("Cable carriers", "IfcCableCarrierSegment"),
    ("Cable carrier fittings", "IfcCableCarrierFitting"),
    ("Air terminals", "IfcAirTerminal"),
    ("Light fixtures", "IfcLightFixture"),
    ("All installations", "IfcDistributionElement"),
];

/// The table's state: counts of what each row leaves out, and the
/// filter text of the pickers.
#[derive(Default)]
pub struct State {
    /// The Check tab's window: the ruleset whose exclusions are shown.
    pub open: Option<usize>,
    counts: FxHashMap<String, usize>,
    filter: String,
    /// The row choosing a property in the window: the table and the row.
    picking: Option<(String, usize)>,
    /// The property set shown in that window.
    picking_set: Option<String>,
}

fn category_label(class: &str) -> String {
    if class.trim().is_empty() {
        return "Any category".to_owned();
    }
    CATEGORIES
        .iter()
        .find(|(_, c)| c.eq_ignore_ascii_case(class.trim()))
        .map_or_else(|| class.trim().to_owned(), |(label, _)| (*label).to_owned())
}

/// A window to choose one of the models' properties: the property sets on
/// the left, the chosen set's properties on the right, a search over all of
/// them. Returns `Set.Property`.
fn property_window(
    ctx: &egui::Context,
    catalogue: &Catalogue,
    state: &mut State,
) -> Option<String> {
    let mut open = true;
    let mut picked = None;
    let dialog = crate::dialog::Dialog {
        id: "exclusion_property",
        icon: icon::LIST_MAGNIFYING_GLASS,
        title: "Properties in the models",
        size: egui::vec2(760.0, 560.0),
        min: egui::vec2(420.0, 300.0),
        scroll: crate::dialog::Scroll::Own,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        if catalogue.psets.is_empty() {
            ui.label(
                RichText::new("Load models to see their properties.").color(theme::text_muted()),
            );
            return;
        }
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .hint_text("Search all properties, e.g. fire or system")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        let needle = state.filter.trim().to_lowercase();
        let height = ui.available_height();
        if !needle.is_empty() {
            // A search: every matching property, with its set.
            egui::ScrollArea::vertical()
                .id_salt("property_search")
                .max_height(height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut shown = 0;
                    for (set, properties) in &catalogue.psets {
                        for (name, entry) in properties {
                            let full = format!("{set}.{name}");
                            if !full.to_lowercase().contains(&needle) {
                                continue;
                            }
                            shown += 1;
                            ui.horizontal(|ui| {
                                if ui.selectable_label(false, &full).clicked() {
                                    picked = Some(full.clone());
                                }
                                ui.label(
                                    RichText::new(format!("{} elements", entry.count))
                                        .small()
                                        .color(theme::text_muted()),
                                );
                            });
                        }
                    }
                    if shown == 0 {
                        ui.label(RichText::new("No property matches.").color(theme::text_muted()));
                    }
                });
            return;
        }
        ui.columns(2, |columns| {
            columns[0].label(
                RichText::new("Property sets")
                    .small()
                    .strong()
                    .color(theme::text_muted()),
            );
            egui::ScrollArea::vertical()
                .id_salt("property_sets")
                .max_height(height - 24.0)
                .auto_shrink([false, false])
                .show(&mut columns[0], |ui| {
                    for (set, properties) in &catalogue.psets {
                        let on = state.picking_set.as_deref() == Some(set.as_str());
                        if ui
                            .selectable_label(on, format!("{set}  ({})", properties.len()))
                            .clicked()
                        {
                            state.picking_set = Some(set.clone());
                        }
                    }
                });
            columns[1].label(
                RichText::new("Properties")
                    .small()
                    .strong()
                    .color(theme::text_muted()),
            );
            egui::ScrollArea::vertical()
                .id_salt("property_names")
                .max_height(height - 24.0)
                .auto_shrink([false, false])
                .show(&mut columns[1], |ui| {
                    let Some(set) = state.picking_set.clone() else {
                        ui.label(
                            RichText::new("Choose a property set on the left.")
                                .color(theme::text_muted()),
                        );
                        return;
                    };
                    let Some(properties) = catalogue.psets.get(&set) else {
                        return;
                    };
                    for (name, entry) in properties {
                        let sample: Vec<&str> =
                            entry.values.iter().take(3).map(String::as_str).collect();
                        let response = ui.horizontal(|ui| {
                            let clicked = ui.selectable_label(false, name).clicked();
                            ui.label(
                                RichText::new(format!("{} elements", entry.count))
                                    .small()
                                    .color(theme::text_muted()),
                            );
                            clicked
                        });
                        if response.inner {
                            picked = Some(format!("{set}.{name}"));
                        }
                        if !sample.is_empty() {
                            response
                                .response
                                .on_hover_text(format!("e.g. {}", sample.join(", ")));
                        }
                    }
                });
        });
    });
    if picked.is_some() || !open {
        state.picking = None;
        state.filter.clear();
    }
    picked
}

/// How many elements with geometry a row leaves out in the loaded models.
fn count(state: &mut State, federation: &Federation, row: &Exclusion) -> usize {
    let key = format!(
        "{}|{:?}",
        federation
            .iter()
            .map(|(id, m)| format!("{}:{}:{:?}", id.0, m.content_hash, m.discipline))
            .collect::<String>(),
        row
    );
    *state.counts.entry(key).or_insert_with(|| {
        federation
            .iter()
            .map(|(_, model)| {
                model
                    .elements
                    .iter()
                    .filter(|e| e.has_geometry() && row.matches(model, e))
                    .count()
            })
            .sum()
    })
}

/// The rows of a rule's exclusions, to change; true when something changed.
pub fn table(
    ui: &mut egui::Ui,
    salt: &str,
    rows: &mut Vec<Exclusion>,
    catalogue: &Catalogue,
    federation: &Federation,
    state: &mut State,
) -> bool {
    let mut changed = false;
    let loaded: Vec<(Discipline, String)> = federation
        .iter()
        .map(|(_, m)| (m.discipline, m.short_name.clone()))
        .collect();
    let mut remove = None;
    if rows.is_empty() {
        ui.label(
            RichText::new("Nothing is left out. Add a row to leave elements out of the check.")
                .small()
                .color(theme::text_muted()),
        );
    } else {
        egui::Grid::new(("exclusions", salt))
            .num_columns(8)
            .striped(true)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                for head in [
                    "",
                    "Model",
                    "Category",
                    "Property",
                    "Operator",
                    "Value",
                    "Leaves out",
                    "",
                ] {
                    ui.label(
                        RichText::new(head)
                            .small()
                            .strong()
                            .color(theme::text_muted()),
                    );
                }
                ui.end_row();
                for (i, row) in rows.iter_mut().enumerate() {
                    let id = (salt, i);
                    let mut on = !row.off;
                    if ui
                        .checkbox(&mut on, "")
                        .on_hover_text("Switch the row off without deleting it")
                        .changed()
                    {
                        row.off = !on;
                        changed = true;
                    }
                    // 1. The discipline's models.
                    let model_text = match row.discipline {
                        None => "Any model".to_owned(),
                        Some(d) => d.label().to_owned(),
                    };
                    egui::ComboBox::from_id_salt(("x_model", id))
                        .selected_text(model_text)
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut row.discipline, None, "Any model")
                                .changed();
                            ui.separator();
                            for d in Discipline::ALL {
                                let names: Vec<&str> = loaded
                                    .iter()
                                    .filter(|(md, _)| *md == d)
                                    .map(|(_, n)| n.as_str())
                                    .collect();
                                if names.is_empty() {
                                    continue;
                                }
                                changed |= ui
                                    .selectable_value(&mut row.discipline, Some(d), d.label())
                                    .on_hover_text(names.join(", "))
                                    .changed();
                            }
                        });
                    // 2. The category.
                    egui::ComboBox::from_id_salt(("x_category", id))
                        .selected_text(category_label(&row.category))
                        .width(170.0)
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            changed |= ui
                                .selectable_value(&mut row.category, String::new(), "Any category")
                                .changed();
                            ui.separator();
                            let mut sorted: Vec<&(&str, &str)> = CATEGORIES.iter().collect();
                            sorted.sort_by_key(|(label, _)| label.to_lowercase());
                            for (label, class) in sorted {
                                changed |= ui
                                    .selectable_value(
                                        &mut row.category,
                                        (*class).to_owned(),
                                        *label,
                                    )
                                    .on_hover_text(*class)
                                    .changed();
                            }
                            ui.separator();
                            ui.label(
                                RichText::new("Other IFC class")
                                    .small()
                                    .color(theme::text_muted()),
                            );
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut row.category)
                                        .hint_text("e.g. IfcFlowController")
                                        .desired_width(160.0),
                                )
                                .changed();
                        });
                    // 3. The property.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut row.property)
                                    .hint_text("none")
                                    .desired_width(130.0),
                            )
                            .on_hover_text(
                                "An attribute (Name, Type, Material, System, Tag, Description, \
                                 GlobalId) or a property as Set.Property, or Property in any set",
                            )
                            .changed();
                        ui.menu_button(RichText::new(icon::CARET_DOWN).small(), |ui| {
                            ui.set_min_width(220.0);
                            if ui.button("None").clicked() {
                                row.property.clear();
                                changed = true;
                                ui.close();
                            }
                            ui.separator();
                            for attribute in EXCLUDE_ATTRIBUTES {
                                if ui.button(attribute).clicked() {
                                    row.property = attribute.to_owned();
                                    changed = true;
                                    ui.close();
                                }
                            }
                            ui.separator();
                            if ui
                                .button(format!("{}  Properties in the models…", icon::LIST_MAGNIFYING_GLASS))
                                .on_hover_text("Choose among the property sets and properties of the loaded models")
                                .clicked()
                            {
                                state.picking = Some((salt.to_owned(), i));
                                state.filter.clear();
                                ui.close();
                            }
                        });
                    });
                    // 4. How it compares.
                    ui.add_enabled_ui(!row.property.trim().is_empty(), |ui| {
                        egui::ComboBox::from_id_salt(("x_operator", id))
                            .selected_text(row.operator.label())
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for op in ExcludeOperator::ALL {
                                    changed |= ui
                                        .selectable_value(&mut row.operator, op, op.label())
                                        .changed();
                                }
                            });
                    });
                    // 5. The value.
                    let needs_value =
                        !row.property.trim().is_empty() && row.operator != ExcludeOperator::Exists;
                    ui.add_enabled_ui(needs_value, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut row.value).desired_width(110.0),
                                )
                                .changed();
                            let (set, name) = match row.property.split_once('.') {
                                Some((s, n)) => (s.to_owned(), n.to_owned()),
                                None => (String::new(), row.property.clone()),
                            };
                            if !EXCLUDE_ATTRIBUTES.contains(&row.property.trim())
                                && let Some(value) = pickers::value_menu(
                                    ui,
                                    (salt, i),
                                    catalogue,
                                    &set,
                                    &name,
                                    &mut state.filter,
                                )
                            {
                                row.value = value;
                                state.filter.clear();
                                changed = true;
                            }
                        });
                    });
                    let n = if row.is_set() && !row.off {
                        count(state, federation, row).to_string()
                    } else {
                        "–".to_owned()
                    };
                    ui.label(RichText::new(n).small().color(theme::text_muted()))
                        .on_hover_text(
                            "Elements with geometry in the loaded models this row leaves out",
                        );
                    if ui
                        .small_button(RichText::new(icon::TRASH).color(theme::danger()))
                        .on_hover_text("Delete the row")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
    }
    // The window for the models' properties, for the row that asked.
    if let Some((table, row)) = state.picking.clone()
        && table == salt
        && let Some(picked) = property_window(ui.ctx(), catalogue, state)
        && let Some(r) = rows.get_mut(row)
    {
        r.property = picked;
        changed = true;
    }
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    if ui
        .button(format!("{}  Add row", icon::PLUS))
        .on_hover_text("Leave out elements: choose the model, then the category, then a property")
        .clicked()
    {
        rows.push(Exclusion::default());
        changed = true;
    }
    changed
}

/// Rules that can leave elements out: all but the information rules.
pub fn takes_exclusions(kind: RuleKind) -> bool {
    kind != RuleKind::Information
}

/// The Check tab's window: the exclusions of a ruleset's rules.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let Some(index) = app.exclusions.open else {
        return;
    };
    let Some(entry) = app.checking.rulesets.get(index) else {
        app.exclusions.open = None;
        return;
    };
    let title = format!("Leave out of {}", entry.ruleset.ruleset.name);
    let mut open = true;
    let dialog = crate::dialog::Dialog {
        id: "exclusions",
        icon: icon::FUNNEL,
        title: &title,
        size: egui::vec2(980.0, 460.0),
        min: egui::vec2(560.0, 260.0),
        scroll: crate::dialog::Scroll::Both,
    };
    app.manager.catalogue.refresh(&app.federation);
    let mut changed = false;
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        ui.label(
            RichText::new(
                "Each row leaves out elements: first the discipline's models, then the category, \
                 then, if needed, a property. An element is left out when it matches every \
                 column set in a row. The rows apply to both sides of the check. Run the check \
                 again to see the result.",
            )
            .small()
            .color(theme::text_muted()),
        );
        ui.add_space(6.0);
        let entry = &mut app.checking.rulesets[index];
        let several = entry
            .ruleset
            .rules
            .iter()
            .filter(|r| takes_exclusions(r.kind))
            .count()
            > 1;
        for (r, rule) in entry.ruleset.rules.iter_mut().enumerate() {
            if !takes_exclusions(rule.kind) {
                continue;
            }
            if several {
                ui.add_space(6.0);
                ui.label(RichText::new(&rule.name).strong());
            }
            changed |= table(
                ui,
                &format!("check_{index}_{r}"),
                &mut rule.exclude,
                &app.manager.catalogue,
                &app.federation,
                &mut app.exclusions,
            );
        }
    });
    if changed {
        // The ruleset's text is what the project saves and autorun reads.
        app.checking.rulesets[index].sync_text();
        app.checking.stale = true;
        app.session.dirty = true;
    }
    if !open {
        app.exclusions.open = None;
    }
}
