//! The pipe and duct network the system checks work on: segments, fittings
//! and equipment as nodes, connected where their ports are connected
//! (`IfcRelConnectsPorts`) or, for loose elements without ports, where
//! their ends meet. Each node belongs to a system: its `IfcSystem`, a
//! system property, or else the connected network it is part of.

use glam::{DVec2, DVec3};
use rustc_hash::{FxHashMap, FxHashSet};

use super::alignment::convex_hull;
use super::{ElementKey, VisualModel};
use crate::check::freezone::RunFrame;
use crate::{Aabb, Discipline, Element, IfcModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Segment,
    Fitting,
    /// Air handling units, fans, pumps, boilers: where a system starts.
    Equipment,
    /// Valves, dampers, terminals and the like.
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Medium {
    Pipe,
    Duct,
    Unknown,
}

pub(super) struct Node {
    pub key: ElementKey,
    pub role: Role,
    pub medium: Medium,
    /// System name, e.g. `SV 11`.
    pub system: String,
    /// What kind of system it is, e.g. `Afløb, Spildevand`; the name when
    /// the model does not say.
    pub kind: String,
    /// Axis and section of a segment.
    pub run: Option<Run>,
    /// Bounds in the shared frame.
    pub bounds: Aabb,
}

/// A straight segment: the centres of its two end sections and its section.
#[derive(Debug, Clone, Copy)]
pub(super) struct Run {
    pub ends: [DVec3; 2],
    /// Section sides, metres.
    pub width: f64,
    pub height: f64,
    /// Section area, square metres.
    pub area: f64,
    pub round: bool,
    /// A straight extrusion: all vertices at its two ends. Silencers,
    /// dampers and flexible ducts exported as segments are not.
    pub straight: bool,
}

impl Run {
    /// The run along `axes[0]`, with the section across `axes[1]` and
    /// `axes[2]` (orthonormal).
    fn of(axes: [DVec3; 3], points: &[DVec3]) -> Self {
        let range = |dir: DVec3| {
            points.iter().fold([f64::MAX, f64::MIN], |r, p| {
                let t = p.dot(dir);
                [r[0].min(t), r[1].max(t)]
            })
        };
        let [along, side, up] = axes.map(range);
        let slack = 0.003_f64.max(0.01 * (along[1] - along[0]));
        let at_ends = points
            .iter()
            .filter(|p| {
                let t = p.dot(axes[0]);
                t - along[0] <= slack || along[1] - t <= slack
            })
            .count();
        let straight = at_ends as f64 >= 0.95 * points.len() as f64;
        let mid = |r: [f64; 2]| (r[0] + r[1]) / 2.0;
        let at = |t: f64| axes[0] * t + axes[1] * mid(side) + axes[2] * mid(up);
        let width = side[1] - side[0];
        let height = up[1] - up[0];
        let section: Vec<DVec2> = points
            .iter()
            .map(|p| DVec2::new(p.dot(axes[1]), p.dot(axes[2])))
            .collect();
        let hull = convex_hull(section);
        let area = polygon_area(&hull).max(1e-9);
        let round =
            (width - height).abs() <= 0.1 * width.max(height) && area < 0.86 * width * height;
        Self {
            ends: [at(along[0]), at(along[1])],
            width,
            height,
            area,
            round,
            straight,
        }
    }

    /// `Ø160` or `400×200`, millimetres.
    pub fn size_label(&self) -> String {
        let mm = |v: f64| (v * 1000.0).round();
        if self.round {
            format!("Ø{}", mm(self.width.max(self.height)))
        } else {
            format!(
                "{}×{}",
                mm(self.width.max(self.height)),
                mm(self.width.min(self.height))
            )
        }
    }

    /// Outer size across, millimetres: the diameter of a round segment.
    pub fn size_mm(&self) -> f64 {
        self.width.max(self.height) * 1000.0
    }

    pub fn length(&self) -> f64 {
        self.ends[0].distance(self.ends[1])
    }

    /// Whether the section can be measured: a piece shorter than it is
    /// wide (a stub, a coupling) has no clear axis.
    pub fn measurable(&self) -> bool {
        self.straight && self.length() >= 0.6 * self.width.max(self.height)
    }

    pub fn horizontal_length(&self) -> f64 {
        (self.ends[1] - self.ends[0]).truncate().length()
    }

    pub fn rise(&self) -> f64 {
        self.ends[1].z - self.ends[0].z
    }
}

/// The run between the two ports furthest apart, with the frame's axes
/// most across it for the section; and the two port points as its ends.
fn port_axis(ports: &[DVec3], frame: &[DVec3; 3]) -> Option<([DVec3; 3], [DVec3; 2])> {
    let mut best: Option<(f64, [DVec3; 2])> = None;
    for (i, a) in ports.iter().enumerate() {
        for b in &ports[i + 1..] {
            let d = a.distance(*b);
            if best.is_none_or(|(l, _)| d > l) {
                best = Some((d, [*a, *b]));
            }
        }
    }
    let (length, ends) = best?;
    if length < 1e-4 {
        return None;
    }
    let axis = (ends[1] - ends[0]) / length;
    let across = frame
        .iter()
        .copied()
        .min_by(|a, b| a.dot(axis).abs().total_cmp(&b.dot(axis).abs()))?;
    let u = (across - axis * across.dot(axis)).try_normalize()?;
    let v = axis.cross(u);
    Some(([axis, u, v], ends))
}

fn polygon_area(points: &[DVec2]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| points[i].perp_dot(points[(i + 1) % n]))
        .sum::<f64>()
        .abs()
        / 2.0
}

