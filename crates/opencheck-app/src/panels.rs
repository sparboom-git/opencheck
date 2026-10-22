//! Side panels: model list and tree on the left, properties on the right.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::{Discipline, Element, IfcModel, ModelId, PropertySetKind, SpatialNode};

use crate::app::OpenCheckApp;
use crate::render::DisplayMode;
use crate::scene::ElementRef;
use crate::theme;

const MAX_SEARCH_RESULTS: usize = 400;

fn model_color(app: &OpenCheckApp, id: ModelId) -> Color32 {
    let index = app
        .federation
        .ids()
        .iter()
        .position(|m| *m == id)
        .unwrap_or(0);
    theme::MODEL_COLORS[index % theme::MODEL_COLORS.len()]
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .small()
            .strong()
            .color(theme::text_muted()),
    );
}

pub fn model_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::left("model_panel")
        .resizable(true)
        .default_size(320.0)
        .min_size(240.0)
        .frame(
            egui::Frame::new()
                .fill(theme::panel())
                .inner_margin(egui::Margin::same(12))
                .stroke(egui::Stroke::new(1.0, theme::border())),
        )
        .show(ui, |ui| {
            if !app.federation.is_empty() {
                area_controls(app, ui);
            }
            section_title(ui, "Models");
            models_list(app, ui);
            ui.add_space(8.0);
            ui.separator();
            section_title(ui, "Model tree");
            ui.add(
                egui::TextEdit::singleline(&mut app.tree_filter)
                    .hint_text(format!(
                        "{}  Search name, class or GlobalId",
                        icon::MAGNIFYING_GLASS
                    ))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Long names are cut off instead of widening the panel.
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    if app.tree_filter.trim().is_empty() {
                        model_tree(app, ui);
                    } else {
                        search_results(app, ui);
                    }
                });
        });
}

