use std::sync::Arc;

use glam::{DMat4, Mat4, Vec3};
use rustc_hash::FxHashMap;

use super::*;
use crate::check::{CheckJob, Ruleset, run_check};
use crate::{
    ClassificationRef, Discipline, ElementMesh, LoadStats, ModelId, Property, PropertySet,
    PropertySetKind, SpatialNode,
};

fn element(id: u32, class: &str, name: &str) -> Element {
    Element {
        express_id: id,
        global_id: format!("guid{id}"),
        ifc_class: class.to_owned(),
        name: name.to_owned(),
        description: None,
        object_type: None,
        tag: None,
        predefined_type: None,
        container: None,
        parent: None,
        parts: Vec::new(),
        type_object: None,
        type_class: None,
        attributes: vec![("Name".to_owned(), PropertyValue::Text(name.to_owned()))],
        classifications: Vec::new(),
        materials: Vec::new(),
        systems: Vec::new(),
        property_sets: Vec::new(),
        meshes: Vec::new(),
        bounds: Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        },
    }
}

fn property(name: &str, value: PropertyValue, value_type: &str) -> Property {
    Property {
        name: name.to_owned(),
        value,
        value_type: Some(value_type.to_owned()),
    }
}

/// A wall with properties, classification and material in a storey; a
/// slab with nothing; a member that is part of the wall; and a pipe in a
/// system.
fn model() -> IfcModel {
    model_with(Vec::new())
}

/// The test model with extra elements.
fn model_with(extra: Vec<Element>) -> IfcModel {
    let mut wall = element(10, "IfcWall", "Wall 1");
    wall.predefined_type = Some("SOLIDWALL".to_owned());
    wall.container = Some(2);
    wall.parts = vec![13];
    wall.property_sets = vec![100, 101];
    wall.classifications = vec![ClassificationRef {
        system: "CCS".to_owned(),
        code: "L4.ABC".to_owned(),
        name: String::new(),
    }];
    wall.materials = vec!["Concrete C30".to_owned()];
    wall.attributes.push((
        "ObjectType".to_owned(),
        PropertyValue::Text("Basic wall".to_owned()),
    ));
    let mut slab = element(11, "IfcSlab", "");
    slab.attributes.clear();
    slab.container = Some(2);
    let mut segment = element(12, "IfcFlowSegment", "Pipe");
    segment.type_class = Some("IfcPipeSegmentType".to_owned());
    segment.systems = vec!["Heating".to_owned()];
    let mut member = element(13, "IfcMember", "Stud");
    member.parent = Some(10);
    member.bounds = Aabb::EMPTY;

    let mut property_sets = FxHashMap::default();
    property_sets.insert(
        100,
        PropertySet {
            express_id: 100,
            name: "Pset_WallCommon".to_owned(),
            kind: PropertySetKind::Properties,
            properties: vec![
                property("IsExternal", PropertyValue::Boolean(true), "IfcBoolean"),
                property(
                    "FireRating",
                    PropertyValue::Text("EI60".to_owned()),
                    "IfcLabel",
                ),
                property(
                    "Reference",
                    PropertyValue::Text(String::new()),
                    "IfcIdentifier",
                ),
            ],
        },
    );
    property_sets.insert(
        101,
        PropertySet {
            express_id: 101,
            name: "Qto_WallBaseQuantities".to_owned(),
            kind: PropertySetKind::Quantities,
            properties: vec![property(
                "Width",
                PropertyValue::Real(200.0),
                "IfcQuantityLength",
            )],
        },
    );
    let storey = SpatialNode {
        express_id: 2,
        ifc_class: "IfcBuildingStorey".to_owned(),
        name: "Level 1".to_owned(),
        long_name: None,
        elevation: Some(0.0),
        placement_z: None,
        children: Vec::new(),
        elements: vec![10, 11],
    };
    let root = SpatialNode {
        express_id: 1,
        ifc_class: "IfcProject".to_owned(),
        name: "P".to_owned(),
        long_name: None,
        elevation: None,
        placement_z: None,
        children: vec![storey],
        elements: Vec::new(),
    };
    IfcModel::new(
        None,
        "test".to_owned(),
        Discipline::Architectural,
        "IFC4".to_owned(),
        None,
        None,
        0,
        0.001,
        Some(root),
        [vec![wall, slab, segment, member], extra].concat(),
        property_sets,
        Vec::<ElementMesh>::new(),
        DMat4::IDENTITY,
        LoadStats::default(),
    )
}

fn ids(specifications: &str) -> Ids {
    let xml = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ids:ids xmlns:ids="http://standards.buildingsmart.org/IDS" xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <ids:info><ids:title>Test &amp; check</ids:title></ids:info>
  <ids:specifications>{specifications}</ids:specifications>
</ids:ids>"#
    );
    Ids::from_xml(&xml).unwrap_or_else(|e| panic!("{e}"))
}

