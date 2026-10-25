//! State of the Checking tab: the project's rulesets, which models they run
//! on, the last results and the reviewer's decisions on findings.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use opencheck_core::check::{
    BUILTIN_RULESETS, CheckJob, CheckReport, Finding, IDS_PREFIX, Ids, InformationStandard, Rule,
    RuleKind, RuleResult, Ruleset, RulesetInfo, RulesetResult, Selector, Severity,
    StandardOverrides, builtin_standard, ids_path,
};
use opencheck_core::project::RulesetFile;
use opencheck_core::{Discipline, ModelId};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

/// The Markdown guide users can hand to a language model to write rulesets.
pub const RULESET_GUIDE: &str = concat!(
    include_str!("../../../docs/ruleset-authoring.md"),
    "

---

",
    include_str!("../../../docs/rule-scripts.md")
);

/// The information standard whose levels the project chooses between.
pub const INFORMATION_STANDARD: &str = "opencheck.information.lod-dk";

/// Where the project's changes to the information standard are stored.
pub fn overrides_path(standard: &str) -> String {
    format!("requirements/{standard}.toml")
}

/// The project's changes to the information standard and the standard with
/// them applied.
pub struct Adapted {
    pub overrides: StandardOverrides,
    pub standard: Arc<InformationStandard>,
}

/// Display name of a level, e.g. "LOD 300 DK".
pub fn level_label(level: &str) -> String {
    format!("LOD {level} DK")
}

/// Levels of the information standard, lowest first.
pub fn information_levels() -> Vec<String> {
    builtin_standard(INFORMATION_STANDARD)
        .map(|s| s.standard.levels.clone())
        .unwrap_or_default()
}

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

/// An IDS file stored in the project (`requirements/*.ids`).
pub struct ProjectIds {
    pub path: String,
    pub text: String,
    pub ids: Arc<Ids>,
}

/// The rule checks against the built-in standard's levels (not an IDS file).
pub fn uses_levels(rule: &Rule) -> bool {
    rule.kind == RuleKind::Information && ids_path(&rule.standard).is_none()
}

/// The two lists on the right of the Checking tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResultsTab {
    #[default]
    Results,
    Issues,
}

/// Which findings the results list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatusFilter {
    #[default]
    All,
    /// Not yet accepted or rejected.
    Open,
    Accepted,
    Rejected,
    /// Already turned into an issue.
    Issues,
}

