//! The Report tab: the reports of the project, built from blocks.
//!
//! One layout throughout: on the left what the report consists of (its
//! revisions and blocks), in the middle the block being edited, on the
//! right how the report looks. The top of the middle holds the report's
//! name, the revision shown and what can be done with it.

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;
use opencheck_core::triage::Category;

use crate::app::OpenCheckApp;
use crate::c402;
use crate::checking::category_of;
use crate::issues::{Issue, IssueStatus};
use crate::reports::{Block, BlockKind, IssueFilter, NewRevision, Report, Revision};
use crate::theme;

/// The report dialogs: small questions before something that matters.
pub enum Dialog {
    NewReport {
        name: String,
        label: String,
    },
    Issue {
        by: String,
        note: String,
    },
    /// New data while the newest revision is still a draft.
    UpdateData,
    DeleteBlock(usize),
}

pub fn show(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    dialogs(app, ui.ctx());
    if app.reports.list.is_empty() {
        egui::CentralPanel::default().show(ui, |ui| empty_state(app, ui));
        return;
    }
    app.reports.selected = app.reports.selected.min(app.reports.list.len() - 1);
    egui::Panel::left("report_contents")
        .resizable(true)
        .default_size(280.0)
        .min_size(220.0)
        .frame(crate::checking_ui::panel_frame())
        .show(ui, |ui| contents_panel(app, ui));
    egui::Panel::right("report_preview")
        .resizable(true)
        .default_size(480.0)
        .min_size(320.0)
        .frame(crate::checking_ui::panel_frame())
        .show(ui, |ui| preview_panel(app, ui));
    egui::CentralPanel::default()
        .frame(crate::checking_ui::panel_frame())
        .show(ui, |ui| editor_panel(app, ui));
}

fn empty_state(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.25);
        ui.label(
            RichText::new(icon::FILE_TEXT)
                .size(48.0)
                .color(theme::border()),
        );
        ui.add_space(8.0);
        ui.label(RichText::new("No reports yet").size(18.0).strong());
        ui.label(
            RichText::new(
                "A report documents the check in words and pictures, after C402: introduction, \
                 basis, the check and a conclusion. It is kept in the project and updated revision \
                 by revision.",
            )
            .color(theme::text_muted()),
        );
        ui.add_space(12.0);
        let button = egui::Button::new(
            RichText::new(format!("{}  Create report", icon::PLUS))
                .strong()
                .color(theme::on_accent()),
        )
        .fill(theme::accent())
        .min_size(egui::vec2(180.0, 34.0));
        if ui.add(button).clicked() {
            app.reports.dialog = Some(Dialog::NewReport {
                name: "K&K-rapport".to_owned(),
                label: "A".to_owned(),
            });
        }
    });
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .small()
            .strong()
            .color(theme::text_muted()),
    );
}

// ---- Left: revisions and blocks --------------------------------------------

fn contents_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        let names: Vec<String> = app.reports.list.iter().map(|r| r.name.clone()).collect();
        egui::ComboBox::from_id_salt("report_pick")
            .selected_text(names.get(app.reports.selected).cloned().unwrap_or_default())
            // A fixed width: one taken from the room available would grow
            // the panel on its first frame.
            .width(200.0)
            .show_ui(ui, |ui| {
                for (i, name) in names.iter().enumerate() {
                    if ui
                        .selectable_label(i == app.reports.selected, name)
                        .clicked()
                    {
                        app.reports.selected = i;
                        app.reports.revision = None;
                        app.reports.block = None;
                    }
                }
            });
        if ui.button(icon::PLUS).on_hover_text("New report…").clicked() {
            app.reports.dialog = Some(Dialog::NewReport {
                name: "K&K-rapport".to_owned(),
                label: "A".to_owned(),
            });
        }
    });
    ui.add_space(8.0);
    section_title(ui, "REVISIONS");
    let Some(report) = app.reports.list.get(app.reports.selected) else {
        return;
    };
    let last = report.revisions.len().saturating_sub(1);
    let shown = app.reports.revision.unwrap_or(last).min(last);
    let mut pick = None;
    for (i, rev) in report.revisions.iter().enumerate().rev() {
        let status = match &rev.issued {
            Some(issued) => format!("issued {}", issued.date.get(..10).unwrap_or("")),
            None => "draft".to_owned(),
        };
        let glyph = if rev.is_locked() {
            icon::LOCK_SIMPLE
        } else {
            icon::PENCIL_SIMPLE
        };
        if ui
            .selectable_label(
                i == shown,
                format!("{glyph}  Rev. {}  ·  {status}", rev.label),
            )
            .clicked()
        {
            pick = Some(i);
        }
    }
    if let Some(i) = pick {
        app.reports.revision = (i != last).then_some(i);
    }
    ui.add_space(10.0);
    section_title(ui, "CONTENTS");
    let editable = shown == last && !report.revisions[last].is_locked();
    let blocks = &report.revisions[shown].blocks;
    let numbers = crate::report::numbering(blocks);
    let mut select = None;
    egui::ScrollArea::vertical()
        .id_salt("report_blocks")
        .max_height(ui.available_height() - 40.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for (i, (block, number)) in blocks.iter().zip(&numbers).enumerate() {
                let name = if block.heading.trim().is_empty() {
                    block.kind.label().to_owned()
                } else {
                    block.heading.trim().to_owned()
                };
                let indent = if block.level <= 1 { "" } else { "    " };
                // A block on a new page is marked before its number.
                let page = if block.new_page {
                    format!("{} ", icon::FILE_ARROW_DOWN)
                } else {
                    String::new()
                };
                let text = RichText::new(format!("{indent}{page}{number}  {name}"));
                let text = if block.level <= 1 {
                    text.strong()
                } else {
                    text
                };
                if ui
                    .add(
                        egui::Button::selectable(app.reports.block == Some(i), text)
                            .frame_when_inactive(false),
                    )
                    .on_hover_text(block.kind.label())
                    .clicked()
                {
                    select = Some(i);
                }
            }
        });
    if let Some(i) = select {
        app.reports.block = Some(i);
        app.reports.scroll_to = Some(i);
    }
    if editable {
        ui.separator();
        ui.horizontal(|ui| {
            ui.menu_button(format!("{}  Add block", icon::PLUS), |ui| {
                for kind in BlockKind::ADDABLE {
                    if ui.button(kind.label()).clicked() {
                        add_block(app, kind);
                        ui.close();
                    }
                }
            });
            let selected = app.reports.block;
            ui.add_enabled_ui(selected.is_some(), |ui| {
                if ui
                    .small_button(icon::ARROW_UP)
                    .on_hover_text("Move up")
                    .clicked()
                {
                    move_block(app, -1);
                }
                if ui
                    .small_button(icon::ARROW_DOWN)
                    .on_hover_text("Move down")
                    .clicked()
                {
                    move_block(app, 1);
                }
                if ui
                    .small_button(RichText::new(icon::TRASH).color(theme::danger()))
                    .on_hover_text("Delete block…")
                    .clicked()
                    && let Some(i) = selected
                {
                    app.reports.dialog = Some(Dialog::DeleteBlock(i));
                }
            });
        });
    }
}

