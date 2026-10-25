//! Dimensions along pipe and duct systems. A system usually starts large at
//! the shaft or the air handling unit and gets smaller towards the
//! consumers; a piece that is larger than the one before it, away from the
//! start, is most likely a modelling error.
//!
//! The start of each connected part of a system is its equipment (air
//! handling unit, fan, pump, boiler) or, without equipment, its largest
//! segments. When the largest segments lie in more than one place, the
//! direction is not clear; then every piece smaller than the pieces on both
//! sides of it along a run is marked instead.

use serde::{Deserialize, Serialize};

use super::network::{self, Medium, Net, Role, system_color};
use super::{Category, ERROR, Problem, VisualModel, VisualOutcome};
use crate::Discipline;
use crate::check::Severity;

/// Settings of the dimension check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DimensionRule {
    /// Models of these disciplines are looked at.
    pub disciplines: Vec<Discipline>,
    pub pipes: bool,
    pub ducts: bool,
    /// Larger than this, in percent of the size (the diameter, or the
    /// side of a square of the same area), counts as a jump.
    pub tolerance_percent: f64,
    /// Loose elements without ports are connected where their ends lie
    /// within this many millimetres.
    pub connect_mm: f64,
}

impl Default for DimensionRule {
    fn default() -> Self {
        Self {
            disciplines: vec![
                Discipline::Mechanical,
                Discipline::Plumbing,
                Discipline::FireProtection,
            ],
            pipes: true,
            ducts: true,
            tolerance_percent: 5.0,
            connect_mm: 10.0,
        }
    }
}

pub(super) fn run(rule: &DimensionRule, models: &[VisualModel]) -> VisualOutcome {
    let net = network::build(models, &rule.disciplines, rule.connect_mm / 1000.0);
    let taking_part = |n: usize| match net.nodes[n].medium {
        Medium::Pipe => rule.pipes,
        Medium::Duct => rule.ducts,
        Medium::Unknown => true,
    };
    // Rectangular ducts are compared by area: 400×200 and 300×300 are
    // about the same size.
    let size = |n: usize| {
        net.nodes[n]
            .run
            .filter(|r| taking_part(n) && r.measurable())
            .map(|r| r.area.sqrt())
    };
    let factor = 1.0 + rule.tolerance_percent.max(0.0) / 100.0;
    let label = |n: usize| net.nodes[n].run.map(|r| r.size_label()).unwrap_or_default();

    let mut outcome = VisualOutcome::default();
    let mut from_equipment = 0;
    let mut from_largest = 0;
    let mut unclear = 0;
    for component in net.components(|a, b| net.same_system(a, b)) {
        let sized: Vec<usize> = component
            .iter()
            .copied()
            .filter(|&n| size(n).is_some())
            .collect();
        if sized.is_empty() {
            continue;
        }
        let system = &net.nodes[component[0]].system;
        // Equipment in the part, or next to it in another system.
        let mut starts: Vec<usize> = component
            .iter()
            .copied()
            .filter(|&n| {
                net.nodes[n].role == Role::Equipment
                    || net.adjacent[n]
                        .iter()
                        .any(|&m| net.nodes[m].role == Role::Equipment)
            })
            .collect();
        if starts.is_empty() {
            let largest = sized
                .iter()
                .map(|&n| size(n).unwrap_or(0.0))
                .fold(0.0, f64::max);
            starts = sized
                .iter()
                .copied()
                .filter(|&n| size(n).unwrap_or(0.0) * factor >= largest)
                .collect();
            if clusters(&net, &starts, |n| size(n).is_some()) > 1 {
                unclear += 1;
                for (s, larger) in dips(&net, &sized, &size, factor) {
                    outcome.problems.push(Problem {
                        elements: vec![net.nodes[s].key, net.nodes[larger].key],
                        severity: Severity::Moderate,
                        text: format!(
                            "{} between larger pieces ({}) in {system}",
                            label(s),
                            label(larger)
                        ),
                        value: net.nodes[s].run.map(|r| r.size_mm()),
                        color: ERROR,
                    });
                }
                continue;
            }
            from_largest += 1;
        } else {
            from_equipment += 1;
        }

        // Out from the start: each sized piece against the last sized
        // piece before it.
        let mut before: Vec<Option<usize>> = vec![None; net.nodes.len()];
        let mut seen = vec![false; net.nodes.len()];
        let mut queue = std::collections::VecDeque::new();
        for &s in &starts {
            seen[s] = true;
            queue.push_back(s);
        }
        while let Some(n) = queue.pop_front() {
            let last = if size(n).is_some() {
                Some(n)
            } else {
                before[n]
            };
            for &m in &net.adjacent[n] {
                if seen[m] || !net.same_system(n, m) {
                    continue;
                }
                seen[m] = true;
                before[m] = last;
                if let (Some(sm), Some(up)) = (size(m), last)
                    && sm > size(up).unwrap_or(f64::MAX) * factor
                {
                    outcome.problems.push(Problem {
                        elements: vec![net.nodes[m].key, net.nodes[up].key],
                        severity: Severity::Moderate,
                        text: format!("{} after {} in {system}", label(m), label(up)),
                        value: net.nodes[m].run.map(|r| r.size_mm()),
                        color: ERROR,
                    });
                }
                queue.push_back(m);
            }
        }
    }

    // A colour per kind of system.
    let mut kinds: Vec<(String, Vec<usize>)> = Vec::new();
    for n in 0..net.nodes.len() {
        if !taking_part(n) {
            continue;
        }
        let kind = &net.nodes[n].kind;
        match kinds.iter_mut().find(|(k, _)| k == kind) {
            Some((_, list)) => list.push(n),
            None => kinds.push((kind.clone(), vec![n])),
        }
    }
    kinds.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    let mut systems = std::collections::BTreeSet::new();
    for (i, (kind, list)) in kinds.iter().enumerate() {
        let names: std::collections::BTreeSet<&str> =
            list.iter().map(|&n| net.nodes[n].system.as_str()).collect();
        systems.extend(names.iter().map(|s| s.to_string()));
        outcome.categories.push(Category {
            label: kind.clone(),
            detail: if names.len() == 1 {
                format!("{} elements", list.len())
            } else {
                format!("{} systems, {} elements", names.len(), list.len())
            },
            color: system_color(kind, i),
            elements: list.iter().map(|&n| net.nodes[n].key).collect(),
        });
    }
    outcome.summary = format!(
        "{} systems ({} elements)",
        systems.len(),
        kinds.iter().map(|(_, l)| l.len()).sum::<usize>()
    );
    outcome.problem_title = "Dimension jumps".to_owned();
    outcome.problem_hint =
        "Larger than the piece before it, seen from the start of the system.".to_owned();
    outcome.notes.push(format!(
        "{} connections from ports, {} from geometry.",
        net.by_ports, net.by_geometry
    ));
    if from_equipment + from_largest > 0 {
        outcome.notes.push(format!(
            "Started from equipment in {from_equipment} and from the largest pieces in {from_largest} parts of systems."
        ));
    }
    if unclear > 0 {
        outcome.notes.push(format!(
            "{unclear} parts of systems have their largest pieces in more than one place: there, pieces smaller than both neighbours are marked."
        ));
    }
    if net.nodes.is_empty() {
        outcome
            .notes
            .push("No pipes or ducts in the models of these disciplines.".to_owned());
    }
    outcome
}