fn spec(applicability: &str, requirements: &str) -> String {
    format!(
        r#"<ids:specification name="S" ifcVersion="IFC2X3 IFC4">
  <ids:applicability minOccurs="0" maxOccurs="unbounded">{applicability}</ids:applicability>
  <ids:requirements>{requirements}</ids:requirements>
</ids:specification>"#
    )
}

fn entity(name: &str) -> String {
    format!(
        "<ids:entity><ids:name><ids:simpleValue>{name}</ids:simpleValue></ids:name></ids:entity>"
    )
}

fn simple(tag: &str, value: &str) -> String {
    format!("<ids:{tag}><ids:simpleValue>{value}</ids:simpleValue></ids:{tag}>")
}

/// Failures of one requirement on the element with this express ID, or
/// "n/a" when the specification does not apply.
fn outcome(applicability: &str, requirement: &str, express_id: u32) -> String {
    let ids = ids(&spec(applicability, requirement));
    let model = model();
    let ctx = IdsContext::new(&model);
    let element = model.element(express_id).unwrap();
    let spec = &ids.specifications[0];
    if !spec.applies(&ctx, element) {
        return "n/a".to_owned();
    }
    match spec.check(&ctx, element).as_slice() {
        [None] => "pass".to_owned(),
        [Some(f)] => format!("{f:?}"),
        other => format!("{other:?}"),
    }
}