/// The draft revision of the current report, to change.
fn draft(app: &mut OpenCheckApp) -> Option<&mut Revision> {
    let report = app.reports.list.get_mut(app.reports.selected)?;
    let rev = report.revisions.last_mut()?;
    (!rev.is_locked()).then_some(rev)
}

fn add_block(app: &mut OpenCheckApp, kind: BlockKind) {
    let at = app.reports.block.map_or(usize::MAX, |b| b + 1);
    let Some(rev) = draft(app) else {
        return;
    };
    let heading = match kind {
        BlockKind::Text => "",
        BlockKind::Properties => "Egenskaber",
        BlockKind::Geometry => "Geometri",
        BlockKind::Clashes => "Kollisioner",
        other => other.label(),
    };
    let level = if matches!(kind, BlockKind::Intro) {
        1
    } else {
        2
    };
    let at = at.min(rev.blocks.len());
    rev.blocks.insert(at, Block::new(kind, level, heading, ""));
    app.reports.block = Some(at);
    app.session.dirty = true;
}

fn move_block(app: &mut OpenCheckApp, by: isize) {
    let Some(i) = app.reports.block else {
        return;
    };
    let Some(rev) = draft(app) else {
        return;
    };
    let j = i as isize + by;
    if j < 0 || j as usize >= rev.blocks.len() {
        return;
    }
    rev.blocks.swap(i, j as usize);
    app.reports.block = Some(j as usize);
    app.session.dirty = true;
}

// ---- Middle: the report and the block being edited ---------------------------

fn editor_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some((_, shown)) = app.reports.shown() else {
        return;
    };
    let index = app.reports.selected;
    let last = app.reports.list[index].revisions.len() - 1;
    let editable = shown == last && !app.reports.list[index].revisions[last].is_locked();
    let busy = app.is_capturing();

    // The report, the revision and what can be done with it.
    ui.horizontal(|ui| {
        let report = &mut app.reports.list[index];
        if ui
            .add(
                egui::TextEdit::singleline(&mut report.name)
                    .font(egui::TextStyle::Heading)
                    .desired_width(ui.available_width() - 8.0),
            )
            .changed()
        {
            app.session.dirty = true;
        }
    });
    let rev = &app.reports.list[index].revisions[shown];
    let status = match &rev.issued {
        Some(i) => format!(
            "Rev. {} · issued {} by {}",
            rev.label,
            i.date.get(..10).unwrap_or(""),
            if i.by.is_empty() { "–" } else { &i.by }
        ),
        None => format!("Rev. {} · draft", rev.label),
    };
    let data = format!(
        "data from {} · {} issues · {} models",
        rev.snapshot.taken.get(..10).unwrap_or(""),
        rev.snapshot.issues.len(),
        rev.snapshot.models.len()
    );
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(status).strong());
        ui.label(RichText::new(data).small().color(theme::text_muted()));
    });
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!busy, |ui| {
            if ui
                .button(format!(
                    "{}  Update data → new revision",
                    icon::ARROWS_CLOCKWISE
                ))
                .on_hover_text(
                    "Takes the issues, pictures and models as they are now into the next revision. \
                     Texts, choices and comments come along.",
                )
                .clicked()
            {
                let draft_open = !app.reports.list[index].revisions[last].is_locked();
                if draft_open {
                    app.reports.dialog = Some(Dialog::UpdateData);
                } else {
                    let id = app.reports.list[index].id.clone();
                    app.request_revision(
                        NewRevision::Next {
                            report: id,
                            replace: false,
                        },
                        ui.ctx(),
                    );
                }
            }
        });
        ui.add_enabled_ui(editable && !busy, |ui| {
            if ui
                .button(format!("{}  Issue revision…", icon::SEAL_CHECK))
                .on_hover_text("Locks the revision: it reads the same from then on")
                .clicked()
            {
                let by = app.reports.list[index].intro.prepared_by.clone();
                app.reports.dialog = Some(Dialog::Issue {
                    by,
                    note: String::new(),
                });
            }
        });
        if ui
            .button(format!("{}  Export PDF…", icon::FILE_PDF))
            .on_hover_text("The revision shown as a PDF, page for page as in the preview")
            .clicked()
        {
            export(app, shown, Format::Pdf);
        }
        if ui
            .button(format!("{}  Export Word…", icon::FILE_DOC))
            .on_hover_text("The revision shown as a Word document made from the report template")
            .clicked()
        {
            export(app, shown, Format::Word);
        }
    });
    if busy {
        let (done, total) = app.capture.as_ref().map_or((0, 0), |c| c.progress());
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!("Taking pictures {done} / {total}…"));
        });
    }
    if !editable {
        ui.label(
            RichText::new(format!(
                "{}  This revision is issued and locked. Update the data to start the next one.",
                icon::LOCK_SIMPLE
            ))
            .small()
            .color(theme::text_muted()),
        );
    }
    ui.separator();

    let Some(b) = app
        .reports
        .block
        .filter(|&b| b < app.reports.list[index].revisions[shown].blocks.len())
    else {
        ui.add_space(16.0);
        ui.label(
            RichText::new("Select a block on the left to edit it, or click it in the preview.")
                .color(theme::text_muted()),
        );
        return;
    };
    let project = &mut app.issues.report.project;
    let report = &mut app.reports.list[index];
    let mut changed = false;
    egui::ScrollArea::vertical()
        .id_salt("report_editor")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_enabled_ui(editable, |ui| {
                changed |= block_editor(ui, report, shown, b, project);
            });
        });
    if changed {
        app.session.dirty = true;
    }
}

