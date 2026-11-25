//! Writing a coordination file (`.smc`): a federated model as the file a
//! model checker opens.
//!
//! The file is a zip around one stream of serialized Java objects, the
//! checker's own classes. This writer makes those objects itself: one model
//! per model given, each with its site, building and storeys, its elements
//! with their GlobalId, class, name, type, tag and property sets, and the
//! shape of each element as faces of three points with its colour.
//!
//! How the file is built was read out of files the checker saved of
//! OpenCheck's own test models, and tried by opening changed copies in the
//! checker (see `docs/smc-files.md`). The checker's program was not looked
//! into. The classes with their version numbers and fields are in
//! `classes.txt`, drawn out of those files.
//!
//! What is not written: the checker's own shapes (profiles pulled along a
//! route, shapes cut from each other), type objects, materials, systems,
//! classifications, the link to the IFC file a model came from, rulesets,
//! results and presentations.

mod build;
mod classes;
mod java;
pub mod models;

use std::io::Write as _;

/// The version of the checker whose files this writer writes like; it stands
/// in the zip, where the checker says which version saved a file.
pub const WRITTEN_AS_VERSION: &str = "25.12.0.84";

/// One model of the file: one IFC file's worth of elements.
#[derive(Debug, Clone, Default)]
pub struct WriteModel {
    /// The model's name, e.g. the stem of its IFC file (`TDH_K09_F2_N01`).
    pub name: String,
    /// The short name the checker shows in front of it, e.g. `A` or `K`.
    pub short_name: String,
    pub discipline: WriteDiscipline,
    /// The program the model was made with, when known.
    pub application: String,
    /// The name of the project in the model.
    pub project: String,
    pub storeys: Vec<WriteStorey>,
    pub elements: Vec<WriteElement>,
}

/// The discipline the checker files a model under: its own list, in its own
/// order, which is the number it keeps in the file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum WriteDiscipline {
    #[default]
    Architectural,
    AirConditioning,
    BuildingServices,
    Electrical,
    Heat,
    Structural,
    Ventilation,
    Plumbing,
    Sprinkler,
    Inventory,
    FacilityManagement,
    Landscape,
    PrefabConcrete,
    SteelStructure,
    SiteOperations,
    Cooling,
    SpecialPiping,
    Process,
    Hvac,
    Geotechnical,
    Bridge,
    Road,
    Railway,
    PortsAndWaterways,
}

impl WriteDiscipline {
    pub const ALL: [WriteDiscipline; 24] = [
        Self::Architectural,
        Self::AirConditioning,
        Self::BuildingServices,
        Self::Electrical,
        Self::Heat,
        Self::Structural,
        Self::Ventilation,
        Self::Plumbing,
        Self::Sprinkler,
        Self::Inventory,
        Self::FacilityManagement,
        Self::Landscape,
        Self::PrefabConcrete,
        Self::SteelStructure,
        Self::SiteOperations,
        Self::Cooling,
        Self::SpecialPiping,
        Self::Process,
        Self::Hvac,
        Self::Geotechnical,
        Self::Bridge,
        Self::Road,
        Self::Railway,
        Self::PortsAndWaterways,
    ];

    /// The number the checker keeps for the discipline.
    pub fn number(self) -> i32 {
        self as i32
    }