#[test]
fn reads_info_specifications_and_restrictions() {
    let ids = ids(&format!(
        r#"{}
<ids:specification name="Doors" identifier="D1" ifcVersion="IFC4" description="d">
  <ids:applicability minOccurs="1" maxOccurs="unbounded">{}</ids:applicability>
  <ids:requirements description="r">
    <ids:property dataType="IFCLABEL" cardinality="optional" instructions="i">
      {}{}
      <ids:value><xs:restriction base="xs:string"><xs:enumeration value="A"/><xs:enumeration value="B"/></xs:restriction></ids:value>
    </ids:property>
    <ids:attribute cardinality="prohibited">{}</ids:attribute>
  </ids:requirements>
</ids:specification>"#,
        spec(&entity("IFCWALL"), ""),
        entity("IFCDOOR"),
        simple("propertySet", "Pset_DoorCommon"),
        simple("baseName", "FireRating"),
        simple("name", "Description"),
    ));
    assert_eq!(ids.info.title, "Test & check");
    assert_eq!(ids.specifications.len(), 2);
    let doors = &ids.specifications[1];
    assert_eq!(doors.identifier, "D1");
    assert_eq!(doors.ifc_versions, vec!["IFC4"]);
    assert_eq!(doors.min_occurs, 1);
    assert_eq!(doors.max_occurs, None);
    assert_eq!(doors.requirements.len(), 2);
    assert_eq!(doors.requirements[0].cardinality, Cardinality::Optional);
    assert_eq!(doors.requirements[1].cardinality, Cardinality::Prohibited);
    match &doors.requirements[0].kind {
        FacetKind::Property {
            data_type, value, ..
        } => {
            assert_eq!(data_type.as_deref(), Some("IFCLABEL"));
            assert_eq!(value.as_ref().unwrap().to_string(), "A | B");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        doors.requirements[0].label(),
        "Pset_DoorCommon.FireRating = A | B"
    );
}

#[test]
fn rejects_broken_files() {
    assert!(matches!(Ids::from_xml("<ids"), Err(IdsError::Xml(_))));
    assert!(Ids::from_xml("<other/>").is_err());
    let no_specs = "<ids xmlns=\"http://standards.buildingsmart.org/IDS\"><info><title>x</title></info><specifications/></ids>";
    assert!(Ids::from_xml(no_specs).is_err());
    let bad_pattern = format!(
        "<ids><specifications>{}</specifications></ids>",
        spec(
            r#"<entity><name><restriction base="xs:string"><pattern value="("/></restriction></name></entity>"#,
            ""
        )
    );
    assert!(Ids::from_xml(&bad_pattern).is_err());
    let bad_cardinality = format!(
        "<ids><specifications>{}</specifications></ids>",
        spec(&entity("IFCWALL"), r#"<material cardinality="sometimes"/>"#)
    );
    assert!(Ids::from_xml(&bad_cardinality).is_err());
}

#[test]
fn entity_names_and_predefined_types() {
    let wall = entity("IFCWALL");
    assert_eq!(outcome(&wall, "", 10), "[]");
    assert_eq!(outcome(&wall, "", 11), "n/a");
    // Exact class, no subtypes, in upper case as IDS writes it.
    assert_eq!(outcome(&entity("IfcWall"), "", 10), "n/a");
    assert_eq!(outcome(&entity("IFCBUILDINGELEMENT"), "", 10), "n/a");
    let typed = "<ids:entity><ids:name><ids:simpleValue>IFCWALL</ids:simpleValue></ids:name>\
        <ids:predefinedType><ids:simpleValue>SOLIDWALL</ids:simpleValue></ids:predefinedType></ids:entity>";
    assert_eq!(outcome(typed, "", 10), "[]");
    let other = typed.replace("SOLIDWALL", "PARTITIONING");
    assert_eq!(outcome(&other, "", 10), "n/a");
    // Predefined types are case-sensitive.
    let lower = typed.replace("SOLIDWALL", "solidwall");
    assert_eq!(outcome(&lower, "", 10), "n/a");
    // IFC2x3 generic segments take their class from the type.
    assert_eq!(outcome(&entity("IFCPIPESEGMENT"), "", 12), "[]");
    let pattern = r#"<ids:entity><ids:name><xs:restriction base="xs:string"><xs:pattern value="IFC(WALL|SLAB)"/></xs:restriction></ids:name></ids:entity>"#;
    assert_eq!(outcome(pattern, "", 11), "[]");
    // The entity facet as a requirement.
    let named = format!("<ids:attribute>{}</ids:attribute>", simple("name", "Name"));
    assert_eq!(outcome(&named, &entity("IFCSLAB"), 10), "WrongValue");
}

#[test]
fn attributes() {
    let wall = entity("IFCWALL");
    let name = format!("<ids:attribute>{}</ids:attribute>", simple("name", "Name"));
    assert_eq!(outcome(&wall, &name, 10), "pass");
    assert_eq!(outcome(&entity("IFCSLAB"), &name, 11), "Missing");
    let value = format!(
        "<ids:attribute>{}{}</ids:attribute>",
        simple("name", "Name"),
        simple("value", "Wall 2")
    );
    assert_eq!(outcome(&wall, &value, 10), "WrongValue");
    let optional = value.replace(
        "<ids:attribute>",
        "<ids:attribute cardinality=\"optional\">",
    );
    assert_eq!(outcome(&wall, &optional, 10), "WrongValue");
    assert_eq!(outcome(&entity("IFCSLAB"), &optional, 11), "pass");
    let prohibited = name.replace(
        "<ids:attribute>",
        "<ids:attribute cardinality=\"prohibited\">",
    );
    assert_eq!(outcome(&wall, &prohibited, 10), "Prohibited");
    assert_eq!(outcome(&entity("IFCSLAB"), &prohibited, 11), "pass");
    let global_id = format!(
        "<ids:attribute>{}</ids:attribute>",
        simple("name", "GlobalId")
    );
    assert_eq!(outcome(&wall, &global_id, 10), "pass");
    // Attributes also select elements.
    assert_eq!(outcome(&name, "", 11), "n/a");
    assert_eq!(outcome(&name, "", 10), "[]");
}

#[test]
fn properties_types_values_and_units() {
    let wall = entity("IFCWALL");
    let prop = |set: &str, name: &str, extra: &str, attrs: &str| {
        format!(
            "<ids:property{attrs}>{}{}{extra}</ids:property>",
            simple("propertySet", set),
            simple("baseName", name)
        )
    };
    assert_eq!(
        outcome(&wall, &prop("Pset_WallCommon", "FireRating", "", ""), 10),
        "pass"
    );
    assert_eq!(
        outcome(&wall, &prop("Pset_WallCommon", "Acoustic", "", ""), 10),
        "Missing"
    );
    // Empty strings count as missing.
    assert_eq!(
        outcome(&wall, &prop("Pset_WallCommon", "Reference", "", ""), 10),
        "Missing"
    );
    // Names are case-sensitive.
    assert_eq!(
        outcome(&wall, &prop("Pset_WallCommon", "firerating", "", ""), 10),
        "Missing"
    );
    let typed = prop(
        "Pset_WallCommon",
        "FireRating",
        "",
        " dataType=\"IFCLABEL\"",
    );
    assert_eq!(outcome(&wall, &typed, 10), "pass");
    let wrong_type = prop("Pset_WallCommon", "FireRating", "", " dataType=\"IFCTEXT\"");
    assert_eq!(outcome(&wall, &wrong_type, 10), "WrongValue");
    let bool_value = prop(
        "Pset_WallCommon",
        "IsExternal",
        &simple("value", "true"),
        "",
    );
    assert_eq!(outcome(&wall, &bool_value, 10), "pass");
    // Booleans are lower case in IDS.
    let upper_bool = prop(
        "Pset_WallCommon",
        "IsExternal",
        &simple("value", "TRUE"),
        "",
    );
    assert_eq!(outcome(&wall, &upper_bool, 10), "WrongValue");
    let false_value = prop(
        "Pset_WallCommon",
        "IsExternal",
        &simple("value", "false"),
        "",
    );
    assert_eq!(outcome(&wall, &false_value, 10), "WrongValue");
    // 200 mm in a millimetre model is 0.2 m in IDS.
    let width = prop(
        "Qto_WallBaseQuantities",
        "Width",
        &simple("value", "0.2"),
        " dataType=\"IFCLENGTHMEASURE\"",
    );
    assert_eq!(outcome(&wall, &width, 10), "pass");
    let bounds = prop(
        "Qto_WallBaseQuantities",
        "Width",
        r#"<ids:value><xs:restriction base="xs:double"><xs:minInclusive value="0.25"/></xs:restriction></ids:value>"#,
        "",
    );
    assert_eq!(outcome(&wall, &bounds, 10), "WrongValue");
    let pset_pattern = r#"<ids:property><ids:propertySet><xs:restriction base="xs:string"><xs:pattern value="Pset_.*"/></xs:restriction></ids:propertySet><ids:baseName><ids:simpleValue>FireRating</ids:simpleValue></ids:baseName>
        <ids:value><xs:restriction base="xs:string"><xs:pattern value="EI\d+"/><xs:maxLength value="4"/></xs:restriction></ids:value></ids:property>"#;
    assert_eq!(outcome(&wall, pset_pattern, 10), "pass");
    let prohibited = prop(
        "Pset_WallCommon",
        "FireRating",
        &simple("value", "EI60"),
        " cardinality=\"prohibited\"",
    );
    assert_eq!(outcome(&wall, &prohibited, 10), "Prohibited");
    let prohibited_other = prop(
        "Pset_WallCommon",
        "FireRating",
        &simple("value", "EI30"),
        " cardinality=\"prohibited\"",
    );
    assert_eq!(outcome(&wall, &prohibited_other, 10), "pass");

    // Every property a pattern matches must meet the requirement:
    // IsExternal is not EI60.
    let both = r#"<ids:property><ids:propertySet><ids:simpleValue>Pset_WallCommon</ids:simpleValue></ids:propertySet><ids:baseName><xs:restriction base="xs:string"><xs:pattern value="FireRating|IsExternal"/></xs:restriction></ids:baseName>
        <ids:value><ids:simpleValue>EI60</ids:simpleValue></ids:value></ids:property>"#;
    assert_eq!(outcome(&wall, both, 10), "WrongValue");
    // Every set a pattern matches must have the property: the quantity set
    // has no FireRating.
    let sets = |pattern: &str| {
        format!(
            r#"<ids:property><ids:propertySet><xs:restriction base="xs:string"><xs:pattern value="{pattern}"/></xs:restriction></ids:propertySet>{}</ids:property>"#,
            simple("baseName", "FireRating")
        )
    };
    assert_eq!(
        outcome(&wall, &sets("Pset_WallCommon|Qto_WallBaseQuantities"), 10),
        "WrongValue"
    );
    // `.*` is no exception: it matches the quantity set too.
    assert_eq!(outcome(&wall, &sets(".*"), 10), "WrongValue");
    assert_eq!(outcome(&wall, &sets("Pset_.*"), 10), "pass");
}

#[test]
fn values_as_the_buildingsmart_test_cases_compare_them() {
    let simple_value = |v: &str| IdsValue::Simple(v.to_owned());
    // Reals match within 1e-6 + 1e-6 * |expected|.
    let one = simple_value("1.");
    assert!(one.matches_value(&PropertyValue::Real(0.999998), 1.0));
    assert!(one.matches_value(&PropertyValue::Real(1.000002), 1.0));
    assert!(!one.matches_value(&PropertyValue::Real(0.9999979), 1.0));
    let zero = simple_value("0.");
    assert!(zero.matches_value(&PropertyValue::Real(-0.000001), 1.0));
    assert!(!zero.matches_value(&PropertyValue::Real(0.0000011), 1.0));
    let million = simple_value("-1000000.");
    assert!(million.matches_value(&PropertyValue::Real(-1000001.000001), 1.0));
    assert!(!million.matches_value(&PropertyValue::Real(-1000001.0000011), 1.0));
    // Bounds are exact.
    let positive = IdsValue::Restriction(Restriction {
        base: "xs:double".to_owned(),
        min_inclusive: Some(0.0),
        ..Restriction::default()
    });
    assert!(positive.matches_value(&PropertyValue::Real(0.0), 1.0));
    assert!(!positive.matches_value(&PropertyValue::Real(-0.0000001), 1.0));
    // Integers are never written with a decimal point.
    assert!(simple_value("42").matches_value(&PropertyValue::Integer(42), 1.0));
    assert!(!simple_value("42.").matches_value(&PropertyValue::Integer(42), 1.0));
    assert!(!simple_value("42.0").matches_value(&PropertyValue::Integer(42), 1.0));
    assert!(simple_value("42.0").matches_value(&PropertyValue::Real(42.0), 1.0));
    // Booleans are lower case.
    assert!(simple_value("true").matches_value(&PropertyValue::Boolean(true), 1.0));
    assert!(!simple_value("TRUE").matches_value(&PropertyValue::Boolean(true), 1.0));
    // Patterns only match texts.
    let any = IdsValue::Restriction(Restriction::pattern(".*").unwrap());
    assert!(any.matches_value(&PropertyValue::Text("42".to_owned()), 1.0));
    assert!(!any.matches_value(&PropertyValue::Real(42.0), 1.0));
}

#[test]
fn classifications_and_materials() {
    let wall = entity("IFCWALL");
    assert_eq!(outcome(&wall, "<ids:classification/>", 10), "pass");
    assert_eq!(
        outcome(&entity("IFCSLAB"), "<ids:classification/>", 11),
        "Missing"
    );
    let ccs = format!(
        "<ids:classification>{}</ids:classification>",
        simple("system", "CCS")
    );
    assert_eq!(outcome(&wall, &ccs, 10), "pass");
    let uniclass = format!(
        "<ids:classification>{}</ids:classification>",
        simple("system", "Uniclass")
    );
    assert_eq!(outcome(&wall, &uniclass, 10), "WrongValue");
    let code = r#"<ids:classification><ids:value><xs:restriction base="xs:string"><xs:pattern value="L4\..*"/></xs:restriction></ids:value></ids:classification>"#;
    assert_eq!(outcome(&wall, code, 10), "pass");
    assert_eq!(outcome(&wall, "<ids:material/>", 10), "pass");
    let concrete = r#"<ids:material><ids:value><xs:restriction base="xs:string"><xs:pattern value="Concrete.*"/></xs:restriction></ids:value></ids:material>"#;
    assert_eq!(outcome(&wall, concrete, 10), "pass");
    assert_eq!(
        outcome(
            &wall,
            &format!("<ids:material>{}</ids:material>", simple("value", "Steel")),
            10
        ),
        "WrongValue"
    );
}

#[test]
fn part_of_relations() {
    let part_of = |relation: &str, class: &str| {
        format!(
            "<ids:partOf relation=\"{relation}\">{}</ids:partOf>",
            entity(class)
        )
    };
    let storey = part_of("IFCRELCONTAINEDINSPATIALSTRUCTURE", "IFCBUILDINGSTOREY");
    assert_eq!(outcome(&entity("IFCWALL"), &storey, 10), "pass");
    // Parts are contained through their assembly.
    assert_eq!(outcome(&entity("IFCMEMBER"), &storey, 13), "pass");
    assert_eq!(outcome(&entity("IFCPIPESEGMENT"), &storey, 12), "Missing");
    let site = part_of("IFCRELCONTAINEDINSPATIALSTRUCTURE", "IFCSITE");
    assert_eq!(outcome(&entity("IFCWALL"), &site, 10), "WrongValue");
    let in_wall = part_of("IFCRELAGGREGATES", "IFCWALL");
    assert_eq!(outcome(&entity("IFCMEMBER"), &in_wall, 13), "pass");
    assert_eq!(outcome(&entity("IFCWALL"), &in_wall, 10), "Missing");
    let system = part_of("IFCRELASSIGNSTOGROUP", "IFCDISTRIBUTIONSYSTEM");
    assert_eq!(outcome(&entity("IFCPIPESEGMENT"), &system, 12), "pass");
    assert_eq!(outcome(&entity("IFCWALL"), &system, 10), "Missing");
}

#[test]
fn part_of_spaces_and_the_project() {
    // A space aggregated into the storey, a beam aggregated into the space,
    // and a column directly under the project.
    let mut space = element(14, "IfcSpace", "Room");
    space.container = Some(2);
    let mut beam = element(15, "IfcBeam", "Beam");
    beam.parent = Some(14);
    let mut column = element(16, "IfcColumn", "Column");
    column.container = Some(1);
    let model = model_with(vec![space, beam, column]);
    let ctx = IdsContext::new(&model);
    let check = |relation: &str, class: &str, express_id: u32| {
        let ids = ids(&spec(
            r#"<ids:entity><ids:name><xs:restriction base="xs:string"><xs:pattern value="IFC.*"/></xs:restriction></ids:name></ids:entity>"#,
            &format!(
                "<ids:partOf relation=\"{relation}\">{}</ids:partOf>",
                entity(class)
            ),
        ));
        ids.specifications[0].check(&ctx, model.element(express_id).unwrap())[0]
    };
    // The spatial structure is an aggregation: the beam is part of the
    // storey and the project through its space.
    assert_eq!(check("IFCRELAGGREGATES", "IFCSPACE", 15), None);
    assert_eq!(check("IFCRELAGGREGATES", "IFCBUILDINGSTOREY", 15), None);
    assert_eq!(check("IFCRELAGGREGATES", "IFCPROJECT", 14), None);
    // The project never contains elements.
    assert_eq!(
        check("IFCRELCONTAINEDINSPATIALSTRUCTURE", "IFCPROJECT", 16),
        Some(Failure::Missing)
    );
    assert_eq!(
        check("IFCRELCONTAINEDINSPATIALSTRUCTURE", "IFCPROJECT", 10),
        Some(Failure::WrongValue)
    );
}

#[test]
fn round_trips_through_xml() {
    let original = ids(&spec(
        &entity("IFCWALL"),
        r#"<ids:property dataType="IFCLABEL" cardinality="optional"><ids:propertySet><ids:simpleValue>P &lt;1&gt;</ids:simpleValue></ids:propertySet><ids:baseName><ids:simpleValue>N</ids:simpleValue></ids:baseName>
           <ids:value><xs:restriction base="xs:double"><xs:minInclusive value="1.5"/><xs:maxExclusive value="3"/></xs:restriction></ids:value></ids:property>
           <ids:partOf relation="IFCRELAGGREGATES"><ids:entity><ids:name><ids:simpleValue>IFCWALL</ids:simpleValue></ids:name></ids:entity></ids:partOf>
           <ids:classification cardinality="prohibited"/>"#,
    ));
    let xml = original.to_xml();
    let again = Ids::from_xml(&xml).unwrap_or_else(|e| panic!("{e}\n{xml}"));
    assert_eq!(again.info, original.info);
    let (a, b) = (&again.specifications[0], &original.specifications[0]);
    assert_eq!(a.min_occurs, 0);
    let labels = |s: &Specification| -> Vec<(String, Cardinality)> {
        s.requirements
            .iter()
            .map(|f| (f.label(), f.cardinality))
            .collect()
    };
    assert_eq!(labels(a), labels(b));
    assert_eq!(labels(a)[0].0, "P <1>.N = ≥ 1.5, < 3");
}

#[test]
fn engine_reports_findings_and_fill_rate() {
    let text = ids(&format!(
        r#"{}
<ids:specification name="Slabs" ifcVersion="IFC4">
  <ids:applicability minOccurs="0">{}</ids:applicability>
  <ids:requirements><ids:attribute>{}</ids:attribute><ids:classification/></ids:requirements>
</ids:specification>
<ids:specification name="Roofs" ifcVersion="IFC4">
  <ids:applicability minOccurs="1">{}</ids:applicability>
</ids:specification>
<ids:specification name="Old" ifcVersion="IFC2X3">
  <ids:applicability>{}</ids:applicability>
</ids:specification>
<ids:specification name="No pipes" ifcVersion="IFC4">
  <ids:applicability minOccurs="0" maxOccurs="0">{}</ids:applicability>
</ids:specification>"#,
        spec(
            &entity("IFCWALL"),
            &format!(
                "<ids:attribute>{}</ids:attribute><ids:material/>",
                simple("name", "Name")
            )
        ),
        entity("IFCSLAB"),
        simple("name", "Name"),
        entity("IFCROOF"),
        entity("IFCWALL"),
        entity("IFCPIPESEGMENT"),
    ))
    .to_xml();
    let ruleset = Ruleset::from_toml(
        r#"
        [ruleset]
        id = "ids-test"
        name = "IDS"
        [[rule]]
        id = "r"
        name = "R"
        type = "information"
        severity = "moderate"
        standard = "ids:requirements/test.ids"
    "#,
    )
    .unwrap();
    let models = vec![CheckModel {
        id: ModelId(1),
        model: Arc::new(model()),
        transform: Mat4::IDENTITY,
        mask: None,
    }];
    let job = CheckJob {
        ruleset,
        models: vec![ModelId(1)],
        levels: Vec::new(),
        ids: vec![(
            "requirements/test.ids".to_owned(),
            Arc::new(Ids::from_xml(&text).unwrap()),
        )],
        standards: Vec::new(),
    };
    let report = run_check(&models, std::slice::from_ref(&job), &|_| {});
    let rule = &report.rulesets[0].rules[0];
    let summary = rule.information.as_ref().unwrap();
    assert_eq!(summary.standard, "Test & check");
    // The wall passes "S" and "Old" (the IFC version is metadata); the slab
    // fails "Slabs" on both requirements; the pipe may not exist.
    assert_eq!(rule.findings.len(), 2);
    let pipe = rule.findings.iter().find(|f| f.a.express_id == 12).unwrap();
    assert_eq!(pipe.details, vec!["Element not allowed"]);
    let finding = rule.findings.iter().find(|f| f.a.express_id == 11).unwrap();
    assert_eq!(finding.a.express_id, 11);
    assert_eq!(finding.category, "Slabs");
    assert_eq!(finding.details, vec!["Name", "Classification"]);
    assert_eq!(summary.parts.len(), 4);
    assert_eq!(summary.parts[0].name, "S");
    assert_eq!(summary.parts[0].complete, 1);
    assert_eq!(summary.parts[1].items[0].required, 1);
    assert_eq!(summary.parts[1].items[0].present, 0);
    assert_eq!(summary.notes.len(), 2, "{:?}", summary.notes);

    // A rule whose file is missing says so.
    let missing = CheckJob {
        ids: Vec::new(),
        ..job
    };
    let report = run_check(&models, &[missing], &|_| {});
    let summary = report.rulesets[0].rules[0].information.clone().unwrap();
    assert!(summary.notes[0].contains("not available"));
}

// ---- buildingSMART test suite -------------------------------------------------

/// (express ID, class in upper case, arguments) of each STEP entity line.
fn step_entities(text: &str) -> impl Iterator<Item = (u32, String, &str)> {
    text.lines().filter_map(|l| {
        let (id, rest) = l.trim().strip_prefix('#')?.split_once('=')?;
        let (class, args) = rest.trim_start().split_once('(')?;
        Some((
            id.trim().parse().ok()?,
            class.trim().to_ascii_uppercase(),
            args,
        ))
    })
}

/// Products, processes and type objects of an IFC file that the loader did
/// not keep as elements or spatial nodes.
fn dropped_entities(model: &IfcModel, text: &str) -> Vec<u32> {
    let mut kept: rustc_hash::FxHashSet<u32> =
        model.elements.iter().map(|e| e.express_id).collect();
    let mut stack: Vec<&SpatialNode> = model.spatial_root.iter().collect();
    while let Some(n) = stack.pop() {
        kept.insert(n.express_id);
        stack.extend(&n.children);
    }
    let is = |class: &str, base: &str| {
        ifc_lite_core::is_subtype_of_for_schema(&model.schema, class, base)
    };
    step_entities(text)
        .filter(|(id, class, _)| {
            !kept.contains(id)
                && class != "IFCPROJECT"
                && (is(class, "IfcProduct")
                    || is(class, "IfcProcess")
                    || is(class, "IfcTypeObject"))
        })
        .map(|(id, _, _)| id)
        .collect()
}

/// The test files are bare entities without containment or geometry, which
/// OpenCheck does not load. Loads the file with every such entity aggregated
/// under the project (added when missing); also returns how many were
/// attached that way.
fn load_test_case(name: &str, text: &str) -> Result<(IfcModel, usize), String> {
    let load = |t: &str| crate::load_ifc_bytes(name, t.as_bytes()).map_err(|e| format!("ifc: {e}"));
    let mut text = text.to_owned();
    let mut model = load(&text)?;
    // Parts and contained elements are reached through their whole or container.
    let related: rustc_hash::FxHashSet<u32> = step_entities(&text)
        .filter(|(_, c, _)| c == "IFCRELAGGREGATES" || c == "IFCRELCONTAINEDINSPATIALSTRUCTURE")
        .flat_map(|(_, _, args)| {
            let list = args.find("(#").map_or("", |i| &args[i..]);
            let list = list.split(')').next().unwrap_or_default();
            list.split(',')
                .filter_map(|r| {
                    r.trim()
                        .trim_start_matches('(')
                        .strip_prefix('#')?
                        .parse()
                        .ok()
                })
                .collect::<Vec<u32>>()
        })
        .collect();
    let mut attached = 0;
    for round in 0..2 {
        let dropped: Vec<u32> = dropped_entities(&model, &text)
            .into_iter()
            .filter(|id| round > 0 || !related.contains(id))
            .collect();
        if dropped.is_empty() {
            continue;
        }
        attached += dropped.len();
        let mut extra = String::new();
        let project = step_entities(&text)
            .find(|(_, c, _)| c == "IFCPROJECT")
            .map(|(id, _, _)| id)
            .unwrap_or_else(|| {
                extra.push_str(
                    "#900000=IFCPROJECT('0YvctVUKr0kugbFTf53O9L',$,'Harness',$,$,$,$,$,$);\n",
                );
                900000
            });
        let list: Vec<String> = dropped.iter().map(|id| format!("#{id}")).collect();
        extra.push_str(&format!(
            "#{}=IFCRELAGGREGATES('1YvctVUKr0kugbFTf53O9{round}',$,$,$,#{project},({}));\n",
            900001 + round,
            list.join(",")
        ));
        let end = text.rfind("ENDSEC;").ok_or("ifc: no DATA section")?;
        text.insert_str(end, &extra);
        model = load(&text)?;
    }
    Ok((model, attached))
}

/// Runs the buildingSMART IDS test cases (`TestCases` folder of
/// github.com/buildingSMART/IDS) found in `$IDS_TESTCASES` and prints the
/// pass rate per facet. `pass-*` cases must pass, `fail-*` and `invalid-*`
/// must fail (an IDS that does not load counts as failing).
#[test]
#[ignore = "needs the buildingSMART test cases in $IDS_TESTCASES"]
fn official_test_cases() {
    let root = std::path::PathBuf::from(
        std::env::var("IDS_TESTCASES").expect("set IDS_TESTCASES to the TestCases folder"),
    );
    let mut folders: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    folders.sort();
    let mut total = (0, 0);
    let mut mismatches = Vec::new();
    for folder in folders {
        let facet = folder.file_name().unwrap().to_string_lossy().into_owned();
        let mut cases: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "ids"))
            .collect();
        cases.sort();
        let mut score = (0, 0);
        for ids_path in cases {
            let name = ids_path.file_stem().unwrap().to_string_lossy().into_owned();
            let expect_pass = name.starts_with("pass-");
            let ifc_path = ids_path.with_extension("ifc");
            let outcome: Result<(bool, usize), String> = (|| {
                let text = std::fs::read_to_string(&ids_path).map_err(|e| e.to_string())?;
                let ids = Ids::from_xml(&text).map_err(|e| format!("ids: {e}"))?;
                let ifc = std::fs::read_to_string(&ifc_path).map_err(|e| format!("ifc: {e}"))?;
                let (model, attached) = load_test_case(&name, &ifc)?;
                let ctx = IdsContext::new(&model);
                let mut ok = true;
                for spec in &ids.specifications {
                    let mut applicable = 0usize;
                    for element in &model.elements {
                        if !spec.applies(&ctx, element) {
                            continue;
                        }
                        applicable += 1;
                        let failures = spec.check(&ctx, element);
                        let forbidden = spec.max_occurs == Some(0) && spec.requirements.is_empty();
                        if forbidden || failures.iter().any(Option::is_some) {
                            ok = false;
                        }
                    }
                    if applicable < spec.min_occurs as usize
                        || spec
                            .max_occurs
                            .is_some_and(|m| m > 0 && applicable > m as usize)
                    {
                        ok = false;
                    }
                }
                Ok((ok, attached))
            })();
            let (passed, note) = match outcome {
                Ok((p, 0)) => (p, String::new()),
                Ok((p, n)) => (p, format!("[attached {n}]")),
                Err(e) => (false, e),
            };
            score.1 += 1;
            if passed == expect_pass {
                score.0 += 1;
            } else {
                mismatches.push(format!("{facet}/{name} {note}"));
            }
        }
        println!("| {facet} | {}/{} |", score.0, score.1);
        total.0 += score.0;
        total.1 += score.1;
    }
    println!("| total | {}/{} |", total.0, total.1);
    for m in &mismatches {
        println!("MISMATCH {m}");
    }
}

