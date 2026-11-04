//! A report revision as a Typst document: the descriptive documentation of
//! a consistency and clash check after C402, made of the revision's blocks
//! in order, with the data frozen in it. [`crate::typeset`] turns it into
//! PDF and page pictures.
//!
//! The document is made from a Word template (the standard one or the
//! user's): its page, header and footer, styles and text, with fields such
//! as `{{project}}` filled in. Next to it are the files it includes:
//! `opencheck.typ` (the values and the shapes of the report's parts),
//! `revisions.typ`, `content.typ` and the pictures.
//!
//! The report is written in Danish, the language of that practice.

use opencheck_core::docx::{self, Layout, ParaFormat, RunFormat};

use crate::c402;
use crate::issues::{ComponentRole, Issue, IssueStatus};
use crate::reports::{Block, BlockKind, Report, Revision};

/// The template OpenCheck uses unless the user chose their own: a Word
/// document (made by `scripts/report-template.py`).
pub const STANDARD_TEMPLATE: &[u8] = include_bytes!("../assets/report-template.docx");

/// The setting that holds the path of the user's report template.
pub const TEMPLATE_SETTING: &str = "report_template";

/// The fields a template can use, with what they hold.
pub const FIELDS: &[(&str, &str)] = &[
    ("project", "Project name"),
    ("report", "Report name"),
    ("revision", "Revision label, e.g. A; – while none"),
    (
        "date",
        "Date of the revision, as typed; else issued, or created while a draft",
    ),
    ("status", "Udsendt or Kladde"),
    ("prepared_by", "Prepared by"),
    ("checked_by", "Checked by"),
    ("company", "Company"),
    ("disciplines", "Disciplines"),
    ("phase", "Phase, e.g. Projektforslag (PF)"),
    ("data_date", "Date the data was taken from the check"),
    ("today", "Date of the export"),
    ("opencheck", "OpenCheck version"),
    ("revisions", "The revision table"),
    ("content", "The report's chapters"),
];

fn date(text: &str) -> String {
    text.get(..10).unwrap_or(text).to_owned()
}

pub fn status_da(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "Åben",
        IssueStatus::InProgress => "I gang",
        IssueStatus::Resolved => "Løst",
        IssueStatus::Closed => "Lukket",
    }
}

/// The numbers of the blocks' headings: "1", "2", "2.1", …; empty for a
/// block without a heading.
pub fn numbering(blocks: &[Block]) -> Vec<String> {
    let (mut chapter, mut section) = (0, 0);
    blocks
        .iter()
        .map(|b| {
            if b.heading.trim().is_empty() {
                return String::new();
            }
            if b.level <= 1 {
                chapter += 1;
                section = 0;
                chapter.to_string()
            } else {
                section += 1;
                format!("{}.{section}", chapter.max(1))
            }
        })
        .collect()
}

/// A Typst string literal of a text.
fn s(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A text as a Typst string; a dash when empty.
fn or_dash(text: &str) -> String {
    s(if text.trim().is_empty() {
        "–"
    } else {
        text.trim()
    })
}

/// The label marking where block `i` starts.
pub fn anchor(i: usize) -> String {
    format!("opencheck-block-{i}")
}

/// The path of a picture inside the document.
pub fn picture_path(hash: &str) -> String {
    format!("/pictures/{hash}.png")
}

/// The shapes of the report's parts, in `opencheck.typ`. Templates style
/// them through set and show rules (text, table, heading, …).
const SHAPES: &str = r##"
#let muted = luma(110)
#let accent = rgb("#2b7ff6")
#let mono(t) = text(font: "IBM Plex Mono", size: 0.88em, t)
#let badge(code) = box(
  fill: (C3: rgb("#d64541"), C2: rgb("#e08a1e"), C1: rgb("#7a8594")).at(code, default: gray),
  inset: (x: 5pt, y: 1.5pt), outset: (y: 1pt), radius: 6pt,
  text(fill: white, weight: "semibold", size: 0.85em, code),
)
#let facts(rows) = table(
  columns: (auto, 1fr), stroke: none, inset: (x: 0pt, y: 2.5pt), column-gutter: 12pt,
  ..rows.map(((k, v)) => (text(fill: muted, k), v)).flatten(),
)
#let paragraphs(t) = for p in t { par(p) }
#let light(hex, label) = box(
  fill: rgb(hex), inset: (x: 5pt, y: 1.5pt), outset: (y: 1pt), radius: 6pt,
  text(fill: white, weight: "bold", size: 0.85em, label),
)
#let check-card(title, verdict, hex, rows, about, method, legend, more, tbl, pictures) = block(
  width: 100%, stroke: (left: 3pt + rgb(hex), rest: 0.5pt + luma(205)), radius: 4pt,
  inset: 9pt, above: 10pt, {
  block(breakable: false, below: 8pt, {
    text(weight: "bold", size: 1.05em, title)
    h(6pt)
    light(hex, verdict)
  })
  facts(rows)
  if about.len() > 0 { paragraphs(about) }
  if method.len() > 0 { paragraphs(method) }
  if legend.len() > 0 {
    block(above: 8pt, {
      set text(size: 0.8em)
      for (label, color, count) in legend {
        box(inset: (right: 9pt), [#box(width: 7pt, height: 7pt, radius: 1pt, fill: rgb(color)) #label #text(fill: muted)[#count]])
      }
      if more != none { text(fill: muted, more) }
    })
  }
  if tbl.len() > 1 {
    block(above: 8pt, {
      set text(size: 0.85em)
      table(columns: tbl.at(0).len(), table.header(..tbl.at(0)), ..tbl.slice(1).flatten())
    })
  }
  // Two pictures a row, each kept low, so a check takes little room.
  if pictures.len() > 0 {
    block(above: 8pt, grid(columns: (1fr, 1fr), column-gutter: 8pt, row-gutter: 8pt,
      ..pictures.map(p => block(breakable: false,
        image(p, width: 100%, height: 6.5cm, fit: "contain")))))
  }
})
#let fill-color(rate, accepted) = if accepted { rgb("#2662b0") } else if rate >= 0.999 { rgb("#1e7a4c") } else if rate >= 0.5 { rgb("#e08a1e") } else { rgb("#d64541") }
#let fill-bar(rate, accepted) = box(width: 2.2cm, height: 5pt, fill: luma(225), radius: 2pt,
  align(left + horizon, box(width: 100% * calc.max(calc.min(rate, 1), 0.02), height: 5pt,
    fill: fill-color(rate, accepted), radius: 2pt)))
