//! Converts a coordination file as OpenCheck does, writes its models as IFC
//! files and reads each back as OpenCheck does, to try a file without the
//! app:
//!
//! ```sh
//! cargo run --release -p opencheck-smc --example smc2ifc -- project.smc [out-folder]
//! ```

use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let Some(input) = args.next().map(std::path::PathBuf::from) else {
        return Err("usage: smc2ifc <project.smc> [out-folder]".into());
    };
    let out = args.next().map(std::path::PathBuf::from);
    let name = input
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let started = Instant::now();
    let converted = opencheck_smc::convert_file(&input)?;
    println!(
        "{} (reader of {} {}): {:.1} s",
        env!("CARGO_PKG_NAME"),
        opencheck_smc::READER_NAME,
        opencheck_smc::READER_REVISION_DATE,
        started.elapsed().as_secs_f32()
    );
    println!("{}", converted.message(&name));
    for note in converted.notes() {
        println!("note: {note}");
    }
    if let Some(out) = &out {
        std::fs::create_dir_all(out)?;
    }
    for model in &converted.models {
        let s = &model.summary;
        println!(
            "{}: {} elements, {} with geometry, {} triangles, {} coloured, {:.1} MB of IFC",
            model.name,
            s.elements,
            s.with_geometry,
            s.triangles,
            s.coloured,
            model.ifc.len() as f64 / 1e6
        );
        if let Some(out) = &out {
            std::fs::write(out.join(format!("{}.ifc", model.name)), &model.ifc)?;
        }
        let started = Instant::now();
        let read = opencheck_core::load_ifc_bytes(&format!("{}.ifc", model.name), &model.ifc)?;
        println!(
            "    OpenCheck reads it: {} elements, {} with geometry, {} storeys, in {:.1} s",
            read.elements.len(),
            read.elements.iter().filter(|e| e.has_geometry()).count(),
            read.storeys().len(),
            started.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
