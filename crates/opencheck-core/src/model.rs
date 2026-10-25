use std::path::PathBuf;

use glam::DMat4;
use rustc_hash::FxHashMap;

use crate::{
    Aabb, ClassificationRef, Discipline, ElementMesh, LoadStats, PropertySet, PropertyValue,
    SpatialNode,
};

/// A building element or other product from an IFC file.
#[derive(Debug, Clone)]
pub struct Element {
    pub express_id: u32,
    pub global_id: String,
    /// IFC entity name in schema spelling, e.g. `IfcWallStandardCase`.
    pub ifc_class: String,
    pub name: String,
    pub description: Option<String>,
    pub object_type: Option<String>,
    pub tag: Option<String>,
    /// PredefinedType when the entity has one and it is set.
    pub predefined_type: Option<String>,
    /// The spatial node (usually a storey) the element is contained in.
    pub container: Option<u32>,
    /// The element this one is a part of (e.g. a stair flight in a stair).
    pub parent: Option<u32>,
    /// Parts of this element (`IfcRelAggregates` to non-spatial children).
    pub parts: Vec<u32>,
    /// Express ID and name of the type object (`IfcRelDefinesByType`).
    pub type_object: Option<(u32, String)>,
    /// IFC class of the type object, e.g. `IfcCableCarrierSegmentType`.
    pub type_class: Option<String>,
    /// Direct attributes with a plain value, by schema name (`Name`,
    /// `ObjectType`, `LongName`, `OverallWidth`, ...).
    pub attributes: Vec<(String, PropertyValue)>,
    /// Classification references of the element and of its type.
    pub classifications: Vec<ClassificationRef>,
    /// Material names of the element, or of its type when it has none.
    pub materials: Vec<String>,
    /// Names of the systems (`IfcSystem`) the element is assigned to.
    pub systems: Vec<String>,
    /// Property and quantity sets, own sets first, then sets inherited from the type.
    pub property_sets: Vec<u32>,
    /// Indices into [`IfcModel::meshes`].
    pub meshes: Vec<usize>,
    /// Bounds of all meshes in the model's local frame.
    pub bounds: Aabb,
}

impl Element {
    pub fn has_geometry(&self) -> bool {
        !self.meshes.is_empty()
    }

    /// A direct attribute by schema name, ignoring case.
    pub fn attribute(&self, name: &str) -> Option<&PropertyValue> {
        self.attributes
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v)
    }

    /// The class the element would have in IFC4. IFC2x3 files write many
    /// building services elements as generic `IfcFlowSegment` and so on, with
    /// the specific kind only in the type: `IfcCableCarrierSegmentType`
    /// makes the occurrence an `IfcCableCarrierSegment`.
    pub fn occurrence_class(&self) -> &str {
        const GENERIC: &[&str] = &[
            "IfcFlowSegment",
            "IfcFlowFitting",
            "IfcFlowTerminal",
            "IfcFlowController",
            "IfcFlowMovingDevice",
            "IfcFlowStorageDevice",
            "IfcFlowTreatmentDevice",
            "IfcEnergyConversionDevice",
            "IfcDistributionControlElement",
            "IfcDistributionFlowElement",
            "IfcDistributionElement",
            "IfcBuildingElementProxy",
        ];
        if GENERIC.contains(&self.ifc_class.as_str())
            && let Some(type_class) = &self.type_class
            && let Some(class) = type_class.strip_suffix("Type")
            && class != self.ifc_class
            && !class.ends_with("Element")
        {
            return class;
        }
        &self.ifc_class
    }

    /// Display label: the name, or the class when the element is unnamed.
    pub fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.ifc_class
        } else {
            &self.name
        }
    }
}