fn models_list(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    if app.federation.is_empty() {
        ui.label(RichText::new("No models loaded yet.").color(theme::text_muted()));
        if ui
            .button(format!("{}  Add models…", icon::FOLDER_OPEN))
            .clicked()
        {
            app.open_file_dialog(ui.ctx());
        }
        return;
    }
    let mut remove = None;
    let mut visibility_change = None;
    let mut discipline_change = None;
    let mut update = None;
    let mut level_change = None;
    let project_level = app.checking.project_level();
    let levels = crate::checking::information_levels();
    for id in app.federation.ids() {
        let own_level = app.checking.model_levels.get(&id).cloned();
        let color = model_color(app, id);
        let visible = app.view.model(id).is_some_and(|m| m.visible);
        let Some(model) = app.federation.get(id) else {
            continue;
        };
        let mut discipline = model.discipline;
        let (revision, updated) = app
            .session
            .records
            .get(&id)
            .map(|r| (r.entry.revision, r.entry.updated.clone()))
            .unwrap_or((1, String::new()));
        egui::Frame::new()
            .fill(if visible {
                theme::surface()
            } else {
                theme::panel()
            })
            .corner_radius(8)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let eye = if visible { icon::EYE } else { icon::EYE_CLOSED };
                    let eye_color = if visible {
                        theme::text()
                    } else {
                        theme::text_muted()
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(eye).size(17.0).color(eye_color))
                                .frame(false),
                        )
                        .on_hover_text(if visible { "Hide model" } else { "Show model" })
                        .clicked()
                    {
                        visibility_change = Some((id, !visible));
                    }
                    let (dot, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 5.0, color);
                    ui.vertical(|ui| {
                        let name = RichText::new(&model.short_name).strong();
                        let name = if visible {
                            name
                        } else {
                            name.color(theme::text_muted())
                        };
                        ui.label(name).on_hover_text(
                            model
                                .source_path
                                .as_ref()
                                .map(|p| p.display().to_string())
                                .unwrap_or_default(),
                        );
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            egui::ComboBox::from_id_salt(("discipline", id.0))
                                .selected_text(RichText::new(discipline.code()).small())
                                .width(56.0)
                                .show_ui(ui, |ui| {
                                    for d in Discipline::ALL {
                                        ui.selectable_value(&mut discipline, d, d.label());
                                    }
                                })
                                .response
                                .on_hover_text(discipline.label());
                            let level = own_level
                                .as_ref()
                                .map(|l| format!(" · {}", crate::checking::level_label(l)))
                                .unwrap_or_default();
                            ui.label(
                                RichText::new(format!(
                                    "rev {revision} · {} · {} elements{level}",
                                    model.schema,
                                    model.elements.len()
                                ))
                                .small()
                                .color(theme::text_muted()),
                            )
                            .on_hover_text(format!(
                                "Revision {revision}, last updated {}",
                                updated.replace('T', " ").trim_end_matches('Z')
                            ));
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button(
                            RichText::new(icon::DOTS_THREE_VERTICAL).color(theme::text_muted()),
                            |ui| {
                                ui.set_min_width(220.0);
                                if ui
                                    .button(format!("{}  Update model…", icon::ARROWS_CLOCKWISE))
                                    .clicked()
                                {
                                    update = Some(id);
                                    ui.close();
                                }
                                ui.menu_button(format!("{}  Information level", icon::STAIRS), |ui| {
                                    ui.set_min_width(200.0);
                                    let follow = format!(
                                        "Same as project ({})",
                                        crate::checking::level_label(&project_level)
                                    );
                                    if ui.selectable_label(own_level.is_none(), follow).clicked() {
                                        level_change = Some((id, None));
                                        ui.close();
                                    }
                                    ui.separator();
                                    for level in &levels {
                                        let on = own_level.as_ref() == Some(level);
                                        if ui
                                            .selectable_label(on, crate::checking::level_label(level))
                                            .clicked()
                                        {
                                            level_change = Some((id, Some(level.clone())));
                                            ui.close();
                                        }
                                    }
                                });
                                ui.separator();
                                if ui
                                    .button(
                                        RichText::new(format!("{}  Remove model", icon::TRASH))
                                            .color(theme::danger()),
                                    )
                                    .clicked()
                                {
                                    remove = Some(id);
                                    ui.close();
                                }
                            },
                        )
                        .response
                        .on_hover_text("More: update, information level, remove");
                        if ui
                            .add(
                                egui::Button::new(RichText::new(icon::X).color(theme::text_muted()))
                                    .frame(false),
                            )
                            .on_hover_text("Remove model")
                            .clicked()
                        {
                            remove = Some(id);
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(icon::ARROWS_CLOCKWISE).color(theme::text_muted()),
                                )
                                .frame(false),
                            )
                            .on_hover_text(
                                "Update model: load a newer version of this IFC file. Issues whose clashes are gone are marked \"Solved by update\".",
                            )
                            .clicked()
                        {
                            update = Some(id);
                        }
                    });
                });
            });
        if discipline != model.discipline {
            discipline_change = Some((id, discipline));
        }
    }
    if let Some((id, visible)) = visibility_change {
        app.set_model_visible(id, visible);
        app.session.dirty = true;
    }
    if let Some((id, discipline)) = discipline_change {
        match app.federation.get_mut(id) {
            Some(model) => {
                model.discipline = discipline;
                app.session.dirty = true;
                app.checking.stale = true;
            }
            None => app
                .errors
                .push("Wait until the check has finished before changing disciplines.".to_owned()),
        }
    }
    if let Some(id) = remove {
        app.remove_model(id);
    }
    if let Some(id) = update {
        app.update_model_dialog(id, &ui.ctx().clone());
    }
    if let Some((id, level)) = level_change {
        app.checking.set_model_level(id, level);
        app.session.dirty = true;
    }
}

