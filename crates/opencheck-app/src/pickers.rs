//! Pickers for the Ruleset Manager that offer what the loaded models contain
//! (IFC classes, property sets, properties and their values), so rules can be
//! built without remembering names.

use std::collections::{BTreeMap, BTreeSet};

use eframe::egui::containers::menu::{MenuButton, MenuConfig};
use eframe::egui::{self, PopupCloseBehavior, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::{Discipline, Federation};
use rustc_hash::FxHashMap;

use crate::theme;

/// Distinct values kept per property; enough to pick from, small in memory.
const MAX_VALUES: usize = 60;

pub struct ClassEntry {
    pub name: String,
    /// Elements with geometry of this class, per discipline of their model.
    pub counts: FxHashMap<Discipline, usize>,
}

impl ClassEntry {
    /// Elements in the given disciplines; all of them when none are given.
    pub fn count_in(&self, disciplines: &[Discipline]) -> usize {
        self.counts
            .iter()
            .filter(|(d, _)| disciplines.is_empty() || disciplines.contains(d))
            .map(|(_, n)| n)
            .sum()
    }
}

#[derive(Default)]
pub struct PropertyEntry {
    /// Elements that have the property.
    pub count: usize,
    pub values: BTreeSet<String>,
    /// More distinct values exist than are kept.
    pub more: bool,
}

/// What the loaded models contain. Rebuilt when the models change.
#[derive(Default)]
pub struct Catalogue {
    key: String,
    /// Most common first.
    pub classes: Vec<ClassEntry>,
    /// Property set -> property -> what was seen.
    pub psets: BTreeMap<String, BTreeMap<String, PropertyEntry>>,
}

impl Catalogue {
    pub fn refresh(&mut self, federation: &Federation) {
        let key: String = federation
            .iter()
            .map(|(id, m)| format!("{}:{}:{:?};", id.0, m.content_hash, m.discipline))
            .collect();
        if key == self.key {
            return;
        }
        let mut classes: FxHashMap<String, FxHashMap<Discipline, usize>> = FxHashMap::default();
        let mut psets: BTreeMap<String, BTreeMap<String, PropertyEntry>> = BTreeMap::new();
        for (_, model) in federation.iter() {
            for element in model.elements.iter().filter(|e| e.has_geometry()) {
                let occurrence = element.occurrence_class();
                for class in [element.ifc_class.as_str(), occurrence] {
                    *classes
                        .entry(class.to_owned())
                        .or_default()
                        .entry(model.discipline)
                        .or_default() += 1;
                    if occurrence == element.ifc_class {
                        break;
                    }
                }
                for pset in model.property_sets_of(element) {
                    let properties = psets.entry(pset.name.clone()).or_default();
                    for property in &pset.properties {
                        let entry = properties.entry(property.name.clone()).or_default();
                        entry.count += 1;
                        let value = property.value.to_string();
                        if value.trim().is_empty() || entry.values.contains(&value) {
                            continue;
                        }
                        if entry.values.len() < MAX_VALUES {
                            entry.values.insert(value);
                        } else {
                            entry.more = true;
                        }
                    }
                }
            }
        }
        let mut classes: Vec<ClassEntry> = classes
            .into_iter()
            .map(|(name, counts)| ClassEntry { name, counts })
            .collect();
        classes.sort_by(|a, b| {
            b.count_in(&[])
                .cmp(&a.count_in(&[]))
                .then_with(|| a.name.cmp(&b.name))
        });
        *self = Catalogue {
            key,
            classes,
            psets,
        };
    }

    pub fn is_empty(&self) -> bool {
        self.classes.is_empty()
    }

    /// Project classes whose name contains `needle`, for suggestions while typing.
    pub fn suggest<'a>(&'a self, needle: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        let needle = needle.to_lowercase();
        self.classes
            .iter()
            .map(|c| c.name.as_str())
            .filter(move |c| c.to_lowercase().contains(&needle))
    }
}

fn filter_field(ui: &mut egui::Ui, filter: &mut String) {
    let response = ui.add(
        egui::TextEdit::singleline(filter)
            .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
            .desired_width(f32::INFINITY),
    );
    if response.gained_focus() || filter.is_empty() {
        response.request_focus();
    }
}

