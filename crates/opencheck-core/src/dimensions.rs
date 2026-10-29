//! Dimensions measured from the model itself, for elements whose file does
//! not state them. Revit, for one, writes an element's length, width and
//! height only when base quantities are exported, yet the geometry and the
//! material layers always carry them.
//!
//! Each element with geometry gets a quantity set [`SET_NAME`]: its length,
//! width and height (the smallest box around it, turned to lie along it in
//! plan), its thickness (the smallest of those) and, for layered walls,
//! slabs and roofs, the total thickness of its material layers. Values are
//! in the model's length unit, like the file's own properties.

use glam::Vec2;
use rustc_hash::FxHashMap;

use crate::IfcModel;
use crate::properties::{Property, PropertySet, PropertySetKind, PropertyValue};

/// Name of the quantity set with the measured dimensions.
pub const SET_NAME: &str = "Dimensions (measured by OpenCheck)";

/// Directions in plan tried for the box around an element, at most.
const MAX_DIRECTIONS: usize = 24;

/// Adds the measured dimensions to every element with geometry. `layers`:
/// total material layer thickness per element, in the model's unit.
pub(crate) fn add(model: &mut IfcModel, layers: &FxHashMap<u32, f64>) {
    let scale = model.length_unit_scale.max(1e-9);
    // Metres (the geometry) to the model's unit, to the millimetre.
    let unit = |metres: f32| ((metres as f64 * 1000.0).round() / 1000.0) / scale;
    let mut next_id = u32::MAX;
    for index in 0..model.elements.len() {
        let element = &model.elements[index];
        let layered = layers.get(&element.express_id).copied();
        // An assembly without geometry of its own (a curtain wall, a roof
        // of slabs) is measured by its parts.
        let meshes = if element.meshes.is_empty() {
            parts_meshes(model, element)
        } else {
            element.meshes.clone()
        };
        let measured = measure(model, &meshes);
        if measured.is_none() && layered.is_none() {
            continue;
        }
        let mut properties = Vec::new();
        let mut add = |name: &str, value: f64| {
            properties.push(Property {
                name: name.to_owned(),
                value: PropertyValue::Real(value),
                value_type: Some("IfcQuantityLength".to_owned()),
            });
        };
        if let Some(t) = layered {
            add("LayerThickness", t);
        }
        if let Some([length, width, height]) = measured {
            add("Thickness", unit(length.min(width).min(height)));
            add("Length", unit(length));
            add("Width", unit(width));
            add("Height", unit(height));
        }
        // Ids from the top down, clear of the file's own.
        while model.property_sets.contains_key(&next_id) {
            next_id -= 1;
        }
        model.property_sets.insert(
            next_id,
            PropertySet {
                express_id: next_id,
                name: SET_NAME.to_owned(),
                kind: PropertySetKind::Quantities,
                properties,
            },
        );
        model.elements[index].property_sets.push(next_id);
        next_id -= 1;
    }
}

/// The meshes of an element's parts, and of their parts.
fn parts_meshes(model: &IfcModel, element: &crate::Element) -> Vec<usize> {
    let mut out = Vec::new();
    let mut stack: Vec<u32> = element.parts.clone();
    let mut seen = 0;
    while let Some(id) = stack.pop() {
        seen += 1;
        if seen > 10_000 {
            break;
        }
        if let Some(part) = model.element(id) {
            out.extend(part.meshes.iter().copied());
            stack.extend(part.parts.iter().copied());
        }
    }
    out
}

/// Length, width (both in plan, length the longer) and height of the
/// smallest box around the meshes, turned about the vertical to fit; metres.
fn measure(model: &IfcModel, meshes: &[usize]) -> Option<[f32; 3]> {
    let points: Vec<[f32; 3]> = meshes
        .iter()
        .filter_map(|&m| model.meshes.get(m))
        .flat_map(|m| m.positions.iter().copied())
        .collect();
    if points.len() < 2 {
        return None;
    }
    let (zmin, zmax) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        (lo.min(p[2]), hi.max(p[2]))
    });

    // Directions to try: the long edges seen in plan, and the main axis of
    // the points.
    let mut edges: Vec<(f32, f32)> = Vec::new();
    for m in meshes.iter().filter_map(|&m| model.meshes.get(m)) {
        for t in m.indices.as_chunks::<3>().0 {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let (Some(a), Some(b)) = (m.positions.get(a as usize), m.positions.get(b as usize))
                else {
                    continue;
                };
                let d = Vec2::new(b[0] - a[0], b[1] - a[1]);
                let length = d.length();
                if length > 1e-3 {
                    // A box side and its perpendicular are the same box.
                    let angle = d.y.atan2(d.x).rem_euclid(std::f32::consts::FRAC_PI_2);
                    edges.push((length, angle));
                }
            }
        }
    }
    edges.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut angles: Vec<f32> = vec![principal_angle(&points)];
    for (_, angle) in edges {
        if angles.len() >= MAX_DIRECTIONS {
            break;
        }
        if !angles.iter().any(|a| (a - angle).abs() < 0.002) {
            angles.push(angle);
        }
    }

    let mut best: Option<(f32, f32, f32)> = None;
    for angle in angles {
        let (u, v) = (
            Vec2::new(angle.cos(), angle.sin()),
            Vec2::new(-angle.sin(), angle.cos()),
        );
        let (mut umin, mut umax, mut vmin, mut vmax) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for p in &points {
            let q = Vec2::new(p[0], p[1]);
            let (a, b) = (q.dot(u), q.dot(v));
            umin = umin.min(a);
            umax = umax.max(a);
            vmin = vmin.min(b);
            vmax = vmax.max(b);
        }
        let (eu, ev) = (umax - umin, vmax - vmin);
        if best.is_none_or(|(area, _, _)| eu * ev < area) {
            best = Some((eu * ev, eu.max(ev), eu.min(ev)));
        }
    }
    let (_, length, width) = best?;
    Some([length, width, zmax - zmin])
}

