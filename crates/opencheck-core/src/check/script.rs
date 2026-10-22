//! Script rules: a rule written in Rhai, a small scripting language, for
//! checks the built-in rule types cannot express. Scripts are meant to be
//! written with the help of a language model; [`SCRIPT_API`] is the reference
//! to hand it.
//!
//! A script sees the elements of the rule's two groups, measures them with
//! the same geometry as the built-in rules (runs, free zones, distances) and
//! reports findings. It cannot read files or reach the network, and it is
//! stopped when it runs too long.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use glam::Vec3;
use rhai::{AST, Array, Dynamic, Engine, EvalAltResult, ImmutableString, Map, Position};

use super::engine::{
    CheckModel, Finding, FindingElement, FindingKind, Prepared, blocked_zones, connected, distance,
    finding_element, finding_id, joins,
};
use super::freezone::{RunFrame, ZoneBox, intersect, merge};
use super::ruleset::{Rule, ZoneSide};
use crate::{Aabb, PropertyValue};

/// The script API reference, for people and language models writing rules.
pub const SCRIPT_API: &str = include_str!("../../../../docs/rule-scripts.md");

/// Script work is counted in operations; this stops runaway loops.
const MAX_OPERATIONS: u64 = 500_000_000;

/// Checks a script for syntax errors.
pub fn compile(script: &str) -> Result<(), String> {
    Engine::new_raw()
        .compile(script)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// What a running script shares with the functions it calls.
struct World {
    prepared: Arc<Vec<Prepared>>,
    models: Vec<CheckModel>,
    subjects: Vec<usize>,
    others: Vec<usize>,
    params: BTreeMap<String, f64>,
    rule: Rule,
    ruleset_id: String,
    findings: Mutex<Vec<Finding>>,
    notes: Mutex<Vec<String>>,
}

/// An element as a script sees it.
#[derive(Clone)]
struct El {
    world: Arc<World>,
    index: usize,
}

impl El {
    fn prepared(&self) -> &Prepared {
        &self.world.prepared[self.index]
    }

    fn element(&self) -> &crate::Element {
        let p = self.prepared();
        &self.world.models[p.model].model.elements[p.element]
    }

    fn model(&self) -> &crate::IfcModel {
        &self.world.models[self.prepared().model].model
    }

    fn frame(&self) -> Option<RunFrame> {
        RunFrame::of(&self.prepared().triangles)
    }
}

/// A blocked stretch of a zone, kept so a finding can draw it.
#[derive(Clone)]
struct Stretch {
    el: El,
    by: Option<El>,
    from: f32,
    to: f32,
    zones: Vec<ZoneBox>,
}

type Fail = Box<EvalAltResult>;

fn fail(message: impl Into<String>) -> Fail {
    Box::new(EvalAltResult::ErrorRuntime(
        Dynamic::from(message.into()),
        Position::NONE,
    ))
}

/// Whole millimetres: scripts work in integers, so `%` and text read well.
fn mm(metres: f32) -> i64 {
    (f64::from(metres) * 1000.0).round() as i64
}

fn value(v: &PropertyValue) -> Dynamic {
    match v {
        PropertyValue::Integer(i) => Dynamic::from(*i),
        PropertyValue::Real(r) => Dynamic::from(*r),
        PropertyValue::Boolean(b) => Dynamic::from(*b),
        other => Dynamic::from(other.to_string()),
    }
}

/// Runs a script rule. Returns its findings and what it printed (and its
/// error, if it failed).
pub(super) fn run(
    prepared: &Arc<Vec<Prepared>>,
    models: &[CheckModel],
    rule: &Rule,
    subjects: &[usize],
    others: &[usize],
    ruleset_id: &str,
) -> (Vec<Finding>, Vec<String>) {
    let world = Arc::new(World {
        prepared: Arc::clone(prepared),
        models: models.to_vec(),
        subjects: subjects.to_vec(),
        others: others.to_vec(),
        params: rule.params.clone(),
        rule: rule.clone(),
        ruleset_id: ruleset_id.to_owned(),
        findings: Mutex::new(Vec::new()),
        notes: Mutex::new(Vec::new()),
    });
    let engine = engine(&world);
    let result = engine
        .compile(&rule.script)
        .map_err(|e| e.to_string())
        .and_then(|ast: AST| engine.run_ast(&ast).map_err(|e| e.to_string()));
    let mut notes = std::mem::take(&mut *world.notes.lock().unwrap_or_else(|e| e.into_inner()));
    if let Err(e) = result {
        notes.push(format!("Script error: {e}"));
    }
    let findings = std::mem::take(&mut *world.findings.lock().unwrap_or_else(|e| e.into_inner()));
    (findings, notes)
}

fn engine(world: &Arc<World>) -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(MAX_OPERATIONS);
    engine.set_max_call_levels(64);
    engine.set_max_string_size(100_000);
    engine.set_max_array_size(1_000_000);

    let w = Arc::clone(world);
    engine.on_print(move |text| {
        let mut notes = w.notes.lock().unwrap_or_else(|e| e.into_inner());
        if notes.len() < 500 {
            notes.push(text.to_owned());
        }
    });
    engine.on_debug(|_, _, _| {});

    engine.register_type_with_name::<El>("Element");
    engine.register_type_with_name::<Stretch>("Stretch");

    // Groups and parameters.
    let w = Arc::clone(world);
    engine.register_fn("subjects", move || -> Array {
        w.subjects
            .iter()
            .map(|&i| {
                Dynamic::from(El {
                    world: Arc::clone(&w),
                    index: i,
                })
            })
            .collect()
    });
    let w = Arc::clone(world);
    engine.register_fn("others", move || -> Array {
        w.others
            .iter()
            .map(|&i| {
                Dynamic::from(El {
                    world: Arc::clone(&w),
                    index: i,
                })
            })
            .collect()
    });
    let w = Arc::clone(world);
    engine.register_fn("param", move |name: &str| -> Result<Dynamic, Fail> {
        let v =
            w.params.get(name).copied().ok_or_else(|| {
                fail(format!("no parameter named \"{name}\"; add it to the rule"))
            })?;
        // Whole numbers as integers, so they mix with the millimetres the
        // other functions return.
        Ok(if v.fract() == 0.0 && v.abs() < 1e15 {
            Dynamic::from(v as i64)
        } else {
            Dynamic::from(v)
        })
    });

    // Element facts.
    engine.register_get("name", |e: &mut El| e.element().name.clone());
    engine.register_get("class", |e: &mut El| e.element().ifc_class.clone());
    engine.register_get("type_class", |e: &mut El| {
        e.element().occurrence_class().to_owned()
    });
    engine.register_get("global_id", |e: &mut El| e.element().global_id.clone());
    engine.register_get("type_name", |e: &mut El| {
        e.element()
            .type_object
            .as_ref()
            .map(|(_, n)| n.clone())
            .unwrap_or_default()
    });
    engine.register_get("object_type", |e: &mut El| {
        e.element().object_type.clone().unwrap_or_default()
    });
    engine.register_get("systems", |e: &mut El| -> Array {
        e.element()
            .systems
            .iter()
            .map(|s| Dynamic::from(s.clone()))
            .collect()
    });
    engine.register_get("model", |e: &mut El| e.model().short_name.clone());
    engine.register_get("discipline", |e: &mut El| {
        e.model().discipline.code().to_owned()
    });
    engine.register_get("storey", |e: &mut El| {
        let element = e.element();
        storey_of(e.model(), element.container)
            .map(|(name, _)| name)
            .unwrap_or_default()
    });
    engine.register_get("storey_elevation", |e: &mut El| -> Dynamic {
        let element = e.element();
        let model = e.model();
        match storey_of(model, element.container).and_then(|(_, z)| z) {
            Some(z) => Dynamic::from((z * model.length_unit_scale * 1000.0).round() as i64),
            None => Dynamic::UNIT,
        }
    });
    engine.register_fn("prop", |e: &mut El, name: &str| -> Dynamic {
        let (pset, prop) = match name.split_once('.') {
            Some((s, p)) => (Some(s), p),
            None => (None, name),
        };
        let element = e.element();
        if pset.is_none()
            && let Some(v) = element.attribute(prop)
        {
            return value(v);
        }
        for set in e.model().property_sets_of(element) {
            if pset.is_some_and(|s| !s.eq_ignore_ascii_case(&set.name)) {
                continue;
            }
            if let Some(p) = set
                .properties
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(prop))
            {
                return value(&p.value);
            }
        }
        Dynamic::UNIT
    });
    engine.register_get("bounds", |e: &mut El| -> Map {
        let b = e.prepared().bounds;
        let mut m = Map::new();
        for (k, v) in [
            ("min_x", b.min.x),
            ("min_y", b.min.y),
            ("min_z", b.min.z),
            ("max_x", b.max.x),
            ("max_y", b.max.y),
            ("max_z", b.max.z),
        ] {
            m.insert(k.into(), Dynamic::from(mm(v)));
        }
        m
    });
    engine.register_get("run", |e: &mut El| -> Map {
        let mut m = Map::new();
        if let Some(f) = e.frame() {
            m.insert("length".into(), Dynamic::from(mm(f.length())));
            m.insert("width".into(), Dynamic::from(mm(f.width())));
            m.insert("height".into(), Dynamic::from(mm(f.height())));
            m.insert("vertical".into(), Dynamic::from(f.is_vertical()));
            m.insert("horizontal".into(), Dynamic::from(f.axis.z.abs() <= 0.5));
            let slope = f.axis.z.abs().clamp(0.0, 1.0).asin().to_degrees();
            m.insert(
                "slope".into(),
                Dynamic::from(f64::from(slope).round() as i64),
            );
        }
        m
    });

    // Measuring.
    engine.register_fn("distance", |a: &mut El, b: El| -> i64 {
        // Up to 10 m; farther apart counts as 10 m.
        let d = distance(a.prepared(), b.prepared(), 10.0).unwrap_or(10.0);
        mm(d)
    });
    engine.register_fn("distance", |a: &mut El, b: El, max: i64| -> i64 {
        // Only looks as far as `max`: much faster for short distances.
        let limit = (max.max(1) as f32) / 1000.0;
        distance(a.prepared(), b.prepared(), limit).map_or(max, mm)
    });
    engine.register_fn("nearby", |e: &mut El, within: i64| -> Array {
        let grow = (within.max(0) as f32) / 1000.0;
        let b = e.prepared().bounds;
        let reach = Aabb {
            min: b.min - Vec3::splat(grow),
            max: b.max + Vec3::splat(grow),
        };
        e.world
            .others
            .iter()
            .filter(|&&j| j != e.index && e.world.prepared[j].bounds.intersects(&reach))
            .map(|&j| {
                Dynamic::from(El {
                    world: Arc::clone(&e.world),
                    index: j,
                })
            })
            .collect()
    });
    engine.register_fn("connected", |a: &mut El, b: El| -> bool {
        connected(a.prepared(), b.prepared(), &a.world.models)
    });
    engine.register_fn("joins", |a: &mut El, b: El| -> bool {
        joins(a.prepared(), b.prepared(), 0.1)
    });
    engine.register_fn("angle", |a: &mut El, b: El| -> i64 {
        match (a.frame(), b.frame()) {
            (Some(fa), Some(fb)) => {
                let cos = fa.axis.dot(fb.axis).abs().clamp(0.0, 1.0);
                f64::from(cos.acos().to_degrees()).round() as i64
            }
            _ => 0,
        }
    });
    let number = |v: &Dynamic| -> Result<f64, Fail> {
        v.as_float()
            .or_else(|_| v.as_int().map(|i| i as f64))
            .map_err(|_| fail("the zone depth must be a number of millimetres"))
    };
    engine.register_fn(
        "blocked",
        move |e: &mut El, side: &str, depth: Dynamic| -> Result<Array, Fail> {
            blocked(e, side, number(&depth)?, None)
        },
    );
    engine.register_fn(
        "blocked",
        move |e: &mut El, side: &str, depth: Dynamic, by: Array| -> Result<Array, Fail> {
            let others: Vec<usize> = by
                .into_iter()
                .filter_map(|d| d.try_cast::<El>())
                .map(|el| el.index)
                .collect();
            blocked(e, side, number(&depth)?, Some(others))
        },
    );
    engine.register_get("from", |s: &mut Stretch| mm(s.from));
    engine.register_get("to", |s: &mut Stretch| mm(s.to));
    engine.register_get("length", |s: &mut Stretch| mm(s.to - s.from));
    engine.register_get("element", |s: &mut Stretch| s.el.clone());
    engine.register_get("by", |s: &mut Stretch| -> Dynamic {
        s.by.clone().map_or(Dynamic::UNIT, Dynamic::from)
    });

    // Reporting.
    engine.register_fn("report", |e: El, message: &str| {
        report(&e, None, message, &[], 0.0, None);
    });
    engine.register_fn("report", |e: El, other: El, message: &str| {
        report(&e, Some(&other), message, &[], 0.0, None);
    });
    engine.register_fn("report", |s: Stretch, message: &str| {
        report(
            &s.el,
            s.by.as_ref(),
            message,
            &s.zones,
            s.to - s.from,
            Some(s.from),
        );
    });
    engine.register_fn("to_string", |e: &mut El| -> ImmutableString {
        format!("{} ({})", e.element().label(), e.element().ifc_class).into()
    });
    engine
}

