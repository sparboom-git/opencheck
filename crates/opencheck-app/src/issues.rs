//! Selected issues: findings the reviewer has picked out to be handled.
//!
//! The fields follow the BCF 3.0 topic model (guid, title, description,
//! status, priority, labels, components, viewpoint) so issues can later be
//! exported as `.bcfzip`, sent to a BCF API such as Dalux, and collected in
//! reports without conversion losses.

use std::sync::atomic::{AtomicU64, Ordering};
use web_time::{SystemTime, UNIX_EPOCH};

use opencheck_core::Discipline;
use opencheck_core::check::Severity;
use opencheck_core::project::now_rfc3339;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueStatus {
    Open,
    InProgress,
    Resolved,
    Closed,
}

impl IssueStatus {
    pub const ALL: [IssueStatus; 4] = [
        IssueStatus::Open,
        IssueStatus::InProgress,
        IssueStatus::Resolved,
        IssueStatus::Closed,
    ];

    pub fn label(self) -> &'static str {
        match self {
            IssueStatus::Open => "Open",
            IssueStatus::InProgress => "In progress",
            IssueStatus::Resolved => "Resolved",
            IssueStatus::Closed => "Closed",
        }
    }
}

/// Which side of the finding a component was on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentRole {
    A,
    B,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueComponent {
    pub global_id: String,
    pub name: String,
    pub ifc_class: String,
    /// Short name of the model, to tell models apart when GlobalIds repeat.
    pub model: String,
    pub role: ComponentRole,
    /// The authoring tool's element id (Revit's Element ID, from the IFC
    /// `Tag`), to find the object again in the source model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tag: String,
}

/// The authoring tool's id of an element: the IFC `Tag` (Revit writes its
/// Element ID there), or the number Revit appends to names (`Family:Type:123`).
pub fn element_id(element: &opencheck_core::Element) -> String {
    if let Some(tag) = element.tag.as_deref().map(str::trim)
        && !tag.is_empty()
    {
        return tag.to_owned();
    }
    element
        .name
        .rsplit_once(':')
        .map(|(_, id)| id.trim())
        .filter(|id| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or_default()
        .to_owned()
}

/// Camera of the viewpoint, in world coordinates (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IssueCamera {
    pub eye: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
}

/// Horizontal section of the viewpoint, world coordinates (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IssueSection {
    pub height: f64,
    pub anchor: [f64; 3],
}

/// An element by GlobalId, with its model to tell repeated GlobalIds apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueElement {
    pub global_id: String,
    pub model: String,
}

/// The check result an issue was created from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IssueSource {
    pub finding: String,
    pub ruleset: String,
    pub rule: String,
    pub kind: String,
    /// Penetration depth, distance or offset in millimetres.
    pub magnitude_mm: f32,
    /// Stable ids of the ruleset and rule; empty in files from before model updates.
    #[serde(default)]
    pub ruleset_id: String,
    #[serde(default)]
    pub rule_id: String,
    /// The model versions the finding was seen in.
    #[serde(default)]
    pub revisions: Vec<ModelRevision>,
    /// Set when a check after a model update no longer finds it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solved_by_update: Option<SolvedByUpdate>,
    /// Missing information: the items that were missing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub details: Vec<String>,
}

/// A model, by short name, at a revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRevision {
    pub model: String,
    pub revision: u32,
}

/// When a finding disappeared, and which newer model versions did it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolvedByUpdate {
    pub date: String,
    pub models: Vec<ModelRevision>,
}

/// What a finished check run knows, for [`Issues::apply_run`].
pub struct RunContext<'a> {
    /// The finding is in the latest results.
    pub present: &'a dyn Fn(&str) -> bool,
    /// The finding's rule ran on all of its models in the latest run.
    pub checked: &'a dyn Fn(&IssueSource) -> bool,
    /// Current revision of a model by short name; None when it is not loaded.
    pub revision: &'a dyn Fn(&str) -> Option<u32>,
}