/// Collects the elements of a spatial node grouped by class, parts excluded.
fn class_groups<'a>(model: &'a IfcModel, node: &SpatialNode) -> Vec<(&'a str, Vec<&'a Element>)> {
    let mut groups: Vec<(&str, Vec<&Element>)> = Vec::new();
    for id in &node.elements {
        let Some(element) = model.element(*id) else {
            continue;
        };
        match groups.iter_mut().find(|(c, _)| *c == element.ifc_class) {
            Some((_, list)) => list.push(element),
            None => groups.push((element.ifc_class.as_str(), vec![element])),
        }
    }
    groups.sort_by(|a, b| a.0.cmp(b.0));
    for (_, list) in &mut groups {
        list.sort_by(|a, b| a.label().cmp(b.label()));
    }
    groups
}

fn model_tree(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let mut actions = TreeActions::default();
    for (id, model) in app.federation.iter() {
        let color = model_color(app, id);
        let header = RichText::new(format!("{}  {}", icon::CUBE, model.short_name))
            .strong()
            .color(color);
        let shown = egui::CollapsingHeader::new(header)
            .id_salt(("tree_model", id.0))
            .default_open(app.federation.len() == 1)
            .show(ui, |ui| match &model.spatial_root {
                Some(root) => spatial_node(app, ui, id, model, root, &mut actions),
                None => {
                    ui.label(RichText::new("No spatial structure").color(theme::text_muted()));
                }
            });
        shown.header_response.context_menu(|ui| {
            let mut refs = Vec::new();
            let ids: Vec<u32> = model.elements.iter().map(|e| e.express_id).collect();
            collect_refs(id, model, &ids, &mut refs);
            tree_menu(ui, &refs, &mut actions);
        });
    }
    actions.apply(app);
}

#[derive(Default)]
struct TreeActions {
    select: Option<(ElementRef, bool)>,
    zoom: Option<ElementRef>,
    toggle_hidden: Vec<(ElementRef, bool)>,
    /// Show only these elements (right-click > Isolate).
    isolate: Option<Vec<ElementRef>>,
    select_many: Option<Vec<ElementRef>>,
    fit: Option<Vec<ElementRef>>,
}

impl TreeActions {
    fn apply(self, app: &mut OpenCheckApp) {
        if let Some((element, additive)) = self.select {
            app.select(Some(element), additive);
        }
        if let Some(element) = self.zoom {
            app.select(Some(element), false);
            app.fit_element(element);
        }
        if !self.toggle_hidden.is_empty() {
            for (element, hide) in self.toggle_hidden {
                if hide {
                    app.view.hidden.insert(element);
                } else {
                    app.view.hidden.remove(&element);
                }
            }
            app.view.modes_dirty = true;
        }
        if let Some(elements) = self.isolate {
            // Isolating a group also brings back the parts of it that were hidden.
            for element in &elements {
                app.view.hidden.remove(element);
            }
            app.view.isolated = Some(elements.into_iter().collect());
            app.view.modes_dirty = true;
        }
        if let Some(elements) = self.select_many {
            app.select(None, false);
            for element in elements {
                app.select(Some(element), true);
            }
        }
        if let Some(elements) = self.fit {
            let bounds = app.view.bounds_of(&elements);
            if !bounds.is_empty() {
                app.camera.fit(&bounds, app.nav.aspect);
            }
        }
    }
}

/// Right-click menu for a tree row: what to do with all its elements.
fn tree_menu(ui: &mut egui::Ui, refs: &[ElementRef], actions: &mut TreeActions) {
    ui.set_min_width(190.0);
    if ui
        .button(format!(
            "{}  Isolate (hide everything else)",
            icon::SELECTION_FOREGROUND
        ))
        .clicked()
    {
        actions.isolate = Some(refs.to_vec());
        ui.close();
    }
    if ui.button(format!("{}  Hide", icon::EYE_SLASH)).clicked() {
        actions
            .toggle_hidden
            .extend(refs.iter().map(|r| (*r, true)));
        ui.close();
    }
    if ui.button(format!("{}  Show", icon::EYE)).clicked() {
        actions
            .toggle_hidden
            .extend(refs.iter().map(|r| (*r, false)));
        ui.close();
    }
    ui.separator();
    if ui
        .button(format!("{}  Select", icon::CURSOR_CLICK))
        .clicked()
    {
        actions.select_many = Some(refs.to_vec());
        ui.close();
    }
    if ui
        .button(format!("{}  Zoom to", icon::CORNERS_OUT))
        .clicked()
    {
        actions.fit = Some(refs.to_vec());
        ui.close();
    }
}