fn block_editor(
    ui: &mut egui::Ui,
    report: &mut Report,
    shown: usize,
    b: usize,
    project: &mut String,
) -> bool {
    let mut changed = false;
    let intro = &mut report.intro;
    let rev = &mut report.revisions[shown];
    let (blocks, snapshot) = (&mut rev.blocks, &mut rev.snapshot);
    let block = &mut blocks[b];
    ui.label(
        RichText::new(block.kind.label())
            .small()
            .color(theme::text_muted()),
    );
    egui::Grid::new("block_head")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Heading");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut block.heading).desired_width(360.0))
                .changed();
            ui.end_row();
            ui.label("Level");
            ui.horizontal(|ui| {
                changed |= ui
                    .selectable_value(&mut block.level, 1, "Chapter")
                    .changed();
                changed |= ui
                    .selectable_value(&mut block.level, 2, "Section")
                    .changed();
            });
            ui.end_row();
            ui.label("Page");
            changed |= ui
                .checkbox(&mut block.new_page, "Start on a new page")
                .on_hover_text("A page break before the block, in the PDF and in Word")
                .changed();
            ui.end_row();
        });
    ui.add_space(8.0);

    let text_label = match block.kind {
        BlockKind::Text | BlockKind::Intro => "Text",
        BlockKind::Software => "How the check was done, besides OpenCheck",
        _ => "Comment above the data",
    };
    let fields = |ui: &mut egui::Ui, rows: &mut [(&str, &mut String)]| -> bool {
        let mut changed = false;
        egui::Grid::new("intro_fields")
            .num_columns(2)
            .spacing([10.0, 6.0])
            .show(ui, |ui| {
                for (label, value) in rows.iter_mut() {
                    ui.label(*label);
                    changed |= ui
                        .add(egui::TextEdit::singleline(*value).desired_width(300.0))
                        .changed();
                    ui.end_row();
                }
            });
        changed
    };
    if block.kind == BlockKind::Intro {
        ui.label(RichText::new("Introduction").strong());
        changed |= fields(
            ui,
            &mut [
                ("Project", project),
                ("Prepared by", &mut intro.prepared_by),
                ("Checked by (optional)", &mut intro.checked_by),
                ("Company", &mut intro.company),
                ("Disciplines", &mut intro.disciplines),
                ("Phase", &mut intro.phase),
            ],
        );
        ui.label(
            RichText::new("Date and revision come from the revision. The fields are kept for the report's next revisions.")
                .small()
                .color(theme::text_muted()),
        );
        ui.add_space(8.0);
    }
    ui.label(RichText::new(text_label).strong());
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut block.body)
                .desired_rows(if block.kind == BlockKind::Text { 12 } else { 4 })
                .desired_width(f32::INFINITY)
                .hint_text("Blank lines start a new paragraph"),
        )
        .changed();
    ui.add_space(8.0);

    match block.kind {
        BlockKind::Models => {
            ui.label(RichText::new("Models in this revision").strong());
            egui::Grid::new("report_models")
                .num_columns(3)
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("File").small().strong());
                    ui.label(RichText::new("Version").small().strong());
                    ui.label(RichText::new("Company").small().strong());
                    ui.end_row();
                    for m in &mut snapshot.models {
                        ui.label(&m.file);
                        ui.label(if m.date.is_empty() {
                            &m.imported
                        } else {
                            &m.date
                        });
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut m.company).desired_width(160.0))
                            .changed();
                        ui.end_row();
                    }
                });
        }
        BlockKind::Software => {
            ui.label(
                RichText::new(format!(
                    "OpenCheck {} with: {}",
                    snapshot.opencheck,
                    if snapshot.rulesets.is_empty() {
                        "–".to_owned()
                    } else {
                        snapshot.rulesets.join(", ")
                    }
                ))
                .small()
                .color(theme::text_muted()),
            );
        }
        kind if kind.shows_issues() => {
            changed |= filter_editor(ui, block, snapshot);
        }
        BlockKind::Visual => {
            ui.label(
                RichText::new(format!("Documented checks  {}", snapshot.checks.len())).strong(),
            );
            if snapshot.checks.is_empty() {
                ui.label(
                    RichText::new(
                        "None in this revision. Document checks on the Visual Check tab,                          then Update data.",
                    )
                    .small()
                    .color(theme::text_muted()),
                );
            }
            for c in &snapshot.checks {
                let mut shown = !block.filter.excluded.contains(&c.id);
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut shown, "").changed() {
                        flip(&mut block.filter.excluded, c.id.clone());
                        changed = true;
                    }
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 5.0, c.verdict.color());
                    ui.label(&c.title);
                    ui.label(
                        RichText::new(c.verdict.label())
                            .small()
                            .color(theme::text_muted()),
                    );
                });
            }
        }
        _ => {}
    }
    changed
}

