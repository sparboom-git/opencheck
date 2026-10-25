//! Visual checking: rules that colour elements in the 3D view, so a pattern
//! (or a break in it) shows at a glance. Each rule sorts the elements it
//! looks at into coloured categories (groups, systems, storeys) and lists
//! the problems it finds, which also become findings for autorun.
//!
//! - [`alignment`]: walls and columns standing in line from storey to storey.
//! - [`dimension`]: pipes and ducts that get larger again away from the
//!   start of their system.
//! - [`slope`]: the fall of drain pipes.
//! - [`storey`]: elements standing on another storey than they belong to.

mod alignment;
mod dimension;
mod network;
mod slope;
mod storey;

use glam::{DMat4, DVec3, Vec3};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::{Deserialize, Serialize};

pub use alignment::{
    AlignElement, AlignMode, Alignment, AlignmentRule, Member, align, member_kind,
};
pub use dimension::DimensionRule;
pub use slope::{DrainSystem, SlopeLimit, SlopeRule, drain_systems};
pub use storey::StoreyRule;

use crate::check::Severity;
use crate::{Element, IfcModel, ModelId};

/// Problems that need fixing: out of line, a jump in dimension, a drain
/// falling the wrong way, a wrong storey.
pub const ERROR: [u8; 3] = [214, 39, 40];
const OUT_OF_LINE: [u8; 3] = ERROR;
/// Not quite right: a drain with too little fall.
pub const WARNING: [u8; 3] = [232, 178, 30];
/// Fine: a drain with enough fall.
pub const GOOD: [u8; 3] = [86, 160, 92];
/// Nothing to judge: no element above or below, vertical pipes, fittings.
pub const NEUTRAL: [u8; 3] = [196, 192, 184];
/// Missing data, such as an element without a storey.
pub const MISSING: [u8; 3] = [190, 70, 200];

/// One visual rule as kept in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualRule {
    pub name: String,
    /// Runs with autorun; switched off rules are kept but not run there.
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(flatten)]
    pub kind: VisualKind,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VisualKind {
    Alignment(AlignmentRule),
    Dimensions(DimensionRule),
    DrainSlope(SlopeRule),
    Storeys(StoreyRule),
}

impl VisualRule {
    fn new(name: &str, kind: VisualKind) -> Self {
        Self {
            name: name.to_owned(),
            enabled: true,
            kind,
        }
    }

    /// Alignment of walls and columns in the structural models.
    pub fn alignment() -> Self {
        Self::new(
            "Alignment of walls and columns",
            VisualKind::Alignment(AlignmentRule::default()),
        )
    }

    /// Dimensions along pipe and duct systems.
    pub fn dimensions() -> Self {
        Self::new(
            "Dimensions in pipe and duct systems",
            VisualKind::Dimensions(DimensionRule::default()),
        )
    }

    /// Fall of drain pipes.
    pub fn drain_slope() -> Self {
        Self::new(
            "Fall of drain pipes",
            VisualKind::DrainSlope(SlopeRule::default()),
        )
    }

    /// Elements on the storey they belong to.
    pub fn storeys() -> Self {
        Self::new("Storeys", VisualKind::Storeys(StoreyRule::default()))
    }

    /// Every kind of rule, new, in the order the Add menu shows them.
    pub fn all_kinds() -> Vec<Self> {
        vec![
            Self::alignment(),
            Self::dimensions(),
            Self::drain_slope(),
            Self::storeys(),
        ]
    }
}

impl VisualKind {
    /// The check type its findings count under in autorun and the dashboard.
    pub fn check_type(&self) -> &'static str {
        match self {
            VisualKind::Alignment(_) => "alignment",
            VisualKind::Dimensions(_) => "dimension_jump",
            VisualKind::DrainSlope(_) => "drain_slope",
            VisualKind::Storeys(_) => "storey_mismatch",
        }
    }

    /// What the rule does, for menus and tooltips.
    pub fn description(&self) -> &'static str {
        match self {
            VisualKind::Alignment(_) => {
                "Colours walls and columns that stand in line with each other, from storey to storey"
            }
            VisualKind::Dimensions(_) => {
                "Colours pipe and duct systems and marks where a dimension gets larger again away from the start"
            }
            VisualKind::DrainSlope(_) => {
                "Colours drain pipes by their fall and marks flat pipes and pipes falling the wrong way"
            }
            VisualKind::Storeys(_) => {
                "Colours elements by storey and marks those standing on another storey than they belong to"
            }
        }
    }
}

/// Identifies one element across the models.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementKey {
    pub model: ModelId,
    pub express_id: u32,
}

/// A model as the visual checks see it.
pub struct VisualModel<'a> {
    pub id: ModelId,
    pub model: &'a IfcModel,
    /// From the model's mesh frame into one frame shared by all models, in
    /// metres with z up (the federation frame).
    pub transform: DMat4,
    /// Elements taking part, by express id; None means all.
    pub include: Option<&'a FxHashSet<u32>>,
}

