//! Snapping for point-to-point measuring.
//!
//! A tessellated mesh has many edges that are not real: the diagonals that
//! split a flat face into triangles. Snapping only considers *feature edges*,
//! where the surface actually folds (or ends), and *corners*, where feature
//! edges meet at an angle. Both are computed once per mesh and cached.

use std::sync::Arc;

use glam::{Mat4, Vec3};
use opencheck_core::{ElementMesh, ModelId};
use rustc_hash::FxHashMap;

use crate::camera::Ray;

/// Faces meeting at more than this angle form a visible edge.
const FEATURE_ANGLE_DEG: f32 = 20.0;
/// Edges continuing within this angle do not make a corner.
const CORNER_ANGLE_DEG: f32 = 10.0;
/// Vertices closer than this (metres) are treated as the same point.
const WELD: f32 = 1e-4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind {
    Corner,
    Edge,
    Surface,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snap {
    pub point: Vec3,
    pub kind: SnapKind,
}

/// Feature edges and corners of one mesh, in the mesh's local frame.
#[derive(Debug, Default)]
pub struct FeatureEdges {
    pub edges: Vec<[Vec3; 2]>,
    pub corners: Vec<Vec3>,
}

impl FeatureEdges {
    pub fn from_mesh(mesh: &ElementMesh) -> Self {
        // Weld split vertices: flat-shaded meshes duplicate every corner.
        let key = |p: [f32; 3]| p.map(|c| (c / WELD).round() as i64);
        let mut ids: FxHashMap<[i64; 3], u32> = FxHashMap::default();
        let mut points: Vec<Vec3> = Vec::new();
        let mut welded = |p: [f32; 3]| -> u32 {
            *ids.entry(key(p)).or_insert_with(|| {
                points.push(Vec3::from_array(p));
                points.len() as u32 - 1
            })
        };
        let vertex_ids: Vec<u32> = mesh.positions.iter().map(|p| welded(*p)).collect();

        // Normals of the faces on each side of every edge.
        let mut edge_faces: FxHashMap<(u32, u32), Vec<Vec3>> = FxHashMap::default();
        for tri in mesh.indices.as_chunks::<3>().0 {
            let [a, b, c] = tri.map(|i| vertex_ids[i as usize]);
            if a == b || b == c || a == c {
                continue;
            }
            let normal = (points[b as usize] - points[a as usize])
                .cross(points[c as usize] - points[a as usize])
                .normalize_or_zero();
            if normal == Vec3::ZERO {
                continue;
            }
            for (u, v) in [(a, b), (b, c), (c, a)] {
                edge_faces
                    .entry((u.min(v), u.max(v)))
                    .or_default()
                    .push(normal);
            }
        }

        let cos_feature = FEATURE_ANGLE_DEG.to_radians().cos();
        let mut edges = Vec::new();
        let mut incident: FxHashMap<u32, Vec<Vec3>> = FxHashMap::default();
        for ((u, v), normals) in edge_faces {
            // Open boundaries and non-manifold edges count, as do folds.
            let is_feature = normals.len() != 2 || normals[0].dot(normals[1]).abs() < cos_feature;
            if !is_feature {
                continue;
            }
            let (pu, pv) = (points[u as usize], points[v as usize]);
            edges.push([pu, pv]);
            let dir = (pv - pu).normalize_or_zero();
            incident.entry(u).or_default().push(dir);
            incident.entry(v).or_default().push(-dir);
        }

        // A corner is where feature edges meet and do not simply continue.
        let cos_corner = CORNER_ANGLE_DEG.to_radians().cos();
        let corners = incident
            .into_iter()
            .filter(|(_, dirs)| match dirs.as_slice() {
                [a, b] => a.dot(-*b) < cos_corner,
                _ => true,
            })
            .map(|(id, _)| points[id as usize])
            .collect();

        Self { edges, corners }
    }
}

/// Feature edges per mesh, built on first use.
#[derive(Debug, Default)]
pub struct EdgeCache {
    meshes: FxHashMap<(ModelId, usize), Arc<FeatureEdges>>,
}

impl EdgeCache {
    pub fn get(
        &mut self,
        model: ModelId,
        mesh_index: usize,
        mesh: &ElementMesh,
    ) -> Arc<FeatureEdges> {
        self.meshes
            .entry((model, mesh_index))
            .or_insert_with(|| Arc::new(FeatureEdges::from_mesh(mesh)))
            .clone()
    }

    pub fn forget_model(&mut self, model: ModelId) {
        self.meshes.retain(|(m, _), _| *m != model);
    }
}

/// Closest points between a ray and a segment: returns (t on ray, point on segment).
pub fn ray_segment_closest(ray: &Ray, a: Vec3, b: Vec3) -> (f32, Vec3) {
    let seg = b - a;
    let w = ray.origin - a;
    let (aa, bb, cc) = (ray.dir.dot(ray.dir), ray.dir.dot(seg), seg.dot(seg));
    let (dd, ee) = (ray.dir.dot(w), seg.dot(w));
    let denom = aa * cc - bb * bb;
    let s = if denom.abs() < 1e-9 || cc < 1e-12 {
        0.0
    } else {
        ((aa * ee - bb * dd) / denom).clamp(0.0, 1.0)
    };
    let on_segment = a + seg * s;
    let t = (on_segment - ray.origin).dot(ray.dir) / aa;
    (t, on_segment)
}

/// Candidate geometry near the cursor, already in the federation frame.
pub struct Candidates {
    pub corners: Vec<Vec3>,
    pub edges: Vec<[Vec3; 2]>,
}

impl Candidates {
    pub fn new() -> Self {
        Self {
            corners: Vec::new(),
            edges: Vec::new(),
        }
    }

    pub fn add(&mut self, features: &FeatureEdges, transform: &Mat4) {
        self.corners.extend(
            features
                .corners
                .iter()
                .map(|p| transform.transform_point3(*p)),
        );
        self.edges.extend(
            features
                .edges
                .iter()
                .map(|e| e.map(|p| transform.transform_point3(p))),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencheck_core::Aabb;

    /// A flat-shaded unit cube: 24 vertices, 12 triangles.
    fn cube() -> ElementMesh {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        let faces: [[[f32; 3]; 4]; 6] = [
            [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
            [[0., 0., 1.], [0., 1., 1.], [1., 1., 1.], [1., 0., 1.]],
            [[0., 0., 0.], [0., 0., 1.], [1., 0., 1.], [1., 0., 0.]],
            [[0., 1., 0.], [1., 1., 0.], [1., 1., 1.], [0., 1., 1.]],
            [[0., 0., 0.], [0., 1., 0.], [0., 1., 1.], [0., 0., 1.]],
            [[1., 0., 0.], [1., 0., 1.], [1., 1., 1.], [1., 1., 0.]],
        ];
        for face in faces {
            let base = positions.len() as u32;
            positions.extend(face);
            indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        ElementMesh {
            express_id: 1,
            normals: vec![[0.0, 0.0, 1.0]; positions.len()],
            bounds: Aabb::from_points(positions.iter().map(|p| Vec3::from_array(*p))),
            positions,
            indices,
            color: [1.0; 4],
        }
    }

    #[test]
    fn cube_has_twelve_edges_and_eight_corners() {
        let features = FeatureEdges::from_mesh(&cube());
        // The face diagonals are not feature edges.
        assert_eq!(features.edges.len(), 12);
        assert_eq!(features.corners.len(), 8);
    }

    #[test]
    fn closest_point_on_segment() {
        let ray = Ray {
            origin: Vec3::new(0.5, -5.0, 1.0),
            dir: Vec3::Y,
        };
        let (t, p) = ray_segment_closest(&ray, Vec3::ZERO, Vec3::X);
        assert!((p - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-5);
        assert!((t - 5.0).abs() < 1e-5);
    }
}
