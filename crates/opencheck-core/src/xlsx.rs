//! Excel workbooks (`.xlsx`, Office Open XML) without a spreadsheet library:
//!
//! - [`workbook`] writes a workbook of text and numbers with a few fixed
//!   styles, frozen rows, a filter, print set-up (A4 portrait) and page
//!   header and footer. OpenCheck's standard export template is made with it.
//! - [`fill`] fills a template: any workbook, e.g. one a user changed in
//!   Excel, with fields written as `{{name}}` in its cells and in the page
//!   header and footer. A row with a field of a list (`{{issue.title}}`) is
//!   repeated once per item of the list; everything below it moves down. A
//!   cell that is only a field holding a picture gets the picture, as wide
//!   as its column, and its row grows to fit.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use std::sync::Arc;

use zip::ZipWriter;
use zip::write::SimpleFileOptions;

/// A cell value.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Text(String),
    Number(f64),
    /// A picture placed in the cell (when filling a template).
    Picture(Arc<Picture>),
    Empty,
}

/// A PNG picture and its size in pixels.
#[derive(Debug, PartialEq)]
pub struct Picture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Picture {
    /// A picture from PNG bytes; `None` when they are not a PNG.
    pub fn from_png(png: Vec<u8>) -> Option<Self> {
        // The size is in the header chunk, right after the signature.
        if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" || &png[12..16] != b"IHDR" {
            return None;
        }
        let width = u32::from_be_bytes(png[16..20].try_into().ok()?);
        let height = u32::from_be_bytes(png[20..24].try_into().ok()?);
        (width > 0 && height > 0).then_some(Self { png, width, height })
    }
}

impl From<&str> for Cell {
    fn from(s: &str) -> Self {
        if s.is_empty() {
            Cell::Empty
        } else {
            Cell::Text(s.to_owned())
        }
    }
}

impl From<String> for Cell {
    fn from(s: String) -> Self {
        if s.is_empty() {
            Cell::Empty
        } else {
            Cell::Text(s)
        }
    }
}

impl From<f64> for Cell {
    fn from(n: f64) -> Self {
        Cell::Number(n)
    }
}

impl From<usize> for Cell {
    fn from(n: usize) -> Self {
        Cell::Number(n as f64)
    }
}

impl Cell {
    fn as_text(&self) -> String {
        match self {
            Cell::Text(t) => t.clone(),
            Cell::Number(n) => n.to_string(),
            Cell::Picture(_) | Cell::Empty => String::new(),
        }
    }
}

/// Whether a template has the field `name` anywhere, e.g. to take pictures
/// only when it shows them.
pub fn uses_field(template: &[u8], name: &str) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(Cursor::new(template)) else {
        return false;
    };
    for i in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(i) else {
            continue;
        };
        let xml = entry.name().is_ok_and(|n| n.ends_with(".xml"));
        let mut text = String::new();
        if xml && entry.read_to_string(&mut text).is_ok() {
            let texts = if text.contains("<sst") {
                shared_strings(&text).join("")
            } else {
                unesc(&text)
            };
            if tokens(&texts).contains(&name) {
                return true;
            }
        }
    }
    false
}

// ---- Writing ---------------------------------------------------------------

/// The cell formats of [`workbook`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain = 0,
    /// Bold on grey with a border: table headings.
    Heading = 1,
    /// Wrapped, at the top, with a border: table cells.
    Table = 2,
    /// Large and bold: the sheet's title.
    Title = 3,
    /// Bold: labels beside values.
    Label = 4,
    /// Small and grey: notes.
    Note = 5,
}

/// A column: its width in characters.
pub struct Column {
    pub width: f32,
}

/// One sheet of a workbook.
pub struct Sheet {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<(Cell, Style)>>,
    /// Rows kept in view when scrolling, from the top.
    pub freeze_rows: u32,
    /// A filter on this (1-based) row and the rows below it.
    pub filter_row: Option<u32>,
    /// This (1-based) row is printed at the top of every page.
    pub print_title_row: Option<u32>,
    /// Page header and footer in Excel's codes (`&L`, `&C`, `&R`, `&P`, …).
    pub header: String,
    pub footer: String,
}

/// The bytes of a workbook. Every sheet prints on A4 portrait, scaled to the
/// width of the page.
pub fn workbook(sheets: &[Sheet]) -> std::io::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    let mut file = |name: &str, text: &str| -> std::io::Result<()> {
        zip.start_file(name, options)?;
        zip.write_all(text.as_bytes())
    };
    file("[Content_Types].xml", &content_types(sheets.len()))?;
    file("_rels/.rels", ROOT_RELS)?;
    file("xl/workbook.xml", &workbook_xml(sheets))?;
    file("xl/_rels/workbook.xml.rels", &workbook_rels(sheets.len()))?;
    file("xl/styles.xml", STYLES)?;
    for (i, sheet) in sheets.iter().enumerate() {
        file(
            &format!("xl/worksheets/sheet{}.xml", i + 1),
            &sheet_xml(sheet),
        )?;
    }
    Ok(zip.finish()?.into_inner())
}

/// Excel's name of a column: A, B, … Z, AA, AB, …
fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

fn column_index(name: &str) -> usize {
    name.bytes()
        .filter(u8::is_ascii_uppercase)
        .fold(0usize, |acc, b| acc * 26 + (b - b'A') as usize + 1)
        .saturating_sub(1)
}

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters other than tab and line ends are not
            // allowed in XML.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn unesc(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').and_then(|end| {
            let entity = &rest[1..end];
            let c = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ => entity
                    .strip_prefix("#x")
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32),
            };
            c.map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A sheet name as written in formulas: quoted when it is not plain.
fn quoted_sheet(name: &str) -> String {
    if name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        name.to_owned()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

fn sheet_name(name: &str) -> String {
    // At most 31 characters, none of []:*?/\
    let name: String = name
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    if name.is_empty() {
        "Sheet".to_owned()
    } else {
        name
    }
}

fn content_types(sheets: usize) -> String {
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#,
    );
    for i in 1..=sheets {
        x.push_str(&format!(
            r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        ));
    }
    x.push_str("</Types>");
    x
}