/// One loaded IFC file.
#[derive(Debug, Clone)]
pub struct IfcModel {
    pub source_path: Option<PathBuf>,
    /// Short name shown in the UI; defaults to the file stem.
    pub short_name: String,
    pub discipline: Discipline,
    /// Schema identifier from the file header, e.g. `IFC4`.
    pub schema: String,
    pub project_name: Option<String>,
    /// Application that wrote the file, from the STEP header.
    pub authoring_application: Option<String>,
    pub entity_count: usize,
    /// Multiplier from model length units to metres (0.001 for millimetres).
    pub length_unit_scale: f64,
    pub spatial_root: Option<SpatialNode>,
    pub elements: Vec<Element>,
    pub property_sets: FxHashMap<u32, PropertySet>,
    pub meshes: Vec<ElementMesh>,
    /// Transform from the mesh frame to the model's world (project) frame, in metres.
    ///
    /// ifc-lite moves large site coordinates near the origin to keep `f32`
    /// vertices precise. This matrix undoes that shift so several models can be
    /// placed against each other.
    pub local_to_world: DMat4,
    /// Bounds of all meshes in the local frame.
    pub bounds: Aabb,
    pub stats: LoadStats,
    /// SHA-256 of the IFC file, lowercase hex. Identifies a model revision.
    pub content_hash: String,
    /// Elements connected through their ports (`IfcRelConnectsPorts`), by
    /// express id, each pair once.
    pub connections: Vec<[u32; 2]>,
    /// Port positions per element, by express id, in the mesh frame: the
    /// ends of a pipe or duct, the connections of a fitting.
    pub ports: FxHashMap<u32, Vec<[f32; 3]>>,
    /// System name -> what kind of system it is, from its description,
    /// object type and predefined type (e.g. `Afløb, Spildevand`).
    pub system_info: FxHashMap<String, String>,
    element_index: FxHashMap<u32, usize>,
}

impl IfcModel {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        source_path: Option<PathBuf>,
        short_name: String,
        discipline: Discipline,
        schema: String,
        project_name: Option<String>,
        authoring_application: Option<String>,
        entity_count: usize,
        length_unit_scale: f64,
        spatial_root: Option<SpatialNode>,
        elements: Vec<Element>,
        property_sets: FxHashMap<u32, PropertySet>,
        meshes: Vec<ElementMesh>,
        local_to_world: DMat4,
        stats: LoadStats,
    ) -> Self {
        let element_index = elements
            .iter()
            .enumerate()
            .map(|(i, e)| (e.express_id, i))
            .collect();
        let bounds = meshes
            .iter()
            .fold(Aabb::EMPTY, |acc, m| acc.union(&m.bounds));
        Self {
            source_path,
            short_name,
            discipline,
            schema,
            project_name,
            authoring_application,
            entity_count,
            length_unit_scale,
            spatial_root,
            elements,
            property_sets,
            meshes,
            local_to_world,
            bounds,
            stats,
            content_hash: String::new(),
            connections: Vec::new(),
            ports: FxHashMap::default(),
            system_info: FxHashMap::default(),
            element_index,
        }
    }

    pub fn element(&self, express_id: u32) -> Option<&Element> {
        self.element_index
            .get(&express_id)
            .map(|&i| &self.elements[i])
    }

    pub fn element_by_global_id(&self, global_id: &str) -> Option<&Element> {
        self.elements.iter().find(|e| e.global_id == global_id)
    }

    /// Property and quantity sets of an element, in display order.
    pub fn property_sets_of<'a>(
        &'a self,
        element: &'a Element,
    ) -> impl Iterator<Item = &'a PropertySet> + 'a {
        element
            .property_sets
            .iter()
            .filter_map(|id| self.property_sets.get(id))
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.iter().map(ElementMesh::triangle_count).sum()
    }

    /// Element count per IFC class, most common first.
    pub fn class_histogram(&self) -> Vec<(&str, usize)> {
        let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
        for e in &self.elements {
            *counts.entry(e.ifc_class.as_str()).or_default() += 1;
        }
        let mut list: Vec<_> = counts.into_iter().collect();
        list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        list
    }

    /// The storeys of the model, ordered by elevation when known.
    pub fn storeys(&self) -> Vec<&SpatialNode> {
        let mut storeys = Vec::new();
        if let Some(root) = &self.spatial_root {
            root.walk(&mut |node, _| {
                if node.is_storey() {
                    storeys.push(node);
                }
            });
        }
        storeys.sort_by(|a, b| {
            a.elevation
                .unwrap_or(0.0)
                .total_cmp(&b.elevation.unwrap_or(0.0))
        });
        storeys
    }
}
