//! Rooms: every place enclosed by walls should be a room. Per storey the
//! walls (with doors, windows and columns, so door openings do not join the
//! rooms) are cut at a height above the floor and laid on a grid with the
//! rooms' footprints. What can be reached from outside the building is
//! outside; what is enclosed, wide enough to stand in and not covered by a
//! room is an area without a room. Rooms are green; the walls around an area
//! without a room are red.

use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};

use super::{Category, ERROR, ElementKey, NEUTRAL, Plan, Problem, VisualModel, VisualOutcome};
use crate::check::Severity;
use crate::{Discipline, IfcModel, SpatialNode};

/// Settings of the room check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoomRule {
    /// Models whose rooms (`IfcSpace`) and storeys are used.
    pub room_disciplines: Vec<Discipline>,
    /// Models whose walls, doors, windows and columns enclose the rooms.
    pub wall_disciplines: Vec<Discipline>,
    /// Height above each storey's floor where the walls are cut, millimetres.
    pub cut_height_mm: f64,
    /// Enclosed areas smaller than this are left alone (shafts, wall
    /// cavities), square metres.
    pub min_area_m2: f64,
    /// Openings narrower than this are closed when telling inside from
    /// outside (doors and windows without geometry), millimetres.
    pub gap_mm: f64,
    /// Size of the grid's cells, millimetres.
    pub cell_mm: f64,
}

impl Default for RoomRule {
    fn default() -> Self {
        Self {
            room_disciplines: vec![Discipline::Architectural],
            wall_disciplines: vec![Discipline::Architectural, Discipline::Structural],
            cut_height_mm: 1200.0,
            min_area_m2: 1.0,
            gap_mm: 1200.0,
            cell_mm: 100.0,
        }
    }
}

/// Rooms are this green.
pub const ROOM: [u8; 3] = [46, 158, 74];

/// An area narrower than this is a wall cavity, not a place for a room.
const MIN_WIDTH_M: f64 = 0.8;

/// The grid stays below this many cells; cells grow on large storeys.
const MAX_CELLS: f64 = 6_000_000.0;

/// Elements that enclose rooms.
fn encloses(class: &str) -> bool {
    class.starts_with("IfcWall")
        || class.starts_with("IfcCurtainWall")
        || class.starts_with("IfcColumn")
        || class.starts_with("IfcDoor")
        || class.starts_with("IfcWindow")
        || class == "IfcPlate"
        || class == "IfcMember"
}

/// A storey of the room models: its name and floor height in the shared
/// frame.
struct Level {
    name: String,
    z: f64,
    /// The height as the model states it, for the table.
    elevation: f64,
}

/// Name, floor height in the shared frame and as the model states it.
fn storeys_of(m: &VisualModel) -> Vec<(String, f64, f64)> {
    let model: &IfcModel = m.model;
    let scale = model.length_unit_scale;
    let mut out = Vec::new();
    if let Some(root) = &model.spatial_root {
        root.walk(&mut |node: &SpatialNode, _| {
            if node.is_storey()
                && let Some(z) = node.placement_z.or(node.elevation)
            {
                out.push((
                    node.label().to_owned(),
                    m.world_height(z * scale),
                    z * scale,
                ));
            }
        });
    }
    out
}

/// The room models' storeys from low to high, the same height in two models
/// taken once.
fn levels(models: &[&VisualModel]) -> Vec<Level> {
    let mut all: Vec<(String, f64, f64)> = models.iter().flat_map(|m| storeys_of(m)).collect();
    all.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut out: Vec<Level> = Vec::new();
    for (name, z, elevation) in all {
        match out.last_mut() {
            Some(last) if (z - last.z).abs() < 0.2 => {
                if !last.name.split(" / ").any(|n| n == name) {
                    last.name = format!("{} / {name}", last.name);
                }
            }
            _ => out.push(Level { name, z, elevation }),
        }
    }
    out
}

/// Cells of a storey's plan.
struct Grid {
    min: [f64; 2],
    cell: f64,
    w: usize,
    h: usize,
    /// Per cell: the enclosing element there (index), or `NONE`.
    wall: Vec<u32>,
    room: Vec<bool>,
}

const NONE: u32 = u32::MAX;

