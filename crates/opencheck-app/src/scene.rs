//! Viewer state on top of the federation: which elements are visible,
//! selected or ghosted, and how they map to GPU slots.

use glam::{Mat3, Mat4, Vec3};
use opencheck_core::{Aabb, Element, Federation, IfcModel, ModelId};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::camera::Ray;
use crate::render::{DisplayMode, GeometryChunk, MAX_CHUNK_VERTICES, PAINTED, Vertex};

/// Identifies one element in the federation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementRef {
    pub model: ModelId,
    pub express_id: u32,
}

/// Classes that describe space rather than built things. Hidden unless asked for.
pub fn is_space_class(class: &str) -> bool {
    matches!(class, "IfcSpace" | "IfcSpatialZone" | "IfcZone")
}

/// Per-model view data.
pub struct ViewModel {
    pub id: ModelId,
    pub visible: bool,
    /// Federation-frame transform used when the geometry was uploaded.
    pub transform: Mat4,
    slot_base: u32,
    /// Express IDs in slot order.
    slot_elements: Vec<u32>,
    slot_of: FxHashMap<u32, u32>,
    /// Name of the storey each slot belongs to, after following part-of links.
    slot_storey: Vec<Option<String>>,
    slot_is_space: Vec<bool>,
    /// Element bounds in the federation frame, per slot.
    slot_bounds: Vec<Aabb>,
}

impl ViewModel {
    pub fn slot(&self, express_id: u32) -> Option<u32> {
        self.slot_of.get(&express_id).copied()
    }

    pub fn element_bounds(&self, express_id: u32) -> Option<Aabb> {
        self.slot(express_id)
            .map(|s| self.slot_bounds[s as usize])
            .filter(|b| !b.is_empty())
    }
}

#[derive(Default)]
pub struct ViewState {
    pub models: Vec<ViewModel>,
    next_slot: u32,
    pub selection: Vec<ElementRef>,
    pub hover: Option<ElementRef>,
    pub hidden: FxHashSet<ElementRef>,
    /// Outside the working area: not drawn and not picked.
    pub outside: FxHashSet<ElementRef>,
    /// Outside the area being drawn: shown faintly while editing it.
    pub ghosted: FxHashSet<ElementRef>,
    /// When set, only these elements are shown.
    pub isolated: Option<FxHashSet<ElementRef>>,
    pub xray: bool,
    pub show_spaces: bool,
    /// Show only elements on this storey (matched by name across models).
    pub storey_filter: Option<String>,
    pub section: Option<SectionPlane>,
    /// Elements of the findings under review, shown in the checking colours.
    pub focus: Option<Focus>,
    /// While reviewing a finding: show the rest of the model normally instead of ghosted.
    pub show_context: bool,
    /// Colours from a visual check, drawn instead of the model's own.
    pub paint: Option<FxHashMap<ElementRef, [u8; 3]>>,
    /// With `paint`: ghost the elements it does not colour.
    pub paint_ghost_rest: bool,
    /// Set whenever something that affects element modes changes.
    pub modes_dirty: bool,
}

impl ViewState {
    pub fn model(&self, id: ModelId) -> Option<&ViewModel> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn model_mut(&mut self, id: ModelId) -> Option<&mut ViewModel> {
        self.models.iter_mut().find(|m| m.id == id)
    }

