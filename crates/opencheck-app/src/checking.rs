//! State of the Checking tab: the project's rulesets, which models they run
//! on, the last results and the reviewer's decisions on findings.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use opencheck_core::ModelId;
use opencheck_core::check::{
    BUILTIN_RULESETS, CheckJob, CheckReport, Finding, FindingElement, RuleResult, Ruleset,
    RulesetResult,
};
use opencheck_core::project::RulesetFile;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// The Markdown guide users can hand to a language model to write rulesets.
pub const RULESET_GUIDE: &str = include_str!("../../../docs/ruleset-authoring.md");

/// A reviewer's verdict on a finding. Undecided findings have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// A real problem that must be handled.
    Accepted,
    /// Not a problem (planned penetration, modelling convention, ...).
    Rejected,
}

pub struct ProjectRuleset {
    /// Archive path in the project, e.g. `rulesets/clash-mep-vs-structure.toml`.
    pub path: String,
    pub text: String,
    pub ruleset: Ruleset,
    pub enabled: bool,
    /// Models the ruleset runs on; None means all models.
    pub models: Option<Vec<ModelId>>,
}

/// The two lists on the right of the Checking tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsTab {
    #[default]
    Results,
    Issues,
}

/// Findings of one rule with the same pair of IFC classes, e.g. ducts against beams.
pub struct Category<'a> {
    pub a_class: &'a str,
    pub b_class: &'a str,
    pub components: Vec<Component<'a>>,
}

impl Category<'_> {
    pub fn count(&self) -> usize {
        self.components.iter().map(|c| c.findings.len()).sum()
    }

    pub fn ids(&self) -> Vec<String> {
        self.components
            .iter()
            .flat_map(|c| &c.findings)
            .map(|f| f.id.clone())
            .collect()
    }
}

/// Findings of one element from side a, e.g. one duct and every beam it hits.
pub struct Component<'a> {
    pub element: &'a FindingElement,
    pub findings: Vec<&'a Finding>,
}

/// Groups findings by class pair, then by the element on side a. The most
/// affected groups come first, so the worst problems are on top.
pub fn group_findings<'a>(findings: impl IntoIterator<Item = &'a Finding>) -> Vec<Category<'a>> {
    let mut categories: Vec<Category<'a>> = Vec::new();
    for f in findings {
        let category = match categories
            .iter_mut()
            .position(|c| c.a_class == f.a.ifc_class && c.b_class == f.b.ifc_class)
        {
            Some(i) => &mut categories[i],
            None => {
                categories.push(Category {
                    a_class: &f.a.ifc_class,
                    b_class: &f.b.ifc_class,
                    components: Vec::new(),
                });
                categories.last_mut().expect("just pushed")
            }
        };
        match category
            .components
            .iter_mut()
            .find(|c| c.element.model == f.a.model && c.element.express_id == f.a.express_id)
        {
            Some(component) => component.findings.push(f),
            None => category.components.push(Component {
                element: &f.a,
                findings: vec![f],
            }),
        }
    }
    for category in &mut categories {
        for component in &mut category.components {
            component
                .findings
                .sort_by(|x, y| y.magnitude.total_cmp(&x.magnitude));
        }
        category.components.sort_by(|x, y| {
            y.findings
                .len()
                .cmp(&x.findings.len())
                .then_with(|| x.element.name.cmp(&y.element.name))
        });
    }
    categories.sort_by(|x, y| {
        y.count()
            .cmp(&x.count())
            .then_with(|| (x.a_class, x.b_class).cmp(&(y.a_class, y.b_class)))
    });
    categories
}

#[derive(Default)]
pub struct Checking {
    pub rulesets: Vec<ProjectRuleset>,
    pub report: Option<Arc<CheckReport>>,
    pub decisions: FxHashMap<String, Decision>,
    /// Id of the finding shown in the 3D view.
    pub selected: Option<String>,
    /// Progress of a running check in thousandths; None when idle.
    pub progress: Option<Arc<AtomicU32>>,
    pub show_decided: bool,
    /// Findings picked in the results list (Ctrl/Shift+click), in pick order.
    pub marked: Vec<String>,
    /// Where a Shift+click range starts.
    pub anchor: Option<String>,
    pub tab: ResultsTab,
    /// Rulesets or models changed since the last run.
    pub stale: bool,
    /// `checking.json` waiting for the project's models to load.
    pending: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedChecking {
    rulesets: Vec<SavedRuleset>,
    decisions: Vec<SavedDecision>,
}

#[derive(Serialize, Deserialize)]
struct SavedRuleset {
    path: String,
    enabled: bool,
    /// Model positions in the manifest; null means all models.
    models: Option<Vec<usize>>,
}

#[derive(Serialize, Deserialize)]
struct SavedDecision {
    finding: String,
    decision: Decision,
}

impl Checking {
    pub fn is_running(&self) -> bool {
        self.progress.is_some()
    }

