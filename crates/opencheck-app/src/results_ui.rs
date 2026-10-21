//! Right panel of the Checking tab.
//!
//! Results are organised the way coordinators read them: an overview matrix
//! of discipline against discipline, then one section per discipline pair
//! ("ELE vs STR"), grouped by the kind of component on the side you choose.
//! Groups and findings can be ticked and turned into issues together.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;
use opencheck_core::check::{
    CheckReport, Finding, FindingElement, FindingKind, RuleResult, RulesetResult, Severity,
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::app::{IssueTarget, OpenCheckApp};
use crate::checking::{Decision, PairKey, ResultsTab, StatusFilter, pair_key, rule_key};
use crate::checking_ui::{FOCUS_A, FOCUS_B, accepted, dot, panel_frame, severity_color};
use crate::theme::discipline_color;
use crate::{issues_ui, theme};

pub fn element_label(e: &FindingElement) -> &str {
    if e.name.is_empty() {
        &e.ifc_class
    } else {
        &e.name
    }
}

pub fn class_label(class: &str) -> &str {
    class.strip_prefix("Ifc").unwrap_or(class)
}

/// A plain-language name for the kind of component, as coordinators say it.
pub fn component_kind(class: &str) -> &str {
    match class.to_ascii_uppercase().as_str() {
        "IFCCABLECARRIERSEGMENT" => "Cable trays",
        "IFCCABLECARRIERFITTING" => "Cable tray fittings",
        "IFCCABLESEGMENT" => "Cables",
        "IFCLIGHTFIXTURE" => "Light fixtures",
        "IFCELECTRICAPPLIANCE" => "Electrical appliances",
        "IFCDUCTSEGMENT" => "Ducts",
        "IFCDUCTFITTING" => "Duct fittings",
        "IFCAIRTERMINAL" => "Air terminals",
        "IFCPIPESEGMENT" => "Pipes",
        "IFCPIPEFITTING" => "Pipe fittings",
        "IFCFLOWSEGMENT" => "Pipe and duct segments",
        "IFCFLOWFITTING" => "Fittings",
        "IFCFLOWTERMINAL" => "Terminals and fixtures",
        "IFCFLOWCONTROLLER" => "Valves and dampers",
        "IFCFLOWMOVINGDEVICE" => "Pumps and fans",
        "IFCENERGYCONVERSIONDEVICE" => "Units",
        "IFCDISTRIBUTIONELEMENT" => "Installation elements",
        "IFCDISTRIBUTIONCONTROLELEMENT" => "Control elements",
        "IFCWALL" | "IFCWALLSTANDARDCASE" => "Walls",
        "IFCSLAB" => "Slabs",
        "IFCBEAM" => "Beams",
        "IFCCOLUMN" => "Columns",
        "IFCDOOR" => "Doors",
        "IFCWINDOW" => "Windows",
        "IFCSTAIR" => "Stairs",
        "IFCSTAIRFLIGHT" => "Stair flights",
        "IFCRAILING" => "Railings",
        "IFCROOF" => "Roofs",
        "IFCFOOTING" => "Foundations",
        "IFCPILE" => "Piles",
        "IFCPLATE" => "Plates",
        "IFCMEMBER" => "Members",
        "IFCCOVERING" => "Coverings",
        "IFCCURTAINWALL" => "Curtain walls",
        "IFCCHIMNEY" => "Chimneys",
        "IFCBUILDINGELEMENTPROXY" => "Other building elements",
        "IFCFURNISHINGELEMENT" => "Furniture",
        "IFCSPACE" => "Spaces",
        _ => class_label(class),
    }
}

pub fn magnitude_text(f: &Finding) -> String {
    let mm = f.magnitude * 1000.0;
    match f.kind {
        FindingKind::Clash | FindingKind::Inside => format!("depth {mm:.0} mm"),
        FindingKind::Clearance => format!("distance {mm:.0} mm"),
        FindingKind::Duplicate => format!("offset {mm:.0} mm"),
        FindingKind::MissingInformation => format!("{:.0} missing", f.magnitude),
    }
}

/// A finding with what the results list needs to place and filter it.
#[derive(Clone, Copy)]
struct Row<'a> {
    finding: &'a Finding,
    ruleset: &'a RulesetResult,
    rule: &'a RuleResult,
    discipline_a: Discipline,
    discipline_b: Discipline,
}

struct Group<'a> {
    class: &'a str,
    rows: Vec<Row<'a>>,
}

impl Group<'_> {
    fn ids(&self) -> Vec<String> {
        self.rows.iter().map(|r| r.finding.id.clone()).collect()
    }
}

struct Pair<'a> {
    /// Id of the ruleset the pair's findings come from.
    scope: &'a str,
    key: PairKey,
    group_by: Discipline,
    count: usize,
    groups: Vec<Group<'a>>,
    /// Missing information: one discipline, grouped by building part.
    information: bool,
}

fn issue_findings(app: &OpenCheckApp) -> FxHashSet<&str> {
    app.issues
        .list
        .iter()
        .flat_map(|i| i.findings.iter().map(|s| s.finding.as_str()))
        .collect()
}

/// Findings that pass the filters.
fn visible_rows<'a>(app: &'a OpenCheckApp, report: &'a CheckReport) -> Vec<Row<'a>> {
    let with_issue = issue_findings(app);
    let filter = &app.checking.filter;
    let mut rows = Vec::new();
    for ruleset in &report.rulesets {
        if filter
            .ruleset
            .as_ref()
            .is_some_and(|id| *id != ruleset.info.id)
        {
            continue;
        }
        for rule in &ruleset.rules {
            if filter.severity.is_some_and(|s| s != rule.rule.severity) {
                continue;
            }
            if filter
                .rule
                .as_ref()
                .is_some_and(|k| *k != rule_key(&ruleset.info.id, &rule.rule.id))
            {
                continue;
            }
            for f in &rule.findings {
                let decision = app.checking.decisions.get(&f.id);
                let shown = match filter.status {
                    StatusFilter::All => true,
                    StatusFilter::Open => decision.is_none(),
                    StatusFilter::Accepted => decision == Some(&Decision::Accepted),
                    StatusFilter::Rejected => decision == Some(&Decision::Rejected),
                    StatusFilter::Issues => with_issue.contains(f.id.as_str()),
                };
                let disciplines = (
                    app.federation.get(f.a.model).map(|m| m.discipline),
                    app.federation.get(f.b.model).map(|m| m.discipline),
                );
                if let (true, (Some(discipline_a), Some(discipline_b))) = (shown, disciplines) {
                    rows.push(Row {
                        finding: f,
                        ruleset,
                        rule,
                        discipline_a,
                        discipline_b,
                    });
                }
            }
        }
    }
    rows
}

