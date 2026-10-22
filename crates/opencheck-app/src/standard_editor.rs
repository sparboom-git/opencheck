//! The window that adapts where the information standard looks: which
//! properties, attributes and other sources an item is found in, for all
//! building parts or for one part. The changes are kept in the project.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{
    InformationStandard, ModelFacts, Source, StandardOverrides, builtin_standard, exact_name,
    exact_pattern, validate_sources,
};
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::checking::INFORMATION_STANDARD;
use crate::scene::ElementRef;
use crate::theme;

/// The kinds of source, as offered in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Property,
    Attribute,
    TypeName,
    Classification,
    Material,
    System,
    Storey,
    Geometry,
}

impl Kind {
    const ALL: [Kind; 8] = [
        Kind::Property,
        Kind::Attribute,
        Kind::TypeName,
        Kind::Classification,
        Kind::Material,
        Kind::System,
        Kind::Storey,
        Kind::Geometry,
    ];

    fn label(self) -> &'static str {
        match self {
            Kind::Property => "Property",
            Kind::Attribute => "IFC attribute",
            Kind::TypeName => "Type name",
            Kind::Classification => "Classification",
            Kind::Material => "Material",
            Kind::System => "System",
            Kind::Storey => "Storey",
            Kind::Geometry => "Geometry",
        }
    }
}

/// One source as the user edits it: names are plain text unless marked as
/// patterns (regular expressions).
#[derive(Debug, Clone, PartialEq)]
struct Draft {
    kind: Kind,
    name: String,
    name_pattern: bool,
    set: String,
    set_pattern: bool,
}

impl Draft {
    fn from_source(source: &Source) -> Self {
        let split = |pattern: &Option<String>| match pattern {
            None => (String::new(), false),
            Some(p) => match exact_name(p) {
                Some(name) => (name, false),
                None => (p.clone(), true),
            },
        };
        let (kind, (name, name_pattern)) = if source.property.is_some() {
            (Kind::Property, split(&source.property))
        } else if source.attribute.is_some() {
            (Kind::Attribute, split(&source.attribute))
        } else {
            let kind = if source.type_name {
                Kind::TypeName
            } else if source.classification {
                Kind::Classification
            } else if source.material {
                Kind::Material
            } else if source.system {
                Kind::System
            } else if source.storey {
                Kind::Storey
            } else {
                Kind::Geometry
            };
            (kind, (String::new(), false))
        };
        let (set, set_pattern) = split(&source.pset);
        Self {
            kind,
            name,
            name_pattern,
            set,
            set_pattern,
        }
    }

    fn to_source(&self) -> Source {
        let pattern = |text: &str, is_pattern: bool| {
            if is_pattern {
                text.trim().to_owned()
            } else {
                exact_pattern(text)
            }
        };
        let mut source = Source::default();
        match self.kind {
            Kind::Property => {
                source.property = Some(pattern(&self.name, self.name_pattern));
                if !self.set.trim().is_empty() {
                    source.pset = Some(pattern(&self.set, self.set_pattern));
                }
            }
            Kind::Attribute => source.attribute = Some(pattern(&self.name, self.name_pattern)),
            Kind::TypeName => source.type_name = true,
            Kind::Classification => source.classification = true,
            Kind::Material => source.material = true,
            Kind::System => source.system = true,
            Kind::Storey => source.storey = true,
            Kind::Geometry => source.geometry = true,
        }
        source
    }
}

/// A property or attribute the models have, with how many elements have it.
struct Candidate {
    set: Option<String>,
    name: String,
    count: usize,
}

