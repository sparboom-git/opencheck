//! Converts a Revit file as OpenCheck does and reads the IFC back as
//! OpenCheck does, to try a file or a new rvt-rs release without the app:
//!
//! ```sh
//! cargo run -p opencheck-rvt --example rvt2ifc -- model.rvt [out.ifc]
//! ```

use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next().map(std::path::PathBuf::from) else {
        return Err("usage: rvt2ifc <model.rvt> [out.ifc]".into());
    };
    let name = input
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let started = Instant::now();
    let converted = opencheck_rvt::convert_file(&input)?;
    println!(
        "{} {}: {:.1} s, {:.1} MB of IFC",
        opencheck_rvt::CONVERTER_NAME,
        opencheck_rvt::CONVERTER_VERSION,
        started.elapsed().as_secs_f32(),
        converted.ifc.len() as f64 / 1e6
    );
    println!("{}", converted.summary.message(&name));
    for warning in &converted.summary.warnings {
        println!("warning: {warning}");
    }

    let started = Instant::now();
    let ifc_name = input.with_extension("ifc");
    let ifc_name = ifc_name.file_name().unwrap_or_default().to_string_lossy();
    let model = opencheck_core::load_ifc_bytes(&ifc_name, &converted.ifc)?;
    println!(
        "OpenCheck: {} elements, {} with geometry, read in {:.1} s",
        model.elements.len(),
        model.meshes.len(),
        started.elapsed().as_secs_f32()
    );

    if let Some(output) = args.next() {
        std::fs::write(&output, &converted.ifc)?;
        println!("written to {}", output.to_string_lossy());
    }
    Ok(())
}