pub(super) struct Net {
    pub nodes: Vec<Node>,
    pub adjacent: Vec<Vec<usize>>,
    /// Connections found from ports and from geometry.
    pub by_ports: usize,
    pub by_geometry: usize,
}

impl Net {
    /// Connected groups of nodes, following only edges `follow` allows.
    pub fn components(&self, follow: impl Fn(usize, usize) -> bool) -> Vec<Vec<usize>> {
        let mut seen = vec![false; self.nodes.len()];
        let mut out = Vec::new();
        for start in 0..self.nodes.len() {
            if seen[start] {
                continue;
            }
            seen[start] = true;
            let mut list = vec![start];
            let mut k = 0;
            while k < list.len() {
                let n = list[k];
                k += 1;
                for &m in &self.adjacent[n] {
                    if !seen[m] && follow(n, m) {
                        seen[m] = true;
                        list.push(m);
                    }
                }
            }
            out.push(list);
        }
        out
    }

    pub fn same_system(&self, a: usize, b: usize) -> bool {
        self.nodes[a].system == self.nodes[b].system
    }
}

const EQUIPMENT: &[&str] = &[
    "IfcUnitaryEquipment",
    "IfcAirToAirHeatRecovery",
    "IfcFan",
    "IfcPump",
    "IfcBoiler",
    "IfcChiller",
    "IfcCompressor",
    "IfcCoolingTower",
    "IfcHeatExchanger",
    "IfcFlowMovingDevice",
];

/// Electrical classes that are flow elements in the schema but no part of
/// a pipe or duct system.
fn is_electrical(class: &str) -> bool {
    [
        "IfcCable",
        "IfcElectric",
        "IfcLight",
        "IfcLamp",
        "IfcOutlet",
        "IfcSwitching",
        "IfcProtective",
        "IfcJunctionBox",
        "IfcCommunications",
        "IfcAudioVisual",
    ]
    .iter()
    .any(|p| class.starts_with(p))
}

fn role(model: &IfcModel, element: &Element) -> Option<(Role, Medium)> {
    let class = element.occurrence_class();
    if is_electrical(class) {
        return None;
    }
    let found = match class {
        "IfcPipeSegment" => (Role::Segment, Medium::Pipe),
        "IfcDuctSegment" => (Role::Segment, Medium::Duct),
        "IfcPipeFitting" => (Role::Fitting, Medium::Pipe),
        "IfcDuctFitting" => (Role::Fitting, Medium::Duct),
        "IfcFlowSegment" => (Role::Segment, Medium::Unknown),
        "IfcFlowFitting" => (Role::Fitting, Medium::Unknown),
        c if EQUIPMENT.contains(&c) => (Role::Equipment, Medium::Unknown),
        _ => {
            if !ifc_lite_core::is_subtype_of_for_schema(
                &model.schema,
                &element.ifc_class,
                "IfcDistributionFlowElement",
            ) {
                return None;
            }
            (Role::Other, Medium::Unknown)
        }
    };
    Some(found)
}

/// Names of properties that may hold the system, lower case.
const SYSTEM_PROPERTIES: &[&str] = &[
    "system name",
    "systemname",
    "system type",
    "systemtype",
    "system classification",
    "systemnavn",
    "system",
];

/// The element's system and what kind it is.
fn system_of(model: &IfcModel, element: &Element) -> Option<(String, String)> {
    if let Some(name) = element.systems.first() {
        let kind = model
            .system_info
            .get(name)
            .filter(|k| !k.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| name.clone());
        return Some((name.clone(), kind));
    }
    for id in &element.property_sets {
        let Some(set) = model.property_sets.get(id) else {
            continue;
        };
        for p in &set.properties {
            if SYSTEM_PROPERTIES.contains(&p.name.to_lowercase().as_str())
                && let Some(text) = p.value.as_text().filter(|t| !t.trim().is_empty())
            {
                return Some((text.to_owned(), text.to_owned()));
            }
        }
    }
    None
}