// Marks an item read from the geometry, not from a property.
#let geo-mark = box(baseline: 0.5pt, polygon(stroke: 0.7pt + muted, (0pt, 5.5pt), (6pt, 5.5pt), (3pt, 0pt)))
// An information check: how it was done and a table per model with its
// building parts and, under each, the items.
#let info-card(title, rows, about, notes, geo-note, tables) = block(
  width: 100%, stroke: (left: 3pt + accent, rest: 0.5pt + luma(205)), radius: 4pt,
  inset: 9pt, above: 10pt, {
  block(breakable: false, below: 8pt, text(weight: "bold", size: 1.05em, title))
  facts(rows)
  if about.len() > 0 { paragraphs(about) }
  for note in notes { par(text(fill: rgb("#e08a1e"), note)) }
  if geo-note != none { par[#geo-mark #geo-note] }
  for (file, elements, percent, rate, accepted, items) in tables {
    block(above: 8pt, {
      set text(size: 0.85em)
      table(columns: (1fr, auto, auto, auto), align: (left, right, right, left),
        stroke: (x: none, y: 0.5pt + luma(215)),
        table.header([#text(weight: "bold", file) #h(4pt) #text(fill: muted, elements)], [],
          text(weight: "bold", percent), fill-bar(rate, accepted)),
        ..items.map(((label, sub, count, pct, r, acc, geo)) => (
          if sub { [#h(10pt)#label#if geo [ #geo-mark]] } else { text(weight: "semibold", label) },
          text(fill: muted, count),
          if acc { text(fill: rgb("#2662b0"), weight: "semibold", pct) } else { pct },
          fill-bar(r, acc),
        )).flatten())
    })
  }
})
#let none-text = text(fill: muted, style: "italic", "Ingen emner.")
#let issue-card(number, title, code, note, picture, rows, description, components) = block(
  width: 100%, stroke: 0.5pt + luma(205), radius: 4pt, inset: 9pt, above: 10pt, {
  block(breakable: false, below: 8pt, {
    text(weight: "semibold", size: 1.05em)[\##number #title]
    h(6pt)
    badge(code)
    if note != none {
      block(width: 100%, fill: luma(244), inset: 6pt, stroke: (left: 2pt + accent), note)
    }
    grid(columns: (42%, 1fr), column-gutter: 12pt,
      if picture != none {
        image(picture, width: 100%)
      } else {
        rect(width: 100%, height: 3.5cm, radius: 3pt, stroke: (paint: luma(200), dash: "dashed"),
          align(center + horizon, text(fill: muted, style: "italic", "Intet billede")))
      },
      facts(rows),
    )
  })
  if description != none { par(description) }
  if components.len() > 0 {
    set text(size: 0.82em)
    table(columns: (auto, auto, 1fr, auto, auto, auto),
      table.header[][Revit ID][Objekt][IFC-klasse][Model][IFC GlobalId],
      ..components.map(((role, tag, name, class, model, id)) =>
        (role, text(weight: "semibold", mono(tag)), name, class, model, mono(id))).flatten())
  }
})
"##;

/// The Typst document of a Word template: its page, header and footer,
/// styles and text, with fields filled in and `{{revisions}}` and
/// `{{content}}` including those parts.
pub fn main(layout: &Layout, report: &Report, index: usize, watermark: bool) -> String {
    let fields = values(report, index);
    let styles = &layout.styles;
    let normal = styles.para_style(None);
    let page = &layout.page;
    let mut t = String::from("#import \"/opencheck.typ\": *\n");
    t.push_str(&format!(
        "#set document(title: {})\n",
        s(&format!(
            "{} – {}",
            field(&fields, "project"),
            field(&fields, "report")
        ))
    ));

    // Page, header and footer.
    let header = |blocks: &[docx::Block]| -> String {
        let mut h = String::new();
        for b in blocks {
            block_of(&mut h, b, layout, &fields);
        }
        h
    };
    // As in Word: the header starts its distance from the top edge and the
    // footer ends its distance from the bottom; text starts below a header
    // taller than the margin.
    let height = |blocks: &[docx::Block]| blocks_height(blocks, layout);
    let first_header = layout
        .header_first
        .as_ref()
        .filter(|_| layout.title_page)
        .unwrap_or(&layout.header);
    let top = page.top.max(page.header + height(&layout.header) + 6.0);
    let first_extra = (page.header + height(first_header) + 6.0 - top).max(0.0);
    let bottom = page.bottom.max(page.footer + height(&layout.footer) + 6.0);
    let head = |blocks: &[docx::Block]| {
        format!(
            "place(top + left, dy: {}pt, block(width: 100%)[{}])",
            page.header,
            header(blocks)
        )
    };
    let foot = |blocks: &[docx::Block]| {
        format!(
            "place(bottom + left, dy: -{}pt, block(width: 100%)[{}])",
            page.footer,
            header(blocks)
        )
    };
    let pick = |first: &Option<Vec<docx::Block>>,
                default: &[docx::Block],
                shape: &dyn Fn(&[docx::Block]) -> String|
     -> String {
        match first.as_ref().filter(|_| layout.title_page) {
            Some(first) => format!(
                "context if here().page() == 1 {{ {} }} else {{ {} }}",
                shape(first),
                shape(default)
            ),
            None => shape(default),
        }
    };
    // A trial print: the watermark across every page, behind the text.
    let background = if watermark {
        format!(
            "place(center + horizon, rotate(-35deg, text(size: 64pt, weight: \"bold\", fill: luma(214), {})))",
            s(crate::reports::PRELIMINARY_DA)
        )
    } else {
        "none".to_owned()
    };
    t.push_str(&format!(
        "#set page(width: {}pt, height: {}pt, margin: (top: {top}pt, bottom: {bottom}pt, left: {}pt, right: {}pt),
           header-ascent: 0pt, footer-descent: 0pt,
  background: {background},
  header: {},
  footer: {})
",
        page.width,
        page.height,
        page.left,
        page.right,
        pick(&layout.header_first, &layout.header, &head),
        pick(&layout.footer_first, &layout.footer, &foot),
    ));
    if first_extra > 0.0 {
        t.push_str(&format!(
            "#v({first_extra}pt)
"
        ));
    }

    // The text and the report's headings in the template's styles.
    t.push_str(&format!(
        "#set text({}, lang: \"da\")\n#set par(spacing: {}pt, leading: 0.55em)\n",
        text_args(&normal.run),
        normal.para.after.unwrap_or(6.0)
    ));
    for (level, name) in [(1, "heading 1"), (2, "heading 2")] {
        let style = styles.named(name).unwrap_or_else(|| {
            let mut s = normal.clone();
            s.run.bold = Some(true);
            s.run.size =
                Some(normal.run.size.unwrap_or(10.0) * if level == 1 { 1.4 } else { 1.15 });
            s
        });
        t.push_str(&format!(
            "#show heading.where(level: {level}): it => {}[#text({})[#it.body]]\n",
            block_open(&style.para, true),
            text_args(&style.run)
        ));
    }
    t.push_str(
        "#set table(stroke: (x: none, y: 0.4pt + luma(215)), inset: (x: 5pt, y: 4pt))\n\
         #show table.cell.where(y: 0): set text(fill: luma(100), weight: \"bold\")\n",
    );

    // The template's own text around the report.
    for b in &layout.body {
        block_of(&mut t, b, layout, &fields);
        t.push('\n');
    }
    t
}

/// About how tall blocks are, in points: a line per paragraph or its
/// tallest picture, with the spacing after.
fn blocks_height(blocks: &[docx::Block], layout: &Layout) -> f32 {
    blocks
        .iter()
        .map(|b| match b {
            docx::Block::Para(p) => {
                let style = layout.styles.para_style(p.style.as_deref());
                let line = style.run.size.unwrap_or(10.0) * 1.25;
                let pictures = p
                    .runs
                    .iter()
                    .filter_map(|r| match r {
                        docx::Run::Picture { height, .. } => Some(*height),
                        _ => None,
                    })
                    .fold(0.0, f32::max);
                let rule = style.para.over(&p.format).rule_below.map_or(0.0, |_| 4.0);
                line.max(pictures) + rule
            }
            docx::Block::Table(t) => t
                .rows
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|cell| blocks_height(cell, layout))
                        .fold(0.0, f32::max)
                })
                .sum(),
        })
        .sum()
}