/// A menu listing the classes in the loaded models (for the selector's
/// disciplines) with element counts. Stays open so several can be picked.
pub fn class_menu(
    ui: &mut egui::Ui,
    classes: &mut Vec<String>,
    catalogue: &Catalogue,
    disciplines: &[Discipline],
    filter: &mut String,
) -> bool {
    let mut changed = false;
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    MenuButton::new(RichText::new(format!("{}  Pick from models", icon::LIST_PLUS)).small())
        .config(config)
        .ui(ui, |ui| {
            ui.set_min_width(300.0);
            if catalogue.is_empty() {
                ui.label(
                    RichText::new("Load models to see their classes.").color(theme::text_muted()),
                );
                return;
            }
            filter_field(ui, filter);
            let needle = filter.to_lowercase();
            let scope = if disciplines.is_empty() {
                "all models".to_owned()
            } else {
                disciplines
                    .iter()
                    .map(|d| d.label())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui.label(
                RichText::new(format!("Classes in {scope}; tick to add"))
                    .small()
                    .color(theme::text_muted()),
            );
            egui::ScrollArea::vertical()
                .id_salt("class_menu")
                .max_height(340.0)
                .show(ui, |ui| {
                    let mut shown = 0;
                    for entry in &catalogue.classes {
                        let count = entry.count_in(disciplines);
                        if count == 0 || !entry.name.to_lowercase().contains(&needle) {
                            continue;
                        }
                        shown += 1;
                        let mut on = classes.contains(&entry.name);
                        let label = format!("{}   {count}", entry.name.trim_start_matches("Ifc"));
                        if ui
                            .checkbox(&mut on, label)
                            .on_hover_text(&entry.name)
                            .changed()
                        {
                            if on {
                                classes.push(entry.name.clone());
                            } else {
                                classes.retain(|c| *c != entry.name);
                            }
                            changed = true;
                        }
                    }
                    if shown == 0 {
                        ui.label(RichText::new("No class matches.").color(theme::text_muted()));
                    }
                });
        });
    changed
}

/// A small ▾ menu next to a text field with a list of names to choose from.
/// Returns the chosen one.
fn choice_menu<'a>(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    hover: &str,
    items: impl Iterator<Item = (&'a str, String)>,
    filter: &mut String,
    empty: &str,
) -> Option<String> {
    let mut chosen = None;
    let mut items = items.peekable();
    let has_items = items.peek().is_some();
    let response = MenuButton::new(RichText::new(icon::CARET_DOWN).small())
        .ui(ui, |ui| {
            ui.set_min_width(260.0);
            if !has_items {
                ui.label(RichText::new(empty).color(theme::text_muted()));
                return;
            }
            filter_field(ui, filter);
            let needle = filter.to_lowercase();
            egui::ScrollArea::vertical()
                .id_salt(salt)
                .max_height(320.0)
                .show(ui, |ui| {
                    for (name, detail) in items.filter(|(n, _)| n.to_lowercase().contains(&needle))
                    {
                        let text = if detail.is_empty() {
                            RichText::new(name)
                        } else {
                            RichText::new(format!("{name}   {detail}"))
                        };
                        if ui.button(text).clicked() {
                            chosen = Some(name.to_owned());
                            ui.close();
                        }
                    }
                });
        })
        .0;
    response.on_hover_text(hover);
    chosen
}

/// Pickers for a property condition: property set, property and value.
/// Each returns the picked text for its field.
pub fn pset_menu(
    ui: &mut egui::Ui,
    salt: (&str, usize),
    catalogue: &Catalogue,
    filter: &mut String,
) -> Option<String> {
    choice_menu(
        ui,
        ("pset", salt),
        "Property sets in the loaded models",
        catalogue
            .psets
            .iter()
            .map(|(name, props)| (name.as_str(), format!("{} properties", props.len()))),
        filter,
        "Load models to see their property sets.",
    )
}

pub fn property_menu(
    ui: &mut egui::Ui,
    salt: (&str, usize),
    catalogue: &Catalogue,
    pset: &str,
    filter: &mut String,
) -> Option<String> {
    let pset = pset.trim();
    // With no set chosen, offer the properties of every set.
    let mut names: BTreeMap<&str, usize> = BTreeMap::new();
    for (name, props) in &catalogue.psets {
        if pset.is_empty() || name.eq_ignore_ascii_case(pset) {
            for (property, entry) in props {
                *names.entry(property.as_str()).or_default() += entry.count;
            }
        }
    }
    choice_menu(
        ui,
        ("property", salt),
        "Properties in the loaded models",
        names.into_iter().map(|(n, count)| (n, format!("{count}×"))),
        filter,
        "No properties found. Choose a property set that is in the models.",
    )
}

pub fn value_menu(
    ui: &mut egui::Ui,
    salt: (&str, usize),
    catalogue: &Catalogue,
    pset: &str,
    property: &str,
    filter: &mut String,
) -> Option<String> {
    let (pset, property) = (pset.trim(), property.trim());
    let mut values: BTreeSet<&str> = BTreeSet::new();
    let mut more = false;
    for (name, props) in &catalogue.psets {
        if !pset.is_empty() && !name.eq_ignore_ascii_case(pset) {
            continue;
        }
        for (p, entry) in props {
            if p.eq_ignore_ascii_case(property) {
                values.extend(entry.values.iter().map(String::as_str));
                more |= entry.more;
            }
        }
    }
    let hover = if more {
        format!("Values in the loaded models (the first {MAX_VALUES} per set)")
    } else {
        "Values in the loaded models".to_owned()
    };
    choice_menu(
        ui,
        ("value", salt),
        &hover,
        values.into_iter().map(|v| (v, String::new())),
        filter,
        "Pick a property first to see its values.",
    )
}