/// A tree row's name; right-clicking it opens [`tree_menu`].
fn row_label(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add(egui::Label::new(text).sense(egui::Sense::click()))
}

/// Eye button for a group of elements. Returns Some(hide) when clicked.
fn group_eye(ui: &mut egui::Ui, any_visible: bool) -> Option<bool> {
    let glyph = if any_visible {
        icon::EYE
    } else {
        icon::EYE_CLOSED
    };
    let color = if any_visible {
        theme::text_muted()
    } else {
        Color32::from_gray(190)
    };
    ui.add(egui::Button::new(RichText::new(glyph).color(color)).frame(false))
        .on_hover_text(if any_visible { "Hide" } else { "Show" })
        .clicked()
        .then_some(any_visible)
}

fn collect_refs(model_id: ModelId, model: &IfcModel, ids: &[u32], out: &mut Vec<ElementRef>) {
    for id in ids {
        out.push(ElementRef {
            model: model_id,
            express_id: *id,
        });
        if let Some(e) = model.element(*id) {
            collect_refs(model_id, model, &e.parts, out);
        }
    }
}

fn node_refs(model_id: ModelId, model: &IfcModel, node: &SpatialNode) -> Vec<ElementRef> {
    let mut out = Vec::new();
    node.walk(&mut |n, _| collect_refs(model_id, model, &n.elements, &mut out));
    out
}

fn spatial_node(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    model_id: ModelId,
    model: &IfcModel,
    node: &SpatialNode,
    actions: &mut TreeActions,
) {
    // Skip single-child levels without own elements (project -> site -> building)
    // only when they add nothing; otherwise show them like Solibri does.
    let glyph = match node.ifc_class.as_str() {
        "IfcProject" => icon::BUILDINGS,
        "IfcBuildingStorey" => icon::STACK,
        "IfcSite" => icon::HOUSE,
        _ => icon::BUILDINGS,
    };
    let count = node.element_count();
    let id = ui.make_persistent_id(("node", model_id.0, node.express_id));
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        node.ifc_class != "IfcBuildingStorey",
    );
    state
        .show_header(ui, |ui| {
            let refs = node_refs(model_id, model, node);
            let any_visible = refs
                .iter()
                .any(|r| app.view.display_mode(*r) != DisplayMode::Hidden);
            if let Some(hide) = group_eye(ui, any_visible) {
                actions
                    .toggle_hidden
                    .extend(refs.iter().map(|r| (*r, hide)));
            }
            row_label(ui, format!("{glyph}  {}", node.label()))
                .context_menu(|ui| tree_menu(ui, &refs, actions));
            ui.label(
                RichText::new(count.to_string())
                    .small()
                    .color(theme::text_muted()),
            );
        })
        .body(|ui| {
            for child in &node.children {
                spatial_node(app, ui, model_id, model, child, actions);
            }
            for (class, elements) in class_groups(model, node) {
                let group_id = ui.make_persistent_id(("class", model_id.0, node.express_id, class));
                let group = egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    group_id,
                    false,
                );
                group
                    .show_header(ui, |ui| {
                        let mut refs = Vec::new();
                        let ids: Vec<u32> = elements.iter().map(|e| e.express_id).collect();
                        collect_refs(model_id, model, &ids, &mut refs);
                        let any_visible = refs
                            .iter()
                            .any(|r| app.view.display_mode(*r) != DisplayMode::Hidden);
                        if let Some(hide) = group_eye(ui, any_visible) {
                            actions
                                .toggle_hidden
                                .extend(refs.iter().map(|r| (*r, hide)));
                        }
                        row_label(ui, class.trim_start_matches("Ifc"))
                            .context_menu(|ui| tree_menu(ui, &refs, actions));
                        ui.label(
                            RichText::new(elements.len().to_string())
                                .small()
                                .color(theme::text_muted()),
                        );
                    })
                    .body(|ui| {
                        for element in elements {
                            element_row(app, ui, model_id, model, element, actions, 0);
                        }
                    });
            }
        });
}