/// Discipline pairs, largest first, each grouped by the component class on
/// the side the user groups by. Information findings are grouped by
/// building part per discipline instead.
fn layout<'a>(
    app: &OpenCheckApp,
    scope: &'a str,
    rows: &[Row<'a>],
    information: bool,
) -> Vec<Pair<'a>> {
    let mut by_pair: FxHashMap<PairKey, Vec<Row<'a>>> = FxHashMap::default();
    for row in rows {
        by_pair
            .entry(pair_key(row.discipline_a, row.discipline_b))
            .or_default()
            .push(*row);
    }
    let mut pairs: Vec<Pair<'a>> = by_pair
        .into_iter()
        .map(|(key, rows)| {
            let group_by = app.checking.group_by.get(&key).copied().unwrap_or(key.0);
            let mut groups: Vec<Group<'a>> = Vec::new();
            for row in &rows {
                let class: &'a str = if information {
                    &row.finding.category
                } else if row.discipline_a == group_by {
                    &row.finding.a.ifc_class
                } else {
                    &row.finding.b.ifc_class
                };
                match groups
                    .iter_mut()
                    .find(|g| g.class.eq_ignore_ascii_case(class))
                {
                    Some(group) => group.rows.push(*row),
                    None => groups.push(Group {
                        class,
                        rows: vec![*row],
                    }),
                }
            }
            for group in &mut groups {
                group
                    .rows
                    .sort_by(|x, y| y.finding.magnitude.total_cmp(&x.finding.magnitude));
            }
            groups.sort_by(|x, y| y.rows.len().cmp(&x.rows.len()).then(x.class.cmp(y.class)));
            Pair {
                scope,
                key,
                group_by,
                count: rows.len(),
                groups,
                information,
            }
        })
        .collect();
    pairs.sort_by(|x, y| {
        y.count
            .cmp(&x.count)
            .then((x.key.0 as u8, x.key.1 as u8).cmp(&(y.key.0 as u8, y.key.1 as u8)))
    });
    pairs
}

fn order(pairs: &[Pair]) -> Vec<String> {
    pairs
        .iter()
        .flat_map(|p| &p.groups)
        .flat_map(|g| g.ids())
        .collect()
}

/// Changes requested while drawing, applied afterwards.
#[derive(Default)]
struct Actions {
    focus: Option<String>,
    click: Option<(String, bool, bool)>,
    toggle: Option<(String, bool)>,
    toggle_group: Option<(Vec<String>, bool)>,
    show_group: Option<Vec<String>>,
    /// Open the "where to look" editor on (item, part).
    adapt: Option<(String, String)>,
    decide: Option<(Vec<String>, Option<Decision>)>,
    add_issues: Option<(Vec<String>, IssueTarget)>,
    step: Option<isize>,
    group_by: Option<(PairKey, Discipline)>,
    reveal: Option<(String, PairKey)>,
    revealed: bool,
    close: bool,
}

pub fn results_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    egui::Panel::right("check_results")
        .resizable(true)
        .default_size(440.0)
        .min_size(340.0)
        .frame(panel_frame())
        .show(ui, |ui| {
            // Long names are cut, not allowed to widen the panel.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            ui.horizontal(|ui| {
                let total = app
                    .checking
                    .report
                    .as_ref()
                    .map_or(0, |r| r.finding_count());
                for (tab, label) in [
                    (
                        ResultsTab::Results,
                        format!("{}  Results  {total}", icon::LIST_CHECKS),
                    ),
                    (
                        ResultsTab::Issues,
                        format!("{}  Issues  {}", icon::FLAG, app.issues.list.len()),
                    ),
                ] {
                    let selected = app.checking.tab == tab;
                    let text = RichText::new(label);
                    let text = if selected {
                        text.family(theme::bold())
                    } else {
                        text
                    };
                    if ui.add(egui::Button::selectable(selected, text)).clicked() {
                        app.checking.tab = tab;
                    }
                }
            });
            ui.separator();
            match app.checking.tab {
                ResultsTab::Results => results(app, ui),
                ResultsTab::Issues => issues_ui::issues(app, ui),
            }
        });
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text)
            .small()
            .strong()
            .color(theme::text_muted()),
    );
}

fn swatch(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(10.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
}

fn results(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some(report) = app.checking.report.clone() else {
        ui.add_space(24.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new(icon::LIST_CHECKS)
                    .size(36.0)
                    .color(theme::border()),
            );
            ui.label(RichText::new("Run a check to see results here.").color(theme::text_muted()));
        });
        return;
    };

    // A ruleset chosen earlier may be gone after a new run.
    if app
        .checking
        .filter
        .ruleset
        .as_ref()
        .is_some_and(|id| !report.rulesets.iter().any(|r| r.info.id == *id))
    {
        app.checking.filter.ruleset = None;
    }
    ruleset_tabs(app, ui, &report);
    summary(app, ui, &report);
    filters(app, ui, &report);

    let rows = visible_rows(app, &report);
    let shown: Vec<&RulesetResult> = report
        .rulesets
        .iter()
        .filter(|r| {
            app.checking
                .filter
                .ruleset
                .as_ref()
                .is_none_or(|id| *id == r.info.id)
        })
        .collect();
    let single = shown.len() == 1;
    // Per ruleset: its geometric pairs and its information pairs.
    let sections: Vec<(&RulesetResult, Vec<Pair>, Vec<Pair>)> = shown
        .iter()
        .map(|rs| {
            let own: Vec<Row> = rows
                .iter()
                .filter(|r| std::ptr::eq(r.ruleset, *rs))
                .copied()
                .collect();
            let (info_rows, geo_rows): (Vec<Row>, Vec<Row>) =
                own.into_iter().partition(|r| r.finding.is_single());
            (
                *rs,
                layout(app, &rs.info.id, &geo_rows, false),
                layout(app, &rs.info.id, &info_rows, true),
            )
        })
        .collect();
    let mut ordered = Vec::new();
    for (_, pairs, info_pairs) in &sections {
        ordered.extend(order(pairs));
        ordered.extend(order(info_pairs));
    }
    let mut actions = Actions::default();
    selection_bar(app, ui, &mut actions);
    ui.separator();

    let detail_height = if app.checking.selected.is_some() {
        220.0
    } else {
        0.0
    };
    egui::ScrollArea::vertical()
        .id_salt("results_scroll")
        .max_height((ui.available_height() - detail_height).max(120.0))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            if sections.is_empty() {
                ui.label(RichText::new("No rulesets ran.").color(theme::text_muted()));
            }
            for (rs, pairs, info_pairs) in &sections {
                if single {
                    ruleset_heading(app, ui, rs);
                    ruleset_body(app, ui, rs, pairs, info_pairs, &mut actions);
                } else {
                    ruleset_card(app, ui, rs, pairs, info_pairs, &mut actions);
                }
            }
        });

    if let Some(id) = app.checking.selected.clone() {
        ui.separator();
        finding_detail(app, ui, &id, &mut actions);
    }
    apply(app, actions, &ordered);
}

