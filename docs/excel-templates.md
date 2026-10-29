# Excel export and templates

*Export → Excel (.xlsx)…* in the Issues tab writes the ticked issues (or
all) to an Excel workbook made from a template. Without one of your own,
OpenCheck's standard template is used:

- **Issues**: the project, the date and the number of issues, then one row
  per issue with its number, title, criticality, status, responsible
  disciplines, assignee, description, and per element its Revit ID, name,
  model and IFC GlobalId (one line per element in the cell).
- **Models**: the model files the check is based on: file name, model,
  discipline, revision, the date the file was written (from the IFC file
  itself), when it was imported, schema, application and number of elements.

Both sheets print on A4 portrait, scaled to the width of the page, with the
table's heading row repeated on every page.

## Your own template

1. *File → Settings → Save standard template…* saves the standard template.
2. Change it in Excel: your company's name and logo, colours, fonts, which
   columns and in which order, the page header and footer
   (*Page Layout → Print Titles → Header/Footer*), the paper and orientation.
   Save it as `.xlsx`.
3. *File → Settings → Choose template…* and pick the file. Exports use it
   from then on; *Use the standard template* goes back.

The template is read at each export, so changes to it apply at once. In the
browser version exports use the standard template.

## Fields

A field is written as `{{name}}` in a cell, alone or within text
(`Project: {{project}}`), or in the page header or footer.

- A row with an **issue field** (`{{issue.…}}`) is repeated once for every
  issue, a row with a **file field** (`{{file.…}}`) once for every model
  file. Rows below it move down; a filter, print area or print titles that
  end on the row grow with it.
- A cell that is only a field holding a number (`{{issue.no}}`,
  `{{file.elements}}`) stays a number.
- Fields per element have one line per element, in the same order in every
  such field.
- A field that is not known is left as written, so a misspelt one shows in
  the export.

| Field | Holds |
|---|---|
| `{{project}}` | Project name (from the report settings, or the project file) |
| `{{date}}` | Date of the export |
| `{{issue_count}}` | Number of issues exported |
| `{{model_count}}` | Number of model files |
| `{{rulesets}}` | Rulesets of the check |
| `{{opencheck_version}}` | OpenCheck's version |
| `{{issue.no}}` | Number in the list |
| `{{issue.title}}` | Title |
| `{{issue.criticality}}` | Criticality code: C1, C2 or C3 |
| `{{issue.criticality_text}}` | Criticality with its meaning |
| `{{issue.status}}` | Status |
| `{{issue.responsible}}` | Responsible disciplines |
| `{{issue.assigned_to}}` | Person assigned |
| `{{issue.description}}` | Description |
| `{{issue.rule}}` | Rules of its findings |
| `{{issue.measure}}` | Depth, distance or offset per finding |
| `{{issue.missing_properties}}` | Missing properties (information checks) |
| `{{issue.side}}` | A or B per element |
| `{{issue.revit_id}}` | Revit Element ID per element |
| `{{issue.element}}` | Name per element |
| `{{issue.ifc_class}}` | IFC class per element |
| `{{issue.model}}` | Model per element |
| `{{issue.global_id}}` | IFC GlobalId per element |
| `{{issue.created}}` | Date created |
| `{{issue.modified}}` | Date last changed |
| `{{issue.bcf_status}}` | Status on the BCF server |
| `{{issue.id}}` | Issue ID (the BCF topic's GUID) |
| `{{file.name}}` | File name |
| `{{file.model}}` | Model name in OpenCheck |
| `{{file.discipline}}` | Discipline |
| `{{file.revision}}` | Revision in the project (1, 2, …) |
| `{{file.date}}` | When the file was written, from the IFC file itself |
| `{{file.modified}}` | Last change of the file on disk (desktop) |
| `{{file.imported}}` | When this revision was added to the project |
| `{{file.schema}}` | IFC schema |
| `{{file.application}}` | Application that wrote the file |
| `{{file.elements}}` | Number of elements |
| `{{file.size_mb}}` | File size in MB |

## Limits

- Put each list's row in plain cells, not in an Excel table (*Format as
  Table*); tables are not grown.
- Formulas in a repeated row are copied as they are, not adjusted to their
  new row. Formulas elsewhere are recalculated when the file is opened.
- Merged cells on a repeated row are not repeated.
