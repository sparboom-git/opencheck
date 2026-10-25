//! The fall of drain pipes. Each horizontal drain pipe is coloured by its
//! fall towards the stack or outlet: enough, too little, or none or the
//! wrong way. Other pipes (heating, water) may well rise and are left out.
//!
//! Drain systems are found from their kind (`IfcSystem` description or
//! predefined type, e.g. `Afløb, Spildevand`, DRAINAGE) and name, by a list
//! of words; single systems can be set by hand. The direction of flow is
//! towards the nearest stack (a vertical drain pipe) or, without one, the
//! lowest point of the connected drain pipes: drains are trees running
//! down to their outlet, so the way through the pipes towards the lowest
//! point is downstream.

use std::collections::{BTreeMap, VecDeque};

use glam::DVec3;
use serde::{Deserialize, Serialize};

use super::network::{self, Medium, Role};
use super::{Category, ERROR, GOOD, NEUTRAL, Problem, VisualModel, VisualOutcome, WARNING};
use crate::Discipline;
use crate::check::Severity;

/// Settings of the drain fall check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlopeRule {
    /// Models of these disciplines are looked at.
    pub disciplines: Vec<Discipline>,
    /// Words in a system's name or kind that make it a drain, any case.
    pub words: Vec<String>,
    /// Systems set by hand, by name: drains, and not drains.
    pub drains: Vec<String>,
    pub not_drains: Vec<String>,
    /// Least fall, per mille, where no limit by size applies.
    pub min_permille: f64,
    /// Least fall by pipe size, the first that fits.
    pub limits: Vec<SlopeLimit>,
    /// Falls within this many per mille of a limit count as meeting it; a
    /// pipe falling less is flat.
    pub tolerance_permille: f64,
    /// Loose pipes without ports are connected where their ends lie within
    /// this many millimetres.
    pub connect_mm: f64,
}

/// Least fall for pipes up to a size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlopeLimit {
    /// Outer diameter, millimetres.
    pub up_to_mm: f64,
    pub min_permille: f64,
}

impl Default for SlopeRule {
    fn default() -> Self {
        Self {
            disciplines: vec![Discipline::Plumbing, Discipline::Mechanical],
            words: [
                "afløb",
                "spildevand",
                "regnvand",
                "kloak",
                "dræn",
                "sanitary",
                "drain",
                "waste",
                "sewage",
                "storm",
                "soil",
            ]
            .map(str::to_owned)
            .to_vec(),
            drains: Vec::new(),
            not_drains: Vec::new(),
            min_permille: 10.0,
            limits: Vec::new(),
            tolerance_permille: 0.5,
            connect_mm: 10.0,
        }
    }
}

impl SlopeRule {
    /// Whether a system is a drain, and whether that was set by hand.
    pub fn classify(&self, name: &str, kind: &str) -> (bool, bool) {
        if self.drains.iter().any(|d| d == name) {
            return (true, true);
        }
        if self.not_drains.iter().any(|d| d == name) {
            return (false, true);
        }
        let text = format!("{name} {kind}").to_lowercase();
        let drain = self
            .words
            .iter()
            .map(|w| w.trim().to_lowercase())
            .any(|w| !w.is_empty() && text.contains(&w));
        (drain, false)
    }

    /// The least fall for a pipe of this outer diameter, per mille.
    pub fn min_for(&self, size_mm: f64) -> f64 {
        let mut limits = self.limits.clone();
        limits.sort_by(|a, b| a.up_to_mm.total_cmp(&b.up_to_mm));
        limits
            .iter()
            .find(|l| size_mm <= l.up_to_mm + 0.5)
            .map_or(self.min_permille, |l| l.min_permille)
    }
}

/// A pipe system in the models and how the check sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrainSystem {
    pub name: String,
    pub kind: String,
    pub drain: bool,
    pub by_hand: bool,
    pub pipes: usize,
}

/// The pipe systems of the rule's models, drains first, so they can be
/// shown and set by hand before the check runs.
pub fn drain_systems(rule: &SlopeRule, models: &[VisualModel]) -> Vec<DrainSystem> {
    let mut found: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for m in models
        .iter()
        .filter(|m| rule.disciplines.contains(&m.model.discipline))
    {
        for e in &m.model.elements {
            let class = e.occurrence_class();
            if class != "IfcPipeSegment" && class != "IfcFlowSegment" {
                continue;
            }
            let Some(name) = e.systems.first() else {
                continue;
            };
            let kind = m.model.system_info.get(name).cloned().unwrap_or_default();
            found.entry(name.clone()).or_insert((kind, 0)).1 += 1;
        }
    }
    let mut list: Vec<DrainSystem> = found
        .into_iter()
        .map(|(name, (kind, pipes))| {
            let (drain, by_hand) = rule.classify(&name, &kind);
            DrainSystem {
                name,
                kind,
                drain,
                by_hand,
                pipes,
            }
        })
        .collect();
    list.sort_by(|a, b| b.drain.cmp(&a.drain).then(a.name.cmp(&b.name)));
    list
}

