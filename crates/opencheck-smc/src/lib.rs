//! Coordination files (`.smc`) for OpenCheck.
//!
//! A coordination file is what a model checker saves: the IFC
//! models of a project put together, with their elements, properties and
//! shapes, in the checker's own format, a zip around one stream of
//! serialized Java objects. There is no IFC inside. The reader of that stream
//! ([`stream`]) and the tessellation of its shapes ([`mesh`]) are from
//! [Open BIM Suite](https://github.com/OpenAEC-Foundation/open-bim-suite) by
//! the OpenAEC Foundation, under the LGPL-3.0-or-later; this crate is under
//! that licence as a whole. See `licenses/open-bim-suite-NOTICE`.
//!
//! What this crate adds is [`ifc`]: each model of the file is written as an
//! IFC4 file, with its elements, GlobalIds, property sets, storeys, colours
//! and geometry as triangles, and OpenCheck then reads those files as it
//! reads any IFC. Nothing else in OpenCheck knows the format.
//!
//! The other way, [`write`] makes a coordination file of models: the objects
//! of the checker written by this crate itself, which Open BIM Suite has no
//! part in.
//!
//! Neither Open BIM Suite nor OpenCheck is affiliated with the maker of any
//! model checker.

use std::io::{Read, Seek};
use std::path::Path;

mod ifc;
pub mod mesh;
pub mod stream;
pub mod write;

pub use write::{
    WriteClassification, WriteDiscipline, WriteElement, WriteError, WriteModel, WriteProperty,
    WritePropertySet, WriteShape, WriteStorey, WriteSummary, WriteValue, write_smc,
};

/// Where the reader comes from.
pub const READER_NAME: &str = "Open BIM Suite";
pub const READER_AUTHOR: &str = "OpenAEC Foundation";
pub const READER_URL: &str = "https://github.com/OpenAEC-Foundation/open-bim-suite";
pub const READER_LICENSE: &str = "LGPL-3.0-or-later";
/// The revision of Open BIM Suite the reader was copied from, and its date.
pub const READER_REVISION: &str = "eb4142ee12ea28f796c07f475a24a1d0c66aa8f0";
pub const READER_REVISION_DATE: &str = "2026-11-25";

/// The licence and the notice, to show and to ship with OpenCheck.
pub const READER_LICENSE_TEXT: &str = include_str!("../licenses/open-bim-suite-LICENSE");
pub const READER_NOTICE: &str = include_str!("../licenses/open-bim-suite-NOTICE");

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("cannot read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a coordination file: {0}")]
    Read(String),
    #[error("the coordination file holds no models with elements")]
    NoModels,
}

/// A coordination file as IFC4 files, one per model it holds.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The models, in the order of the file. The first one defines the
    /// origin of the federation when they are added together.
    pub models: Vec<ConvertedModel>,
}

/// One model of the file, as IFC4.
#[derive(Debug, Clone)]
pub struct ConvertedModel {
    /// The model's name: the stem of the IFC file it was made from
    /// (`TDH_K09_F2_N01`), or the checker's short name when the file does
    /// not say.
    pub name: String,
    /// The short name the checker shows for the model, e.g. `K` or `VENT`;
    /// empty when it has none.
    pub short_name: String,
    /// The IFC4 file (STEP text).
    pub ifc: Vec<u8>,
    pub summary: ModelSummary,
}

/// What was written for one model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModelSummary {
    /// Elements written (openings and the project itself are not).
    pub elements: usize,
    pub with_geometry: usize,
    pub triangles: usize,
    /// Elements with a colour of their own; the rest take the model's.
    pub coloured: usize,
    /// Elements whose storey the file says, through their frames.
    pub placed: usize,
    /// Elements put under the first storey because the file does not say.
    pub unplaced: usize,
    /// Site, building or storey added because the model has none.
    pub made_up_spatial: usize,
    /// IFC classes of the file that IFC4 has no concrete entity for, written
    /// as `IfcBuildingElementProxy`: class and count.
    pub proxies: Vec<(String, usize)>,
}

impl Converted {
    /// What was converted, in a few sentences for the user.
    pub fn message(&self, file_name: &str) -> String {
        let mut text = format!(
            "{file_name} holds {} model{} with elements",
            self.models.len(),
            if self.models.len() == 1 { "" } else { "s" }
        );
        if self.models.is_empty() {
            text.push('.');
            return text;
        }
        text.push_str(": ");
        let names: Vec<String> = self
            .models
            .iter()
            .map(|m| format!("{} ({} elements)", m.name, m.summary.elements))
            .collect();
        text.push_str(&names.join(", "));
        text.push_str(
            ". Each is now an IFC4 model: its elements with their GlobalIds, \
                       property sets, storeys and colours, and their shapes as triangles.",
        );
        text
    }

