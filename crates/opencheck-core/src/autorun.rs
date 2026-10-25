//! Autorun: checks that run without the app, on a schedule, against IFC
//! files in a (network) folder. Each run is kept as one JSON file of
//! findings and figures per file; a dashboard shows how a project develops
//! from run to run: how many findings are new and how many were solved.
//!
//! This module holds the formats and the dashboard; reading folders, the
//! lock file and scheduling are the command line's and the app's business.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::check::{CheckReport, FindingKind};
use crate::{Discipline, IfcModel, ModelId};

/// Version of [`AutorunConfig`] and [`RunRecord`] files.
pub const FORMAT: u32 = 1;

/// What an autorun checks, where it reads and where it writes. Written by
/// the app as `autorun.json`; it holds everything a run needs, so the
/// project file is not read.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutorunConfig {
    #[serde(default = "format")]
    pub format: u32,
    /// Project name, shown in the dashboard.
    pub name: String,
    /// Folder with the IFC files; a UNC path (`\\server\share\...`) works on
    /// every machine, drive letters may not.
    pub source: String,
    /// The files and their disciplines.
    #[serde(default)]
    pub files: Vec<SourceFile>,
    /// Also check IFC files in the folder that are not listed, with a
    /// discipline guessed from the file name.
    #[serde(default = "yes")]
    pub new_files: bool,
    /// Folder for the history, the dashboard, the log and the lock file.
    pub output: String,
    /// The rulesets to run, as TOML.
    #[serde(default)]
    pub rulesets: Vec<ConfigRuleset>,
    /// IDS documents the rulesets refer to (`ids:<path>`), as XML.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ids: Vec<ConfigDocument>,
    /// The project's changes to the information standard, as TOML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub information_changes: Option<String>,
    /// Information level (LOD) for information rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub schedule: Schedule,
    /// Seconds a file's size must stay the same before it is read, so a
    /// file still being copied is not checked half.
    #[serde(default = "settle")]
    pub settle_seconds: u64,
}

fn format() -> u32 {
    FORMAT
}

fn yes() -> bool {
    true
}