fn field<'a>(fields: &'a [(&str, String)], name: &str) -> &'a str {
    fields
        .iter()
        .find(|(k, _)| *k == name)
        .map_or("", |(_, v)| v.as_str())
}

/// Typst `text` arguments of a Word character format.
fn text_args(f: &RunFormat) -> String {
    let mut args = Vec::new();
    let font = f.font.as_deref().unwrap_or("Arial");
    args.push(format!("font: ({}, \"IBM Plex Sans\")", s(font)));
    if let Some(size) = f.size {
        args.push(format!("size: {size}pt"));
    }
    if let Some(bold) = f.bold {
        args.push(format!(
            "weight: {}",
            if bold { "\"bold\"" } else { "\"regular\"" }
        ));
    }
    if let Some(italic) = f.italic {
        args.push(format!(
            "style: {}",
            if italic { "\"italic\"" } else { "\"normal\"" }
        ));
    }
    if let Some(color) = &f.color {
        args.push(format!("fill: rgb(\"#{color}\")"));
    }
    args.join(", ")
}

/// The opening of a block with a paragraph's spacing and rule; `[` follows.
fn block_open(p: &ParaFormat, sticky: bool) -> String {
    let mut args = vec!["width: 100%".to_owned()];
    if let Some(before) = p.before {
        args.push(format!("above: {before}pt"));
    }
    if let Some(after) = p.after {
        args.push(format!("below: {after}pt"));
    }
    if let Some((width, color)) = &p.rule_below {
        args.push(format!(
            "stroke: (bottom: {width}pt + rgb(\"#{color}\")), inset: (bottom: 3pt)"
        ));
    }
    if sticky {
        args.push("sticky: true".to_owned());
    }
    format!("block({})", args.join(", "))
}