    pub fn progress_fraction(&self) -> f32 {
        self.progress
            .as_ref()
            .map_or(0.0, |p| p.load(Ordering::Relaxed) as f32 / 1000.0)
    }

    pub fn has_ruleset(&self, id: &str) -> bool {
        self.rulesets.iter().any(|r| r.ruleset.ruleset.id == id)
    }

    /// Adds a ruleset from TOML text. A ruleset with the same id is replaced,
    /// keeping its settings, so re-importing an edited file updates it.
    pub fn add_ruleset(&mut self, text: &str, file_name: Option<&str>) -> Result<(), String> {
        let ruleset = Ruleset::from_toml(text).map_err(|e| e.to_string())?;
        if let Some(existing) = self
            .rulesets
            .iter_mut()
            .find(|r| r.ruleset.ruleset.id == ruleset.ruleset.id)
        {
            existing.text = text.to_owned();
            existing.ruleset = ruleset;
        } else {
            let stem = file_name
                .map(|n| n.trim_end_matches(".toml").to_owned())
                .unwrap_or_else(|| sanitize(&ruleset.ruleset.id));
            let mut path = format!("rulesets/{stem}.toml");
            let mut n = 2;
            while self.rulesets.iter().any(|r| r.path == path) {
                path = format!("rulesets/{stem} ({n}).toml");
                n += 1;
            }
            self.rulesets.push(ProjectRuleset {
                path,
                text: text.to_owned(),
                ruleset,
                enabled: true,
                models: None,
            });
        }
        self.stale = true;
        Ok(())
    }

    pub fn add_builtin(&mut self, index: usize) {
        let builtin = &BUILTIN_RULESETS[index];
        self.add_ruleset(builtin.toml, Some(builtin.file_name))
            .expect("built-in rulesets are tested to parse");
    }

    pub fn remove_ruleset(&mut self, index: usize) {
        self.rulesets.remove(index);
        self.stale = true;
    }

    /// Jobs for the enabled rulesets that have at least one model to run on.
    pub fn jobs(&self, all_models: &[ModelId]) -> Vec<CheckJob> {
        self.rulesets
            .iter()
            .filter(|r| r.enabled)
            .map(|r| CheckJob {
                ruleset: r.ruleset.clone(),
                models: match &r.models {
                    Some(list) => list
                        .iter()
                        .filter(|m| all_models.contains(m))
                        .copied()
                        .collect(),
                    None => all_models.to_vec(),
                },
            })
            .filter(|job| !job.models.is_empty())
            .collect()
    }

    pub fn finish_run(&mut self, report: CheckReport) {
        self.report = Some(Arc::new(report));
        let report = self.report.clone().expect("just set");
        let exists = |id: &String| {
            report
                .rulesets
                .iter()
                .flat_map(|r| &r.rules)
                .any(|r| r.findings.iter().any(|f| f.id == *id))
        };
        self.marked.retain(exists);
        self.progress = None;
        self.stale = false;
        if self
            .selected
            .as_ref()
            .is_some_and(|id| self.find(id).is_none())
        {
            self.selected = None;
        }
    }

    pub fn find(&self, id: &str) -> Option<(&RulesetResult, &RuleResult, &Finding)> {
        let report = self.report.as_ref()?;
        report.rulesets.iter().find_map(|rs| {
            rs.rules.iter().find_map(|rule| {
                rule.findings
                    .iter()
                    .find(|f| f.id == id)
                    .map(|f| (rs, rule, f))
            })
        })
    }

    /// Ids of the findings in the results list, in the order they are drawn.
    pub fn ordered_shown(&self) -> Vec<String> {
        let Some(report) = &self.report else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        for rule in report.rulesets.iter().flat_map(|r| &r.rules) {
            let shown = rule.findings.iter().filter(|f| self.is_shown(f));
            for category in group_findings(shown) {
                ids.extend(category.ids());
            }
        }
        ids
    }