impl StatusFilter {
    pub const ALL: [StatusFilter; 5] = [
        StatusFilter::All,
        StatusFilter::Open,
        StatusFilter::Accepted,
        StatusFilter::Rejected,
        StatusFilter::Issues,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatusFilter::All => "Status: all",
            StatusFilter::Open => "Open",
            StatusFilter::Accepted => "Accepted",
            StatusFilter::Rejected => "Rejected",
            StatusFilter::Issues => "Issues",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResultFilter {
    pub status: StatusFilter,
    pub severity: Option<Severity>,
    /// `ruleset id / rule id`, or None for every rule.
    pub rule: Option<String>,
    /// Ruleset id shown on its own, or None for all rulesets.
    pub ruleset: Option<String>,
}

/// Disciplines of a pair in a fixed order, so "EL vs KON" and "KON vs EL" are one pair.
pub type PairKey = (Discipline, Discipline);

pub fn pair_key(a: Discipline, b: Discipline) -> PairKey {
    if (a as u8) <= (b as u8) {
        (a, b)
    } else {
        (b, a)
    }
}

pub fn rule_key(ruleset: &str, rule: &str) -> String {
    format!("{ruleset} / {rule}")
}

#[derive(Default)]
pub struct Checking {
    pub rulesets: Vec<ProjectRuleset>,
    /// IDS files that information rules refer to.
    pub requirements: Vec<ProjectIds>,
    /// The project's changes to where the information standard looks.
    pub adapted: Option<Adapted>,
    /// The window that edits them, when open.
    pub editor: Option<crate::standard_editor::EditorState>,
    pub report: Option<Arc<CheckReport>>,
    /// Finding id -> (ruleset, rule, finding) positions in the report.
    index: FxHashMap<String, (usize, usize, usize)>,
    pub decisions: FxHashMap<String, Decision>,
    /// Id of the finding shown in the 3D view.
    pub selected: Option<String>,
    /// Progress of a running check in thousandths; None when idle.
    pub progress: Option<Arc<AtomicU32>>,
    pub filter: ResultFilter,
    /// Which side of each discipline pair its findings are grouped by.
    pub group_by: FxHashMap<PairKey, Discipline>,
    /// A pair to open and scroll to (ruleset id, pair), requested from an
    /// overview matrix.
    pub reveal_pair: Option<(String, PairKey)>,
    /// Findings picked in the results list, in pick order.
    pub marked: Vec<String>,
    /// Where a Shift+click range starts.
    pub anchor: Option<String>,
    pub tab: ResultsTab,
    /// Rulesets or models changed since the last run.
    pub stale: bool,
    /// `checking.json` waiting for the project's models to load.
    pending: Option<serde_json::Value>,
    /// Information level (LOD) of the project; None: the standard's default.
    pub level: Option<String>,
    /// Models that use another level than the project.
    pub model_levels: FxHashMap<ModelId, String>,
    /// Working area: only elements touching it are shown and checked.
    pub area: Option<opencheck_core::area::WorkArea>,
    /// Scheduled runs without the app, as last saved from File > Autorun.
    pub autorun: Option<opencheck_core::autorun::AutorunConfig>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedChecking {
    rulesets: Vec<SavedRuleset>,
    decisions: Vec<SavedDecision>,
    /// Information level of the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<String>,
    /// Per-model levels, by model position in the manifest.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    model_levels: Vec<SavedLevel>,
    /// Working area in world coordinates.
    #[serde(skip_serializing_if = "Option::is_none")]
    area: Option<opencheck_core::area::WorkArea>,
    /// Autorun configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    autorun: Option<opencheck_core::autorun::AutorunConfig>,
}

#[derive(Serialize, Deserialize)]
struct SavedLevel {
    model: usize,
    level: String,
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

    /// Adds an IDS file and a ruleset with one information rule that checks
    /// it. Importing a file with the same name again replaces its content.
    pub fn add_ids(&mut self, text: &str, file_name: &str) -> Result<(), String> {
        let ids = Ids::from_xml(text).map_err(|e| e.to_string())?;
        let stem = file_name
            .trim_end_matches(".ids")
            .trim_end_matches(".IDS")
            .trim();
        let stem = sanitize(if stem.is_empty() {
            "requirements"
        } else {
            stem
        });
        let path = format!("requirements/{stem}.ids");
        let title = ids.title().to_owned();
        let description = ids.info.description.clone();
        match self.requirements.iter_mut().find(|r| r.path == path) {
            Some(existing) => {
                existing.text = text.to_owned();
                existing.ids = Arc::new(ids);
            }
            None => self.requirements.push(ProjectIds {
                path: path.clone(),
                text: text.to_owned(),
                ids: Arc::new(ids),
            }),
        }
        let standard = format!("{IDS_PREFIX}{path}");
        let referenced = self
            .rulesets
            .iter()
            .any(|r| r.ruleset.rules.iter().any(|rule| rule.standard == standard));
        if !referenced {
            let ruleset = Ruleset {
                ruleset: RulesetInfo {
                    id: format!("ids.{stem}"),
                    name: format!("IDS: {title}"),
                    description,
                    version: "1".to_owned(),
                    author: String::new(),
                },
                rules: vec![Rule {
                    id: "ids".to_owned(),
                    name: "Elements meet the IDS specifications".to_owned(),
                    description: String::new(),
                    kind: RuleKind::Information,
                    severity: Severity::Moderate,
                    tolerance_mm: 10.0,
                    clearance_mm: 0.0,
                    between: Default::default(),
                    a: Selector::default(),
                    b: None,
                    standard,
                    zone: None,
                    script: String::new(),
                    params: Default::default(),
                }],
            };
            self.add_ruleset(&ruleset.to_toml(), Some(&format!("ids-{stem}")))?;
        }
        self.stale = true;
        Ok(())
    }

    pub fn add_builtin(&mut self, index: usize) {
        let builtin = &BUILTIN_RULESETS[index];
        self.add_ruleset(builtin.toml, Some(builtin.file_name))
            .expect("built-in rulesets are tested to parse");
    }

    /// Adds the built-in ruleset with this file name.
    #[cfg(test)]
    pub fn add_builtin_file(&mut self, file_name: &str) {
        if let Some(index) = BUILTIN_RULESETS
            .iter()
            .position(|b| b.file_name == file_name)
        {
            self.add_builtin(index);
        }
    }

    pub fn remove_ruleset(&mut self, index: usize) {
        self.rulesets.remove(index);
        // IDS files no rule refers to any more go with it.
        let used: Vec<String> = self
            .rulesets
            .iter()
            .flat_map(|r| &r.ruleset.rules)
            .filter_map(|rule| ids_path(&rule.standard).map(str::to_owned))
            .collect();
        self.requirements.retain(|r| used.contains(&r.path));
        self.stale = true;
    }

    /// Jobs for the enabled rulesets that have at least one model to run on.
    pub fn jobs(&self, all_models: &[ModelId]) -> Vec<CheckJob> {
        self.rulesets
            .iter()
            .filter(|r| r.enabled)
            .map(|r| self.job(r, all_models))
            .filter(|job| !job.models.is_empty())
            .collect()
    }

    pub fn job(&self, r: &ProjectRuleset, all_models: &[ModelId]) -> CheckJob {
        let models: Vec<ModelId> = match &r.models {
            Some(list) => list
                .iter()
                .filter(|m| all_models.contains(m))
                .copied()
                .collect(),
            None => all_models.to_vec(),
        };
        CheckJob {
            ruleset: r.ruleset.clone(),
            levels: models.iter().map(|m| (*m, self.level_of(*m))).collect(),
            models,
            ids: self
                .requirements
                .iter()
                .map(|r| (r.path.clone(), r.ids.clone()))
                .collect(),
            standards: self.adapted.iter().map(|a| a.standard.clone()).collect(),
        }
    }

    /// The information standard as the project uses it, with its changes.
    pub fn information_standard(&self) -> Option<&InformationStandard> {
        match &self.adapted {
            Some(a) => Some(&a.standard),
            None => builtin_standard(INFORMATION_STANDARD),
        }
    }

    /// The project's changes, or none yet.
    pub fn overrides(&self) -> StandardOverrides {
        self.adapted
            .as_ref()
            .map(|a| a.overrides.clone())
            .unwrap_or_else(|| StandardOverrides::new(INFORMATION_STANDARD))
    }

    /// Replaces the project's changes to the information standard. Returns
    /// changes that no longer fit the standard.
    pub fn set_overrides(&mut self, overrides: StandardOverrides) -> Vec<String> {
        let Some(base) = builtin_standard(&overrides.base) else {
            return vec![format!(
                "Unknown information standard '{}'.",
                overrides.base
            )];
        };
        if overrides.is_empty() {
            self.adapted = None;
            self.information_changed();
            return Vec::new();
        }
        let (standard, skipped) = overrides.apply(base);
        self.adapted = Some(Adapted {
            overrides,
            standard: Arc::new(standard),
        });
        self.information_changed();
        skipped
    }

    /// The project's information level.
    pub fn project_level(&self) -> String {
        self.level.clone().unwrap_or_else(|| {
            builtin_standard(INFORMATION_STANDARD)
                .map(|s| s.standard.default_level.clone())
                .unwrap_or_default()
        })
    }

    /// The information level a model is checked at.
    pub fn level_of(&self, model: ModelId) -> String {
        self.model_levels
            .get(&model)
            .cloned()
            .unwrap_or_else(|| self.project_level())
    }

    pub fn set_project_level(&mut self, level: String) {
        if self.project_level() != level {
            self.level = Some(level);
            self.information_changed();
        }
    }

    /// None: follow the project.
    pub fn set_model_level(&mut self, model: ModelId, level: Option<String>) {
        let changed = match level {
            Some(l) => self.model_levels.insert(model, l.clone()) != Some(l),
            None => self.model_levels.remove(&model).is_some(),
        };
        if changed {
            self.information_changed();
        }
    }

    /// Results of information rules are out of date.
    fn information_changed(&mut self) {
        let has_information = self
            .rulesets
            .iter()
            .any(|r| r.ruleset.rules.iter().any(uses_levels));
        if has_information {
            self.stale = true;
        }
    }

    pub fn finish_run(&mut self, report: CheckReport) {
        self.index.clear();
        for (si, ruleset) in report.rulesets.iter().enumerate() {
            for (ri, rule) in ruleset.rules.iter().enumerate() {
                for (fi, f) in rule.findings.iter().enumerate() {
                    self.index.insert(f.id.clone(), (si, ri, fi));
                }
            }
        }
        self.report = Some(Arc::new(report));
        let index = &self.index;
        self.marked.retain(|id| index.contains_key(id));
        self.progress = None;
        self.stale = false;
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !self.index.contains_key(id))
        {
            self.selected = None;
        }
    }

    pub fn find(&self, id: &str) -> Option<(&RulesetResult, &RuleResult, &Finding)> {
        let report = self.report.as_ref()?;
        let &(si, ri, fi) = self.index.get(id)?;
        let ruleset = &report.rulesets[si];
        let rule = &ruleset.rules[ri];
        Some((ruleset, rule, &rule.findings[fi]))
    }

    /// Updates the picked findings for a click: plain replaces, Ctrl toggles,
    /// Shift picks the range from the last click in `order` (the list as drawn).
    pub fn click(&mut self, id: &str, ctrl: bool, shift: bool, order: &[String]) {
        if shift
            && let Some(anchor) = self.anchor.clone()
            && let (Some(a), Some(b)) = (
                order.iter().position(|x| *x == anchor),
                order.iter().position(|x| x == id),
            )
        {
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

    /// Ticks or unticks a finding, keeping the others.
    pub fn toggle_mark(&mut self, id: &str, on: bool) {
        let present = self.marked.iter().any(|m| m == id);
        if on && !present {
            self.marked.push(id.to_owned());
        } else if !on && present {
            self.marked.retain(|m| m != id);
        }
        self.anchor = Some(id.to_owned());
    }

    /// Ticks or unticks a whole group.
    pub fn mark_group(&mut self, ids: &[String], on: bool) {
        for id in ids {
            self.toggle_mark(id, on);
        }
    }

    pub fn decide(&mut self, id: &str, decision: Option<Decision>) {
        match decision {
            Some(d) => self.decisions.insert(id.to_owned(), d),
            None => self.decisions.remove(id),
        };
    }

    /// Forgets a removed model in ruleset scopes.
    pub fn forget_model(&mut self, model: ModelId) {
        self.model_levels.remove(&model);
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

    pub fn requirement_files(&self) -> Vec<RulesetFile> {
        let mut files: Vec<RulesetFile> = self
            .requirements
            .iter()
            .map(|r| RulesetFile {
                path: r.path.clone(),
                text: r.text.clone(),
            })
            .collect();
        if let Some(adapted) = &self.adapted {
            files.push(RulesetFile {
                path: overrides_path(&adapted.overrides.base),
                text: adapted.overrides.to_toml(),
            });
        }
        files
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
            level: self.level.clone(),
            area: self.area,
            autorun: self.autorun.clone(),
            model_levels: {
                let mut list: Vec<SavedLevel> = self
                    .model_levels
                    .iter()
                    .filter_map(|(m, level)| {
                        Some(SavedLevel {
                            model: model_order.iter().position(|o| o == m)?,
                            level: level.clone(),
                        })
                    })
                    .collect();
                list.sort_by_key(|l| l.model);
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
        requirements: Vec<RulesetFile>,
        json: Option<serde_json::Value>,
    ) -> Vec<String> {
        *self = Checking::default();
        let mut errors = Vec::new();
        for file in requirements {
            if file.path.ends_with(".toml") {
                match StandardOverrides::from_toml(&file.text) {
                    Ok(overrides) => {
                        for message in self.set_overrides(overrides) {
                            errors.push(format!("Information requirements: {message}"));
                        }
                    }
                    Err(e) => errors.push(format!(
                        "The changes to the information requirements in {} could not be read: {e}",
                        file.path
                    )),
                }
                continue;
            }
            match Ids::from_xml(&file.text) {
                Ok(ids) => self.requirements.push(ProjectIds {
                    path: file.path,
                    text: file.text,
                    ids: Arc::new(ids),
                }),
                Err(e) => errors.push(format!("IDS file {} could not be read: {e}", file.path)),
            }
        }
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
        self.level = saved.level;
        self.area = saved.area;
        self.autorun = saved.autorun;
        self.model_levels = saved
            .model_levels
            .into_iter()
            .filter_map(|l| Some((*model_order.get(l.model)?, l.level)))
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
        let errors = loaded.load(checking.ruleset_files(), Vec::new(), Some(json));
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

    #[test]
    fn adapted_standard_is_saved_and_used_by_jobs() {
        use opencheck_core::check::Source;
        let mut checking = Checking::default();
        checking.add_builtin_file("information-lod-dk.toml");
        let mut overrides = checking.overrides();
        overrides.parts.entry("wall".into()).or_default().insert(
            "thickness".into(),
            vec![Source {
                property: Some("^Tykkelse$".into()),
                ..Source::default()
            }],
        );
        assert!(checking.set_overrides(overrides).is_empty());
        let standard = checking.information_standard().unwrap();
        let wall = standard.parts.iter().find(|p| p.id == "wall").unwrap();
        assert!(wall.sources.contains_key("thickness"));
        let files = checking.requirement_files();
        assert_eq!(files[0].path, overrides_path(INFORMATION_STANDARD));

        let mut loaded = Checking::default();
        let errors = loaded.load(checking.ruleset_files(), files, None);
        assert!(errors.is_empty(), "{errors:?}");
        let job = loaded
            .rulesets
            .iter()
            .find(|r| r.ruleset.ruleset.id == INFORMATION_STANDARD)
            .map(|r| loaded.job(r, &[ModelId(1)]))
            .unwrap();
        assert_eq!(job.standards.len(), 1);

        // Removing every change goes back to the built-in standard.
        assert!(
            loaded
                .set_overrides(StandardOverrides::new(INFORMATION_STANDARD))
                .is_empty()
        );
        assert!(loaded.adapted.is_none());
        assert!(loaded.requirement_files().is_empty());
    }

    #[test]
    fn imported_ids_gets_a_ruleset_and_is_saved_with_it() {
        let ids = r#"<ids xmlns="http://standards.buildingsmart.org/IDS"><info><title>Delivery</title></info>
            <specifications><specification name="Walls" ifcVersion="IFC4">
            <applicability><entity><name><simpleValue>IFCWALL</simpleValue></name></entity></applicability>
            </specification></specifications></ids>"#;
        let mut checking = Checking::default();
        assert!(checking.add_ids("<nope", "bad").is_err());
        checking.add_ids(ids, "Delivery v2").unwrap();
        checking.add_ids(ids, "Delivery v2").unwrap();
        assert_eq!(checking.rulesets.len(), 1);
        assert_eq!(checking.requirements.len(), 1);
        let path = checking.requirements[0].path.clone();
        assert_eq!(path, "requirements/Delivery-v2.ids");
        let rule = &checking.rulesets[0].ruleset.rules[0];
        assert_eq!(rule.standard, format!("ids:{path}"));
        assert!(!uses_levels(rule));
        let job = checking.job(&checking.rulesets[0], &[ModelId(1)]);
        assert_eq!(job.ids[0].0, path);

        let mut loaded = Checking::default();
        let errors = loaded.load(checking.ruleset_files(), checking.requirement_files(), None);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(loaded.requirements[0].ids.title(), "Delivery");

        // The file goes with the last ruleset that uses it.
        checking.remove_ruleset(0);
        assert!(checking.requirements.is_empty());
    }
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    #[test]
    fn clicks_pick_ranges_and_toggle() {
        let order: Vec<String> = ["1", "2", "3", "4"].map(String::from).to_vec();
        let mut checking = Checking::default();
        checking.click("1", false, false, &order);
        checking.click("3", false, true, &order);
        assert_eq!(checking.marked, vec!["1", "2", "3"]);
        checking.click("2", true, false, &order);
        assert_eq!(checking.marked, vec!["1", "3"]);
        checking.mark_group(&order, true);
        assert_eq!(checking.marked.len(), 4);
        checking.mark_group(&order[..2], false);
        assert_eq!(checking.marked, vec!["3", "4"]);
    }

    #[test]
    fn pair_keys_ignore_order() {
        assert_eq!(
            pair_key(Discipline::Electrical, Discipline::Structural),
            pair_key(Discipline::Structural, Discipline::Electrical)
        );
    }
}

#[cfg(test)]
mod standard_tests {
    use super::*;

    #[test]
    fn levels_follow_the_project_unless_a_model_has_its_own() {
        let mut checking = Checking::default();
        checking.add_builtin_file("information-lod-dk.toml");
        let (a, b) = (ModelId(1), ModelId(2));
        assert_eq!(checking.level_of(a), "300");
        checking.set_project_level("325".into());
        checking.set_model_level(b, Some("200".into()));
        assert_eq!(checking.level_of(a), "325");
        assert_eq!(checking.level_of(b), "200");
        let job = checking
            .rulesets
            .iter()
            .find(|r| r.ruleset.ruleset.id == INFORMATION_STANDARD)
            .map(|r| checking.job(r, &[a, b]))
            .unwrap();
        assert_eq!(
            job.levels,
            vec![(a, "325".to_owned()), (b, "200".to_owned())]
        );

        // Saved with the project and read back by model position.
        let json = checking.to_json(&[a, b]);
        let mut loaded = Checking::default();
        loaded.load(checking.ruleset_files(), Vec::new(), Some(json));
        loaded.apply_pending(&[ModelId(7), ModelId(8)]);
        assert_eq!(loaded.level_of(ModelId(7)), "325");
        assert_eq!(loaded.level_of(ModelId(8)), "200");

        checking.set_model_level(b, None);
        assert_eq!(checking.level_of(b), "325");
    }
}