/// Which issues a block shows, and a comment on each.
fn filter_editor(
    ui: &mut egui::Ui,
    block: &mut Block,
    snapshot: &crate::reports::Snapshot,
) -> bool {
    let mut changed = false;
    let filter = &mut block.filter;
    let toggle = |ui: &mut egui::Ui, on: bool, text: &str| ui.selectable_label(on, text).clicked();
    ui.label(RichText::new("Show issues").strong());
    egui::Grid::new("block_filter")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            ui.label("Criticality");
            ui.horizontal_wrapped(|ui| {
                for c in [Category::C3, Category::C2, Category::C1] {
                    let on = filter.categories.contains(&c);
                    if toggle(ui, on, c.code()) {
                        flip(&mut filter.categories, c);
                        changed = true;
                    }
                }
            });
            ui.end_row();
            ui.label("Status");
            ui.horizontal_wrapped(|ui| {
                for s in IssueStatus::ALL {
                    let on = filter.statuses.contains(&s);
                    if toggle(ui, on, s.label()) {
                        flip(&mut filter.statuses, s);
                        changed = true;
                    }
                }
            });
            ui.end_row();
            let mut disciplines: Vec<Discipline> = snapshot
                .issues
                .iter()
                .flat_map(|i| i.responsible.iter().copied())
                .collect();
            disciplines.sort_by_key(|d| *d as u8);
            disciplines.dedup();
            if !disciplines.is_empty() {
                ui.label("Responsible");
                ui.horizontal_wrapped(|ui| {
                    for d in disciplines {
                        let on = filter.disciplines.contains(&d);
                        if toggle(ui, on, d.code()) {
                            flip(&mut filter.disciplines, d);
                            changed = true;
                        }
                    }
                });
                ui.end_row();
            }
            if !snapshot.rulesets.is_empty() {
                ui.label("Rulesets");
                ui.horizontal_wrapped(|ui| {
                    for r in &snapshot.rulesets {
                        let on = filter.rulesets.contains(r);
                        if toggle(ui, on, r) {
                            flip(&mut filter.rulesets, r.clone());
                            changed = true;
                        }
                    }
                });
                ui.end_row();
            }
        });
    ui.label(
        RichText::new("Nothing chosen in a row shows all of it.")
            .small()
            .color(theme::text_muted()),
    );
    ui.add_space(8.0);

    // The candidates, each to leave out or comment on.
    let candidates: Vec<&Issue> = {
        let probe = Block {
            filter: IssueFilter {
                excluded: Vec::new(),
                ..filter.clone()
            },
            ..block_shape(block.kind)
        };
        probe.issues(snapshot)
    };
    let shown = candidates
        .iter()
        .filter(|i| !block.filter.excluded.contains(&i.guid))
        .count();
    ui.label(RichText::new(format!("Issues  {shown} of {}", candidates.len())).strong());
    if candidates.is_empty() {
        ui.label(
            RichText::new("No issues in this revision match.")
                .small()
                .color(theme::text_muted()),
        );
    }
    for issue in candidates {
        let mut included = !block.filter.excluded.contains(&issue.guid);
        ui.horizontal(|ui| {
            if ui.checkbox(&mut included, "").changed() {
                flip(&mut block.filter.excluded, issue.guid.clone());
                changed = true;
            }
            let code = c402::code(issue.priority);
            ui.label(
                RichText::new(code)
                    .strong()
                    .color(crate::checking_ui::severity_color(issue.priority)),
            );
            ui.label(&issue.title);
        });
        if included {
            let note = block.notes.entry(issue.guid.clone()).or_default();
            ui.indent(("note", &issue.guid), |ui| {
                changed |= ui
                    .add(
                        egui::TextEdit::multiline(note)
                            .desired_rows(1)
                            .desired_width(f32::INFINITY)
                            .hint_text("Comment in the report (optional)"),
                    )
                    .changed();
            });
        }
    }
    block.notes.retain(|_, n| !n.trim().is_empty());
    changed
}

/// A block of `kind` with nothing else set, to ask what it would show.
fn block_shape(kind: BlockKind) -> Block {
    Block {
        kind,
        ..Block::default()
    }
}

fn flip<T: PartialEq>(list: &mut Vec<T>, value: T) {
    if let Some(i) = list.iter().position(|v| *v == value) {
        list.remove(i);
    } else {
        list.push(value);
    }
}

/// The file formats a report revision is exported to.
#[derive(Clone, Copy, PartialEq)]
enum Format {
    Pdf,
    Word,
}

