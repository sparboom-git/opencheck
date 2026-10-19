//! Geometric primitives for checking: triangle intersection, distances and
//! point-in-solid tests.

use glam::Vec3;

use crate::Aabb;

pub type Triangle = [Vec3; 3];

pub fn triangle_aabb(t: &Triangle) -> Aabb {
    Aabb {
        min: t[0].min(t[1]).min(t[2]),
        max: t[0].max(t[1]).max(t[2]),
    }
}

/// Where the segment `p`-`q` crosses triangle `t`, if it does.
/// Segments lying in the triangle's plane do not count.
pub fn segment_triangle(p: Vec3, q: Vec3, t: &Triangle) -> Option<Vec3> {
    let dir = q - p;
    let e1 = t[1] - t[0];
    let e2 = t[2] - t[0];
    let h = dir.cross(e2);
    let det = e1.dot(h);
    // Scale-aware parallel test: |det| relative to edge and segment lengths.
    if det.abs() <= 1e-7 * e1.length() * e2.length() * dir.length() {
        return None;
    }
    let inv = 1.0 / det;
    let s = p - t[0];
    let u = s.dot(h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = s.cross(e1);
    let v = dir.dot(qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let s_param = e2.dot(qv) * inv;
    (0.0..=1.0).contains(&s_param).then(|| p + dir * s_param)
}

/// Points where two triangles cross. Empty when they only touch in a common
/// plane, which is how adjoining elements meet.
pub fn triangle_intersection(a: &Triangle, b: &Triangle, out: &mut Vec<Vec3>) -> bool {
    let before = out.len();
    for (x, y) in [(0, 1), (1, 2), (2, 0)] {
        if let Some(p) = segment_triangle(a[x], a[y], b) {
            out.push(p);
        }
        if let Some(p) = segment_triangle(b[x], b[y], a) {
            out.push(p);
        }
    }
    out.len() > before
}

/// Closest point on a triangle (Ericson, Real-Time Collision Detection 5.1.5).
pub fn closest_point_on_triangle(p: Vec3, t: &Triangle) -> Vec3 {
    let [a, b, c] = *t;
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Whether `p` lies inside a closed mesh, by counting crossings of a ray.
/// The ray direction is deliberately skewed so it rarely runs along edges.
pub fn point_in_mesh(p: Vec3, triangles: &[Triangle]) -> bool {
    const DIR: Vec3 = Vec3::new(0.577_215_7, 0.316_227_8, 0.752_691_6);
    let far = p + DIR * 1.0e5;
    let crossings = triangles
        .iter()
        .filter(|t| segment_triangle(p, far, t).is_some())
        .count();
    crossings % 2 == 1
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Closed box mesh, 12 triangles.
    pub fn box_triangles(min: Vec3, max: Vec3) -> Vec<Triangle> {
        let c = |x: bool, y: bool, z: bool| {
            Vec3::new(
                if x { max.x } else { min.x },
                if y { max.y } else { min.y },
                if z { max.z } else { min.z },
            )
        };
        let quads = [
            [
                c(false, false, false),
                c(true, false, false),
                c(true, true, false),
                c(false, true, false),
            ],
            [
                c(false, false, true),
                c(false, true, true),
                c(true, true, true),
                c(true, false, true),
            ],
            [
                c(false, false, false),
                c(false, false, true),
                c(true, false, true),
                c(true, false, false),
            ],
            [
                c(false, true, false),
                c(true, true, false),
                c(true, true, true),
                c(false, true, true),
            ],
            [
                c(false, false, false),
                c(false, true, false),
                c(false, true, true),
                c(false, false, true),
            ],
            [
                c(true, false, false),
                c(true, false, true),
                c(true, true, true),
                c(true, true, false),
            ],
        ];
        quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect()
    }

    #[test]
    fn crossing_triangles_meet_in_a_segment() {
        let a = [
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ];
        let b = [
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ];
        let mut points = Vec::new();
        assert!(triangle_intersection(&a, &b, &mut points));
        assert!(points.len() >= 2);
    }

    #[test]
    fn coplanar_touching_is_not_an_intersection() {
        let a = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let b = [
            Vec3::new(0.2, 0.2, 0.0),
            Vec3::new(1.0, 0.2, 0.0),
            Vec3::new(0.2, 1.0, 0.0),
        ];
        let mut points = Vec::new();
        assert!(!triangle_intersection(&a, &b, &mut points));
    }

    #[test]
    fn inside_and_outside_a_box() {
        let tris = box_triangles(Vec3::ZERO, Vec3::ONE);
        assert!(point_in_mesh(Vec3::splat(0.5), &tris));
        assert!(!point_in_mesh(Vec3::splat(1.5), &tris));
        assert!(!point_in_mesh(Vec3::new(0.5, 0.5, -0.1), &tris));
    }

    #[test]
    fn closest_point() {
        let t = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let p = closest_point_on_triangle(Vec3::new(0.2, 0.2, 3.0), &t);
        assert!((p - Vec3::new(0.2, 0.2, 0.0)).length() < 1e-6);
        let corner = closest_point_on_triangle(Vec3::new(-1.0, -1.0, 0.0), &t);
        assert!(corner.length() < 1e-6);
    }
}
