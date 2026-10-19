//! Selected issues: findings the reviewer has picked out to be handled.
//!
//! The fields follow the BCF 3.0 topic model (guid, title, description,
//! status, priority, labels, components, viewpoint) so issues can later be
//! exported as `.bcfzip`, sent to a BCF API such as Dalux, and collected in
//! reports without conversion losses.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

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
}

/// Camera of the viewpoint, in world coordinates (metres).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct IssueCamera {
    pub eye: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
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
    #[serde(default)]
    pub source: Option<IssueSource>,
    pub components: Vec<IssueComponent>,
    /// Centre of the problem in world coordinates.
    pub point: [f64; 3],
    #[serde(default)]
    pub camera: Option<IssueCamera>,
}

impl Issue {
    pub fn touch(&mut self) {
        self.modified = now_rfc3339();
    }
}

#[derive(Debug, Default)]
pub struct Issues {
    pub list: Vec<Issue>,
    pub selected: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedIssues {
    issues: Vec<Issue>,
}

impl Issues {
    pub fn get(&self, guid: &str) -> Option<&Issue> {
        self.list.iter().find(|i| i.guid == guid)
    }

    pub fn get_mut(&mut self, guid: &str) -> Option<&mut Issue> {
        self.list.iter_mut().find(|i| i.guid == guid)
    }

    pub fn for_finding(&self, finding: &str) -> Option<&Issue> {
        self.list
            .iter()
            .find(|i| i.source.as_ref().is_some_and(|s| s.finding == finding))
    }

    pub fn remove(&mut self, guid: &str) {
        self.list.retain(|i| i.guid != guid);
        if self.selected.as_deref() == Some(guid) {
            self.selected = None;
        }
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(SavedIssues {
            issues: self.list.clone(),
        })
        .expect("issues always serialise")
    }

    pub fn load(json: Option<serde_json::Value>) -> Self {
        let list = json
            .and_then(|j| serde_json::from_value::<SavedIssues>(j).ok())
            .map(|s| s.issues)
            .unwrap_or_default();
        Self {
            list,
            selected: None,
        }
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
            source: None,
            components: vec![],
            point: [1.0, 2.0, 3.0],
            camera: None,
        };
        let issues = Issues {
            list: vec![issue.clone()],
            selected: None,
        };
        let loaded = Issues::load(Some(issues.to_json()));
        assert_eq!(loaded.list, vec![issue]);
    }
}