fn workbook_xml(sheets: &[Sheet]) -> String {
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
    );
    for (i, s) in sheets.iter().enumerate() {
        x.push_str(&format!(
            r#"<sheet name="{}" sheetId="{n}" r:id="rId{n}"/>"#,
            esc(&sheet_name(&s.name)),
            n = i + 1
        ));
    }
    x.push_str("</sheets>");
    let titles: Vec<String> = sheets
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let row = s.print_title_row?;
            Some(format!(
                r#"<definedName name="_xlnm.Print_Titles" localSheetId="{i}">{}!${row}:${row}</definedName>"#,
                esc(&quoted_sheet(&sheet_name(&s.name)))
            ))
        })
        .collect();
    if !titles.is_empty() {
        x.push_str("<definedNames>");
        x.push_str(&titles.concat());
        x.push_str("</definedNames>");
    }
    x.push_str("</workbook>");
    x
}

fn workbook_rels(sheets: usize) -> String {
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 1..=sheets {
        x.push_str(&format!(
            r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#
        ));
    }
    x.push_str(&format!(
        r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
        sheets + 1
    ));
    x
}

fn sheet_xml(sheet: &Sheet) -> String {
    let width = sheet
        .rows
        .iter()
        .map(Vec::len)
        .chain([sheet.columns.len()])
        .max()
        .unwrap_or(1)
        .max(1);
    let last = column_name(width - 1);
    let mut x = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
    );
    x.push_str(r#"<sheetPr><pageSetUpPr fitToPage="1"/></sheetPr>"#);
    x.push_str(r#"<sheetViews><sheetView workbookViewId="0">"#);
    if sheet.freeze_rows > 0 {
        let top = sheet.freeze_rows + 1;
        x.push_str(&format!(
            r#"<pane ySplit="{}" topLeftCell="A{top}" activePane="bottomLeft" state="frozen"/>"#,
            sheet.freeze_rows
        ));
    }
    x.push_str("</sheetView></sheetViews>");
    if !sheet.columns.is_empty() {
        x.push_str("<cols>");
        for (i, c) in sheet.columns.iter().enumerate() {
            x.push_str(&format!(
                r#"<col min="{n}" max="{n}" width="{:.1}" customWidth="1"/>"#,
                c.width,
                n = i + 1
            ));
        }
        x.push_str("</cols>");
    }
    x.push_str("<sheetData>");
    for (r, row) in sheet.rows.iter().enumerate() {
        let n = r + 1;
        x.push_str(&format!(r#"<row r="{n}">"#));
        for (i, (cell, style)) in row.iter().enumerate() {
            let at = format!("{}{n}", column_name(i));
            let s = *style as u8;
            match cell {
                Cell::Text(t) => x.push_str(&format!(
                    r#"<c r="{at}" s="{s}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                    esc(t)
                )),
                Cell::Number(v) if v.is_finite() => {
                    x.push_str(&format!(r#"<c r="{at}" s="{s}"><v>{v}</v></c>"#))
                }
                _ if s != 0 => x.push_str(&format!(r#"<c r="{at}" s="{s}"/>"#)),
                _ => {}
            }
        }
        x.push_str("</row>");
    }
    x.push_str("</sheetData>");
    if let Some(row) = sheet.filter_row {
        let end = sheet.rows.len().max(row as usize);
        x.push_str(&format!(r#"<autoFilter ref="A{row}:{last}{end}"/>"#));
    }
    x.push_str(r#"<pageMargins left="0.5" right="0.5" top="0.75" bottom="0.75" header="0.3" footer="0.3"/>"#);
    // A4 (paper size 9), portrait, as wide as the page and as long as needed.
    x.push_str(
        r#"<pageSetup paperSize="9" orientation="portrait" fitToWidth="1" fitToHeight="0"/>"#,
    );
    if !sheet.header.is_empty() || !sheet.footer.is_empty() {
        x.push_str(&format!(
            "<headerFooter><oddHeader>{}</oddHeader><oddFooter>{}</oddFooter></headerFooter>",
            esc(&sheet.header),
            esc(&sheet.footer)
        ));
    }
    x.push_str("</worksheet>");
    x
}

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

/// Fonts: 0 normal, 1 bold, 2 title, 3 note. Cell formats in the order of
/// [`Style`].
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="4"><font><sz val="10"/><name val="Calibri"/></font><font><b/><sz val="10"/><name val="Calibri"/></font><font><b/><sz val="16"/><name val="Calibri"/></font><font><sz val="9"/><color rgb="FF707070"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFE7E6E6"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="2"><border><left/><right/><top/><bottom/><diagonal/></border><border><left style="thin"><color rgb="FFBFBFBF"/></left><right style="thin"><color rgb="FFBFBFBF"/></right><top style="thin"><color rgb="FFBFBFBF"/></top><bottom style="thin"><color rgb="FFBFBFBF"/></bottom><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="6"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="1" xfId="0" applyFont="1" applyFill="1" applyBorder="1" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf><xf numFmtId="0" fontId="0" fillId="0" borderId="1" xfId="0" applyBorder="1" applyAlignment="1"><alignment vertical="top" wrapText="1"/></xf><xf numFmtId="0" fontId="2" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="0" fontId="3" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"#;

// ---- Filling a template ----------------------------------------------------

/// One item of a list: its fields by name (without the list's prefix).
pub type Fields = BTreeMap<String, Cell>;

/// What fills a template.
#[derive(Debug, Default)]
pub struct Data {
    /// Fields used anywhere: `{{project}}`.
    pub fields: Fields,
    /// Lists by name: `{{issue.title}}` is the field `title` of each item of
    /// the list `issue`.
    pub lists: BTreeMap<String, Vec<Fields>>,
}

type Item<'a> = Option<(&'a str, &'a Fields)>;

impl Data {
    /// The value of `{{name}}`, in a row of an item of a list.
    fn get(&self, name: &str, item: Item) -> Option<Cell> {
        if let Some((list, fields)) = item
            && let Some(field) = name.strip_prefix(list).and_then(|r| r.strip_prefix('.'))
        {
            return fields.get(field).cloned();
        }
        self.fields.get(name).cloned()
    }

    /// The list a text has a field of, if any.
    fn list_in(&self, text: &str) -> Option<&str> {
        tokens(text).into_iter().find_map(|t| {
            let (list, _) = t.split_once('.')?;
            self.lists.get_key_value(list).map(|(k, _)| k.as_str())
        })
    }
}

/// The field names written as `{{name}}` in a text.
fn tokens(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            break;
        };
        out.push(after[..j].trim());
        rest = &after[j + 2..];
    }
    out
}

/// A text with its fields filled in. Fields that are not known are left as
/// written, so a misspelt one shows.
fn substitute(text: &str, data: &Data, item: Item) -> String {
    substitute_with(text, data, item, str::to_owned)
}

/// [`substitute`], each value passed through `value` first.
fn substitute_with(text: &str, data: &Data, item: Item, value: impl Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("{{") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        let Some(j) = after.find("}}") else {
            out.push_str(&rest[i..]);
            return out;
        };
        match data.get(after[..j].trim(), item) {
            Some(cell) => out.push_str(&value(&cell.as_text())),
            None => out.push_str(&rest[i..i + 2 + j + 2]),
        }
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

/// Fills `template` (the bytes of an `.xlsx` file) with `data`.
pub fn fill(template: &[u8], data: &Data) -> Result<Vec<u8>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(template))
        .map_err(|e| format!("not an Excel file: {e}"))?;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        parts.push((name, bytes));
    }
    let text = |name: &str| -> Option<String> {
        parts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    let mut workbook = text("xl/workbook.xml").ok_or("not an Excel workbook")?;
    let shared = text("xl/sharedStrings.xml")
        .map(|x| shared_strings(&x))
        .unwrap_or_default();
    let rels = text("xl/_rels/workbook.xml.rels").unwrap_or_default();

    // Sheet name and (0-based) position per worksheet part.
    let mut sheets: BTreeMap<String, (String, usize)> = BTreeMap::new();
    for (position, sheet) in elements(&workbook, "sheet").into_iter().enumerate() {
        let (Some(name), Some(id)) = (attr(sheet, "name"), attr(sheet, "r:id")) else {
            continue;
        };
        let target = elements(&rels, "Relationship")
            .into_iter()
            .find(|r| attr(r, "Id").as_deref() == Some(id.as_str()))
            .and_then(|r| attr(r, "Target"));
        if let Some(target) = target {
            let target = target.trim_start_matches('/');
            let path = if target.starts_with("xl/") {
                target.to_owned()
            } else {
                format!("xl/{target}")
            };
            sheets.insert(path, (unesc(&name), position));
        }
    }

    let mut pictures: Vec<(String, Vec<Placement>)> = Vec::new();
    for (name, bytes) in &mut parts {
        let Some((sheet, position)) = sheets.get(name.as_str()) else {
            continue;
        };
        let xml = String::from_utf8_lossy(bytes).into_owned();
        let (xml, shifts, placed) = fill_sheet(&xml, &shared, data);
        *bytes = xml.into_bytes();
        workbook = shift_defined_names(&workbook, sheet, *position, &shifts);
        if !placed.is_empty() {
            pictures.push((name.clone(), placed));
        }
    }
    for (sheet, placed) in &pictures {
        add_pictures(&mut parts, sheet, placed);
    }
    // Cells moved: Excel rebuilds its calculation order when it is missing.
    parts.retain(|(n, _)| n != "xl/calcChain.xml");
    for (name, bytes) in &mut parts {
        match name.as_str() {
            "xl/workbook.xml" => *bytes = workbook.clone().into_bytes(),
            "[Content_Types].xml" | "xl/_rels/workbook.xml.rels" => {
                let x = String::from_utf8_lossy(bytes).into_owned();
                let x = remove_elements_with(&x, "Override", "calcChain");
                *bytes = remove_elements_with(&x, "Relationship", "calcChain").into_bytes();
            }
            _ => {}
        }
    }

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default();
    for (name, bytes) in &parts {
        zip.start_file(name.as_str(), options)
            .map_err(|e| e.to_string())?;
        zip.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

/// The texts of `sharedStrings.xml`, rich text runs joined.
fn shared_strings(xml: &str) -> Vec<String> {
    blocks(xml, "si")
        .into_iter()
        .map(|si| blocks(si, "t").into_iter().map(unesc).collect())
        .collect()
}

/// Whether `after` (the text after `<name`) continues the same element name.
fn name_ends(after: &str) -> bool {
    after.starts_with([' ', '/', '>', '\n', '\r', '\t'])
}

/// The start tags `<name …>` / `<name …/>` in `xml`, without the brackets.
fn elements<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        if !name_ends(after) {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end..];
    }
    out
}

/// The contents of every `<name …>contents</name>` in `xml`.
fn blocks<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        if !name_ends(after) {
            rest = after;
            continue;
        }
        let Some(end) = after.find('>') else {
            break;
        };
        if after[..end].ends_with('/') {
            out.push("");
            rest = &after[end..];
            continue;
        }
        let body = &after[end + 1..];
        let Some(stop) = body.find(&close) else {
            break;
        };
        out.push(&body[..stop]);
        rest = &body[stop + close.len()..];
    }
    out
}

/// The value of attribute `name` in a start tag's text.
fn attr(tag: &str, name: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let i = rest.find(name)?;
        let before_ok = i == 0 || rest[..i].ends_with([' ', '\n', '\r', '\t']);
        let after = rest[i + name.len()..].trim_start();
        if before_ok && let Some(after) = after.strip_prefix('=') {
            let after = after.trim_start();
            let quote = after.chars().next()?;
            let value = &after[1..];
            let end = value.find(quote)?;
            return Some(value[..end].to_owned());
        }
        rest = &rest[i + name.len()..];
    }
}

/// A start tag's text without attribute `name`.
fn without_attr(tag: &str, name: &str) -> String {
    let Some(value) = attr(tag, name) else {
        return tag.to_owned();
    };
    for quote in ['"', '\''] {
        let written = format!(" {name}={quote}{value}{quote}");
        if let Some(i) = tag.find(&written) {
            return format!("{}{}", &tag[..i], &tag[i + written.len()..]);
        }
    }
    tag.to_owned()
}

/// Removes the empty elements `<name …/>` that contain `marker`.
fn remove_elements_with(xml: &str, name: &str, marker: &str) -> String {
    let open = format!("<{name} ");
    let mut out = String::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let Some(end) = rest[i..].find("/>") else {
            break;
        };
        let element = &rest[i..i + end + 2];
        out.push_str(&rest[..i]);
        if !element.contains(marker) {
            out.push_str(element);
        }
        rest = &rest[i + end + 2..];
    }
    out.push_str(rest);
    out
}

/// A row of the template that was repeated.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shift {
    row: u32,
    /// Rows added (-1 when the list was empty and the row removed).
    added: i64,
}

/// Where row `row` ends up. The end of a range on a repeated row covers the
/// repeated rows.
fn moved(row: u32, shifts: &[Shift], end_of_range: bool) -> u32 {
    let delta: i64 = shifts
        .iter()
        .filter(|s| s.row < row || (end_of_range && s.row == row))
        .map(|s| s.added)
        .sum();
    (row as i64 + delta).max(1) as u32
}

/// Moves the cell references in a reference text (`A1`, `$A$5:$T$5`,
/// `Sheet!$5:$5`, several separated by spaces or commas).
fn shift_refs(text: &str, shifts: &[Shift]) -> String {
    if shifts.is_empty() {
        return text.to_owned();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte = |k: usize| chars.get(k).map_or(text.len(), |c| c.0);
    let mut out = String::new();
    let mut k = 0;
    let mut in_quote = false;
    while k < chars.len() {
        let c = chars[k].1;
        if c == '\'' {
            in_quote = !in_quote;
        }
        let prev = k.checked_sub(1).map(|p| chars[p].1);
        let boundary = prev.is_none_or(|p| !(p.is_alphanumeric() || p == '_'));
        if !in_quote && boundary && (c == '$' || c.is_ascii_uppercase() || c.is_ascii_digit()) {
            let mut j = k;
            if chars[j].1 == '$' {
                j += 1;
            }
            let letters = j;
            while j < chars.len() && chars[j].1.is_ascii_uppercase() {
                j += 1;
            }
            let letter_count = j - letters;
            if j < chars.len() && chars[j].1 == '$' {
                j += 1;
            }
            let digits = j;
            while j < chars.len() && chars[j].1.is_ascii_digit() {
                j += 1;
            }
            let next = chars.get(j).map(|c| c.1);
            let is_ref = j > digits
                && letter_count <= 3
                && next.is_none_or(|n| !(n.is_alphanumeric() || matches!(n, '_' | '!' | '(')));
            let end = j.max(k + 1);
            if is_ref {
                let row: u32 = text[byte(digits)..byte(j)].parse().unwrap_or(0);
                out.push_str(&text[byte(k)..byte(digits)]);
                out.push_str(&moved(row, shifts, prev == Some(':')).to_string());
            } else {
                out.push_str(&text[byte(k)..byte(end)]);
            }
            k = end;
            continue;
        }
        out.push(c);
        k += 1;
    }
    out
}

/// Moves the references in the workbook's defined names (print area, print
/// titles, filter ranges) that point into `sheet`.
fn shift_defined_names(workbook: &str, sheet: &str, position: usize, shifts: &[Shift]) -> String {
    if shifts.is_empty() {
        return workbook.to_owned();
    }
    // Quoted or not; some programs quote every name.
    let prefixes = [
        format!("{}!", esc(&quoted_sheet(sheet))),
        format!("{}!", esc(sheet)),
        format!("'{}'!", esc(&sheet.replace('\'', "''"))),
        format!("&apos;{}&apos;!", esc(&sheet.replace('\'', "''"))),
    ];
    let mut out = String::new();
    let mut rest = workbook;
    while let Some(i) = rest.find("<definedName ") {
        let Some(open_end) = rest[i..].find('>') else {
            break;
        };
        let tag = &rest[i..i + open_end];
        let body_start = i + open_end + 1;
        let Some(close) = rest[body_start..].find("</definedName>") else {
            break;
        };
        let body = &rest[body_start..body_start + close];
        out.push_str(&rest[..body_start]);
        let local = attr(tag, "localSheetId").and_then(|v| v.parse::<usize>().ok());
        let ours = prefixes.iter().any(|p| body.contains(p.as_str()))
            && local.is_none_or(|l| l == position);
        out.push_str(&if ours {
            shift_refs(body, shifts)
        } else {
            body.to_owned()
        });
        rest = &rest[body_start + close..];
    }
    out.push_str(rest);
    out
}

/// A cell of the template.
struct TemplateCell<'a> {
    /// The start tag's attributes other than `r` and `t`.
    attrs: String,
    column: String,
    /// The contents as written, for cells copied unchanged.
    body: Option<&'a str>,
    kind: Option<String>,
    /// Its text, for cells that hold one.
    text: Option<String>,
}

/// Fills one worksheet. Returns the new XML, the rows repeated and the
/// pictures to place.
fn fill_sheet(xml: &str, shared: &[String], data: &Data) -> (String, Vec<Shift>, Vec<Placement>) {
    let (Some(start), Some(end)) = (xml.find("<sheetData"), xml.find("</sheetData>")) else {
        // No rows (`<sheetData/>`): only the page header and footer.
        return (fill_header_footer(xml, data), Vec::new(), Vec::new());
    };
    let Some(open_end) = xml[start..].find('>') else {
        return (xml.to_owned(), Vec::new(), Vec::new());
    };
    let body_start = start + open_end + 1;
    let widths = column_widths(&xml[..start]);
    let mut placed: Vec<Placement> = Vec::new();

    let mut shifts: Vec<Shift> = Vec::new();
    let mut rows = String::new();
    let mut previous = 0u32;
    let mut rest = &xml[body_start..end];
    while let Some(i) = rest.find("<row") {
        let after = &rest[i..];
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let tag = &after[4..tag_end];
        let (inner, consumed) = if tag.ends_with('/') {
            ("", tag_end + 1)
        } else {
            let Some(close) = after.find("</row>") else {
                break;
            };
            (&after[tag_end + 1..close], close + "</row>".len())
        };
        rest = &after[consumed..];
        let tag = tag.trim_end_matches('/');
        let row: u32 = attr(tag, "r")
            .and_then(|r| r.parse().ok())
            .unwrap_or(previous + 1);
        previous = row;
        let row_attrs = without_attr(tag, "r");
        let cells = parse_cells(inner, shared);
        let list = cells
            .iter()
            .filter_map(|c| c.text.as_deref())
            .find_map(|t| data.list_in(t));
        let items: Vec<Item> = match list {
            Some(list) => data.lists[list]
                .iter()
                .map(|fields| Some((list, fields)))
                .collect(),
            None => vec![None],
        };
        let first = moved(row, &shifts, false);
        for (k, item) in items.iter().enumerate() {
            let r = first + k as u32;
            let mut written = String::new();
            let mut tallest = 0u32;
            for cell in &cells {
                if let Some(picture) = picture_in(cell, data, *item) {
                    let column = column_index(&cell.column);
                    let (width, height) = fit(&picture, widths(column));
                    tallest = tallest.max(height);
                    placed.push(Placement {
                        column,
                        row: r,
                        picture,
                        width,
                        height,
                    });
                    written.push_str(&format!("<c r=\"{}{r}\"{}/>", cell.column, cell.attrs));
                } else {
                    written.push_str(&write_cell(cell, r, data, *item));
                }
            }
            let attrs = if tallest > 0 {
                // Points: 3/4 of a pixel, with the picture's margins.
                let points = (tallest + 2 * PICTURE_MARGIN) as f32 * 0.75;
                let rest = without_attr(&without_attr(&row_attrs, "ht"), "customHeight");
                format!("{rest} ht=\"{points:.1}\" customHeight=\"1\"")
            } else {
                row_attrs.clone()
            };
            rows.push_str(&format!("<row r=\"{r}\"{attrs}>{written}</row>"));
        }
        if list.is_some() {
            shifts.push(Shift {
                row,
                added: items.len() as i64 - 1,
            });
        }
    }

    // The used range is worked out again by Excel.
    let head = remove_elements_with(&xml[..body_start], "dimension", "ref");
    let head = shift_ref_attrs(&head, &shifts);
    let tail = shift_ref_attrs(&xml[end..], &shifts);
    (
        fill_header_footer(&format!("{head}{rows}{tail}"), data),
        shifts,
        placed,
    )
}

/// Space around a picture in its cell, pixels.
const PICTURE_MARGIN: u32 = 4;

/// A picture to place at a cell.
struct Placement {
    /// 0-based column; 1-based row.
    column: usize,
    row: u32,
    picture: Arc<Picture>,
    /// Shown size, pixels.
    width: u32,
    height: u32,
}

/// The picture a cell shows: the cell is only a field holding one.
fn picture_in(cell: &TemplateCell, data: &Data, item: Item) -> Option<Arc<Picture>> {
    let text = cell.text.as_deref()?.trim();
    let fields = tokens(text);
    if fields.len() != 1 || !text.starts_with("{{") || !text.ends_with("}}") {
        return None;
    }
    match data.get(fields[0], item)? {
        Cell::Picture(p) => Some(p),
        _ => None,
    }
}

/// The width in pixels of each (0-based) column, from the sheet's `<cols>`.
fn column_widths(head: &str) -> impl Fn(usize) -> u32 {
    // Excel's default column is 8.43 characters, 64 pixels.
    let ranges: Vec<(usize, usize, f32)> = elements(head, "col")
        .into_iter()
        .filter_map(|c| {
            let min = attr(c, "min")?.parse::<usize>().ok()?;
            let max = attr(c, "max")?.parse::<usize>().ok()?;
            let width = attr(c, "width")?.parse::<f32>().ok()?;
            Some((min, max, width))
        })
        .collect();
    move |column| {
        let n = column + 1;
        ranges
            .iter()
            .find(|(min, max, _)| (*min..=*max).contains(&n))
            .map_or(64, |(_, _, w)| (w * 7.0 + 5.0).round() as u32)
    }
}

/// The size a picture is shown at: the column's width less the margins,
/// keeping its proportions.
fn fit(picture: &Picture, column_px: u32) -> (u32, u32) {
    let width = column_px.saturating_sub(2 * PICTURE_MARGIN).max(16);
    let height = (picture.height as u64 * width as u64 / picture.width.max(1) as u64) as u32;
    (width, height.max(1))
}

/// A part path relative to another part's folder (`../drawings/x.xml`).
fn resolve(from: &str, target: &str) -> String {
    if let Some(absolute) = target.strip_prefix('/') {
        return absolute.to_owned();
    }
    let mut path: Vec<&str> = from.split('/').collect();
    path.pop();
    for piece in target.split('/') {
        match piece {
            ".." => {
                path.pop();
            }
            "." | "" => {}
            p => path.push(p),
        }
    }
    path.join("/")
}

/// The relationships file of a part: `a/b.xml` → `a/_rels/b.xml.rels`.
fn rels_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// A relationship id not used in `rels`.
fn free_id(rels: &str) -> String {
    let used: Vec<u32> = elements(rels, "Relationship")
        .into_iter()
        .filter_map(|r| attr(r, "Id")?.strip_prefix("rId")?.parse().ok())
        .collect();
    format!("rId{}", used.into_iter().max().unwrap_or(0) + 1)
}

const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const DOC_RELS_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// Places pictures on a sheet: in its drawing (the one with a logo, say,
/// or a new one), each picture a part of its own.
fn add_pictures(parts: &mut Vec<(String, Vec<u8>)>, sheet: &str, placed: &[Placement]) {
    let get = |parts: &Vec<(String, Vec<u8>)>, name: &str| -> Option<String> {
        parts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, b)| String::from_utf8_lossy(b).into_owned())
    };
    let set = |parts: &mut Vec<(String, Vec<u8>)>, name: &str, text: String| match parts
        .iter_mut()
        .find(|(n, _)| n == name)
    {
        Some((_, bytes)) => *bytes = text.into_bytes(),
        None => parts.push((name.to_owned(), text.into_bytes())),
    };
    let empty_rels = || {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="{RELS_NS}"></Relationships>"#
        )
    };
    let add_rel = |rels: &str, rel: String| -> String {
        match rels.rfind("</Relationships>") {
            Some(i) => format!("{}{rel}{}", &rels[..i], &rels[i..]),
            None => rels.to_owned(),
        }
    };
    let mut types = get(parts, "[Content_Types].xml").unwrap_or_default();

    // The sheet's drawing, or a new one.
    let sheet_rels_path = rels_of(sheet);
    let mut sheet_rels = get(parts, &sheet_rels_path).unwrap_or_else(empty_rels);
    let existing = elements(&sheet_rels, "Relationship")
        .into_iter()
        .find(|r| attr(r, "Type").is_some_and(|t| t.ends_with("/drawing")))
        .and_then(|r| attr(r, "Target"))
        .map(|t| resolve(sheet, &t));
    let drawing = match existing {
        Some(path) => path,
        None => {
            let path = (1..)
                .map(|n| format!("xl/drawings/drawing{n}.xml"))
                .find(|p| !parts.iter().any(|(n, _)| n == p))
                .unwrap_or_default();
            let id = free_id(&sheet_rels);
            sheet_rels = add_rel(
                &sheet_rels,
                format!(
                    r#"<Relationship Id="{id}" Type="{DOC_RELS_NS}/drawing" Target="../drawings/{}"/>"#,
                    path.rsplit('/').next().unwrap_or_default()
                ),
            );
            set(parts, &sheet_rels_path, sheet_rels.clone());
            types = add_rel_like(
                &types,
                "</Types>",
                &format!(
                    r#"<Override PartName="/{path}" ContentType="application/vnd.openxmlformats-officedocument.drawing+xml"/>"#
                ),
            );
            // The drawing goes after the page set-up, before the parts that
            // must follow it.
            let mut xml = get(parts, sheet).unwrap_or_default();
            if !xml.contains("xmlns:r=") {
                xml = xml.replacen(
                    "<worksheet ",
                    &format!("<worksheet xmlns:r=\"{DOC_RELS_NS}\" "),
                    1,
                );
            }
            let element = format!(r#"<drawing r:id="{id}"/>"#);
            let at = [
                "<legacyDrawing",
                "<legacyDrawingHF",
                "<drawingHF",
                "<picture",
                "<oleObjects",
                "<controls",
                "<webPublishItems",
                "<tableParts",
                "<extLst",
                "</worksheet>",
            ]
            .iter()
            .filter_map(|t| xml.find(t))
            .min();
            if let Some(at) = at {
                xml.insert_str(at, &element);
            }
            set(parts, sheet, xml);
            set(
                parts,
                &path,
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<xdr:wsDr xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"></xdr:wsDr>"#
                    .to_owned(),
            );
            path
        }
    };

    if !types.to_ascii_lowercase().contains(r#"extension="png""#) {
        types = add_rel_like(
            &types,
            "</Types>",
            r#"<Default Extension="png" ContentType="image/png"/>"#,
        );
    }
    set(parts, "[Content_Types].xml", types);

    // The pictures: each its own part, linked from the drawing.
    let drawing_rels_path = rels_of(&drawing);
    let mut drawing_rels = get(parts, &drawing_rels_path).unwrap_or_else(empty_rels);
    let mut anchors = String::new();
    let emu = |px: u32| px as u64 * 9525;
    for (k, p) in placed.iter().enumerate() {
        let media = (1..)
            .map(|n| format!("xl/media/opencheck{n}.png"))
            .find(|m| !parts.iter().any(|(n, _)| n == m))
            .unwrap_or_default();
        parts.push((media.clone(), p.picture.png.clone()));
        let id = free_id(&drawing_rels);
        drawing_rels = add_rel(
            &drawing_rels,
            format!(
                r#"<Relationship Id="{id}" Type="{DOC_RELS_NS}/image" Target="../media/{}"/>"#,
                media.rsplit('/').next().unwrap_or_default()
            ),
        );
        let (cx, cy) = (emu(p.width), emu(p.height));
        let shape = 100_000 + k;
        // The namespaces are declared here too, whatever the drawing uses.
        anchors.push_str(&format!(
            r#"<xdr:oneCellAnchor xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="{DOC_RELS_NS}"><xdr:from><xdr:col>{col}</xdr:col><xdr:colOff>{off}</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>{off}</xdr:rowOff></xdr:from><xdr:ext cx="{cx}" cy="{cy}"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="{shape}" name="Picture {shape}"/><xdr:cNvPicPr><a:picLocks noChangeAspect="1"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="{id}"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>"#,
            col = p.column,
            row = p.row - 1,
            off = emu(PICTURE_MARGIN),
        ));
    }
    set(parts, &drawing_rels_path, drawing_rels);
    let mut xml = get(parts, &drawing).unwrap_or_default();
    // Before the drawing's closing tag, whatever its prefix.
    if let Some(i) = xml.rfind("</") {
        xml.insert_str(i, &anchors);
    }
    set(parts, &drawing, xml);
}

