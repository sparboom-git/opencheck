//! `opencheck export-smc`: IFC models as one coordination file (`.smc`), the
//! file a model checker opens. Each IFC file becomes a model in it.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use opencheck_smc::write::models::{NamedModel, models_to_write};

pub fn export_smc(files: &[PathBuf], output: &Path) -> Result<()> {
    let mut loaded = Vec::new();
    for path in files {
        loaded.push(crate::load(path)?);
    }
    let models = models_to_write(loaded.iter().map(NamedModel::new));
    let file_name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "model.smc".into());
    let (bytes, summary) = opencheck_smc::write_smc(&models, &file_name)?;
    std::fs::write(output, &bytes).with_context(|| format!("writing {}", output.display()))?;
    println!(
        "Wrote {} ({:.1} MB): {} models, {} elements, {} with a shape, {} triangles",
        output.display(),
        bytes.len() as f64 / 1e6,
        summary.models,
        summary.elements,
        summary.with_shape,
        summary.triangles
    );
    for m in &models {
        println!(
            "  {:<4} {:<28} {:<16} {:>6} elements, {} storeys",
            m.short_name,
            m.name,
            m.discipline.label(),
            m.elements.len(),
            m.storeys.len()
        );
    }
    for (class, count) in &summary.as_objects {
        println!(
            "  note: {count} elements of {class} are written as plain objects; the checker has no class of its own for them"
        );
    }
    println!(
        "  Shapes are triangles. Types, materials, systems and classification codes are written by name. Not written: the link to the IFC files."
    );
    Ok(())
}