/// Builds the network of the flow elements in the models of `disciplines`.
/// Loose elements are connected where a segment end lies within
/// `tolerance` metres of another segment's end or inside another
/// element's box.
pub(super) fn build(models: &[VisualModel], disciplines: &[Discipline], tolerance: f64) -> Net {
    let mut nodes = Vec::new();
    let mut index: FxHashMap<ElementKey, usize> = FxHashMap::default();
    let mut has_system = Vec::new();
    let mut shapes: Vec<Option<([DVec3; 3], Vec<DVec3>)>> = Vec::new();
    for m in models {
        if !disciplines.contains(&m.model.discipline) {
            continue;
        }
        for e in &m.model.elements {
            if !e.has_geometry() || !m.includes(e.express_id) {
                continue;
            }
            let Some((role, medium)) = role(m.model, e) else {
                continue;
            };
            let system = system_of(m.model, e);
            // Segments: the frame's axes and the vertices, to measure the
            // run once it is known which way the segment connects.
            let shape = if role == Role::Segment {
                let triangles = m.triangles(e);
                RunFrame::of(&triangles).map(|frame| {
                    let axes = [frame.axis, frame.across, frame.normal].map(|a| a.as_dvec3());
                    let points: Vec<DVec3> =
                        triangles.iter().flatten().map(|p| p.as_dvec3()).collect();
                    (axes, points)
                })
            } else {
                None
            };
            // With two ports, the run is the line between them.
            let ports: Vec<DVec3> = m
                .model
                .ports
                .get(&e.express_id)
                .map(|p| {
                    p.iter()
                        .map(|q| m.transform.transform_point3(DVec3::from(q.map(f64::from))))
                        .collect()
                })
                .unwrap_or_default();
            let run = shape
                .as_ref()
                .map(|(axes, points)| match port_axis(&ports, axes) {
                    Some((axes, ends)) => Run {
                        ends,
                        ..Run::of(axes, points)
                    },
                    None => Run::of(*axes, points),
                });
            let shape = shape.filter(|_| ports.len() < 2);
            shapes.push(shape);
            let bounds = e.bounds.transformed(&m.transform);
            index.insert(m.key(e.express_id), nodes.len());
            has_system.push(system.is_some());
            let (system, kind) = system.unwrap_or_default();
            nodes.push(Node {
                key: m.key(e.express_id),
                role,
                medium,
                system,
                kind,
                run,
                bounds,
            });
        }
    }

    let mut edges: FxHashSet<[usize; 2]> = FxHashSet::default();
    let add = |a: usize, b: usize, edges: &mut FxHashSet<[usize; 2]>| {
        a != b && edges.insert([a.min(b), a.max(b)])
    };
    for m in models {
        for [a, b] in &m.model.connections {
            if let (Some(&na), Some(&nb)) = (index.get(&m.key(*a)), index.get(&m.key(*b))) {
                add(na, nb, &mut edges);
            }
        }
    }
    let by_ports = edges.len();
    let mut connected = vec![false; nodes.len()];
    for [a, b] in &edges {
        connected[*a] = true;
        connected[*b] = true;
    }

    // Loose elements: connect by geometry, within the same system.
    let cell = 1.0;
    let mut grid: FxHashMap<(i64, i64, i64), Vec<usize>> = FxHashMap::default();
    let cell_of = |p: DVec3| {
        (
            (p.x / cell).floor() as i64,
            (p.y / cell).floor() as i64,
            (p.z / cell).floor() as i64,
        )
    };
    for (i, n) in nodes.iter().enumerate() {
        let lo = cell_of(n.bounds.min.as_dvec3() - DVec3::splat(tolerance));
        let hi = cell_of(n.bounds.max.as_dvec3() + DVec3::splat(tolerance));
        // Very long elements would fill many cells; their ends are enough.
        if (hi.0 - lo.0 + 1) * (hi.1 - lo.1 + 1) * (hi.2 - lo.2 + 1) > 4096 {
            if let Some(run) = &n.run {
                for end in run.ends {
                    grid.entry(cell_of(end)).or_default().push(i);
                }
            }
            continue;
        }
        for x in lo.0..=hi.0 {
            for y in lo.1..=hi.1 {
                for z in lo.2..=hi.2 {
                    grid.entry((x, y, z)).or_default().push(i);
                }
            }
        }
    }
    let mut by_geometry = 0;
    for (i, n) in nodes.iter().enumerate() {
        let Some(run) = &n.run else { continue };
        for end in run.ends {
            let Some(candidates) = grid.get(&cell_of(end)) else {
                continue;
            };
            for &c in candidates {
                if c == i || (connected[i] && connected[c]) {
                    continue;
                }
                let other = &nodes[c];
                if has_system[i] && has_system[c] && n.system != other.system {
                    continue;
                }
                let meets = match &other.run {
                    Some(o) => o.ends.iter().any(|e| e.distance(end) <= tolerance),
                    None => {
                        let lo = other.bounds.min.as_dvec3() - DVec3::splat(tolerance);
                        let hi = other.bounds.max.as_dvec3() + DVec3::splat(tolerance);
                        end.cmpge(lo).all() && end.cmple(hi).all()
                    }
                };
                if meets && add(i, c, &mut edges) {
                    by_geometry += 1;
                }
            }
        }
    }

    let mut adjacent = vec![Vec::new(); nodes.len()];
    let mut list: Vec<[usize; 2]> = edges.into_iter().collect();
    list.sort_unstable();
    for [a, b] in list {
        adjacent[a].push(b);
        adjacent[b].push(a);
    }

    // A segment runs towards what it connects to. The longest side is not
    // always the run: a short piece of a wide duct is wider than long.
    for (i, shape) in shapes.iter().enumerate() {
        let Some((axes, points)) = shape else {
            continue;
        };
        if adjacent[i].is_empty() {
            continue;
        }
        let centre = nodes[i].bounds.center().as_dvec3();
        let score = |axis: DVec3| {
            adjacent[i]
                .iter()
                .filter_map(|&n| (nodes[n].bounds.center().as_dvec3() - centre).try_normalize())
                .map(|d| d.dot(axis).abs())
                .sum::<f64>()
        };
        let best = (0..3)
            .max_by(|&a, &b| score(axes[a]).total_cmp(&score(axes[b])))
            .unwrap_or(0);
        if best != 0 && score(axes[best]) > score(axes[0]) + 1e-6 {
            let rest: Vec<DVec3> = (0..3).filter(|&k| k != best).map(|k| axes[k]).collect();
            nodes[i].run = Some(Run::of([axes[best], rest[0], rest[1]], points));
        }
    }

    // Elements without a system take their neighbours'; what is left
    // forms a network of its own.
    let mut queue: std::collections::VecDeque<usize> =
        (0..nodes.len()).filter(|&i| has_system[i]).collect();
    let mut known = has_system.clone();
    while let Some(n) = queue.pop_front() {
        for &m in &adjacent[n] {
            if !known[m] {
                known[m] = true;
                nodes[m].system = nodes[n].system.clone();
                nodes[m].kind = nodes[n].kind.clone();
                queue.push_back(m);
            }
        }
    }
    let mut net = Net {
        nodes,
        adjacent,
        by_ports,
        by_geometry,
    };
    let loose: Vec<Vec<usize>> = net
        .components(|_, _| true)
        .into_iter()
        .filter(|c| !known[c[0]])
        .collect();
    for (k, component) in loose.into_iter().enumerate() {
        let name = format!("Network {}", k + 1);
        for n in component {
            net.nodes[n].system = name.clone();
            net.nodes[n].kind = name.clone();
        }
    }
    net
}