/// `text` with `insert` put before `before`.
fn add_rel_like(text: &str, before: &str, insert: &str) -> String {
    match text.rfind(before) {
        Some(i) => format!("{}{insert}{}", &text[..i], &text[i..]),
        None => text.to_owned(),
    }
}

/// Moves the references in `ref="…"` and `sqref="…"` attributes (filters,
/// merged cells, conditional formats, selections).
fn shift_ref_attrs(xml: &str, shifts: &[Shift]) -> String {
    if shifts.is_empty() {
        return xml.to_owned();
    }
    let mut out = xml.to_owned();
    for name in [" ref=\"", " sqref=\""] {
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(i) = rest.find(name) {
            let value_start = i + name.len();
            let Some(len) = rest[value_start..].find('"') else {
                break;
            };
            result.push_str(&rest[..value_start]);
            result.push_str(&shift_refs(&rest[value_start..value_start + len], shifts));
            rest = &rest[value_start + len..];
        }
        result.push_str(rest);
        out = result;
    }
    out
}

/// Fills the fields in the page header and footer.
fn fill_header_footer(xml: &str, data: &Data) -> String {
    let mut out = xml.to_owned();
    for name in [
        "oddHeader",
        "oddFooter",
        "evenHeader",
        "evenFooter",
        "firstHeader",
        "firstFooter",
    ] {
        let open = format!("<{name}>");
        let close = format!("</{name}>");
        if let (Some(i), Some(j)) = (out.find(&open), out.find(&close))
            && i < j
        {
            let text = unesc(&out[i + open.len()..j]);
            // `&` starts a code there (`&P`, `&R`); a written one is `&&`.
            let filled = esc(&substitute_with(&text, data, None, |v| {
                v.replace('&', "&&")
            }));
            out = format!("{}{filled}{}", &out[..i + open.len()], &out[j..]);
        }
    }
    out
}