impl VisualModel<'_> {
    pub fn includes(&self, express_id: u32) -> bool {
        self.include.is_none_or(|s| s.contains(&express_id))
    }

    pub fn key(&self, express_id: u32) -> ElementKey {
        ElementKey {
            model: self.id,
            express_id,
        }
    }

    /// The element's vertices in the shared frame.
    pub fn points<'b>(&'b self, element: &'b Element) -> impl Iterator<Item = DVec3> + 'b {
        element.meshes.iter().flat_map(move |&m| {
            self.model.meshes[m].positions.iter().map(move |p| {
                self.transform
                    .transform_point3(DVec3::from(p.map(f64::from)))
            })
        })
    }

    /// The element's triangles in the shared frame.
    pub fn triangles(&self, element: &Element) -> Vec<[Vec3; 3]> {
        let transform = self.transform.as_mat4();
        let mut out = Vec::new();
        for &m in &element.meshes {
            let mesh = &self.model.meshes[m];
            let point =
                |i: u32| transform.transform_point3(Vec3::from_array(mesh.positions[i as usize]));
            for t in mesh.indices.as_chunks::<3>().0 {
                out.push([point(t[0]), point(t[1]), point(t[2])]);
            }
        }
        out
    }
}

/// Elements sharing a colour: a group, a system, a storey.
#[derive(Debug, Clone, PartialEq)]
pub struct Category {
    pub label: String,
    pub detail: String,
    pub color: [u8; 3],
    pub elements: Vec<ElementKey>,
}

/// Something to fix, about its first element; a second one, when given,
/// is the element it was compared with.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    pub elements: Vec<ElementKey>,
    pub severity: Severity,
    /// What is wrong, e.g. `Ø160 after Ø110` or `Assigned to E2, stands on E1`.
    pub text: String,
    /// The measured value, if any: dimension in mm, fall in ‰, offset in mm.
    pub value: Option<f64>,
    pub color: [u8; 3],
}

/// What a visual rule found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisualOutcome {
    /// One line, e.g. `12 systems (1,830 elements)`.
    pub summary: String,
    /// The colour legend; every coloured element is in one category or
    /// one problem.
    pub categories: Vec<Category>,
    /// Heading and explanation of the problem list.
    pub problem_title: String,
    pub problem_hint: String,
    pub problems: Vec<Problem>,
    /// Elements left out for lack of geometry.
    pub skipped: usize,
    /// How the check went about it, e.g. which systems had no start point.
    pub notes: Vec<String>,
}

impl VisualOutcome {
    /// The colour of every element: its category's, or its problem's.
    pub fn colors(&self) -> FxHashMap<ElementKey, [u8; 3]> {
        let mut colors = FxHashMap::default();
        for c in &self.categories {
            for e in &c.elements {
                colors.insert(*e, c.color);
            }
        }
        for p in &self.problems {
            if let Some(e) = p.elements.first() {
                colors.insert(*e, p.color);
            }
        }
        colors
    }

    /// Number of elements that got a colour.
    pub fn coloured(&self) -> usize {
        self.colors().len()
    }
}

/// Runs a rule on the models.
pub fn run(rule: &VisualRule, models: &[VisualModel]) -> VisualOutcome {
    match &rule.kind {
        VisualKind::Alignment(r) => alignment::run(r, models),
        VisualKind::Dimensions(r) => dimension::run(r, models),
        VisualKind::DrainSlope(r) => slope::run(r, models),
        VisualKind::Storeys(r) => storey::run(r, models),
    }
}

/// A colour per group: hues spread by the golden ratio, leaving out the
/// reds and yellows that mark problems.
pub fn palette(index: usize) -> [u8; 3] {
    let mut hue = (0.13 + index as f32 * 0.618_034).fract();
    if !(0.18..=0.92).contains(&hue) {
        hue = (hue + 0.26).fract();
    }
    let value = if index.is_multiple_of(2) { 0.86 } else { 0.68 };
    let saturation = if index % 3 == 2 { 0.45 } else { 0.62 };
    hsv(hue, saturation, value)
}

/// A muted colour per index, for systems and storeys.
pub fn muted(index: usize) -> [u8; 3] {
    let hue = (0.55 + index as f32 * 0.618_034).fract();
    let hue = if (0.0..0.06).contains(&hue) || hue > 0.95 {
        (hue + 0.12).fract()
    } else {
        hue
    };
    let value = if index.is_multiple_of(2) { 0.78 } else { 0.64 };
    hsv(hue, 0.38, value)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_round_trip() {
        for rule in VisualRule::all_kinds() {
            let json = serde_json::to_string(&rule).unwrap();
            assert!(json.contains("\"type\":"));
            assert_eq!(serde_json::from_str::<VisualRule>(&json).unwrap(), rule);
        }
        // Rules saved before `enabled` existed are on.
        let old = r#"{"name":"A","type":"alignment","mode":"face"}"#;
        let rule: VisualRule = serde_json::from_str(old).unwrap();
        assert!(rule.enabled);
        assert!(matches!(
            rule.kind,
            VisualKind::Alignment(AlignmentRule {
                mode: AlignMode::Face,
                ..
            })
        ));
        assert!(AlignmentRule::default().includes_class("IfcWallStandardCase"));
        assert!(!AlignmentRule::default().includes_class("IfcWallType"));
    }

    #[test]
    fn palettes_keep_clear_of_the_problem_colours() {
        for i in 0..40 {
            assert_ne!(palette(i), ERROR);
            assert_ne!(muted(i), ERROR);
        }
    }
}
