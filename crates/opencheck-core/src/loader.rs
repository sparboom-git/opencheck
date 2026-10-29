//! Loading IFC files through ifc-lite.
//!
//! Geometry comes from `ifc_lite_processing::process_geometry`, which
//! tessellates every product in parallel. The semantic side (spatial
//! structure, decomposition, types and property sets) is read here from the
//! relationship entities with ifc-lite's STEP decoder.

use std::path::{Path, PathBuf};
use std::time::Duration;

use web_time::Instant;

use glam::{DMat4, DVec3, Vec3};
use ifc_lite_core::{AttributeValue, DecodedEntity, EntityDecoder, EntityScanner};
use ifc_lite_processing::{MeshFrame, ProcessingResult};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    Aabb, ClassificationRef, Discipline, Element, ElementMesh, IfcModel, Property, PropertySet,
    PropertySetKind, PropertyValue, SpatialNode,
};

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("not an IFC file (missing ISO-10303-21 header)")]
    NotIfc,
    #[error("the file contains no IFC entities")]
    Empty,
}

/// Timings and counters from loading one model.
#[derive(Debug, Clone, Default)]
pub struct LoadStats {
    pub file_size: usize,
    pub geometry_time: Duration,
    pub semantics_time: Duration,
}

/// Loads an IFC file from disk.
pub fn load_ifc_file(path: impl AsRef<Path>) -> Result<IfcModel, LoadError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut model = load_ifc_bytes(&name, &bytes)?;
    model.source_path = Some(path.to_path_buf());
    Ok(model)
}

/// Loads an IFC model from memory. `file_name` is used for the short name and
/// the discipline guess.
pub fn load_ifc_bytes(file_name: &str, bytes: &[u8]) -> Result<IfcModel, LoadError> {
    let head = &bytes[..bytes.len().min(64)];
    if !String::from_utf8_lossy(head)
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with("ISO-10303-21")
    {
        return Err(LoadError::NotIfc);
    }

    let t0 = Instant::now();
    let geometry: ProcessingResult = ifc_lite_processing::process_geometry(bytes);
    let geometry_time = t0.elapsed();

    let t1 = Instant::now();
    let mut reader = SemanticReader::new(bytes, geometry.metadata.schema_version.clone());
    reader.scan();
    if reader.entity_count == 0 {
        return Err(LoadError::Empty);
    }
    let length_unit_scale = geometry
        .metadata
        .length_unit_scale
        .unwrap_or_else(|| reader.decoder.length_unit_scale());

    let meshes = convert_meshes(&geometry);
    let spatial_root = reader.build_spatial_tree();
    let elements = reader.build_elements(&meshes);
    let project_name = reader.project_name();
    let connections = reader.connections();
    let port_points = reader.port_points();
    let system_info = std::mem::take(&mut reader.system_info);
    let semantics_time = t1.elapsed();

    let short_name = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Model".to_owned());
    let discipline = Discipline::guess(file_name, elements.iter().map(|e| e.ifc_class.as_str()));

    let mut model = IfcModel::new(
        None,
        short_name,
        discipline,
        geometry.metadata.schema_version.clone(),
        project_name,
        originating_system(bytes),
        reader.entity_count,
        length_unit_scale,
        spatial_root,
        elements,
        reader.property_sets,
        meshes,
        frame_to_world(geometry.frame),
        LoadStats {
            file_size: bytes.len(),
            geometry_time,
            semantics_time,
        },
    );
    model.connections = connections;
    model.file_timestamp = file_name_arg(bytes, 1);
    // Ports in the mesh frame, where the geometry is.
    let to_mesh = model.local_to_world.inverse();
    let port_points: FxHashMap<u32, Vec<DVec3>> = port_points
        .into_iter()
        .map(|(e, points)| {
            let points = points
                .into_iter()
                .map(|p| to_mesh.transform_point3(p * length_unit_scale))
                .collect();
            (e, points)
        })
        .collect();
    model.ports = fit_ports(&model, port_points);
    model.system_info = system_info;
    model.content_hash = crate::project::sha256_hex(bytes);
    Ok(model)
}