fn parse_cells<'a>(row: &'a str, shared: &[String]) -> Vec<TemplateCell<'a>> {
    let mut cells = Vec::new();
    let mut rest = row;
    let mut column = 0usize;
    while let Some(i) = rest.find("<c") {
        let after = &rest[i..];
        if !name_ends(&after[2..]) {
            rest = &after[2..];
            continue;
        }
        let Some(tag_end) = after.find('>') else {
            break;
        };
        let closed = after[..tag_end].ends_with('/');
        let tag = after[2..tag_end].trim_end_matches('/');
        let (body, consumed) = if closed {
            (None, tag_end + 1)
        } else {
            let Some(close) = after.find("</c>") else {
                break;
            };
            (Some(&after[tag_end + 1..close]), close + "</c>".len())
        };
        rest = &after[consumed..];
        let name = attr(tag, "r")
            .map(|r| r.trim_end_matches(|c: char| c.is_ascii_digit()).to_owned())
            .unwrap_or_else(|| column_name(column));
        column = column_index(&name) + 1;
        let kind = attr(tag, "t");
        let text = match (kind.as_deref(), body) {
            (Some("s"), Some(b)) => blocks(b, "v")
                .first()
                .and_then(|v| v.trim().parse::<usize>().ok())
                .and_then(|n| shared.get(n).cloned()),
            (Some("inlineStr"), Some(b)) => Some(blocks(b, "t").into_iter().map(unesc).collect()),
            _ => None,
        };
        cells.push(TemplateCell {
            attrs: without_attr(&without_attr(tag, "r"), "t"),
            column: name,
            body,
            kind,
            text,
        });
    }
    cells
}