/// The storey (name, elevation in model units) containing an element.
fn storey_of(model: &crate::IfcModel, container: Option<u32>) -> Option<(String, Option<f64>)> {
    let container = container?;
    let root = model.spatial_root.as_ref()?;
    let mut found = None;
    root.walk(&mut |node, _| {
        if node.express_id == container {
            found = Some((node.label().to_owned(), node.elevation));
        }
    });
    found
}

/// Blocked stretches of a zone around `e`, by the rule's other group or by
/// the given elements.
fn blocked(e: &El, side: &str, depth: f64, by: Option<Vec<usize>>) -> Result<Array, Fail> {
    let Some(frame) = e.frame() else {
        return Ok(Array::new());
    };
    let depth = (depth / 1000.0) as f32;
    if depth <= 0.0 {
        return Err(fail("the zone depth must be above 0 mm"));
    }
    let world = &e.world;
    let others = by.unwrap_or_else(|| world.others.clone());
    // "left"/"right" and "front"/"back" are single zones of the pairs.
    let (zone_side, pick): (ZoneSide, Option<usize>) = match side {
        "above" => (ZoneSide::Above, None),
        "below" => (ZoneSide::Below, None),
        "left" => (ZoneSide::BothSides, Some(0)),
        "right" => (ZoneSide::BothSides, Some(1)),
        "one_side" => (ZoneSide::OneSide, None),
        "back" => (ZoneSide::OneFace, Some(0)),
        "front" => (ZoneSide::OneFace, Some(1)),
        "one_face" => (ZoneSide::OneFace, None),
        other => {
            return Err(fail(format!(
                "unknown side \"{other}\"; use above, below, left, right, one_side, front, back or one_face"
            )));
        }
    };
    let mut zones = ZoneBox::around(&frame, zone_side, depth);
    // Both zones of a pair are measured, so an element surrounding this one
    // (the edges of an opening it passes through) is seen as a crossing.
    let mut per_zone = blocked_zones(&world.prepared, &world.models, e.index, &zones, &others);
    if let Some(i) = pick {
        zones = vec![zones[i]];
        per_zone = vec![per_zone.swap_remove(i)];
    }
    let union = |list: &Vec<(usize, Vec<[f32; 2]>)>| {
        merge(
            list.iter().flat_map(|(_, s)| s.iter().copied()).collect(),
            0.02,
        )
    };
    let mut stretches = union(&per_zone[0]);
    for list in &per_zone[1..] {
        stretches = intersect(&stretches, &union(list));
    }
    let blockers: Vec<(usize, Vec<[f32; 2]>)> = per_zone.concat();
    Ok(stretches
        .into_iter()
        .map(|s| {
            let main = blockers
                .iter()
                .map(|(j, list)| (*j, super::freezone::covered(s, list)))
                .filter(|(_, c)| *c > 0.0)
                .max_by(|x, y| x.1.total_cmp(&y.1))
                .map(|(j, _)| El {
                    world: Arc::clone(world),
                    index: j,
                });
            Dynamic::from(Stretch {
                el: e.clone(),
                by: main,
                from: s[0] - frame.along[0],
                to: s[1] - frame.along[0],
                zones: zones.iter().map(|z| z.stretch(s[0], s[1])).collect(),
            })
        })
        .collect())
}

