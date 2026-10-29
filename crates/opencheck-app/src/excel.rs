//! Issues as an Excel workbook, made from a template: OpenCheck's standard
//! one, or the user's own (File → Settings), e.g. the standard one changed
//! in Excel with the company's name, logo, columns and page set-up.
//!
//! Fields are written as `{{name}}` in the template's cells and page header
//! and footer. A row with an issue field (`{{issue.title}}`) is repeated for
//! every issue, a row with a file field (`{{file.name}}`) for every model
//! file; see [`FIELDS`]. `{{issue.image}}` shows the issue's picture, taken
//! before the export only when the template uses it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use opencheck_core::xlsx::{self, Cell, Column, Data, Fields, Picture, Sheet, Style};
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::c402;
use crate::issues::{ComponentRole, Issue, IssueComponent};

/// Settings key of the template's path.
pub const TEMPLATE_SETTING: &str = "excel_template";

/// Every field a template can use, with what it holds.
pub const FIELDS: &[(&str, &str)] = &[
    (
        "project",
        "Project name (from the report settings, or the project file)",
    ),
    ("date", "Date of the export"),
    ("issue_count", "Number of issues exported"),
    ("model_count", "Number of model files"),
    ("rulesets", "Rulesets of the check"),
    ("opencheck_version", "OpenCheck's version"),
    ("issue.no", "Number in the list"),
    ("issue.title", "Title"),
    (
        "issue.image",
        "Picture of the issue's viewpoint, as wide as its column",
    ),
    ("issue.criticality", "Criticality code: C1, C2 or C3"),
    ("issue.criticality_text", "Criticality with its meaning"),
    ("issue.status", "Status"),
    ("issue.responsible", "Responsible disciplines"),
    ("issue.assigned_to", "Person assigned"),
    ("issue.description", "Description"),
    ("issue.rule", "Rules of its findings"),
    ("issue.measure", "Depth, distance or offset per finding"),
    (
        "issue.missing_properties",
        "Missing properties (information checks)",
    ),
    ("issue.side", "A or B per element"),
    ("issue.revit_id", "Revit Element ID per element"),
    ("issue.element", "Name per element"),
    ("issue.ifc_class", "IFC class per element"),
    ("issue.model", "Model per element"),
    ("issue.global_id", "IFC GlobalId per element"),
    ("issue.created", "Date created"),
    ("issue.modified", "Date last changed"),
    ("issue.bcf_status", "Status on the BCF server"),
    ("issue.id", "Issue ID (the BCF topic's GUID)"),
    ("file.name", "File name"),
    ("file.model", "Model name in OpenCheck"),
    ("file.discipline", "Discipline"),
    ("file.revision", "Revision in the project (1, 2, …)"),
    (
        "file.date",
        "When the file was written, from the IFC file itself",
    ),
    ("file.modified", "Last change of the file on disk (desktop)"),
    (
        "file.imported",
        "When this revision was added to the project",
    ),
    ("file.schema", "IFC schema"),
    ("file.application", "Application that wrote the file"),
    ("file.elements", "Number of elements"),
    ("file.size_mb", "File size in MB"),
];