pub struct EditorState {
    /// Selected item id.
    item: String,
    /// Selected part id; None edits the item for all parts.
    part: Option<String>,
    item_filter: String,
    /// The sources being edited, for (item, part).
    drafts: Vec<Draft>,
    /// The part follows the item (no sources of its own).
    inherited: bool,
    error: Option<String>,
    candidates: Option<Vec<Candidate>>,
    candidate_filter: String,
    /// Bring the window in front of the others on the next frame.
    raise: bool,
    side: Side,
    /// Elements the item is checked on, once listed.
    rows: Option<Vec<ElementRow>>,
    /// Per model: name, elements that have the item, elements checked.
    per_model: Vec<(String, usize, usize)>,
    row_filter: String,
    show: Show,
    selected: Option<ElementRef>,
    /// Folded down to a small bar at the bottom, to look at the model.
    minimized: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Elements,
    Properties,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Show {
    All,
    Missing,
    Found,
}

/// An element the item is checked on, and where it was found.
struct ElementRow {
    element: ElementRef,
    model_name: String,
    label: String,
    class: String,
    part: String,
    found: Option<String>,
}

impl EditorState {
    /// Opens on an item, and on a part when given.
    pub fn new(item: Option<&str>, part: Option<&str>) -> Self {
        Self {
            item: item.unwrap_or_default().to_owned(),
            part: part.map(str::to_owned),
            item_filter: String::new(),
            drafts: Vec::new(),
            inherited: false,
            error: None,
            candidates: None,
            candidate_filter: String::new(),
            raise: true,
            side: Side::Elements,
            rows: None,
            per_model: Vec::new(),
            row_filter: String::new(),
            show: Show::All,
            selected: None,
            minimized: false,
        }
    }
}

/// Opens the editor, on an item and part when given.
pub fn open(app: &mut OpenCheckApp, item: Option<&str>, part: Option<&str>) {
    let mut state = EditorState::new(item, part);
    if let Some(standard) = app.checking.information_standard() {
        if state.item.is_empty() {
            state.item = standard
                .items
                .first()
                .map(|i| i.id.clone())
                .unwrap_or_default();
        }
        load_drafts(&mut state, standard);
    }
    app.checking.editor = Some(state);
}

/// The sources shown for the selection: the part's own, else the item's.
fn load_drafts(state: &mut EditorState, standard: &InformationStandard) {
    state.error = None;
    state.candidates = None;
    state.rows = None;
    state.per_model.clear();
    let Some(item) = standard.item(&state.item) else {
        state.drafts.clear();
        return;
    };
    let part = state
        .part
        .as_ref()
        .and_then(|id| standard.parts.iter().find(|p| &p.id == id));
    let own = part.and_then(|p| p.sources.get(&item.id));
    state.inherited = part.is_some() && own.is_none();
    let sources = own.unwrap_or(&item.any);
    state.drafts = sources.iter().map(Draft::from_source).collect();
}

/// Asks for the editor from places that have no access to the app state;
/// [`window`] opens it on the next frame.
pub fn request_open(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("standard_editor_request"), true));
}

/// Draws the window while it is open.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let requested = ctx.data_mut(|d| {
        d.remove_temp::<bool>(egui::Id::new("standard_editor_request"))
            .unwrap_or(false)
    });
    if requested {
        open(app, None, None);
    }
    let Some(mut state) = app.checking.editor.take() else {
        return;
    };
    let Some(base) = builtin_standard(INFORMATION_STANDARD) else {
        return;
    };
    if state.minimized {
        if minimized_bar(ctx, &mut state, base) {
            app.checking.editor = Some(state);
        }
        return;
    }
    let mut open = true;
    let shown = egui::Window::new(
        RichText::new(format!(
            "{}  Information requirements: where to look",
            icon::SLIDERS_HORIZONTAL
        ))
        .strong(),
    )
    .id(egui::Id::new("standard_editor"))
    .open(&mut open)
    .default_size(egui::vec2(1240.0, 720.0).min(ctx.content_rect().size() - egui::vec2(40.0, 80.0)))
    .min_size([560.0, 300.0])
    .resizable(true)
    .collapsible(false)
    .show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui
                .button(format!("{}  Minimise", icon::ARROW_LINE_DOWN))
                .on_hover_text("Fold the window down to a bar at the bottom, to look at the model")
                .clicked()
            {
                state.minimized = true;
            }
            ui.add(
                egui::Label::new(
                    RichText::new(
                        "Choose where each item is read from. An item counts as present when any \
                         of its sources has a value. Changes are saved with the project; the \
                         built-in standard stays as it is.",
                    )
                    .small()
                    .color(theme::text_muted()),
                )
                .wrap(),
            );
        });
        ui.add_space(4.0);
        // The columns share the width, so the window can be made small.
        let width = ui.available_width();
        let left = (width * 0.2).clamp(150.0, 250.0);
        let right = (width * 0.32).clamp(220.0, 390.0);
        let middle = (width - left - right - 56.0).max(200.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(left);
                item_list(app, ui, &mut state, base);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(middle);
                egui::ScrollArea::both()
                    .id_salt("standard_editor_sources")
                    .auto_shrink([false, false])
                    .show(ui, |ui| source_editor(app, ui, &mut state, base));
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width(right);
                ui.horizontal(|ui| {
                    ui.selectable_value(
                        &mut state.side,
                        Side::Elements,
                        format!("{}  Elements", icon::CUBE),
                    )
                    .on_hover_text("The elements this item is checked on, and their properties");
                    ui.selectable_value(
                        &mut state.side,
                        Side::Properties,
                        format!("{}  All properties", icon::LIST),
                    )
                    .on_hover_text("Every property the elements carry, counted");
                });
                ui.separator();
                match state.side {
                    Side::Elements => elements(app, ui, &mut state),
                    Side::Properties => candidates(app, ui, &mut state),
                }
            });
        });
    });
    if let Some(shown) = shown
        && std::mem::take(&mut state.raise)
    {
        ctx.move_to_top(shown.response.layer_id);
    }
    if open {
        app.checking.editor = Some(state);
    }
}

