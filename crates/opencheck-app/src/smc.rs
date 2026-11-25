//! Coordination files (`.smc`): the IFC models of a project as a model
//! checker (Solibri) saved them together.
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

use eframe::egui::{self, RichText};

use crate::theme;

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
                 model as IFC. Solibri is a trademark of Solibri Oy; neither project is \
                 affiliated with it.",
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