/// How many issue findings a run solved or brought back.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunOutcome {
    pub solved: usize,
    pub reopened: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub guid: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub status: IssueStatus,
    pub priority: Severity,
    pub created: String,
    pub modified: String,
    #[serde(default)]
    pub labels: Vec<String>,
    /// The check results bundled in this issue; one or more.
    #[serde(default)]
    pub findings: Vec<IssueSource>,
    /// Single finding of the first file version; moved into `findings` on load.
    #[serde(default, skip_serializing)]
    pub source: Option<IssueSource>,
    pub components: Vec<IssueComponent>,
    /// Centre of the problem in world coordinates.
    pub point: [f64; 3],
    #[serde(default)]
    pub camera: Option<IssueCamera>,
    /// Section plane of the viewpoint, if the model was cut.
    #[serde(default)]
    pub section: Option<IssueSection>,
    /// The rest of the model was shown (true) or ghosted around the components.
    #[serde(default)]
    pub show_context: bool,
    /// Elements hidden in the viewpoint.
    #[serde(default)]
    pub hidden: Vec<IssueElement>,
    /// Every finding disappeared after model updates; set together with Resolved.
    #[serde(default)]
    pub solved_by_update: bool,
    /// Disciplines responsible for solving the issue. New issues get the
    /// disciplines of the models their findings come from.
    #[serde(default)]
    pub responsible: Vec<Discipline>,
    /// Person the issue is assigned to on the BCF server (its `assigned_to`,
    /// usually an e-mail address); empty when not assigned.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub assigned_to: String,
    /// The topic on the BCF server, once the issue has been sent there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bcf: Option<BcfTopicLink>,
}

/// Where an issue lives on the BCF server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BcfTopicLink {
    pub project_id: String,
    pub topic_guid: String,
    /// When OpenCheck last sent or fetched the topic.
    pub synced: String,
    /// The topic's status and assignee on the server at that time.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_status: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remote_assigned_to: String,
    /// The issue's `modified` time when it was last sent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sent_version: String,
}

/// The BCF project the issues are shared through. Stored in `issues.json`;
/// sign-in tokens are not, they stay with the user.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BcfProjectLink {
    /// Server address as the user entered it, e.g. `https://node1.field.dalux.com/service/`.
    pub server: String,
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    /// Who gets issues of a discipline when an issue has no assignee.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignees: Vec<DisciplineAssignee>,
    /// How OpenCheck's values become the server's values.
    #[serde(default, skip_serializing_if = "BcfMapping::is_empty")]
    pub mapping: BcfMapping,
}

/// The server's value for each of OpenCheck's values. Servers name things
/// their own way (Dalux: "Aktiv", "Lukkede", "xyz@example.com | Role"), so the
/// user pairs them once per project, as in Solibri. Empty entries fall back
/// to matching by name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BcfMapping {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub status: Vec<ValueMap<IssueStatus>>,
    /// Criticality (C1–C3) to the server's priority.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub priority: Vec<ValueMap<Severity>>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub topic_type: String,
    /// Phase or kind of check ("stage" in BCF), e.g. "Tværfaglig kollisionskontrol".
    #[serde(skip_serializing_if = "String::is_empty")]
    pub stage: String,
    /// Initials or names used in OpenCheck to the server's people.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub people: Vec<ValueMap<String>>,
    /// The user has looked at and saved the mapping.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueMap<T> {
    pub from: T,
    pub to: String,
}

impl BcfMapping {
    pub fn is_empty(&self) -> bool {
        *self == BcfMapping::default()
    }