fn report(
    e: &El,
    other: Option<&El>,
    message: &str,
    zones: &[ZoneBox],
    length: f32,
    at: Option<f32>,
) {
    let world = &e.world;
    let a: FindingElement = finding_element(&world.models, e.prepared());
    let b = other.map_or_else(
        || a.clone(),
        |o| finding_element(&world.models, o.prepared()),
    );
    let corners: Vec<[Vec3; 8]> = zones.iter().map(ZoneBox::corners).collect();
    let region = if corners.is_empty() {
        let mut r = e.prepared().bounds;
        if let Some(o) = other {
            r = r.union(&o.prepared().bounds);
        }
        r
    } else {
        Aabb::from_points(corners.iter().flatten().copied())
    };
    let tag = match at {
        Some(at) => format!("{}@{at:.1}", b.global_id),
        None => format!("{}|{message}", b.global_id),
    };
    let finding = Finding {
        id: finding_id(&world.ruleset_id, &world.rule.id, &a.global_id, &tag),
        kind: FindingKind::Script,
        a,
        b,
        point: region.center(),
        region,
        magnitude: length,
        category: world.rule.name.clone(),
        details: message.lines().map(str::to_owned).collect(),
        zones: corners,
    };
    let mut findings = world.findings.lock().unwrap_or_else(|e| e.into_inner());
    // A script may report the same thing twice; keep one.
    if !findings.iter().any(|f| f.id == finding.id) {
        findings.push(finding);
    }
}
