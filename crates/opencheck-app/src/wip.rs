//! Features under development ("WIP").
//!
//! Something new that is not yet ready to rely on is built behind this one
//! switch (File → Settings → WIP). It is off until the user turns it on, and
//! with it off OpenCheck offers only what is stable. A feature moves out of
//! WIP by dropping its `app.wip.on` conditions and its line in [`FEATURES`].
//!
//! In WIP now:
//!
//! - the built-in rulesets marked `work_in_progress` (`checking_ui`,
//!   `ruleset_manager`);
//! - the visual checks whose kind is `work_in_progress` (`visual_ui`);
//! - the check of classification codes against a classification system, on
//!   the card of an information ruleset (`checking_ui`);
//! - Revit files, converted to IFC with rvt-rs (`opencheck-rvt`);
//! - coordination files (`.smc`), converted to IFC with the reader of Open
//!   BIM Suite (`opencheck-smc`).
//!
//! The switch decides what is offered, never what a project holds: a ruleset
//! or check added while WIP was on stays and runs with it off.

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use serde_json::Value;

use crate::app::OpenCheckApp;
use crate::platform::Filter;
use crate::theme;

const SETTING: &str = "wip";

/// The WIP switch and what the features behind it need to keep.
#[derive(Default)]
pub struct Wip {
    /// Features under development are offered.
    pub on: bool,
    /// What the conversions of Revit and coordination files said, newest
    /// last.
    pub conversions: Vec<Conversion>,
    /// The window with `conversions` is open.
    pub conversions_open: bool,
}

/// What one conversion of a Revit or coordination file said.
pub struct Conversion {
    /// The summary: what was converted and what to be careful about.
    pub message: String,
    /// The converter's own warnings, as it words them.
    pub warnings: Vec<String>,
    /// A Revit file; else a coordination file.
    pub revit: bool,
}

impl Wip {
    /// The switch as saved last time; off the first time.
    pub fn saved() -> Self {
        Self {
            on: crate::settings::get(SETTING)
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            ..Self::default()
        }
    }

    fn set(&mut self, on: bool) {
        self.on = on;
        crate::settings::set(SETTING, Value::Bool(on));
    }

    /// A file was converted: keeps what the converter said and shows it.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "nothing is converted in the browser")
    )]
    pub fn converted(&mut self, message: String, warnings: Vec<String>, revit: bool) {
        self.conversions.push(Conversion {
            message,
            warnings,
            revit,
        });
        self.conversions_open = true;
    }

    /// The model files that can be added: IFC, and coordination files
    /// (.smc) and Revit while WIP is on.
    pub fn model_filter(&self) -> Filter<'static> {
        if self.revit() {
            (
                "IFC, SMC or Revit model",
                &["ifc", "IFC", "smc", "SMC", "rvt", "RVT"],
            )
        } else {
            ("IFC model", &["ifc", "IFC"])
        }
    }

    /// Revit files can be opened: WIP is on, in the desktop app.
    pub fn revit(&self) -> bool {
        self.on && CONVERTERS_BUILT_IN
    }

    /// Coordination files can be opened: WIP is on, in the desktop app.
    pub fn smc(&self) -> bool {
        self.on && CONVERTERS_BUILT_IN
    }
}

/// The converters are only built into the desktop app.
const CONVERTERS_BUILT_IN: bool = !crate::platform::IS_WEB;

/// A feature under development, as the Settings dialog lists it.
struct Feature {
    name: &'static str,
    what: &'static str,
}

const FEATURES: &[Feature] = &[
    Feature {
        name: "Rulesets: clearances",
        what: "Three built-in rulesets under \"Work in progress\" on the Check tab and in the \
               Ruleset Manager: Clearance: MEP near structure, Electrical: cable tray \
               clearances and Ventilation: clearances. The rules may miss things or report \
               too much.",
    },
    Feature {
        name: "Visual checks: pipes and ducts",
        what: "Two checks on the Visual Check tab: Dimensions in pipe and duct systems, and \
               Fall of drain pipes.",
    },
    Feature {
        name: "Classification codes",
        what: "A tick on the card of the Information: LOD DK ruleset: also check the \
               classification codes themselves against a classification system (BIMTypeCode \
               2022 DK), that a code is in the list and fits the element. The results get a \
               section of their own. Which IFC classes a code belongs on is OpenCheck's own \
               judgement and still being tuned.",
    },
    REVIT,
    SMC,
];

const REVIT: Feature = Feature {
    name: "Revit files (.rvt)",
    what: "Add a Revit file as a model: it is converted to IFC when it is opened, and the \
           converted IFC is kept in the project. Desktop app only. The converter does not read \
           everything: many elements come as a box of the right size and place rather than \
           their real shape, and some parameters are missing, so clashes and information \
           checks on such a model can be wrong. A window says what was converted.",
};

const SMC: Feature = Feature {
    name: "Coordination files (.smc)",
    what: "Add a coordination file, the file a model checker (Solibri) saves with the IFC \
           models of a project put together: each model in it becomes a model here, with its \
           elements, properties, storeys, colours and shapes as the checker kept them, and \
           is stored in the project as IFC. Desktop app only. Tried on few files so far: \
           other files may hold objects the reader does not know. The original IFC files \
           are the models to rely on: update a model from its IFC file with the ↻ button \
           when you have it. A window says what was converted.",
};

