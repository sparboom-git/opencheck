//! Triage of findings: a category per finding by the Danish C402 practice
//! (C1 not critical, C2 critical, C3 very critical), suggested by
//! OpenCheck's pre-sorting or by Claude and approved by the reviewer.
//!
//! - [`Rating`]: a suggested (or approved) category with how sure it is and
//!   why.
//! - [`groups`]: findings alike enough to be judged together: same rule
//!   and the same kinds of elements (class and type) on the two sides,
//!   with their systems and storeys counted.
//! - [`presort`]: fixed rules for known noise (insulation around its own
//!   pipe, the same element in two models, shallow overlaps, penetrations
//!   at a planned opening), switched on per clash rule.
//! - [`TriageFile`]: a project's own rules and the reviewer's corrections,
//!   which Claude reads before a review.

use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

use crate::check::{CheckReport, Finding, FindingElement, FindingKind, Severity};
use crate::{Aabb, Element, Federation, IfcModel, ModelId};

/// Name of the file the running app writes while Claude may connect: the
/// port and key of its local connection.
pub const LINK_FILE: &str = "claude-link.json";

/// C402 criticality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Category {
    C1,
    C2,
    C3,
}

impl Category {
    pub const ALL: [Category; 3] = [Category::C1, Category::C2, Category::C3];

    /// The issue priority the category is.
    pub fn severity(self) -> Severity {
        match self {
            Category::C1 => Severity::Low,
            Category::C2 => Severity::Moderate,
            Category::C3 => Severity::Critical,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Category::C1 => "C1",
            Category::C2 => "C2",
            Category::C3 => "C3",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_uppercase().as_str() {
            "C1" => Some(Category::C1),
            "C2" => Some(Category::C2),
            "C3" => Some(Category::C3),
            _ => None,
        }
    }
}

/// Who suggested a rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// OpenCheck's fixed pre-sorting rules.
    Presort,
    Claude,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Presort => "Pre-sorting",
            Source::Claude => "Claude",
        }
    }
}

/// A suggested category for a finding. Approved ratings are kept as a
/// record of what the reviewer decided on, as long as the finding is in
/// the results.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rating {
    pub category: Category,
    /// Not a real problem at all (always C1).
    #[serde(default)]
    pub noise: bool,
    /// How sure the suggestion is, 0 to 1.
    pub confidence: f32,
    /// One or two sentences a reviewer can check quickly.
    pub reason: String,
    pub source: Source,
    /// Approved by the reviewer, possibly as another category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<Category>,
    /// The group the rating was given for, when it was given for a group.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Whether an image of the finding was looked at.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub image: bool,
    /// When the rating was given, RFC 3339.
    #[serde(default)]
    pub at: String,
}

impl Rating {
    pub fn is_proposed(&self) -> bool {
        self.approved.is_none()
    }
}

/// Pre-sorting settings of a clash rule: which fixed rules mark findings
/// as noise. Off unless switched on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Presort {
    /// Runs after every check.
    pub enabled: bool,
    /// Overlaps shallower than this are noise, millimetres; 0 switches it off.
    /// Above the clash rules' own tolerance (10 mm) to make a difference.
    pub depth_mm: f32,
    /// Insulation around its own pipe or duct.
    pub insulation: bool,
    /// The same element (same GlobalId) in two models.
    pub same_element: bool,
    /// A penetration where a planned opening (provision for void) is.
    pub openings: bool,
    /// Kinds of clashes the reviewer marked as noise ("Add to C1
    /// pre-sort"): every clash like them is suggested as C1.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub noise: Vec<NoisePattern>,
}

/// A kind of clash that is noise: the two elements, either way round, by
/// their type or their IFC class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoisePattern {
    pub by: PatternBy,
    pub a: String,
    pub b: String,
}

/// What a [`NoisePattern`] compares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PatternBy {
    /// The element type, e.g. `Basic Wall:VBE250` (the class when it has
    /// none).
    Type,
    /// The IFC class, e.g. `IfcWall`.
    Class,
}

impl NoisePattern {
    /// The pattern for clashes like the one between `a` and `b`.
    pub fn of(a: &Element, b: &Element, by: PatternBy) -> Self {
        Self {
            by,
            a: pattern_key(a, by),
            b: pattern_key(b, by),
        }
    }