impl Grid {
    fn new(min: [f64; 2], max: [f64; 2], cell: f64) -> Self {
        // Room outside for the closed gaps' band.
        let margin = 3.0 * cell + 1.0;
        let mut cell = cell;
        let (sx, sy) = (
            max[0] - min[0] + 2.0 * margin,
            max[1] - min[1] + 2.0 * margin,
        );
        while (sx / cell) * (sy / cell) > MAX_CELLS {
            cell *= 1.5;
        }
        let w = (sx / cell).ceil() as usize + 1;
        let h = (sy / cell).ceil() as usize + 1;
        Self {
            min: [min[0] - margin, min[1] - margin],
            cell,
            w,
            h,
            wall: vec![NONE; w * h],
            room: vec![false; w * h],
        }
    }

    fn index(&self, x: f64, y: f64) -> Option<usize> {
        let i = ((x - self.min[0]) / self.cell).floor();
        let j = ((y - self.min[1]) / self.cell).floor();
        (i >= 0.0 && j >= 0.0 && (i as usize) < self.w && (j as usize) < self.h)
            .then(|| j as usize * self.w + i as usize)
    }

    fn centre(&self, index: usize) -> [f64; 2] {
        let (i, j) = (index % self.w, index / self.w);
        [
            self.min[0] + (i as f64 + 0.5) * self.cell,
            self.min[1] + (j as f64 + 0.5) * self.cell,
        ]
    }

    /// Marks the cells along a line as the element's.
    fn line(&mut self, a: [f64; 2], b: [f64; 2], element: u32) {
        let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        let steps = (length / (self.cell * 0.5)).ceil().max(1.0) as usize;
        for s in 0..=steps {
            let t = s as f64 / steps as f64;
            if let Some(i) = self.index(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t) {
                self.wall[i] = element;
            }
        }
    }

    /// Marks the cells whose centre is in the triangle as room.
    fn fill(&mut self, t: [[f64; 2]; 3]) {
        let lo = [
            t.iter().map(|p| p[0]).fold(f64::MAX, f64::min),
            t.iter().map(|p| p[1]).fold(f64::MAX, f64::min),
        ];
        let hi = [
            t.iter().map(|p| p[0]).fold(f64::MIN, f64::max),
            t.iter().map(|p| p[1]).fold(f64::MIN, f64::max),
        ];
        let (Some(a), Some(b)) = (self.index(lo[0], lo[1]), self.index(hi[0], hi[1])) else {
            return;
        };
        let cross = |o: [f64; 2], p: [f64; 2], q: [f64; 2]| {
            (p[0] - o[0]) * (q[1] - o[1]) - (p[1] - o[1]) * (q[0] - o[0])
        };
        let area = cross(t[0], t[1], t[2]);
        if area.abs() < 1e-9 {
            return;
        }
        for j in a / self.w..=b / self.w {
            for i in a % self.w..=b % self.w {
                let index = j * self.w + i;
                let c = self.centre(index);
                let (d0, d1, d2) = (
                    cross(t[0], t[1], c) * area.signum(),
                    cross(t[1], t[2], c) * area.signum(),
                    cross(t[2], t[0], c) * area.signum(),
                );
                if d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0 {
                    self.room[index] = true;
                }
            }
        }
    }

    fn neighbours(&self, index: usize) -> impl Iterator<Item = usize> + '_ {
        let (i, j) = (index % self.w, index / self.w);
        [
            (i > 0).then(|| index - 1),
            (i + 1 < self.w).then(|| index + 1),
            (j > 0).then(|| index - self.w),
            (j + 1 < self.h).then(|| index + self.w),
        ]
        .into_iter()
        .flatten()
    }
}

/// The segment where a triangle crosses the height `z`, in plan.
fn cut(t: &[glam::Vec3; 3], z: f64) -> Option<([f64; 2], [f64; 2])> {
    let mut points = Vec::with_capacity(2);
    for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
        let (za, zb) = (f64::from(a.z) - z, f64::from(b.z) - z);
        if (za < 0.0) != (zb < 0.0) {
            let s = za / (za - zb);
            points.push([
                f64::from(a.x) + (f64::from(b.x) - f64::from(a.x)) * s,
                f64::from(a.y) + (f64::from(b.y) - f64::from(a.y)) * s,
            ]);
        }
    }
    (points.len() == 2).then(|| (points[0], points[1]))
}

/// What one storey showed.
struct StoreyResult {
    rooms: usize,
    /// Areas without a room: square metres, bounding elements.
    missing: Vec<(f64, Vec<u32>)>,
}