/// How many separate places the nodes `of` lie in, connected through each
/// other and through unsized nodes (fittings, valves).
fn clusters(net: &Net, of: &[usize], sized: impl Fn(usize) -> bool) -> usize {
    let wanted: std::collections::HashSet<usize> = of.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();
    let mut count = 0;
    for &start in of {
        if !seen.insert(start) {
            continue;
        }
        count += 1;
        let mut stack = vec![start];
        while let Some(n) = stack.pop() {
            for &m in &net.adjacent[n] {
                if net.same_system(n, m) && (wanted.contains(&m) || !sized(m)) && seen.insert(m) {
                    stack.push(m);
                }
            }
        }
    }
    count
}

/// Pieces smaller than the pieces on both sides of them along a run: the
/// neighbours reached through bends and other fittings with two
/// connections, not through tees. Each with one of the larger neighbours.
fn dips(
    net: &Net,
    sized: &[usize],
    size: &impl Fn(usize) -> Option<f64>,
    factor: f64,
) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for &s in sized {
        let own = size(s).unwrap_or(0.0);
        let mut larger = Vec::new();
        for &first in &net.adjacent[s] {
            let (mut previous, mut n) = (s, first);
            let mut steps = 0;
            // Follow the run to the next sized piece.
            let found = loop {
                steps += 1;
                if steps > 64 || n == s || !net.same_system(previous, n) {
                    break None;
                }
                if size(n).is_some() {
                    break Some(n);
                }
                let next: Vec<usize> = net.adjacent[n]
                    .iter()
                    .copied()
                    .filter(|&m| m != previous && net.same_system(n, m))
                    .collect();
                if next.len() != 1 {
                    break None;
                }
                (previous, n) = (n, next[0]);
            };
            if let Some(f) = found
                && size(f).unwrap_or(0.0) > own * factor
            {
                larger.push(f);
            }
        }
        if larger.len() >= 2 {
            out.push((s, larger[0]));
        }
    }
    out
}
