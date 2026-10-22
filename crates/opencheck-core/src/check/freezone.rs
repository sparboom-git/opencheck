//! Free zones: space an element needs around it along its whole run, such as
//! the service area beside a cable tray or the headroom above it.
//!
//! Each element gets a run frame: its longest axis (the run), the axis across
//! it and its normal (up for horizontal runs). A zone is a box in that frame
//! beside, above or below the element. Other elements block stretches of the
//! zone; the stretches are measured along the run, so a rule can allow short
//! interruptions and report long ones.

use glam::Vec3;

use super::geom::{Triangle, point_in_mesh};
use super::ruleset::{RunDirection, ZoneSide};
use crate::Aabb;

/// The element's own axes: `axis` along the run, `across` sideways and
/// `normal` up (for horizontal runs) or out of the broad face (vertical).
/// Ranges are the element's extent along each axis from `origin`, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunFrame {
    pub origin: Vec3,
    pub axis: Vec3,
    pub across: Vec3,
    pub normal: Vec3,
    pub along: [f32; 2],
    pub side: [f32; 2],
    pub up: [f32; 2],
}

/// Runs within this angle of vertical count as vertical (cos 30 degrees).
const VERTICAL_COS: f32 = 0.866;

impl RunFrame {
    /// The frame of an element from its triangles. None for a degenerate
    /// element without volume.
    pub fn of(triangles: &[Triangle]) -> Option<Self> {
        let points: Vec<Vec3> = triangles.iter().flatten().copied().collect();
        if points.len() < 3 {
            return None;
        }
        let n = points.len() as f32;
        let mean = points.iter().copied().sum::<Vec3>() / n;
        // Covariance of the vertices; its main eigenvector is the run.
        let mut cov = [[0.0f32; 3]; 3];
        for p in &points {
            let d = (*p - mean).to_array();
            for (i, row) in cov.iter_mut().enumerate() {
                for (j, c) in row.iter_mut().enumerate() {
                    *c += d[i] * d[j];
                }
            }
        }
        let mul = |m: &[[f32; 3]; 3], v: Vec3| {
            Vec3::new(
                m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
                m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
                m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
            )
        };
        let size = Aabb::from_points(points.iter().copied()).size();
        // Start from the longest box side so the iteration settles quickly.
        let mut axis = if size.x >= size.y && size.x >= size.z {
            Vec3::X
        } else if size.y >= size.z {
            Vec3::Y
        } else {
            Vec3::Z
        };
        for _ in 0..64 {
            let next = mul(&cov, axis);
            if next.length_squared() < 1e-20 {
                break;
            }
            axis = next.normalize();
        }
        // Extruded elements (trays, ducts, pipes, beams) have several long
        // edges exactly along the run; diagonals of the faces do not share a
        // direction that often. Prefer that direction to the vertex average,
        // which the triangulation skews slightly.
        if let Some(edge_axis) = shared_edge_direction(triangles) {
            axis = edge_axis;
        }
        // Make the direction deterministic: pointing up, then towards +x, +y.
        if axis.z < -1e-4
            || (axis.z.abs() <= 1e-4 && (axis.x < -1e-4 || (axis.x.abs() <= 1e-4 && axis.y < 0.0)))
        {
            axis = -axis;
        }

        let (across, normal) = if axis.z.abs() >= VERTICAL_COS {
            // Vertical: the across axis lies along the broad face.
            let helper = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
            let u = axis.cross(helper).normalize();
            let v = axis.cross(u).normalize();
            let spread = |dir: Vec3| {
                let (lo, hi) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                    let t = (*p - mean).dot(dir);
                    (lo.min(t), hi.max(t))
                });
                hi - lo
            };
            // The face normal is the direction the element is thinnest in
            // (the widest direction would be a diagonal of the section).
            let mut best = (u, spread(u));
            for k in 1..90 {
                let a = std::f32::consts::PI * k as f32 / 90.0;
                let dir = u * a.cos() + v * a.sin();
                let w = spread(dir);
                if w < best.1 {
                    best = (dir, w);
                }
            }
            let normal = best.0;
            let across = normal.cross(axis).normalize();
            (across, normal)
        } else {
            let normal = (Vec3::Z - axis * axis.z).normalize();
            (normal.cross(axis).normalize(), normal)
        };

        let range = |dir: Vec3| {
            points.iter().fold([f32::MAX, f32::MIN], |r, p| {
                let t = (*p - mean).dot(dir);
                [r[0].min(t), r[1].max(t)]
            })
        };
        Some(Self {
            origin: mean,
            axis,
            across,
            normal,
            along: range(axis),
            side: range(across),
            up: range(normal),
        })
    }

    pub fn is_vertical(&self) -> bool {
        self.axis.z.abs() >= VERTICAL_COS
    }

    pub fn matches(&self, direction: RunDirection) -> bool {
        match direction {
            RunDirection::Any => true,
            RunDirection::Horizontal => self.axis.z.abs() <= 0.5,
            RunDirection::Vertical => self.is_vertical(),
        }
    }

    pub fn length(&self) -> f32 {
        self.along[1] - self.along[0]
    }

    pub fn width(&self) -> f32 {
        self.side[1] - self.side[0]
    }

    pub fn height(&self) -> f32 {
        self.up[1] - self.up[0]
    }

    fn local(&self, p: Vec3) -> Vec3 {
        let d = p - self.origin;
        Vec3::new(d.dot(self.axis), d.dot(self.across), d.dot(self.normal))
    }

    fn world(&self, l: Vec3) -> Vec3 {
        self.origin + self.axis * l.x + self.across * l.y + self.normal * l.z
    }
}