/// OpenCheck's standard template: issues on A4 portrait, then the model
/// files the check is based on.
pub fn standard_template() -> std::io::Result<Vec<u8>> {
    let cell = |s: &str, style: Style| (Cell::from(s), style);
    let heading = |names: &[&str]| -> Vec<(Cell, Style)> {
        names.iter().map(|n| cell(n, Style::Heading)).collect()
    };
    let fields = |names: &[&str]| -> Vec<(Cell, Style)> {
        names
            .iter()
            .map(|n| cell(&format!("{{{{{n}}}}}"), Style::Table))
            .collect()
    };
    let label = |l: &str, field: &str| {
        vec![
            cell(l, Style::Label),
            cell(&format!("{{{{{field}}}}}"), Style::Plain),
        ]
    };
    let issue_columns: [(&str, &str, f32); 12] = [
        ("No.", "issue.no", 5.0),
        ("Title", "issue.title", 22.0),
        ("Picture", "issue.image", 30.0),
        ("Criticality", "issue.criticality", 9.0),
        ("Status", "issue.status", 10.0),
        ("Responsible", "issue.responsible", 13.0),
        ("Assigned to", "issue.assigned_to", 11.0),
        ("Description", "issue.description", 22.0),
        ("Revit ID", "issue.revit_id", 10.0),
        ("Element", "issue.element", 22.0),
        ("Model", "issue.model", 10.0),
        ("IFC GlobalId", "issue.global_id", 24.0),
    ];
    let file_columns: [(&str, &str, f32); 9] = [
        ("File", "file.name", 34.0),
        ("Model", "file.model", 14.0),
        ("Discipline", "file.discipline", 14.0),
        ("Revision", "file.revision", 8.0),
        ("File date", "file.date", 16.0),
        ("Imported", "file.imported", 11.0),
        ("Schema", "file.schema", 9.0),
        ("Application", "file.application", 22.0),
        ("Elements", "file.elements", 9.0),
    ];
    let header = "&L&\"Calibri,Bold\"Company name&R{{project}}".to_owned();
    let footer = "&LIssue list {{date}}&C&P / &N&RMade with OpenCheck".to_owned();
    let issues = Sheet {
        name: "Issues".into(),
        columns: issue_columns
            .iter()
            .map(|c| Column { width: c.2 })
            .collect(),
        rows: vec![
            vec![cell("Issue list", Style::Title)],
            label("Project", "project"),
            label("Date", "date"),
            label("Issues", "issue_count"),
            vec![cell(
                "The model files the check is based on are on the sheet Models.",
                Style::Note,
            )],
            vec![],
            heading(&issue_columns.map(|c| c.0)),
            fields(&issue_columns.map(|c| c.1)),
        ],
        freeze_rows: 7,
        filter_row: Some(7),
        print_title_row: Some(7),
        header: header.clone(),
        footer: footer.clone(),
    };
    let models = Sheet {
        name: "Models".into(),
        columns: file_columns.iter().map(|c| Column { width: c.2 }).collect(),
        rows: vec![
            vec![cell("Models in the check", Style::Title)],
            label("Project", "project"),
            label("Rulesets", "rulesets"),
            vec![],
            heading(&file_columns.map(|c| c.0)),
            fields(&file_columns.map(|c| c.1)),
        ],
        freeze_rows: 5,
        filter_row: None,
        print_title_row: Some(5),
        header,
        footer,
    };
    xlsx::workbook(&[issues, models])
}

impl OpenCheckApp {
    /// The authoring tool's element id of a component (Revit's Element ID):
    /// stored on it, or read from the loaded model.
    pub fn revit_id(&self, c: &IssueComponent) -> String {
        if !c.tag.is_empty() {
            return c.tag.clone();
        }
        self.federation
            .iter()
            .filter(|(_, m)| m.short_name == c.model)
            .find_map(|(_, m)| m.element_by_global_id(&c.global_id))
            .map(crate::issues::element_id)
            .unwrap_or_default()
    }

    /// The template exports use: the user's, or the standard one.
    pub fn excel_template_bytes(&self) -> Result<Vec<u8>, String> {
        match &self.excel_template {
            Some(path) => std::fs::read(path).map_err(|e| {
                format!(
                    "the Excel template {} could not be read ({e}); choose another under File → Settings",
                    path.display()
                )
            }),
            None => standard_template().map_err(|e| e.to_string()),
        }
    }

    /// The workbook for `issues` (GUIDs), in that order, with their
    /// pictures (PNG by GUID; none when the template shows none).
    pub fn excel_file(
        &self,
        issues: &[String],
        images: &FxHashMap<String, Vec<u8>>,
    ) -> Result<Vec<u8>, String> {
        let template = self.excel_template_bytes()?;
        xlsx::fill(&template, &self.excel_data(issues, images)).map_err(|e| {
            format!(
                "the Excel template could not be used ({e}); choose another under File → Settings"
            )
        })
    }