/// Picture formats Typst can show.
fn shows_picture(part: &str) -> bool {
    let lower = part.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp"]
        .iter()
        .any(|x| lower.ends_with(x))
}

fn block_of(t: &mut String, b: &docx::Block, layout: &Layout, fields: &[(&str, String)]) {
    match b {
        docx::Block::Para(p) => para_of(t, p, layout, fields),
        docx::Block::Table(table) => {
            let columns: Vec<String> =
                if table.widths.iter().all(|w| *w > 0.0) && !table.widths.is_empty() {
                    table.widths.iter().map(|w| format!("{w}pt")).collect()
                } else {
                    let n = table.rows.iter().map(Vec::len).max().unwrap_or(1);
                    vec!["1fr".to_owned(); n]
                };
            t.push_str(&format!(
                "#table(columns: ({},), stroke: none, inset: (x: 0pt, y: 2pt), column-gutter: 8pt,\n",
                columns.join(", ")
            ));
            for row in &table.rows {
                for cell in row {
                    let mut c = String::new();
                    for b in cell {
                        block_of(&mut c, b, layout, fields);
                    }
                    t.push_str(&format!("  [{c}],\n"));
                }
                for _ in row.len()..columns.len() {
                    t.push_str("  [],\n");
                }
            }
            t.push_str(")\n");
        }
    }
}

fn para_of(t: &mut String, p: &docx::Para, layout: &Layout, fields: &[(&str, String)]) {
    let marker = p.text();
    match marker.trim() {
        "{{content}}" => {
            t.push_str("#include \"/content.typ\"\n");
            return;
        }
        "{{revisions}}" => {
            t.push_str("#include \"/revisions.typ\"\n");
            return;
        }
        _ => {}
    }
    let style = layout.styles.para_style(p.style.as_deref());
    let format = style.para.over(&p.format);
    if format.page_break_before == Some(true) {
        t.push_str("#pagebreak(weak: true)");
    }
    let mut inner = String::new();
    for run in &p.runs {
        match run {
            docx::Run::Text(text, f) => {
                let f = style.run.over(f);
                let mut text = docx::substitute(text, fields);
                if f.caps == Some(true) {
                    text = text.to_uppercase();
                }
                let piece = format!("#text({})[#{}]", text_args(&f), s(&text));
                if f.underline == Some(true) {
                    inner.push_str(&format!("#underline[{piece}]"));
                } else {
                    inner.push_str(&piece);
                }
            }
            docx::Run::Tab => inner.push_str("#h(1fr)"),
            docx::Run::Break => inner.push_str("#linebreak()"),
            docx::Run::PageBreak => inner.push_str("#pagebreak(weak: true)"),
            docx::Run::Page(f) => inner.push_str(&format!(
                "#text({})[#context counter(page).display()]",
                text_args(&style.run.over(f))
            )),
            docx::Run::Pages(f) => inner.push_str(&format!(
                "#text({})[#context counter(page).final().first()]",
                text_args(&style.run.over(f))
            )),
            docx::Run::Picture {
                part,
                width,
                height,
            } => {
                if shows_picture(part) {
                    inner.push_str(&format!(
                        "#box(image({}, width: {width}pt, height: {height}pt))",
                        s(&format!("/{part}"))
                    ));
                }
            }
        }
    }
    let align = match format.align.unwrap_or_default() {
        docx::Align::Left => "left",
        docx::Align::Center => "center",
        docx::Align::Right => "right",
        docx::Align::Justify => "left",
    };
    let justify = format.align == Some(docx::Align::Justify);
    if inner.is_empty() {
        // An empty paragraph still takes a line.
        inner = format!("#v({}pt)", style.run.size.unwrap_or(10.0) * 1.2);
    }
    t.push_str(&format!(
        "#{}[#set par(justify: {justify}); #align({align})[{inner}]]",
        block_open(&format, false)
    ));
}

/// The fonts a template names, to load from the computer when it has them.
pub fn fonts(layout: &Layout) -> Vec<String> {
    let mut list = layout.styles.fonts();
    let mut add = |blocks: &[docx::Block]| {
        for b in blocks {
            if let docx::Block::Para(p) = b {
                for r in &p.runs {
                    if let docx::Run::Text(_, f) = r
                        && let Some(font) = &f.font
                    {
                        list.push(font.clone());
                    }
                }
            }
        }
    };
    add(&layout.body);
    add(&layout.header);
    add(&layout.footer);
    list.push("Arial".to_owned());
    list.sort();
    list.dedup();
    list
}

/// The text fields' values.
pub fn values(report: &Report, index: usize) -> Vec<(&'static str, String)> {
    let Some(rev) = report.revisions.get(index) else {
        return Vec::new();
    };
    let intro = report.intro_for(rev);
    let project = if rev.snapshot.project.trim().is_empty() {
        "Projekt".to_owned()
    } else {
        rev.snapshot.project.trim().to_owned()
    };
    vec![
        ("project", project),
        ("report", report.name_for(rev).trim().to_owned()),
        ("revision", rev.label_text().to_owned()),
        ("date", rev.date_text()),
        (
            "status",
            if rev.issued.is_some() {
                "Udsendt"
            } else {
                "Kladde"
            }
            .to_owned(),
        ),
        ("prepared_by", intro.prepared_by.trim().to_owned()),
        ("checked_by", intro.checked_by.trim().to_owned()),
        ("company", intro.company.trim().to_owned()),
        ("disciplines", intro.disciplines.trim().to_owned()),
        ("phase", crate::reports::phase_name(report.phase_for(rev))),
        ("data_date", date(&rev.snapshot.taken)),
        ("today", date(&opencheck_core::project::now_rfc3339())),
        ("opencheck", rev.snapshot.opencheck.clone()),
    ]
}