    /// What to be careful about, model by model.
    pub fn notes(&self) -> Vec<String> {
        let mut notes = Vec::new();
        for m in &self.models {
            let s = &m.summary;
            if s.unplaced > 0 {
                notes.push(format!(
                    "{}: {} elements are put under the first storey because the file does not say which storey they are on.",
                    m.name, s.unplaced
                ));
            }
            if s.made_up_spatial > 0 {
                notes.push(format!(
                    "{}: the model has no site, building or storey of its own; {} were added.",
                    m.name, s.made_up_spatial
                ));
            }
            for (class, count) in &s.proxies {
                notes.push(format!(
                    "{}: {count} elements of {class} are written as IfcBuildingElementProxy, since IFC4 has no such entity.",
                    m.name
                ));
            }
            if s.elements > 0 && s.with_geometry < s.elements {
                notes.push(format!(
                    "{}: {} of {} elements have no shape in the file.",
                    m.name,
                    s.elements - s.with_geometry,
                    s.elements
                ));
            }
        }
        notes
    }
}

/// Converts a coordination file: one IFC4 file per model with elements.
pub fn convert_file(path: &Path) -> Result<Converted, ConvertError> {
    let file = std::fs::File::open(path)?;
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    convert(std::io::BufReader::new(file), stem)
}

/// [`convert_file`] for a file held in memory.
pub fn convert_bytes(file_name: &str, bytes: &[u8]) -> Result<Converted, ConvertError> {
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    convert(std::io::Cursor::new(bytes.to_vec()), stem)
}

fn convert<R: Read + Seek + Send + 'static>(
    input: R,
    stem: String,
) -> Result<Converted, ConvertError> {
    // The objects of the stream lie deep in each other (a model, its
    // classes, their elements, their shapes): the reader follows them on the
    // stack, so it gets a large one of its own.
    let worker = std::thread::Builder::new()
        .name("smc".into())
        .stack_size(1 << 30)
        .spawn(move || -> Result<Converted, ConvertError> {
            let data = ifc::read(input).map_err(ConvertError::Read)?;
            let models = ifc::write_models(&data, &stem);
            if models.is_empty() {
                return Err(ConvertError::NoModels);
            }
            Ok(Converted { models })
        })?;
    worker
        .join()
        .map_err(|_| ConvertError::Read("the reader stopped on this file".into()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_message_names_the_models() {
        let converted = Converted {
            models: vec![
                ConvertedModel {
                    name: "TDH_K01".into(),
                    short_name: "A".into(),
                    ifc: Vec::new(),
                    summary: ModelSummary {
                        elements: 10,
                        with_geometry: 9,
                        ..ModelSummary::default()
                    },
                },
                ConvertedModel {
                    name: "TDH_K09".into(),
                    short_name: "K".into(),
                    ifc: Vec::new(),
                    summary: ModelSummary {
                        elements: 5,
                        with_geometry: 5,
                        unplaced: 2,
                        ..ModelSummary::default()
                    },
                },
            ],
        };
        let message = converted.message("TDH.smc");
        assert!(message.starts_with(
            "TDH.smc holds 2 models with elements: TDH_K01 (10 elements), TDH_K09 (5 elements)."
        ));
        let notes = converted.notes();
        assert_eq!(notes.len(), 2);
        assert!(notes[0].starts_with("TDH_K01: 1 of 10 elements have no shape"));
        assert!(notes[1].starts_with("TDH_K09: 2 elements are put under the first storey"));
    }

    #[test]
    fn a_file_that_is_no_coordination_file_is_refused() {
        let err = convert_bytes("x.smc", b"PK\x03\x04 not a zip").unwrap_err();
        assert!(matches!(err, ConvertError::Read(_)), "{err}");
        let err = convert_bytes("x.smc", b"just text").unwrap_err();
        assert!(matches!(err, ConvertError::Read(_)), "{err}");
    }

    /// A real file, when there is one: `OPENCHECK_SMC` names it.
    #[test]
    fn a_coordination_file_converts() {
        let Ok(path) = std::env::var("OPENCHECK_SMC") else {
            return;
        };
        let converted = convert_file(Path::new(&path)).unwrap();
        assert!(!converted.models.is_empty());
        for model in &converted.models {
            assert!(model.summary.elements > 0, "{}", model.name);
            assert!(model.ifc.starts_with(b"ISO-10303-21;"));
        }
    }
}