/// The WIP part of File → Settings.
pub fn section(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.label(RichText::new(format!("{}  WIP", icon::FLASK)).strong());
    ui.add_space(4.0);
    ui.label(
        RichText::new(
            "Features that are still being developed. They can be incomplete, give wrong \
             results and change without notice. With WIP off, OpenCheck offers only what is \
             stable.",
        )
        .small()
        .color(theme::text_muted()),
    );
    ui.add_space(6.0);
    let mut on = app.wip.on;
    if ui
        .checkbox(&mut on, "Use features under development")
        .changed()
    {
        app.wip.set(on);
    }
    ui.add_space(6.0);
    for feature in FEATURES {
        ui.label(RichText::new(feature.name).strong().small());
        ui.label(
            RichText::new(feature.what)
                .small()
                .color(theme::text_muted()),
        );
        ui.add_space(4.0);
    }
    ui.label(
        RichText::new(
            "Switching WIP off takes nothing out of a project: a ruleset or check already \
             added stays and still runs.",
        )
        .small()
        .color(theme::text_muted()),
    );
    ui.add_space(4.0);
    #[cfg(not(target_arch = "wasm32"))]
    {
        converter_credit(ui);
        ui.add_space(4.0);
        crate::smc::credit(ui);
    }
}

/// Who the Revit conversion is by: shown wherever it is offered.
#[cfg(not(target_arch = "wasm32"))]
fn converter_credit(ui: &mut egui::Ui) {
    use opencheck_rvt as rvt;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.label(
            RichText::new("Revit files are read by ")
                .small()
                .color(theme::text_muted()),
        );
        ui.hyperlink_to(
            RichText::new(format!(
                "{} {}",
                rvt::CONVERTER_NAME,
                rvt::CONVERTER_VERSION
            ))
            .small(),
            rvt::CONVERTER_URL,
        );
        ui.label(
            RichText::new(format!(
                " by {} ({}), an independent open source project. OpenCheck only calls it. \
                 It is not affiliated with Autodesk; Revit is a trademark of Autodesk, Inc.",
                rvt::CONVERTER_AUTHOR,
                rvt::CONVERTER_LICENSE
            ))
            .small()
            .color(theme::text_muted()),
        );
    });
    egui::CollapsingHeader::new(RichText::new("Licence and notices of rvt-rs").small())
        .id_salt("rvt_rs_notice")
        .show(ui, |ui| {
            for text in [rvt::CONVERTER_NOTICE, rvt::CONVERTER_LICENSE_TEXT] {
                ui.label(RichText::new(text).monospace().small());
                ui.add_space(8.0);
            }
        });
}

/// What the conversions of Revit and coordination files said; opens after
/// each conversion.
pub fn conversions_window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.wip.conversions_open {
        return;
    }
    let any_revit = app.wip.conversions.iter().any(|c| c.revit);
    let mut open = true;
    let mut close = false;
    let dialog = crate::dialog::Dialog {
        id: "revit_conversions",
        icon: icon::FLASK,
        title: "Models converted (WIP)",
        size: egui::vec2(480.0, 300.0),
        min: egui::vec2(320.0, 180.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        for (i, conversion) in app.wip.conversions.iter().enumerate().rev() {
            ui.label(&conversion.message);
            if !conversion.warnings.is_empty() {
                egui::CollapsingHeader::new(
                    RichText::new(format!(
                        "{} notes from the converter",
                        conversion.warnings.len()
                    ))
                    .small(),
                )
                .id_salt(("revit_conversion_notes", i))
                .show(ui, |ui| {
                    for warning in &conversion.warnings {
                        ui.label(RichText::new(warning).small().color(theme::text_muted()));
                        ui.add_space(4.0);
                    }
                });
            }
            ui.add_space(8.0);
        }
        ui.label(
            RichText::new(if any_revit {
                "Reading Revit files is under development. The model in OpenCheck is the \
                 converted IFC, not the Revit model itself."
            } else {
                "Reading coordination files is under development. The models in OpenCheck \
                 are the converted IFC files, as the model checker kept them, not the \
                 original IFC files. Update a model from its own IFC file with the ↻ button \
                 when you have it."
            })
            .small()
            .color(theme::text_muted()),
        );
        ui.add_space(8.0);
        close = ui.button("OK").clicked();
    });
    app.wip.conversions_open = open && !close;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revit_files_are_only_offered_with_wip_on() {
        let off = Wip::default();
        assert!(!off.revit() && !off.smc());
        assert_eq!(off.model_filter().1, ["ifc", "IFC"]);
        let on = Wip {
            on: true,
            ..Wip::default()
        };
        assert!(on.revit() && on.smc());
        assert!(on.model_filter().1.contains(&"rvt"));
        assert!(on.model_filter().1.contains(&"smc"));
    }

    #[test]
    fn a_conversion_opens_its_window() {
        let mut wip = Wip::default();
        wip.converted("K09.rvt was converted".to_owned(), Vec::new(), true);
        assert!(wip.conversions_open);
        assert_eq!(wip.conversions.len(), 1);
        assert!(wip.conversions[0].revit);
    }
}