/// The folded-down window: a bar at the bottom of the screen that opens it
/// again. Returns false when it was closed.
fn minimized_bar(ctx: &egui::Context, state: &mut EditorState, base: &InformationStandard) -> bool {
    let mut keep = true;
    let item = base
        .item(&state.item)
        .map_or_else(String::new, |i| format!(": {}", i.name));
    egui::Area::new(egui::Id::new("standard_editor_minimized"))
        // In the middle of the status bar, where it covers nothing.
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -3.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new().show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{}  Where to look{item}", icon::ARROW_LINE_UP))
                        .on_hover_text("Open the window again")
                        .clicked()
                    {
                        state.minimized = false;
                        state.raise = true;
                    }
                    if ui.small_button(icon::X).on_hover_text("Close").clicked() {
                        keep = false;
                    }
                });
            });
        });
    keep
}

fn item_list(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    state: &mut EditorState,
    base: &InformationStandard,
) {
    let overrides = app.checking.overrides();
    ui.add(
        egui::TextEdit::singleline(&mut state.item_filter)
            .hint_text(format!("{}  Find item", icon::MAGNIFYING_GLASS))
            .desired_width(f32::INFINITY),
    );
    let filter = state.item_filter.to_lowercase();
    let mut select = None;
    egui::ScrollArea::vertical()
        .id_salt("standard_items")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for item in &base.items {
                if !filter.is_empty()
                    && !item.name.to_lowercase().contains(&filter)
                    && !item.id.contains(&filter)
                {
                    continue;
                }
                let changed = overrides.items.contains_key(&item.id)
                    || overrides.parts.values().any(|p| p.contains_key(&item.id));
                let text = if changed {
                    RichText::new(format!("{}  {}", item.name, icon::PENCIL_SIMPLE)).strong()
                } else {
                    RichText::new(&item.name)
                };
                let response = ui
                    .selectable_label(state.item == item.id, text)
                    .on_hover_text(if item.description.is_empty() {
                        item.id.clone()
                    } else {
                        item.description.clone()
                    });
                if response.clicked() {
                    select = Some(item.id.clone());
                }
            }
        });
    if let Some(id) = select {
        state.item = id;
        state.part = None;
        if let Some(standard) = app.checking.information_standard() {
            load_drafts(state, standard);
        }
    }
}