/// A colour for a kind of system: muted, and by what it carries where the
/// name tells (cold water blue, heating orange, drains brown, ...).
pub(super) fn system_color(kind: &str, fallback: usize) -> [u8; 3] {
    let k = kind.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| k.contains(w));
    if has(&["sprinkler", "brand", "fire"]) {
        [196, 120, 120]
    } else if has(&[
        "afløb",
        "spildevand",
        "sewage",
        "waste",
        "sanitary",
        "kloak",
        "soil",
    ]) {
        [150, 118, 88]
    } else if has(&["regn", "storm", "tagvand", "rain"]) {
        [96, 150, 150]
    } else if has(&["dræn", "drain"]) {
        [128, 136, 86]
    } else if has(&["kold", "cold", "køl", "cool", "chill"]) {
        [104, 146, 196]
    } else if has(&["retur", "return"]) && has(&["varm", "heat", "hot"]) {
        [196, 150, 118]
    } else if has(&["varm", "heat", "hot", "frem"]) {
        [216, 150, 92]
    } else if has(&["tilluft", "indblæsning", "supply air", "(vi)", "supply"]) {
        [110, 168, 140]
    } else if has(&["fraluft", "udsugning", "exhaust", "(vu)", "extract"]) {
        [170, 150, 200]
    } else if has(&["afkast", "udeluft", "outdoor", "intake"]) {
        [150, 170, 110]
    } else {
        super::muted(fallback)
    }
}