/// Rulesets as tabs: all of them, or one at a time, with their counts.
fn ruleset_tabs(app: &mut OpenCheckApp, ui: &mut egui::Ui, report: &CheckReport) {
    if report.rulesets.len() < 2 {
        return;
    }
    let mut choice = app.checking.filter.ruleset.clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
        let tab = |ui: &mut egui::Ui, selected: bool, text: String, color: Option<Color32>| {
            let mut rich = RichText::new(text);
            if selected {
                rich = rich.family(theme::bold());
            }
            let response = ui.add(egui::Button::selectable(selected, rich));
            if let Some(color) = color {
                let r = response.rect;
                ui.painter().line_segment(
                    [
                        r.left_bottom() + egui::vec2(4.0, -1.5),
                        r.right_bottom() + egui::vec2(-4.0, -1.5),
                    ],
                    egui::Stroke::new(2.5, color),
                );
            }
            response
        };
        if tab(
            ui,
            choice.is_none(),
            format!("All  {}", report.finding_count()),
            None,
        )
        .on_hover_text("Every ruleset, each in its own section")
        .clicked()
        {
            choice = None;
        }
        for rs in &report.rulesets {
            let count: usize = rs.rules.iter().map(|r| r.findings.len()).sum();
            let selected = choice.as_deref() == Some(rs.info.id.as_str());
            if tab(
                ui,
                selected,
                format!("{}  {count}", short_name(&rs.info.name)),
                Some(ruleset_color(rs)),
            )
            .on_hover_text(&rs.info.name)
            .clicked()
            {
                choice = Some(rs.info.id.clone());
            }
        }
    });
    if choice != app.checking.filter.ruleset {
        app.checking.filter.ruleset = choice;
        // A rule of another ruleset would hide everything.
        app.checking.filter.rule = None;
    }
    ui.add_space(2.0);
}

/// "Clash: MEP vs structure" -> "MEP vs structure" for tabs.
fn short_name(name: &str) -> &str {
    name.split_once(": ")
        .map(|(_, rest)| rest)
        .filter(|rest| !rest.is_empty())
        .unwrap_or(name)
}

/// A stable colour per ruleset, to tell sections apart at a glance.
fn ruleset_color(rs: &RulesetResult) -> Color32 {
    const COLORS: [Color32; 8] = [
        Color32::from_rgb(43, 127, 246),
        Color32::from_rgb(230, 126, 34),
        Color32::from_rgb(39, 174, 96),
        Color32::from_rgb(142, 68, 173),
        Color32::from_rgb(214, 69, 65),
        Color32::from_rgb(22, 160, 133),
        Color32::from_rgb(192, 57, 43),
        Color32::from_rgb(127, 140, 141),
    ];
    let hash = rs
        .info
        .id
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    COLORS[hash as usize % COLORS.len()]
}

/// Counts of a ruleset's findings per severity, before filters.
fn ruleset_counts(ui: &mut egui::Ui, rs: &RulesetResult) {
    for severity in [Severity::Critical, Severity::Moderate, Severity::Low] {
        let n: usize = rs
            .rules
            .iter()
            .filter(|r| r.rule.severity == severity)
            .map(|r| r.findings.len())
            .sum();
        if n > 0 {
            dot(ui, severity_color(severity));
            ui.label(RichText::new(n.to_string()).small())
                .on_hover_text(severity.label());
        }
    }
}

/// Title of the one ruleset shown.
fn ruleset_heading(app: &OpenCheckApp, ui: &mut egui::Ui, rs: &RulesetResult) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(4.0, 18.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, ruleset_color(rs));
        ui.label(RichText::new(&rs.info.name).strong());
        ruleset_counts(ui, rs);
    });
    ruleset_scope(app, ui, rs);
}

/// Which models and rules the ruleset ran.
fn ruleset_scope(app: &OpenCheckApp, ui: &mut egui::Ui, rs: &RulesetResult) {
    let models: Vec<String> = rs
        .models
        .iter()
        .filter_map(|id| {
            app.federation
                .get(*id)
                .map(|m| m.discipline.code().to_owned())
        })
        .collect();
    ui.label(
        RichText::new(format!(
            "{} rules · {} models ({})",
            rs.rules.len(),
            rs.models.len(),
            models.join(", ")
        ))
        .small()
        .color(theme::text_muted()),
    );
}