    /// The discipline as the checker names it.
    pub fn label(self) -> &'static str {
        match self {
            Self::Architectural => "Architectural",
            Self::AirConditioning => "Air Conditioning",
            Self::BuildingServices => "Building Services",
            Self::Electrical => "Electrical",
            Self::Heat => "Heat",
            Self::Structural => "Structural",
            Self::Ventilation => "Ventilation",
            Self::Plumbing => "Plumbing",
            Self::Sprinkler => "Sprinkler",
            Self::Inventory => "Inventory",
            Self::FacilityManagement => "Facility Management",
            Self::Landscape => "Landscape",
            Self::PrefabConcrete => "Prefab Concrete",
            Self::SteelStructure => "Steel Structure",
            Self::SiteOperations => "Site Operations",
            Self::Cooling => "Cooling",
            Self::SpecialPiping => "Special Piping",
            Self::Process => "Process",
            Self::Hvac => "HVAC",
            Self::Geotechnical => "Geotechnical",
            Self::Bridge => "Bridge",
            Self::Road => "Road",
            Self::Railway => "Railway",
            Self::PortsAndWaterways => "Ports and Waterways",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WriteStorey {
    /// The storey's GlobalId; empty for a new one.
    pub guid: String,
    pub name: String,
    /// The storey's height in metres.
    pub elevation: f64,
}

#[derive(Debug, Clone, Default)]
pub struct WriteElement {
    pub guid: String,
    /// The IFC class, e.g. `IfcWall`.
    pub ifc_class: String,
    pub name: String,
    pub description: Option<String>,
    pub object_type: Option<String>,
    pub predefined_type: Option<String>,
    pub tag: Option<String>,
    /// The storey the element is on: its place in the model's storeys.
    pub storey: Option<usize>,
    pub property_sets: Vec<WritePropertySet>,
    pub shape: Option<WriteShape>,
}

#[derive(Debug, Clone, Default)]
pub struct WritePropertySet {
    pub name: String,
    pub properties: Vec<WriteProperty>,
}

#[derive(Debug, Clone)]
pub struct WriteProperty {
    pub name: String,
    pub value: WriteValue,
    /// The IFC type of the value, e.g. `IfcLabel`, when known.
    pub value_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WriteValue {
    Text(String),
    Number(f64),
    Integer(i64),
    Bool(bool),
}

/// The shape of an element: triangles in the frame all models share.
#[derive(Debug, Clone, Default)]
pub struct WriteShape {
    /// The corners in metres.
    pub positions: Vec<[f64; 3]>,
    /// Three corners per triangle, seen anticlockwise from outside.
    pub triangles: Vec<[u32; 3]>,
    /// The colour of the shape: red, green, blue and opacity, 0 to 1.
    pub color: [f32; 4],
    /// A colour per triangle, for a shape of several colours (a window's
    /// frame and glass). Empty when the whole shape has `color`.
    pub triangle_colors: Vec<[f32; 4]>,
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("there are no models to write")]
    NoModels,
    #[error("cannot write the file: {0}")]
    Zip(String),
    #[error("the models are too large for one coordination file (4 GB of model data)")]
    TooLarge,
}

/// What was written, to tell the user.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteSummary {
    pub models: usize,
    pub elements: usize,
    pub with_shape: usize,
    pub triangles: usize,
    /// IFC classes the checker has no class of its own for in the table,
    /// written as plain objects: class and count.
    pub as_objects: Vec<(String, usize)>,
}

/// A coordination file of the models: the bytes of the `.smc`.
///
/// `file_name` is the name the file is saved under (`project.smc`); the
/// checker names the model inside the zip after it.
pub fn write_smc(
    models: &[WriteModel],
    file_name: &str,
) -> Result<(Vec<u8>, WriteSummary), WriteError> {
    if models.is_empty() {
        return Err(WriteError::NoModels);
    }
    let (stream, summary) = build::stream(models, file_name);
    if stream.len() >= u32::MAX as usize {
        return Err(WriteError::TooLarge);
    }
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let failed = |e: zip::result::ZipError| WriteError::Zip(e.to_string());
    zip.start_file(file_name, options).map_err(failed)?;
    zip.write_all(&stream)
        .map_err(|e| WriteError::Zip(e.to_string()))?;
    // the checker keeps a list per plugin beside the model; this one is empty
    zip.start_file(classes::full(PLUGIN_ENTRY), options)
        .map_err(failed)?;
    zip.write_all(EMPTY_LIST)
        .map_err(|e| WriteError::Zip(e.to_string()))?;
    let bytes = zip.finish().map_err(failed)?.into_inner();
    let bytes = with_entry_comment(bytes, file_name, WRITTEN_AS_VERSION)
        .ok_or_else(|| WriteError::Zip("the zip came out in a form that is not known".into()))?;
    Ok((bytes, summary))
}

/// The entry of a plugin's list; `~` as in the class table.
const PLUGIN_ENTRY: &str = "com.~.saf.plugins.hyperlinkplugin";

/// An empty `java.util.Collections$EmptyList`, serialized.
const EMPTY_LIST: &[u8] = &[
    0xac, 0xed, 0x00, 0x05, 0x73, 0x72, 0x00, 0x1f, 0x6a, 0x61, 0x76, 0x61, 0x2e, 0x75, 0x74, 0x69,
    0x6c, 0x2e, 0x43, 0x6f, 0x6c, 0x6c, 0x65, 0x63, 0x74, 0x69, 0x6f, 0x6e, 0x73, 0x24, 0x45, 0x6d,
    0x70, 0x74, 0x79, 0x4c, 0x69, 0x73, 0x74, 0x7a, 0xb8, 0x17, 0xb4, 0x3c, 0xa7, 0x9e, 0xde, 0x02,
    0x00, 0x00, 0x78, 0x70,
];

/// Gives one entry of a zip a comment. The checker writes its version there,
/// and the zip library has no way to set it, so the directory at the end of
/// the zip is written again with the comment in it.
fn with_entry_comment(zip: Vec<u8>, entry: &str, comment: &str) -> Option<Vec<u8>> {
    let u16_at = |at: usize| Some(u16::from_le_bytes(zip.get(at..at + 2)?.try_into().ok()?));
    let u32_at = |at: usize| Some(u32::from_le_bytes(zip.get(at..at + 4)?.try_into().ok()?));
    // the end record: 22 bytes, when the zip has no comment of its own
    let end = zip.len().checked_sub(22)?;
    if u32_at(end)? != 0x0605_4b50 || u16_at(end + 20)? != 0 {
        return None;
    }
    let count = u16_at(end + 10)? as usize;
    let size = u32_at(end + 12)? as usize;
    let start = u32_at(end + 16)? as usize;
    let mut directory = Vec::with_capacity(size + comment.len());
    let mut at = start;
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let (name, extra, old) = (
            u16_at(at + 28)? as usize,
            u16_at(at + 30)? as usize,
            u16_at(at + 32)? as usize,
        );
        let fixed_and_names = at + 46 + name + extra;
        let is_it = zip.get(at + 46..at + 46 + name)? == entry.as_bytes();
        directory.extend_from_slice(zip.get(at..fixed_and_names)?);
        if is_it {
            let header = directory.len() - (46 + name + extra);
            directory[header + 32..header + 34]
                .copy_from_slice(&(comment.len() as u16).to_le_bytes());
            directory.extend_from_slice(comment.as_bytes());
        } else {
            directory.extend_from_slice(zip.get(fixed_and_names..fixed_and_names + old)?);
        }
        at = fixed_and_names + old;
    }
    if at != end {
        return None;
    }
    let mut out = zip[..start].to_vec();
    out.extend_from_slice(&directory);
    let mut tail = zip[end..].to_vec();
    tail[12..16].copy_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&tail);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_shape(at: [f64; 3], size: [f64; 3], color: [f32; 4]) -> WriteShape {
        let mut positions = Vec::new();
        for z in [0.0, size[2]] {
            for (x, y) in [
                (0.0, 0.0),
                (size[0], 0.0),
                (size[0], size[1]),
                (0.0, size[1]),
            ] {
                positions.push([at[0] + x, at[1] + y, at[2] + z]);
            }
        }
        let quads = [
            [0, 3, 2, 1],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [1, 2, 6, 5],
            [2, 3, 7, 6],
            [3, 0, 4, 7],
        ];
        let triangles = quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect();
        WriteShape {
            positions,
            triangles,
            color,
            triangle_colors: Vec::new(),
        }
    }

    fn two_models() -> Vec<WriteModel> {
        let storeys = vec![
            WriteStorey {
                guid: "1BifckUPnGguTZWkyxRyXB".into(),
                name: "Stue".into(),
                elevation: 0.0,
            },
            WriteStorey {
                guid: "26YfOZzy5GCOxnopVrZ24M".into(),
                name: "1. sal".into(),
                elevation: 3.0,
            },
        ];
        let wall = WriteElement {
            guid: "3GrD7s0NPRhf7jZnEDOjHY".into(),
            ifc_class: "IfcWall".into(),
            name: "Væg W1".into(),
            object_type: Some("Betonvæg 200".into()),
            predefined_type: Some("SOLIDWALL".into()),
            tag: Some("W1".into()),
            storey: Some(0),
            property_sets: vec![WritePropertySet {
                name: "Pset_WallCommon".into(),
                properties: vec![
                    WriteProperty {
                        name: "IsExternal".into(),
                        value: WriteValue::Bool(true),
                        value_type: Some("IfcBoolean".into()),
                    },
                    WriteProperty {
                        name: "FireRating".into(),
                        value: WriteValue::Text("REI 60".into()),
                        value_type: Some("IfcLabel".into()),
                    },
                    WriteProperty {
                        name: "ThermalTransmittance".into(),
                        value: WriteValue::Number(0.18),
                        value_type: Some("IfcThermalTransmittanceMeasure".into()),
                    },
                    WriteProperty {
                        name: "Heltal".into(),
                        value: WriteValue::Integer(42),
                        value_type: Some("IfcInteger".into()),
                    },
                ],
            }],
            shape: Some(box_shape(
                [0.0, 0.0, 0.0],
                [4.0, 0.2, 3.0],
                [0.8, 0.2, 0.2, 1.0],
            )),
            ..WriteElement::default()
        };
        let slab = WriteElement {
            guid: "04re88i7nULQMBfIiq4xam".into(),
            ifc_class: "IfcSlab".into(),
            name: "Dæk S2".into(),
            storey: Some(1),
            shape: Some(box_shape(
                [0.0, 0.0, 2.8],
                [4.0, 3.2, 0.2],
                [0.2, 0.2, 0.8, 0.5],
            )),
            ..WriteElement::default()
        };
        let odd = WriteElement {
            guid: "1CZ1ehIAvJ_w7wD5ZYCC9K".into(),
            ifc_class: "IfcSomethingNew".into(),
            name: "Ukendt klasse".into(),
            storey: None,
            shape: Some(box_shape(
                [2.5, 0.5, 0.0],
                [1.0, 1.0, 1.0],
                [0.9, 0.1, 0.9, 1.0],
            )),
            ..WriteElement::default()
        };
        vec![
            WriteModel {
                name: "T1_ARK".into(),
                short_name: "A".into(),
                discipline: WriteDiscipline::Architectural,
                application: "OpenCheck".into(),
                project: "Testhus".into(),
                storeys: storeys.clone(),
                elements: vec![wall, slab],
            },
            WriteModel {
                name: "T3_VENT".into(),
                short_name: "V".into(),
                discipline: WriteDiscipline::Ventilation,
                application: "OpenCheck".into(),
                project: "Testhus".into(),
                storeys,
                elements: vec![odd],
            },
        ]
    }

    #[test]
    fn the_disciplines_have_the_numbers_of_the_checkers_list() {
        for (k, d) in WriteDiscipline::ALL.iter().enumerate() {
            assert_eq!(d.number(), k as i32, "{}", d.label());
        }
        assert_eq!(WriteDiscipline::Structural.number(), 5);
        assert_eq!(WriteDiscipline::Ventilation.number(), 6);
        assert_eq!(WriteDiscipline::Landscape.number(), 11);
    }

    #[test]
    fn nothing_to_write_is_an_error() {
        assert!(matches!(write_smc(&[], "x.smc"), Err(WriteError::NoModels)));
    }

    #[test]
    fn the_zip_names_the_model_after_the_file_and_says_the_version() {
        let (bytes, summary) = write_smc(&two_models(), "Testhus.smc").unwrap();
        assert_eq!(summary.models, 2);
        assert_eq!(summary.elements, 3);
        assert_eq!(summary.with_shape, 3);
        assert_eq!(summary.triangles, 36);
        assert_eq!(summary.as_objects, vec![("IfcSomethingNew".to_string(), 1)]);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert_eq!(zip.len(), 2);
        let entry = zip.by_index(0).unwrap();
        assert_eq!(entry.name().unwrap(), "Testhus.smc");
        assert_eq!(entry.comment(), WRITTEN_AS_VERSION);
        drop(entry);
        assert_eq!(
            zip.by_index(1).unwrap().name().unwrap(),
            classes::full(PLUGIN_ENTRY)
        );
    }

    /// What is written comes back through the reader: the models, their
    /// elements with GlobalId, class and name, their storeys and their shapes
    /// where they were put.
    #[test]
    fn what_is_written_is_read_back() {
        let (bytes, _) = write_smc(&two_models(), "Testhus.smc").unwrap();
        if let Ok(folder) = std::env::var("OPENCHECK_SMC_OUT") {
            std::fs::write(std::path::Path::new(&folder).join("Testhus.smc"), &bytes).unwrap();
        }
        let read = crate::convert_bytes("Testhus.smc", &bytes).unwrap();
        assert_eq!(read.models.len(), 2);
        let (ark, vent) = (&read.models[0], &read.models[1]);
        assert_eq!(
            (ark.name.as_str(), ark.short_name.as_str()),
            ("T1_ARK", "A")
        );
        assert_eq!(
            (vent.name.as_str(), vent.short_name.as_str()),
            ("T3_VENT", "V")
        );
        assert_eq!(ark.summary.with_geometry, 2);
        assert_eq!(ark.summary.triangles, 24);
        assert_eq!(ark.summary.unplaced, 0);
        assert_eq!(vent.summary.with_geometry, 1);
        let ifc = String::from_utf8_lossy(&ark.ifc);
        for expected in [
            "3GrD7s0NPRhf7jZnEDOjHY",
            "04re88i7nULQMBfIiq4xam",
            "IFCWALL(",
            "IFCSLAB(",
            "Pset_WallCommon",
            "FireRating",
            "REI 60",
            "1BifckUPnGguTZWkyxRyXB",
        ] {
            assert!(ifc.contains(expected), "{expected} is missing");
        }
        let ifc = String::from_utf8_lossy(&vent.ifc);
        assert!(ifc.contains("1CZ1ehIAvJ_w7wD5ZYCC9K"));
    }
}
