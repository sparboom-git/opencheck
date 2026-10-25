//! Visual checking: rules that colour elements in the 3D view instead of
//! reporting findings, so a pattern (or a break in it) shows at a glance.
//!
//! The first kind is the alignment check of structural walls and columns:
//! elements that stand in line with each other, typically stacked from
//! storey to storey, form a group and share a colour. A wall or column that
//! is offset from the one below gets another colour than its neighbours.

use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

use crate::Discipline;

/// One visual rule as kept in the project.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VisualRule {
    pub name: String,
    #[serde(flatten)]
    pub kind: VisualKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VisualKind {
    Alignment(AlignmentRule),
}

impl VisualRule {
    /// Alignment of walls and columns in the structural models.
    pub fn alignment() -> Self {
        Self {
            name: "Alignment of walls and columns".to_owned(),
            kind: VisualKind::Alignment(AlignmentRule::default()),
        }
    }
}

/// Settings of the alignment check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlignmentRule {
    pub mode: AlignMode,
    /// Largest offset that still counts as aligned, in millimetres.
    pub tolerance_mm: f64,
    pub walls: bool,
    pub columns: bool,
    /// Models of these disciplines are looked at.
    pub disciplines: Vec<Discipline>,
}

impl Default for AlignmentRule {
    fn default() -> Self {
        Self {
            mode: AlignMode::Centerline,
            tolerance_mm: 0.0,
            walls: true,
            columns: true,
            disciplines: vec![Discipline::Structural],
        }
    }
}