    pub fn matches(&self, a: &Element, b: &Element) -> bool {
        let (ka, kb) = (pattern_key(a, self.by), pattern_key(b, self.by));
        (ka == self.a && kb == self.b) || (ka == self.b && kb == self.a)
    }

    /// The same pattern, whichever way round.
    pub fn same_as(&self, other: &NoisePattern) -> bool {
        self.by == other.by
            && ((self.a == other.a && self.b == other.b)
                || (self.a == other.b && self.b == other.a))
    }

    pub fn label(&self) -> String {
        format!("{}  ↔  {}", self.a, self.b)
    }
}

/// What a pattern compares an element by.
fn pattern_key(e: &Element, by: PatternBy) -> String {
    match by {
        PatternBy::Type => {
            let t = type_name(e);
            if t.is_empty() {
                e.occurrence_class().to_owned()
            } else {
                t
            }
        }
        PatternBy::Class => e.occurrence_class().to_owned(),
    }
}

impl Default for Presort {
    fn default() -> Self {
        Self {
            enabled: false,
            depth_mm: 20.0,
            insulation: true,
            same_element: true,
            openings: true,
            noise: Vec::new(),
        }
    }
}

impl Presort {
    /// Nothing set: left out of the rule file.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// One side of a group: what kind of element.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct GroupSide {
    /// IFC class as in IFC4, e.g. `IfcPipeSegment`.
    pub class: String,
    /// Type name, or object type, e.g. `Pipe Types:Rør - Kobber`.
    pub type_name: String,
    /// System name, e.g. `VF 3`.
    pub system: String,
}

/// Findings alike enough to be judged together.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Group {
    /// Stable for the same rule and kinds of elements.
    pub id: String,
    pub ruleset: String,
    pub rule: String,
    /// The kinds of elements; their systems are in `systems`.
    pub a: GroupSide,
    pub b: GroupSide,
    /// Systems of the elements, most findings first, with counts.
    pub systems: Vec<(String, usize)>,
    /// Storeys of the findings, most first, with counts.
    pub storeys: Vec<(String, usize)>,
    pub findings: Vec<String>,
    /// Smallest, middle and largest depth (clash) or distance, millimetres.
    pub depth_mm: [f32; 3],
}

/// The storey each element is contained in, by express id, for a model.
pub fn storey_names(model: &IfcModel) -> FxHashMap<u32, String> {
    let mut out = FxHashMap::default();
    fn walk(node: &crate::SpatialNode, current: Option<&str>, out: &mut FxHashMap<u32, String>) {
        let current = if node.is_storey() {
            Some(node.label())
        } else {
            current
        };
        if let Some(name) = current {
            for e in &node.elements {
                out.insert(*e, name.to_owned());
            }
        }
        for child in &node.children {
            walk(child, current, out);
        }
    }
    if let Some(root) = &model.spatial_root {
        walk(root, None, &mut out);
    }
    out
}

/// What the triage needs to know about the elements of a federation.
pub struct Lookup<'a> {
    federation: &'a Federation,
    storeys: FxHashMap<ModelId, FxHashMap<u32, String>>,
}

impl<'a> Lookup<'a> {
    pub fn new(federation: &'a Federation) -> Self {
        let storeys = federation
            .iter()
            .map(|(id, m)| (id, storey_names(m)))
            .collect();
        Self {
            federation,
            storeys,
        }
    }

    pub fn element(&self, e: &FindingElement) -> Option<&'a Element> {
        self.federation.get(e.model)?.element(e.express_id)
    }

    /// The storey of an element, or of the element it is part of.
    pub fn storey(&self, e: &FindingElement) -> String {
        let Some(map) = self.storeys.get(&e.model) else {
            return String::new();
        };
        let parent = self.element(e).and_then(|el| el.parent);
        map.get(&e.express_id)
            .or_else(|| parent.and_then(|p| map.get(&p)))
            .cloned()
            .unwrap_or_default()
    }

    pub fn side(&self, e: &FindingElement) -> GroupSide {
        let element = self.element(e);
        GroupSide {
            class: element.map_or_else(
                || e.ifc_class.clone(),
                |el| el.occurrence_class().to_owned(),
            ),
            type_name: element.map(type_name).unwrap_or_default(),
            system: element
                .and_then(|el| el.systems.first().cloned())
                .unwrap_or_default(),
        }
    }

    /// One line about a finding, for the triage file's examples.
    pub fn describe(&self, f: &Finding) -> String {
        let side = |e: &FindingElement| {
            let s = self.side(e);
            let mut text = s.class.clone();
            if !s.type_name.is_empty() {
                text.push_str(&format!(" \"{}\"", s.type_name));
            }
            if !s.system.is_empty() {
                text.push_str(&format!(" ({})", s.system));
            }
            text
        };
        let storey = self.storey(&f.a);
        format!(
            "{} vs {}, {:.0} mm{}",
            side(&f.a),
            side(&f.b),
            f.magnitude * 1000.0,
            if storey.is_empty() {
                String::new()
            } else {
                format!(", {storey}")
            }
        )
    }
}

