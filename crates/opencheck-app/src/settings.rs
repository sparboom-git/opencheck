//! The user's choices for the app itself, kept in
//! `%APPDATA%/OpenCheck/settings.json` (the theme, the Excel template), and
//! the Settings dialog (File → Settings).

use std::path::PathBuf;

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use serde_json::Value;

use crate::app::OpenCheckApp;
use crate::excel;
use crate::theme;

fn file() -> PathBuf {
    crate::library::folder()
        .parent()
        .map(|p| p.join("settings.json"))
        .unwrap_or_else(|| PathBuf::from("settings.json"))
}

/// A saved setting; `None` when not set (always in the browser).
pub fn get(key: &str) -> Option<Value> {
    std::fs::read_to_string(file())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get(key).cloned())
}

/// Saves a setting; `Value::Null` removes it.
pub fn set(key: &str, value: Value) {
    let path = file();
    let mut all = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    if let Some(map) = all.as_object_mut() {
        if value.is_null() {
            map.remove(key);
        } else {
            map.insert(key.to_owned(), value);
        }
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(
        &path,
        serde_json::to_string_pretty(&all).unwrap_or_default(),
    );
}

/// The Excel template chosen last time.
pub fn saved_excel_template() -> Option<PathBuf> {
    get(excel::TEMPLATE_SETTING)
        .and_then(|v| v.as_str().map(PathBuf::from))
        .filter(|p| !p.as_os_str().is_empty())
}

impl OpenCheckApp {
    fn set_excel_template(&mut self, path: Option<PathBuf>) {
        set(
            excel::TEMPLATE_SETTING,
            path.as_ref().map_or(Value::Null, |p| {
                Value::String(p.to_string_lossy().into_owned())
            }),
        );
        self.excel_template = path;
    }

    /// Writes OpenCheck's standard Excel template, for the user to change.
    fn export_standard_template(&mut self) {
        let Some(path) = crate::platform::save_file(
            "Save the standard Excel template",
            ("Excel workbook", &["xlsx"]),
            "OpenCheck issue template.xlsx",
        ) else {
            return;
        };
        match excel::standard_template().and_then(|bytes| crate::platform::write(&path, &bytes)) {
            Ok(()) => self.notify(format!(
                "Standard template saved: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
            Err(e) => self
                .errors
                .push(format!("Could not save the template: {e}")),
        }
    }
}

/// File → Settings.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.settings_open {
        return;
    }
    let mut open = true;
    let dialog = crate::dialog::Dialog {
        id: "settings",
        icon: icon::GEAR,
        title: "Settings",
        size: egui::vec2(520.0, 560.0),
        min: egui::vec2(360.0, 240.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        ui.label(RichText::new(format!("{}  Excel export", icon::FILE_XLS)).strong());
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Issues are exported to Excel with a template. To use your own (company name, \
                 logo, columns, page header and footer): save the standard template, change it \
                 in Excel and choose it here. Exports then always use it.",
            )
            .small()
            .color(theme::text_muted()),
        );
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Template").color(theme::text_muted()));
            match &app.excel_template {
                Some(path) => {
                    let missing = !crate::platform::IS_WEB && !path.is_file();
                    let text = RichText::new(path.display().to_string()).monospace();
                    ui.label(if missing {
                        text.color(theme::danger())
                    } else {
                        text
                    })
                    .on_hover_text(if missing {
                        "The file is not there; exports fail until you choose another"
                    } else {
                        "Exports use this file"
                    });
                }
                None => {
                    ui.label("Standard (built in)");
                }
            }
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if !crate::platform::IS_WEB
                && ui
                    .button(format!("{}  Choose template…", icon::FOLDER_OPEN))
                    .clicked()
                && let Some(path) = crate::platform::pick_file(
                    "Choose the Excel template",
                    ("Excel workbook", &["xlsx"]),
                )
            {
                app.set_excel_template(Some(path));
            }
            if app.excel_template.is_some()
                && ui
                    .button(format!(
                        "{}  Use the standard template",
                        icon::ARROW_COUNTER_CLOCKWISE
                    ))
                    .clicked()
            {
                app.set_excel_template(None);
            }
            if ui
                .button(format!(
                    "{}  Save standard template…",
                    icon::DOWNLOAD_SIMPLE
                ))
                .on_hover_text("The template OpenCheck uses, to change in Excel")
                .clicked()
            {
                app.export_standard_template();
            }
        });
        if crate::platform::IS_WEB {
            ui.label(
                RichText::new("In the browser exports use the standard template; your own works in the desktop app.")
                    .small()
                    .color(theme::text_muted()),
            );
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Fields a template can use")
            .id_salt("excel_fields")
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Write a field in a cell, or in the page header or footer, as {{name}}. \
                         A row with an issue field is repeated for every issue, a row with a file \
                         field for every model file; rows below it move down. Fields per element \
                         have one line per element.",
                    )
                    .small()
                    .color(theme::text_muted()),
                );
                ui.add_space(4.0);
                egui::Grid::new("excel_field_list")
                    .num_columns(2)
                    .spacing([12.0, 3.0])
                    .striped(true)
                    .show(ui, |ui| {
                        for (name, what) in excel::FIELDS {
                            ui.label(RichText::new(format!("{{{{{name}}}}}")).monospace().small());
                            ui.label(RichText::new(*what).small());
                            ui.end_row();
                        }
                    });
            });
    });
    app.settings_open = open;
}