/// One ruleset as a card in the "All" view, collapsible.
fn ruleset_card(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    rs: &RulesetResult,
    pairs: &[Pair],
    info_pairs: &[Pair],
    actions: &mut Actions,
) {
    let total: usize = rs.rules.iter().map(|r| r.findings.len()).sum();
    let shown: usize = pairs.iter().chain(info_pairs).map(|p| p.count).sum();
    let id = ui.make_persistent_id(("ruleset_card", &rs.info.id));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, total > 0);
    if app
        .checking
        .reveal_pair
        .as_ref()
        .is_some_and(|(scope, _)| *scope == rs.info.id)
    {
        state.set_open(true);
    }
    let color = ruleset_color(rs);
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(8)
        .inner_margin(egui::Margin {
            left: 10,
            right: 8,
            top: 6,
            bottom: 6,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let rect = ui.max_rect();
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    rect.left_top() + egui::vec2(-8.0, -2.0),
                    rect.left_bottom() + egui::vec2(-5.0, 2.0),
                ),
                2.0,
                color,
            );
            state
                .show_header(ui, |ui| {
                    ui.label(RichText::new(&rs.info.name).strong())
                        .on_hover_text(&rs.info.description);
                    if total == 0 {
                        ui.label(
                            RichText::new(format!("{}  No findings", icon::CHECK_CIRCLE))
                                .small()
                                .color(accepted()),
                        );
                    } else {
                        count_badge(ui, shown);
                        if shown != total {
                            ui.label(
                                RichText::new(format!("of {total}"))
                                    .small()
                                    .color(theme::text_muted()),
                            )
                            .on_hover_text("Findings that pass the filters, of all findings");
                        }
                        ruleset_counts(ui, rs);
                    }
                })
                .body(|ui| {
                    ruleset_scope(app, ui, rs);
                    ruleset_body(app, ui, rs, pairs, info_pairs, actions);
                });
        });
    ui.add_space(6.0);
}

/// The overview and findings of one ruleset.
fn ruleset_body(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    rs: &RulesetResult,
    pairs: &[Pair],
    info_pairs: &[Pair],
    actions: &mut Actions,
) {
    let has_geometry_rules = rs.rules.iter().any(|r| r.information.is_none());
    let has_information = rs.rules.iter().any(|r| r.information.is_some());
    let total: usize = rs.rules.iter().map(|r| r.findings.len()).sum();
    if has_geometry_rules {
        if total > 0 {
            section_label(ui, "OVERVIEW (DISCIPLINE VS DISCIPLINE)");
            matrix(app, ui, &rs.info.id, pairs, actions);
            section_label(ui, "FINDINGS PER PAIR");
        }
        if pairs.is_empty() && !has_information {
            let text = if total == 0 {
                "No findings."
            } else {
                "No findings match the filters."
            };
            ui.label(RichText::new(text).color(theme::text_muted()));
        }
        for pair in pairs {
            pair_section(app, ui, pair, actions);
        }
    }
    if has_information {
        section_label(ui, "INFORMATION: FILL RATE PER BUILDING PART");
        information_overview(app, ui, rs, info_pairs, actions);
        section_label(ui, "MISSING INFORMATION PER DISCIPLINE");
        if info_pairs.is_empty() {
            ui.label(
                RichText::new("Every checked element has its information.")
                    .color(theme::text_muted()),
            );
        }
        for pair in info_pairs {
            pair_section(app, ui, pair, actions);
        }
    }
}

/// Totals before filtering, like "238 total · 210 open · …".
fn summary(app: &OpenCheckApp, ui: &mut egui::Ui, report: &CheckReport) {
    let with_issue = issue_findings(app);
    let all: Vec<(&Finding, Severity)> = report
        .rulesets
        .iter()
        .filter(|r| {
            app.checking
                .filter
                .ruleset
                .as_ref()
                .is_none_or(|id| *id == r.info.id)
        })
        .flat_map(|r| &r.rules)
        .flat_map(|r| r.findings.iter().map(move |f| (f, r.rule.severity)))
        .collect();
    let decided = |f: &Finding, d: Decision| app.checking.decisions.get(&f.id) == Some(&d);
    let open = all
        .iter()
        .filter(|(f, _)| !app.checking.decisions.contains_key(&f.id))
        .count();
    let accepted = all
        .iter()
        .filter(|(f, _)| decided(f, Decision::Accepted))
        .count();
    let rejected = all
        .iter()
        .filter(|(f, _)| decided(f, Decision::Rejected))
        .count();
    let issues = all
        .iter()
        .filter(|(f, _)| with_issue.contains(f.id.as_str()))
        .count();
    ui.label(
        RichText::new(format!(
            "{} total · {open} open · {accepted} accepted · {rejected} rejected · {issues} issues",
            all.len()
        ))
        .small(),
    );
    ui.horizontal_wrapped(|ui| {
        for severity in [Severity::Critical, Severity::Moderate, Severity::Low] {
            let n = all
                .iter()
                .filter(|(f, s)| *s == severity && !app.checking.decisions.contains_key(&f.id))
                .count();
            dot(ui, severity_color(severity));
            ui.label(RichText::new(format!("{n} {}", severity.label().to_lowercase())).small());
            ui.add_space(4.0);
        }
        ui.label(
            RichText::new(format!(
                "· checked in {:.1} s",
                report.duration.as_secs_f32()
            ))
            .small()
            .color(theme::text_muted()),
        );
    });
    if app.checking.stale {
        ui.label(
            RichText::new(format!(
                "{}  Models or rulesets changed since this run",
                icon::WARNING
            ))
            .small()
            .color(severity_color(Severity::Moderate)),
        );
    }
}

fn filters(app: &mut OpenCheckApp, ui: &mut egui::Ui, report: &CheckReport) {
    let filter = &mut app.checking.filter;
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("filter_status")
            .selected_text(filter.status.label())
            .width(110.0)
            .show_ui(ui, |ui| {
                for s in StatusFilter::ALL {
                    ui.selectable_value(&mut filter.status, s, s.label());
                }
            });
        egui::ComboBox::from_id_salt("filter_severity")
            .selected_text(filter.severity.map_or("Severity: all", |s| s.label()))
            .width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.severity, None, "Severity: all");
                for s in [Severity::Critical, Severity::Moderate, Severity::Low] {
                    ui.selectable_value(&mut filter.severity, Some(s), s.label());
                }
            });
        let rule_name = filter
            .rule
            .as_ref()
            .and_then(|key| {
                report.rulesets.iter().find_map(|rs| {
                    rs.rules
                        .iter()
                        .find(|r| rule_key(&rs.info.id, &r.rule.id) == *key)
                        .map(|r| r.rule.name.clone())
                })
            })
            .unwrap_or_else(|| "Rule: all".to_owned());
        egui::ComboBox::from_id_salt("filter_rule")
            .selected_text(rule_name)
            .width(150.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter.rule, None, "Rule: all");
                for rs in report
                    .rulesets
                    .iter()
                    .filter(|rs| filter.ruleset.as_ref().is_none_or(|id| *id == rs.info.id))
                {
                    for r in &rs.rules {
                        let key = rule_key(&rs.info.id, &r.rule.id);
                        ui.selectable_value(&mut filter.rule, Some(key), &r.rule.name)
                            .on_hover_text(&rs.info.name);
                    }
                }
            });
    });
}