/// The direction most long edges share, if at least three do.
fn shared_edge_direction(triangles: &[Triangle]) -> Option<Vec3> {
    let mut edges: Vec<(Vec3, f32)> = triangles
        .iter()
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .filter_map(|(p, q)| {
            let d = q - p;
            let length = d.length();
            (length > 1e-6).then(|| {
                // One direction per line, whichever way the edge was drawn.
                let d = d / length;
                let flip = d.x < 0.0 || (d.x == 0.0 && (d.y < 0.0 || (d.y == 0.0 && d.z < 0.0)));
                (if flip { -d } else { d }, length)
            })
        })
        .collect();
    let longest = edges.iter().map(|e| e.1).fold(0.0f32, f32::max);
    edges.retain(|e| e.1 >= longest * 0.5);
    edges.sort_by(|a, b| b.1.total_cmp(&a.1));
    edges.truncate(256);
    const PARALLEL: f32 = 0.999_99;
    let mut best: Option<(usize, f32, Vec3)> = None;
    for (d, _) in &edges {
        let (count, total, sum) = edges.iter().fold((0, 0.0, Vec3::ZERO), |acc, (e, l)| {
            if d.dot(*e).abs() >= PARALLEL {
                let e = if d.dot(*e) < 0.0 { -*e } else { *e };
                (acc.0 + 1, acc.1 + l, acc.2 + e * *l)
            } else {
                acc
            }
        });
        if best.is_none_or(|(c, t, _)| count > c || (count == c && total > t)) {
            best = Some((count, total, sum));
        }
    }
    let (count, _, sum) = best?;
    (count >= 3).then(|| sum.normalize())
}

/// A box in a run frame: `along`, `side` and `up` ranges, metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneBox {
    pub frame: RunFrame,
    pub along: [f32; 2],
    pub side: [f32; 2],
    pub up: [f32; 2],
}

/// Zones keep this far from the ends of the element, so the next segment of
/// the same run, which meets it there, does not count as blocking.
const END_MARGIN: f32 = 0.05;
/// Surfaces this close to the zone's boundary only touch it.
const TOUCH: f32 = 0.002;