impl AlignmentRule {
    /// Whether elements of this IFC class take part.
    pub fn includes_class(&self, class: &str) -> bool {
        member_kind(class).is_some_and(|k| match k {
            Member::Wall => self.walls,
            Member::Column => self.columns,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlignMode {
    /// Walls by their centre lines, columns by their centre points.
    #[default]
    Centerline,
    /// Walls by their faces: two walls are aligned when a face of one is in
    /// line with a face of the other, also when they differ in thickness.
    /// Columns are still compared by their centre points.
    Face,
}

/// What an element is for the alignment check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Member {
    Wall,
    Column,
}

/// Walls and columns by IFC class (`IfcWallStandardCase`, `IfcColumn`, ...).
pub fn member_kind(class: &str) -> Option<Member> {
    if class.ends_with("Type") {
        None
    } else if class.starts_with("IfcWall") {
        Some(Member::Wall)
    } else if class.starts_with("IfcColumn") {
        Some(Member::Column)
    } else {
        None
    }
}

/// An element to align: its kind and its vertices in one shared frame, in
/// metres with z up.
pub struct AlignElement {
    pub kind: Member,
    pub points: Vec<DVec3>,
}

/// The outcome: a group per element (None for an element without usable
/// geometry), groups numbered from the largest down, and per element
/// whether another wall or column stands right above or below it, aligned
/// or not. An element alone in its group with one above or below is out of
/// line; alone with none, there is nothing to line up with.
#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    pub group: Vec<Option<usize>>,
    pub groups: usize,
    pub stacked: Vec<bool>,
}

/// Parallel walls are those within this angle of each other (cos 10°).
const PARALLEL_COS: f64 = 0.985;
/// Extra room for rounding in the model geometry, metres.
const SLACK: f64 = 0.0005;
/// Walls must overlap along their length by more than this, metres.
const MIN_OVERLAP: f64 = 0.001;
/// Elements count as above one another when they share at most this much
/// height, metres.
const LEVEL_OVERLAP: f64 = 0.05;
/// How far apart in plan elements above one another are looked for, metres.
const NEAR: f64 = 0.5;

/// Groups elements that are in line with each other. Walls are compared
/// with walls, columns with columns; offsets up to `tolerance` metres count
/// as aligned.
pub fn align(elements: &[AlignElement], mode: AlignMode, tolerance: f64) -> Alignment {
    let tolerance = tolerance.max(0.0) + SLACK;
    let plans: Vec<Option<Plan>> = elements.iter().map(|e| Plan::of(&e.points)).collect();
    let mut sets = Sets::new(elements.len());
    let mut stacked = vec![false; elements.len()];
    let reach = tolerance.max(NEAR);

    // Sweep along x over the plan boxes, so only neighbours are compared.
    let mut order: Vec<usize> = (0..elements.len())
        .filter(|&i| plans[i].is_some())
        .collect();
    let lo = |i: usize| plans[i].as_ref().map_or(0.0, |p| p.min.x);
    order.sort_by(|a, b| lo(*a).total_cmp(&lo(*b)));
    for (n, &i) in order.iter().enumerate() {
        let a = plans[i].as_ref().expect("filtered above");
        for &j in &order[n + 1..] {
            let b = plans[j].as_ref().expect("filtered above");
            if b.min.x > a.max.x + reach {
                break;
            }
            if elements[i].kind != elements[j].kind
                || b.min.y > a.max.y + reach
                || a.min.y > b.max.y + reach
            {
                continue;
            }
            let (aligned, near) = match elements[i].kind {
                Member::Column => {
                    let d = a.centre().distance(b.centre());
                    (d <= tolerance, d <= a.size().max(b.size()))
                }
                Member::Wall => WallPair::of(a, b).map_or((false, false), |pair| {
                    (pair.aligned(a, b, mode, tolerance), pair.near(a, b))
                }),
            };
            if aligned {
                sets.union(i, j);
            }
            let apart = a.z[1] <= b.z[0] + LEVEL_OVERLAP || b.z[1] <= a.z[0] + LEVEL_OVERLAP;
            if near && apart {
                stacked[i] = true;
                stacked[j] = true;
            }
        }
    }

    // Number the groups from the largest; ties by position, so the colours
    // stay put between runs.
    let mut members: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut root_index = std::collections::HashMap::new();
    for i in order {
        let root = sets.find(i);
        let k = *root_index.entry(root).or_insert_with(|| {
            members.push((root, Vec::new()));
            members.len() - 1
        });
        members[k].1.push(i);
    }
    let key = |list: &[usize]| {
        let p = plans[list[0]].as_ref().expect("filtered above").centre();
        (
            std::cmp::Reverse(list.len()),
            (p.x * 1000.0).round() as i64,
            (p.y * 1000.0).round() as i64,
        )
    };
    members.sort_by_key(|(_, list)| key(list));
    let mut group = vec![None; elements.len()];
    for (g, (_, list)) in members.iter().enumerate() {
        for &i in list {
            group[i] = Some(g);
        }
    }
    Alignment {
        group,
        groups: members.len(),
        stacked,
    }
}

/// Two parallel walls that overlap along their length, seen from the
/// first: the overlap runs from `from` to `to` along its axis.
struct WallPair {
    from: f64,
    to: f64,
    /// The first wall's axis and normal, dotted with the second's normal.
    ab: f64,
    nb: f64,
}

impl WallPair {
    fn of(a: &Plan, b: &Plan) -> Option<Self> {
        if a.axis.dot(b.axis).abs() < PARALLEL_COS {
            return None;
        }
        let ends = b.corners().map(|c| c.dot(a.axis));
        let b_lo = ends.iter().copied().fold(f64::MAX, f64::min);
        let b_hi = ends.iter().copied().fold(f64::MIN, f64::max);
        let from = a.along[0].max(b_lo);
        let to = a.along[1].min(b_hi);
        (to - from > MIN_OVERLAP).then(|| Self {
            from,
            to,
            ab: a.axis.dot(b.normal),
            nb: a.normal.dot(b.normal),
        })
    }

    /// A line of the second wall (p · normal = s) in the first wall's
    /// frame: its offset along the first wall's normal at `u` along it.
    fn offset(&self, s: f64, u: f64) -> f64 {
        (s - u * self.ab) / self.nb
    }

    fn in_line(&self, sa: f64, sb: f64, tolerance: f64) -> bool {
        (self.offset(sb, self.from) - sa).abs() <= tolerance
            && (self.offset(sb, self.to) - sa).abs() <= tolerance
    }

    /// Aligned: the centre lines (or a pair of faces) lie within the
    /// tolerance of each other at both ends of the overlap.
    fn aligned(&self, a: &Plan, b: &Plan, mode: AlignMode, tolerance: f64) -> bool {
        match mode {
            AlignMode::Centerline => self.in_line(a.mid_side(), b.mid_side(), tolerance),
            AlignMode::Face => a
                .side
                .iter()
                .any(|&sa| b.side.iter().any(|&sb| self.in_line(sa, sb, tolerance))),
        }
    }

    /// Close enough to be meant to stand in line: centre lines less than
    /// the thicker wall's thickness apart.
    fn near(&self, a: &Plan, b: &Plan) -> bool {
        let mid = (self.from + self.to) / 2.0;
        (self.offset(b.mid_side(), mid) - a.mid_side()).abs() <= a.width().max(b.width())
    }
}

/// An element's footprint as its smallest enclosing rectangle in plan:
/// `axis` along the length, `normal` across. Ranges are dot products of
/// the points with those directions.
#[derive(Debug, Clone, Copy)]
struct Plan {
    axis: DVec2,
    normal: DVec2,
    along: [f64; 2],
    side: [f64; 2],
    min: DVec2,
    max: DVec2,
    /// Height range.
    z: [f64; 2],
}

impl Plan {
    fn of(points: &[DVec3]) -> Option<Self> {
        let hull = convex_hull(points.iter().map(|p| p.truncate()).collect());
        if hull.len() < 3 {
            return None;
        }
        let range = |dir: DVec2| {
            hull.iter().fold([f64::MAX, f64::MIN], |r, p| {
                let t = p.dot(dir);
                [r[0].min(t), r[1].max(t)]
            })
        };
        // The thinnest direction is across one of the hull's edges.
        let mut best: Option<(f64, DVec2)> = None;
        for (k, p) in hull.iter().enumerate() {
            let edge = hull[(k + 1) % hull.len()] - *p;
            if edge.length_squared() < 1e-12 {
                continue;
            }
            let axis = edge.normalize();
            let [lo, hi] = range(axis.perp());
            if best.is_none_or(|(w, _)| hi - lo < w - 1e-9) {
                best = Some((hi - lo, axis));
            }
        }
        let (_, mut axis) = best?;
        // One direction per line: pointing to +x, or to +y when along y.
        if axis.x < -1e-9 || (axis.x.abs() <= 1e-9 && axis.y < 0.0) {
            axis = -axis;
        }
        let normal = axis.perp();
        let min = hull.iter().copied().fold(DVec2::MAX, DVec2::min);
        let max = hull.iter().copied().fold(DVec2::MIN, DVec2::max);
        let z = points
            .iter()
            .fold([f64::MAX, f64::MIN], |r, p| [r[0].min(p.z), r[1].max(p.z)]);
        Some(Self {
            axis,
            normal,
            along: range(axis),
            side: range(normal),
            min,
            max,
            z,
        })
    }

    fn width(&self) -> f64 {
        self.side[1] - self.side[0]
    }

    /// The larger side of the footprint.
    fn size(&self) -> f64 {
        (self.along[1] - self.along[0]).max(self.width())
    }

    fn mid_side(&self) -> f64 {
        (self.side[0] + self.side[1]) / 2.0
    }

    fn centre(&self) -> DVec2 {
        self.axis * (self.along[0] + self.along[1]) / 2.0 + self.normal * self.mid_side()
    }

    fn corners(&self) -> [DVec2; 4] {
        let at = |u: f64, v: f64| self.axis * u + self.normal * v;
        [
            at(self.along[0], self.side[0]),
            at(self.along[1], self.side[0]),
            at(self.along[1], self.side[1]),
            at(self.along[0], self.side[1]),
        ]
    }
}

/// Andrew's monotone chain; counter-clockwise, without repeated points.
fn convex_hull(mut points: Vec<DVec2>) -> Vec<DVec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| a.distance_squared(*b) < 1e-14);
    if points.len() < 3 {
        return points;
    }
    let cross = |o: DVec2, a: DVec2, b: DVec2| (a - o).perp_dot(b - o);
    let mut hull: Vec<DVec2> = Vec::with_capacity(points.len() * 2);
    for pass in 0..2 {
        let start = hull.len();
        let iter: Box<dyn Iterator<Item = &DVec2>> = if pass == 0 {
            Box::new(points.iter())
        } else {
            Box::new(points.iter().rev())
        };
        for &p in iter {
            while hull.len() >= start + 2
                && cross(hull[hull.len() - 2], hull[hull.len() - 1], p) <= 1e-12
            {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop();
    }
    hull
}

/// Union-find over element indexes.
struct Sets(Vec<usize>);

impl Sets {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    fn find(&mut self, mut i: usize) -> usize {
        while self.0[i] != i {
            self.0[i] = self.0[self.0[i]];
            i = self.0[i];
        }
        i
    }

    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box from `min` to `max`, as its eight corners.
    fn block(kind: Member, min: [f64; 3], max: [f64; 3]) -> AlignElement {
        let mut points = Vec::new();
        for x in [min[0], max[0]] {
            for y in [min[1], max[1]] {
                for z in [min[2], max[2]] {
                    points.push(DVec3::new(x, y, z));
                }
            }
        }
        AlignElement { kind, points }
    }

    /// A wall along x, `thickness` thick with its near face at `y`, on
    /// storey `level` (3 m storeys).
    fn wall(y: f64, thickness: f64, level: f64) -> AlignElement {
        block(
            Member::Wall,
            [0.0, y, level * 3.0],
            [6.0, y + thickness, level * 3.0 + 3.0],
        )
    }

    #[test]
    fn stacked_walls_in_line_share_a_group() {
        let walls = [
            wall(0.0, 0.2, 0.0),
            wall(0.0, 0.2, 1.0),
            wall(0.003, 0.2, 2.0),
        ];
        let exact = align(&walls, AlignMode::Centerline, 0.0);
        assert_eq!(exact.groups, 2);
        assert_eq!(exact.stacked, vec![true; 3]);
        assert_eq!(exact.group[0], exact.group[1]);
        assert_ne!(exact.group[1], exact.group[2]);
        // Within a 3 mm tolerance the third wall is in line too.
        let loose = align(&walls, AlignMode::Centerline, 0.003);
        assert_eq!(loose.groups, 1);
    }

    #[test]
    fn faces_line_up_across_thicknesses() {
        // Flush on the near face, 200 mm below and 150 mm above.
        let walls = [wall(0.0, 0.2, 0.0), wall(0.0, 0.15, 1.0)];
        assert_eq!(align(&walls, AlignMode::Centerline, 0.0).groups, 2);
        assert_eq!(align(&walls, AlignMode::Face, 0.0).groups, 1);
    }

    #[test]
    fn turned_walls_are_not_in_line() {
        let a = wall(0.0, 0.2, 0.0);
        // The same wall turned by about 0.2 degrees: 20 mm off at its end.
        let mut b = wall(0.0, 0.2, 1.0);
        for p in &mut b.points {
            p.y += p.x / 300.0;
        }
        assert_eq!(align(&[a, b], AlignMode::Centerline, 0.005).groups, 2);
    }

    #[test]
    fn columns_by_their_centres() {
        let column = |x: f64, level: f64| {
            block(
                Member::Column,
                [x, 0.0, level * 3.0],
                [x + 0.3, 0.3, level * 3.0 + 3.0],
            )
        };
        let result = align(
            &[column(0.0, 0.0), column(0.0, 1.0), column(0.05, 2.0)],
            AlignMode::Centerline,
            0.0,
        );
        assert_eq!(result.group[0], result.group[1]);
        assert_ne!(result.group[0], result.group[2]);
        // The larger group comes first.
        assert_eq!(result.group[0], Some(0));
        // A column on its own storey has nothing to line up with.
        let lone = align(
            &[column(0.0, 0.0), column(5.0, 1.0)],
            AlignMode::Centerline,
            0.0,
        );
        assert_eq!(lone.stacked, vec![false, false]);
    }

    #[test]
    fn rules_round_trip() {
        let rule = VisualRule::alignment();
        let json = serde_json::to_string(&rule).unwrap();
        assert!(json.contains("\"type\":\"alignment\""));
        assert_eq!(serde_json::from_str::<VisualRule>(&json).unwrap(), rule);
        assert!(AlignmentRule::default().includes_class("IfcWallStandardCase"));
        assert!(!AlignmentRule::default().includes_class("IfcWallType"));
        assert!(!AlignmentRule::default().includes_class("IfcSlab"));
    }
}