/// Counts per discipline pair; a click opens that pair below.
fn matrix(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    scope: &str,
    pairs: &[Pair],
    actions: &mut Actions,
) {
    let mut disciplines: Vec<Discipline> =
        app.federation.iter().map(|(_, m)| m.discipline).collect();
    disciplines.sort_by_key(|d| *d as u8);
    disciplines.dedup();
    if disciplines.is_empty() {
        return;
    }
    let counts: FxHashMap<PairKey, usize> = pairs.iter().map(|p| (p.key, p.count)).collect();
    let heading = |ui: &mut egui::Ui, d: Discipline| {
        ui.horizontal(|ui| {
            swatch(ui, discipline_color(d));
            ui.label(RichText::new(d.code()).small().strong())
                .on_hover_text(d.label());
        });
    };
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(6))
        .show(ui, |ui| {
            egui::Grid::new(("discipline_matrix", scope))
                .spacing([4.0, 4.0])
                .min_col_width(46.0)
                .show(ui, |ui| {
                    ui.label("");
                    for d in &disciplines {
                        heading(ui, *d);
                    }
                    ui.end_row();
                    for (r, row) in disciplines.iter().enumerate() {
                        heading(ui, *row);
                        for (c, col) in disciplines.iter().enumerate() {
                            if c < r {
                                ui.label("");
                                continue;
                            }
                            let key = pair_key(*row, *col);
                            match counts.get(&key) {
                                Some(&n) => {
                                    let cell = egui::Button::new(
                                        RichText::new(n.to_string())
                                            .strong()
                                            .color(badge_colors().0),
                                    )
                                    .fill(badge_colors().1)
                                    .min_size(egui::vec2(46.0, 24.0));
                                    if ui
                                        .add(cell)
                                        .on_hover_text(format!(
                                            "{} vs {}",
                                            row.label(),
                                            col.label()
                                        ))
                                        .clicked()
                                    {
                                        actions.reveal = Some((scope.to_owned(), key));
                                    }
                                }
                                None => {
                                    ui.label(RichText::new("–").color(theme::text_muted()));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

/// Fill rate per building part from the information rules, before filters.
/// A click on a part shows its findings in 3D.
fn information_overview(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    ruleset: &RulesetResult,
    info_pairs: &[Pair],
    actions: &mut Actions,
) {
    let levels: Vec<String> = {
        let mut list: Vec<String> = app
            .federation
            .ids()
            .into_iter()
            .map(|id| app.checking.level_of(id))
            .collect();
        list.sort();
        list.dedup();
        list
    };
    let rules: Vec<&RuleResult> = ruleset
        .rules
        .iter()
        .filter(|r| r.information.is_some())
        .collect();
    if rules.iter().any(|r| crate::checking::uses_levels(&r.rule)) {
        ui.label(
            RichText::new(format!(
                "Checked at {}",
                levels
                    .iter()
                    .map(|l| crate::checking::level_label(l))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
            .small()
            .color(theme::text_muted()),
        );
    }
    for rule in rules {
        let Some(summary) = &rule.information else {
            continue;
        };
        egui::Frame::new()
            .stroke(egui::Stroke::new(1.0, theme::border()))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(&summary.standard).small().strong());
                for note in &summary.notes {
                    ui.label(
                        RichText::new(note)
                            .small()
                            .color(severity_color(Severity::Moderate)),
                    );
                }
                if summary.parts.is_empty() {
                    ui.label(
                        RichText::new("No elements matched a building part or specification.")
                            .small()
                            .color(theme::text_muted()),
                    );
                }
                for part in &summary.parts {
                    let share = part.complete as f32 / part.elements.max(1) as f32;
                    let id = ui.make_persistent_id(("fill", &ruleset.info.id, &part.part));
                    egui::collapsing_header::CollapsingState::load_with_default_open(
                        ui.ctx(),
                        id,
                        false,
                    )
                    .show_header(ui, |ui| {
                        let name = ui
                            .add(egui::Button::new(RichText::new(&part.name).strong()).frame(false))
                            .on_hover_text("Show the elements with missing information in 3D");
                        if name.clicked() {
                            let ids: Vec<String> = info_pairs
                                .iter()
                                .flat_map(|p| &p.groups)
                                .filter(|g| g.class == part.name)
                                .flat_map(|g| g.ids())
                                .collect();
                            if !ids.is_empty() {
                                actions.show_group = Some(ids);
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{}/{}", part.complete, part.elements))
                                    .small()
                                    .color(theme::text_muted()),
                            )
                            .on_hover_text("Elements with all required information");
                            fill_bar(ui, share);
                        });
                    })
                    .body(|ui| {
                        let adaptable = crate::checking::uses_levels(&rule.rule);
                        for item in &part.items {
                            let share = item.present as f32 / item.required.max(1) as f32;
                            ui.horizontal(|ui| {
                                let label = ui.add(
                                    egui::Label::new(RichText::new(&item.name).small())
                                        .sense(egui::Sense::click()),
                                );
                                let label = if item.found_in.is_empty() {
                                    label
                                } else {
                                    label.on_hover_ui(|ui| {
                                        ui.label(RichText::new("Found in").small().strong());
                                        for (place, n) in item.found_in.iter().take(8) {
                                            ui.label(
                                                RichText::new(format!("{n:>6}  {place}"))
                                                    .small()
                                                    .monospace(),
                                            );
                                        }
                                        if item.found_in.len() > 8 {
                                            ui.label(
                                                RichText::new(format!(
                                                    "… {} more places",
                                                    item.found_in.len() - 8
                                                ))
                                                .small(),
                                            );
                                        }
                                        if adaptable {
                                            ui.label(
                                                RichText::new("Click to change where to look")
                                                    .small()
                                                    .color(theme::text_muted()),
                                            );
                                        }
                                    })
                                };
                                if adaptable && label.clicked() {
                                    actions.adapt = Some((item.item.clone(), part.part.clone()));
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{}/{}",
                                                item.present, item.required
                                            ))
                                            .small()
                                            .color(theme::text_muted()),
                                        );
                                        fill_bar(ui, share);
                                    },
                                );
                            });
                        }
                    });
                }
            });
    }
}

/// A small bar from red (nothing filled in) to green (complete).
fn fill_bar(ui: &mut egui::Ui, share: f32) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(90.0, 10.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, theme::border());
    let color = if share >= 0.999 {
        accepted()
    } else if share >= 0.5 {
        severity_color(Severity::Moderate)
    } else {
        severity_color(Severity::Critical)
    };
    let mut filled = rect;
    filled.set_width(rect.width() * share.clamp(0.0, 1.0));
    painter.rect_filled(filled, 3.0, color);
    response.on_hover_text(format!("{:.0} %", share * 100.0));
}

/// Text and background of finding counts, for the light or dark theme.
fn badge_colors() -> (Color32, Color32) {
    if theme::is_dark() {
        (
            Color32::from_rgb(255, 160, 150),
            Color32::from_rgb(78, 30, 30),
        )
    } else {
        (
            Color32::from_rgb(180, 30, 40),
            Color32::from_rgb(253, 228, 230),
        )
    }
}

fn count_badge(ui: &mut egui::Ui, n: usize) {
    let (text, background) = badge_colors();
    ui.label(
        RichText::new(format!(" {n} "))
            .strong()
            .color(text)
            .background_color(background),
    );
}

fn pair_section(app: &OpenCheckApp, ui: &mut egui::Ui, pair: &Pair, actions: &mut Actions) {
    let (d1, d2) = pair.key;
    let id = ui.make_persistent_id(("pair", pair.scope, d1 as u8, d2 as u8, pair.information));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    let reveal = app
        .checking
        .reveal_pair
        .as_ref()
        .is_some_and(|(scope, key)| scope == pair.scope && *key == pair.key);
    if reveal {
        state.set_open(true);
    }
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, theme::border()))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            state
                .show_header(ui, |ui| {
                    swatch(ui, discipline_color(d1));
                    let header = ui.label(RichText::new(d1.code()).strong());
                    if pair.information {
                        ui.label(RichText::new(d1.label()).color(theme::text_muted()));
                    } else {
                        ui.label(RichText::new("vs").color(theme::text_muted()));
                        swatch(ui, discipline_color(d2));
                        ui.label(RichText::new(d2.code()).strong());
                    }
                    count_badge(ui, pair.count);
                    if reveal {
                        header.scroll_to_me(Some(egui::Align::TOP));
                        actions.revealed = true;
                    }
                })
                .body(|ui| {
                    if d1 != d2 && !pair.information {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("Group by:")
                                    .small()
                                    .color(theme::text_muted()),
                            );
                            for d in [d1, d2] {
                                let selected = pair.group_by == d;
                                let text =
                                    RichText::new(format!("■ {}", d.code())).color(if selected {
                                        discipline_color(d)
                                    } else {
                                        theme::text_muted()
                                    });
                                let text = if selected { text.strong() } else { text };
                                if ui.add(egui::Button::selectable(selected, text)).clicked() {
                                    actions.group_by = Some((pair.key, d));
                                }
                            }
                        });
                    }
                    for group in &pair.groups {
                        group_section(app, ui, pair, group, actions);
                    }
                });
        });
    ui.add_space(4.0);
}

