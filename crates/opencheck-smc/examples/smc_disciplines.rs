//! Writes a coordination file with one model per discipline of the model
//! checker, each a box and named after its discipline. Opened in the checker,
//! every model should show the discipline its name says.
//!
//! ```sh
//! cargo run -p opencheck-smc --example smc_disciplines -- out-folder
//! ```

use opencheck_smc::{
    WriteDiscipline, WriteElement, WriteModel, WriteShape, WriteStorey, write_smc,
};

fn main() {
    let folder = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let models: Vec<WriteModel> = WriteDiscipline::ALL.into_iter().map(model).collect();
    let file = "discipliner.smc";
    let (bytes, _) = write_smc(&models, file).expect("a file");
    let path = std::path::Path::new(&folder).join(file);
    std::fs::write(&path, bytes).expect("writing the file");
    println!("{}", path.display());
}

fn model(discipline: WriteDiscipline) -> WriteModel {
    let number = discipline.number();
    // a box of one metre, two metres from the next
    let x = number as f64 * 2.0;
    let mut positions = Vec::new();
    for z in [0.0, 1.0] {
        for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
            positions.push([x + dx, dy, z]);
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
    WriteModel {
        name: format!("{number:02} {}", discipline.label()),
        short_name: format!("D{number:02}"),
        discipline,
        application: "OpenCheck".into(),
        project: "Discipliner".into(),
        storeys: vec![WriteStorey {
            guid: String::new(),
            name: "Stue".into(),
            elevation: 0.0,
        }],
        elements: vec![WriteElement {
            // 22 characters, the last two the number
            guid: format!("0OpenCheckDisciplin__{number:02}")[..22].to_string(),
            ifc_class: "IfcBuildingElementProxy".into(),
            name: format!("Kasse {number:02}"),
            storey: Some(0),
            shape: Some(WriteShape {
                positions,
                triangles: quads
                    .iter()
                    .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
                    .collect(),
                color: [0.2, 0.5, 0.8, 1.0],
                triangle_colors: Vec::new(),
            }),
            ..WriteElement::default()
        }],
    }
}