fn source_editor(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    state: &mut EditorState,
    base: &InformationStandard,
) {
    let Some(item) = base.item(&state.item) else {
        ui.label("Choose an item on the left.");
        return;
    };
    ui.label(RichText::new(&item.name).heading());
    if !item.description.is_empty() {
        ui.label(
            RichText::new(&item.description)
                .small()
                .color(theme::text_muted()),
        );
    }
    ui.add_space(6.0);

    // Which parts need the item at which level; a row selects the part.
    let parts: Vec<&opencheck_core::check::BuildingPart> = base
        .parts
        .iter()
        .filter(|p| p.levels.values().any(|items| items.contains(&item.id)))
        .collect();
    let overrides = app.checking.overrides();
    let project_level = app.checking.project_level();
    let mut new_part = None;
    ui.label(RichText::new("Required at").strong())
        .on_hover_text(
            "Which building parts need this item at which LOD level. Pick a row to give that \
         part its own sources; \"All building parts\" edits the item's sources.",
        );
    egui::ScrollArea::vertical()
        .id_salt("standard_levels")
        .max_height(170.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("standard_level_grid")
                .striped(true)
                .spacing([14.0, 3.0])
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Building part")
                            .small()
                            .color(theme::text_muted()),
                    );
                    for level in &base.standard.levels {
                        let text = RichText::new(crate::checking::level_label(level)).small();
                        let text = if *level == project_level {
                            text.family(theme::bold())
                        } else {
                            text.color(theme::text_muted())
                        };
                        ui.label(text).on_hover_text(if *level == project_level {
                            "The project's level"
                        } else {
                            ""
                        });
                    }
                    ui.end_row();
                    if ui
                        .selectable_label(
                            state.part.is_none(),
                            RichText::new("All building parts").small(),
                        )
                        .clicked()
                    {
                        new_part = Some(None);
                    }
                    for _ in &base.standard.levels {
                        ui.label("");
                    }
                    ui.end_row();
                    for part in &parts {
                        let own = overrides
                            .parts
                            .get(&part.id)
                            .is_some_and(|p| p.contains_key(&item.id));
                        let name = if own {
                            format!("{}  {}", part.name, icon::PENCIL_SIMPLE)
                        } else {
                            part.name.clone()
                        };
                        let response = ui
                            .selectable_label(
                                state.part.as_deref() == Some(part.id.as_str()),
                                RichText::new(name).small(),
                            )
                            .on_hover_text(if own {
                                "This part has its own sources for the item"
                            } else {
                                "This part uses the item's sources"
                            });
                        if response.clicked() {
                            new_part = Some(Some(part.id.clone()));
                        }
                        for level in &base.standard.levels {
                            let required = part
                                .levels
                                .get(level)
                                .is_some_and(|items| items.contains(&item.id));
                            // A filled mark, painted: the font has no dot glyph.
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                            if required {
                                let color = if *level == project_level {
                                    theme::accent()
                                } else {
                                    theme::text_muted()
                                };
                                ui.painter().rect_filled(rect.shrink(1.5), 2.0, color);
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    ui.add_space(6.0);
    if let Some(part) = new_part {
        state.part = part;
        if let Some(standard) = app.checking.information_standard() {
            load_drafts(state, standard);
        }
    }

    found_in_last_check(app, ui, state, base);
    ui.add_space(6.0);

    if state.inherited {
        ui.label(
            RichText::new(
                "This part uses the item's sources (below). Change them to give this part its own.",
            )
            .small()
            .color(theme::text_muted()),
        );
    }
    let mut remove = None;
    egui::ScrollArea::vertical()
        .id_salt("standard_sources")
        .max_height((ui.available_height() - 80.0).max(120.0))
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, draft) in state.drafts.iter_mut().enumerate() {
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, theme::border()))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            egui::ComboBox::from_id_salt(("source_kind", i))
                                .selected_text(draft.kind.label())
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    for kind in Kind::ALL {
                                        ui.selectable_value(&mut draft.kind, kind, kind.label());
                                    }
                                });
                            if matches!(draft.kind, Kind::Property | Kind::Attribute) {
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.name)
                                        .hint_text(if draft.kind == Kind::Property {
                                            "Property name, e.g. Width"
                                        } else {
                                            "Attribute, e.g. ObjectType"
                                        })
                                        .desired_width(200.0),
                                );
                                ui.checkbox(&mut draft.name_pattern, "Pattern")
                                    .on_hover_text(
                                        "Off: the exact name (case and spaces do not matter). \
                                         On: a regular expression, e.g. ^(Width|Bredde)$",
                                    );
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .button(icon::TRASH)
                                        .on_hover_text("Remove this source")
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                },
                            );
                        });
                        if draft.kind == Kind::Property {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("in property set").small());
                                ui.add(
                                    egui::TextEdit::singleline(&mut draft.set)
                                        .hint_text("any set")
                                        .desired_width(200.0),
                                );
                                ui.checkbox(&mut draft.set_pattern, "Pattern");
                            });
                        }
                    });
            }
        });
    if let Some(i) = remove {
        state.drafts.remove(i);
    }
    if ui.button(format!("{}  Add source", icon::PLUS)).clicked() {
        state.drafts.push(Draft {
            kind: Kind::Property,
            name: String::new(),
            name_pattern: false,
            set: String::new(),
            set_pattern: false,
        });
    }
    if let Some(error) = &state.error {
        ui.label(RichText::new(error).color(theme::danger()));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new(format!("{}  Apply", icon::CHECK)).strong())
            .on_hover_text("Use these sources from the next check on")
            .clicked()
        {
            apply(app, state, base);
        }
        if unapplied(app, state) {
            ui.label(
                RichText::new("Not applied yet")
                    .small()
                    .color(theme::accent()),
            );
        }
        let changed = match &state.part {
            Some(p) => overrides
                .parts
                .get(p)
                .is_some_and(|p| p.contains_key(&state.item)),
            None => overrides.items.contains_key(&state.item),
        };
        let reset = ui
            .add_enabled(
                changed,
                egui::Button::new(format!(
                    "{}  Back to standard",
                    icon::ARROW_COUNTER_CLOCKWISE
                )),
            )
            .on_hover_text(if state.part.is_some() {
                "This part follows the item again"
            } else {
                "The item uses the built-in sources again"
            });
        if reset.clicked() {
            let mut overrides = app.checking.overrides();
            match &state.part {
                Some(p) => {
                    if let Some(items) = overrides.parts.get_mut(p) {
                        items.remove(&state.item);
                        if items.is_empty() {
                            overrides.parts.remove(p);
                        }
                    }
                }
                None => {
                    overrides.items.remove(&state.item);
                }
            }
            store(app, overrides);
            if let Some(standard) = app.checking.information_standard() {
                load_drafts(state, standard);
            }
        }
    });
}