fn settle() -> u64 {
    10
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceFile {
    /// File name in the source folder, e.g. `KON_model.ifc`.
    pub name: String,
    pub discipline: Discipline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigRuleset {
    /// Archive path in the project, e.g. `rulesets/clash-internal.toml`.
    pub path: String,
    pub toml: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfigDocument {
    pub path: String,
    pub text: String,
}

/// How often the scheduled task runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schedule {
    pub every: Every,
    /// Time of day, `HH:MM`.
    pub time: String,
    /// Weekly: the day, `MON` to `SUN`.
    #[serde(default = "monday")]
    pub day: String,
}

fn monday() -> String {
    "MON".to_owned()
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            every: Every::Daily,
            time: "06:00".to_owned(),
            day: monday(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Every {
    Daily,
    Weekly,
}

impl AutorunConfig {
    pub fn new(name: &str) -> Self {
        Self {
            format: FORMAT,
            name: name.to_owned(),
            source: String::new(),
            files: Vec::new(),
            new_files: true,
            output: String::new(),
            rulesets: Vec::new(),
            ids: Vec::new(),
            information_changes: None,
            level: None,
            schedule: Schedule::default(),
            settle_seconds: settle(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("the config always serialises")
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let config: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if config.format > FORMAT {
            return Err(format!(
                "the configuration was written by a newer OpenCheck (format {}); please update",
                config.format
            ));
        }
        Ok(config)
    }

    /// The discipline set for a file, if it is listed.
    pub fn discipline_of(&self, file_name: &str) -> Option<Discipline> {
        self.files
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(file_name))
            .map(|f| f.discipline)
    }
}

/// One run: figures per file and every finding. Never changed after it is
/// written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    #[serde(default = "format")]
    pub format: u32,
    /// Local start time, `YYYY-MM-DDTHH:MM:SS`; sorts like the runs.
    pub run_id: String,
    pub machine: String,
    pub generator: String,
    pub files: Vec<FileFacts>,
    pub findings: Vec<RunFinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileFacts {
    pub name: String,
    /// Discipline code, e.g. `STR`.
    pub discipline: String,
    pub size_bytes: u64,
    pub ifc_schema: String,
    /// SHA-256 of the file.
    pub hash: String,
    /// Last change of the file, local time.
    pub modified: String,
    pub object_count: usize,
    pub objects_by_class: BTreeMap<String, usize>,
    /// Elements missing information, when an information rule ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_properties: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunFinding {
    /// Stable between runs: the same problem keeps its id.
    pub id: String,
    pub ruleset: String,
    pub rule: String,
    /// `clash`, `clearance`, `duplicate`, `free_zone`, `information` or `script`.
    pub check_type: String,
    /// Discipline codes of the two sides (the same twice for one element).
    pub disciplines: [String; 2],
    pub objects: Vec<RunObject>,
    pub severity: String,
    /// Measured value in mm (depth, distance, blocked length), if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunObject {
    pub global_id: String,
    pub class: String,
    pub name: String,
    pub file: String,
}

/// The check type a finding counts under in the dashboard.
pub fn check_type(kind: FindingKind) -> &'static str {
    match kind {
        FindingKind::Clash | FindingKind::Inside => "clash",
        FindingKind::Clearance => "clearance",
        FindingKind::Duplicate => "duplicate",
        FindingKind::FreeZone => "free_zone",
        FindingKind::MissingInformation => "information",
        FindingKind::Script => "script",
    }
}

/// Figures for one file.
pub fn file_facts(
    model: &IfcModel,
    name: &str,
    size_bytes: u64,
    hash: &str,
    modified: &str,
    missing_properties: Option<usize>,
) -> FileFacts {
    let objects: Vec<&crate::Element> =
        model.elements.iter().filter(|e| e.has_geometry()).collect();
    let mut by_class = BTreeMap::new();
    for e in &objects {
        *by_class.entry(e.ifc_class.clone()).or_insert(0) += 1;
    }
    FileFacts {
        name: name.to_owned(),
        discipline: model.discipline.code().to_owned(),
        size_bytes,
        ifc_schema: model.schema.clone(),
        hash: hash.to_owned(),
        modified: modified.to_owned(),
        object_count: objects.len(),
        objects_by_class: by_class,
        missing_properties,
    }
}

/// The findings of a check report, with each model's discipline code and
/// file name from `model_info`.
pub fn run_findings(
    report: &CheckReport,
    model_info: &dyn Fn(ModelId) -> (String, String),
) -> Vec<RunFinding> {
    let mut out = Vec::new();
    for ruleset in &report.rulesets {
        for rule in &ruleset.rules {
            for f in &rule.findings {
                let (da, fa) = model_info(f.a.model);
                let (db, fb) = model_info(f.b.model);
                let mut objects = vec![RunObject {
                    global_id: f.a.global_id.clone(),
                    class: f.a.ifc_class.clone(),
                    name: f.a.name.clone(),
                    file: fa,
                }];
                if !f.is_single() {
                    objects.push(RunObject {
                        global_id: f.b.global_id.clone(),
                        class: f.b.ifc_class.clone(),
                        name: f.b.name.clone(),
                        file: fb,
                    });
                }
                let value = match f.kind {
                    FindingKind::MissingInformation => None,
                    FindingKind::Script if f.magnitude == 0.0 => None,
                    _ => Some((f64::from(f.magnitude) * 1000.0).round()),
                };
                out.push(RunFinding {
                    id: f.id.clone(),
                    ruleset: ruleset.info.id.clone(),
                    rule: rule.rule.id.clone(),
                    check_type: check_type(f.kind).to_owned(),
                    disciplines: [da, db],
                    objects,
                    severity: rule.rule.severity.label().to_lowercase(),
                    value,
                });
            }
        }
    }
    out
}

/// Elements with missing information per file name.
pub fn missing_per_file(findings: &[RunFinding]) -> BTreeMap<String, usize> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for f in findings.iter().filter(|f| f.check_type == "information") {
        if let Some(o) = f.objects.first() {
            seen.insert((o.file.clone(), o.global_id.clone()));
        }
    }
    let mut out = BTreeMap::new();
    for (file, _) in seen {
        *out.entry(file).or_insert(0) += 1;
    }
    out
}

/// What the dashboard shows, worked out from the runs, oldest first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DashboardData {
    pub name: String,
    pub generated: String,
    pub runs: Vec<DashboardRun>,
    /// Per run, check type and pair of disciplines: findings, and how many
    /// are new or solved since the run before.
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DashboardRun {
    pub id: String,
    pub machine: String,
    pub files: Vec<FileFacts>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cell {
    /// Index into `runs`.
    pub run: usize,
    pub check_type: String,
    /// Discipline codes, sorted.
    pub a: String,
    pub b: String,
    pub count: usize,
    pub new: usize,
    pub solved: usize,
}

type CellKey = (String, String, String);

fn cell_key(f: &RunFinding) -> CellKey {
    let [x, y] = &f.disciplines;
    let (a, b) = if x <= y { (x, y) } else { (y, x) };
    (f.check_type.clone(), a.clone(), b.clone())
}

pub fn dashboard_data(name: &str, generated: &str, runs: &[RunRecord]) -> DashboardData {
    let mut cells = Vec::new();
    let mut previous: BTreeMap<CellKey, BTreeSet<&str>> = BTreeMap::new();
    for (index, run) in runs.iter().enumerate() {
        let mut current: BTreeMap<CellKey, BTreeSet<&str>> = BTreeMap::new();
        for f in &run.findings {
            current
                .entry(cell_key(f))
                .or_default()
                .insert(f.id.as_str());
        }
        let keys: BTreeSet<&CellKey> = current.keys().chain(previous.keys()).collect();
        for key in keys {
            let now = current.get(key);
            let before = previous.get(key);
            let count = now.map_or(0, BTreeSet::len);
            // The first run has nothing to compare with.
            let (new, solved) = if index == 0 {
                (0, 0)
            } else {
                (
                    now.map_or(0, |n| {
                        n.iter()
                            .filter(|id| before.is_none_or(|b| !b.contains(*id)))
                            .count()
                    }),
                    before.map_or(0, |b| {
                        b.iter()
                            .filter(|id| now.is_none_or(|n| !n.contains(*id)))
                            .count()
                    }),
                )
            };
            cells.push(Cell {
                run: index,
                check_type: key.0.clone(),
                a: key.1.clone(),
                b: key.2.clone(),
                count,
                new,
                solved,
            });
        }
        previous = current;
    }
    DashboardData {
        name: name.to_owned(),
        generated: generated.to_owned(),
        runs: runs
            .iter()
            .map(|r| DashboardRun {
                id: r.run_id.clone(),
                machine: r.machine.clone(),
                files: r.files.clone(),
            })
            .collect(),
        cells,
    }
}

/// The dashboard: one self-contained HTML page with the data inside, so it
/// opens from a network drive in any browser.
pub fn dashboard_html(data: &DashboardData) -> String {
    let json = serde_json::to_string(data).expect("dashboard data always serialises");
    // Keep the data from closing the script element.
    let json = json.replace("</", "<\\/");
    DASHBOARD_TEMPLATE
        .replace("{{TITLE}}", &html_escape(&data.name))
        .replace("/*DATA*/null", &json)
}

const DASHBOARD_TEMPLATE: &str = include_str!("dashboard.html");

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(id: &str, kind: &str, a: &str, b: &str) -> RunFinding {
        RunFinding {
            id: id.to_owned(),
            ruleset: "r".to_owned(),
            rule: "x".to_owned(),
            check_type: kind.to_owned(),
            disciplines: [a.to_owned(), b.to_owned()],
            objects: Vec::new(),
            severity: "critical".to_owned(),
            value: None,
        }
    }

    fn run(id: &str, findings: Vec<RunFinding>) -> RunRecord {
        RunRecord {
            format: FORMAT,
            run_id: id.to_owned(),
            machine: "PC".to_owned(),
            generator: "test".to_owned(),
            files: Vec::new(),
            findings,
        }
    }

    #[test]
    fn new_and_solved_follow_the_ids() {
        let runs = vec![
            run(
                "2026-10-01T06:00:00",
                vec![
                    finding("1", "clash", "STR", "VEN"),
                    finding("2", "clash", "VEN", "STR"),
                ],
            ),
            run(
                "2026-10-02T06:00:00",
                vec![
                    finding("2", "clash", "STR", "VEN"),
                    finding("3", "clash", "STR", "VEN"),
                    finding("4", "clash", "STR", "VEN"),
                ],
            ),
        ];
        let data = dashboard_data("P", "now", &runs);
        let cell = |run: usize| {
            data.cells
                .iter()
                .find(|c| c.run == run && c.a == "STR" && c.b == "VEN")
                .unwrap()
        };
        // Both disciplines orders count in one cell.
        assert_eq!(cell(0).count, 2);
        assert_eq!((cell(0).new, cell(0).solved), (0, 0));
        assert_eq!(cell(1).count, 3);
        assert_eq!((cell(1).new, cell(1).solved), (2, 1));
    }

    #[test]
    fn a_cell_that_empties_shows_what_was_solved() {
        let runs = vec![
            run("1", vec![finding("1", "clearance", "ELE", "VEN")]),
            run("2", Vec::new()),
        ];
        let data = dashboard_data("P", "now", &runs);
        let last = data.cells.iter().find(|c| c.run == 1).unwrap();
        assert_eq!((last.count, last.new, last.solved), (0, 0, 1));
    }

    #[test]
    fn config_round_trips_and_the_page_carries_the_data() {
        let mut config = AutorunConfig::new("Test");
        config.source = r"\\server\share\ifc".to_owned();
        config.files.push(SourceFile {
            name: "KON.ifc".to_owned(),
            discipline: Discipline::Structural,
        });
        let back = AutorunConfig::from_json(&config.to_json()).unwrap();
        assert_eq!(back, config);
        assert_eq!(back.discipline_of("kon.IFC"), Some(Discipline::Structural));

        let data = dashboard_data("A <b> project", "now", &[run("1", Vec::new())]);
        let html = dashboard_html(&data);
        assert!(html.contains("A &lt;b&gt; project"));
        assert!(!html.contains("/*DATA*/null"));
    }
}