    /// Updates the picked findings for a click: plain replaces, Ctrl toggles,
    /// Shift picks the range from the last click.
    pub fn click(&mut self, id: &str, ctrl: bool, shift: bool) {
        if shift && let Some(anchor) = self.anchor.clone() {
            let order = self.ordered_shown();
            if let (Some(a), Some(b)) = (
                order.iter().position(|x| *x == anchor),
                order.iter().position(|x| x == id),
            ) {
                let range = order[a.min(b)..=a.max(b)].to_vec();
                if ctrl {
                    for r in range {
                        if !self.marked.contains(&r) {
                            self.marked.push(r);
                        }
                    }
                } else {
                    self.marked = range;
                }
                return;
            }
        }
        if ctrl {
            if let Some(pos) = self.marked.iter().position(|m| m == id) {
                self.marked.remove(pos);
            } else {
                self.marked.push(id.to_owned());
            }
        } else {
            self.marked = vec![id.to_owned()];
        }
        self.anchor = Some(id.to_owned());
    }

    /// Picks every finding in `ids` (a whole category or component).
    pub fn mark_all(&mut self, ids: Vec<String>) {
        self.anchor = ids.first().cloned();
        self.marked = ids;
    }

    pub fn is_shown(&self, finding: &Finding) -> bool {
        self.show_decided || !self.decisions.contains_key(&finding.id)
    }

    pub fn decide(&mut self, id: &str, decision: Option<Decision>) {
        match decision {
            Some(d) => self.decisions.insert(id.to_owned(), d),
            None => self.decisions.remove(id),
        };
    }

    /// Forgets a removed model in ruleset scopes.
    pub fn forget_model(&mut self, model: ModelId) {
        for r in &mut self.rulesets {
            if let Some(list) = &mut r.models {
                list.retain(|m| *m != model);
            }
        }
        self.stale = true;
    }

    // ---- Persistence -------------------------------------------------------

    pub fn ruleset_files(&self) -> Vec<RulesetFile> {
        self.rulesets
            .iter()
            .map(|r| RulesetFile {
                path: r.path.clone(),
                text: r.text.clone(),
            })
            .collect()
    }

    /// `checking.json`; model scopes are stored as positions in `model_order`.
    pub fn to_json(&self, model_order: &[ModelId]) -> serde_json::Value {
        let saved = SavedChecking {
            rulesets: self
                .rulesets
                .iter()
                .map(|r| SavedRuleset {
                    path: r.path.clone(),
                    enabled: r.enabled,
                    models: r.models.as_ref().map(|list| {
                        list.iter()
                            .filter_map(|m| model_order.iter().position(|o| o == m))
                            .collect()
                    }),
                })
                .collect(),
            decisions: {
                let mut list: Vec<SavedDecision> = self
                    .decisions
                    .iter()
                    .map(|(finding, decision)| SavedDecision {
                        finding: finding.clone(),
                        decision: *decision,
                    })
                    .collect();
                list.sort_by(|a, b| a.finding.cmp(&b.finding));
                list
            },
        };
        serde_json::to_value(saved).expect("checking state always serialises")
    }

    /// Loads rulesets now; settings that refer to models wait for
    /// [`Checking::apply_pending`].
    pub fn load(
        &mut self,
        files: Vec<RulesetFile>,
        json: Option<serde_json::Value>,
    ) -> Vec<String> {
        *self = Checking::default();
        let mut errors = Vec::new();
        for file in files {
            match Ruleset::from_toml(&file.text) {
                Ok(ruleset) => self.rulesets.push(ProjectRuleset {
                    path: file.path,
                    text: file.text,
                    ruleset,
                    enabled: true,
                    models: None,
                }),
                Err(e) => errors.push(format!("Ruleset {} could not be read: {e}", file.path)),
            }
        }
        self.pending = json;
        errors
    }