fn element_row(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    model_id: ModelId,
    model: &IfcModel,
    element: &Element,
    actions: &mut TreeActions,
    depth: usize,
) {
    let r = ElementRef {
        model: model_id,
        express_id: element.express_id,
    };
    let selected = app.view.selection.contains(&r);
    let mode = app.view.display_mode(r);
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        if let Some(hide) = group_eye(ui, mode != DisplayMode::Hidden) {
            actions.toggle_hidden.push((r, hide));
        }
        let mut label = RichText::new(element.label());
        if !element.has_geometry() && element.parts.is_empty() {
            label = label.color(theme::text_muted());
        } else if selected {
            label = label.family(theme::bold());
        }
        let response = ui.add(egui::Button::selectable(selected, label).frame_when_inactive(false));
        response.context_menu(|ui| {
            let mut refs = Vec::new();
            collect_refs(model_id, model, &[element.express_id], &mut refs);
            tree_menu(ui, &refs, actions);
        });
        if response.double_clicked() {
            actions.zoom = Some(r);
        } else if response.clicked() {
            let additive = ui.input(|i| i.modifiers.command || i.modifiers.shift);
            actions.select = Some((r, additive));
        }
    });
    for part in element.parts.iter().filter_map(|p| model.element(*p)) {
        element_row(app, ui, model_id, model, part, actions, depth + 1);
    }
}

fn search_results(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let needle = app.tree_filter.trim().to_lowercase();
    let mut actions = TreeActions::default();
    let mut shown = 0;
    'models: for (id, model) in app.federation.iter() {
        for element in &model.elements {
            let hit = element.name.to_lowercase().contains(&needle)
                || element.ifc_class.to_lowercase().contains(&needle)
                || element.global_id.to_lowercase() == needle
                || element
                    .object_type
                    .as_deref()
                    .is_some_and(|t| t.to_lowercase().contains(&needle));
            if !hit {
                continue;
            }
            if shown == MAX_SEARCH_RESULTS {
                ui.label(
                    RichText::new("More results not shown; refine the search.")
                        .color(theme::text_muted()),
                );
                break 'models;
            }
            shown += 1;
            let r = ElementRef {
                model: id,
                express_id: element.express_id,
            };
            let selected = app.view.selection.contains(&r);
            let text = RichText::new(format!(
                "{}  ·  {}",
                element.label(),
                element.ifc_class.trim_start_matches("Ifc")
            ));
            let response =
                ui.add(egui::Button::selectable(selected, text).frame_when_inactive(false));
            if response.double_clicked() {
                actions.zoom = Some(r);
            } else if response.clicked() {
                actions.select = Some((r, ui.input(|i| i.modifiers.command || i.modifiers.shift)));
            }
        }
    }
    if shown == 0 {
        ui.label(RichText::new("No matching elements.").color(theme::text_muted()));
    }
    actions.apply(app);
}