/// Where the last check found the item, so a wrong parameter stands out.
fn found_in_last_check(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    state: &EditorState,
    base: &InformationStandard,
) {
    let Some(report) = &app.checking.report else {
        return;
    };
    let mut found: FxHashMap<&str, usize> = FxHashMap::default();
    let (mut required, mut present) = (0, 0);
    for summary in report
        .rulesets
        .iter()
        .flat_map(|r| &r.rules)
        .filter(|r| r.rule.standard == base.standard.id)
        .filter_map(|r| r.information.as_ref())
    {
        for part in &summary.parts {
            if state.part.as_ref().is_some_and(|p| *p != part.part) {
                continue;
            }
            for row in part.items.iter().filter(|r| r.item == state.item) {
                required += row.required;
                present += row.present;
                for (place, n) in &row.found_in {
                    *found.entry(place.as_str()).or_default() += n;
                }
            }
        }
    }
    if required == 0 {
        return;
    }
    let mut list: Vec<(&str, usize)> = found.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    egui::CollapsingHeader::new(
        RichText::new(format!(
            "Last check: found on {present} of {required} elements"
        ))
        .small(),
    )
    .id_salt("standard_found")
    .default_open(true)
    .show(ui, |ui| {
        for (place, n) in list.iter().take(12) {
            ui.label(
                RichText::new(format!("{n:>6}  {place}"))
                    .small()
                    .monospace(),
            );
        }
        if required > present {
            ui.label(
                RichText::new(format!("{:>6}  missing", required - present))
                    .small()
                    .monospace()
                    .color(theme::danger()),
            );
        }
    });
}

fn apply(app: &mut OpenCheckApp, state: &mut EditorState, base: &InformationStandard) {
    if state
        .drafts
        .iter()
        .any(|d| matches!(d.kind, Kind::Property | Kind::Attribute) && d.name.trim().is_empty())
    {
        state.error = Some("Every property and attribute source needs a name.".to_owned());
        return;
    }
    let sources: Vec<Source> = state.drafts.iter().map(Draft::to_source).collect();
    if let Err(e) = validate_sources("This item", &sources) {
        state.error = Some(e.to_string());
        return;
    }
    let mut overrides = app.checking.overrides();
    let item = state.item.clone();
    match &state.part {
        Some(part) => {
            overrides
                .parts
                .entry(part.clone())
                .or_default()
                .insert(item, sources);
        }
        None => {
            // Back to the built-in sources: no change to keep.
            if base.item(&item).is_some_and(|i| i.any == sources) {
                overrides.items.remove(&item);
            } else {
                overrides.items.insert(item, sources);
            }
        }
    }
    store(app, overrides);
    state.error = None;
    state.inherited = false;
    // Show the effect on the listed elements right away.
    if state.rows.is_some() {
        list_elements(app, state);
    }
}

/// The edited sources differ from those in use.
fn unapplied(app: &OpenCheckApp, state: &EditorState) -> bool {
    let Some(standard) = app.checking.information_standard() else {
        return false;
    };
    let Some(item) = standard.item(&state.item) else {
        return false;
    };
    let current = match state
        .part
        .as_ref()
        .and_then(|id| standard.parts.iter().find(|p| &p.id == id))
    {
        Some(part) => standard.sources_for(part, item),
        None => &item.any,
    };
    let drafts: Vec<Source> = state.drafts.iter().map(Draft::to_source).collect();
    drafts.as_slice() != current
}

fn store(app: &mut OpenCheckApp, overrides: StandardOverrides) {
    let messages = app.checking.set_overrides(overrides);
    app.errors.extend(messages);
    app.session.dirty = true;
}