    pub fn apply_pending(&mut self, model_order: &[ModelId]) {
        let Some(json) = self.pending.take() else {
            return;
        };
        let Ok(saved) = serde_json::from_value::<SavedChecking>(json) else {
            return;
        };
        for s in saved.rulesets {
            if let Some(r) = self.rulesets.iter_mut().find(|r| r.path == s.path) {
                r.enabled = s.enabled;
                r.models = s.models.map(|list| {
                    list.iter()
                        .filter_map(|i| model_order.get(*i).copied())
                        .collect()
                });
            }
        }
        self.decisions = saved
            .decisions
            .into_iter()
            .map(|d| (d.finding, d.decision))
            .collect();
    }
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_json() {
        let mut checking = Checking::default();
        checking.add_builtin(0);
        checking.add_builtin(1);
        let (a, b) = (ModelId(10), ModelId(11));
        checking.rulesets[1].models = Some(vec![b]);
        checking.rulesets[0].enabled = false;
        checking.decide("x", Some(Decision::Accepted));
        let json = checking.to_json(&[a, b]);

        let mut loaded = Checking::default();
        let errors = loaded.load(checking.ruleset_files(), Some(json));
        assert!(errors.is_empty());
        // Models get new ids when the project is opened again.
        let (a2, b2) = (ModelId(20), ModelId(21));
        loaded.apply_pending(&[a2, b2]);
        assert!(!loaded.rulesets[0].enabled);
        assert_eq!(loaded.rulesets[1].models, Some(vec![b2]));
        assert_eq!(loaded.decisions.get("x"), Some(&Decision::Accepted));
        assert_eq!(loaded.jobs(&[a2, b2]).len(), 1);
    }

    #[test]
    fn importing_the_same_ruleset_replaces_it() {
        let mut checking = Checking::default();
        checking.add_builtin(0);
        checking.add_builtin(0);
        assert_eq!(checking.rulesets.len(), 1);
    }
}

#[cfg(test)]
mod grouping_tests {
    use super::*;
    use glam::Vec3;
    use opencheck_core::check::FindingKind;
    use opencheck_core::{Aabb, ModelId};

    fn finding(id: &str, a: (u32, &str), b: (u32, &str), depth: f32) -> Finding {
        let element = |(express_id, class): (u32, &str)| FindingElement {
            model: ModelId(0),
            express_id,
            global_id: format!("g{express_id}"),
            name: format!("e{express_id}"),
            ifc_class: class.to_owned(),
        };
        Finding {
            id: id.to_owned(),
            kind: FindingKind::Clash,
            a: element(a),
            b: element(b),
            point: Vec3::ZERO,
            region: Aabb::EMPTY,
            magnitude: depth,
        }
    }

    #[test]
    fn groups_by_class_pair_then_component() {
        let findings = [
            finding("1", (1, "IfcDuctSegment"), (10, "IfcBeam"), 0.1),
            finding("2", (1, "IfcDuctSegment"), (11, "IfcBeam"), 0.3),
            finding("3", (2, "IfcDuctSegment"), (10, "IfcBeam"), 0.2),
            finding("4", (3, "IfcPipeSegment"), (12, "IfcSlab"), 0.2),
        ];
        let groups = group_findings(&findings);
        assert_eq!(groups.len(), 2);
        assert_eq!(
            (groups[0].a_class, groups[0].b_class),
            ("IfcDuctSegment", "IfcBeam")
        );
        assert_eq!(groups[0].count(), 3);
        // The duct with two clashes first, deepest clash first.
        assert_eq!(groups[0].components[0].element.express_id, 1);
        assert_eq!(groups[0].ids(), vec!["2", "1", "3"]);
    }

    #[test]
    fn shift_click_picks_a_range() {
        let mut checking = Checking::default();
        let report = CheckReport {
            rulesets: vec![RulesetResult {
                info: Ruleset::from_toml(BUILTIN_RULESETS[0].toml)
                    .unwrap()
                    .ruleset,
                rules: vec![RuleResult {
                    rule: Ruleset::from_toml(BUILTIN_RULESETS[0].toml).unwrap().rules[0].clone(),
                    findings: vec![
                        finding("1", (1, "IfcDuctSegment"), (10, "IfcBeam"), 0.3),
                        finding("2", (2, "IfcDuctSegment"), (10, "IfcBeam"), 0.2),
                        finding("3", (3, "IfcDuctSegment"), (10, "IfcBeam"), 0.1),
                    ],
                    pairs_tested: 3,
                    duration: std::time::Duration::ZERO,
                }],
            }],
            duration: std::time::Duration::ZERO,
        };
        checking.finish_run(report);
        checking.click("1", false, false);
        checking.click("3", false, true);
        assert_eq!(checking.marked, vec!["1", "2", "3"]);
        checking.click("2", true, false);
        assert_eq!(checking.marked, vec!["1", "3"]);
    }
}