pub(super) fn run(rule: &SlopeRule, models: &[VisualModel]) -> VisualOutcome {
    let net = network::build(models, &rule.disciplines, rule.connect_mm / 1000.0);
    let drain: Vec<bool> = net
        .nodes
        .iter()
        .map(|n| n.medium != Medium::Duct && rule.classify(&n.system, &n.kind).0)
        .collect();
    // Stacks: drain pipes steeper than 45°.
    let vertical = |n: usize| {
        net.nodes[n]
            .run
            .is_some_and(|r| r.horizontal_length() < r.rise().abs() + 1e-6)
    };

    // Distance through the pipes to the lowest element of each connected
    // part of the drains.
    let mut distance = vec![usize::MAX; net.nodes.len()];
    let mut queue = VecDeque::new();
    let parts = net.components(|a, b| drain[a] && drain[b]);
    for part in parts.iter().filter(|p| drain[p[0]]) {
        if let Some(low) = part.iter().copied().min_by(|&a, &b| {
            net.nodes[a]
                .bounds
                .min
                .z
                .total_cmp(&net.nodes[b].bounds.min.z)
        }) {
            distance[low] = 0;
            queue.push_back(low);
        }
    }
    while let Some(n) = queue.pop_front() {
        for &m in &net.adjacent[n] {
            if drain[m] && distance[m] == usize::MAX {
                distance[m] = distance[n] + 1;
                queue.push_back(m);
            }
        }
    }

    let mut outcome = VisualOutcome::default();
    let mut good = Vec::new();
    let mut neutral = Vec::new();
    let mut undirected = 0;
    let mut pipes = 0;
    let mut systems = std::collections::BTreeSet::new();
    let tolerance = rule.tolerance_permille.max(0.0);
    for (i, node) in net.nodes.iter().enumerate() {
        if !drain[i] {
            continue;
        }
        systems.insert(node.system.as_str());
        let Some(run) = node
            .run
            .filter(|_| node.role == Role::Segment && !vertical(i))
        else {
            neutral.push(node.key);
            continue;
        };
        pipes += 1;
        let length = run.horizontal_length();
        if length < 1e-3 {
            neutral.push(node.key);
            continue;
        }
        // The end towards the stack, if the pipes tell.
        let downstream = net.adjacent[i]
            .iter()
            .copied()
            .filter(|&m| drain[m] && distance[m] < distance[i])
            .min_by_key(|&m| distance[m]);
        let (permille, directed) = match downstream {
            Some(d) => {
                let target = net.nodes[d].bounds.center().as_dvec3();
                let near = |p: DVec3| p.distance(target);
                let (low, high) = if near(run.ends[0]) <= near(run.ends[1]) {
                    (run.ends[0], run.ends[1])
                } else {
                    (run.ends[1], run.ends[0])
                };
                ((high.z - low.z) / length * 1000.0, true)
            }
            None => {
                undirected += 1;
                (run.rise().abs() / length * 1000.0, false)
            }
        };
        let need = rule.min_for(run.size_mm());
        let problem = |severity, text: String, color| Problem {
            elements: vec![node.key],
            severity,
            text: format!("{text} in {}", node.system),
            value: Some((permille * 10.0).round() / 10.0),
            color,
        };
        if directed && permille < -tolerance {
            outcome.problems.push(problem(
                Severity::Critical,
                format!("Falls the wrong way: {:.1} ‰", permille),
                ERROR,
            ));
        } else if permille.abs() <= tolerance {
            outcome.problems.push(problem(
                Severity::Critical,
                format!("Flat: {:.1} ‰", permille.abs()),
                ERROR,
            ));
        } else if permille < need - tolerance {
            outcome.problems.push(problem(
                Severity::Low,
                format!("{:.1} ‰, needs {} ‰", permille, need),
                WARNING,
            ));
        } else {
            good.push(node.key);
        }
    }
    outcome
        .problems
        .sort_by_key(|p| (p.severity != Severity::Critical, p.text.clone()));

    if !good.is_empty() {
        outcome.categories.push(Category {
            label: "Enough fall".to_owned(),
            detail: format!("{} pipes", good.len()),
            color: GOOD,
            elements: good,
        });
    }
    if !neutral.is_empty() {
        outcome.categories.push(Category {
            label: "Stacks and fittings".to_owned(),
            detail: format!("{} elements", neutral.len()),
            color: NEUTRAL,
            elements: neutral,
        });
    }
    outcome.summary = format!(
        "{pipes} horizontal drain pipes in {} systems",
        systems.len()
    );
    outcome.problem_title = "Fall".to_owned();
    outcome.problem_hint =
        "Red: flat or falling the wrong way. Yellow: less fall than required.".to_owned();
    if systems.is_empty() {
        outcome.notes.push(
            "No drain systems found: check the list of systems and the words that mark drains."
                .to_owned(),
        );
    }
    if undirected > 0 {
        outcome.notes.push(format!(
            "{undirected} pipes are not connected to other drain pipes: only the size of their fall is judged."
        ));
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drains_by_words_and_by_hand() {
        let mut rule = SlopeRule::default();
        assert!(rule.classify("SV 11", "Afløb, Spildevand").0);
        assert!(rule.classify("DV 18", "Terræn, Dræn(vand)").0);
        assert!(rule.classify("X", "DRAINAGE").0);
        assert!(!rule.classify("VR 9", "Varme, Retur").0);
        rule.not_drains.push("SV 11".to_owned());
        assert_eq!(rule.classify("SV 11", "Afløb, Spildevand"), (false, true));
    }

    #[test]
    fn limits_by_size() {
        let rule = SlopeRule {
            limits: vec![
                SlopeLimit {
                    up_to_mm: 110.0,
                    min_permille: 20.0,
                },
                SlopeLimit {
                    up_to_mm: 75.0,
                    min_permille: 25.0,
                },
            ],
            ..SlopeRule::default()
        };
        assert_eq!(rule.min_for(50.0), 25.0);
        assert_eq!(rule.min_for(110.0), 20.0);
        assert_eq!(rule.min_for(160.0), 10.0);
    }
}