/// The direction in plan the points spread most along, radians.
fn principal_angle(points: &[[f32; 3]]) -> f32 {
    let n = points.len() as f32;
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(x, y), p| (x + p[0], y + p[1]));
    let (mx, my) = (mx / n, my / n);
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for p in points {
        let (dx, dy) = (p[0] - mx, p[1] - my);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    (0.5 * (2.0 * sxy).atan2(sxx - syy)).rem_euclid(std::f32::consts::FRAC_PI_2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A box's corners, turned by `angle` in plan.
    fn turned_box(length: f32, width: f32, height: f32, angle: f32) -> Vec<[f32; 3]> {
        let (c, s) = (angle.cos(), angle.sin());
        let mut out = Vec::new();
        for x in [0.0, length] {
            for y in [0.0, width] {
                for z in [0.0, height] {
                    out.push([x * c - y * s + 10.0, x * s + y * c - 4.0, z]);
                }
            }
        }
        out
    }

    /// A model of one element: a box mesh, turned in plan, in a model
    /// whose unit is the millimetre.
    fn wall_model(angle: f32) -> IfcModel {
        let corners = turned_box(5.0, 0.3, 3.0, angle);
        // The 12 edges of the box as degenerate triangles: enough for the
        // directions.
        let mut indices = Vec::new();
        for (a, b) in [(0, 4), (4, 6), (6, 2), (2, 0), (0, 1)] {
            indices.extend([a, b, b]);
        }
        let mesh = crate::ElementMesh {
            express_id: 10,
            positions: corners,
            normals: Vec::new(),
            indices,
            color: [1.0; 4],
            bounds: crate::Aabb::EMPTY,
        };
        let mut wall = crate::Element {
            express_id: 10,
            global_id: "w".to_owned(),
            ifc_class: "IfcWall".to_owned(),
            name: "Wall".to_owned(),
            description: None,
            object_type: None,
            tag: None,
            predefined_type: None,
            container: None,
            parent: None,
            parts: Vec::new(),
            type_object: None,
            type_class: None,
            attributes: Vec::new(),
            classifications: Vec::new(),
            materials: Vec::new(),
            systems: Vec::new(),
            property_sets: Vec::new(),
            meshes: vec![0],
            bounds: crate::Aabb::EMPTY,
        };
        wall.meshes = vec![0];
        IfcModel::new(
            None,
            "test".to_owned(),
            crate::Discipline::Architectural,
            "IFC4".to_owned(),
            None,
            None,
            0,
            0.001,
            None,
            vec![wall],
            FxHashMap::default(),
            vec![mesh],
            glam::DMat4::IDENTITY,
            crate::LoadStats::default(),
        )
    }

    #[test]
    fn measures_a_turned_wall_in_the_models_unit() {
        let mut model = wall_model(0.6);
        let layers = FxHashMap::from_iter([(10, 300.0)]);
        add(&mut model, &layers);
        let wall = &model.elements[0];
        let set = model.property_sets_of(wall).next().expect("measured");
        assert_eq!(set.name, SET_NAME);
        let value = |name: &str| {
            set.properties
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.value.clone())
        };
        assert_eq!(value("LayerThickness"), Some(PropertyValue::Real(300.0)));
        assert_eq!(value("Thickness"), Some(PropertyValue::Real(300.0)));
        assert_eq!(value("Length"), Some(PropertyValue::Real(5000.0)));
        assert_eq!(value("Width"), Some(PropertyValue::Real(300.0)));
        assert_eq!(value("Height"), Some(PropertyValue::Real(3000.0)));
    }

    #[test]
    fn principal_axis_of_a_turned_wall() {
        let points = turned_box(5.0, 0.3, 3.0, 0.4);
        let angle = principal_angle(&points);
        assert!((angle - 0.4).abs() < 0.01, "{angle}");
    }
}