/// The files next to the filled-in template (`opencheck.typ`,
/// `revisions.typ` and `content.typ`) and the hashes of the pictures used.
pub fn parts(
    report: &Report,
    index: usize,
    page: &docx::Page,
) -> (Vec<(&'static str, String)>, Vec<String>) {
    let Some(rev) = report.revisions.get(index) else {
        return (Vec::new(), Vec::new());
    };
    let mut shared = String::from("#let info = (");
    for (k, v) in values(report, index) {
        shared.push_str(&format!("{k}: {}, ", s(&v)));
    }
    shared.push_str(")\n");
    shared.push_str(SHAPES);

    let mut revisions = String::from("#import \"/opencheck.typ\": *\n");
    revision_table(&mut revisions, report, &report.revisions[..=index]);

    let mut content = String::from("#import \"/opencheck.typ\": *\n");
    let mut pictures = Vec::new();
    let numbers = numbering(&rev.blocks);
    for (i, (block, number)) in rev.blocks.iter().zip(&numbers).enumerate() {
        if block.new_page {
            content.push_str(
                "#pagebreak(weak: true)
",
            );
        }
        // Where the block starts, for the preview to find it in the pages.
        content.push_str(&format!(
            "#metadata({i}) <{}>
",
            anchor(i)
        ));
        if !block.heading.trim().is_empty() {
            content.push_str(&format!(
                "#heading(level: {}, {})\n",
                block.level.clamp(1, 2),
                s(&format!("{number} {}", block.heading.trim()))
            ));
        }
        block_typst(&mut content, report, rev, block, page, &mut pictures);
    }
    pictures.sort();
    pictures.dedup();
    (
        vec![
            ("opencheck.typ", shared),
            ("revisions.typ", revisions),
            ("content.typ", content),
        ],
        pictures,
    )
}

/// The revisions up to the one shown, newest first: phase, label, date,
/// issued by, change.
fn revision_table(t: &mut String, report: &Report, revisions: &[Revision]) {
    t.push_str(
        "#table(columns: (auto, auto, auto, auto, 1fr),\n  table.header[Fase][Revision][Dato][Udsendt af][Ændring],\n",
    );
    for r in revisions.iter().rev() {
        let (by, note) = match &r.issued {
            Some(i) => (s(&i.by), s(&i.note)),
            None => ("text(fill: muted, \"kladde\")".to_owned(), "[]".to_owned()),
        };
        t.push_str(&format!(
            "  {}, {}, {}, {by}, {note},\n",
            or_dash(report.phase_for(r)),
            s(r.label_text()),
            or_dash(&r.date_text())
        ));
    }
    t.push_str(")\n");
}

/// Paragraphs of a typed text: blank lines part them.
fn paragraphs(t: &mut String, text: &str) {
    let list: Vec<String> = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(s)
        .collect();
    if !list.is_empty() {
        t.push_str(&format!("#paragraphs(({},))\n", list.join(", ")));
    }
}

/// The block's text and the pictures added to it, each sized to the text
/// column (see [`crate::reports::picture_box`]).
fn body(t: &mut String, block: &Block, page: &docx::Page, pictures: &mut Vec<String>) {
    paragraphs(t, &block.body);
    let text_width = page.width - page.left - page.right;
    let max_height = (page.height - page.top - page.bottom) * 0.55;
    for p in &block.pictures {
        pictures.push(p.hash.clone());
        let (w, h) = crate::reports::picture_box((p.width, p.height), text_width, max_height);
        t.push_str(&format!(
            "#block(above: 6pt, below: 10pt, image({}, width: {w:.1}pt, height: {h:.1}pt))\n",
            s(&picture_path(&p.hash))
        ));
    }
}

fn block_typst(
    t: &mut String,
    report: &Report,
    rev: &Revision,
    block: &Block,
    page: &docx::Page,
    pictures: &mut Vec<String>,
) {
    let snapshot = &rev.snapshot;
    match block.kind {
        BlockKind::Intro => {
            let intro = report.intro_for(rev);
            let mut rows = vec![("Udarbejdet af", or_dash(&intro.prepared_by))];
            if !intro.checked_by.trim().is_empty() {
                rows.push(("Kontrolleret af", s(intro.checked_by.trim())));
            }
            rows.extend([
                ("Firma", or_dash(&intro.company)),
                ("Dato", or_dash(&rev.date_text())),
                ("Revision", s(rev.label_text())),
                ("Fag", or_dash(&intro.disciplines)),
                (
                    "Fase",
                    or_dash(&crate::reports::phase_name(report.phase_for(rev))),
                ),
            ]);
            facts(t, &rows);
            body(t, block, page, pictures);
        }
        BlockKind::Text => body(t, block, page, pictures),
        BlockKind::Models => {
            body(t, block, page, pictures);
            t.push_str(
                "#table(columns: (1fr, auto, auto, auto),\n  table.header[Bygningsmodel (filnavn)][Fag][Version][Virksomhed],\n",
            );
            for m in &snapshot.models {
                let version = if m.date.is_empty() {
                    format!("rev. {} ({})", m.revision, m.imported)
                } else {
                    m.date.clone()
                };
                t.push_str(&format!(
                    "  {}, {}, {}, {},\n",
                    s(&m.file),
                    s(&m.discipline),
                    s(&version),
                    or_dash(&m.company)
                ));
            }
            t.push_str(")\n");
        }
        BlockKind::Software => {
            let rulesets = if snapshot.rulesets.is_empty() {
                "–".to_owned()
            } else {
                snapshot.rulesets.join(", ")
            };
            t.push_str(&format!(
                "#par({})\n",
                s(&format!(
                    "Kontrollen er foretaget i OpenCheck {} med følgende regelsæt: {rulesets}.",
                    snapshot.opencheck
                ))
            ));
            body(t, block, page, pictures);
        }
        BlockKind::Visual => {
            body(t, block, page, pictures);
            let checks = block.checks(snapshot);
            if checks.is_empty() {
                t.push_str(
                    "#text(fill: muted, style: \"italic\", \"Ingen dokumenterede kontroller.\")
",
                );
            }
            for c in checks {
                pictures.extend(c.pictures.iter().cloned());
                check_card(t, c);
            }
        }
        BlockKind::Information => {
            body(t, block, page, pictures);
            let docs = block.information(snapshot);
            if docs.is_empty() {
                t.push_str(
                    "#text(fill: muted, style: \"italic\", \"Ingen kontrol af informationsniveau i denne revision.\")\n",
                );
            }
            for d in docs {
                info_card(t, d);
            }
        }
        BlockKind::Properties | BlockKind::Geometry | BlockKind::Clashes | BlockKind::Issues => {
            body(t, block, page, pictures);
            let issues = block.issues(snapshot);
            if issues.is_empty() {
                t.push_str("#none-text\n");
                return;
            }
            counts(t, &issues);
            for issue in issues {
                let number = snapshot
                    .issues
                    .iter()
                    .position(|i| i.guid == issue.guid)
                    .map_or(0, |n| n + 1);
                let picture = snapshot.pictures.get(&issue.guid).cloned();
                if let Some(hash) = &picture {
                    pictures.push(hash.clone());
                }
                issue_card(
                    t,
                    number,
                    issue,
                    picture.as_deref(),
                    block.notes.get(&issue.guid),
                );
            }
        }
    }
}

/// A documented check: result, how it was done, colours and pictures.
fn check_card(t: &mut String, c: &crate::check_docs::CheckDoc) {
    let rows = [
        ("Kontrol", s(&c.check)),
        ("Resultat", s(c.verdict.label_da())),
        ("Udført af", or_dash(&c.by)),
        ("Dato", s(&date(&c.modified))),
        ("Fundet", or_dash(&c.summary)),
    ];
    let rows: Vec<String> = rows
        .iter()
        .map(|(k, v)| format!("({}, {v})", s(k)))
        .collect();
    let texts = |text: &str| -> Vec<String> {
        text.split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(s)
            .collect()
    };
    let (shown, more) = crate::check_docs::legend_shown(c);
    let legend: Vec<String> = shown
        .iter()
        .map(|l| {
            let [r, g, b] = l.color;
            format!(
                "({}, \"#{r:02x}{g:02x}{b:02x}\", {})",
                s(&crate::check_docs::swatch_text(l)),
                l.count
            )
        })
        .collect();
    let more = more.map_or_else(|| "none".to_owned(), |m| s(&m));
    let pictures: Vec<String> = c.pictures.iter().map(|h| s(&picture_path(h))).collect();
    // Rows of the same length, so the table fills up.
    let columns = c.table.first().map_or(0, Vec::len);
    let table: Vec<String> = c
        .table
        .iter()
        .map(|row| {
            let cells: Vec<String> = (0..columns)
                .map(|i| s(row.get(i).map_or("", String::as_str)))
                .collect();
            format!("({},)", cells.join(", "))
        })
        .collect();
    let list = |items: &[String]| {
        if items.is_empty() {
            "()".to_owned()
        } else {
            format!("({},)", items.join(", "))
        }
    };
    t.push_str(&format!(
        "#check-card({}, {}, \"#{}\", {}, {}, {}, {}, {more}, {}, {})\n",
        s(&c.title),
        s(c.verdict.label_da()),
        c.verdict.hex(),
        list(&rows),
        list(&texts(&c.about)),
        list(&texts(&c.method)),
        list(&legend),
        list(&table),
        list(&pictures),
    ));
}

/// An information check: how it was done and its fill rates per model.
fn info_card(t: &mut String, d: &crate::reports::InfoDoc) {
    use crate::reports::percent;
    let levels = if d.levels.is_empty() {
        None
    } else {
        let mut distinct: Vec<&str> = d.levels.iter().map(|(_, l)| l.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        Some(if distinct.len() == 1 {
            distinct[0].to_owned()
        } else {
            d.levels
                .iter()
                .map(|(m, l)| format!("{m} {l}"))
                .collect::<Vec<_>>()
                .join(", ")
        })
    };
    let elements: usize = d.models.iter().map(|m| m.elements).sum();
    let mut rows = vec![("Kontrol", s(&d.rule)), ("Grundlag", s(&d.standard))];
    if let Some(levels) = levels {
        rows.push(("Niveau", s(&levels)));
    }
    rows.push((
        "Omfang",
        s(&crate::reports::scope_da(
            elements,
            d.models.len(),
            d.findings,
        )),
    ));
    let rows: Vec<String> = rows
        .iter()
        .map(|(k, v)| format!("({}, {v})", s(k)))
        .collect();
    let texts: Vec<String> = d
        .about
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(s)
        .collect();
    let notes: Vec<String> = d.notes.iter().map(|n| s(n)).collect();
    let list = |items: &[String]| {
        if items.is_empty() {
            "()".to_owned()
        } else {
            format!("({},)", items.join(", "))
        }
    };
    let tables: Vec<String> = d
        .models
        .iter()
        .map(|m| {
            let mut items = Vec::new();
            for p in &m.parts {
                items.push(format!(
                    "({}, false, {}, {}, {:.3}, {}, false)",
                    s(&p.name),
                    s(&format!("{} elementer", p.elements)),
                    s(&percent(p.filled, p.values)),
                    crate::reports::fill_rate(p.filled, p.values),
                    p.has_accepted() && p.filled >= p.values
                ));
                for i in &p.items {
                    items.push(format!(
                        "({}, true, {}, {}, {:.3}, {}, {})",
                        s(&i.name),
                        s(&format!("{}/{}", i.present, i.required)),
                        s(&if i.accepted {
                            "OK".to_owned()
                        } else {
                            percent(i.present, i.required)
                        }),
                        if i.accepted {
                            1.0
                        } else {
                            crate::reports::fill_rate(i.present, i.required)
                        },
                        i.accepted,
                        i.geometry > 0
                    ));
                }
            }
            format!(
                "({}, {}, {}, {:.3}, {}, {})",
                s(&m.name),
                s(&format!("{} elementer", m.elements)),
                s(&percent(m.filled, m.values)),
                crate::reports::fill_rate(m.filled, m.values),
                m.has_accepted() && m.filled >= m.values,
                list(&items)
            )
        })
        .collect();
    let geo_note = if d.has_geometry() {
        s(crate::reports::GEOMETRY_NOTE_DA)
    } else {
        "none".to_owned()
    };
    t.push_str(&format!(
        "#info-card({}, {}, {}, {}, {geo_note}, {})\n",
        s(&d.title),
        list(&rows),
        list(&texts),
        list(&notes),
        list(&tables),
    ));
}

fn facts(t: &mut String, rows: &[(&str, String)]) {
    t.push_str("#facts((");
    for (k, v) in rows {
        t.push_str(&format!("({}, {v}), ", s(k)));
    }
    t.push_str("))\n");
}

/// Issues by criticality against status.
fn counts(t: &mut String, issues: &[&Issue]) {
    t.push_str(&format!(
        "#table(columns: (1fr,{} auto),\n  align: (left,{} center),\n  table.header[Kritikalitet]",
        " auto,".repeat(IssueStatus::ALL.len()),
        " center,".repeat(IssueStatus::ALL.len()),
    ));
    for st in IssueStatus::ALL {
        t.push_str(&format!("[{}]", status_da(st)));
    }
    t.push_str("[I alt],\n");
    for p in c402::LEVELS {
        let row: Vec<usize> = IssueStatus::ALL
            .iter()
            .map(|st| {
                issues
                    .iter()
                    .filter(|i| i.priority == p && i.status == *st)
                    .count()
            })
            .collect();
        let total: usize = row.iter().sum();
        if total == 0 {
            continue;
        }
        t.push_str(&format!(
            "  [#badge({}) #{}],",
            s(c402::code(p)),
            s(c402::label(p).split('–').nth(1).unwrap_or("").trim())
        ));
        for n in &row {
            t.push_str(&format!(" [{n}],"));
        }
        t.push_str(&format!(" strong[{total}],\n"));
    }
    t.push_str(")\n");
}

fn issue_card(
    t: &mut String,
    number: usize,
    issue: &Issue,
    picture: Option<&str>,
    note: Option<&String>,
) {
    let responsible: Vec<String> = issue
        .responsible
        .iter()
        .map(|d| c402::discipline_label(*d).unwrap_or(d.label()).to_owned())
        .collect();
    let rules: Vec<String> = {
        let mut list: Vec<String> = issue.findings.iter().map(|f| f.rule.clone()).collect();
        list.sort();
        list.dedup();
        list
    };
    let mut rows = vec![
        ("ID", format!("mono({})", s(&issue.guid))),
        ("Kritikalitet", s(c402::label(issue.priority))),
        ("Status", s(status_da(issue.status))),
        ("Ansvarlige fag", or_dash(&responsible.join(", "))),
        ("Tildelt", or_dash(&issue.assigned_to)),
        (
            "Oprettet / ændret",
            s(&format!(
                "{} / {}",
                date(&issue.created),
                date(&issue.modified)
            )),
        ),
    ];
    if !rules.is_empty() {
        rows.push(("Regel", s(&rules.join("; "))));
    }
    let measures: Vec<String> = issue
        .findings
        .iter()
        .filter(|f| f.details.is_empty() && f.magnitude_mm > 0.0)
        .map(|f| {
            let what = match f.kind.as_str() {
                "Clearance" => "afstand",
                "Duplicate" => "forskydning",
                _ => "dybde",
            };
            format!("{what} {:.0} mm", f.magnitude_mm)
        })
        .collect();
    if !measures.is_empty() {
        rows.push(("Mål", s(&measures.join(", "))));
    }
    let mut missing: Vec<String> = issue
        .findings
        .iter()
        .flat_map(|f| f.details.clone())
        .collect();
    if !missing.is_empty() {
        missing.sort();
        missing.dedup();
        rows.push(("Manglende egenskaber", s(&missing.join(", "))));
    }
    if issue.solved_by_update {
        rows.push(("Løst", s("Løst ved modelopdatering")));
    }
    let rows: Vec<String> = rows
        .iter()
        .map(|(k, v)| format!("({}, {v})", s(k)))
        .collect();
    let note = note
        .map(|n| n.trim())
        .filter(|n| !n.is_empty())
        .map_or_else(|| "none".to_owned(), s);
    let description = if issue.description.trim().is_empty() {
        "none".to_owned()
    } else {
        s(issue.description.trim())
    };
    let components: Vec<String> = issue
        .components
        .iter()
        .map(|c| {
            let role = match c.role {
                ComponentRole::A => "A",
                ComponentRole::B => "B",
            };
            format!(
                "({}, {}, {}, {}, {}, {})",
                s(role),
                or_dash(&c.tag),
                s(if c.name.is_empty() {
                    &c.ifc_class
                } else {
                    &c.name
                }),
                s(&c.ifc_class),
                s(&c.model),
                s(&c.global_id)
            )
        })
        .collect();
    t.push_str(&format!(
        "#issue-card({number}, {}, {}, {note}, {}, ({},), {description}, ({}))\n",
        s(&issue.title),
        s(c402::code(issue.priority)),
        picture.map_or_else(|| "none".to_owned(), |h| s(&picture_path(h))),
        rows.join(", "),
        if components.is_empty() {
            String::new()
        } else {
            format!("{},", components.join(", "))
        }
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{Report, Revision, default_blocks};
    use opencheck_core::check::Severity;

    #[test]
    fn severities_sort_most_critical_first() {
        let mut list = vec![Severity::Low, Severity::Critical, Severity::Moderate];
        list.sort();
        assert_eq!(
            list,
            [Severity::Critical, Severity::Moderate, Severity::Low]
        );
    }

    #[test]
    fn texts_become_typst_strings() {
        assert_eq!(s("a \"b\" \\ #c\nd"), r#""a \"b\" \\ #c\nd""#);
        assert_eq!(date("2026-10-20T10:00:00Z"), "2026-10-20");
    }

    #[test]
    fn chapters_and_sections_are_numbered() {
        let numbers = numbering(&default_blocks());
        assert_eq!(numbers[0], "1");
        assert_eq!(numbers[1], "2");
        assert_eq!(numbers[2], "2.1");
        assert_eq!(numbers[3], "2.2");
        assert_eq!(numbers.last().map(String::as_str), Some("4"));
    }

    pub fn report() -> Report {
        Report {
            name: "K&K-rapport".into(),
            revisions: vec![Revision {
                label: "A".into(),
                blocks: default_blocks(),
                ..Revision::default()
            }],
            ..Report::default()
        }
    }

    #[test]
    fn the_standard_template_becomes_typst() {
        let layout = docx::Docx::open(STANDARD_TEMPLATE)
            .and_then(|d| d.layout())
            .unwrap();
        assert!((layout.page.width - 595.3).abs() < 0.1);
        assert!(layout.title_page);
        let main = main(&layout, &report(), 0, false);
        assert!(main.contains("#include \"/content.typ\""));
        assert!(main.contains("#include \"/revisions.typ\""));
        assert!(
            main.contains("[#\"Projekt\"]"),
            "the project field is filled"
        );
        assert!(main.contains("counter(page).final()"));
        assert!(main.contains("show heading.where(level: 1)"));
        assert!(fonts(&layout).contains(&"Arial".to_owned()));
    }

    #[test]
    fn writes_the_parts_of_a_revision() {
        let (files, pictures) = parts(&report(), 0, &docx::Page::default());
        let content = &files.iter().find(|(n, _)| *n == "content.typ").unwrap().1;
        assert!(content.contains("#heading(level: 1, \"1 Indledning\")"));
        assert!(content.contains("#heading(level: 2, \"2.1 Bygningsmodeller\")"));
        assert!(content.contains("#none-text"));
        assert!(pictures.is_empty());
        assert!(!content.contains("pagebreak"));
        let mut with_check = report();
        with_check.revisions[0]
            .snapshot
            .checks
            .push(crate::check_docs::CheckDoc {
                id: "c1".into(),
                title: "Søjler over hinanden".into(),
                verdict: crate::check_docs::Verdict::Approved,
                method: "Farvet pr. etage.

Set fra siden."
                    .into(),
                legend: vec![crate::check_docs::Swatch {
                    label: "E1".into(),
                    color: [10, 200, 30],
                    count: 12,
                    ..Default::default()
                }],
                pictures: vec!["h1".into()],
                ..Default::default()
            });
        let (files, pictures) = parts(&with_check, 0, &docx::Page::default());
        let content = &files.iter().find(|(n, _)| *n == "content.typ").unwrap().1;
        assert!(
            content.contains("#check-card(\"Søjler over hinanden\", \"Godkendt\", \"#2E9E4A\"")
        );
        assert!(content.contains("(\"E1\", \"#0ac81e\", 12)"));
        assert_eq!(pictures, vec!["h1".to_owned()]);
        let mut report = report();
        report.revisions[0].blocks[3].new_page = true;
        let (files, _) = parts(&report, 0, &docx::Page::default());
        let content = &files.iter().find(|(n, _)| *n == "content.typ").unwrap().1;
        assert!(content.contains(
            "#pagebreak(weak: true)
#metadata(3)"
        ));
    }
}
