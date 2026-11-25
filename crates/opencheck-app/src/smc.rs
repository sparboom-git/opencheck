//! Coordination files (`.smc`): the IFC models of a project as a model
//! checker saved them together.
//!
//! Such a file is added like an IFC file (*File → Add models*, or dropped on
//! the window) while WIP is on. `opencheck-smc` converts it to one IFC4 file
//! per model it holds, with the elements, GlobalIds, property sets, storeys,
//! colours and shapes the checker kept, and OpenCheck loads those as it
//! loads any IFC. From there on nothing knows the models came from a
//! coordination file; a model is updated from its own IFC file with the ↻
//! button like any other. The converted IFC is kept in the project when it
//! is saved. Desktop only; a feature under development (`wip`).
//!
//! The reader is from Open BIM Suite by the OpenAEC Foundation
//! (LGPL-3.0-or-later): its credit, licence and notice are shown in *File →
//! Settings → WIP*, and `docs/smc-files.md` says what the conversion keeps
//! and leaves out.

use std::path::Path;

use eframe::egui::{self, RichText};
use opencheck_smc::WriteSummary;
use opencheck_smc::write::models::{NamedModel, models_to_write};

use crate::app::OpenCheckApp;
use crate::theme;

impl OpenCheckApp {
    /// Asks where to write the models of the project as one coordination
    /// file, and writes it in the background (WIP). The models are written
    /// under the names and disciplines they have in the model list.
    pub fn export_smc_dialog(&mut self, ctx: &egui::Context) {
        if self.federation.is_empty() {
            return;
        }
        let name = format!("{}.smc", self.export_base_name());
        let Some(path) = crate::platform::save_file(
            "Export the models as a coordination file",
            ("Coordination file", &["smc"]),
            &name,
        ) else {
            return;
        };
        let models = models_to_write(self.federation.iter().map(|(id, model)| {
            let entry = self.session.records.get(&id).map(|r| &r.entry);
            NamedModel {
                model,
                name: entry.map_or_else(|| model.short_name.clone(), |e| e.name.clone()),
                discipline: entry.map_or(model.discipline, |e| e.discipline),
            }
        }));
        self.jobs.export_smc(models, path, ctx);
    }

    /// The coordination file was written, or could not be.
    pub fn smc_exported(&mut self, path: &Path, result: Result<WriteSummary, String>) {
        let file = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        match result {
            Ok(summary) => {
                self.notify(format!(
                    "{file} written: {} model{}, {} elements, {} with a shape. Shapes are \
                     triangles; types, materials, systems and classification codes are \
                     written by name (WIP).",
                    summary.models,
                    if summary.models == 1 { "" } else { "s" },
                    summary.elements,
                    summary.with_shape
                ));
                for (class, count) in summary.as_objects {
                    self.notify(format!(
                        "{file}: {count} elements of {class} are written as plain objects."
                    ));
                }
            }
            Err(e) => self
                .errors
                .push(format!("Could not write the coordination file {file}: {e}")),
        }
    }
}

/// Whose reader it is: shown wherever the feature is offered (the WIP part
/// of Settings).
pub fn credit(ui: &mut egui::Ui) {
    use opencheck_smc as smc;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            RichText::new("Coordination files are read by the reader of ")
                .small()
                .color(theme::text_muted()),
        );
        ui.hyperlink_to(
            RichText::new(format!(
                "{} ({})",
                smc::READER_NAME,
                smc::READER_REVISION_DATE
            ))
            .small(),
            smc::READER_URL,
        );
        ui.label(
            RichText::new(format!(
                " by the {}, under the {}. OpenCheck copies that reader into its crate \
                 opencheck-smc, which is under the same licence, and adds the writing of each \
                 model as IFC. Neither project is affiliated with the maker of any \
                 model checker.",
                smc::READER_AUTHOR,
                smc::READER_LICENSE
            ))
            .small()
            .color(theme::text_muted()),
        );
    });
    egui::CollapsingHeader::new(RichText::new("Notice and licence of the reader").small())
        .id_salt("open_bim_suite_notice")
        .show(ui, |ui| {
            for text in [smc::READER_NOTICE, smc::READER_LICENSE_TEXT] {
                ui.label(RichText::new(text).monospace().small());
                ui.add_space(8.0);
            }
        });
}