impl ZoneBox {
    /// The zones for `side`, `depth` metres deep. Several zones are
    /// alternatives or must all hold, see [`ZoneSide`].
    pub fn around(frame: &RunFrame, side: ZoneSide, depth: f32) -> Vec<ZoneBox> {
        let along = [
            frame.along[0] + END_MARGIN.min(frame.length() * 0.25),
            frame.along[1] - END_MARGIN.min(frame.length() * 0.25),
        ];
        let make = |s: [f32; 2], u: [f32; 2]| ZoneBox {
            frame: *frame,
            along,
            side: s,
            up: u,
        };
        let [s0, s1] = frame.side;
        let [u0, u1] = frame.up;
        match side {
            ZoneSide::Above => vec![make([s0, s1], [u1, u1 + depth])],
            ZoneSide::Below => vec![make([s0, s1], [u0 - depth, u0])],
            ZoneSide::BothSides | ZoneSide::OneSide => vec![
                make([s0 - depth, s0], [u0, u1]),
                make([s1, s1 + depth], [u0, u1]),
            ],
            ZoneSide::OneFace => vec![
                make([s0, s1], [u0 - depth, u0]),
                make([s0, s1], [u1, u1 + depth]),
            ],
        }
    }

    /// The element's own box over the same length: what a blocker crossing
    /// the element runs through.
    pub fn core(&self) -> ZoneBox {
        ZoneBox {
            side: self.frame.side,
            up: self.frame.up,
            ..*self
        }
    }

    /// The part of the zone between `from` and `to` along the run.
    pub fn stretch(&self, from: f32, to: f32) -> ZoneBox {
        ZoneBox {
            along: [from.max(self.along[0]), to.min(self.along[1])],
            ..*self
        }
    }

    pub fn corners(&self) -> [Vec3; 8] {
        let mut out = [Vec3::ZERO; 8];
        for (i, c) in out.iter_mut().enumerate() {
            let l = Vec3::new(
                self.along[i & 1],
                self.side[(i >> 1) & 1],
                self.up[(i >> 2) & 1],
            );
            *c = self.frame.world(l);
        }
        out
    }

    pub fn bounds(&self) -> Aabb {
        Aabb::from_points(self.corners())
    }

    /// Stretches along the run (metres from the frame origin) where the
    /// triangles of a blocker (with `bounds`) reach into the zone or enclose it.
    pub fn blocked_by(&self, triangles: &[Triangle], bounds: &Aabb, out: &mut Vec<[f32; 2]>) {
        let side = [self.side[0] + TOUCH, self.side[1] - TOUCH];
        let up = [self.up[0] + TOUCH, self.up[1] - TOUCH];
        if side[0] >= side[1] || up[0] >= up[1] {
            return;
        }
        let mut poly: Vec<Vec3> = Vec::with_capacity(9);
        let mut next: Vec<Vec3> = Vec::with_capacity(9);
        for t in triangles {
            poly.clear();
            poly.extend(t.iter().map(|p| self.frame.local(*p)));
            // Clip to the zone's cross-section and length (Sutherland-Hodgman).
            let planes: [(usize, f32, bool); 6] = [
                (1, side[0], true),
                (1, side[1], false),
                (2, up[0], true),
                (2, up[1], false),
                (0, self.along[0], true),
                (0, self.along[1], false),
            ];
            for (axis, value, keep_above) in planes {
                clip(&poly, axis, value, keep_above, &mut next);
                std::mem::swap(&mut poly, &mut next);
                if poly.is_empty() {
                    break;
                }
            }
            if poly.is_empty() {
                continue;
            }
            let (lo, hi) = poly.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                (lo.min(p.x), hi.max(p.x))
            });
            out.push([lo, hi]);
        }
        // A blocker can also enclose the zone without any surface inside it,
        // e.g. a large duct the tray runs through: sample the zone's centre.
        let step = 0.1;
        let centre = |s: f32| {
            self.frame.world(Vec3::new(
                s,
                (side[0] + side[1]) / 2.0,
                (up[0] + up[1]) / 2.0,
            ))
        };
        let mut s = self.along[0] + step / 2.0;
        while s < self.along[1] {
            let p = centre(s);
            if bounds.contains_point(p) && point_in_mesh(p, triangles) {
                out.push([s - step / 2.0, (s + step / 2.0).min(self.along[1])]);
            }
            s += step;
        }
    }
}

/// Keeps the part of a convex polygon on one side of `p[axis] = value`.
fn clip(poly: &[Vec3], axis: usize, value: f32, keep_above: bool, out: &mut Vec<Vec3>) {
    out.clear();
    let inside = |p: &Vec3| {
        if keep_above {
            p[axis] >= value
        } else {
            p[axis] <= value
        }
    };
    for (i, p) in poly.iter().enumerate() {
        let q = &poly[(i + 1) % poly.len()];
        let (pin, qin) = (inside(p), inside(q));
        if pin {
            out.push(*p);
        }
        if pin != qin {
            let t = (value - p[axis]) / (q[axis] - p[axis]);
            out.push(*p + (*q - *p) * t);
        }
    }
}