    /// Registers a model, assigns GPU slots and builds its vertex data.
    pub fn add_model(
        &mut self,
        id: ModelId,
        model: &IfcModel,
        transform: Mat4,
    ) -> Vec<GeometryChunk> {
        let slot_base = self.next_slot;
        let storey_names = storey_names(model);
        let mut slot_elements = Vec::with_capacity(model.elements.len());
        let mut slot_of = FxHashMap::default();
        let mut slot_storey = Vec::with_capacity(model.elements.len());
        let mut slot_is_space = Vec::with_capacity(model.elements.len());
        let mut slot_bounds = Vec::with_capacity(model.elements.len());
        let transform_d = transform.as_dmat4();
        for element in &model.elements {
            slot_of.insert(element.express_id, slot_elements.len() as u32);
            slot_elements.push(element.express_id);
            slot_storey.push(storey_of(model, element, &storey_names));
            slot_is_space.push(is_space_class(&element.ifc_class));
            slot_bounds.push(element.bounds.transformed(&transform_d));
        }
        self.next_slot += slot_elements.len() as u32;

        let normal_matrix = Mat3::from_mat4(transform).inverse().transpose();
        // Large models are split into several GPU buffers; a mesh never straddles two.
        let mut chunks = vec![GeometryChunk::default()];
        for mesh in &model.meshes {
            let Some(&local_slot) = slot_of.get(&mesh.express_id) else {
                continue;
            };
            let current = chunks.last().expect("chunks starts non-empty");
            if !current.vertices.is_empty()
                && current.vertices.len() + mesh.positions.len() > MAX_CHUNK_VERTICES
            {
                chunks.push(GeometryChunk::default());
            }
            let chunk = chunks.last_mut().expect("chunks starts non-empty");
            let slot = slot_base + local_slot;
            let color = mesh
                .color
                .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
            let base = chunk.vertices.len() as u32;
            for (i, p) in mesh.positions.iter().enumerate() {
                let n = mesh.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
                let n = (normal_matrix * Vec3::from_array(n)).normalize_or_zero();
                chunk.vertices.push(Vertex {
                    position: transform.transform_point3(Vec3::from_array(*p)).to_array(),
                    normal: pack_normal(n),
                    color,
                    slot,
                });
            }
            chunk.indices.extend(mesh.indices.iter().map(|i| base + i));
        }
        chunks.retain(|c| !c.indices.is_empty());

        self.models.push(ViewModel {
            id,
            visible: true,
            transform,
            slot_base,
            slot_elements,
            slot_of,
            slot_storey,
            slot_is_space,
            slot_bounds,
        });
        self.modes_dirty = true;
        chunks
    }

    pub fn remove_model(&mut self, id: ModelId) {
        self.models.retain(|m| m.id != id);
        self.selection.retain(|e| e.model != id);
        self.hidden.retain(|e| e.model != id);
        self.outside.retain(|e| e.model != id);
        if let Some(iso) = &mut self.isolated {
            iso.retain(|e| e.model != id);
        }
        self.modes_dirty = true;
    }

    /// Storey names across all models, ordered by first appearance.
    pub fn storey_names(&self, federation: &Federation) -> Vec<String> {
        let mut names: Vec<(f64, String)> = Vec::new();
        for (_, model) in federation.iter() {
            for storey in model.storeys() {
                let name = storey.label().to_owned();
                if !names.iter().any(|(_, n)| *n == name) {
                    let elevation = storey.elevation.unwrap_or(0.0) * model.length_unit_scale;
                    names.push((elevation, name));
                }
            }
        }
        names.sort_by(|a, b| a.0.total_cmp(&b.0));
        names.into_iter().map(|(_, n)| n).collect()
    }

    fn mode_of(&self, model: &ViewModel, local_slot: usize) -> DisplayMode {
        let element = ElementRef {
            model: model.id,
            express_id: model.slot_elements[local_slot],
        };
        // A finding under review: its two elements in colour, everything else ghosted.
        if let Some(focus) = &self.focus {
            if focus.a.contains(&element) {
                return DisplayMode::FocusA;
            }
            if focus.b.contains(&element) {
                return DisplayMode::FocusB;
            }
        }
        let hidden = self.hidden.contains(&element)
            || self.outside.contains(&element)
            || (model.slot_is_space[local_slot] && !self.show_spaces)
            || self
                .isolated
                .as_ref()
                .is_some_and(|iso| !iso.contains(&element))
            || self
                .storey_filter
                .as_ref()
                .is_some_and(|s| model.slot_storey[local_slot].as_deref() != Some(s.as_str()));
        if hidden {
            DisplayMode::Hidden
        } else if self.ghosted.contains(&element) || (self.focus.is_some() && !self.show_context) {
            DisplayMode::Ghost
        } else if self.selection.contains(&element) {
            DisplayMode::Selected
        } else if self.hover == Some(element) {
            DisplayMode::Hover
        } else if self.xray {
            DisplayMode::Ghost
        } else {
            DisplayMode::Normal
        }
    }