    fn excel_data(&self, issues: &[String], images: &FxHashMap<String, Vec<u8>>) -> Data {
        let mut data = Data::default();
        let project = if self.issues.report.project.trim().is_empty() {
            self.session
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            self.issues.report.project.trim().to_owned()
        };
        let list: Vec<&Issue> = issues.iter().filter_map(|g| self.issues.get(g)).collect();
        let rulesets: Vec<String> = self
            .checking
            .rulesets
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.ruleset.ruleset.name.clone())
            .collect();
        let field = |name: &str, value: Cell| (name.to_owned(), value);
        data.fields = Fields::from([
            field("project", Cell::from(project)),
            field("date", Cell::from(today())),
            field("issue_count", Cell::from(list.len())),
            field("model_count", Cell::from(self.federation.len())),
            field("rulesets", Cell::from(rulesets.join(", "))),
            field("opencheck_version", Cell::from(env!("CARGO_PKG_VERSION"))),
        ]);
        data.lists.insert(
            "issue".into(),
            list.iter()
                .enumerate()
                .map(|(n, issue)| {
                    let mut fields = self.issue_fields(n + 1, issue);
                    let picture = images
                        .get(&issue.guid)
                        .and_then(|png| Picture::from_png(png.clone()));
                    fields.insert(
                        "image".into(),
                        picture.map_or(Cell::Empty, |p| Cell::Picture(Arc::new(p))),
                    );
                    fields
                })
                .collect(),
        );
        data.lists.insert("file".into(), self.file_fields());
        data
    }

    fn issue_fields(&self, number: usize, issue: &Issue) -> Fields {
        let unique = |mut list: Vec<String>| -> Cell {
            list.sort();
            list.dedup();
            Cell::from(list.join("\n"))
        };
        let responsible: Vec<String> = issue
            .responsible
            .iter()
            .map(|d| c402::discipline_label(*d).unwrap_or(d.label()).to_owned())
            .collect();
        // One per finding, in their order.
        let measures: Vec<String> = issue
            .findings
            .iter()
            .filter(|f| f.details.is_empty() && f.magnitude_mm > 0.0)
            .map(|f| {
                let what = match f.kind.as_str() {
                    "Clearance" => "distance",
                    "Duplicate" => "offset",
                    _ => "depth",
                };
                format!("{what} {:.0} mm", f.magnitude_mm)
            })
            .collect();
        // One line per element, in the same order in each of these fields.
        let per_element = |f: &dyn Fn(&IssueComponent) -> String| -> Cell {
            Cell::from(
                issue
                    .components
                    .iter()
                    .map(f)
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        };
        let field = |name: &str, value: Cell| (name.to_owned(), value);
        Fields::from([
            field("no", Cell::from(number)),
            field("title", Cell::from(issue.title.as_str())),
            field("criticality", Cell::from(c402::code(issue.priority))),
            field("criticality_text", Cell::from(c402::label(issue.priority))),
            field("status", Cell::from(issue.status.label())),
            field("responsible", Cell::from(responsible.join(", "))),
            field(
                "assigned_to",
                Cell::from(
                    issue
                        .effective_assignee(self.issues.bcf.as_ref())
                        .unwrap_or(""),
                ),
            ),
            field("description", Cell::from(issue.description.trim())),
            field(
                "rule",
                unique(issue.findings.iter().map(|f| f.rule.clone()).collect()),
            ),
            field("measure", Cell::from(measures.join("\n"))),
            field(
                "missing_properties",
                unique(
                    issue
                        .findings
                        .iter()
                        .flat_map(|f| f.details.clone())
                        .collect(),
                ),
            ),
            field(
                "side",
                per_element(&|c| {
                    match c.role {
                        ComponentRole::A => "A",
                        ComponentRole::B => "B",
                    }
                    .to_owned()
                }),
            ),
            field("revit_id", per_element(&|c| self.revit_id(c))),
            field(
                "element",
                per_element(&|c| {
                    if c.name.is_empty() {
                        c.ifc_class.clone()
                    } else {
                        c.name.clone()
                    }
                }),
            ),
            field("ifc_class", per_element(&|c| c.ifc_class.clone())),
            field("model", per_element(&|c| c.model.clone())),
            field("global_id", per_element(&|c| c.global_id.clone())),
            field("created", Cell::from(date(&issue.created))),
            field("modified", Cell::from(date(&issue.modified))),
            field(
                "bcf_status",
                Cell::from(
                    issue
                        .bcf
                        .as_ref()
                        .map(|b| b.remote_status.clone())
                        .unwrap_or_default(),
                ),
            ),
            field("id", Cell::from(issue.guid.as_str())),
        ])
    }

    /// The model files the check is based on, in the order of the model list.
    fn file_fields(&self) -> Vec<Fields> {
        self.federation
            .iter()
            .map(|(id, model)| {
                let entry = self.session.records.get(&id).map(|r| &r.entry);
                let source = entry
                    .and_then(|e| e.source_path.as_deref())
                    .map(PathBuf::from)
                    .or_else(|| model.source_path.clone());
                let name = source
                    .as_deref()
                    .and_then(Path::file_name)
                    .map(|n| n.to_string_lossy().into_owned())
                    .or_else(|| entry.map(|e| e.file.rsplit('/').next().unwrap_or("").to_owned()))
                    .unwrap_or_else(|| format!("{}.ifc", model.short_name));
                let field = |name: &str, value: Cell| (name.to_owned(), value);
                Fields::from([
                    field("name", Cell::from(name)),
                    field("model", Cell::from(model.short_name.as_str())),
                    field(
                        "discipline",
                        Cell::from(
                            c402::discipline_label(model.discipline)
                                .unwrap_or(model.discipline.label()),
                        ),
                    ),
                    field(
                        "revision",
                        entry.map_or(Cell::Empty, |e| Cell::from(e.revision as usize)),
                    ),
                    field(
                        "date",
                        Cell::from(
                            model
                                .file_timestamp
                                .as_deref()
                                .map(header_time)
                                .unwrap_or_default(),
                        ),
                    ),
                    field(
                        "modified",
                        Cell::from(source.as_deref().map(file_modified).unwrap_or_default()),
                    ),
                    field(
                        "imported",
                        Cell::from(entry.map(|e| date(&e.updated)).unwrap_or_default()),
                    ),
                    field("schema", Cell::from(model.schema.as_str())),
                    field(
                        "application",
                        Cell::from(model.authoring_application.as_deref().unwrap_or("")),
                    ),
                    field("elements", Cell::from(model.elements.len())),
                    field(
                        "size_mb",
                        Cell::from((model.stats.file_size as f64 / 1e5).round() / 10.0),
                    ),
                ])
            })
            .collect()
    }
}