fn export(app: &mut OpenCheckApp, shown: usize, format: Format) {
    let Some(report) = app.reports.current() else {
        return;
    };
    let Some(rev) = report.revisions.get(shown) else {
        return;
    };
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| if r#"\/:*?"<>|"#.contains(c) { '-' } else { c })
            .collect()
    };
    let project = if rev.snapshot.project.is_empty() {
        "OpenCheck".to_owned()
    } else {
        rev.snapshot.project.clone()
    };
    let (extension, filter) = match format {
        Format::Pdf => ("pdf", "PDF"),
        Format::Word => ("docx", "Word document"),
    };
    let name = format!(
        "{}-{}-rev{}.{extension}",
        clean(&project),
        clean(report.name_for(rev)),
        clean(&rev.label)
    );
    let template = match crate::typeset::template(app.report_template.as_ref()) {
        Ok(t) => t,
        Err(e) => {
            app.errors.push(e);
            return;
        }
    };
    let Some(path) = crate::platform::save_file("Export report", (filter, &[extension]), &name)
    else {
        return;
    };
    let bytes = match format {
        Format::Pdf => {
            let mut input = crate::typeset::Input::new(report, shown, &template);
            input.attach(&app.reports.pictures);
            crate::typeset::compile(input)
                .and_then(|pages| crate::typeset::pdf(&pages.doc))
                .map_err(|e| {
                    format!(
                        "Could not typeset the report:
{e}"
                    )
                })
        }
        Format::Word => crate::report_word::build(
            &template.docx,
            &template.layout.page,
            report,
            shown,
            &app.reports.pictures,
        )
        .map_err(|e| format!("Could not write the Word document: {e}")),
    };
    match bytes {
        Ok(bytes) => match crate::platform::write(&path, &bytes) {
            Ok(()) => {
                app.notify(format!(
                    "Report written: {}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ));
                if !crate::platform::IS_WEB {
                    let _ = webbrowser::open(&path.to_string_lossy());
                }
            }
            Err(e) => app.errors.push(format!("Could not export: {e}")),
        },
        Err(e) => app.errors.push(e),
    }
}

// ---- Right: the preview -----------------------------------------------------

const PAPER: Color32 = Color32::WHITE;
const INK: Color32 = Color32::from_rgb(0x20, 0x25, 0x2e);
const MUTED: Color32 = Color32::from_rgb(0x6b, 0x73, 0x80);

/// The setting that keeps whether the preview shows the pages.
pub const PAGES_SETTING: &str = "report_preview_pages";

fn preview_panel(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        section_title(ui, "PREVIEW");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let before = app.report_pages;
            ui.selectable_value(&mut app.report_pages, false, "Draft")
                .on_hover_text("The report's text at once, without pages");
            ui.selectable_value(&mut app.report_pages, true, "Pages")
                .on_hover_text("The pages as they print and export to PDF");
            if app.report_pages != before {
                crate::settings::set(PAGES_SETTING, app.report_pages.into());
            }
        });
    });
    if app.report_pages {
        pages_view(app, ui);
    } else {
        draft_view(app, ui);
    }
}

/// The report set in A4 pages, as the PDF will be. A click on a page selects
/// the block there.
fn pages_view(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    crate::typeset::Preview::update(app, ui.ctx());
    let preview = &app.reports.preview;
    ui.horizontal(|ui| {
        if let Some(pages) = &preview.pages {
            let n = pages.doc.pages().len();
            ui.label(
                RichText::new(format!(
                    "{n} {} · A4",
                    if n == 1 { "page" } else { "pages" }
                ))
                .small()
                .color(theme::text_muted()),
            );
        }
        if preview.working() {
            ui.spinner();
        }
    });
    if let Some(e) = &preview.error {
        ui.label(
            RichText::new(format!("{}  {e}", icon::WARNING))
                .small()
                .color(theme::danger()),
        );
    }
    let Some(pages) = preview.pages.clone() else {
        if preview.error.is_none() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Setting the pages…");
            });
        }
        return;
    };
    let scroll_to = app
        .reports
        .scroll_to
        .take()
        .and_then(|b| pages.anchors.get(b).copied().flatten());
    let mut select = None;
    let mut seen = Vec::new();
    let mut rendered = 0;
    egui::ScrollArea::vertical()
        .id_salt("report_pages_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = (ui.available_width() - 4.0).max(100.0);
            for (n, page) in pages.doc.pages().iter().enumerate() {
                let size = page.frame.size();
                let scale = width / size.x.to_pt() as f32;
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(width, size.y.to_pt() as f32 * scale),
                    egui::Sense::click(),
                );
                if let Some((p, y)) = scroll_to
                    && p == n
                {
                    let top = rect.top() + (y - 24.0).max(0.0) * scale;
                    ui.scroll_to_rect(
                        egui::Rect::from_min_size(
                            egui::pos2(rect.left(), top),
                            egui::vec2(1.0, 1.0),
                        ),
                        Some(egui::Align::TOP),
                    );
                }
                let painter = ui.painter();
                painter.rect_filled(rect.translate(egui::vec2(2.0, 2.0)), 2, theme::border());
                painter.rect_filled(rect, 2, PAPER);
                if ui.is_rect_visible(rect) {
                    seen.push(n);
                    let preview = &mut app.reports.preview;
                    if !preview.textures.contains_key(&n) && rendered < 2 {
                        rendered += 1;
                        if let Some(image) =
                            crate::typeset::render(&pages.doc, n, crate::typeset::SCALE)
                        {
                            let texture = ui.ctx().load_texture(
                                format!("report_page_{n}"),
                                image,
                                egui::TextureOptions::LINEAR,
                            );
                            preview.textures.insert(n, texture);
                        }
                    }
                    match preview.textures.get(&n) {
                        Some(texture) => {
                            ui.painter().image(
                                texture.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                Color32::WHITE,
                            );
                        }
                        None => ui.ctx().request_repaint(),
                    }
                }
                let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    select = pages.block_at(n, (pos.y - rect.top()) / scale);
                }
                ui.add_space(10.0);
            }
        });
    // Keep the pictures of the pages in view and a few more.
    let textures = &mut app.reports.preview.textures;
    if textures.len() > seen.len() + 6 {
        textures.retain(|n, _| seen.contains(n));
    }
    if let Some(b) = select {
        app.reports.block = Some(b);
    }
}

