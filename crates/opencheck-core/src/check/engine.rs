//! Runs rulesets against a federation.
//!
//! For each rule the two element groups are paired up with a sweep over
//! bounding boxes, then every candidate pair is tested on its triangles in
//! parallel. Only triangles inside the overlap of the two boxes take part, so
//! large elements stay cheap when they barely touch.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use sha2::{Digest, Sha256};

use super::geom::{
    Triangle, closest_point_on_triangle, point_in_mesh, triangle_aabb, triangle_intersection,
};
use super::ruleset::{Between, Rule, RuleKind, Ruleset, RulesetInfo, Selector};
use crate::{Aabb, IfcModel, ModelId};

/// A model taking part in a check, placed in the federation frame.
#[derive(Clone)]
pub struct CheckModel {
    pub id: ModelId,
    pub model: Arc<IfcModel>,
    pub transform: Mat4,
}

/// One ruleset to run and the models it runs on.
#[derive(Clone)]
pub struct CheckJob {
    pub ruleset: Ruleset,
    pub models: Vec<ModelId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FindingKind {
    /// Surfaces cross each other.
    Clash,
    /// One element lies completely inside the other.
    Inside,
    /// Closer than the required clearance.
    Clearance,
    /// The same element twice.
    Duplicate,
}

impl FindingKind {
    pub fn label(self) -> &'static str {
        match self {
            FindingKind::Clash => "Clash",
            FindingKind::Inside => "Inside",
            FindingKind::Clearance => "Clearance",
            FindingKind::Duplicate => "Duplicate",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FindingElement {
    pub model: ModelId,
    pub express_id: u32,
    pub global_id: String,
    pub name: String,
    pub ifc_class: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    /// Stable across runs for the same rule and element pair (UUID format,
    /// so it can become a BCF topic guid).
    pub id: String,
    pub kind: FindingKind,
    pub a: FindingElement,
    pub b: FindingElement,
    /// Centre of the problem, federation frame.
    pub point: Vec3,
    /// Extent of the problem, federation frame.
    pub region: Aabb,
    /// Clash: estimated penetration depth. Clearance: actual distance.
    /// Duplicate: largest offset between the copies. Metres.
    pub magnitude: f32,
}

#[derive(Debug, Clone)]
pub struct RuleResult {
    pub rule: Rule,
    pub findings: Vec<Finding>,
    pub pairs_tested: usize,
    pub duration: Duration,
}

#[derive(Debug, Clone)]
pub struct RulesetResult {
    pub info: RulesetInfo,
    pub rules: Vec<RuleResult>,
}

#[derive(Debug, Clone, Default)]
pub struct CheckReport {
    pub rulesets: Vec<RulesetResult>,
    pub duration: Duration,
}

impl CheckReport {
    pub fn finding_count(&self) -> usize {
        self.rulesets
            .iter()
            .flat_map(|r| &r.rules)
            .map(|r| r.findings.len())
            .sum()
    }
}

/// An element ready for testing: its triangles in the federation frame.
struct Prepared {
    model: usize,
    element: usize,
    triangles: Vec<Triangle>,
    boxes: Vec<Aabb>,
    bounds: Aabb,
}

impl Prepared {
    fn new(models: &[CheckModel], model: usize, element: usize) -> Option<Self> {
        let source = &models[model];
        let e = &source.model.elements[element];
        let mut triangles = Vec::new();
        for &mesh_index in &e.meshes {
            let mesh = &source.model.meshes[mesh_index];
            let world: Vec<Vec3> = mesh
                .positions
                .iter()
                .map(|p| source.transform.transform_point3(Vec3::from_array(*p)))
                .collect();
            for tri in mesh.indices.as_chunks::<3>().0 {
                let t = tri.map(|i| world[i as usize]);
                // Degenerate triangles add nothing and confuse the tests.
                if (t[1] - t[0]).cross(t[2] - t[0]).length_squared() > 1e-14 {
                    triangles.push(t);
                }
            }
        }
        if triangles.is_empty() {
            return None;
        }
        let boxes: Vec<Aabb> = triangles.iter().map(triangle_aabb).collect();
        let bounds = boxes.iter().fold(Aabb::EMPTY, |acc, b| acc.union(b));
        Some(Self {
            model,
            element,
            triangles,
            boxes,
            bounds,
        })
    }
}

/// Elements on side a and side b of one rule, as (model index, element index).
type RuleGroups = (Vec<(usize, usize)>, Vec<(usize, usize)>);

/// Runs the jobs. `progress` receives values from 0 to 1.
pub fn run_check(
    models: &[CheckModel],
    jobs: &[CheckJob],
    progress: &(dyn Fn(f32) + Sync),
) -> CheckReport {
    let started = Instant::now();

    // Element groups per rule, by (model index, element index).
    let groups: Vec<Vec<RuleGroups>> = jobs
        .iter()
        .map(|job| {
            job.ruleset
                .rules
                .iter()
                .map(|rule| {
                    let a = select(models, &job.models, &rule.a);
                    let b = match &rule.b {
                        Some(selector) => select(models, &job.models, selector),
                        None => a.clone(),
                    };
                    (a, b)
                })
                .collect()
        })
        .collect();

    // Prepare every element any rule needs, once.
    let needed: FxHashSet<(usize, usize)> = groups
        .iter()
        .flatten()
        .flat_map(|(a, b)| a.iter().chain(b.iter()).copied())
        .collect();
    let needed: Vec<(usize, usize)> = needed.into_iter().collect();
    let prepared: Vec<Prepared> = needed
        .par_iter()
        .filter_map(|&(m, e)| Prepared::new(models, m, e))
        .collect();
    let index: FxHashMap<(usize, usize), usize> = prepared
        .iter()
        .enumerate()
        .map(|(i, p)| ((p.model, p.element), i))
        .collect();
    progress(0.1);

    let total_rules: usize = jobs
        .iter()
        .map(|j| j.ruleset.rules.len())
        .sum::<usize>()
        .max(1);
    let done = AtomicUsize::new(0);
    let mut report = CheckReport::default();
    for (job, job_groups) in jobs.iter().zip(groups) {
        let mut rules = Vec::new();
        for (rule, (a, b)) in job.ruleset.rules.iter().zip(job_groups) {
            let rule_started = Instant::now();
            let resolve = |list: &[(usize, usize)]| -> Vec<usize> {
                list.iter()
                    .filter_map(|key| index.get(key).copied())
                    .collect()
            };
            let (a, b) = (resolve(&a), resolve(&b));
            let pairs = candidate_pairs(&prepared, models, rule, &a, &b, rule.b.is_none());
            let findings: Vec<Finding> = pairs
                .par_iter()
                .filter_map(|&(i, j)| {
                    test_pair(
                        &prepared[i],
                        &prepared[j],
                        models,
                        rule,
                        &job.ruleset.ruleset.id,
                    )
                })
                .collect();
            let mut findings = findings;
            findings.sort_by(|x, y| y.magnitude.total_cmp(&x.magnitude));
            rules.push(RuleResult {
                rule: rule.clone(),
                findings,
                pairs_tested: pairs.len(),
                duration: rule_started.elapsed(),
            });
            let n = done.fetch_add(1, Ordering::Relaxed) + 1;
            progress(0.1 + 0.9 * n as f32 / total_rules as f32);
        }
        report.rulesets.push(RulesetResult {
            info: job.ruleset.ruleset.clone(),
            rules,
        });
    }
    report.duration = started.elapsed();
    report
}

fn select(models: &[CheckModel], scope: &[ModelId], selector: &Selector) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (m, check_model) in models.iter().enumerate() {
        if !scope.contains(&check_model.id) {
            continue;
        }
        let model = &check_model.model;
        for (e, element) in model.elements.iter().enumerate() {
            if element.has_geometry() && selector.matches(model, element) {
                out.push((m, e));
            }
        }
    }
    out
}

/// Pairs whose boxes overlap (grown by the clearance), after the rule's
/// filters. With `symmetric` each unordered pair appears once.
fn candidate_pairs(
    prepared: &[Prepared],
    models: &[CheckModel],
    rule: &Rule,
    a: &[usize],
    b: &[usize],
    symmetric: bool,
) -> Vec<(usize, usize)> {
    let margin = match rule.kind {
        RuleKind::Clearance => rule.clearance_mm / 1000.0,
        RuleKind::Duplicates => rule.tolerance_mm / 1000.0,
        RuleKind::Clash => 0.0,
    };
    let mut sorted_b: Vec<usize> = b.to_vec();
    sorted_b.sort_by(|x, y| {
        prepared[*x]
            .bounds
            .min
            .x
            .total_cmp(&prepared[*y].bounds.min.x)
    });
    let starts: Vec<f32> = sorted_b.iter().map(|i| prepared[*i].bounds.min.x).collect();
    let widest = sorted_b
        .iter()
        .map(|i| prepared[*i].bounds.size().x)
        .fold(0.0f32, f32::max);

    let mut seen = FxHashSet::default();
    let mut pairs = Vec::new();
    for &i in a {
        let pa = &prepared[i];
        let grown = Aabb {
            min: pa.bounds.min - Vec3::splat(margin),
            max: pa.bounds.max + Vec3::splat(margin),
        };
        let from = starts.partition_point(|x| *x < grown.min.x - widest);
        for (&j, &start) in sorted_b[from..].iter().zip(&starts[from..]) {
            if start > grown.max.x {
                break;
            }
            if i == j || !grown.intersects(&prepared[j].bounds) {
                continue;
            }
            if !pair_allowed(pa, &prepared[j], models, rule) {
                continue;
            }
            // Each unordered pair once; keep the a-b order when the groups differ.
            if seen.insert((i.min(j), i.max(j))) {
                pairs.push(if symmetric {
                    (i.min(j), i.max(j))
                } else {
                    (i, j)
                });
            }
        }
    }
    pairs
}

fn pair_allowed(a: &Prepared, b: &Prepared, models: &[CheckModel], rule: &Rule) -> bool {
    let same_model = a.model == b.model;
    match rule.between {
        Between::DifferentModels if same_model => return false,
        Between::SameModel if !same_model => return false,
        _ => {}
    }
    if same_model {
        // Parts touch their assembly and each other by design.
        let elements = &models[a.model].model.elements;
        let (ea, eb) = (&elements[a.element], &elements[b.element]);
        if ea.parent == Some(eb.express_id)
            || eb.parent == Some(ea.express_id)
            || (ea.parent.is_some() && ea.parent == eb.parent)
        {
            return false;
        }
    }
    true
}

fn test_pair(
    a: &Prepared,
    b: &Prepared,
    models: &[CheckModel],
    rule: &Rule,
    ruleset_id: &str,
) -> Option<Finding> {
    let (kind, point, region, magnitude) = match rule.kind {
        RuleKind::Clash => clash(a, b, rule.tolerance_mm / 1000.0)?,
        RuleKind::Clearance => clearance(a, b, rule.clearance_mm / 1000.0)?,
        RuleKind::Duplicates => duplicate(a, b, models, rule.tolerance_mm / 1000.0)?,
    };
    let element = |p: &Prepared| {
        let model = &models[p.model];
        let e = &model.model.elements[p.element];
        FindingElement {
            model: model.id,
            express_id: e.express_id,
            global_id: e.global_id.clone(),
            name: e.name.clone(),
            ifc_class: e.ifc_class.clone(),
        }
    };
    let (ea, eb) = (element(a), element(b));
    Some(Finding {
        id: finding_id(ruleset_id, &rule.id, &ea.global_id, &eb.global_id),
        kind,
        a: ea,
        b: eb,
        point,
        region,
        magnitude,
    })
}

/// UUID-formatted hash of the rule and the (unordered) element pair.
pub fn finding_id(ruleset: &str, rule: &str, guid_a: &str, guid_b: &str) -> String {
    let (x, y) = if guid_a <= guid_b {
        (guid_a, guid_b)
    } else {
        (guid_b, guid_a)
    };
    let hash = Sha256::digest(format!("{ruleset}\n{rule}\n{x}\n{y}").as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50; // version 5 style
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn overlap(a: &Aabb, b: &Aabb) -> Option<Aabb> {
    let o = Aabb {
        min: a.min.max(b.min),
        max: a.max.min(b.max),
    };
    (!o.is_empty()).then_some(o)
}

fn grow(b: &Aabb, by: f32) -> Aabb {
    Aabb {
        min: b.min - Vec3::splat(by),
        max: b.max + Vec3::splat(by),
    }
}

/// Indices of the triangles whose boxes reach into `region`.
fn triangles_in(p: &Prepared, region: &Aabb) -> Vec<usize> {
    (0..p.triangles.len())
        .filter(|&i| p.boxes[i].intersects(region))
        .collect()
}

type Detection = (FindingKind, Vec3, Aabb, f32);

fn clash(a: &Prepared, b: &Prepared, tolerance: f32) -> Option<Detection> {
    let region = grow(&overlap(&a.bounds, &b.bounds)?, 1e-4);
    let ta = triangles_in(a, &region);
    let tb = triangles_in(b, &region);

    let mut points = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    for &i in &ta {
        for &j in &tb {
            if !a.boxes[i].intersects(&b.boxes[j]) {
                continue;
            }
            if triangle_intersection(&a.triangles[i], &b.triangles[j], &mut points) {
                for t in [&a.triangles[i], &b.triangles[j]] {
                    let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
                    if normals.len() < 32 && !normals.iter().any(|m| m.dot(n).abs() > 0.999) {
                        normals.push(n);
                    }
                }
            }
        }
    }

    if points.is_empty() {
        return containment(a, b).or_else(|| containment(b, a));
    }

    // The intersection points lie on both surfaces. Their spread across the
    // faces involved estimates how deep the elements overlap: a pipe through
    // a slab spreads over the slab thickness, two walls that merely meet
    // spread over almost nothing.
    let spread = normals
        .iter()
        .map(|n| {
            let (lo, hi) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
                let d = p.dot(*n);
                (lo.min(d), hi.max(d))
            });
            hi - lo
        })
        .fold(f32::MAX, f32::min);
    let depth = spread
        .max(vertex_penetration(a, b, &region))
        .max(vertex_penetration(b, a, &region));
    // Touching surfaces give points with no depth at all.
    if depth <= tolerance.max(1e-4) {
        return None;
    }
    let found = Aabb::from_points(points);
    Some((FindingKind::Clash, found.center(), found, depth))
}

/// How far vertices of `a` reach into `b`, for the vertices within `region`.
fn vertex_penetration(a: &Prepared, b: &Prepared, region: &Aabb) -> f32 {
    const MAX_VERTICES: usize = 128;
    let mut depth = 0.0f32;
    let mut tested = 0;
    for t in &a.triangles {
        for &v in t {
            if tested >= MAX_VERTICES {
                return depth;
            }
            if v.cmplt(region.min).any() || v.cmpgt(region.max).any() {
                continue;
            }
            tested += 1;
            if point_in_mesh(v, &b.triangles) {
                let distance = b
                    .triangles
                    .iter()
                    .map(|t| closest_point_on_triangle(v, t).distance(v))
                    .fold(f32::MAX, f32::min);
                depth = depth.max(distance);
            }
        }
    }
    depth
}

/// `inner` lies completely inside `outer` without the surfaces crossing.
fn containment(inner: &Prepared, outer: &Prepared) -> Option<Detection> {
    let inside_box = inner.bounds.min.cmpge(outer.bounds.min).all()
        && inner.bounds.max.cmple(outer.bounds.max).all();
    if !inside_box || !point_in_mesh(inner.triangles[0][0], &outer.triangles) {
        return None;
    }
    Some((
        FindingKind::Inside,
        inner.bounds.center(),
        inner.bounds,
        inner.bounds.size().min_element(),
    ))
}

fn clearance(a: &Prepared, b: &Prepared, required: f32) -> Option<Detection> {
    let near_a = triangles_in(a, &grow(&b.bounds, required));
    let near_b = triangles_in(b, &grow(&a.bounds, required));
    if near_a.is_empty() || near_b.is_empty() {
        return None;
    }
    let mut best = required;
    let mut closest: Option<(Vec3, Vec3)> = None;
    let probe = |from: &Prepared,
                 from_tris: &[usize],
                 to: &Prepared,
                 to_tris: &[usize],
                 best: &mut f32,
                 closest: &mut Option<(Vec3, Vec3)>| {
        for &i in from_tris {
            for &v in &from.triangles[i] {
                for &j in to_tris {
                    if !grow(&to.boxes[j], *best).contains_point(v) {
                        continue;
                    }
                    let p = closest_point_on_triangle(v, &to.triangles[j]);
                    let d = p.distance(v);
                    if d < *best {
                        *best = d;
                        *closest = Some((v, p));
                    }
                }
            }
        }
    };
    probe(a, &near_a, b, &near_b, &mut best, &mut closest);
    probe(b, &near_b, a, &near_a, &mut best, &mut closest);
    let (p, q) = closest?;
    let region = Aabb::from_points([p, q]);
    Some((FindingKind::Clearance, (p + q) * 0.5, region, best))
}

fn duplicate(
    a: &Prepared,
    b: &Prepared,
    models: &[CheckModel],
    tolerance: f32,
) -> Option<Detection> {
    let class_a = &models[a.model].model.elements[a.element].ifc_class;
    let class_b = &models[b.model].model.elements[b.element].ifc_class;
    if class_a != class_b || a.triangles.len() != b.triangles.len() {
        return None;
    }
    let offset = (a.bounds.min - b.bounds.min)
        .abs()
        .max((a.bounds.max - b.bounds.max).abs())
        .max_element();
    (offset <= tolerance).then(|| (FindingKind::Duplicate, a.bounds.center(), a.bounds, offset))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::geom::tests::box_triangles;

    fn prepared(min: Vec3, max: Vec3) -> Prepared {
        let triangles = box_triangles(min, max);
        let boxes: Vec<Aabb> = triangles.iter().map(triangle_aabb).collect();
        Prepared {
            model: 0,
            element: 0,
            bounds: Aabb { min, max },
            triangles,
            boxes,
        }
    }

    #[test]
    fn pipe_through_slab_is_a_clash_with_slab_depth() {
        let slab = prepared(Vec3::new(-2.0, -2.0, 0.0), Vec3::new(2.0, 2.0, 0.25));
        let pipe = prepared(Vec3::new(-0.05, -0.05, -1.0), Vec3::new(0.05, 0.05, 1.0));
        let (kind, _, _, depth) = clash(&pipe, &slab, 0.01).expect("clash");
        assert_eq!(kind, FindingKind::Clash);
        // Limited by the pipe width (0.1) rather than the slab (0.25).
        assert!((depth - 0.1).abs() < 0.01, "depth {depth}");
    }

    #[test]
    fn shallow_overlap_is_within_tolerance() {
        let column = prepared(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.4, 0.4, 3.0));
        // Beam reaches 4 mm into the column face.
        let beam = prepared(Vec3::new(0.396, 0.1, 2.5), Vec3::new(5.0, 0.3, 2.9));
        assert!(clash(&beam, &column, 0.010).is_none());
        let (_, _, _, depth) = clash(&beam, &column, 0.001).expect("deeper than 1 mm");
        assert!((depth - 0.004).abs() < 0.001, "depth {depth}");
    }