    fn get<'a, T: PartialEq>(list: &'a [ValueMap<T>], from: &T) -> Option<&'a str> {
        list.iter()
            .find(|m| m.from == *from && !m.to.is_empty())
            .map(|m| m.to.as_str())
    }

    fn set<T: PartialEq>(list: &mut Vec<ValueMap<T>>, from: T, to: String) {
        list.retain(|m| m.from != from);
        if !to.is_empty() {
            list.push(ValueMap { from, to });
        }
    }

    pub fn status_for(&self, status: IssueStatus) -> Option<&str> {
        Self::get(&self.status, &status)
    }

    pub fn set_status(&mut self, status: IssueStatus, to: String) {
        Self::set(&mut self.status, status, to);
    }

    /// OpenCheck's status for a server status, by the mapping.
    pub fn status_from(&self, remote: &str) -> Option<IssueStatus> {
        self.status.iter().find(|m| m.to == remote).map(|m| m.from)
    }

    pub fn priority_for(&self, priority: Severity) -> Option<&str> {
        Self::get(&self.priority, &priority)
    }

    pub fn set_priority(&mut self, priority: Severity, to: String) {
        Self::set(&mut self.priority, priority, to);
    }

    pub fn person_for(&self, person: &str) -> Option<&str> {
        self.people
            .iter()
            .find(|m| m.from.eq_ignore_ascii_case(person) && !m.to.is_empty())
            .map(|m| m.to.as_str())
    }

    /// OpenCheck's name for a server person, by the mapping.
    pub fn person_from(&self, remote: &str) -> Option<&str> {
        self.people
            .iter()
            .find(|m| m.to == remote)
            .map(|m| m.from.as_str())
    }

    pub fn set_person(&mut self, person: String, to: String) {
        self.people
            .retain(|m| !m.from.eq_ignore_ascii_case(&person));
        if !to.is_empty() {
            self.people.push(ValueMap { from: person, to });
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisciplineAssignee {
    pub discipline: Discipline,
    pub user: String,
}

impl BcfProjectLink {
    pub fn assignee_for(&self, discipline: Discipline) -> Option<&str> {
        self.assignees
            .iter()
            .find(|a| a.discipline == discipline && !a.user.is_empty())
            .map(|a| a.user.as_str())
    }
}

impl Issue {
    /// The person to assign on the BCF server: the issue's own assignee, or
    /// the default person of its first responsible discipline.
    pub fn effective_assignee<'a>(&'a self, link: Option<&'a BcfProjectLink>) -> Option<&'a str> {
        if !self.assigned_to.is_empty() {
            return Some(&self.assigned_to);
        }
        let link = link?;
        self.responsible.iter().find_map(|d| link.assignee_for(*d))
    }

    /// Adds disciplines not yet responsible, keeping the order.
    pub fn add_responsible(&mut self, disciplines: impl IntoIterator<Item = Discipline>) {
        for d in disciplines {
            if !self.responsible.contains(&d) {
                self.responsible.push(d);
            }
        }
    }

    /// Changed since it was last sent to the BCF server.
    pub fn needs_sending(&self) -> bool {
        self.bcf
            .as_ref()
            .is_none_or(|link| link.sent_version != self.modified)
    }
}

impl Issue {
    pub fn touch(&mut self) {
        self.modified = now_rfc3339();
    }

    /// Findings marked solved by a model update.
    pub fn solved_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.solved_by_update.is_some())
            .count()
    }

    pub fn contains_finding(&self, finding: &str) -> bool {
        self.findings.iter().any(|f| f.finding == finding)
    }

    /// Adds components not yet in the issue (same GlobalId and side).
    pub fn add_components(&mut self, components: impl IntoIterator<Item = IssueComponent>) {
        for c in components {
            let known = self
                .components
                .iter()
                .any(|k| k.global_id == c.global_id && k.role == c.role);
            if !known {
                self.components.push(c);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct Issues {
    pub list: Vec<Issue>,
    pub selected: Option<String>,
    /// Issues ticked in the list, for merging.
    pub marked: Vec<String>,
    /// The BCF project the issues are shared through.
    pub bcf: Option<BcfProjectLink>,
    /// Assignees used before (initials or e-mail), offered again.
    pub people: Vec<String>,
    /// Header fields of the issue report.
    pub report: ReportSettings,
}

/// What the issue report says about itself: who made it, for which project
/// and phase, and general remarks. Saved with the project.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportSettings {
    pub project: String,
    pub author: String,
    pub company: String,
    pub revision: String,
    /// Own discipline(s) the report covers.
    pub disciplines: String,
    pub phase: String,
    /// How the check was done, besides OpenCheck.
    pub method: String,
    pub remarks: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedIssues {
    issues: Vec<Issue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bcf: Option<BcfProjectLink>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    people: Vec<String>,
    #[serde(skip_serializing_if = "is_default_report")]
    report: ReportSettings,
}

fn is_default_report(r: &ReportSettings) -> bool {
    *r == ReportSettings::default()
}

impl Issues {
    pub fn get(&self, guid: &str) -> Option<&Issue> {
        self.list.iter().find(|i| i.guid == guid)
    }

    pub fn get_mut(&mut self, guid: &str) -> Option<&mut Issue> {
        self.list.iter_mut().find(|i| i.guid == guid)
    }

    pub fn for_finding(&self, finding: &str) -> Option<&Issue> {
        self.list.iter().find(|i| i.contains_finding(finding))
    }

    pub fn remove(&mut self, guid: &str) {
        self.list.retain(|i| i.guid != guid);
        self.marked.retain(|m| m != guid);
        if self.selected.as_deref() == Some(guid) {
            self.selected = None;
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(SavedIssues {
            issues: self.list.clone(),
            bcf: self.bcf.clone(),
            people: self.people.clone(),
            report: self.report.clone(),
        })
        .expect("issues always serialise")
    }

    pub fn load(json: Option<serde_json::Value>) -> Self {
        let saved = json
            .and_then(|j| serde_json::from_value::<SavedIssues>(j).ok())
            .unwrap_or_default();
        let bcf = saved.bcf;
        let report = saved.report;
        let mut people = saved.people;
        let mut list = saved.issues;
        for issue in &mut list {
            if let Some(source) = issue.source.take() {
                issue.findings.push(source);
            }
            // Files from before model updates: every model was at revision 1.
            let mut models: Vec<String> =
                issue.components.iter().map(|c| c.model.clone()).collect();
            models.sort();
            models.dedup();
            for source in issue.findings.iter_mut().filter(|f| f.revisions.is_empty()) {
                source.revisions = models
                    .iter()
                    .map(|model| ModelRevision {
                        model: model.clone(),
                        revision: 1,
                    })
                    .collect();
            }
        }
        for issue in &list {
            remember_person(&mut people, &issue.assigned_to);
        }
        Self {
            list,
            selected: None,
            marked: Vec::new(),
            bcf,
            people,
            report,
        }
    }

    /// Compares the issues with a finished check run. A finding that is gone,
    /// although its rule ran on the same models and at least one of them has
    /// a newer revision, is marked solved by update; it is never deleted.
    /// An issue whose findings are all solved becomes Resolved. A solved
    /// finding that shows up again is unmarked and its issue reopened.
    pub fn apply_run(&mut self, run: &RunContext, now: &str) -> RunOutcome {
        let mut outcome = RunOutcome::default();
        for issue in &mut self.list {
            let mut changed = false;
            for source in &mut issue.findings {
                if (run.present)(&source.finding) {
                    if source.solved_by_update.take().is_some() {
                        outcome.reopened += 1;
                        changed = true;
                    }
                    continue;
                }
                if source.solved_by_update.is_some() || !(run.checked)(source) {
                    continue;
                }
                let newer: Vec<ModelRevision> = source
                    .revisions
                    .iter()
                    .filter_map(|seen| {
                        let current = (run.revision)(&seen.model)?;
                        (current > seen.revision).then(|| ModelRevision {
                            model: seen.model.clone(),
                            revision: current,
                        })
                    })
                    .collect();
                if !newer.is_empty() {
                    source.solved_by_update = Some(SolvedByUpdate {
                        date: now.to_owned(),
                        models: newer,
                    });
                    outcome.solved += 1;
                    changed = true;
                }
            }
            if !changed {
                continue;
            }
            let all_solved = !issue.findings.is_empty()
                && issue.findings.iter().all(|f| f.solved_by_update.is_some());
            if all_solved && !issue.solved_by_update {
                issue.solved_by_update = true;
                if issue.status != IssueStatus::Closed {
                    issue.status = IssueStatus::Resolved;
                }
            } else if !all_solved && issue.solved_by_update {
                // Something came back: the update did not solve it after all.
                issue.solved_by_update = false;
                if issue.status == IssueStatus::Resolved {
                    issue.status = IssueStatus::Open;
                }
            }
            issue.modified = now.to_owned();
        }
        outcome
    }

    /// Folds the other issues into the first one in `guids` and returns it.
    /// Findings and components are combined; the highest priority wins.
    pub fn merge(&mut self, guids: &[String]) -> Option<String> {
        let (target, rest) = guids.split_first()?;
        let others: Vec<Issue> = self
            .list
            .iter()
            .filter(|i| rest.contains(&i.guid))
            .cloned()
            .collect();
        let issue = self.get_mut(target)?;
        for other in &others {
            for f in &other.findings {
                if !issue.contains_finding(&f.finding) {
                    issue.findings.push(f.clone());
                }
            }
            issue.add_components(other.components.iter().cloned());
            issue.priority = issue.priority.min(other.priority);
            issue.add_responsible(other.responsible.iter().copied());
            if !other.description.trim().is_empty() {
                if !issue.description.is_empty() {
                    issue.description.push_str(
                        "

",
                    );
                }
                issue.description.push_str(&other.description);
            }
        }
        issue.solved_by_update =
            issue.solved_by_update && issue.findings.iter().all(|f| f.solved_by_update.is_some());
        issue.touch();
        self.list.retain(|i| !rest.contains(&i.guid));
        self.marked.clear();
        self.selected = Some(target.clone());
        Some(target.clone())
    }
}

/// Adds an assignee to the list offered in other issues.
pub fn remember_person(people: &mut Vec<String>, person: &str) {
    let person = person.trim();
    if !person.is_empty() && !people.iter().any(|p| p.eq_ignore_ascii_case(person)) {
        people.push(person.to_owned());
        people.sort_by_key(|p| p.to_lowercase());
    }
}

/// A random-enough UUID (version 4 layout) without an extra dependency.
pub fn new_guid(seed: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let hash = Sha256::digest(format!("{nanos}\n{n}\n{seed}\n{}", std::process::id()).as_bytes());
    let mut b = [0u8; 16];
    b.copy_from_slice(&hash[..16]);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_are_unique_and_uuid_shaped() {
        let a = new_guid("x");
        let b = new_guid("x");
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        assert_eq!(&a[14..15], "4");
    }

    #[test]
    fn issues_round_trip() {
        let issue = Issue {
            guid: new_guid("t"),
            title: "Duct vs beam".into(),
            description: String::new(),
            status: IssueStatus::Open,
            priority: Severity::Critical,
            created: now_rfc3339(),
            modified: now_rfc3339(),
            labels: vec![],
            findings: vec![],
            source: None,
            components: vec![],
            point: [1.0, 2.0, 3.0],
            camera: None,
            section: None,
            show_context: false,
            hidden: vec![],
            solved_by_update: false,
            responsible: vec![],
            assigned_to: String::new(),
            bcf: None,
        };
        let issues = Issues {
            list: vec![issue.clone()],
            ..Default::default()
        };
        let loaded = Issues::load(Some(issues.to_json()));
        assert_eq!(loaded.list, vec![issue]);
    }
}

#[cfg(test)]
mod bundle_tests {
    use super::*;

    fn source(finding: &str) -> IssueSource {
        IssueSource {
            finding: finding.into(),
            ruleset: "R".into(),
            rule: "r".into(),
            kind: "Clash".into(),
            magnitude_mm: 10.0,
            ruleset_id: "rs".into(),
            rule_id: "r1".into(),
            revisions: vec![
                ModelRevision {
                    model: "VENT".into(),
                    revision: 1,
                },
                ModelRevision {
                    model: "KON".into(),
                    revision: 1,
                },
            ],
            solved_by_update: None,
            details: vec![],
        }
    }

    fn issue(guid: &str, findings: &[&str], priority: Severity) -> Issue {
        Issue {
            guid: guid.into(),
            title: guid.into(),
            description: format!("about {guid}"),
            status: IssueStatus::Open,
            priority,
            created: now_rfc3339(),
            modified: now_rfc3339(),
            labels: vec![],
            findings: findings.iter().map(|f| source(f)).collect(),
            source: None,
            components: vec![IssueComponent {
                global_id: format!("g-{guid}"),
                name: String::new(),
                ifc_class: "IfcBeam".into(),
                model: "KON".into(),
                role: ComponentRole::B,
                tag: String::new(),
            }],
            point: [0.0; 3],
            camera: None,
            section: None,
            show_context: false,
            hidden: vec![],
            solved_by_update: false,
            responsible: vec![],
            assigned_to: String::new(),
            bcf: None,
        }
    }

    #[test]
    fn merge_combines_findings_components_and_priority() {
        let mut issues = Issues {
            list: vec![
                issue("one", &["f1", "f2"], Severity::Low),
                issue("two", &["f2", "f3"], Severity::Critical),
                issue("three", &["f4"], Severity::Moderate),
            ],
            ..Default::default()
        };
        let kept = issues.merge(&["one".into(), "two".into()]).unwrap();
        assert_eq!(kept, "one");
        assert_eq!(issues.list.len(), 2);
        let merged = issues.get("one").unwrap();
        let ids: Vec<&str> = merged.findings.iter().map(|f| f.finding.as_str()).collect();
        assert_eq!(ids, ["f1", "f2", "f3"]);
        assert_eq!(merged.components.len(), 2);
        assert_eq!(merged.priority, Severity::Critical);
        assert!(merged.description.contains("about two"));
        assert!(issues.for_finding("f3").is_some());
    }

    #[test]
    fn single_finding_issues_from_older_files_are_read() {
        let mut old = issue("old", &[], Severity::Low);
        old.source = Some(source("f9"));
        let json = serde_json::json!({ "issues": [serde_json::to_value(&old).unwrap()] });
        // `source` is not written any more, so put it back as an older file had it.
        let mut json = json;
        json["issues"][0]["source"] = serde_json::to_value(source("f9")).unwrap();
        let loaded = Issues::load(Some(json));
        assert_eq!(loaded.list[0].findings.len(), 1);
        assert!(loaded.for_finding("f9").is_some());
    }

    fn run_with<'a>(
        present: &'a dyn Fn(&str) -> bool,
        checked: &'a dyn Fn(&IssueSource) -> bool,
        revision: &'a dyn Fn(&str) -> Option<u32>,
    ) -> RunContext<'a> {
        RunContext {
            present,
            checked,
            revision,
        }
    }

    #[test]
    fn findings_gone_after_an_update_are_solved_not_deleted() {
        let mut issues = Issues {
            list: vec![issue("one", &["f1", "f2"], Severity::Low)],
            ..Default::default()
        };
        let checked = |_: &IssueSource| true;
        let vent_updated = |m: &str| Some(if m == "VENT" { 2 } else { 1 });

        // f1 is gone after VENT went to revision 2; f2 is still there.
        let present = |f: &str| f == "f2";
        let outcome = issues.apply_run(&run_with(&present, &checked, &vent_updated), "t1");
        assert_eq!(
            outcome,
            RunOutcome {
                solved: 1,
                reopened: 0
            }
        );
        let one = issues.get("one").unwrap();
        assert_eq!(one.findings.len(), 2);
        let solved = one.findings[0].solved_by_update.as_ref().unwrap();
        assert_eq!(
            solved.models,
            [ModelRevision {
                model: "VENT".into(),
                revision: 2
            }]
        );
        assert_eq!(one.status, IssueStatus::Open);
        assert!(!one.solved_by_update);

        // Now f2 is gone too: the whole issue is solved by update.
        let present = |_: &str| false;
        issues.apply_run(&run_with(&present, &checked, &vent_updated), "t2");
        let one = issues.get("one").unwrap();
        assert!(one.solved_by_update);
        assert_eq!(one.status, IssueStatus::Resolved);
        assert_eq!(
            one.findings[0].solved_by_update.as_ref().unwrap().date,
            "t1"
        );

        // The flags survive saving.
        let reloaded = Issues::load(Some(issues.to_json()));
        assert_eq!(reloaded.list, issues.list);

        // f1 comes back in a later version: the issue is reopened.
        let present = |f: &str| f == "f1";
        let outcome = issues.apply_run(&run_with(&present, &checked, &vent_updated), "t3");
        assert_eq!(outcome.reopened, 1);
        let one = issues.get("one").unwrap();
        assert!(!one.solved_by_update);
        assert_eq!(one.status, IssueStatus::Open);
        assert!(one.findings[0].solved_by_update.is_none());
        assert!(one.findings[1].solved_by_update.is_some());
    }

    #[test]
    fn missing_findings_without_an_update_or_check_stay_open() {
        let mut issues = Issues {
            list: vec![issue("one", &["f1"], Severity::Low)],
            ..Default::default()
        };
        let absent = |_: &str| false;
        let unchanged = |_: &str| Some(1);
        let updated = |_: &str| Some(3);
        let checked = |_: &IssueSource| true;
        let not_checked = |_: &IssueSource| false;

        // Gone, but no model changed (e.g. the rule was edited): not an update.
        let outcome = issues.apply_run(&run_with(&absent, &checked, &unchanged), "t");
        assert_eq!(outcome, RunOutcome::default());
        // Gone, but the rule did not run on these models this time.
        let outcome = issues.apply_run(&run_with(&absent, &not_checked, &updated), "t");
        assert_eq!(outcome, RunOutcome::default());
        assert_eq!(issues.get("one").unwrap().solved_count(), 0);
    }
}
