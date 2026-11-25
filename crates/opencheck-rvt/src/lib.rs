//! Revit files for OpenCheck.
//!
//! A `.rvt` file is converted to IFC4 in memory, and OpenCheck then reads that
//! IFC as it reads any other. The conversion is the work of
//! [rvt-rs](https://github.com/DrunkOnJava/rvt-rs) by Griffin Long
//! (Apache-2.0): it reads Revit files without Revit, from public observations
//! of the file format. OpenCheck only calls it.
//!
//! This crate is the only place that knows rvt-rs. Everything else sees IFC
//! bytes and a [`Summary`], so a new rvt-rs release is taken in by changing
//! its version in the workspace `Cargo.toml` and [`CONVERTER_VERSION`] here;
//! `docs/revit-files.md` has the steps.
//!
//! rvt-rs is not affiliated with Autodesk. "Autodesk" and "Revit" are
//! trademarks of Autodesk, Inc.

use std::path::Path;

use rvt::RevitFile;
use rvt::ifc::step_writer::{StepOptions, write_step_to};
use rvt::ifc::{ExportDiagnostics, ExportOptions, run_export};

/// The project the conversion comes from.
pub const CONVERTER_NAME: &str = "rvt-rs";
/// Who writes it.
pub const CONVERTER_AUTHOR: &str = "Griffin Long";
pub const CONVERTER_URL: &str = "https://github.com/DrunkOnJava/rvt-rs";
pub const CONVERTER_LICENSE: &str = "Apache-2.0";
/// The rvt-rs release built in; a test holds it to `Cargo.lock`.
pub const CONVERTER_VERSION: &str = "0.25.0";

/// The licence and notices of rvt-rs, to show and to ship with OpenCheck.
pub const CONVERTER_LICENSE_TEXT: &str = include_str!("../licenses/rvt-rs-LICENSE");
pub const CONVERTER_NOTICE: &str = include_str!("../licenses/rvt-rs-NOTICE");

#[derive(Debug, thiserror::Error)]
pub enum ConvertError {
    #[error("{CONVERTER_NAME} could not read the Revit file: {0}")]
    Read(#[from] rvt::Error),
    #[error("{CONVERTER_NAME} could not write the IFC: {0}")]
    Write(#[from] std::io::Error),
    #[error(
        "{CONVERTER_NAME} {CONVERTER_VERSION} found no building elements in this Revit file{}",
        release_hint(*.0)
    )]
    NoElements(Option<u32>),
}

fn release_hint(revit: Option<u32>) -> String {
    revit.map_or_else(String::new, |year| format!(" (Revit {year})"))
}

/// A Revit file as IFC4, with what the conversion says about itself.
#[derive(Debug, Clone)]
pub struct Converted {
    /// The IFC4 file (STEP text).
    pub ifc: Vec<u8>,
    pub summary: Summary,
}

/// What was converted and how well, from the export diagnostics of rvt-rs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Summary {
    /// The Revit release that saved the file, e.g. 2026.
    pub revit_version: Option<u32>,
    /// How rvt-rs rates its support for that release: `verified`, `partial`
    /// or `experimental`.
    pub release_support: Option<String>,
    /// Building elements written to the IFC.
    pub elements: usize,
    pub elements_with_geometry: usize,
    /// Elements whose body is only their bounding box: placed and sized, but
    /// the shape was not read.
    pub boxes: usize,
    /// Model elements rvt-rs found but could not export.
    pub not_exported: usize,
    /// A resource limit kept part of the file from being read.
    pub partial: bool,
    /// The warnings rvt-rs gave.
    pub warnings: Vec<String>,
}

impl Summary {
    fn from_diagnostics(diagnostics: &ExportDiagnostics) -> Self {
        Self {
            revit_version: diagnostics.input.revit_version,
            release_support: diagnostics
                .input
                .release_support
                .as_ref()
                .map(|support| support.status.clone()),
            elements: diagnostics.exported.building_elements,
            elements_with_geometry: diagnostics.exported.building_elements_with_geometry,
            boxes: diagnostics.exported.bounding_box_bodies,
            not_exported: diagnostics.confidence.unexported_element_records,
            partial: diagnostics.is_partial() || diagnostics.work_budget_stops().is_some(),
            warnings: diagnostics.warnings.clone(),
        }
    }