pub fn properties_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::right("properties_panel")
        .resizable(true)
        .default_size(360.0)
        .min_size(260.0)
        .frame(
            egui::Frame::new()
                .fill(theme::panel())
                .inner_margin(egui::Margin::same(12))
                .stroke(egui::Stroke::new(1.0, theme::border())),
        )
        .show(ui, |ui| {
            // Long values are cut (full text on hover), not allowed to widen the panel.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            if app.view.selection.len() > 1 {
                multi_selection(app, ui);
                return;
            }
            let Some(&r) = app.view.selection.first() else {
                empty_properties(ui);
                return;
            };
            let Some(model) = app.federation.get(r.model) else {
                return;
            };
            let Some(element) = model.element(r.express_id) else {
                return;
            };
            let color = model_color(app, r.model);

            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(element.label()).heading().strong());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&element.ifc_class).color(theme::text_muted()));
                        ui.label(
                            RichText::new(format!("● {}", model.short_name))
                                .small()
                                .color(color),
                        );
                    });
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui
                        .add(egui::Button::new(icon::X).frame(false))
                        .on_hover_text("Clear selection (Esc)")
                        .clicked()
                    {
                        app.view.selection.clear();
                        app.view.modes_dirty = true;
                    }
                });
            });
            ui.add_space(6.0);
            ui.add(
                egui::TextEdit::singleline(&mut app.property_filter)
                    .hint_text(format!("{}  Filter properties", icon::FUNNEL))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);

            let filter = app.property_filter.trim().to_lowercase();
            let matches = |k: &str, v: &str| {
                filter.is_empty()
                    || k.to_lowercase().contains(&filter)
                    || v.to_lowercase().contains(&filter)
            };
            let storey = element_storey(model, element);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let mut rows: Vec<(&str, String)> = vec![
                        ("Name", element.name.clone()),
                        ("Class", element.ifc_class.clone()),
                        ("GlobalId", element.global_id.clone()),
                    ];
                    for (k, v) in [
                        ("PredefinedType", &element.predefined_type),
                        ("ObjectType", &element.object_type),
                        ("Description", &element.description),
                        ("Tag", &element.tag),
                    ] {
                        if let Some(v) = v {
                            rows.push((k, v.clone()));
                        }
                    }
                    if let Some((_, type_name)) = &element.type_object {
                        rows.push(("Type", type_name.clone()));
                    }
                    if let Some(storey) = storey {
                        rows.push(("Storey", storey));
                    }
                    rows.push(("Model", model.short_name.clone()));
                    rows.push(("Discipline", model.discipline.label().to_owned()));
                    property_group(
                        ui,
                        "Element",
                        ("identity", r.express_id),
                        true,
                        &rows,
                        &matches,
                    );

                    for pset in model.property_sets_of(element) {
                        let rows: Vec<(&str, String)> = pset
                            .properties
                            .iter()
                            .map(|p| (p.name.as_str(), format_value(model, p)))
                            .collect();
                        let title = match pset.kind {
                            PropertySetKind::Quantities => {
                                format!("{}  {}", icon::RULER, pset.name)
                            }
                            PropertySetKind::Properties => pset.name.clone(),
                        };
                        property_group(
                            ui,
                            &title,
                            ("pset", pset.express_id),
                            true,
                            &rows,
                            &matches,
                        );
                    }
                });
        });
}

/// Quantities are stored in model units; show lengths in metres-friendly form.
fn format_value(model: &IfcModel, p: &opencheck_core::Property) -> String {
    let value = p.value.to_string();
    let Some(kind) = p.value_type.as_deref() else {
        return value;
    };
    let scale = model.length_unit_scale;
    match (kind, p.value.as_f64()) {
        ("IfcQuantityLength" | "IfcLengthMeasure" | "IfcPositiveLengthMeasure", Some(v)) => {
            format!("{} m", trim_number(v * scale))
        }
        ("IfcQuantityArea" | "IfcAreaMeasure", Some(v)) => format!("{} m²", trim_number(v)),
        ("IfcQuantityVolume" | "IfcVolumeMeasure", Some(v)) => format!("{} m³", trim_number(v)),
        ("IfcQuantityWeight" | "IfcMassMeasure", Some(v)) => format!("{} kg", trim_number(v)),
        _ => value,
    }
}

fn trim_number(v: f64) -> String {
    opencheck_core::PropertyValue::Real((v * 1000.0).round() / 1000.0).to_string()
}

fn property_group(
    ui: &mut egui::Ui,
    title: &str,
    id: (&str, u32),
    default_open: bool,
    rows: &[(&str, String)],
    matches: &dyn Fn(&str, &str) -> bool,
) {
    let visible: Vec<_> = rows.iter().filter(|(k, v)| matches(k, v)).collect();
    if visible.is_empty() {
        return;
    }
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(id)
        .default_open(default_open)
        .show(ui, |ui| {
            egui::Grid::new(id)
                .num_columns(2)
                .striped(true)
                .spacing([12.0, 4.0])
                .min_col_width(110.0)
                .show(ui, |ui| {
                    for (k, v) in visible {
                        ui.label(RichText::new(*k).color(theme::text_muted()));
                        let value = ui.add(
                            egui::Label::new(v.as_str())
                                .wrap()
                                .sense(egui::Sense::click()),
                        );
                        if value.on_hover_text("Click to copy").clicked() {
                            ui.ctx().copy_text(v.clone());
                        }
                        ui.end_row();
                    }
                });
        });
}

