//! OpenCheck's own models as the models of a coordination file.
//!
//! The writer ([`super::write_smc`]) takes plain models and knows nothing of
//! OpenCheck. This is the step before it: a loaded IFC model with its
//! storeys, elements, property sets and meshes becomes a [`WriteModel`]. The
//! shapes are taken in the frame all models share, so the models lie against
//! each other in the file as they do in OpenCheck.

use std::collections::HashMap;

use opencheck_core::{Discipline, IfcModel, PropertyValue};

use super::{
    WriteDiscipline, WriteElement, WriteModel, WriteProperty, WritePropertySet, WriteShape,
    WriteStorey, WriteValue,
};

/// A model to put in the file, with what the user calls it.
pub struct NamedModel<'a> {
    pub model: &'a IfcModel,
    /// The model's name; the file's own when the user gave none.
    pub name: String,
    pub discipline: Discipline,
}

impl<'a> NamedModel<'a> {
    /// A model under its own name and discipline.
    pub fn new(model: &'a IfcModel) -> Self {
        NamedModel {
            model,
            name: model.short_name.clone(),
            discipline: model.discipline,
        }
    }
}

/// The models as the writer takes them, in the order given. Each gets the
/// short name of its discipline (`A`, `K`, `V` ...); a second model of a
/// discipline gets a number after it (`V2`).
pub fn models_to_write<'a>(models: impl IntoIterator<Item = NamedModel<'a>>) -> Vec<WriteModel> {
    let mut used: HashMap<&'static str, usize> = HashMap::new();
    models
        .into_iter()
        .map(|named| {
            let code = short_name(named.discipline);
            let n = used.entry(code).or_default();
            *n += 1;
            let short = if *n > 1 && !code.is_empty() {
                format!("{code}{n}")
            } else {
                code.to_string()
            };
            model_to_write(named.model, named.name, short, named.discipline)
        })
        .collect()
}

/// The short name a coordinator gives a model of the discipline.
pub fn short_name(discipline: Discipline) -> &'static str {
    match discipline {
        Discipline::Architectural => "A",
        Discipline::Structural => "K",
        Discipline::Mechanical => "V",
        Discipline::Plumbing => "R",
        Discipline::Electrical => "E",
        Discipline::FireProtection => "B",
        Discipline::Landscape => "L",
        Discipline::Infrastructure => "I",
        Discipline::Unknown => "",
    }
}

/// The discipline a model is filed under in the file.
pub fn discipline(discipline: Discipline) -> WriteDiscipline {
    match discipline {
        Discipline::Architectural => WriteDiscipline::Architectural,
        Discipline::Structural => WriteDiscipline::Structural,
        Discipline::Mechanical => WriteDiscipline::Ventilation,
        Discipline::Plumbing => WriteDiscipline::Plumbing,
        Discipline::Electrical => WriteDiscipline::Electrical,
        Discipline::FireProtection => WriteDiscipline::Sprinkler,
        Discipline::Landscape => WriteDiscipline::Landscape,
        // the list has roads, bridges, railways and ports, and no one
        // discipline for them all
        Discipline::Infrastructure => WriteDiscipline::Road,
        // a model has to be filed under one
        Discipline::Unknown => WriteDiscipline::Architectural,
    }
}

fn model_to_write(
    model: &IfcModel,
    name: String,
    short_name: String,
    of: Discipline,
) -> WriteModel {
    // the storeys from the lowest up, and which storey each spatial node is on
    let mut storeys = Vec::new();
    let mut storey_of: HashMap<u32, usize> = HashMap::new();
    for node in model.storeys() {
        let at = storeys.len();
        node.walk(&mut |inner, _| {
            storey_of.insert(inner.express_id, at);
        });
        let elevation = node.elevation.or(node.placement_z).unwrap_or(0.0);
        storeys.push(WriteStorey {
            // the loader does not keep the GlobalId of a storey: it gets a new one
            guid: String::new(),
            name: node.name.clone(),
            elevation: elevation * model.length_unit_scale,
        });
    }

    let mut elements = Vec::new();
    for e in &model.elements {
        let mut shape = WriteShape::default();
        let mut largest = 0;
        for &k in &e.meshes {
            let Some(mesh) = model.meshes.get(k) else {
                continue;
            };
            let base = shape.positions.len() as u32;
            shape.positions.extend(mesh.positions.iter().map(|p| {
                model
                    .local_to_world
                    .transform_point3([p[0] as f64, p[1] as f64, p[2] as f64].into())
                    .to_array()
            }));
            for t in mesh.indices.as_chunks::<3>().0 {
                shape
                    .triangles
                    .push([base + t[0], base + t[1], base + t[2]]);
                shape.triangle_colors.push(mesh.color);
            }
            if mesh.triangle_count() > largest {
                largest = mesh.triangle_count();
                shape.color = mesh.color;
            }
        }
        let property_sets = model
            .property_sets_of(e)
            .map(|set| WritePropertySet {
                name: set.name.clone(),
                properties: set
                    .properties
                    .iter()
                    .filter_map(|p| {
                        Some(WriteProperty {
                            name: p.name.clone(),
                            value: value(&p.value)?,
                            value_type: p.value_type.clone(),
                        })
                    })
                    .collect(),
            })
            .filter(|set: &WritePropertySet| !set.properties.is_empty())
            .collect();
        elements.push(WriteElement {
            guid: e.global_id.clone(),
            ifc_class: e.ifc_class.clone(),
            name: e.name.clone(),
            description: e.description.clone(),
            object_type: e.object_type.clone(),
            predefined_type: e.predefined_type.clone(),
            tag: e.tag.clone(),
            storey: e.container.and_then(|c| storey_of.get(&c).copied()),
            property_sets,
            shape: (!shape.triangles.is_empty()).then_some(shape),
        });
    }

    WriteModel {
        name,
        short_name,
        discipline: discipline(of),
        application: model.authoring_application.clone().unwrap_or_default(),
        project: model.project_name.clone().unwrap_or_default(),
        storeys,
        elements,
    }
}

fn value(v: &PropertyValue) -> Option<WriteValue> {
    Some(match v {
        PropertyValue::Text(t) => WriteValue::Text(t.clone()),
        PropertyValue::Integer(n) => WriteValue::Integer(*n),
        PropertyValue::Real(n) => WriteValue::Number(*n),
        PropertyValue::Boolean(b) => WriteValue::Bool(*b),
        PropertyValue::Logical(Some(b)) => WriteValue::Bool(*b),
        PropertyValue::Logical(None) | PropertyValue::Null => return None,
        other => WriteValue::Text(other.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_discipline_has_a_place_in_the_file() {
        assert_eq!(discipline(Discipline::Electrical).label(), "Electrical");
        assert_eq!(discipline(Discipline::Plumbing).label(), "Plumbing");
        assert_eq!(discipline(Discipline::Mechanical).label(), "Ventilation");
        assert_eq!(discipline(Discipline::FireProtection).label(), "Sprinkler");
        for d in Discipline::ALL {
            // no panic, and a short name of at most one letter
            assert!(short_name(d).len() <= 1);
            let _ = discipline(d);
        }
    }
}