/// The date part of an RFC 3339 time.
fn date(text: &str) -> String {
    text.get(..10).unwrap_or(text).to_owned()
}

fn today() -> String {
    date(&opencheck_core::project::now_rfc3339())
}

/// The IFC header's time stamp (`2026-10-20T14:03:11`, in the writer's own
/// time) as `2026-10-20 14:03`.
fn header_time(stamp: &str) -> String {
    stamp.get(..16).unwrap_or(stamp).replace('T', " ")
}

/// The date the file on disk last changed; empty in the browser or when the
/// file is not there any more.
fn file_modified(path: &Path) -> String {
    if crate::platform::IS_WEB {
        return String::new();
    }
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| date(&opencheck_core::project::rfc3339(t)))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn standard_template_uses_known_fields_only() {
        let bytes = standard_template().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        for name in ["xl/worksheets/sheet1.xml", "xl/worksheets/sheet2.xml"] {
            zip.by_name(name)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
        }
        let mut rest = text.as_str();
        let mut found = 0;
        while let Some(i) = rest.find("{{") {
            let end = rest[i..].find("}}").unwrap();
            let name = &rest[i + 2..i + end];
            assert!(FIELDS.iter().any(|(f, _)| *f == name), "{name}");
            found += 1;
            rest = &rest[i + end..];
        }
        assert!(found > 20, "{found}");
    }

    /// Sample data: two issues, one with two elements, and a model file.
    fn sample() -> Data {
        // Every field, empty unless set below: as the app fills them.
        let all = |list: &str| -> Fields {
            FIELDS
                .iter()
                .filter_map(|(name, _)| name.strip_prefix(list)?.strip_prefix('.'))
                .map(|f| (f.to_owned(), Cell::Empty))
                .collect()
        };
        let issue = |no: usize, title: &str, ids: &str| {
            let mut fields = all("issue");
            fields.extend([
                ("no".to_owned(), Cell::from(no)),
                ("title".to_owned(), Cell::from(title)),
                ("criticality".to_owned(), Cell::from("C3")),
                ("rule".to_owned(), Cell::from("No clashes")),
                ("revit_id".to_owned(), Cell::from(ids)),
            ]);
            fields
        };
        let mut data = Data::default();
        data.fields
            .insert("project".into(), Cell::from("Tower & Co"));
        data.fields.insert("date".into(), Cell::from("2026-10-29"));
        data.fields.insert("issue_count".into(), Cell::from(2usize));
        data.lists.insert(
            "issue".into(),
            vec![
                {
                    let mut first = issue(1, "Duct vs Beam", "3949807\n3595938");
                    first.insert(
                        "image".into(),
                        Cell::Picture(Arc::new(Picture::from_png(test_png(160, 100)).unwrap())),
                    );
                    first
                },
                issue(2, "Pipe vs Wall", "17"),
            ],
        );
        data.lists.insert(
            "file".into(),
            vec![{
                let mut fields = all("file");
                fields.insert("name".to_owned(), Cell::from("K09_ZKON-model.ifc"));
                fields
            }],
        );
        data
    }

    /// A grey PNG of the given size.
    fn test_png(width: u32, height: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![128; (width * height * 4) as usize])
            .unwrap();
        drop(writer);
        out
    }

    fn sheet(bytes: &[u8], name: &str) -> String {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    /// Where the tests leave their workbooks, to open and look at.
    fn out(name: &str, bytes: &[u8]) {
        let dir = std::env::temp_dir().join("opencheck-excel-test");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(name), bytes);
    }

    #[test]
    fn fills_the_standard_template() {
        let template = standard_template().unwrap();
        let filled = xlsx::fill(&template, &sample()).unwrap();
        out("standard-template.xlsx", &template);
        out("standard-filled.xlsx", &filled);
        let issues = sheet(&filled, "xl/worksheets/sheet1.xml");
        assert!(issues.contains("Duct vs Beam"));
        assert!(issues.contains("Pipe vs Wall"));
        assert!(issues.contains("3949807\n3595938"));
        assert!(!issues.contains("{{issue."), "{issues}");
        // The first issue's picture, in column C; the second has none.
        let drawing = sheet(&filled, "xl/drawings/drawing1.xml");
        assert_eq!(drawing.matches("<xdr:oneCellAnchor").count(), 1);
        assert!(drawing.contains("<xdr:col>2</xdr:col>"), "{drawing}");
        assert!(issues.contains(r#"orientation="portrait""#));
        assert!(issues.contains(r#"paperSize="9""#));
        assert!(
            issues.contains("Tower &amp;&amp; Co"),
            "page header: {issues}"
        );
        let models = sheet(&filled, "xl/worksheets/sheet2.xml");
        assert!(models.contains("K09_ZKON-model.ifc"));
    }

    /// A template changed and saved by another program (openpyxl, which
    /// like Excel keeps texts in sharedStrings.xml): a company name, an
    /// extra column, another page header.
    #[test]
    fn fills_a_template_changed_elsewhere() {
        let template = include_bytes!("../testdata/changed-template.xlsx");
        let filled = xlsx::fill(template, &sample()).unwrap();
        out("changed-filled.xlsx", &filled);
        let issues = sheet(&filled, "xl/worksheets/sheet1.xml");
        assert!(issues.contains("Duct vs Beam"), "{issues}");
        assert!(issues.contains("Pipe vs Wall"));
        assert!(issues.contains("No clashes"), "the added column: {issues}");
        assert!(!issues.contains("{{issue."), "{issues}");
        assert!(issues.contains("Acme Engineering A/S"));
        assert!(issues.contains(r#"<autoFilter ref="A8:L10""#), "{issues}");
        let book = sheet(&filled, "xl/workbook.xml");
        assert!(book.contains("'Issues'!$A$8:$L$10"), "{book}");
        assert!(book.contains("'Issues'!$8:$8"), "{book}");
    }

    #[test]
    fn header_times() {
        assert_eq!(header_time("2026-10-20T14:03:11"), "2026-10-20 14:03");
        assert_eq!(header_time("2024-01-01"), "2024-01-01");
    }
}