/// Each room's number (its Name) and name (its LongName) in the middle of
/// its footprint.
fn room_labels(
    models: &[&VisualModel],
    rooms: &[(ElementKey, &Vec<[glam::Vec3; 3]>)],
) -> Vec<super::PlaceLabel> {
    let mut out = Vec::new();
    for (key, triangles) in rooms {
        let Some(element) = models
            .iter()
            .find(|m| m.id == key.model)
            .and_then(|m| m.model.element(key.express_id))
        else {
            continue;
        };
        let long_name = element
            .attributes
            .iter()
            .find(|(k, _)| k == "LongName")
            .map(|(_, v)| v.to_string())
            .filter(|v| !v.trim().is_empty() && v != "$")
            .unwrap_or_default();
        // The middle of the footprint: triangles weighed by their area in plan.
        let (mut sum, mut weight) = ([0.0_f64; 2], 0.0_f64);
        let mut lo = [f64::MAX; 3];
        let mut hi = [f64::MIN; 3];
        for t in triangles.iter() {
            let p = t.map(|v| [f64::from(v.x), f64::from(v.y), f64::from(v.z)]);
            let area = ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1])
                - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]))
                .abs()
                / 2.0;
            sum[0] += area * (p[0][0] + p[1][0] + p[2][0]) / 3.0;
            sum[1] += area * (p[0][1] + p[1][1] + p[2][1]) / 3.0;
            weight += area;
            for v in p {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        if weight <= 0.0 {
            continue;
        }
        out.push(super::PlaceLabel {
            at: [sum[0] / weight, sum[1] / weight],
            low: lo[2],
            high: hi[2],
            min: [lo[0], lo[1]],
            max: [hi[0], hi[1]],
            number: element.name.trim().to_owned(),
            name: long_name,
        });
    }
    out
}