fn group_section(
    app: &OpenCheckApp,
    ui: &mut egui::Ui,
    pair: &Pair,
    group: &Group,
    actions: &mut Actions,
) {
    let ids = group.ids();
    let id = ui.make_persistent_id((
        "group",
        pair.key.0 as u8,
        pair.key.1 as u8,
        pair.group_by as u8,
        group.class,
        pair.information,
        pair.scope,
    ));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false)
        .show_header(ui, |ui| {
            let mut all = ids.iter().all(|i| app.checking.marked.contains(i));
            if ui
                .checkbox(&mut all, "")
                .on_hover_text("Select every finding in the group")
                .changed()
            {
                actions.toggle_group = Some((ids.clone(), all));
            }
            swatch(ui, discipline_color(pair.group_by));
            let kind = component_kind(group.class);
            let raw = class_label(group.class);
            let response = ui
                .add(egui::Button::new(RichText::new(kind).strong()).frame(false))
                .on_hover_text("Show all findings of the group in 3D");
            if response.clicked() {
                actions.show_group = Some(ids.clone());
            }
            if !kind.eq_ignore_ascii_case(raw) {
                ui.label(
                    RichText::new(raw.to_uppercase())
                        .small()
                        .color(theme::text_muted()),
                );
            }
            ui.label(RichText::new(group.rows.len().to_string()).small().strong());
        })
        .body(|ui| {
            // Large groups show their rows in pages so scrolling stays smooth.
            let limit_id = id.with("limit");
            let mut limit: usize = ui
                .ctx()
                .data_mut(|d| d.get_temp(limit_id))
                .unwrap_or(ROWS_PER_PAGE);
            // Keep the row under review visible even when it is further down.
            if let Some(at) = app
                .checking
                .selected
                .as_ref()
                .and_then(|s| group.rows.iter().position(|r| r.finding.id == *s))
            {
                limit = limit.max(at + 1);
            }
            for row in group.rows.iter().take(limit) {
                finding_row(app, ui, row, actions);
            }
            let rest = group.rows.len().saturating_sub(limit);
            if rest > 0 {
                let more = rest.min(ROWS_PER_PAGE);
                if ui
                    .small_button(format!("Show {more} more ({rest} hidden)"))
                    .clicked()
                {
                    ui.ctx()
                        .data_mut(|d| d.insert_temp(limit_id, limit + ROWS_PER_PAGE));
                }
            }
        });
}

/// Adds a widget that may use the row's width minus `reserve` points, so
/// what follows it on the row still fits and the panel is not widened.
pub fn bounded(
    ui: &mut egui::Ui,
    reserve: f32,
    add: impl FnOnce(&mut egui::Ui) -> egui::Response,
) -> egui::Response {
    let width = (ui.available_width() - reserve).max(60.0);
    ui.scope(|ui| {
        ui.set_max_width(width);
        add(ui)
    })
    .inner
}