    #[test]
    fn touching_boxes_do_not_clash() {
        let a = prepared(Vec3::ZERO, Vec3::ONE);
        let b = prepared(Vec3::new(1.0, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        assert!(clash(&a, &b, 0.0).is_none());
    }

    #[test]
    fn element_inside_another() {
        let outer = prepared(Vec3::ZERO, Vec3::splat(2.0));
        let inner = prepared(Vec3::splat(0.5), Vec3::splat(1.0));
        let (kind, ..) = clash(&inner, &outer, 0.01).expect("inside");
        assert_eq!(kind, FindingKind::Inside);
    }

    #[test]
    fn clearance_reports_distance() {
        let a = prepared(Vec3::ZERO, Vec3::ONE);
        let b = prepared(Vec3::new(1.03, 0.0, 0.0), Vec3::new(2.0, 1.0, 1.0));
        let (_, _, _, distance) = clearance(&a, &b, 0.05).expect("too close");
        assert!((distance - 0.03).abs() < 1e-4);
        assert!(clearance(&a, &b, 0.02).is_none());
    }

    #[test]
    fn finding_ids_ignore_pair_order() {
        assert_eq!(
            finding_id("s", "r", "A", "B"),
            finding_id("s", "r", "B", "A")
        );
        assert_ne!(
            finding_id("s", "r", "A", "B"),
            finding_id("s", "r2", "A", "B")
        );
        assert_eq!(finding_id("s", "r", "A", "B").len(), 36);
    }
}