    pub fn display_mode(&self, element: ElementRef) -> DisplayMode {
        self.model(element.model)
            .and_then(|m| m.slot(element.express_id).map(|s| (m, s)))
            .map(|(m, s)| self.mode_of(m, s as usize))
            .unwrap_or(DisplayMode::Hidden)
    }

    /// What the GPU gets for a slot: its display mode, or a painted colour
    /// (see [`PAINTED`]) where a visual check colours the element.
    fn mode_word(&self, model: &ViewModel, local: usize) -> u32 {
        let mode = self.mode_of(model, local);
        let Some(paint) = &self.paint else {
            return mode as u32;
        };
        let element = ElementRef {
            model: model.id,
            express_id: model.slot_elements[local],
        };
        match (mode, paint.get(&element)) {
            (DisplayMode::Normal | DisplayMode::Hover | DisplayMode::Ghost, Some([r, g, b])) => {
                PAINTED | (u32::from(*r) << 16) | (u32::from(*g) << 8) | u32::from(*b)
            }
            (DisplayMode::Normal | DisplayMode::Hover, None) if self.paint_ghost_rest => {
                DisplayMode::Ghost as u32
            }
            _ => mode as u32,
        }
    }

    /// Name of the storey an element belongs to.
    pub fn storey_of(&self, element: ElementRef) -> Option<&str> {
        let model = self.model(element.model)?;
        let slot = model.slot(element.express_id)? as usize;
        model.slot_storey[slot].as_deref()
    }

    /// Display mode for every slot, ready for the GPU.
    pub fn build_modes(&self) -> Vec<u32> {
        let mut modes = vec![DisplayMode::Hidden as u32; self.next_slot.max(1) as usize];
        for model in &self.models {
            for local in 0..model.slot_elements.len() {
                modes[model.slot_base as usize + local] = self.mode_word(model, local);
            }
        }
        modes
    }

    /// Bounds of everything currently drawn.
    pub fn visible_bounds(&self) -> Aabb {
        let mut bounds = Aabb::EMPTY;
        for model in self.models.iter().filter(|m| m.visible) {
            for (local, b) in model.slot_bounds.iter().enumerate() {
                if self.mode_of(model, local) != DisplayMode::Hidden {
                    bounds = bounds.union(b);
                }
            }
        }
        bounds
    }

    pub fn bounds_of(&self, elements: &[ElementRef]) -> Aabb {
        elements
            .iter()
            .filter_map(|e| self.model(e.model)?.element_bounds(e.express_id))
            .fold(Aabb::EMPTY, |acc, b| acc.union(&b))
    }

    /// Drawn, pickable elements whose bounds come within `radius` of `point`,
    /// with the transform of their model and their index in the model.
    pub fn elements_near(&self, point: Vec3, radius: f32) -> Vec<(ModelId, Mat4, usize)> {
        let mut near = Vec::new();
        for view in self.models.iter().filter(|m| m.visible) {
            for (local, bounds) in view.slot_bounds.iter().enumerate() {
                if bounds.is_empty() {
                    continue;
                }
                let closest = point.clamp(bounds.min, bounds.max);
                if closest.distance(point) > radius {
                    continue;
                }
                if matches!(
                    self.mode_of(view, local),
                    DisplayMode::Hidden | DisplayMode::Ghost
                ) {
                    continue;
                }
                near.push((view.id, view.transform, local));
            }
        }
        near
    }

    /// Whether a point is kept by the section plane.
    pub fn is_visible_point(&self, p: Vec3) -> bool {
        self.inside_section(p)
    }