/// Sorted, merged stretches. Stretches closer than `gap` join.
pub fn merge(mut stretches: Vec<[f32; 2]>, gap: f32) -> Vec<[f32; 2]> {
    stretches.retain(|s| s[1] >= s[0]);
    stretches.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(stretches.len());
    for s in stretches {
        match out.last_mut() {
            Some(last) if s[0] <= last[1] + gap => last[1] = last[1].max(s[1]),
            _ => out.push(s),
        }
    }
    out
}

/// Where both sets of stretches are blocked.
pub fn intersect(a: &[[f32; 2]], b: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        let lo = a[i][0].max(b[j][0]);
        let hi = a[i][1].min(b[j][1]);
        if lo < hi {
            out.push([lo, hi]);
        }
        if a[i][1] < b[j][1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// The parts of the sorted stretches `a` not covered by the sorted `b`.
pub fn subtract(a: &[[f32; 2]], b: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for s in a {
        let mut from = s[0];
        for c in b.iter().filter(|c| c[1] > s[0] && c[0] < s[1]) {
            if c[0] > from {
                out.push([from, c[0]]);
            }
            from = from.max(c[1]);
        }
        if from < s[1] {
            out.push([from, s[1]]);
        }
    }
    out
}

/// How much of `stretch` the stretches in `list` cover.
pub fn covered(stretch: [f32; 2], list: &[[f32; 2]]) -> f32 {
    list.iter()
        .map(|s| (s[1].min(stretch[1]) - s[0].max(stretch[0])).max(0.0))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::super::geom::tests::box_triangles;
    use super::*;

    /// A 6 m tray along x: 0.3 m wide, 0.1 m high, top at z = 0.1.
    fn bounds(t: &[Triangle]) -> Aabb {
        Aabb::from_points(t.iter().flatten().copied())
    }

    fn tray() -> Vec<Triangle> {
        box_triangles(Vec3::new(0.0, -0.15, 0.0), Vec3::new(6.0, 0.15, 0.1))
    }

    #[test]
    fn frame_follows_the_run() {
        let f = RunFrame::of(&tray()).unwrap();
        assert!(f.axis.abs_diff_eq(Vec3::X, 1e-3), "{f:?}");
        assert!(f.normal.abs_diff_eq(Vec3::Z, 1e-3));
        assert!((f.length() - 6.0).abs() < 1e-3);
        assert!((f.width() - 0.3).abs() < 1e-3);
        assert!((f.height() - 0.1).abs() < 1e-3);
        assert!(!f.is_vertical());

        let ladder = box_triangles(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.4, 0.08, 3.0));
        let f = RunFrame::of(&ladder).unwrap();
        assert!(f.is_vertical());
        // The broad face (0.4 m) lies across, the face normal is y.
        assert!((f.width() - 0.4).abs() < 0.01);
        assert!(f.normal.y.abs() > 0.99);
    }

    #[test]
    fn a_duct_above_blocks_part_of_the_headroom() {
        let f = RunFrame::of(&tray()).unwrap();
        let zone = ZoneBox::around(&f, ZoneSide::Above, 0.2)[0];
        // A 1.2 m wide duct 0.1 m above the tray, from x = 2 to x = 3.2.
        let duct = box_triangles(Vec3::new(2.0, -1.0, 0.2), Vec3::new(3.2, 1.0, 0.6));
        let mut hits = Vec::new();
        zone.blocked_by(&duct, &bounds(&duct), &mut hits);
        let blocked = merge(hits, 0.01);
        assert_eq!(blocked.len(), 1);
        let length = blocked[0][1] - blocked[0][0];
        assert!((length - 1.2).abs() < 0.01, "{length}");

        // A duct 0.3 m above leaves the 0.2 m zone free.
        let high = box_triangles(Vec3::new(2.0, -1.0, 0.4), Vec3::new(3.2, 1.0, 0.8));
        let mut hits = Vec::new();
        zone.blocked_by(&high, &bounds(&high), &mut hits);
        assert!(hits.is_empty());
    }

    #[test]
    fn an_enclosing_blocker_counts() {
        let f = RunFrame::of(&tray()).unwrap();
        let zone = ZoneBox::around(&f, ZoneSide::Above, 0.2)[0];
        // A huge block whose faces lie outside the zone's cross-section.
        let block = box_triangles(Vec3::new(1.0, -5.0, -5.0), Vec3::new(2.0, 5.0, 5.0));
        let mut hits = Vec::new();
        zone.blocked_by(&block, &bounds(&block), &mut hits);
        let blocked = merge(hits, 0.01);
        let length: f32 = blocked.iter().map(|s| s[1] - s[0]).sum();
        assert!((length - 1.0).abs() < 0.11, "{length}");
    }

    #[test]
    fn zone_boxes_lie_where_they_should() {
        let f = RunFrame::of(&tray()).unwrap();
        let above = ZoneBox::around(&f, ZoneSide::Above, 0.2)[0].bounds();
        assert!((above.min.z - 0.1).abs() < 1e-3 && (above.max.z - 0.3).abs() < 1e-3);
        assert!((above.min.x - 0.05).abs() < 1e-3 && (above.max.x - 5.95).abs() < 1e-3);
        let sides = ZoneBox::around(&f, ZoneSide::OneSide, 0.5);
        let (left, right) = (sides[0].bounds(), sides[1].bounds());
        // One zone on each side of the 0.3 m wide tray.
        let (lo, hi) = if left.max.y < right.min.y {
            (left, right)
        } else {
            (right, left)
        };
        assert!((lo.min.y + 0.65).abs() < 1e-3 && (lo.max.y + 0.15).abs() < 1e-3);
        assert!((hi.min.y - 0.15).abs() < 1e-3 && (hi.max.y - 0.65).abs() < 1e-3);
        // A stretch keeps the cross-section and narrows the length.
        let part = sides[0].stretch(1.0, 2.0).bounds();
        assert!((part.size().x - 1.0).abs() < 1e-3);
    }

    #[test]
    fn a_wall_the_tray_goes_through_is_a_crossing() {
        let f = RunFrame::of(&tray()).unwrap();
        // A 0.3 m wall across the tray at x = 3.
        let wall = box_triangles(Vec3::new(3.0, -5.0, -2.0), Vec3::new(3.3, 5.0, 3.0));
        let side = ZoneBox::around(&f, ZoneSide::OneSide, 0.1)[0];
        let mut hits = Vec::new();
        side.blocked_by(&wall, &bounds(&wall), &mut hits);
        assert!(!hits.is_empty(), "the wall reaches into the side zone");
        let mut crossing = Vec::new();
        side.core().blocked_by(&wall, &bounds(&wall), &mut crossing);
        let crossing: Vec<[f32; 2]> = merge(crossing, 0.01)
            .into_iter()
            .map(|s| [s[0] - 0.02, s[1] + 0.02])
            .collect();
        assert!(subtract(&merge(hits, 0.01), &crossing).is_empty());

        // A wall along the tray, 50 mm beside it, does not cross it.
        let along = box_triangles(Vec3::new(0.0, 0.2, -2.0), Vec3::new(6.0, 0.5, 3.0));
        let mut crossing = Vec::new();
        side.core()
            .blocked_by(&along, &bounds(&along), &mut crossing);
        assert!(crossing.is_empty());
    }

    #[test]
    fn stretches_merge_and_intersect() {
        let m = merge(vec![[3.0, 4.0], [0.0, 1.0], [0.995, 2.0]], 0.01);
        assert_eq!(m, vec![[0.0, 2.0], [3.0, 4.0]]);
        let i = intersect(&m, &[[1.5, 3.5]]);
        assert_eq!(i, vec![[1.5, 2.0], [3.0, 3.5]]);
        assert!((covered([0.0, 4.0], &i) - 1.0).abs() < 1e-6);
        assert_eq!(
            subtract(&[[0.0, 4.0]], &[[1.0, 2.0], [3.0, 5.0]]),
            vec![[0.0, 1.0], [2.0, 3.0]]
        );
    }
}