/// Lines the ports up with the geometry. The placements followed here and
/// the frame ifc-lite puts the geometry in can differ by a turn about the
/// vertical (seen with some Revit exports), so the turn and shift that put
/// the middle of each two-port element on the middle of its geometry is
/// fitted on the straight segments and applied. Ports that do not fit
/// well are dropped.
fn fit_ports(model: &IfcModel, ports: FxHashMap<u32, Vec<DVec3>>) -> FxHashMap<u32, Vec<[f32; 3]>> {
    let pairs: Vec<(DVec3, DVec3)> = ports
        .iter()
        .filter(|(_, p)| p.len() == 2)
        .filter_map(|(e, p)| {
            let element = model.element(*e)?;
            // Straight pieces only: a bend's ports are not around its middle.
            (element.occurrence_class().ends_with("Segment") && !element.bounds.is_empty())
                .then(|| ((p[0] + p[1]) / 2.0, element.bounds.center().as_dvec3()))
        })
        .collect();
    if pairs.len() < 3 {
        return FxHashMap::default();
    }
    // A turn about the vertical, then the shift that best follows it.
    let fit_for = |angle: f64| {
        let turn = DMat4::from_rotation_z(angle);
        let mut shifts: Vec<DVec3> = pairs
            .iter()
            .map(|(a, b)| *b - turn.transform_point3(*a))
            .collect();
        let median = |axis: usize, list: &mut Vec<DVec3>| {
            list.sort_by(|p, q| p[axis].total_cmp(&q[axis]));
            list[list.len() / 2][axis]
        };
        let shift = DVec3::new(
            median(0, &mut shifts),
            median(1, &mut shifts),
            median(2, &mut shifts),
        );
        let fit = DMat4::from_translation(shift) * turn;
        let mut misses: Vec<f64> = pairs
            .iter()
            .map(|(a, b)| fit.transform_point3(*a).distance(*b))
            .collect();
        misses.sort_by(f64::total_cmp);
        (misses[misses.len() / 2], fit)
    };
    let (dot, cross) = pairs.iter().fold((0.0, 0.0), |(d, c), (a, b)| {
        let (a, b) = (a.truncate(), b.truncate());
        (d + a.dot(b), c + a.perp_dot(b))
    });
    let quarter = std::f64::consts::FRAC_PI_2;
    let (miss, fit) = [0.0, quarter, 2.0 * quarter, 3.0 * quarter, cross.atan2(dot)]
        .into_iter()
        .map(fit_for)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("five candidates");
    if miss > 0.05 {
        return FxHashMap::default();
    }
    ports
        .into_iter()
        .filter_map(|(e, points)| {
            let points: Vec<DVec3> = points
                .into_iter()
                .map(|p| fit.transform_point3(p))
                .collect();
            // Ports must lie on their element.
            let bounds = model.element(e)?.bounds;
            let margin = DVec3::splat(0.05);
            let lo = bounds.min.as_dvec3() - margin;
            let hi = bounds.max.as_dvec3() + margin;
            points
                .iter()
                .all(|p| p.cmpge(lo).all() && p.cmple(hi).all())
                .then(|| (e, points.iter().map(|p| p.as_vec3().to_array()).collect()))
        })
        .collect()
}

/// The inverse of the shift ifc-lite applied to the vertices.
///
/// For a site-local frame the vertices were brought in with
/// `R^T * (world - t)`, so the way back is the site placement itself.
fn frame_to_world(frame: MeshFrame) -> DMat4 {
    match frame {
        MeshFrame::SiteLocal { placement } => DMat4::from_cols_array(&placement),
        MeshFrame::ModelRtc { anchor } => {
            DMat4::from_translation(DVec3::new(anchor.0, anchor.1, anchor.2))
        }
        MeshFrame::RawIfc => DMat4::IDENTITY,
    }
}

fn convert_meshes(result: &ProcessingResult) -> Vec<ElementMesh> {
    result
        .meshes
        .iter()
        .filter(|m| !m.indices.is_empty() && !is_void_class(&m.ifc_type))
        .map(|m| {
            // In its WebAssembly build ifc-lite stores each element relative to
            // its own centre (`origin`); natively the origin is zero.
            let [ox, oy, oz] = m.origin;
            let positions: Vec<[f32; 3]> = if m.origin == [0.0; 3] {
                m.positions.as_chunks::<3>().0.to_vec()
            } else {
                m.positions
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|p| {
                        [
                            (ox + f64::from(p[0])) as f32,
                            (oy + f64::from(p[1])) as f32,
                            (oz + f64::from(p[2])) as f32,
                        ]
                    })
                    .collect()
            };
            let normals = m.normals.as_chunks::<3>().0.to_vec();
            let bounds = Aabb::from_points(positions.iter().map(|p| Vec3::from_array(*p)));
            ElementMesh {
                express_id: m.express_id,
                positions,
                normals,
                indices: m.indices.clone(),
                color: m.color,
                bounds,
            }
        })
        .collect()
}

/// Openings are subtracted from their host walls and slabs; their own
/// volumes are never drawn or checked.
fn is_void_class(ifc_type: &str) -> bool {
    matches!(
        ifc_type,
        "IfcOpeningElement" | "IfcOpeningStandardCase" | "IfcVoidingFeature"
    )
}

/// Reads `FILE_NAME.originating_system` from the STEP header.
fn originating_system(bytes: &[u8]) -> Option<String> {
    file_name_arg(bytes, 5).or_else(|| file_name_arg(bytes, 4))
}

