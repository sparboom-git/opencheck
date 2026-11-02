//! Levels: the storeys of all models side by side, by height, to see that
//! the models agree on the levels' heights and names. Storeys at the same
//! height (within the tolerance) form one level.
//!
//! With a master model, every other model's storey is held against the
//! master's: it is wrong when the master has the name at another height, a
//! storey at its height by another name, or neither. Without one, a level
//! whose storeys have different names, and a name used at different
//! heights, are problems. The result is a table and lines in the 3D view; no
//! element is coloured.

use std::collections::BTreeMap;

use super::ElementKey;

use serde::{Deserialize, Serialize};

use super::{Category, ERROR, LevelLine, Problem, VisualModel, VisualOutcome, palette};
use crate::check::Severity;
use crate::{Discipline, SpatialNode};

/// Settings of the level check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelRule {
    /// Models of these disciplines are looked at.
    pub disciplines: Vec<Discipline>,
    /// Storeys closer than this are at the same height, millimetres.
    pub tolerance_mm: f64,
    /// The model the others are held against, by short name; empty for
    /// none.
    pub master: String,
}

impl Default for LevelRule {
    fn default() -> Self {
        Self {
            disciplines: Discipline::ALL.to_vec(),
            // Storeys this close are one level: exporters round heights
            // differently, and a placement is a product of matrices.
            tolerance_mm: 10.0,
            master: String::new(),
        }
    }
}

/// One storey of one model.
struct Storey {
    model: usize,
    /// The storey itself (a spatial node, not an element): the subject of
    /// a problem, so autorun can keep it as a finding.
    key: ElementKey,
    name: String,
    /// Height as the model states it (the IFC world frame), metres.
    z: f64,
    /// Height in the 3D view.
    shared: f64,
}