fn element_storey(model: &IfcModel, element: &Element) -> Option<String> {
    let mut current = element;
    for _ in 0..16 {
        if let Some(container) = current.container {
            let mut name = None;
            if let Some(root) = &model.spatial_root {
                root.walk(&mut |n, _| {
                    if n.express_id == container {
                        name = Some(n.label().to_owned());
                    }
                });
            }
            return name;
        }
        current = model.element(current.parent?)?;
    }
    None
}

fn empty_properties(ui: &mut egui::Ui) {
    section_title(ui, "Properties");
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(icon::CURSOR_CLICK)
                .size(36.0)
                .color(theme::border()),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new("Select an element to see its properties").color(theme::text_muted()),
        );
        ui.add_space(18.0);
        for (keys, action) in [
            ("Left drag", "Orbit around the point under the cursor"),
            ("Right drag", "Pan"),
            ("Scroll", "Zoom towards the cursor"),
            ("Double click", "Zoom to element"),
            ("W A S D  Q E", "Walk (Shift = faster)"),
            ("F / Home", "Zoom to selection / everything"),
            ("H / I / X", "Hide / isolate / X-ray"),
            ("C", "Section just above the selection"),
            ("M", "Measure with the laser"),
        ] {
            ui.label(RichText::new(keys).small().strong());
            ui.label(RichText::new(action).small().color(theme::text_muted()));
            ui.add_space(4.0);
        }
    });
}

fn multi_selection(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(
        RichText::new(format!("{} elements selected", app.view.selection.len()))
            .heading()
            .strong(),
    );
    ui.add_space(8.0);
    let mut select = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for r in &app.view.selection {
                let Some(model) = app.federation.get(r.model) else {
                    continue;
                };
                let Some(element) = model.element(r.express_id) else {
                    continue;
                };
                let text = format!(
                    "{}  ·  {}",
                    element.label(),
                    element.ifc_class.trim_start_matches("Ifc")
                );
                if ui
                    .add(egui::Button::new(text).frame_when_inactive(false))
                    .clicked()
                {
                    select = Some(*r);
                }
            }
        });
    if let Some(r) = select {
        app.select(Some(r), false);
    }
}

/// "Edit view" and "Reset view" at the bottom of the model panel: limit what
/// is shown and checked to a working area.
fn area_controls(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::bottom("area_controls")
        .frame(egui::Frame::new().inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 8,
            bottom: 0,
        }))
        .show(ui, |ui| {
            ui.separator();
            if let Some(_area) = app.checking.area {
                let (inside, total) = app.area_counts();
                ui.label(
                    RichText::new(format!(
                        "{}  Working area: {inside} of {total} elements are shown and checked",
                        icon::CROP
                    ))
                    .small()
                    .color(theme::accent()),
                );
            }
            ui.horizontal(|ui| {
                let editing = app.is_editing_area();
                if ui
                    .add_enabled(
                        !editing,
                        egui::Button::new(format!("{}  Edit view", icon::CROP)),
                    )
                    .on_hover_text(
                        "Draw an area in a top view; only what touches it is shown here and checked in Checking",
                    )
                    .clicked()
                {
                    app.enter_area_edit();
                }
                if app.checking.area.is_some()
                    && ui
                        .add_enabled(
                            !editing,
                            egui::Button::new(format!("{}  Reset view", icon::ARROW_COUNTER_CLOCKWISE)),
                        )
                        .on_hover_text("Show and check everything again")
                        .clicked()
                {
                    app.reset_area();
                }
            });
        });
}