/// Argument `i` of the header's `FILE_NAME`: 1 is the time stamp, 5 the
/// application that wrote the file.
fn file_name_arg(bytes: &[u8], i: usize) -> Option<String> {
    let header_end = find(bytes, b"ENDSEC")?;
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let start = header.find("FILE_NAME(")? + "FILE_NAME(".len();
    split_step_args(&header[start..])
        .get(i)
        .map(|s| s.trim().trim_matches('\'').trim().to_owned())
        .filter(|s| !s.is_empty() && s != "$")
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Splits the top-level arguments of a STEP parameter list (after the opening paren).
fn split_step_args(s: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let (mut depth, mut in_string, mut start) = (0i32, false, 0usize);
    for (i, c) in s.char_indices() {
        match c {
            '\'' => in_string = !in_string,
            '(' if !in_string => depth += 1,
            ')' if !in_string => {
                if depth == 0 {
                    args.push(&s[start..i]);
                    return args;
                }
                depth -= 1;
            }
            ',' if !in_string && depth == 0 => {
                args.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    args
}

// IFC attribute positions shared by all schemas we support.
const ROOT_GLOBAL_ID: usize = 0;
const ROOT_NAME: usize = 2;
const ROOT_DESCRIPTION: usize = 3;
const OBJECT_TYPE: usize = 4;
const ELEMENT_TAG: usize = 7;
const SPATIAL_LONG_NAME: usize = 7;
const STOREY_ELEVATION: usize = 9;
const OBJECT_PLACEMENT: usize = 5;
const REL_FIRST: usize = 4;
const REL_SECOND: usize = 5;
const TYPE_HAS_PROPERTY_SETS: usize = 5;
const REL_GROUP: usize = 6;

/// Collects relationship entities in one scan and resolves them on demand.
struct SemanticReader<'a> {
    decoder: EntityDecoder<'a>,
    bytes: &'a [u8],
    schema: String,
    entity_count: usize,
    project: Option<u32>,
    /// parent -> children from `IfcRelAggregates` (and `IfcRelNests` for IFC4 parts).
    aggregates: FxHashMap<u32, Vec<u32>>,
    /// spatial structure -> contained elements.
    contained: FxHashMap<u32, Vec<u32>>,
    /// object -> property set definitions.
    defined_by_properties: FxHashMap<u32, Vec<u32>>,
    /// object -> type object.
    defined_by_type: FxHashMap<u32, u32>,
    /// object -> classification references (or other classification selects).
    classified_by: FxHashMap<u32, Vec<u32>>,
    /// object -> material select.
    material_of: FxHashMap<u32, u32>,
    /// object -> groups it is assigned to.
    groups_of: FxHashMap<u32, Vec<u32>>,
    /// port -> element it belongs to (`IfcRelConnectsPortToElement`).
    port_element: FxHashMap<u32, u32>,
    /// element -> nested objects (`IfcRelNests`); some of them ports.
    nested: Vec<(u32, Vec<u32>)>,
    /// Connected ports and the element realising the connection, if any
    /// (`IfcRelConnectsPorts`).
    port_links: Vec<(u32, u32, Option<u32>)>,
    /// System name -> its description, object type and predefined type.
    system_info: FxHashMap<String, String>,
    /// Resolved `IfcLocalPlacement` transforms.
    placements: FxHashMap<u32, DMat4>,
    property_sets: FxHashMap<u32, PropertySet>,
    class_cache: FxHashMap<u32, String>,
}

impl<'a> SemanticReader<'a> {
    fn new(bytes: &'a [u8], schema: String) -> Self {
        Self {
            decoder: EntityDecoder::new(bytes),
            bytes,
            schema,
            entity_count: 0,
            project: None,
            aggregates: FxHashMap::default(),
            contained: FxHashMap::default(),
            defined_by_properties: FxHashMap::default(),
            defined_by_type: FxHashMap::default(),
            classified_by: FxHashMap::default(),
            material_of: FxHashMap::default(),
            groups_of: FxHashMap::default(),
            port_element: FxHashMap::default(),
            nested: Vec::new(),
            port_links: Vec::new(),
            system_info: FxHashMap::default(),
            placements: FxHashMap::default(),
            property_sets: FxHashMap::default(),
            class_cache: FxHashMap::default(),
        }
    }

    fn scan(&mut self) {
        let mut scanner = EntityScanner::new(self.bytes);
        let mut rels: Vec<(u32, RelKind)> = Vec::new();
        while let Some((id, type_name, _, _)) = scanner.next_entity() {
            self.entity_count += 1;
            let kind = match type_name.to_ascii_uppercase().as_str() {
                "IFCPROJECT" => {
                    self.project.get_or_insert(id);
                    continue;
                }
                "IFCRELAGGREGATES" => RelKind::Aggregates,
                "IFCRELCONTAINEDINSPATIALSTRUCTURE" => RelKind::Contained,
                "IFCRELDEFINESBYPROPERTIES" => RelKind::Properties,
                "IFCRELDEFINESBYTYPE" => RelKind::Type,
                "IFCRELASSOCIATESCLASSIFICATION" => RelKind::Classification,
                "IFCRELASSOCIATESMATERIAL" => RelKind::Material,
                "IFCRELASSIGNSTOGROUP" => RelKind::Group,
                "IFCRELCONNECTSPORTTOELEMENT" => RelKind::PortToElement,
                "IFCRELCONNECTSPORTS" => RelKind::ConnectsPorts,
                "IFCRELNESTS" => RelKind::Nests,
                _ => continue,
            };
            rels.push((id, kind));
        }

        for (id, kind) in rels {
            let Some(rel) = self.decode(id) else { continue };
            match kind {
                // RelatingObject, RelatedObjects
                RelKind::Aggregates => {
                    if let (Some(parent), Some(children)) =
                        (rel.get_ref(REL_FIRST), rel.get_refs(REL_SECOND))
                    {
                        self.aggregates.entry(parent).or_default().extend(children);
                    }
                }
                // RelatedElements, RelatingStructure
                RelKind::Contained => {
                    if let (Some(elements), Some(structure)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        self.contained
                            .entry(structure)
                            .or_default()
                            .extend(elements);
                    }
                }
                // RelatedObjects, RelatingPropertyDefinition
                RelKind::Properties => {
                    if let (Some(objects), Some(pset)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        for object in objects {
                            self.defined_by_properties
                                .entry(object)
                                .or_default()
                                .push(pset);
                        }
                    }
                }
                // RelatedObjects, RelatingType
                RelKind::Type => {
                    if let (Some(objects), Some(type_object)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        for object in objects {
                            self.defined_by_type.insert(object, type_object);
                        }
                    }
                }
                // RelatedObjects, RelatingClassification
                RelKind::Classification => {
                    if let (Some(objects), Some(reference)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        for object in objects {
                            self.classified_by
                                .entry(object)
                                .or_default()
                                .push(reference);
                        }
                    }
                }
                // RelatedObjects, RelatingMaterial
                RelKind::Material => {
                    if let (Some(objects), Some(material)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        for object in objects {
                            self.material_of.entry(object).or_insert(material);
                        }
                    }
                }
                // RelatedObjects, RelatedObjectsType, RelatingGroup
                RelKind::Group => {
                    if let (Some(objects), Some(group)) =
                        (rel.get_refs(REL_FIRST), rel.get_ref(REL_GROUP))
                    {
                        for object in objects {
                            self.groups_of.entry(object).or_default().push(group);
                        }
                    }
                }
                // RelatingPort, RelatedElement
                RelKind::PortToElement => {
                    if let (Some(port), Some(element)) =
                        (rel.get_ref(REL_FIRST), rel.get_ref(REL_SECOND))
                    {
                        self.port_element.insert(port, element);
                    }
                }
                // RelatingPort, RelatedPort, RealizingElement
                RelKind::ConnectsPorts => {
                    if let (Some(a), Some(b)) = (rel.get_ref(REL_FIRST), rel.get_ref(REL_SECOND)) {
                        self.port_links.push((a, b, rel.get_ref(REL_SECOND + 1)));
                    }
                }
                // RelatingObject, RelatedObjects
                RelKind::Nests => {
                    if let (Some(parent), Some(children)) =
                        (rel.get_ref(REL_FIRST), rel.get_refs(REL_SECOND))
                    {
                        self.nested.push((parent, children));
                    }
                }
            }
        }
    }

    fn decode(&mut self, id: u32) -> Option<DecodedEntity> {
        self.decoder.decode_by_id(id).ok()
    }

    fn class_of(&mut self, id: u32) -> String {
        if let Some(c) = self.class_cache.get(&id) {
            return c.clone();
        }
        let class = self
            .decode(id)
            .map(|e| e.ifc_type.name().to_owned())
            .unwrap_or_default();
        self.class_cache.insert(id, class.clone());
        class
    }

    /// Project, site, building, storey and other spatial containers. Spaces are
    /// treated as elements, the way checking tools present them.
    fn is_spatial_container(&mut self, id: u32) -> bool {
        let class = self.class_of(id);
        if class == "IfcProject" {
            return true;
        }
        if class == "IfcSpace" || class.is_empty() {
            return false;
        }
        ifc_lite_core::is_subtype_of_for_schema(&self.schema, &class, "IfcSpatialStructureElement")
            || ifc_lite_core::is_subtype_of_for_schema(&self.schema, &class, "IfcFacility")
            || ifc_lite_core::is_subtype_of_for_schema(&self.schema, &class, "IfcFacilityPart")
    }

    fn project_name(&mut self) -> Option<String> {
        let project = self.decode(self.project?)?;
        string_attr(&project, ROOT_NAME).or_else(|| string_attr(&project, 5))
    }

    fn build_spatial_tree(&mut self) -> Option<SpatialNode> {
        let root = self.project?;
        let mut visited = FxHashSet::default();
        Some(self.build_spatial_node(root, &mut visited))
    }

    fn build_spatial_node(&mut self, id: u32, visited: &mut FxHashSet<u32>) -> SpatialNode {
        visited.insert(id);
        let entity = self.decode(id);
        let class = self.class_of(id);
        let name = entity
            .as_ref()
            .and_then(|e| string_attr(e, ROOT_NAME))
            .unwrap_or_default();
        let is_spatial_element = class != "IfcProject";
        let long_name = entity
            .as_ref()
            .filter(|_| is_spatial_element)
            .and_then(|e| string_attr(e, SPATIAL_LONG_NAME));
        let elevation = entity
            .as_ref()
            .filter(|_| class == "IfcBuildingStorey")
            .and_then(|e| e.get_float(STOREY_ELEVATION));
        let placement_z = entity
            .as_ref()
            .filter(|_| is_spatial_element)
            .and_then(|e| e.get_ref(OBJECT_PLACEMENT))
            .and_then(|p| self.placement(p, 0))
            .map(|m| m.w_axis.z);

        let mut node = SpatialNode {
            express_id: id,
            ifc_class: class,
            name,
            long_name,
            elevation,
            placement_z,
            children: Vec::new(),
            elements: Vec::new(),
        };

        let children = self.aggregates.get(&id).cloned().unwrap_or_default();
        for child in children {
            if visited.contains(&child) {
                continue;
            }
            if self.is_spatial_container(child) {
                let child_node = self.build_spatial_node(child, visited);
                node.children.push(child_node);
            } else {
                // Spaces and other products aggregated directly under a container.
                node.elements.push(child);
                self.collect_space_contents(child, &mut node, visited);
            }
        }
        let contained = self.contained.get(&id).cloned().unwrap_or_default();
        for element in contained {
            if visited.insert(element) {
                node.elements.push(element);
            }
        }
        node
    }

    /// Elements contained in a space are listed under the space's container.
    fn collect_space_contents(
        &mut self,
        space: u32,
        node: &mut SpatialNode,
        visited: &mut FxHashSet<u32>,
    ) {
        if let Some(inside) = self.contained.get(&space).cloned() {
            for element in inside {
                if visited.insert(element) {
                    node.elements.push(element);
                }
            }
        }
    }

    fn build_elements(&mut self, meshes: &[ElementMesh]) -> Vec<Element> {
        // Container of every element listed in the spatial tree.
        let mut container: FxHashMap<u32, u32> = FxHashMap::default();
        let mut order: Vec<u32> = Vec::new();
        let mut seen: FxHashSet<u32> = FxHashSet::default();
        if let Some(root) = self.project {
            let mut stack = vec![root];
            let mut visited_nodes = FxHashSet::default();
            while let Some(node) = stack.pop() {
                if !visited_nodes.insert(node) {
                    continue;
                }
                let mut direct: Vec<u32> = self.contained.get(&node).cloned().unwrap_or_default();
                for child in self.aggregates.get(&node).cloned().unwrap_or_default() {
                    if self.is_spatial_container(child) {
                        stack.push(child);
                    } else {
                        direct.push(child);
                        direct.extend(self.contained.get(&child).cloned().unwrap_or_default());
                    }
                }
                for element in direct {
                    container.entry(element).or_insert(node);
                    if seen.insert(element) {
                        order.push(element);
                    }
                }
            }
        }

        // Parts of elements (stair flights, curtain wall panels, assembly members).
        let mut parent: FxHashMap<u32, u32> = FxHashMap::default();
        let mut i = 0;
        while i < order.len() {
            let id = order[i];
            if let Some(parts) = self.aggregates.get(&id).cloned() {
                for part in parts {
                    parent.entry(part).or_insert(id);
                    if seen.insert(part) {
                        order.push(part);
                    }
                }
            }
            i += 1;
        }

        // Anything with geometry that the tree did not reach.
        let mut meshes_of: FxHashMap<u32, Vec<usize>> = FxHashMap::default();
        for (index, mesh) in meshes.iter().enumerate() {
            meshes_of.entry(mesh.express_id).or_default().push(index);
            if seen.insert(mesh.express_id) {
                order.push(mesh.express_id);
            }
        }

        order
            .into_iter()
            .filter_map(|id| {
                let entity = self.decode(id)?;
                let ifc_class = entity.ifc_type.name().to_owned();
                if is_void_class(&ifc_class) {
                    return None;
                }
                let mesh_indices = meshes_of.remove(&id).unwrap_or_default();
                let bounds = mesh_indices
                    .iter()
                    .fold(Aabb::EMPTY, |acc, &m| acc.union(&meshes[m].bounds));
                let parts = self.aggregates.get(&id).cloned().unwrap_or_default();
                let type_object = self.defined_by_type.get(&id).copied().map(|t| {
                    let name = self
                        .decode(t)
                        .and_then(|e| string_attr(&e, ROOT_NAME))
                        .unwrap_or_default();
                    (t, name)
                });
                let type_id = type_object.as_ref().map(|t| t.0);
                let property_sets = self.property_sets_for(id, type_id);
                let type_class = type_id.map(|t| self.class_of(t));
                let attributes = self.plain_attributes(&entity, &ifc_class);
                let mut classifications = self.classifications_of(id);
                if let Some(t) = type_id {
                    for c in self.classifications_of(t) {
                        if !classifications.contains(&c) {
                            classifications.push(c);
                        }
                    }
                }
                let mut materials = self.materials_of(id);
                if materials.is_empty()
                    && let Some(t) = type_id
                {
                    materials = self.materials_of(t);
                }
                let systems = self.systems_of(id);
                Some(Element {
                    express_id: id,
                    global_id: string_attr(&entity, ROOT_GLOBAL_ID).unwrap_or_default(),
                    name: string_attr(&entity, ROOT_NAME).unwrap_or_default(),
                    description: string_attr(&entity, ROOT_DESCRIPTION),
                    object_type: string_attr(&entity, OBJECT_TYPE),
                    tag: self
                        .has_tag(&ifc_class)
                        .then(|| string_attr(&entity, ELEMENT_TAG))
                        .flatten(),
                    predefined_type: self.predefined_type(&entity, &ifc_class),
                    ifc_class,
                    container: container.get(&id).copied(),
                    parent: parent.get(&id).copied(),
                    parts,
                    type_object,
                    type_class,
                    attributes,
                    classifications,
                    materials,
                    systems,
                    property_sets,
                    meshes: mesh_indices,
                    bounds,
                })
            })
            .collect()
    }

    /// Direct attributes with a plain (non-reference) value.
    fn plain_attributes(
        &self,
        entity: &DecodedEntity,
        class: &str,
    ) -> Vec<(String, PropertyValue)> {
        let Some(names) = ifc_lite_core::attribute_names_for_schema(&self.schema, class) else {
            return Vec::new();
        };
        names
            .iter()
            .enumerate()
            .skip(ROOT_NAME)
            .filter_map(|(index, name)| {
                let value = match entity.get(index)? {
                    AttributeValue::String(s) if !s.trim().is_empty() => {
                        PropertyValue::Text(s.trim().to_owned())
                    }
                    attr @ (AttributeValue::Integer(_)
                    | AttributeValue::Float(_)
                    | AttributeValue::Enum(_)) => plain_value(attr),
                    _ => return None,
                };
                Some(((*name).to_owned(), value))
            })
            .collect()
    }

    fn classifications_of(&mut self, id: u32) -> Vec<ClassificationRef> {
        let mut out = Vec::new();
        for reference in self.classified_by.get(&id).cloned().unwrap_or_default() {
            let Some(entity) = self.decode(reference) else {
                continue;
            };
            match entity.ifc_type.name() {
                // Location, ItemReference/Identification, Name, ReferencedSource
                "IfcClassificationReference" => {
                    let system = entity
                        .get_ref(3)
                        .and_then(|s| self.decode(s))
                        .and_then(|s| {
                            // IfcClassification: Source, Edition, EditionDate, Name
                            match s.ifc_type.name() {
                                "IfcClassification" => string_attr(&s, 3),
                                "IfcClassificationReference" => string_attr(&s, 2),
                                _ => None,
                            }
                        })
                        .unwrap_or_default();
                    out.push(ClassificationRef {
                        system,
                        code: string_attr(&entity, 1).unwrap_or_default(),
                        name: string_attr(&entity, 2).unwrap_or_default(),
                    });
                }
                // The whole system without a specific item.
                "IfcClassification" => out.push(ClassificationRef {
                    system: string_attr(&entity, 3).unwrap_or_default(),
                    code: String::new(),
                    name: String::new(),
                }),
                _ => {}
            }
        }
        out
    }

    /// Material names behind any material select: single materials, lists,
    /// layer, profile and constituent sets and their usages.
    fn materials_of(&mut self, id: u32) -> Vec<String> {
        let Some(&root) = self.material_of.get(&id) else {
            return Vec::new();
        };
        let mut names = Vec::new();
        let mut stack = vec![root];
        let mut seen = FxHashSet::default();
        while let Some(m) = stack.pop() {
            if !seen.insert(m) || seen.len() > 64 {
                continue;
            }
            let Some(entity) = self.decode(m) else {
                continue;
            };
            match entity.ifc_type.name() {
                "IfcMaterial" => {
                    if let Some(name) = string_attr(&entity, 0)
                        && !names.contains(&name)
                    {
                        names.push(name);
                    }
                }
                // Materials
                "IfcMaterialList" => stack.extend(entity.get_refs(0).unwrap_or_default()),
                // ForLayerSet / ForProfileSet
                "IfcMaterialLayerSetUsage" | "IfcMaterialProfileSetUsage" => {
                    stack.extend(entity.get_ref(0))
                }
                // MaterialLayers
                "IfcMaterialLayerSet" => stack.extend(entity.get_refs(0).unwrap_or_default()),
                // Material (first attribute)
                "IfcMaterialLayer" => stack.extend(entity.get_ref(0)),
                // Name, Description, MaterialProfiles / MaterialConstituents
                "IfcMaterialProfileSet" | "IfcMaterialConstituentSet" => {
                    stack.extend(entity.get_refs(2).unwrap_or_default())
                }
                // Name, Description, Material, ...
                "IfcMaterialProfile" | "IfcMaterialConstituent" => stack.extend(entity.get_ref(2)),
                _ => {}
            }
        }
        names
    }

    fn systems_of(&mut self, id: u32) -> Vec<String> {
        let mut names = Vec::new();
        for group in self.groups_of.get(&id).cloned().unwrap_or_default() {
            let class = self.class_of(group);
            if !ifc_lite_core::is_subtype_of_for_schema(&self.schema, &class, "IfcSystem") {
                continue;
            }
            let Some(entity) = self.decode(group) else {
                continue;
            };
            if let Some(name) = string_attr(&entity, ROOT_NAME)
                && !names.contains(&name)
            {
                if !self.system_info.contains_key(&name) {
                    // What kind of system: "Afløb, Spildevand", DRAINAGE, ...
                    let mut info: Vec<String> = [ROOT_DESCRIPTION, OBJECT_TYPE]
                        .iter()
                        .filter_map(|i| string_attr(&entity, *i))
                        .collect();
                    info.extend(self.predefined_type(&entity, &class));
                    self.system_info.insert(name.clone(), info.join(", "));
                }
                names.push(name);
            }
        }
        names
    }

    /// The world transform of an `IfcLocalPlacement`, in model length units,
    /// following the placements it is relative to.
    fn placement(&mut self, id: u32, depth: usize) -> Option<DMat4> {
        if depth > 32 {
            return None;
        }
        if let Some(m) = self.placements.get(&id) {
            return Some(*m);
        }
        let entity = self.decode(id)?;
        if !entity
            .ifc_type
            .name()
            .eq_ignore_ascii_case("IfcLocalPlacement")
        {
            return Some(DMat4::IDENTITY);
        }
        let parent = match entity.get_ref(0) {
            Some(p) => self.placement(p, depth + 1)?,
            None => DMat4::IDENTITY,
        };
        let relative = entity
            .get_ref(1)
            .and_then(|a| self.axis_placement(a))
            .unwrap_or(DMat4::IDENTITY);
        let world = parent * relative;
        self.placements.insert(id, world);
        Some(world)
    }

    /// `IfcAxis2Placement3D` (or 2D) as a transform: location, axis, ref direction.
    fn axis_placement(&mut self, id: u32) -> Option<DMat4> {
        let entity = self.decode(id)?;
        let mut vector = |index: usize| -> Option<DVec3> {
            let r = entity.get_ref(index)?;
            let e = self.decode(r)?;
            let list = e.get_list(0)?;
            let v: Vec<f64> = list.iter().filter_map(|a| a.as_float()).collect();
            Some(DVec3::new(
                v.first().copied()?,
                v.get(1).copied().unwrap_or(0.0),
                v.get(2).copied().unwrap_or(0.0),
            ))
        };
        let location = vector(0).unwrap_or(DVec3::ZERO);
        if !entity
            .ifc_type
            .name()
            .eq_ignore_ascii_case("IfcAxis2Placement3D")
        {
            return Some(DMat4::from_translation(location));
        }
        let z = vector(1)
            .and_then(|v| v.try_normalize())
            .unwrap_or(DVec3::Z);
        let x = vector(2)
            .map(|r| r - z * r.dot(z))
            .and_then(|v| v.try_normalize())
            .unwrap_or_else(|| z.any_orthonormal_vector());
        let y = z.cross(x);
        Some(DMat4::from_cols(
            x.extend(0.0),
            y.extend(0.0),
            z.extend(0.0),
            location.extend(1.0),
        ))
    }

    /// Port -> element: `IfcRelConnectsPortToElement`, and in IFC4 the
    /// connected ports nested in their element.
    fn ports(&self) -> FxHashMap<u32, u32> {
        let mut port_element = self.port_element.clone();
        let linked: FxHashSet<u32> = self
            .port_links
            .iter()
            .flat_map(|(a, b, _)| [*a, *b])
            .collect();
        for (parent, children) in &self.nested {
            for child in children {
                if linked.contains(child) {
                    port_element.entry(*child).or_insert(*parent);
                }
            }
        }
        port_element
    }

    /// Where each element's ports are, in the IFC world frame and model
    /// length units: the ends of a pipe, the connections of a fitting.
    fn port_points(&mut self) -> FxHashMap<u32, Vec<DVec3>> {
        let mut out: FxHashMap<u32, Vec<DVec3>> = FxHashMap::default();
        let mut ports: Vec<(u32, u32)> = self.ports().into_iter().collect();
        ports.sort_unstable();
        for (port, element) in ports {
            let Some(placement) = self.decode(port).and_then(|p| p.get_ref(OBJECT_PLACEMENT))
            else {
                continue;
            };
            if let Some(m) = self.placement(placement, 0) {
                out.entry(element).or_default().push(m.w_axis.truncate());
            }
        }
        out
    }

    /// Pairs of elements whose ports are connected; an element realising a
    /// connection is connected to both sides.
    fn connections(&self) -> Vec<[u32; 2]> {
        let port_element = self.ports();
        let mut pairs = FxHashSet::default();
        let mut add = |a: u32, b: u32| {
            if a != b {
                pairs.insert([a.min(b), a.max(b)]);
            }
        };
        for (a, b, realizing) in &self.port_links {
            let (Some(&ea), Some(&eb)) = (port_element.get(a), port_element.get(b)) else {
                continue;
            };
            match realizing {
                Some(r) => {
                    add(ea, *r);
                    add(*r, eb);
                }
                None => add(ea, eb),
            }
        }
        let mut list: Vec<[u32; 2]> = pairs.into_iter().collect();
        list.sort_unstable();
        list
    }

    fn has_tag(&self, class: &str) -> bool {
        ifc_lite_core::is_subtype_of_for_schema(&self.schema, class, "IfcElement")
    }

    fn predefined_type(&self, entity: &DecodedEntity, class: &str) -> Option<String> {
        let names = ifc_lite_core::attribute_names_for_schema(&self.schema, class)?;
        let index = names.iter().position(|n| *n == "PredefinedType")?;
        entity
            .get(index)?
            .as_enum()
            .filter(|v| *v != "NOTDEFINED")
            .map(str::to_owned)
    }

    /// Own property sets first, then those of the type object that the
    /// occurrence does not override by name.
    fn property_sets_for(&mut self, id: u32, type_object: Option<u32>) -> Vec<u32> {
        let mut ids: Vec<u32> = self
            .defined_by_properties
            .get(&id)
            .cloned()
            .unwrap_or_default();
        if let Some(type_id) = type_object
            && let Some(type_entity) = self.decode(type_id)
            && let Some(type_sets) = type_entity.get_refs(TYPE_HAS_PROPERTY_SETS)
        {
            ids.extend(type_sets);
        }
        let mut result = Vec::with_capacity(ids.len());
        let mut names = FxHashSet::default();
        for pset_id in ids {
            if self.load_property_set(pset_id)
                && let Some(pset) = self.property_sets.get(&pset_id)
                && names.insert(pset.name.clone())
            {
                result.push(pset_id);
            }
        }
        result
    }

    /// Parses a property or quantity set once and caches it. Returns false
    /// when the definition is of a kind we do not show (e.g. templates).
    fn load_property_set(&mut self, id: u32) -> bool {
        if self.property_sets.contains_key(&id) {
            return true;
        }
        let Some(entity) = self.decode(id) else {
            return false;
        };
        let class = entity.ifc_type.name();
        let (kind, list_index) = match class {
            "IfcPropertySet" => (PropertySetKind::Properties, 4),
            "IfcElementQuantity" => (PropertySetKind::Quantities, 5),
            _ => return false,
        };
        let name = string_attr(&entity, ROOT_NAME).unwrap_or_default();
        let members = entity.get_refs(list_index).unwrap_or_default();
        let properties = members
            .into_iter()
            .filter_map(|m| self.decode(m))
            .filter_map(|m| read_property(&m))
            .collect();
        self.property_sets.insert(
            id,
            PropertySet {
                express_id: id,
                name,
                kind,
                properties,
            },
        );
        true
    }
}

#[derive(Clone, Copy)]
enum RelKind {
    Aggregates,
    Contained,
    Properties,
    Type,
    Classification,
    Material,
    Group,
    PortToElement,
    ConnectsPorts,
    Nests,
}

fn string_attr(entity: &DecodedEntity, index: usize) -> Option<String> {
    entity
        .get_string(index)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Reads one property or quantity entity.
fn read_property(entity: &DecodedEntity) -> Option<Property> {
    let class = entity.ifc_type.name();
    let name = entity.get_string(0)?.to_owned();
    let (value, value_type) = match class {
        // Name, Description, NominalValue, Unit
        "IfcPropertySingleValue" => typed_value(entity.get(2)?),
        // Name, Description, EnumerationValues, EnumerationReference
        "IfcPropertyEnumeratedValue" | "IfcPropertyListValue" => {
            let items: Vec<_> = entity
                .get_list(2)
                .unwrap_or_default()
                .iter()
                .map(typed_value)
                .collect();
            let value_type = items.first().and_then(|(_, t)| t.clone());
            (
                PropertyValue::List(items.into_iter().map(|(v, _)| v).collect()),
                value_type,
            )
        }
        // Name, Description, UpperBoundValue, LowerBoundValue, ...
        "IfcPropertyBoundedValue" => {
            let (upper, value_type) = typed_value(entity.get(2)?);
            let (lower, _) = typed_value(entity.get(3)?);
            (
                PropertyValue::Range {
                    lower: Box::new(lower),
                    upper: Box::new(upper),
                },
                value_type,
            )
        }
        // Name, Description, Unit, Value, (Formula)
        "IfcQuantityLength" | "IfcQuantityArea" | "IfcQuantityVolume" | "IfcQuantityCount"
        | "IfcQuantityWeight" | "IfcQuantityTime" | "IfcQuantityNumber" => {
            let value = match entity.get(3)? {
                AttributeValue::Integer(i) => PropertyValue::Integer(*i),
                other => other.as_float().map(PropertyValue::Real)?,
            };
            (value, Some(class.to_owned()))
        }
        _ => return None,
    };
    Some(Property {
        name,
        value,
        value_type,
    })
}

/// Converts an attribute that may hold a typed value such as `IFCLABEL('x')`.
fn typed_value(attr: &AttributeValue) -> (PropertyValue, Option<String>) {
    if let AttributeValue::List(items) = attr
        && let [AttributeValue::String(type_name), inner] = items.as_slice()
        && type_name.starts_with("IFC")
    {
        let value = match (type_name.as_str(), inner) {
            ("IFCBOOLEAN", AttributeValue::Enum(e)) => PropertyValue::Boolean(e == "T"),
            ("IFCLOGICAL", AttributeValue::Enum(e)) => PropertyValue::Logical(match e.as_str() {
                "T" => Some(true),
                "F" => Some(false),
                _ => None,
            }),
            _ => plain_value(inner),
        };
        return (value, Some(measure_type_name(type_name)));
    }
    (plain_value(attr), None)
}

fn plain_value(attr: &AttributeValue) -> PropertyValue {
    match attr {
        AttributeValue::String(s) => PropertyValue::Text(s.clone()),
        AttributeValue::Integer(i) => PropertyValue::Integer(*i),
        AttributeValue::Float(f) => PropertyValue::Real(*f),
        AttributeValue::Enum(e) => match e.as_str() {
            "T" => PropertyValue::Boolean(true),
            "F" => PropertyValue::Boolean(false),
            _ => PropertyValue::Text(e.clone()),
        },
        AttributeValue::List(items) => {
            PropertyValue::List(items.iter().map(|i| typed_value(i).0).collect())
        }
        AttributeValue::EntityRef(id) => PropertyValue::Text(format!("#{id}")),
        AttributeValue::Null | AttributeValue::Derived => PropertyValue::Null,
    }
}

/// `IFCPOSITIVELENGTHMEASURE` -> `IfcPositiveLengthMeasure`.
fn measure_type_name(step_name: &str) -> String {
    const WORDS: &[&str] = &[
        "Positive",
        "Normalised",
        "Negative",
        "Length",
        "Area",
        "Volume",
        "Plane",
        "Angle",
        "Measure",
        "Label",
        "Text",
        "Identifier",
        "Boolean",
        "Logical",
        "Integer",
        "Real",
        "Count",
        "Mass",
        "Density",
        "Power",
        "Volumetric",
        "Flow",
        "Rate",
        "Ratio",
        "Pressure",
        "Electric",
        "Current",
        "Voltage",
        "Thermodynamic",
        "Temperature",
        "Thermal",
        "Transmittance",
        "Time",
        "Descriptive",
        "Numeric",
        "Force",
        "Luminous",
        "Flux",
        "Frequency",
        "Date",
        "Duration",
        "Resistance",
        "Conductivity",
        "Heat",
        "Fire",
        "Sound",
        "Acoustic",
        "Moment",
        "Linear",
        "Specific",
        "Energy",
        "Illuminance",
        "Monetary",
        "Speed",
        "Velocity",
        "Weight",
    ];
    let upper = step_name.trim_start_matches("IFC");
    let mut out = String::from("Ifc");
    let mut rest = upper;
    'outer: while !rest.is_empty() {
        // Longest known word first, so "TEMPERATURE" wins over "TEMP"-like prefixes.
        let mut best: Option<&str> = None;
        for w in WORDS {
            if rest.len() >= w.len()
                && rest[..w.len()].eq_ignore_ascii_case(w)
                && best.is_none_or(|b| w.len() > b.len())
            {
                best = Some(w);
            }
        }
        if let Some(w) = best {
            out.push_str(w);
            rest = &rest[w.len()..];
            continue 'outer;
        }
        // Unknown remainder: keep it readable rather than exact.
        let mut chars = rest.chars();
        if let Some(first) = chars.next() {
            out.push(first.to_ascii_uppercase());
            out.push_str(&chars.as_str().to_ascii_lowercase());
        }
        break;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_type_names_are_camel_cased() {
        assert_eq!(measure_type_name("IFCLABEL"), "IfcLabel");
        assert_eq!(
            measure_type_name("IFCPOSITIVELENGTHMEASURE"),
            "IfcPositiveLengthMeasure"
        );
        assert_eq!(
            measure_type_name("IFCTHERMALTRANSMITTANCEMEASURE"),
            "IfcThermalTransmittanceMeasure"
        );
    }

    #[test]
    fn reads_originating_system_from_header() {
        let header = b"ISO-10303-21;\nHEADER;\nFILE_NAME('a.ifc','2024-01-01',('x'),('y'),'Pre 1.0','Revit 2024','');\nENDSEC;";
        assert_eq!(originating_system(header).as_deref(), Some("Revit 2024"));
        assert_eq!(file_name_arg(header, 1).as_deref(), Some("2024-01-01"));
    }

    #[test]
    fn rejects_non_ifc_input() {
        assert!(matches!(
            load_ifc_bytes("x.ifc", b"hello"),
            Err(LoadError::NotIfc)
        ));
    }
}