    /// The summary in a few sentences, for the user: what to be careful
    /// about comes last.
    pub fn message(&self, file_name: &str) -> String {
        let mut text = format!(
            "{file_name} was converted to IFC with {CONVERTER_NAME} {CONVERTER_VERSION}: {} elements",
            self.elements
        );
        if self.boxes > 0 {
            text.push_str(&format!(
                ", {} of them only as a box (placed and sized, the shape was not read)",
                self.boxes
            ));
        }
        text.push('.');
        if self.not_exported > 0 {
            text.push_str(&format!(
                " {} elements could not be converted and are missing.",
                self.not_exported
            ));
        }
        if self.partial {
            text.push_str(" Part of the file was not read, so the model is incomplete.");
        }
        match (self.revit_version, self.release_support.as_deref()) {
            (Some(year), Some(status)) if status != "verified" => {
                text.push_str(&format!(" Support for Revit {year} is {status}."));
            }
            _ => {}
        }
        text.push_str(" Check the result against the Revit model before relying on it.");
        text
    }
}

/// Converts a Revit file on disk.
pub fn convert_file(path: impl AsRef<Path>) -> Result<Converted, ConvertError> {
    convert(RevitFile::open(path)?)
}

/// Converts a Revit file held in memory.
pub fn convert_bytes(bytes: Vec<u8>) -> Result<Converted, ConvertError> {
    convert(RevitFile::open_bytes(bytes)?)
}

fn convert(mut file: RevitFile) -> Result<Converted, ConvertError> {
    // As `rvt-ifc --revit-property-sets`: the elements with the best body
    // rvt-rs has for each, and the Revit parameters as property sets, which
    // information checks need.
    let options = ExportOptions::default().with_revit_property_sets(true);
    let run = run_export(&mut file, &options)?;
    // The file and the streams it holds inflated are not needed any more.
    drop(file);
    let summary = Summary::from_diagnostics(&run.diagnostics);
    if summary.elements == 0 {
        return Err(ConvertError::NoElements(summary.revit_version));
    }
    let mut ifc = Vec::new();
    write_step_to(&run.model, &StepOptions::default(), &mut ifc)?;
    Ok(Converted { ifc, summary })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `CONVERTER_VERSION` is shown to the user and written in notices, so it
    /// must be the rvt-rs release that is really built in.
    #[test]
    fn converter_version_is_the_locked_one() {
        let lock = include_str!("../../../Cargo.lock").replace("\r\n", "\n");
        let locked = format!("name = \"rvt\"\nversion = \"{CONVERTER_VERSION}\"\n");
        assert!(
            lock.contains(&locked),
            "CONVERTER_VERSION ({CONVERTER_VERSION}) is not the rvt release in Cargo.lock"
        );
    }

    #[test]
    fn something_that_is_not_a_revit_file_is_refused() {
        let error = convert_bytes(b"ISO-10303-21;".to_vec()).unwrap_err();
        assert!(matches!(error, ConvertError::Read(_)), "{error}");
    }

    #[test]
    fn the_message_names_the_converter_and_what_to_watch() {
        let summary = Summary {
            revit_version: Some(2026),
            release_support: Some("experimental".to_owned()),
            elements: 6239,
            elements_with_geometry: 6236,
            boxes: 3647,
            ..Summary::default()
        };
        let message = summary.message("K09.rvt");
        assert!(message.starts_with("K09.rvt was converted to IFC with rvt-rs "));
        assert!(message.contains("6239 elements, 3647 of them only as a box"));
        assert!(message.contains("Support for Revit 2026 is experimental."));
        assert!(!message.contains("missing"));
    }

    #[test]
    fn the_licence_and_notice_are_those_of_rvt_rs() {
        assert!(CONVERTER_LICENSE_TEXT.contains("Apache License"));
        assert!(CONVERTER_NOTICE.contains(CONVERTER_AUTHOR));
    }
}