/// An element's type name: its type object, or its object type.
pub fn type_name(element: &Element) -> String {
    element
        .type_object
        .as_ref()
        .map(|(_, n)| n.clone())
        .or_else(|| element.object_type.clone())
        .unwrap_or_default()
}

/// The findings of `report` that `include` lets through, in groups, largest
/// first: same rule and the same kinds of elements (class and type) on the
/// two sides. Their systems and storeys are counted in the group.
pub fn groups(
    report: &CheckReport,
    lookup: &Lookup,
    include: impl Fn(&Finding) -> bool,
) -> Vec<Group> {
    type Key = (String, String, GroupSide, GroupSide);
    let mut by_key: FxHashMap<Key, Vec<&Finding>> = FxHashMap::default();
    let mut order = Vec::new();
    for ruleset in &report.rulesets {
        for rule in &ruleset.rules {
            for f in rule.findings.iter().filter(|f| include(f)) {
                let kind = |e: &FindingElement| GroupSide {
                    system: String::new(),
                    ..lookup.side(e)
                };
                let (a, b) = (kind(&f.a), kind(&f.b));
                // The same pair the other way round is the same group.
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                let key = (ruleset.info.id.clone(), rule.rule.id.clone(), a, b);
                let list = by_key.entry(key.clone()).or_default();
                if list.is_empty() {
                    order.push(key);
                }
                list.push(f);
            }
        }
    }
    let count = |names: Vec<String>| {
        let mut counts: FxHashMap<String, usize> = FxHashMap::default();
        for n in names.into_iter().filter(|n| !n.is_empty()) {
            *counts.entry(n).or_insert(0) += 1;
        }
        let mut list: Vec<(String, usize)> = counts.into_iter().collect();
        list.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
        list
    };
    let mut out: Vec<Group> = order
        .into_iter()
        .map(|key| {
            let findings = &by_key[&key];
            let mut depths: Vec<f32> = findings.iter().map(|f| f.magnitude * 1000.0).collect();
            depths.sort_by(f32::total_cmp);
            let id = crate::project::sha256_hex(format!("{key:?}").as_bytes())[..12].to_owned();
            let systems = count(
                findings
                    .iter()
                    .flat_map(|f| {
                        let mut s = vec![lookup.side(&f.a).system];
                        if !f.is_single() {
                            s.push(lookup.side(&f.b).system);
                        }
                        s
                    })
                    .collect(),
            );
            let storeys = count(findings.iter().map(|f| lookup.storey(&f.a)).collect());
            let (ruleset, rule, a, b) = key;
            Group {
                id,
                ruleset,
                rule,
                a,
                b,
                systems,
                storeys,
                findings: findings.iter().map(|f| f.id.clone()).collect(),
                depth_mm: [
                    depths[0],
                    depths[depths.len() / 2],
                    depths[depths.len() - 1],
                ],
            }
        })
        .collect();
    out.sort_by(|x, y| {
        y.findings
            .len()
            .cmp(&x.findings.len())
            .then_with(|| x.id.cmp(&y.id))
    });
    out
}

/// Words that name a planned opening, lower case.
const OPENING_WORDS: &[&str] = &[
    "provisionforvoid",
    "provision for void",
    "provision_for_void",
    "udsparing",
    "hulafsætning",
    "hulafsaetning",
    // Revit families such as "Round Face Opening".
    "opening",
];