/// A name compared loosely: case, spaces and dots do not count.
fn plain(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '.' | '_' | '-'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Short labels for the models: their discipline's code, numbered when
/// two share one.
fn labels(models: &[&VisualModel]) -> Vec<String> {
    let codes: Vec<&str> = models.iter().map(|m| m.model.discipline.code()).collect();
    codes
        .iter()
        .enumerate()
        .map(|(i, code)| {
            if codes.iter().filter(|c| *c == code).count() > 1 {
                let n = codes[..=i].iter().filter(|c| *c == code).count();
                format!("{code}{n}")
            } else {
                (*code).to_owned()
            }
        })
        .collect()
}

pub(super) fn run(rule: &LevelRule, models: &[VisualModel]) -> VisualOutcome {
    let mut outcome = VisualOutcome::default();
    let tolerance = rule.tolerance_mm.max(0.0) / 1000.0 + 1e-9;
    let used: Vec<&VisualModel> = models
        .iter()
        .filter(|m| rule.disciplines.contains(&m.model.discipline))
        .collect();
    let label = labels(&used);
    let mut storeys: Vec<Storey> = Vec::new();
    for (index, m) in used.iter().enumerate() {
        let scale = m.model.length_unit_scale;
        let before = storeys.len();
        if let Some(root) = &m.model.spatial_root {
            root.walk(&mut |node: &SpatialNode, _| {
                if node.is_storey()
                    && let Some(z) = node.placement_z.or(node.elevation)
                {
                    storeys.push(Storey {
                        model: index,
                        key: m.key(node.express_id),
                        name: node.label().to_owned(),
                        z: z * scale,
                        shared: m.world_height(z * scale),
                    });
                }
            });
        }
        if storeys.len() == before {
            outcome.notes.push(format!(
                "{} has no storeys with a height.",
                m.model.short_name
            ));
        }
    }
    storeys.sort_by(|a, b| a.z.total_cmp(&b.z));
    let master = used
        .iter()
        .position(|m| !rule.master.is_empty() && m.model.short_name == rule.master);
    if !rule.master.is_empty() && master.is_none() {
        outcome.notes.push(format!(
            "The master model {} is not among the models; the models are compared with each other.",
            rule.master
        ));
    }

    // Levels: storeys within the tolerance of the level's lowest.
    let mut levels: Vec<Vec<usize>> = Vec::new();
    for (i, s) in storeys.iter().enumerate() {
        match levels.last_mut() {
            Some(level) if s.z - storeys[level[0]].z <= tolerance => level.push(i),
            _ => levels.push(vec![i]),
        }
    }
    let at = |i: usize| {
        let s = &storeys[i];
        format!("{} {:+.3} ({})", s.name, s.z, label[s.model])
    };

    let mut wrong = vec![false; storeys.len()];
    match master {
        Some(master) => {
            let ours: Vec<usize> = (0..storeys.len())
                .filter(|&i| storeys[i].model == master)
                .collect();
            for (i, s) in storeys.iter().enumerate() {
                if s.model == master {
                    continue;
                }
                let by_name = ours
                    .iter()
                    .find(|&&m| plain(&storeys[m].name) == plain(&s.name));
                let by_height = ours
                    .iter()
                    .find(|&&m| (storeys[m].z - s.z).abs() <= tolerance);
                let text = match (by_name, by_height) {
                    (Some(&m), _) if (storeys[m].z - s.z).abs() > tolerance => Some(format!(
                        "{}: the master has {} at {:+.3}",
                        at(i),
                        storeys[m].name,
                        storeys[m].z
                    )),
                    (Some(_), _) => None,
                    (None, Some(&m)) => Some(format!(
                        "{}: the master calls this level {}",
                        at(i),
                        storeys[m].name
                    )),
                    (None, None) => Some(format!("{}: not in the master", at(i))),
                };
                if let Some(text) = text {
                    wrong[i] = true;
                    outcome.problems.push(Problem {
                        elements: vec![s.key],
                        severity: Severity::Moderate,
                        text,
                        value: Some(s.z),
                        color: ERROR,
                    });
                }
            }
        }
        None => {
            // One level, several names.
            for level in &levels {
                let names: BTreeMap<String, Vec<usize>> =
                    level.iter().fold(BTreeMap::new(), |mut map, &i| {
                        map.entry(plain(&storeys[i].name)).or_default().push(i);
                        map
                    });
                if names.len() > 1 {
                    for &i in level {
                        wrong[i] = true;
                    }
                    let list: Vec<String> = level
                        .iter()
                        .map(|&i| format!("{} ({})", storeys[i].name, label[storeys[i].model]))
                        .collect();
                    outcome.problems.push(Problem {
                        elements: level.iter().map(|&i| storeys[i].key).collect(),
                        severity: Severity::Low,
                        text: format!(
                            "{:+.3}: different names: {}",
                            storeys[level[0]].z,
                            list.join(", ")
                        ),
                        value: Some(storeys[level[0]].z),
                        color: ERROR,
                    });
                }
            }
            // One name, several heights.
            let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
            for (i, s) in storeys.iter().enumerate() {
                by_name.entry(plain(&s.name)).or_default().push(i);
            }
            for list in by_name.values() {
                let (lo, hi) = list.iter().fold((f64::MAX, f64::MIN), |(lo, hi), &i| {
                    (lo.min(storeys[i].z), hi.max(storeys[i].z))
                });
                if hi - lo > tolerance {
                    for &i in list {
                        wrong[i] = true;
                    }
                    let heights: Vec<String> = list
                        .iter()
                        .map(|&i| format!("{:+.3} ({})", storeys[i].z, label[storeys[i].model]))
                        .collect();
                    outcome.problems.push(Problem {
                        elements: list.iter().map(|&i| storeys[i].key).collect(),
                        severity: Severity::Moderate,
                        text: format!(
                            "{}: different heights: {}",
                            storeys[list[0]].name,
                            heights.join(", ")
                        ),
                        value: Some((hi - lo) * 1000.0),
                        color: ERROR,
                    });
                }
            }
        }
    }

    // The table: a row per level, a column per model, the master first.
    let mut order: Vec<usize> = (0..used.len()).collect();
    if let Some(master) = master {
        order.retain(|&m| m != master);
        order.insert(0, master);
    }
    let mut header = vec!["Kote".to_owned()];
    header.extend(order.iter().map(|&m| {
        if Some(m) == master {
            format!("{} (master)", label[m])
        } else {
            label[m].clone()
        }
    }));
    outcome.table.push(header);
    for level in &levels {
        let mut row = vec![format!("{:+.3}", storeys[level[0]].z)];
        for &model in &order {
            let names: Vec<String> = level
                .iter()
                .filter(|&&i| storeys[i].model == model)
                .map(|&i| {
                    let s = &storeys[i];
                    let mut text = s.name.clone();
                    if (s.z - storeys[level[0]].z).abs() > 0.0005 {
                        text = format!("{text} ({:+.3})", s.z);
                    }
                    if wrong[i] {
                        text = format!("{text} !");
                    }
                    text
                })
                .collect();
            row.push(if names.is_empty() {
                "–".to_owned()
            } else {
                names.join(", ")
            });
        }
        outcome.table.push(row);
    }

    // Lines in the 3D view, a colour per model; red where something is wrong.
    // Where the building is: the middle 96 % of the elements in each direction.
    let points: Vec<glam::DVec3> = used.iter().flat_map(|m| m.element_points()).collect();
    if !points.is_empty() {
        let range = |mut v: Vec<f64>| {
            v.sort_by(f64::total_cmp);
            let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
            (at(0.02), at(0.98))
        };
        let (x0, x1) = range(points.iter().map(|p| p.x).collect());
        let (y0, y1) = range(points.iter().map(|p| p.y).collect());
        outcome.extent = Some(([x0, y0], [x1, y1]));
    }
    for (i, s) in storeys.iter().enumerate() {
        outcome.levels.push(LevelLine {
            z: s.shared,
            elevation: s.z,
            name: s.name.clone(),
            model: label[s.model].clone(),
            color: if wrong[i] { ERROR } else { palette(s.model) },
            problem: wrong[i],
        });
    }
    for &index in &order {
        let count = storeys.iter().filter(|s| s.model == index).count();
        outcome.categories.push(Category {
            label: format!("{}  {}", label[index], used[index].model.short_name),
            detail: if Some(index) == master {
                format!("master, {count} storeys")
            } else {
                format!("{count} storeys")
            },
            color: palette(index),
            elements: Vec::new(),
        });
    }
    outcome.problem_title = if master.is_some() {
        "Levels that differ from the master".to_owned()
    } else {
        "Levels that disagree".to_owned()
    };
    outcome.problem_hint = match master {
        Some(m) => format!(
            "Each storey should have the master's ({}) name at the master's height, within {:.0} mm.",
            label[m], rule.tolerance_mm
        ),
        None => format!(
            "Storeys within {:.0} mm are one level; a level should have one name, and a name \
             one height, in all models.",
            rule.tolerance_mm
        ),
    };
    outcome.summary = format!(
        "{} levels in {} models ({} storeys)",
        levels.len(),
        used.len(),
        storeys.len()
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_loosely() {
        assert_eq!(plain("Niveau 01"), plain("niveau_01"));
        assert_ne!(plain("Niveau 01"), plain("Niveau 02"));
    }
}