/// Properties and attributes with values on the elements of the selected
/// part (or of all parts that need the item), most common first.
fn scan(app: &OpenCheckApp, state: &EditorState) -> Vec<Candidate> {
    let Some(standard) = app.checking.information_standard() else {
        return Vec::new();
    };
    let parts: Vec<&str> = match &state.part {
        Some(p) => vec![p.as_str()],
        None => standard
            .parts
            .iter()
            .filter(|p| p.levels.values().any(|items| items.contains(&state.item)))
            .map(|p| p.id.as_str())
            .collect(),
    };
    let mut counts: FxHashMap<(Option<String>, String), usize> = FxHashMap::default();
    for id in app.federation.ids() {
        let Some(model) = app.federation.get(id) else {
            continue;
        };
        for element in &model.elements {
            if element.parent.is_some() {
                continue;
            }
            let Some(part) = standard.part_of(model, element) else {
                continue;
            };
            if !parts.contains(&part.id.as_str()) {
                continue;
            }
            let mut seen = rustc_hash::FxHashSet::default();
            for pset in model.property_sets_of(element) {
                for p in &pset.properties {
                    if p.value.to_string().trim().is_empty() {
                        continue;
                    }
                    let key = (Some(pset.name.clone()), p.name.clone());
                    if seen.insert(key.clone()) {
                        *counts.entry(key).or_default() += 1;
                    }
                }
            }
            for (name, value) in &element.attributes {
                if value.to_string().trim().is_empty() {
                    continue;
                }
                let key = (None, name.clone());
                if seen.insert(key.clone()) {
                    *counts.entry(key).or_default() += 1;
                }
            }
        }
    }
    let mut list: Vec<Candidate> = counts
        .into_iter()
        .map(|((set, name), count)| Candidate { set, name, count })
        .collect();
    list.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.name.cmp(&b.name)));
    list
}

/// Lists the elements the selected item is checked on, with where each has
/// it, and counts per model.
fn list_elements(app: &OpenCheckApp, state: &mut EditorState) {
    let Some(standard) = app.checking.information_standard() else {
        return;
    };
    let mut rows = Vec::new();
    let mut per_model = Vec::new();
    for id in app.federation.ids() {
        let Some(model) = app.federation.get(id) else {
            continue;
        };
        let facts = ModelFacts::new(model);
        let (mut found, mut total) = (0, 0);
        for element in &model.elements {
            let Some((part, place)) = standard.item_on(model, &facts, element, &state.item) else {
                continue;
            };
            let wanted = match &state.part {
                Some(p) => *p == part.id,
                None => part
                    .levels
                    .values()
                    .any(|items| items.contains(&state.item)),
            };
            if !wanted {
                continue;
            }
            total += 1;
            found += usize::from(place.is_some());
            rows.push(ElementRow {
                element: ElementRef {
                    model: id,
                    express_id: element.express_id,
                },
                model_name: model.short_name.clone(),
                label: element.label().to_owned(),
                class: element.ifc_class.clone(),
                part: part.name.clone(),
                found: place,
            });
        }
        if total > 0 {
            per_model.push((model.short_name.clone(), found, total));
        }
    }
    // Missing first, then by model and name.
    rows.sort_by(|a, b| {
        a.found
            .is_some()
            .cmp(&b.found.is_some())
            .then_with(|| a.model_name.cmp(&b.model_name))
            .then_with(|| a.label.cmp(&b.label))
    });
    state.rows = Some(rows);
    state.per_model = per_model;
}

fn elements(app: &mut OpenCheckApp, ui: &mut egui::Ui, state: &mut EditorState) {
    if app.federation.ids().is_empty() {
        ui.label(
            RichText::new("No models loaded.")
                .small()
                .color(theme::text_muted()),
        );
        return;
    }
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  List elements", icon::LIST_BULLETS))
            .on_hover_text(
                "The elements of the selected building part(s) and where each has the item",
            )
            .clicked()
        {
            list_elements(app, state);
        }
        if let Some(&first) = app.view.selection.first()
            && ui
                .button(format!("{}  Selected in 3D", icon::CURSOR_CLICK))
                .on_hover_text("Show the properties of the element selected in the 3D view")
                .clicked()
        {
            state.selected = Some(first);
        }
    });
    if state.rows.is_some() {
        for (model, found, total) in &state.per_model {
            let color = if found == total {
                crate::checking_ui::accepted()
            } else {
                theme::danger()
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(model).small());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("{found} / {total} have it"))
                            .small()
                            .color(color),
                    );
                });
            });
        }
        ui.horizontal(|ui| {
            for (show, label) in [
                (Show::All, "All"),
                (Show::Missing, "Missing"),
                (Show::Found, "Has it"),
            ] {
                ui.selectable_value(&mut state.show, show, RichText::new(label).small());
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.row_filter)
                    .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
                    .desired_width(f32::INFINITY),
            );
        });
        let filter = state.row_filter.to_lowercase();
        let visible: Vec<usize> = state
            .rows
            .iter()
            .flatten()
            .enumerate()
            .filter(|(_, r)| match state.show {
                Show::All => true,
                Show::Missing => r.found.is_none(),
                Show::Found => r.found.is_some(),
            })
            .filter(|(_, r)| {
                filter.is_empty()
                    || r.label.to_lowercase().contains(&filter)
                    || r.class.to_lowercase().contains(&filter)
                    || r.model_name.to_lowercase().contains(&filter)
            })
            .map(|(i, _)| i)
            .collect();
        let row_height = ui.text_style_height(&egui::TextStyle::Small) + 6.0;
        let mut pick = None;
        egui::ScrollArea::vertical()
            .id_salt("standard_elements")
            .max_height(200.0)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, visible.len(), |ui, range| {
                let rows = state.rows.as_ref().expect("checked above");
                for &i in &visible[range] {
                    let row = &rows[i];
                    let color = match &row.found {
                        Some(_) => crate::checking_ui::accepted(),
                        None => theme::danger(),
                    };
                    ui.horizontal(|ui| {
                        crate::checking_ui::dot(ui, color);
                        let response = ui
                            .selectable_label(
                                state.selected == Some(row.element),
                                RichText::new(format!("{}  ·  {}", row.label, row.model_name))
                                    .small(),
                            )
                            .on_hover_text(format!(
                                "{} · {}\n{}",
                                row.class,
                                row.part,
                                row.found.as_deref().unwrap_or("missing")
                            ));
                        if response.clicked() {
                            pick = Some(row.element);
                        }
                    });
                }
            });
        if let Some(element) = pick {
            state.selected = Some(element);
        }
    }
    ui.separator();
    element_details(app, ui, state);
}