/// A planned opening: `IfcOpeningElement`, or an element named as one.
fn is_opening(element: &Element) -> bool {
    if element.ifc_class == "IfcOpeningElement" {
        return true;
    }
    let text = format!(
        "{} {} {}",
        element.name,
        type_name(element),
        element.predefined_type.as_deref().unwrap_or_default()
    )
    .to_lowercase();
    OPENING_WORDS.iter().any(|w| text.contains(w))
}

fn is_insulation(element: &Element) -> bool {
    let text = format!("{} {}", element.name, type_name(element)).to_lowercase();
    element.ifc_class == "IfcCovering"
        && ["insulation", "isolering", "isolation"]
            .iter()
            .any(|w| text.contains(w))
}

fn is_flow(element: &Element) -> bool {
    let class = element.occurrence_class();
    ["Segment", "Fitting"].iter().any(|s| class.ends_with(s))
        || class.starts_with("IfcFlow")
        || class.starts_with("IfcDuct")
        || class.starts_with("IfcPipe")
}

/// Suggestions from the fixed rules, for the clash findings of the rules
/// that have pre-sorting switched on (or of all clash rules with `all`,
/// each with its own settings). Findings in `skip` are left alone.
/// Rules' pre-sorting settings as they are now, by (ruleset id, rule id);
/// they may have changed since the check ran.
pub type PresortSettings = FxHashMap<(String, String), Presort>;

pub fn presort(
    report: &CheckReport,
    federation: &Federation,
    current: &PresortSettings,
    all: bool,
    skip: &FxHashSet<String>,
    now: &str,
) -> Vec<(String, Rating)> {
    let lookup = Lookup::new(federation);
    // Planned openings, in the federation frame.
    let mut openings: Vec<(Aabb, String)> = Vec::new();
    for (id, model) in federation.iter() {
        let Some(transform) = federation.model_transform(id).map(|m| m.as_dmat4()) else {
            continue;
        };
        for e in &model.elements {
            if e.has_geometry() && is_opening(e) {
                openings.push((e.bounds.transformed(&transform), e.label().to_owned()));
            }
        }
    }
    let mut out = Vec::new();
    for ruleset in &report.rulesets {
        for rule in &ruleset.rules {
            let settings = current
                .get(&(ruleset.info.id.clone(), rule.rule.id.clone()))
                .unwrap_or(&rule.rule.presort);
            if !(settings.enabled || all) {
                continue;
            }
            for f in &rule.findings {
                // An element inside another (a duct inside an opening's
                // box) is a clash too.
                let clash = matches!(f.kind, FindingKind::Clash | FindingKind::Inside);
                if !clash || skip.contains(&f.id) {
                    continue;
                }
                if let Some((confidence, reason)) = noise(f, settings, &lookup, &openings) {
                    out.push((
                        f.id.clone(),
                        Rating {
                            category: Category::C1,
                            noise: true,
                            confidence,
                            reason,
                            source: Source::Presort,
                            approved: None,
                            group: String::new(),
                            image: false,
                            at: now.to_owned(),
                        },
                    ));
                }
            }
        }
    }
    out
}