    /// Nearest element hit by a ray in the federation frame.
    pub fn pick(&self, federation: &Federation, ray: &Ray) -> Option<PickHit> {
        let mut best: Option<PickHit> = None;
        for view in self.models.iter().filter(|m| m.visible) {
            let Some(model) = federation.get(view.id) else {
                continue;
            };
            let inverse = view.transform.inverse();
            let local_ray = Ray {
                origin: inverse.transform_point3(ray.origin),
                dir: inverse.transform_vector3(ray.dir).normalize(),
            };
            for (local, element) in model.elements.iter().enumerate() {
                let mode = self.mode_of(view, local);
                if matches!(mode, DisplayMode::Hidden | DisplayMode::Ghost) {
                    continue;
                }
                let limit = best.as_ref().map_or(f32::INFINITY, |b| b.distance);
                match ray_aabb(&local_ray, &element.bounds) {
                    Some(t) if t < limit => {}
                    _ => continue,
                }
                for &mesh_index in &element.meshes {
                    let mesh = &model.meshes[mesh_index];
                    for tri in mesh.indices.as_chunks::<3>().0 {
                        let [a, b, c] = [tri[0], tri[1], tri[2]]
                            .map(|i| Vec3::from_array(mesh.positions[i as usize]));
                        if let Some(t) = ray_triangle(&local_ray, a, b, c) {
                            let point = view.transform.transform_point3(local_ray.at(t));
                            let distance = point.distance(ray.origin);
                            if best.as_ref().is_none_or(|b| distance < b.distance)
                                && self.inside_section(point)
                            {
                                // Face normal turned towards the ray, so it points
                                // out of the surface the ray hit.
                                let mut normal = view
                                    .transform
                                    .transform_vector3((b - a).cross(c - a))
                                    .normalize_or_zero();
                                if normal.dot(ray.dir) > 0.0 {
                                    normal = -normal;
                                }
                                best = Some(PickHit {
                                    element: ElementRef {
                                        model: view.id,
                                        express_id: element.express_id,
                                    },
                                    point,
                                    normal,
                                    distance,
                                });
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Whether something drawn lies on the ray before `max_distance`,
    /// leaving out the elements in `skip` (e.g. the two sides of a clash,
    /// whose point lies inside them). Stops at the first hit.
    pub fn blocked(
        &self,
        federation: &Federation,
        ray: &Ray,
        max_distance: f32,
        skip: &[ElementRef],
    ) -> bool {
        for view in self.models.iter().filter(|m| m.visible) {
            let Some(model) = federation.get(view.id) else {
                continue;
            };
            let inverse = view.transform.inverse();
            let local_ray = Ray {
                origin: inverse.transform_point3(ray.origin),
                dir: inverse.transform_vector3(ray.dir).normalize(),
            };
            for (local, element) in model.elements.iter().enumerate() {
                let key = ElementRef {
                    model: view.id,
                    express_id: element.express_id,
                };
                if skip.contains(&key)
                    || matches!(
                        self.mode_of(view, local),
                        DisplayMode::Hidden | DisplayMode::Ghost
                    )
                {
                    continue;
                }
                match ray_aabb(&local_ray, &element.bounds) {
                    Some(t) if t < max_distance => {}
                    _ => continue,
                }
                for &mesh_index in &element.meshes {
                    let mesh = &model.meshes[mesh_index];
                    for tri in mesh.indices.as_chunks::<3>().0 {
                        let [a, b, c] = [tri[0], tri[1], tri[2]]
                            .map(|i| Vec3::from_array(mesh.positions[i as usize]));
                        if let Some(t) = ray_triangle(&local_ray, a, b, c) {
                            let point = view.transform.transform_point3(local_ray.at(t));
                            if point.distance(ray.origin) < max_distance
                                && self.inside_section(point)
                            {
                                return true;
                            }
                        }
                    }
                }
            }
        }
        false
    }

    fn inside_section(&self, p: Vec3) -> bool {
        self.section.is_none_or(|s| s.keeps(p))
    }
}

/// The two sides of one or more findings: side a in orange, side b in blue.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Focus {
    pub a: FxHashSet<ElementRef>,
    pub b: FxHashSet<ElementRef>,
}

impl Focus {
    pub fn involves_model(&self, model: ModelId) -> bool {
        self.a.iter().chain(&self.b).any(|e| e.model == model)
    }
}

/// A section plane: everything on the side `normal` points to is cut
/// away. Usually horizontal (normal up: the model above is cut away), or
/// parallel to a face picked in the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionPlane {
    /// A point on the plane, in the federation frame; the scissors handle
    /// sits there.
    pub point: Vec3,
    /// Unit normal, towards the side that is cut away.
    pub normal: Vec3,
}

impl SectionPlane {
    /// A horizontal cut at `height`, the handle above `anchor` in plan.
    pub fn horizontal(height: f32, anchor: Vec3) -> Self {
        Self {
            point: anchor.with_z(height),
            normal: Vec3::Z,
        }
    }

    pub fn is_horizontal(&self) -> bool {
        self.normal.z > 0.999
    }

    /// Whether a point is kept (on the side away from the normal).
    pub fn keeps(&self, p: Vec3) -> bool {
        (p - self.point).dot(self.normal) <= 0.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PickHit {
    pub element: ElementRef,
    pub point: Vec3,
    /// Unit normal of the hit face, facing the ray origin.
    pub normal: Vec3,
    pub distance: f32,
}

/// Unit normal to `Snorm8x4`; plenty of precision for shading.
fn pack_normal(n: Vec3) -> [i8; 4] {
    let q = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
    [q(n.x), q(n.y), q(n.z), 0]
}

/// Storey node id -> storey name.
fn storey_names(model: &IfcModel) -> FxHashMap<u32, String> {
    model
        .storeys()
        .into_iter()
        .map(|s| (s.express_id, s.label().to_owned()))
        .collect()
}

/// The storey an element is on, following "part of" links up to the
/// element that is actually contained in the storey.
fn storey_of(
    model: &IfcModel,
    element: &Element,
    storeys: &FxHashMap<u32, String>,
) -> Option<String> {
    let mut current = element;
    for _ in 0..16 {
        if let Some(container) = current.container {
            return storeys.get(&container).cloned();
        }
        current = model.element(current.parent?)?;
    }
    None
}

/// Slab test; returns the entry distance (0 when the origin is inside).
fn ray_aabb(ray: &Ray, b: &Aabb) -> Option<f32> {
    if b.is_empty() {
        return None;
    }
    let inv = ray.dir.recip();
    let t1 = (b.min - ray.origin) * inv;
    let t2 = (b.max - ray.origin) * inv;
    let t_min = t1.min(t2).max_element();
    let t_max = t1.max(t2).min_element();
    (t_max >= t_min.max(0.0)).then_some(t_min.max(0.0))
}

/// Möller-Trumbore, two-sided.
fn ray_triangle(ray: &Ray, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let e1 = b - a;
    let e2 = c - a;
    let p = ray.dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv_det = 1.0 / det;
    let s = ray.origin - a;
    let u = s.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = ray.dir.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv_det;
    (t > 1e-5).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ray_hits_triangle_and_box() {
        let ray = Ray {
            origin: Vec3::new(0.2, 0.2, 5.0),
            dir: Vec3::NEG_Z,
        };
        let t = ray_triangle(&ray, Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
        assert!((t - 5.0).abs() < 1e-5);
        let b = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        assert!((ray_aabb(&ray, &b).unwrap() - 4.0).abs() < 1e-5);
        let miss = Ray {
            origin: Vec3::new(3.0, 3.0, 5.0),
            dir: Vec3::NEG_Z,
        };
        assert!(ray_aabb(&miss, &b).is_none());
    }
}
