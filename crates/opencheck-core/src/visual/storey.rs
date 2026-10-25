//! Storeys: each element is coloured by the storey it belongs to
//! (`IfcRelContainedInSpatialStructure`), and marked when it stands on
//! another storey, judged by its underside. A storey reaches from its own
//! height up to the next storey; an element may hang a little below its
//! storey (a slab, a beam under the deck) by the tolerance.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::{Category, ERROR, MISSING, Problem, VisualModel, VisualOutcome, muted};
use crate::check::Severity;
use crate::{Discipline, IfcModel, SpatialNode};

/// Settings of the storey check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreyRule {
    /// Models of these disciplines are looked at.
    pub disciplines: Vec<Discipline>,
    /// How far an element may reach below its storey, millimetres.
    pub tolerance_mm: f64,
    /// IFC classes that may stand anywhere, e.g. `IfcBeam` for beams
    /// modelled on the storey above.
    pub exempt: Vec<String>,
}

impl Default for StoreyRule {
    fn default() -> Self {
        Self {
            disciplines: Discipline::ALL.to_vec(),
            tolerance_mm: 500.0,
            exempt: Vec::new(),
        }
    }
}

/// Classes that are no physical element on a storey.
const LEFT_OUT: &[&str] = &[
    "IfcSpace",
    "IfcSpatialZone",
    "IfcZone",
    "IfcOpeningElement",
    "IfcOpeningStandardCase",
    "IfcVirtualElement",
    "IfcAnnotation",
    "IfcGrid",
];

struct Storey {
    name: String,
    /// World height of its floor, metres.
    z: f64,
}

/// The model's storeys from low to high, and the storey each spatial node
/// and contained element belongs to (index into the storeys).
fn storeys(model: &IfcModel) -> (Vec<Storey>, BTreeMap<u32, usize>) {
    let mut list = Vec::new();
    let mut of: BTreeMap<u32, usize> = BTreeMap::new();
    let Some(root) = &model.spatial_root else {
        return (list, of);
    };
    let scale = model.length_unit_scale;
    fn walk(
        node: &SpatialNode,
        current: Option<usize>,
        scale: f64,
        list: &mut Vec<Storey>,
        of: &mut BTreeMap<u32, usize>,
    ) {
        let mut current = current;
        if node.is_storey()
            && let Some(z) = node.placement_z.or(node.elevation)
        {
            list.push(Storey {
                name: node.label().to_owned(),
                z: z * scale,
            });
            current = Some(list.len() - 1);
        }
        if let Some(c) = current {
            of.insert(node.express_id, c);
            for e in &node.elements {
                of.insert(*e, c);
            }
        }
        for child in &node.children {
            walk(child, current, scale, list, of);
        }
    }
    walk(root, None, scale, &mut list, &mut of);
    // Sort low to high and renumber.
    let mut order: Vec<usize> = (0..list.len()).collect();
    order.sort_by(|a, b| list[*a].z.total_cmp(&list[*b].z));
    let mut rank = vec![0; list.len()];
    for (r, &i) in order.iter().enumerate() {
        rank[i] = r;
    }
    for v in of.values_mut() {
        *v = rank[*v];
    }
    let mut sorted: Vec<Option<Storey>> = list.into_iter().map(Some).collect();
    let list = order.iter().filter_map(|&i| sorted[i].take()).collect();
    (list, of)
}

pub(super) fn run(rule: &StoreyRule, models: &[VisualModel]) -> VisualOutcome {
    let tolerance = rule.tolerance_mm.max(0.0) / 1000.0;
    let mut outcome = VisualOutcome::default();
    // Elements per storey name, with the storey's height for the order.
    let mut by_name: BTreeMap<String, (f64, Vec<super::ElementKey>)> = BTreeMap::new();
    let mut checked = 0;
    for m in models
        .iter()
        .filter(|m| rule.disciplines.contains(&m.model.discipline))
    {
        let model = m.model;
        let (list, of) = storeys(model);
        if list.is_empty() {
            outcome.notes.push(format!(
                "{} has no storeys with a height.",
                model.short_name
            ));
            continue;
        }
        for e in &model.elements {
            if !e.has_geometry()
                || !m.includes(e.express_id)
                || LEFT_OUT.contains(&e.ifc_class.as_str())
            {
                continue;
            }
            checked += 1;
            let key = m.key(e.express_id);
            // Parts belong where their whole belongs.
            let assigned = of
                .get(&e.express_id)
                .or_else(|| e.parent.and_then(|p| of.get(&p)))
                .copied();
            let Some(assigned) = assigned else {
                outcome.problems.push(Problem {
                    elements: vec![key],
                    severity: Severity::Low,
                    text: "Belongs to no storey".to_owned(),
                    value: None,
                    color: MISSING,
                });
                continue;
            };
            let bottom = e.bounds.transformed(&model.local_to_world).min.z as f64;
            let storey = &list[assigned];
            let floor_ok = assigned == 0 || bottom >= storey.z - tolerance;
            let ceiling_ok = list
                .get(assigned + 1)
                .is_none_or(|next| bottom < next.z - 0.001);
            let exempt = rule.exempt.iter().any(|c| c.trim() == e.ifc_class);
            if floor_ok && ceiling_ok || exempt {
                by_name
                    .entry(storey.name.clone())
                    .or_insert((storey.z, Vec::new()))
                    .1
                    .push(key);
                continue;
            }
            let stands = list
                .iter()
                .rposition(|s| s.z <= bottom + 0.001)
                .unwrap_or(0);
            outcome.problems.push(Problem {
                elements: vec![key],
                severity: Severity::Moderate,
                text: format!("On {}, stands on {}", storey.name, list[stands].name),
                value: Some(((bottom - storey.z) * 1000.0).round()),
                color: ERROR,
            });
        }
    }

    let mut names: Vec<(String, (f64, Vec<super::ElementKey>))> = by_name.into_iter().collect();
    names.sort_by(|a, b| a.1.0.total_cmp(&b.1.0));
    for (i, (name, (_, elements))) in names.into_iter().enumerate() {
        outcome.categories.push(Category {
            label: name,
            detail: format!("{} elements", elements.len()),
            color: muted(i),
            elements,
        });
    }
    let wrong = outcome.problems.iter().filter(|p| p.color == ERROR).count();
    let missing = outcome.problems.len() - wrong;
    outcome.summary = format!("{} storeys ({checked} elements)", outcome.categories.len());
    outcome.problem_title = "Wrong storey".to_owned();
    outcome.problem_hint = format!(
        "Red: stands on another storey ({wrong}). Purple: belongs to no storey ({missing})."
    );
    outcome
}
