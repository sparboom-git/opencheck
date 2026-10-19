//! The open project: where it lives, whether it has unsaved changes, and the
//! view state stored in its `view.json`.

use std::path::PathBuf;

use glam::{DVec3, Vec3};
use opencheck_core::project::{ModelEntry, ModelSource};
use opencheck_core::{Federation, ModelId};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::measure::{MeasureKind, Measurement};
use crate::scene::{ElementRef, SectionPlane, ViewState};

/// Where a loaded model came from and its project metadata.
#[derive(Debug, Clone)]
pub struct ModelRecord {
    pub source: ModelSource,
    pub entry: ModelEntry,
}

#[derive(Debug, Default)]
pub struct Session {
    /// None until the project has been saved or opened.
    pub path: Option<PathBuf>,
    pub created: Option<String>,
    pub records: FxHashMap<ModelId, ModelRecord>,
    /// Model list or settings changed since the last save.
    pub dirty: bool,
    /// View state waiting for the project's models to finish loading.
    pub pending_view: Option<serde_json::Value>,
}

impl Session {
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_owned());
        let marker = if self.dirty { " •" } else { "" };
        format!("{name}{marker} — OpenCheck")
    }
}

/// An element by model position and GlobalId, stable across sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedElement {
    model: usize,
    global_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedCamera {
    /// World coordinates (metres), independent of the federation origin.
    eye: [f64; 3],
    yaw: f32,
    pitch: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedSection {
    height: f64,
    anchor: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedMeasurement {
    a: [f64; 3],
    b: [f64; 3],
    kind: MeasureKind,
}

/// Contents of `view.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct SavedView {
    camera: Option<SavedCamera>,
    xray: bool,
    show_spaces: bool,
    storey_filter: Option<String>,
    section: Option<SavedSection>,
    hidden: Vec<SavedElement>,
    isolated: Option<Vec<SavedElement>>,
    measurements: Vec<SavedMeasurement>,
}

fn to_world(origin: DVec3, p: Vec3) -> [f64; 3] {
    (origin + p.as_dvec3()).to_array()
}

fn from_world(origin: DVec3, p: [f64; 3]) -> Vec3 {
    (DVec3::from_array(p) - origin).as_vec3()
}

pub fn capture_view(
    federation: &Federation,
    view: &ViewState,
    camera: &Camera,
    measurements: &[Measurement],
) -> serde_json::Value {
    let origin = federation.origin();
    let ids = federation.ids();
    let save = |e: &ElementRef| -> Option<SavedElement> {
        let model = ids.iter().position(|id| *id == e.model)?;
        let element = federation.get(e.model)?.element(e.express_id)?;
        Some(SavedElement {
            model,
            global_id: element.global_id.clone(),
        })
    };
    let saved = SavedView {
        camera: Some(SavedCamera {
            eye: to_world(origin, camera.eye),
            yaw: camera.yaw,
            pitch: camera.pitch,
        }),
        xray: view.xray,
        show_spaces: view.show_spaces,
        storey_filter: view.storey_filter.clone(),
        section: view.section.map(|s| SavedSection {
            height: origin.z + s.height as f64,
            anchor: to_world(origin, s.anchor),
        }),
        hidden: view.hidden.iter().filter_map(save).collect(),
        isolated: view
            .isolated
            .as_ref()
            .map(|set| set.iter().filter_map(save).collect()),
        measurements: measurements
            .iter()
            .map(|m| SavedMeasurement {
                a: to_world(origin, m.a),
                b: to_world(origin, m.b),
                kind: m.kind,
            })
            .collect(),
    };
    serde_json::to_value(saved).expect("view state always serialises")
}

/// Restores a saved view. Elements that no longer exist are skipped.
pub fn apply_view(
    value: serde_json::Value,
    federation: &Federation,
    view: &mut ViewState,
    camera: &mut Camera,
    measurements: &mut Vec<Measurement>,
) {
    let Ok(saved) = serde_json::from_value::<SavedView>(value) else {
        return;
    };
    let origin = federation.origin();
    let ids = federation.ids();
    let load = |s: &SavedElement| -> Option<ElementRef> {
        let id = *ids.get(s.model)?;
        let element = federation.get(id)?.element_by_global_id(&s.global_id)?;
        Some(ElementRef {
            model: id,
            express_id: element.express_id,
        })
    };
    if let Some(c) = saved.camera {
        camera.eye = from_world(origin, c.eye);
        camera.yaw = c.yaw;
        camera.pitch = c.pitch;
    }
    view.xray = saved.xray;
    view.show_spaces = saved.show_spaces;
    view.storey_filter = saved.storey_filter;
    view.section = saved.section.map(|s| SectionPlane {
        height: (s.height - origin.z) as f32,
        anchor: from_world(origin, s.anchor),
    });
    view.hidden = saved.hidden.iter().filter_map(load).collect();
    view.isolated = saved
        .isolated
        .map(|list| list.iter().filter_map(load).collect());
    *measurements = saved
        .measurements
        .iter()
        .map(|m| Measurement {
            a: from_world(origin, m.a),
            b: from_world(origin, m.b),
            kind: m.kind,
        })
        .collect();
    view.modes_dirty = true;
}