/// The report's text laid out at once, without pages.
fn draft_view(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    let Some((report, shown)) = app.reports.shown() else {
        return;
    };
    let report = report.clone();
    let rev = &report.revisions[shown];
    let numbers = crate::report::numbering(&rev.blocks);
    let mut select = None;
    egui::ScrollArea::vertical()
        .id_salt("report_preview_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::new()
                .fill(PAPER)
                .stroke(egui::Stroke::new(1.0, theme::border()))
                .inner_margin(egui::Margin::same(24))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.visuals_mut().override_text_color = Some(INK);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    let project = if rev.snapshot.project.is_empty() {
                        "Projekt"
                    } else {
                        &rev.snapshot.project
                    };
                    ui.label(RichText::new(project).size(20.0).strong());
                    ui.label(
                        RichText::new(format!(
                            "{} · Konsistens- og kollisionskontrol · Revision {}",
                            report.name_for(rev),
                            rev.label
                        ))
                        .color(MUTED),
                    );
                    ui.add_space(12.0);
                    for (i, (block, number)) in rev.blocks.iter().zip(&numbers).enumerate() {
                        let selected = app.reports.block == Some(i);
                        let response = egui::Frame::new()
                            .stroke(if selected {
                                egui::Stroke::new(1.5, theme::accent())
                            } else {
                                egui::Stroke::NONE
                            })
                            .corner_radius(4)
                            .inner_margin(egui::Margin::same(4))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                preview_block(app, ui, &report, rev, block, number);
                            })
                            .response;
                        if app.reports.scroll_to == Some(i) {
                            response.scroll_to_me(Some(egui::Align::TOP));
                            app.reports.scroll_to = None;
                        }
                        if response.interact(egui::Sense::click()).clicked() {
                            select = Some(i);
                        }
                    }
                });
        });
    if let Some(i) = select {
        app.reports.block = Some(i);
    }
}

fn preview_block(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    report: &Report,
    rev: &Revision,
    block: &Block,
    number: &str,
) {
    if block.new_page {
        // Where the page breaks, as in the pages.
        ui.horizontal(|ui| {
            let line = |ui: &mut egui::Ui| {
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(((ui.available_width() - 70.0) / 2.0).max(10.0), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.center().y,
                    egui::Stroke::new(1.0, MUTED),
                );
            };
            line(ui);
            ui.label(RichText::new("new page").small().color(MUTED));
            line(ui);
        });
    }
    if !block.heading.trim().is_empty() {
        let size = if block.level <= 1 { 16.0 } else { 14.0 };
        ui.add_space(if block.level <= 1 { 10.0 } else { 6.0 });
        ui.label(
            RichText::new(format!("{number}  {}", block.heading.trim()))
                .size(size)
                .strong(),
        );
    }
    let paragraphs = |ui: &mut egui::Ui, text: &str| {
        for p in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
            ui.label(p);
            ui.add_space(4.0);
        }
    };
    let snapshot = &rev.snapshot;
    match block.kind {
        BlockKind::Intro => {
            let intro = report.intro_for(rev);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Grid::new(("pv_intro", number))
                .num_columns(2)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    let mut row = |label: &str, value: &str| {
                        ui.label(RichText::new(label).color(MUTED));
                        ui.label(if value.trim().is_empty() {
                            "–"
                        } else {
                            value
                        });
                        ui.end_row();
                    };
                    row("Udarbejdet af", &intro.prepared_by);
                    if !intro.checked_by.trim().is_empty() {
                        row("Kontrolleret af", &intro.checked_by);
                    }
                    row("Firma", &intro.company);
                    row(
                        "Dato",
                        rev.issued
                            .as_ref()
                            .map_or(&rev.created, |i| &i.date)
                            .get(..10)
                            .unwrap_or(""),
                    );
                    row("Revision", &rev.label);
                    row("Fag", &intro.disciplines);
                    row("Fase", &intro.phase);
                });
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            paragraphs(ui, &block.body);
        }
        BlockKind::Text => paragraphs(ui, &block.body),
        BlockKind::Models => {
            paragraphs(ui, &block.body);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            egui::Grid::new(("pv_models", number))
                .num_columns(3)
                .spacing([12.0, 2.0])
                .show(ui, |ui| {
                    for h in ["Bygningsmodel", "Version", "Virksomhed"] {
                        ui.label(RichText::new(h).small().color(MUTED));
                    }
                    ui.end_row();
                    for m in &snapshot.models {
                        ui.label(RichText::new(&m.file).small());
                        ui.label(
                            RichText::new(if m.date.is_empty() {
                                &m.imported
                            } else {
                                &m.date
                            })
                            .small(),
                        );
                        ui.label(
                            RichText::new(if m.company.is_empty() {
                                "–"
                            } else {
                                &m.company
                            })
                            .small(),
                        );
                        ui.end_row();
                    }
                });
        }
        BlockKind::Software => {
            ui.label(format!(
                "Kontrollen er foretaget i OpenCheck {} med følgende regelsæt: {}.",
                snapshot.opencheck,
                if snapshot.rulesets.is_empty() {
                    "–".to_owned()
                } else {
                    snapshot.rulesets.join(", ")
                }
            ));
            paragraphs(ui, &block.body);
        }
        BlockKind::Visual => {
            paragraphs(ui, &block.body);
            let checks = block.checks(snapshot);
            if checks.is_empty() {
                ui.label(
                    RichText::new("Ingen dokumenterede kontroller.")
                        .italics()
                        .color(MUTED),
                );
            }
            for c in checks {
                egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, Color32::from_gray(215)))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let (rect, _) = ui
                                .allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                            ui.painter()
                                .circle_filled(rect.center(), 5.0, c.verdict.color());
                            ui.label(RichText::new(&c.title).strong());
                            ui.label(
                                RichText::new(c.verdict.label_da())
                                    .small()
                                    .color(c.verdict.color()),
                            );
                        });
                        ui.label(
                            RichText::new(format!("{} · {}", c.check, c.summary))
                                .small()
                                .color(MUTED),
                        );
                        paragraphs(ui, &c.about);
                        paragraphs(ui, &c.method);
                        let (shown, more) = crate::check_docs::legend_shown(c);
                        if !shown.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for l in shown {
                                    let [r, g, b] = l.color;
                                    ui.label(
                                        RichText::new("■")
                                            .small()
                                            .color(Color32::from_rgb(r, g, b)),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} {}  ",
                                            crate::check_docs::swatch_text(l),
                                            l.count
                                        ))
                                        .small(),
                                    );
                                }
                                if let Some(more) = more {
                                    ui.label(RichText::new(more).small().color(MUTED));
                                }
                            });
                        }
                        if c.table.len() > 1 {
                            egui::Grid::new(("check_table", &c.id))
                                .striped(true)
                                .spacing([10.0, 2.0])
                                .show(ui, |ui| {
                                    for (i, row) in c.table.iter().enumerate() {
                                        for cell in row {
                                            let text = RichText::new(cell).small();
                                            ui.label(if i == 0 { text.strong() } else { text });
                                        }
                                        ui.end_row();
                                    }
                                });
                        }
                        let width = ((ui.available_width() - 8.0) / 2.0).clamp(60.0, 210.0);
                        ui.horizontal_wrapped(|ui| {
                            for hash in &c.pictures {
                                if let Some(texture) = texture(app, ui.ctx(), hash) {
                                    let size = texture.size_vec2();
                                    ui.add(egui::Image::new(&texture).fit_to_exact_size(
                                        egui::vec2(width, width * size.y / size.x.max(1.0)),
                                    ));
                                }
                            }
                        });
                    });
                ui.add_space(6.0);
            }
        }
        _ => {
            paragraphs(ui, &block.body);
            let issues = block.issues(snapshot);
            if issues.is_empty() {
                ui.label(RichText::new("Ingen emner.").italics().color(MUTED));
                return;
            }
            let count = |c: Category| {
                issues
                    .iter()
                    .filter(|i| category_of(i.priority) == c)
                    .count()
            };
            ui.label(
                RichText::new(format!(
                    "{} emner · C3 {} · C2 {} · C1 {}",
                    issues.len(),
                    count(Category::C3),
                    count(Category::C2),
                    count(Category::C1)
                ))
                .small()
                .color(MUTED),
            );
            for issue in issues {
                preview_issue(app, ui, snapshot, block, issue);
            }
        }
    }
}