/// Rows drawn per group before "Show more".
const ROWS_PER_PAGE: usize = 200;

/// Scrolls a row into view once when it becomes the selected one, and then
/// leaves scrolling to the user.
pub fn scroll_once(ui: &egui::Ui, response: &egui::Response, list: &str, id: &str) {
    let key = egui::Id::new(("scrolled_to", list));
    let done = ui
        .ctx()
        .data(|d| d.get_temp::<String>(key))
        .is_some_and(|last| last == id);
    if !done {
        response.scroll_to_me(None);
        ui.ctx().data_mut(|d| d.insert_temp(key, id.to_owned()));
    }
}

fn finding_row(app: &OpenCheckApp, ui: &mut egui::Ui, row: &Row, actions: &mut Actions) {
    let f = row.finding;
    let focused = app.checking.selected.as_deref() == Some(f.id.as_str());
    let mut marked = app.checking.marked.contains(&f.id);
    let decision = app.checking.decisions.get(&f.id).copied();
    let is_issue = app.issues.for_finding(&f.id).is_some();
    ui.horizontal(|ui| {
        if ui.checkbox(&mut marked, "").changed() {
            actions.toggle = Some((f.id.clone(), marked));
        }
        let (glyph, color) = if is_issue {
            (icon::FLAG, theme::accent())
        } else {
            match decision {
                Some(Decision::Accepted) => (icon::CHECK_CIRCLE, accepted()),
                Some(Decision::Rejected) => (icon::X_CIRCLE, theme::text_muted()),
                None => (icon::CIRCLE, severity_color(row.rule.rule.severity)),
            }
        };
        ui.label(RichText::new(glyph).color(color));
        let text = if f.is_single() {
            element_label(&f.a).to_owned()
        } else {
            format!(
                "{}  {}  {}",
                element_label(&f.a),
                icon::ARROWS_LEFT_RIGHT,
                element_label(&f.b)
            )
        };
        let mut label = RichText::new(text);
        if decision == Some(Decision::Rejected) {
            label = label.color(theme::text_muted());
        }
        if focused {
            label = label.family(theme::bold());
        }
        let hover = if f.is_single() {
            format!(
                "{} [{}]\n{}\nMissing: {}",
                element_label(&f.a),
                f.a.ifc_class,
                f.category,
                f.details.join(", ")
            )
        } else {
            format!(
                "{} [{}]\n{} [{}]\n{}\n{}",
                element_label(&f.a),
                f.a.ifc_class,
                element_label(&f.b),
                f.b.ifc_class,
                magnitude_text(f),
                row.rule.rule.name
            )
        };
        let response = bounded(ui, 80.0, |ui| {
            ui.add(
                egui::Button::selectable(focused, label)
                    .frame_when_inactive(false)
                    .truncate(),
            )
        })
        .on_hover_text(hover);
        let size = if f.is_single() {
            format!("{} missing", f.details.len())
        } else {
            format!("{:.0} mm", f.magnitude * 1000.0)
        };
        ui.label(RichText::new(size).small().color(theme::text_muted()));
        if focused {
            scroll_once(ui, &response, "results", &f.id);
        }
        if response.clicked() {
            let (ctrl, shift) = ui.input(|i| (i.modifiers.command, i.modifiers.shift));
            if ctrl || shift {
                actions.click = Some((f.id.clone(), ctrl, shift));
            }
            actions.focus = Some(f.id.clone());
        }
    });
}

/// Bulk actions for the ticked findings.
fn selection_bar(app: &OpenCheckApp, ui: &mut egui::Ui, actions: &mut Actions) {
    let marked = &app.checking.marked;
    if marked.is_empty() {
        ui.label(
            RichText::new("Tick findings or whole groups to act on several at once")
                .small()
                .color(theme::text_muted()),
        );
        return;
    }
    egui::Frame::new()
        .fill(theme::accent_soft())
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} selected", marked.len())).strong());
                let new: Vec<String> = marked
                    .iter()
                    .filter(|id| app.issues.for_finding(id).is_none())
                    .cloned()
                    .collect();
                let label = if new.len() > 1 {
                    format!("{}  Add as one issue ({})", icon::FLAG, new.len())
                } else {
                    format!("{}  Add to issues", icon::FLAG)
                };
                let add =
                    egui::Button::new(RichText::new(label).strong().color(theme::on_accent()))
                        .fill(theme::accent());
                if ui
                    .add_enabled(!new.is_empty(), add)
                    .on_hover_text(
                        "Bundle the selected findings in one issue. \
                         Issues are what goes into the report.",
                    )
                    .clicked()
                {
                    actions.add_issues = Some((new.clone(), IssueTarget::Bundle));
                }
                ui.add_enabled_ui(!new.is_empty(), |ui| {
                    ui.menu_button(icon::CARET_DOWN, |ui| {
                        ui.set_min_width(260.0);
                        if new.len() > 1
                            && ui
                                .button(format!("{}  As {} separate issues", icon::COPY, new.len()))
                                .clicked()
                        {
                            actions.add_issues = Some((new.clone(), IssueTarget::Separate));
                            ui.close();
                        }
                        if !app.issues.list.is_empty() {
                            ui.separator();
                            ui.label(
                                RichText::new("Add to an existing issue")
                                    .small()
                                    .color(theme::text_muted()),
                            );
                            for issue in app.issues.list.iter().rev() {
                                let text = format!(
                                    "{}  {}  ({})",
                                    icon::FLAG,
                                    issue.title,
                                    issue.findings.len()
                                );
                                if ui.button(text).clicked() {
                                    actions.add_issues = Some((
                                        new.clone(),
                                        IssueTarget::Existing(issue.guid.clone()),
                                    ));
                                    ui.close();
                                }
                            }
                        }
                    })
                    .response
                    .on_hover_text("More ways to add");
                });
                if ui
                    .button(RichText::new(format!("{}  Accept", icon::CHECK)).color(accepted()))
                    .clicked()
                {
                    actions.decide = Some((marked.clone(), Some(Decision::Accepted)));
                }
                if ui.button(format!("{}  Reject", icon::X)).clicked() {
                    actions.decide = Some((marked.clone(), Some(Decision::Rejected)));
                }
                if ui
                    .button(format!("{}  Show", icon::EYE))
                    .on_hover_text("Show the selected findings in 3D")
                    .clicked()
                {
                    actions.show_group = Some(marked.clone());
                }
                if ui.button("Clear").clicked() {
                    actions.toggle_group = Some((marked.clone(), false));
                }
            });
        });
}

