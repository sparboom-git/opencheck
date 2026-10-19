use glam::{DMat4, DVec3, Vec3};

/// Axis-aligned bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// An inverted box that any point will expand.
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        let mut aabb = Self::EMPTY;
        for p in points {
            aabb.expand(p);
        }
        aabb
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x || self.min.y > self.max.y || self.min.z > self.max.z
    }

    pub fn expand(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn union(&self, other: &Aabb) -> Aabb {
        Aabb {
            min: self.min.min(other.min),
            max: self.max.max(other.max),
        }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn size(&self) -> Vec3 {
        self.max - self.min
    }

    pub fn contains_point(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }

    pub fn intersects(&self, other: &Aabb) -> bool {
        self.min.cmple(other.max).all() && other.min.cmple(self.max).all()
    }

    /// The box around this box after a transform (the eight corners, re-boxed).
    pub fn transformed(&self, m: &DMat4) -> Aabb {
        if self.is_empty() {
            return *self;
        }
        let (lo, hi) = (self.min.as_dvec3(), self.max.as_dvec3());
        Aabb::from_points((0..8).map(|i| {
            let c = DVec3::new(
                if i & 1 == 0 { lo.x } else { hi.x },
                if i & 2 == 0 { lo.y } else { hi.y },
                if i & 4 == 0 { lo.z } else { hi.z },
            );
            m.transform_point3(c).as_vec3()
        }))
    }
}

/// One triangle mesh belonging to an element.
///
/// An element may own several meshes (one per representation item or style).
/// Coordinates are in metres in the model's local frame, see
/// [`IfcModel::local_to_world`](crate::IfcModel::local_to_world). The IFC
/// convention of Z up is kept.
#[derive(Debug, Clone)]
pub struct ElementMesh {
    /// Express ID of the owning element.
    pub express_id: u32,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Linear RGBA in the 0-1 range, taken from the IFC surface style.
    pub color: [f32; 4],
    pub bounds: Aabb,
}

impl ElementMesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn is_transparent(&self) -> bool {
        self.color[3] < 0.99
    }
}