/// The LOD standard reports where each item was found, and a project's
/// overrides change where it looks. (Here because `model()` lives here.)
#[test]
fn information_rules_report_and_follow_overrides() {
    use crate::check::{InformationStandard, Source, StandardOverrides, builtin_standard};

    let builtin = crate::check::BUILTIN_RULESETS
        .iter()
        .find(|b| b.file_name == "information-lod-dk.toml")
        .unwrap();
    let ruleset = Ruleset::from_toml(builtin.toml).unwrap();
    let models = vec![CheckModel {
        id: ModelId(1),
        model: Arc::new(model()),
        transform: Mat4::IDENTITY,
        mask: None,
    }];
    let run = |standards: Vec<Arc<InformationStandard>>| {
        let job = CheckJob {
            ruleset: ruleset.clone(),
            models: vec![ModelId(1)],
            levels: vec![(ModelId(1), "300".to_owned())],
            ids: Vec::new(),
            standards,
        };
        let report = run_check(&models, &[job], &|_| {});
        let summary = report.rulesets[0].rules[0].information.clone().unwrap();
        let wall = summary
            .parts
            .into_iter()
            .find(|p| p.part == "wall")
            .unwrap();
        let row = |id: &str| wall.items.iter().find(|i| i.item == id).cloned().unwrap();
        (
            row("thickness"),
            row("type-name"),
            row("classification-code"),
        )
    };

    let (thickness, type_name, code) = run(Vec::new());
    assert_eq!(
        thickness.found_in,
        vec![("Qto_WallBaseQuantities.Width".to_owned(), 1)]
    );
    assert_eq!(
        type_name.found_in,
        vec![("Attribute ObjectType".to_owned(), 1)]
    );
    assert_eq!(code.found_in, vec![("Classification CCS".to_owned(), 1)]);

    // Thickness only from Pset_WallCommon for walls; classification only
    // from a property everywhere.
    let mut overrides = StandardOverrides::new("opencheck.information.lod-dk");
    overrides.items.insert(
        "classification-code".into(),
        vec![Source {
            property: Some("^Klassifikation$".into()),
            ..Source::default()
        }],
    );
    overrides.parts.entry("wall".into()).or_default().insert(
        "thickness".into(),
        vec![Source {
            property: Some("^FireRating$".into()),
            pset: Some("^Pset_WallCommon$".into()),
            ..Source::default()
        }],
    );
    let overrides = StandardOverrides::from_toml(&overrides.to_toml()).unwrap();
    let base = builtin_standard("opencheck.information.lod-dk").unwrap();
    let (adapted, skipped) = overrides.apply(base);
    assert!(skipped.is_empty());
    let (thickness, _, code) = run(vec![Arc::new(adapted)]);
    assert_eq!(
        thickness.found_in,
        vec![("Pset_WallCommon.FireRating".to_owned(), 1)]
    );
    assert_eq!(code.present, 0);
}