fn preview_issue(
    app: &mut OpenCheckApp,
    ui: &mut egui::Ui,
    snapshot: &crate::reports::Snapshot,
    block: &Block,
    issue: &Issue,
) {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, Color32::from_gray(220)))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(c402::code(issue.priority))
                        .strong()
                        .color(Color32::WHITE)
                        .background_color(crate::checking_ui::severity_color(issue.priority)),
                );
                ui.label(RichText::new(&issue.title).strong());
            });
            if let Some(note) = block.notes.get(&issue.guid) {
                ui.label(RichText::new(note).italics());
            }
            ui.horizontal_top(|ui| {
                let width = (ui.available_width() * 0.45).min(240.0);
                match snapshot
                    .pictures
                    .get(&issue.guid)
                    .and_then(|hash| texture(app, ui.ctx(), hash))
                {
                    Some(tex) => {
                        let size = tex.size_vec2();
                        let height = width * size.y / size.x.max(1.0);
                        ui.add(egui::Image::new(&tex).fit_to_exact_size(egui::vec2(width, height)));
                    }
                    None => {
                        ui.add_sized(
                            [width, width * 0.6],
                            egui::Label::new(RichText::new("Intet billede").italics().color(MUTED)),
                        );
                    }
                }
                ui.vertical(|ui| {
                    let responsible: Vec<&str> =
                        issue.responsible.iter().map(|d| d.code()).collect();
                    ui.label(
                        RichText::new(format!(
                            "{}\nStatus: {}\nAnsvarlige: {}\nTildelt: {}",
                            c402::label(issue.priority),
                            crate::report::status_da(issue.status),
                            if responsible.is_empty() {
                                "–".to_owned()
                            } else {
                                responsible.join(", ")
                            },
                            if issue.assigned_to.is_empty() {
                                "–"
                            } else {
                                &issue.assigned_to
                            }
                        ))
                        .small(),
                    );
                    for c in issue.components.iter().take(4) {
                        ui.label(
                            RichText::new(format!(
                                "{}  {}",
                                if c.tag.is_empty() { "–" } else { &c.tag },
                                if c.name.is_empty() {
                                    &c.ifc_class
                                } else {
                                    &c.name
                                }
                            ))
                            .small()
                            .color(MUTED),
                        );
                    }
                });
            });
            if !issue.description.trim().is_empty() {
                ui.label(RichText::new(issue.description.trim()).small());
            }
        });
    ui.add_space(4.0);
}

/// A report picture on the GPU, decoded once.
/// A PNG as an image for the screen.
pub fn png_image(png: &[u8]) -> Option<egui::ColorImage> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png));
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3]
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
}

fn texture(app: &mut OpenCheckApp, ctx: &egui::Context, hash: &str) -> Option<egui::TextureHandle> {
    if let Some(t) = app.reports.textures.get(hash) {
        return Some(t.clone());
    }
    let image = png_image(app.reports.pictures.get(hash)?)?;
    let texture = ctx.load_texture(
        format!("report-{hash}"),
        image,
        egui::TextureOptions::LINEAR,
    );
    app.reports
        .textures
        .insert(hash.to_owned(), texture.clone());
    Some(texture)
}

// ---- Dialogs -------------------------------------------------------------------

fn dialogs(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let Some(dialog) = app.reports.dialog.take() else {
        return;
    };
    let mut keep = true;
    let modal = egui::Modal::new(egui::Id::new("report_dialog")).show(ctx, |ui| {
        ui.set_width(420.0);
        keep = dialog_body(app, ui, &dialog);
    });
    // Escape or a click beside it closes it, whatever it put back.
    if modal.should_close() {
        app.reports.dialog = None;
        return;
    }
    if keep && app.reports.dialog.is_none() {
        app.reports.dialog = Some(dialog);
    }
}

