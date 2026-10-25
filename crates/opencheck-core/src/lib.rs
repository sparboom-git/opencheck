//! Core data model for OpenCheck.
//!
//! This crate turns IFC files into an [`IfcModel`]: a list of elements with
//! their property sets, the spatial structure (project, site, building,
//! storeys) and a triangle mesh per element. Parsing and tessellation are
//! delegated to [ifc-lite](https://github.com/LTplus-AG/ifc-lite); everything
//! that the rest of OpenCheck sees is defined here, so the ifc-lite API stays
//! behind this crate boundary.
//!
//! Several models are combined into a [`Federation`], which places them in a
//! shared world frame.

pub mod area;
pub mod autorun;
pub mod bcf;
pub mod bcf_file;
pub mod check;
mod discipline;
mod federation;
mod geometry;
mod loader;
mod model;
pub mod project;
mod properties;
mod spatial;

pub use discipline::Discipline;
pub use federation::{Federation, ModelId};
pub use geometry::{Aabb, ElementMesh};
pub use loader::{LoadError, LoadStats, load_ifc_bytes, load_ifc_file};
pub use model::{Element, IfcModel};
pub use properties::{ClassificationRef, Property, PropertySet, PropertySetKind, PropertyValue};
pub use spatial::SpatialNode;