/// Why a clash is noise by the fixed rules, if it is, and how sure that is.
fn noise(
    f: &Finding,
    settings: &Presort,
    lookup: &Lookup,
    openings: &[(Aabb, String)],
) -> Option<(f32, String)> {
    if settings.same_element && !f.is_single() && f.a.global_id == f.b.global_id {
        return Some((
            0.95,
            "The same element in two models (same GlobalId).".to_owned(),
        ));
    }
    let (ea, eb) = (lookup.element(&f.a)?, lookup.element(&f.b)?);
    // Kinds the reviewer marked as noise.
    if let Some(p) = settings.noise.iter().find(|p| p.matches(ea, eb)) {
        return Some((
            0.9,
            format!("Like the clashes you marked as noise: {}.", p.label()),
        ));
    }
    // One of them is the planned opening itself (a void family modelled as
    // an element): what runs through it is meant to.
    if settings.openings
        && let Some(opening) = [ea, eb].into_iter().find(|e| is_opening(e))
    {
        return Some((
            0.95,
            format!(
                "A planned opening ({}), not a built element.",
                opening.label()
            ),
        ));
    }
    if settings.insulation {
        for (cover, other) in [(ea, eb), (eb, ea)] {
            if !is_insulation(cover) || !is_flow(other) {
                continue;
            }
            let same_system = !cover.systems.is_empty()
                && cover.systems.iter().any(|s| other.systems.contains(s));
            // Insulation wraps its own pipe: the pipe's box lies inside it.
            let margin = glam::Vec3::splat(0.01);
            let inside = other.bounds.min.cmpge(cover.bounds.min - margin).all()
                && other.bounds.max.cmple(cover.bounds.max + margin).all();
            if same_system || inside {
                return Some((
                    0.9,
                    format!(
                        "Insulation around its own {}.",
                        if other.occurrence_class().contains("Duct") {
                            "duct"
                        } else {
                            "pipe"
                        }
                    ),
                ));
            }
        }
    }
    let depth = f.magnitude * 1000.0;
    if settings.depth_mm > 0.0 && depth < settings.depth_mm {
        return Some((
            0.85,
            format!(
                "Overlap of {:.0} mm, under the {:.0} mm limit.",
                // Rounded down: 19.6 under a 20 mm limit reads as 19.
                depth.floor(),
                settings.depth_mm
            ),
        ));
    }
    if settings.openings {
        let margin = glam::Vec3::splat(0.02);
        if let Some((_, name)) = openings.iter().find(|(b, _)| {
            f.point.cmpge(b.min - margin).all() && f.point.cmple(b.max + margin).all()
        }) {
            return Some((0.8, format!("A penetration at a planned opening ({name}).")));
        }
    }
    None
}

/// The triage skill as a zip file for Claude Desktop: a folder
/// `opencheck-triage` holding `SKILL.md`.
pub fn skill_zip(skill: &str) -> Vec<u8> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // Writing to memory does not fail.
        let _ = zip.start_file("opencheck-triage/SKILL.md", options);
        let _ = zip.write_all(skill.as_bytes());
        let _ = zip.finish();
    }
    buffer.into_inner()
}

/// A project's own triage rules and the reviewer's corrections, read by
/// Claude before every review.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TriageFile {
    /// The project's rules in plain words, e.g. "Insulation against pipes
    /// in shafts is always C1".
    pub rules: String,
    pub examples: Vec<TriageExample>,
}

/// A finding the reviewer put in another category than suggested.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TriageExample {
    /// The finding in one line: classes, types, systems, depth, storey.
    pub finding: String,
    pub suggested: Category,
    pub category: Category,
    /// The suggestion's reason, for comparison.
    #[serde(default)]
    pub reason: String,
}

impl TriageFile {
    pub fn is_empty(&self) -> bool {
        self.rules.trim().is_empty() && self.examples.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_are_priorities() {
        assert_eq!(Category::C3.severity(), Severity::Critical);
        assert_eq!(Category::parse(" c2 "), Some(Category::C2));
        assert_eq!(Category::parse("C4"), None);
        let rating = Rating {
            category: Category::C1,
            noise: true,
            confidence: 0.9,
            reason: "x".to_owned(),
            source: Source::Claude,
            approved: None,
            group: String::new(),
            image: false,
            at: String::new(),
        };
        let json = serde_json::to_string(&rating).unwrap();
        assert!(json.contains("\"category\":\"C1\""));
        assert_eq!(serde_json::from_str::<Rating>(&json).unwrap(), rating);
        assert!(rating.is_proposed());
    }

    #[test]
    fn presort_defaults_stay_out_of_rule_files() {
        assert!(Presort::default().is_default());
        assert!(!Presort::default().enabled);
    }

    #[test]
    fn noise_patterns_are_kept_in_rule_files() {
        let mut presort = Presort::default();
        presort.noise.push(NoisePattern {
            by: PatternBy::Type,
            a: "Basic Wall:VBE250".into(),
            b: "Floor:DBEM220".into(),
        });
        let text = toml::to_string(&presort).unwrap();
        assert!(text.contains("by = \"type\""), "{text}");
        let back: Presort = toml::from_str(&text).unwrap();
        assert_eq!(back, presort);
        let swapped = NoisePattern {
            by: PatternBy::Type,
            a: "Floor:DBEM220".into(),
            b: "Basic Wall:VBE250".into(),
        };
        assert!(swapped.same_as(&presort.noise[0]));
    }
}