/// The selected element's property sets and attributes; + adds one as a
/// source, and the property the item was found in is marked.
fn element_details(app: &mut OpenCheckApp, ui: &mut egui::Ui, state: &mut EditorState) {
    let Some(selected) = state.selected else {
        ui.label(
            RichText::new(
                "Pick an element to see its properties. + adds a property as a source; several \
                 sources are alternatives, so models that keep the information in different \
                 places can all pass.",
            )
            .small()
            .color(theme::text_muted()),
        );
        return;
    };
    let Some(model) = app.federation.get(selected.model) else {
        state.selected = None;
        return;
    };
    let Some(element) = model.element(selected.express_id) else {
        state.selected = None;
        return;
    };
    let found = app.checking.information_standard().and_then(|standard| {
        let facts = ModelFacts::new(model);
        standard.item_on(model, &facts, element, &state.item)
    });
    let mut show_3d = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(element.label()).strong());
        ui.label(
            RichText::new(format!("{} · {}", element.ifc_class, model.short_name))
                .small()
                .color(theme::text_muted()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(format!("{}  Show in 3D", icon::EYE))
                .clicked()
            {
                show_3d = true;
            }
        });
    });
    let place = match &found {
        Some((part, Some(place))) => {
            ui.label(
                RichText::new(format!("{}: found in {place}", part.name))
                    .small()
                    .color(crate::checking_ui::accepted()),
            );
            Some(place.clone())
        }
        Some((part, None)) => {
            ui.label(
                RichText::new(format!("{}: missing", part.name))
                    .small()
                    .color(theme::danger()),
            );
            None
        }
        None => {
            ui.label(
                RichText::new("Not checked by this item (no building part needs it).")
                    .small()
                    .color(theme::text_muted()),
            );
            None
        }
    };
    let mut add = None;
    egui::ScrollArea::vertical()
        .id_salt("standard_element_props")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for pset in model.property_sets_of(element) {
                egui::CollapsingHeader::new(RichText::new(&pset.name).small().strong())
                    .id_salt(("standard_pset", selected.model, pset.express_id))
                    .default_open(true)
                    .show(ui, |ui| {
                        for p in &pset.properties {
                            let value = p.value.to_string();
                            let here = place.as_deref()
                                == Some(format!("{}.{}", pset.name, p.name).as_str());
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button(icon::PLUS)
                                    .on_hover_text("Add as a source for this item")
                                    .clicked()
                                {
                                    add = Some((Kind::Property, p.name.clone(), pset.name.clone()));
                                }
                                let name = RichText::new(&p.name).small();
                                ui.label(if here {
                                    name.strong().color(crate::checking_ui::accepted())
                                } else {
                                    name
                                });
                                ui.label(
                                    RichText::new(if value.trim().is_empty() {
                                        "(empty)".to_owned()
                                    } else {
                                        value
                                    })
                                    .small()
                                    .color(theme::text_muted()),
                                );
                            });
                        }
                    });
            }
            egui::CollapsingHeader::new(RichText::new("IFC attributes").small().strong())
                .id_salt(("standard_attributes", selected.model, selected.express_id))
                .default_open(false)
                .show(ui, |ui| {
                    for (name, value) in &element.attributes {
                        let here = place.as_deref() == Some(format!("Attribute {name}").as_str());
                        ui.horizontal(|ui| {
                            if ui
                                .small_button(icon::PLUS)
                                .on_hover_text("Add as a source for this item")
                                .clicked()
                            {
                                add = Some((Kind::Attribute, name.clone(), String::new()));
                            }
                            let label = RichText::new(name).small();
                            ui.label(if here {
                                label.strong().color(crate::checking_ui::accepted())
                            } else {
                                label
                            });
                            ui.label(
                                RichText::new(value.to_string())
                                    .small()
                                    .color(theme::text_muted()),
                            );
                        });
                    }
                });
        });
    if let Some((kind, name, set)) = add {
        let draft = Draft {
            kind,
            name,
            name_pattern: false,
            set,
            set_pattern: false,
        };
        if !state.drafts.contains(&draft) {
            state.drafts.push(draft);
        }
    }
    if show_3d {
        app.select(Some(selected), false);
        app.fit_element(selected);
    }
}