fn finding_detail(app: &OpenCheckApp, ui: &mut egui::Ui, id: &str, actions: &mut Actions) {
    let Some((ruleset, rule, f)) = app.checking.find(id) else {
        return;
    };
    let decision = app.checking.decisions.get(id).copied();
    ui.horizontal(|ui| {
        dot(ui, severity_color(rule.rule.severity));
        ui.label(
            RichText::new(format!(
                "{} · {}",
                f.kind.label(),
                rule.rule.severity.label()
            ))
            .heading()
            .strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new(icon::X).size(16.0)).frame(false))
                .on_hover_text("Close")
                .clicked()
            {
                actions.close = true;
            }
            ui.add_space(8.0);
            if ui
                .add(egui::Button::new(icon::CARET_RIGHT).frame(false))
                .on_hover_text("Next finding")
                .clicked()
            {
                actions.step = Some(1);
            }
            if ui
                .add(egui::Button::new(icon::CARET_LEFT).frame(false))
                .on_hover_text("Previous finding")
                .clicked()
            {
                actions.step = Some(-1);
            }
        });
    });
    ui.label(RichText::new(&rule.rule.name).color(theme::text_muted()))
        .on_hover_text(&ruleset.info.name);
    let sides: &[(&FindingElement, Color32)] = if f.is_single() {
        if crate::checking::uses_levels(&rule.rule) {
            let level = crate::checking::level_label(&app.checking.level_of(f.a.model));
            ui.label(RichText::new(format!("{} · {level}", f.category)).strong());
        } else {
            ui.label(RichText::new(&f.category).strong());
        }
        ui.label(
            RichText::new(format!("Missing: {}", f.details.join(", ")))
                .color(severity_color(rule.rule.severity)),
        );
        &[(&f.a, FOCUS_A)]
    } else {
        ui.label(magnitude_text(f));
        &[(&f.a, FOCUS_A), (&f.b, FOCUS_B)]
    };
    for &(element, color) in sides {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(12.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 3.0, color);
            let model = app
                .federation
                .get(element.model)
                .map(|m| m.short_name.clone())
                .unwrap_or_default();
            ui.label(RichText::new(element_label(element)).strong());
            ui.label(
                RichText::new(format!("{} · {model}", element.ifc_class))
                    .small()
                    .color(theme::text_muted()),
            );
        });
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let one = vec![id.to_owned()];
        if app.issues.for_finding(id).is_none() {
            let add = egui::Button::new(
                RichText::new(format!("{}  Add to issues", icon::FLAG)).color(theme::on_accent()),
            )
            .fill(theme::accent());
            if ui.add(add).clicked() {
                actions.add_issues = Some((one.clone(), IssueTarget::Separate));
            }
        }
        let accept = egui::Button::selectable(
            decision == Some(Decision::Accepted),
            RichText::new(format!("{}  Accept", icon::CHECK)).color(accepted()),
        );
        if ui
            .add(accept)
            .on_hover_text("A real problem that must be handled")
            .clicked()
        {
            actions.decide = Some((one.clone(), Some(Decision::Accepted)));
        }
        let reject = egui::Button::selectable(
            decision == Some(Decision::Rejected),
            format!("{}  Reject", icon::X),
        );
        if ui.add(reject).on_hover_text("Not a problem").clicked() {
            actions.decide = Some((one.clone(), Some(Decision::Rejected)));
        }
        if decision.is_some() && ui.button("Reset").clicked() {
            actions.decide = Some((one, None));
        }
    });
}

fn apply(app: &mut OpenCheckApp, actions: Actions, order: &[String]) {
    if actions.close {
        app.checking.selected = None;
        app.refresh_finding_focus();
    }
    if let Some(key) = actions.reveal {
        app.checking.reveal_pair = Some(key);
    } else if actions.revealed {
        app.checking.reveal_pair = None;
    }
    if let Some((key, d)) = actions.group_by {
        app.checking.group_by.insert(key, d);
    }
    let marks_changed =
        actions.toggle.is_some() || actions.toggle_group.is_some() || actions.click.is_some();
    if let Some((id, on)) = actions.toggle {
        app.checking.toggle_mark(&id, on);
    }
    if let Some((ids, on)) = actions.toggle_group {
        app.checking.mark_group(&ids, on);
    }
    if let Some((id, ctrl, shift)) = actions.click {
        app.checking.click(&id, ctrl, shift, order);
    }
    // Every ticked finding lights up in 3D, without moving the camera.
    if marks_changed {
        app.refresh_finding_focus();
    }
    if let Some(id) = actions.focus {
        app.focus_finding(&id);
    }
    if let Some((item, part)) = actions.adapt {
        crate::standard_editor::open(app, Some(&item), Some(&part));
    }
    if let Some(ids) = actions.show_group {
        app.show_group(&ids);
    }
    if let Some((ids, target)) = actions.add_issues {
        app.request_issues(ids, target);
    }
    if let Some((ids, decision)) = actions.decide {
        let current = app.checking.selected.clone();
        for id in &ids {
            app.checking.decide(id, decision);
        }
        app.session.dirty = true;
        // Reviewing one at a time: move on to the next undecided finding.
        if decision.is_some()
            && ids.len() == 1
            && let Some(current) = current
            && let Some(at) = order.iter().position(|o| *o == current)
            && let Some(next) = order[at + 1..]
                .iter()
                .find(|o| !app.checking.decisions.contains_key(*o))
                .cloned()
        {
            app.focus_finding(&next);
        }
    }
    if let Some(offset) = actions.step
        && let Some(current) = app.checking.selected.clone()
        && let Some(at) = order.iter().position(|o| *o == current)
    {
        let next = (at as isize + offset).rem_euclid(order.len() as isize) as usize;
        let id = order[next].clone();
        app.focus_finding(&id);
    }
}