fn write_cell(cell: &TemplateCell, row: u32, data: &Data, item: Item) -> String {
    let at = format!("{}{row}", cell.column);
    let attrs = &cell.attrs;
    if let Some(text) = &cell.text
        && text.contains("{{")
    {
        // A cell that is just one field holding a number stays a number.
        let fields = tokens(text);
        let trimmed = text.trim();
        if fields.len() == 1
            && trimmed.starts_with("{{")
            && trimmed.ends_with("}}")
            && let Some(Cell::Number(n)) = data.get(fields[0], item)
            && n.is_finite()
        {
            return format!("<c r=\"{at}\"{attrs}><v>{n}</v></c>");
        }
        let filled = substitute(text, data, item);
        if filled.is_empty() {
            return format!("<c r=\"{at}\"{attrs}/>");
        }
        return format!(
            "<c r=\"{at}\"{attrs} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
            esc(&filled)
        );
    }
    let kind = cell
        .kind
        .as_ref()
        .map(|t| format!(" t=\"{t}\""))
        .unwrap_or_default();
    match cell.body {
        Some(body) => format!("<c r=\"{at}\"{attrs}{kind}>{body}</c>"),
        None => format!("<c r=\"{at}\"{attrs}{kind}/>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8], name: &str) -> String {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        zip.by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    #[test]
    fn column_names() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
        assert_eq!(column_index("A"), 0);
        assert_eq!(column_index("AA"), 26);
    }

    #[test]
    fn references_move_below_repeated_rows() {
        let shifts = [Shift { row: 5, added: 2 }];
        assert_eq!(shift_refs("A1:T5", &shifts), "A1:T7");
        assert_eq!(shift_refs("$A$6", &shifts), "$A$8");
        assert_eq!(shift_refs("Issues!$4:$4", &shifts), "Issues!$4:$4");
        assert_eq!(
            shift_refs("'My sheet'!$A$1:$C$5", &shifts),
            "'My sheet'!$A$1:$C$7"
        );
        assert_eq!(shift_refs("A5", &shifts), "A5");
        assert_eq!(shift_refs("ÆØ A9", &shifts), "ÆØ A11");
    }

    fn template() -> Vec<u8> {
        let t = |s: &str| (Cell::from(s), Style::Table);
        workbook(&[Sheet {
            name: "Issues".into(),
            columns: vec![Column { width: 10.0 }, Column { width: 30.0 }],
            rows: vec![
                vec![(Cell::from("{{project}} – {{count}} issues"), Style::Title)],
                vec![t("No."), t("Title")],
                vec![t("{{issue.no}}"), t("{{issue.title}} ({{nope}})")],
                vec![(Cell::from("End of {{project}}"), Style::Note)],
            ],
            freeze_rows: 2,
            filter_row: Some(2),
            print_title_row: Some(2),
            header: "&LACME &R{{project}}".into(),
            footer: "&C&P / &N".into(),
        }])
        .unwrap()
    }

    fn data(n: usize) -> Data {
        let mut d = Data::default();
        d.fields.insert("project".into(), Cell::from("Tower <A>"));
        d.fields.insert("count".into(), Cell::from(n));
        d.lists.insert(
            "issue".into(),
            (1..=n)
                .map(|i| {
                    Fields::from([
                        ("no".to_owned(), Cell::from(i)),
                        ("title".to_owned(), Cell::from(format!("Duct {i}"))),
                    ])
                })
                .collect(),
        );
        d
    }

    #[test]
    fn fills_and_repeats_rows() {
        let filled = fill(&template(), &data(3)).unwrap();
        let sheet = read(&filled, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("Tower &lt;A&gt; – 3 issues"), "{sheet}");
        assert!(sheet.contains(r#"<c r="A3" s="2"><v>1</v></c>"#), "{sheet}");
        assert!(sheet.contains("Duct 3 ({{nope}})"), "{sheet}");
        assert!(sheet.contains(r#"<c r="A5" s="2"><v>3</v></c>"#), "{sheet}");
        assert!(
            sheet.contains(r#"<row r="6"><c r="A6" s="5" t="inlineStr"><is><t xml:space="preserve">End of Tower"#),
            "{sheet}"
        );
        assert!(sheet.contains(r#"<autoFilter ref="A2:B6"/>"#), "{sheet}");
        assert!(
            sheet.contains("&amp;LACME &amp;RTower &lt;A&gt;"),
            "{sheet}"
        );
        let book = read(&filled, "xl/workbook.xml");
        assert!(book.contains("Issues!$2:$2"), "{book}");
    }

    #[test]
    fn an_empty_list_removes_its_row() {
        let filled = fill(&template(), &data(0)).unwrap();
        let sheet = read(&filled, "xl/worksheets/sheet1.xml");
        assert!(!sheet.contains("{{issue"), "{sheet}");
        assert!(sheet.contains(r#"<row r="3"><c r="A3" s="5""#), "{sheet}");
    }

    /// The start of a PNG: signature and header chunk with the size.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(width.to_be_bytes());
        b.extend(height.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn places_pictures_in_their_cells() {
        let template = workbook(&[Sheet {
            name: "Issues".into(),
            columns: vec![Column { width: 10.0 }, Column { width: 40.0 }],
            rows: vec![
                vec![
                    (Cell::from("No."), Style::Heading),
                    (Cell::from("Picture"), Style::Heading),
                ],
                vec![
                    (Cell::from("{{issue.no}}"), Style::Table),
                    (Cell::from("{{issue.image}}"), Style::Table),
                ],
            ],
            freeze_rows: 0,
            filter_row: None,
            print_title_row: None,
            header: String::new(),
            footer: String::new(),
        }])
        .unwrap();
        assert!(uses_field(&template, "issue.image"));
        assert!(!uses_field(&template, "issue.title"));
        let picture = Arc::new(Picture::from_png(png(960, 600)).unwrap());
        let mut data = data(2);
        for item in data.lists.get_mut("issue").unwrap() {
            item.insert("image".into(), Cell::Picture(picture.clone()));
        }
        let filled = fill(&template, &data).unwrap();
        let sheet = read(&filled, "xl/worksheets/sheet1.xml");
        // Column B is 40 characters, 285 px: the picture 277 × 173 px.
        assert!(
            sheet.contains(r#"<row r="2" ht="135.8" customHeight="1">"#),
            "{sheet}"
        );
        assert!(sheet.contains(r#"<c r="B3" s="2"/>"#), "{sheet}");
        assert!(
            sheet.contains(r#"<drawing r:id="rId1"/></worksheet>"#),
            "{sheet}"
        );
        let rels = read(&filled, "xl/worksheets/_rels/sheet1.xml.rels");
        assert!(
            rels.contains(r#"Target="../drawings/drawing1.xml""#),
            "{rels}"
        );
        let drawing = read(&filled, "xl/drawings/drawing1.xml");
        assert_eq!(drawing.matches("<xdr:oneCellAnchor").count(), 2);
        assert!(drawing.contains("<xdr:col>1</xdr:col>"));
        assert!(drawing.contains("<xdr:row>2</xdr:row>"));
        assert!(
            drawing.contains(&format!(r#"cx="{}""#, 277 * 9525)),
            "{drawing}"
        );
        let drels = read(&filled, "xl/drawings/_rels/drawing1.xml.rels");
        assert!(drels.contains("../media/opencheck2.png"), "{drels}");
        let types = read(&filled, "[Content_Types].xml");
        assert!(
            types.contains(r#"Extension="png""#) && types.contains("/xl/drawings/drawing1.xml")
        );
        let mut zip = zip::ZipArchive::new(Cursor::new(&filled)).unwrap();
        assert!(zip.by_name("xl/media/opencheck1.png").is_ok());
    }

    #[test]
    fn adds_pictures_to_a_drawing_with_a_logo() {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let o = SimpleFileOptions::default();
        let files = [
            (
                "[Content_Types].xml",
                r#"<Types><Default Extension="PNG" ContentType="image/png"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="r"><sheets><sheet name="A" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="ws" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns:r="r"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>{{issue.image}}</t></is></c></row></sheetData><drawing r:id="rId4"/></worksheet>"#,
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                r#"<Relationships><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/drawing" Target="../drawings/drawing7.xml"/></Relationships>"#,
            ),
            (
                "xl/drawings/drawing7.xml",
                r#"<xdr:wsDr><xdr:twoCellAnchor>logo</xdr:twoCellAnchor></xdr:wsDr>"#,
            ),
            (
                "xl/drawings/_rels/drawing7.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="image" Target="../media/image1.png"/></Relationships>"#,
            ),
            ("xl/media/image1.png", "logo"),
        ];
        for (name, text) in files {
            zip.start_file(name, o).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let template = zip.finish().unwrap().into_inner();
        let mut data = data(1);
        let picture = Arc::new(Picture::from_png(png(100, 50)).unwrap());
        data.lists.get_mut("issue").unwrap()[0].insert("image".into(), Cell::Picture(picture));
        let filled = fill(&template, &data).unwrap();
        let drawing = read(&filled, "xl/drawings/drawing7.xml");
        assert!(
            drawing.contains("logo</xdr:twoCellAnchor><xdr:oneCellAnchor"),
            "{drawing}"
        );
        assert!(drawing.ends_with("</xdr:wsDr>"));
        let drels = read(&filled, "xl/drawings/_rels/drawing7.xml.rels");
        assert!(
            drels.contains(r#"Id="rId2""#) && drels.contains("image1.png"),
            "{drels}"
        );
        let sheet = read(&filled, "xl/worksheets/sheet1.xml");
        assert_eq!(sheet.matches("<drawing ").count(), 1, "{sheet}");
        assert!(!read(&filled, "[Content_Types].xml").contains(r#"Extension="png""#));
    }

    #[test]
    fn reads_shared_strings_as_excel_saves_them() {
        // A template as Excel writes it: texts in sharedStrings.xml, rich
        // text runs, a calculation chain.
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let o = SimpleFileOptions::default();
        let files = [
            (
                "[Content_Types].xml",
                r#"<Types><Override PartName="/xl/calcChain.xml" ContentType="x"/></Types>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns:r="r"><sheets><sheet name="Fund" sheetId="1" r:id="rId1"/></sheets><definedNames><definedName name="_xlnm.Print_Area" localSheetId="0">Fund!$A$1:$B$2</definedName></definedNames></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="ws" Target="worksheets/sheet1.xml"/><Relationship Id="rId9" Type="x" Target="calcChain.xml"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                r#"<sst><si><t>Firma ApS</t></si><si><r><t>{{issue.</t></r><r><t>title}}</t></r></si></sst>"#,
            ),
            ("xl/calcChain.xml", "<calcChain/>"),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet><dimension ref="A1:B2"/><sheetData><row r="1" spans="1:2"><c r="A1" s="3" t="s"><v>0</v></c></row><row r="2"><c r="A2" s="4" t="s"><v>1</v></c><c r="B2" s="4"><f>ROW()</f><v>2</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="A1:B1"/></mergeCells></worksheet>"#,
            ),
        ];
        for (name, text) in files {
            zip.start_file(name, o).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        let template = zip.finish().unwrap().into_inner();
        let filled = fill(&template, &data(2)).unwrap();
        let sheet = read(&filled, "xl/worksheets/sheet1.xml");
        assert!(
            sheet.contains(r#"<c r="A1" s="3" t="s"><v>0</v></c>"#),
            "{sheet}"
        );
        assert!(
            sheet.contains(
                r#"<row r="3"><c r="A3" s="4" t="inlineStr"><is><t xml:space="preserve">Duct 2</t>"#
            ),
            "{sheet}"
        );
        assert!(
            sheet.contains(r#"<c r="B3" s="4"><f>ROW()</f><v>2</v></c>"#),
            "{sheet}"
        );
        assert!(!sheet.contains("<dimension"), "{sheet}");
        assert!(sheet.contains(r#"<mergeCell ref="A1:B1"/>"#), "{sheet}");
        let book = read(&filled, "xl/workbook.xml");
        assert!(book.contains("Fund!$A$1:$B$3"), "{book}");
        let mut zip = zip::ZipArchive::new(Cursor::new(&filled)).unwrap();
        assert!(zip.by_name("xl/calcChain.xml").is_err());
        assert!(!read(&filled, "[Content_Types].xml").contains("calcChain"));
        assert!(!read(&filled, "xl/_rels/workbook.xml.rels").contains("calcChain"));
    }
}