fn candidates(app: &mut OpenCheckApp, ui: &mut egui::Ui, state: &mut EditorState) {
    ui.label(RichText::new("From the models").strong());
    ui.label(
        RichText::new("Properties and attributes the elements have. + adds one as a source.")
            .small()
            .color(theme::text_muted()),
    );
    if state.candidates.is_none() {
        if app.federation.ids().is_empty() {
            ui.label(
                RichText::new("No models loaded.")
                    .small()
                    .color(theme::text_muted()),
            );
            return;
        }
        if ui
            .button(format!("{}  Scan the models", icon::MAGNIFYING_GLASS))
            .on_hover_text("List what the elements of the selected building part carry")
            .clicked()
        {
            state.candidates = Some(scan(app, state));
        }
        return;
    }
    ui.add(
        egui::TextEdit::singleline(&mut state.candidate_filter)
            .hint_text(format!("{}  Filter", icon::MAGNIFYING_GLASS))
            .desired_width(f32::INFINITY),
    );
    let filter = state.candidate_filter.to_lowercase();
    let mut add = None;
    egui::ScrollArea::vertical()
        .id_salt("standard_candidates")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let list = state.candidates.as_ref().expect("checked above");
            if list.is_empty() {
                ui.label(
                    RichText::new("No elements of this building part in the models.")
                        .small()
                        .color(theme::text_muted()),
                );
            }
            for (i, c) in list
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    filter.is_empty()
                        || c.name.to_lowercase().contains(&filter)
                        || c.set
                            .as_ref()
                            .is_some_and(|s| s.to_lowercase().contains(&filter))
                })
                .take(300)
            {
                ui.horizontal(|ui| {
                    if ui.small_button(icon::PLUS).clicked() {
                        add = Some(i);
                    }
                    ui.label(RichText::new(&c.name).small().strong());
                    ui.label(
                        RichText::new(match &c.set {
                            Some(set) => format!("{set} · {}", c.count),
                            None => format!("attribute · {}", c.count),
                        })
                        .small()
                        .color(theme::text_muted()),
                    );
                });
            }
        });
    if let Some(i) = add {
        let c = &state.candidates.as_ref().expect("checked above")[i];
        let draft = match &c.set {
            Some(set) => Draft {
                kind: Kind::Property,
                name: c.name.clone(),
                name_pattern: false,
                set: set.clone(),
                set_pattern: false,
            },
            None => Draft {
                kind: Kind::Attribute,
                name: c.name.clone(),
                name_pattern: false,
                set: String::new(),
                set_pattern: false,
            },
        };
        if !state.drafts.contains(&draft) {
            state.drafts.push(draft);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_round_trip_sources() {
        let sources = [
            Source {
                property: Some("^Width$".into()),
                pset: Some("^Pset_WallCommon$".into()),
                ..Source::default()
            },
            Source {
                property: Some("^(Width|Bredde)$".into()),
                ..Source::default()
            },
            Source {
                attribute: Some("^ObjectType$".into()),
                ..Source::default()
            },
            Source {
                classification: true,
                ..Source::default()
            },
        ];
        for source in &sources {
            assert_eq!(&Draft::from_source(source).to_source(), source);
        }
        let draft = Draft::from_source(&sources[1]);
        assert!(draft.name_pattern);
        // Plain names are matched exactly, special characters included.
        let plain = Draft {
            kind: Kind::Property,
            name: "Bredde (mm)".into(),
            name_pattern: false,
            set: String::new(),
            set_pattern: false,
        };
        assert_eq!(
            plain.to_source().property.as_deref(),
            Some("^Bredde \\(mm\\)$")
        );
    }
}