#[allow(clippy::needless_range_loop)]
pub(super) fn run(rule: &RoomRule, models: &[VisualModel]) -> VisualOutcome {
    let mut outcome = VisualOutcome::default();
    let room_models: Vec<&VisualModel> = models
        .iter()
        .filter(|m| rule.room_disciplines.contains(&m.model.discipline))
        .collect();
    let wall_models: Vec<&VisualModel> = models
        .iter()
        .filter(|m| rule.wall_disciplines.contains(&m.model.discipline))
        .collect();
    if room_models.is_empty() {
        outcome.summary = "No model with rooms".to_owned();
        outcome
            .notes
            .push("Choose the models with rooms (usually the architect's).".to_owned());
        return outcome;
    }
    let levels = levels(&room_models);
    if levels.is_empty() {
        outcome.summary = "No storeys".to_owned();
        outcome
            .notes
            .push("The models with rooms have no storeys with a height.".to_owned());
        return outcome;
    }

    // The enclosing elements and the rooms, with their triangles and heights.
    struct Part {
        key: ElementKey,
        triangles: Vec<[glam::Vec3; 3]>,
        low: f64,
        high: f64,
    }
    let parts = |list: &[&VisualModel], keep: &dyn Fn(&str) -> bool| -> Vec<Part> {
        let mut out = Vec::new();
        for m in list {
            for e in &m.model.elements {
                if !e.has_geometry() || !m.includes(e.express_id) || !keep(&e.ifc_class) {
                    continue;
                }
                let triangles = m.triangles(e);
                if triangles.is_empty() {
                    continue;
                }
                let zs = triangles
                    .iter()
                    .flat_map(|t| t.iter().map(|p| f64::from(p.z)));
                let (low, high) =
                    zs.fold((f64::MAX, f64::MIN), |(lo, hi), z| (lo.min(z), hi.max(z)));
                out.push(Part {
                    key: m.key(e.express_id),
                    triangles,
                    low,
                    high,
                });
            }
        }
        out
    };
    let walls = parts(&wall_models, &encloses);
    let rooms = parts(&room_models, &|c: &str| c == "IfcSpace");
    outcome.labels = room_labels(
        &room_models,
        &rooms
            .iter()
            .map(|r| (r.key, &r.triangles))
            .collect::<Vec<_>>(),
    );
    if rooms.is_empty() {
        outcome
            .notes
            .push("The models have no rooms (IfcSpace) with geometry.".to_owned());
    }

    let mut results: Vec<StoreyResult> = Vec::new();
    let mut walls_around: BTreeSet<u32> = BTreeSet::new();
    let mut used_walls: BTreeSet<u32> = BTreeSet::new();
    for level in &levels {
        let z = level.z + rule.cut_height_mm / 1000.0;
        // The walls' cut at this height.
        let mut segments: Vec<([f64; 2], [f64; 2], u32)> = Vec::new();
        for (index, w) in walls.iter().enumerate() {
            if w.low > z || w.high < z {
                continue;
            }
            for t in &w.triangles {
                if let Some((a, b)) = cut(t, z) {
                    segments.push((a, b, index as u32));
                }
            }
        }
        let here: Vec<&Part> = rooms.iter().filter(|r| r.low <= z && r.high >= z).collect();
        if segments.is_empty() && here.is_empty() {
            results.push(StoreyResult {
                rooms: 0,
                missing: Vec::new(),
            });
            continue;
        }
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        let mut grow = |p: [f64; 2]| {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        };
        for (a, b, _) in &segments {
            grow(*a);
            grow(*b);
        }
        for r in &here {
            for t in &r.triangles {
                for p in t {
                    grow([f64::from(p.x), f64::from(p.y)]);
                }
            }
        }
        outcome.plans.push(Plan {
            name: level.name.clone(),
            elevation: level.elevation,
            floor: level.z,
            cut: z,
            min: lo,
            max: hi,
        });
        let mut grid = Grid::new(lo, hi, (rule.cell_mm / 1000.0).max(0.02));
        for (a, b, index) in &segments {
            grid.line(*a, *b, *index);
            used_walls.insert(*index);
        }
        for r in &here {
            for t in &r.triangles {
                grid.fill(t.map(|p| [f64::from(p.x), f64::from(p.y)]));
            }
        }

        // How far each cell is from a wall, in cells.
        let cells = grid.w * grid.h;
        let mut queue = VecDeque::new();
        let mut distance = vec![u32::MAX; cells];
        for i in 0..cells {
            if grid.wall[i] != NONE {
                distance[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            for n in grid.neighbours(i).collect::<Vec<_>>() {
                if distance[n] == u32::MAX {
                    distance[n] = distance[i] + 1;
                    queue.push_back(n);
                }
            }
        }
        // Outside: what the grid's edge reaches between the walls, openings
        // narrower than the gap closed (a door without geometry, a window).
        let close = ((rule.gap_mm / 2000.0) / grid.cell).ceil() as u32;
        let mut outside = vec![false; cells];
        for i in 0..cells {
            let (x, y) = (i % grid.w, i / grid.w);
            if (x == 0 || y == 0 || x + 1 == grid.w || y + 1 == grid.h) && distance[i] > close {
                outside[i] = true;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            for n in grid.neighbours(i).collect::<Vec<_>>() {
                if !outside[n] && distance[n] > close {
                    outside[n] = true;
                    queue.push_back(n);
                }
            }
        }
        // Back up to the walls, as far as the gap was closed.
        let mut steps = vec![u32::MAX; cells];
        for i in 0..cells {
            if outside[i] {
                steps[i] = 0;
                queue.push_back(i);
            }
        }
        while let Some(i) = queue.pop_front() {
            if steps[i] > close {
                continue;
            }
            for n in grid.neighbours(i).collect::<Vec<_>>() {
                if !outside[n] && grid.wall[n] == NONE {
                    outside[n] = true;
                    steps[n] = steps[i] + 1;
                    queue.push_back(n);
                }
            }
        }
        // Enclosed areas without a room.
        let wide = ((MIN_WIDTH_M / 2.0) / grid.cell).ceil() as u32;
        let cell_area = grid.cell * grid.cell;
        let mut seen = vec![false; cells];
        let mut missing = Vec::new();
        for start in 0..cells {
            if seen[start] || outside[start] || grid.wall[start] != NONE || grid.room[start] {
                continue;
            }
            let mut area = 0.0;
            let mut widest = 0;
            let mut around = BTreeSet::new();
            seen[start] = true;
            queue.push_back(start);
            while let Some(i) = queue.pop_front() {
                area += cell_area;
                widest = widest.max(distance[i]);
                for n in grid.neighbours(i).collect::<Vec<_>>() {
                    if grid.wall[n] != NONE {
                        around.insert(grid.wall[n]);
                    } else if !seen[n] && !outside[n] && !grid.room[n] {
                        seen[n] = true;
                        queue.push_back(n);
                    }
                }
            }
            if area >= rule.min_area_m2 && widest >= wide {
                walls_around.extend(around.iter().copied());
                missing.push((area, around.into_iter().collect()));
            }
        }
        missing.sort_by(|a: &(f64, Vec<u32>), b| b.0.total_cmp(&a.0));
        results.push(StoreyResult {
            rooms: here.len(),
            missing,
        });
    }

    // Colours: rooms green, the walls around areas without a room red, the
    // other walls cut neutral.
    let room_keys: Vec<ElementKey> = rooms.iter().map(|r| r.key).collect();
    for (level, result) in levels.iter().zip(&results) {
        for (area, around) in &result.missing {
            let elements: Vec<ElementKey> = around.iter().map(|&i| walls[i as usize].key).collect();
            outcome.problems.push(Problem {
                elements,
                severity: Severity::Moderate,
                text: format!("No room in about {area:.1} m² on {}", level.name),
                value: Some(*area),
                color: ERROR,
            });
        }
    }
    outcome.categories.push(Category {
        label: "Rooms".to_owned(),
        detail: format!("{} rooms", room_keys.len()),
        color: ROOM,
        elements: room_keys,
    });
    let neutral: Vec<ElementKey> = used_walls
        .iter()
        .filter(|i| !walls_around.contains(i))
        .map(|&i| walls[i as usize].key)
        .collect();
    outcome.categories.push(Category {
        label: "Walls, doors, windows, columns".to_owned(),
        detail: format!("{} elements", neutral.len()),
        color: NEUTRAL,
        elements: neutral,
    });
    if !walls_around.is_empty() {
        let red: Vec<ElementKey> = walls_around
            .iter()
            .map(|&i| walls[i as usize].key)
            .collect();
        outcome.categories.push(Category {
            label: "Around an area without a room".to_owned(),
            detail: format!("{} elements", red.len()),
            color: ERROR,
            elements: red,
        });
    }
    outcome.problem_title = "Areas without a room".to_owned();
    outcome.problem_hint = format!(
        "Enclosed by walls at {:.0} mm above the floor, at least {:.1} m² and {:.1} m wide, \
         and no room there. Its walls are red.",
        rule.cut_height_mm, rule.min_area_m2, MIN_WIDTH_M
    );
    let missing: usize = results.iter().map(|r| r.missing.len()).sum();
    outcome.summary = format!(
        "{} rooms on {} storeys, {missing} without a room",
        rooms.len(),
        levels.len()
    );
    outcome.table = std::iter::once(vec![
        "Etage".to_owned(),
        "Kote".to_owned(),
        "Rum".to_owned(),
        "Uden rum".to_owned(),
    ])
    .chain(levels.iter().zip(&results).map(|(level, r)| {
        vec![
            level.name.clone(),
            format!("{:+.3}", level.elevation),
            r.rooms.to_string(),
            if r.missing.is_empty() {
                "–".to_owned()
            } else {
                let area: f64 = r.missing.iter().map(|m| m.0).sum();
                format!("{} ({area:.1} m²)", r.missing.len())
            },
        ]
    }))
    .collect();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    fn square_grid() -> Grid {
        let mut grid = Grid::new([0.0, 0.0], [10.0, 10.0], 0.1);
        // A box of walls with a wall through the middle: two rooms of 5 x 10.
        let walls = [
            ([0.0, 0.0], [10.0, 0.0]),
            ([10.0, 0.0], [10.0, 10.0]),
            ([10.0, 10.0], [0.0, 10.0]),
            ([0.0, 10.0], [0.0, 0.0]),
            ([5.0, 0.0], [5.0, 10.0]),
        ];
        for (i, (a, b)) in walls.iter().enumerate() {
            grid.line(*a, *b, i as u32);
        }
        grid
    }

    #[test]
    fn a_triangle_cut_at_a_height_gives_a_segment() {
        let t = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
        ];
        let (a, b) = cut(&t, 1.0).unwrap();
        let xs = [a[0], b[0]];
        assert!(xs.contains(&0.0) && xs.contains(&1.0));
        assert!(cut(&t, 3.0).is_none());
    }

    #[test]
    fn rooms_fill_their_footprint() {
        let mut grid = square_grid();
        grid.fill([[0.0, 0.0], [5.0, 0.0], [5.0, 10.0]]);
        grid.fill([[0.0, 0.0], [5.0, 10.0], [0.0, 10.0]]);
        let room = grid.room.iter().filter(|r| **r).count() as f64 * 0.01;
        assert!((room - 50.0).abs() < 2.0, "{room}");
        let inside = grid.index(7.0, 5.0).unwrap();
        assert!(!grid.room[inside]);
        assert!(
            grid.wall[grid.index(5.0, 5.0).unwrap()] == 4
                || grid.wall[grid.index(5.02, 5.0).unwrap()] == 4
        );
    }
}