/// Draws one dialog; true to keep it open. A dialog with fields puts
/// itself back with the fields as typed and returns false.
fn dialog_body(app: &mut OpenCheckApp, ui: &mut egui::Ui, dialog: &Dialog) -> bool {
    let ctx = ui.ctx().clone();
    match dialog {
        Dialog::NewReport { name, label } => {
            let (mut name, mut label) = (name.clone(), label.clone());
            ui.heading(RichText::new(format!("{}  New report", icon::FILE_TEXT)).strong());
            ui.label(
                RichText::new(
                    "The report starts with the chapters of C402 and the data of the latest check: \
                     issues with pictures, models and rulesets. Pictures of every issue are taken first.",
                )
                .small()
                .color(theme::text_muted()),
            );
            egui::Grid::new("new_report")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(260.0));
                    ui.end_row();
                    ui.label("First revision");
                    ui.add(egui::TextEdit::singleline(&mut label).desired_width(80.0))
                        .on_hover_text("A as default, or the project's own label, e.g. P01");
                    ui.end_row();
                });
            let mut done = false;
            ui.horizontal(|ui| {
                let create = egui::Button::new(
                    RichText::new(format!("{}  Create", icon::CHECK)).color(theme::on_accent()),
                )
                .fill(theme::accent());
                if ui.add_enabled(!name.trim().is_empty(), create).clicked() {
                    app.request_revision(
                        NewRevision::Report {
                            name: name.trim().to_owned(),
                            label: label.clone(),
                        },
                        &ctx,
                    );
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
            if !done {
                app.reports.dialog = Some(Dialog::NewReport { name, label });
            }
            false
        }
        Dialog::Issue { by, note } => {
            let (mut by, mut note) = (by.clone(), note.clone());
            let label = app
                .reports
                .current()
                .and_then(|r| r.revisions.last())
                .map(|r| r.label.clone())
                .unwrap_or_default();
            ui.heading(
                RichText::new(format!("{}  Issue revision {label}", icon::SEAL_CHECK)).strong(),
            );
            ui.label(
                RichText::new(
                    "The revision is locked: its texts and data read the same from then on. Changes \
                     go into the next revision.",
                )
                .small()
                .color(theme::text_muted()),
            );
            egui::Grid::new("issue_rev")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Issued by");
                    ui.add(egui::TextEdit::singleline(&mut by).desired_width(260.0));
                    ui.end_row();
                    ui.label("Change");
                    ui.add(
                        egui::TextEdit::singleline(&mut note)
                            .desired_width(260.0)
                            .hint_text("e.g. Updated after the check of 14/11"),
                    );
                    ui.end_row();
                });
            let mut done = false;
            ui.horizontal(|ui| {
                let issue = egui::Button::new(
                    RichText::new(format!("{}  Issue and lock", icon::LOCK_SIMPLE))
                        .color(theme::on_accent()),
                )
                .fill(theme::accent());
                if ui.add(issue).clicked() {
                    if let Some(report) = app.reports.list.get_mut(app.reports.selected) {
                        report.issue_draft(crate::reports::Issued {
                            date: opencheck_core::project::now_rfc3339(),
                            by: by.trim().to_owned(),
                            note: note.trim().to_owned(),
                        });
                    }
                    app.session.dirty = true;
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
            if !done {
                app.reports.dialog = Some(Dialog::Issue { by, note });
            }
            false
        }
        Dialog::UpdateData => {
            let label = app
                .reports
                .current()
                .and_then(|r| r.revisions.last())
                .map(|r| r.label.clone())
                .unwrap_or_default();
            ui.heading(
                RichText::new(format!(
                    "{}  Revision {label} is a draft",
                    icon::ARROWS_CLOCKWISE
                ))
                .strong(),
            );
            ui.label(
                "New data can renew the draft's data, or the draft can be locked first so the new \
                 data goes into a new revision.",
            );
            let id = app
                .reports
                .current()
                .map(|r| r.id.clone())
                .unwrap_or_default();
            let mut done = false;
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button(format!("Renew the data of {label}"))
                    .on_hover_text(
                        "Texts and choices stay; the issues, pictures and models are taken anew",
                    )
                    .clicked()
                {
                    app.request_revision(
                        NewRevision::Next {
                            report: id.clone(),
                            replace: true,
                        },
                        &ctx,
                    );
                    done = true;
                }
                if ui
                    .button(format!("Lock {label}, then a new revision"))
                    .clicked()
                {
                    if let Some(report) = app.reports.list.get_mut(app.reports.selected) {
                        report.issue_draft(crate::reports::Issued {
                            date: opencheck_core::project::now_rfc3339(),
                            ..Default::default()
                        });
                    }
                    app.request_revision(
                        NewRevision::Next {
                            report: id.clone(),
                            replace: false,
                        },
                        &ctx,
                    );
                    done = true;
                }
                if ui.button("Cancel").clicked() {
                    done = true;
                }
            });
            !done
        }
        Dialog::DeleteBlock(i) => {
            let name = app
                .reports
                .current()
                .and_then(|r| r.revisions.last())
                .and_then(|r| r.blocks.get(*i))
                .map(|b| {
                    if b.heading.trim().is_empty() {
                        b.kind.label().to_owned()
                    } else {
                        b.heading.clone()
                    }
                })
                .unwrap_or_default();
            ui.heading(RichText::new(format!("{}  Delete \"{name}\"?", icon::TRASH)).strong());
            ui.label("The block and its text and comments leave this revision. Earlier revisions keep theirs.");
            let mut done = false;
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    done = true;
                }
                let delete = egui::Button::new(
                    RichText::new(format!("{}  Delete block", icon::TRASH))
                        .color(theme::on_accent()),
                )
                .fill(theme::danger());
                if ui.add(delete).clicked() {
                    if let Some(rev) = draft(app)
                        && *i < rev.blocks.len()
                    {
                        rev.blocks.remove(*i);
                    }
                    app.reports.block = None;
                    app.session.dirty = true;
                    done = true;
                }
            });
            !done
        }
    }
}
