//! A report revision as a Word document: the template with its fields
//! filled in and the report's chapters written where `{{content}}` stands,
//! the revision table where `{{revisions}}` stands. The same parts as in
//! the PDF ([`crate::report`]), in Word's own terms, so the two read alike.

use opencheck_core::docx::{self, Docx, escape};
use rustc_hash::FxHashMap;

use crate::c402;
use crate::issues::{ComponentRole, Issue, IssueStatus};
use crate::report::{numbering, status_da, values};
use crate::reports::{Block, BlockKind, Report, Revision};

const MUTED: &str = "6B7380";
const RULE: &str = "DFE3EA";
const ACCENT: &str = "2B7FF6";

fn date(text: &str) -> String {
    text.get(..10).unwrap_or(text).to_owned()
}

fn or_dash(text: &str) -> String {
    if text.trim().is_empty() {
        "–".to_owned()
    } else {
        text.trim().to_owned()
    }
}

/// What the Word body is written with: the template's styles and the
/// pictures added.
struct Writer {
    /// The height a block picture may take, pt.
    max_height: f32,
    heading1: String,
    heading2: String,
    /// The width of the text in twips.
    width: u32,
    pictures: Vec<(String, Vec<u8>)>,
}

/// A run of text; line breaks kept.
fn run(text: &str, rpr: &str) -> String {
    let rpr = if rpr.is_empty() {
        String::new()
    } else {
        format!("<w:rPr>{rpr}</w:rPr>")
    };
    let mut out = String::new();
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str(&format!("<w:r>{rpr}<w:br/></w:r>"));
        }
        out.push_str(&format!(
            "<w:r>{rpr}<w:t xml:space=\"preserve\">{}</w:t></w:r>",
            escape(line)
        ));
    }
    out
}

fn para(runs: &str, ppr: &str) -> String {
    format!("<w:p><w:pPr>{ppr}</w:pPr>{runs}</w:p>")
}

fn muted() -> String {
    format!("<w:color w:val=\"{MUTED}\"/>")
}

fn badge(code: &str) -> String {
    let fill = match code {
        "C3" => "D64541",
        "C2" => "E08A1E",
        _ => "7A8594",
    };
    run(
        &format!(" {code} "),
        &format!(
            "<w:b/><w:color w:val=\"FFFFFF\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{fill}\"/>"
        ),
    )
}

/// A cell: paragraphs, at least one.
fn cell(width: u32, content: &str) -> String {
    let content = if content.is_empty() {
        "<w:p/>".to_owned()
    } else {
        content.to_owned()
    };
    format!("<w:tc><w:tcPr><w:tcW w:w=\"{width}\" w:type=\"dxa\"/></w:tcPr>{content}</w:tc>")
}

/// A table with fixed column widths; `ruled` puts light lines between rows.
fn table(widths: &[u32], rows: &[Vec<String>], header: bool, ruled: bool) -> String {
    let borders = if ruled {
        format!(
            "<w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:color=\"{RULE}\"/>\
             <w:bottom w:val=\"single\" w:sz=\"4\" w:color=\"{RULE}\"/>\
             <w:insideH w:val=\"single\" w:sz=\"4\" w:color=\"{RULE}\"/></w:tblBorders>"
        )
    } else {
        "<w:tblBorders><w:top w:val=\"nil\"/><w:left w:val=\"nil\"/><w:bottom w:val=\"nil\"/>\
         <w:right w:val=\"nil\"/><w:insideH w:val=\"nil\"/><w:insideV w:val=\"nil\"/></w:tblBorders>"
            .to_owned()
    };
    let mut t = format!(
        "<w:tbl><w:tblPr><w:tblW w:w=\"{}\" w:type=\"dxa\"/>{borders}<w:tblLayout w:type=\"fixed\"/>\
         <w:tblCellMar><w:top w:w=\"40\" w:type=\"dxa\"/><w:left w:w=\"80\" w:type=\"dxa\"/>\
         <w:bottom w:w=\"40\" w:type=\"dxa\"/><w:right w:w=\"80\" w:type=\"dxa\"/></w:tblCellMar>\
         <w:tblLook w:val=\"0000\"/></w:tblPr><w:tblGrid>",
        widths.iter().sum::<u32>()
    );
    for w in widths {
        t.push_str(&format!("<w:gridCol w:w=\"{w}\"/>"));
    }
    t.push_str("</w:tblGrid>");
    for (i, row) in rows.iter().enumerate() {
        t.push_str("<w:tr>");
        if header && i == 0 {
            t.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        } else {
            t.push_str("<w:trPr><w:cantSplit/></w:trPr>");
        }
        for (c, content) in row.iter().enumerate() {
            t.push_str(&cell(widths.get(c).copied().unwrap_or(1000), content));
        }
        t.push_str("</w:tr>");
    }
    t.push_str("</w:tbl>");
    // Word needs a paragraph between a table and what follows it.
    t.push_str(&para(
        "",
        "<w:spacing w:before=\"0\" w:after=\"0\" w:line=\"120\" w:lineRule=\"exact\"/>",
    ));
    t
}

/// A cell's paragraph: small text, no space after.
fn cell_text(text: &str, rpr: &str) -> String {
    para(&run(text, rpr), "<w:spacing w:before=\"0\" w:after=\"0\"/>")
}

/// Widths in twips from fractions of the text width.
fn widths(total: u32, parts: &[f32]) -> Vec<u32> {
    let sum: f32 = parts.iter().sum();
    parts
        .iter()
        .map(|p| (total as f32 * p / sum).round() as u32)
        .collect()
}

impl Writer {
    fn heading(&self, level: u8, text: &str, new_page: bool) -> String {
        let style = if level <= 1 {
            &self.heading1
        } else {
            &self.heading2
        };
        let page = if new_page { "<w:pageBreakBefore/>" } else { "" };
        para(
            &run(text, ""),
            &format!("<w:pStyle w:val=\"{style}\"/>{page}"),
        )
    }

    fn paragraphs(&self, text: &str) -> String {
        text.split("\n\n")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|p| para(&run(p, ""), ""))
            .collect()
    }

    fn facts(&self, rows: &[(&str, String)], width: u32) -> String {
        let w = widths(width, &[0.32, 0.68]);
        let rows: Vec<Vec<String>> = rows
            .iter()
            .map(|(k, v)| vec![cell_text(k, &muted()), cell_text(v, "")])
            .collect();
        table(&w, &rows, false, false)
    }

    /// The block's text and the pictures added to it, each sized to the
    /// text column (see [`crate::reports::picture_box`]).
    fn body(&mut self, block: &Block, pictures: &FxHashMap<String, Vec<u8>>) -> String {
        let mut out = self.paragraphs(&block.body);
        let text_width = self.width as f32 / 20.0;
        for p in &block.pictures {
            let Some(png) = pictures.get(&p.hash) else {
                continue;
            };
            let (w, _) =
                crate::reports::picture_box((p.width, p.height), text_width, self.max_height);
            out.push_str(&para(
                &self.picture(png, (w * 20.0) as u32),
                "<w:spacing w:before=\"120\" w:after=\"200\"/>",
            ));
        }
        out
    }

    /// A picture added to the document, `width` twips wide.
    fn picture(&mut self, png: &[u8], width: u32) -> String {
        let Some((pw, ph)) = docx::png_size(png) else {
            return String::new();
        };
        let i = self.pictures.len();
        self.pictures
            .push((format!("opencheck{i}.png"), png.to_vec()));
        // Twips to EMU: 635 per twip.
        let cx = u64::from(width) * 635;
        let cy = cx * u64::from(ph) / u64::from(pw.max(1));
        let id = 1000 + i;
        let rel = docx::picture_rel(i);
        format!(
            "<w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
             distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{cx}\" cy=\"{cy}\"/>\
             <wp:docPr id=\"{id}\" name=\"Picture {id}\"/>\
             <a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\">\
             <a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:nvPicPr><pic:cNvPr id=\"{id}\" name=\"opencheck{i}.png\"/><pic:cNvPicPr/></pic:nvPicPr>\
             <pic:blipFill><a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"{rel}\"/>\
             <a:stretch><a:fillRect/></a:stretch></pic:blipFill>\
             <pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>\
             <a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic>\
             </wp:inline></w:drawing></w:r>"
        )
    }

    fn revisions(&self, report: &Report, revisions: &[Revision]) -> String {
        let w = widths(self.width, &[0.1, 0.12, 0.16, 0.22, 0.4]);
        let mut rows = vec![
            ["Fase", "Revision", "Dato", "Udsendt af", "Ændring"]
                .iter()
                .map(|h| cell_text(h, &format!("<w:b/>{}", muted())))
                .collect::<Vec<_>>(),
        ];
        for r in revisions.iter().rev() {
            let (by, note, by_rpr) = match &r.issued {
                Some(i) => (i.by.clone(), i.note.clone(), String::new()),
                None => ("kladde".to_owned(), String::new(), muted()),
            };
            rows.push(vec![
                cell_text(&or_dash(report.phase_for(r)), ""),
                cell_text(r.label_text(), ""),
                cell_text(&or_dash(&r.date_text()), ""),
                cell_text(&by, &by_rpr),
                cell_text(&note, ""),
            ]);
        }
        table(&w, &rows, true, true)
    }

    fn content(
        &mut self,
        report: &Report,
        rev: &Revision,
        pictures: &FxHashMap<String, Vec<u8>>,
    ) -> String {
        let mut out = String::new();
        let numbers = numbering(&rev.blocks);
        for (block, number) in rev.blocks.iter().zip(&numbers) {
            if !block.heading.trim().is_empty() {
                out.push_str(&self.heading(
                    block.level,
                    &format!("{number} {}", block.heading.trim()),
                    block.new_page,
                ));
            } else if block.new_page {
                out.push_str(&para(
                    "<w:r><w:br w:type=\"page\"/></w:r>",
                    "<w:spacing w:before=\"0\" w:after=\"0\"/>",
                ));
            }
            self.block(&mut out, report, rev, block, pictures);
        }
        out
    }

    fn block(
        &mut self,
        out: &mut String,
        report: &Report,
        rev: &Revision,
        block: &Block,
        pictures: &FxHashMap<String, Vec<u8>>,
    ) {
        let snapshot = &rev.snapshot;
        match block.kind {
            BlockKind::Intro => {
                let intro = report.intro_for(rev);
                let mut rows = vec![("Udarbejdet af", or_dash(&intro.prepared_by))];
                if !intro.checked_by.trim().is_empty() {
                    rows.push(("Kontrolleret af", intro.checked_by.trim().to_owned()));
                }
                rows.extend([
                    ("Firma", or_dash(&intro.company)),
                    ("Dato", or_dash(&rev.date_text())),
                    ("Revision", rev.label_text().to_owned()),
                    ("Fag", or_dash(&intro.disciplines)),
                    (
                        "Fase",
                        or_dash(&crate::reports::phase_name(report.phase_for(rev))),
                    ),
                ]);
                out.push_str(&self.facts(&rows, self.width * 6 / 10));
                out.push_str(&self.body(block, pictures));
            }
            BlockKind::Text => out.push_str(&self.body(block, pictures)),
            BlockKind::Models => {
                out.push_str(&self.body(block, pictures));
                let w = widths(self.width, &[0.42, 0.18, 0.2, 0.2]);
                let mut rows = vec![
                    ["Bygningsmodel (filnavn)", "Fag", "Version", "Virksomhed"]
                        .iter()
                        .map(|h| cell_text(h, &format!("<w:b/>{}", muted())))
                        .collect::<Vec<_>>(),
                ];
                for m in &snapshot.models {
                    let version = if m.date.is_empty() {
                        format!("rev. {} ({})", m.revision, m.imported)
                    } else {
                        m.date.clone()
                    };
                    rows.push(vec![
                        cell_text(&m.file, ""),
                        cell_text(&m.discipline, ""),
                        cell_text(&version, ""),
                        cell_text(&or_dash(&m.company), ""),
                    ]);
                }
                out.push_str(&table(&w, &rows, true, true));
            }
            BlockKind::Software => {
                let rulesets = if snapshot.rulesets.is_empty() {
                    "–".to_owned()
                } else {
                    snapshot.rulesets.join(", ")
                };
                out.push_str(&para(
                    &run(
                        &format!(
                            "Kontrollen er foretaget i OpenCheck {} med følgende regelsæt: {rulesets}.",
                            snapshot.opencheck
                        ),
                        "",
                    ),
                    "",
                ));
                out.push_str(&self.body(block, pictures));
            }
            BlockKind::Visual => {
                out.push_str(&self.body(block, pictures));
                let checks = block.checks(snapshot);
                if checks.is_empty() {
                    out.push_str(&para(
                        &run(
                            "Ingen dokumenterede kontroller.",
                            &format!("<w:i/>{}", muted()),
                        ),
                        "",
                    ));
                }
                for c in checks {
                    let card = self.check_card(c, pictures);
                    out.push_str(&card);
                }
            }
            BlockKind::Information => {
                out.push_str(&self.body(block, pictures));
                let docs = block.information(snapshot);
                if docs.is_empty() {
                    out.push_str(&para(
                        &run(
                            "Ingen kontrol af informationsniveau i denne revision.",
                            &format!("<w:i/>{}", muted()),
                        ),
                        "",
                    ));
                }
                for d in docs {
                    let card = self.info_card(d);
                    out.push_str(&card);
                }
            }
            BlockKind::Properties
            | BlockKind::Geometry
            | BlockKind::Clashes
            | BlockKind::Issues => {
                out.push_str(&self.body(block, pictures));
                let issues = block.issues(snapshot);
                if issues.is_empty() {
                    out.push_str(&para(
                        &run("Ingen emner.", &format!("<w:i/>{}", muted())),
                        "",
                    ));
                    return;
                }
                out.push_str(&self.counts(&issues));
                for issue in issues {
                    let number = snapshot
                        .issues
                        .iter()
                        .position(|i| i.guid == issue.guid)
                        .map_or(0, |n| n + 1);
                    let picture = snapshot
                        .pictures
                        .get(&issue.guid)
                        .and_then(|hash| pictures.get(hash));
                    let card =
                        self.issue_card(number, issue, picture, block.notes.get(&issue.guid));
                    out.push_str(&card);
                }
            }
        }
    }

    /// An information check: how it was done and a table per model with
    /// its building parts and their items.
    fn info_card(&self, d: &crate::reports::InfoDoc) -> String {
        use crate::reports::percent;
        const ACCEPTED: &str = "<w:color w:val=\"2662B0\"/>";
        let mut out = para(
            &run(&d.title, "<w:b/>"),
            &format!(
                "<w:keepNext/><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"6\" w:color=\"{RULE}\"/><w:left w:val=\"single\" w:sz=\"24\" w:space=\"6\" w:color=\"2B7FF6\"/></w:pBdr>                 <w:spacing w:before=\"240\" w:after=\"80\"/>"
            ),
        );
        let elements: usize = d.models.iter().map(|m| m.elements).sum();
        let mut rows = vec![
            ("Kontrol", d.rule.clone()),
            ("Grundlag", d.standard.clone()),
        ];
        if !d.levels.is_empty() {
            let mut distinct: Vec<&str> = d.levels.iter().map(|(_, l)| l.as_str()).collect();
            distinct.sort_unstable();
            distinct.dedup();
            rows.push((
                "Niveau",
                if distinct.len() == 1 {
                    distinct[0].to_owned()
                } else {
                    d.levels
                        .iter()
                        .map(|(m, l)| format!("{m} {l}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                },
            ));
        }
        rows.push((
            "Omfang",
            crate::reports::scope_da(elements, d.models.len(), d.findings),
        ));
        out.push_str(&self.facts(&rows, self.width * 7 / 10));
        out.push_str(&self.paragraphs(&d.about));
        for note in &d.notes {
            out.push_str(&para(&run(note, "<w:color w:val=\"E08A1E\"/>"), ""));
        }
        if d.has_geometry() {
            out.push_str(&para(
                &format!(
                    "{}{}",
                    run("\u{25B3} ", &muted()),
                    run(crate::reports::GEOMETRY_NOTE_DA, "")
                ),
                "",
            ));
        }
        let small = "<w:sz w:val=\"17\"/>";
        let w = widths(self.width, &[0.58, 0.22, 0.2]);
        for m in &d.models {
            let mut rows = vec![vec![
                cell_text(&m.name, &format!("<w:b/>{small}")),
                cell_text(
                    &format!("{} elementer", m.elements),
                    &format!("{}{small}", muted()),
                ),
                cell_text(
                    &percent(m.filled, m.values),
                    &format!(
                        "<w:b/>{}{small}",
                        if m.has_accepted() && m.filled >= m.values {
                            ACCEPTED
                        } else {
                            ""
                        }
                    ),
                ),
            ]];
            for p in &m.parts {
                rows.push(vec![
                    cell_text(&p.name, &format!("<w:b/>{small}")),
                    cell_text(
                        &format!("{} elementer", p.elements),
                        &format!("{}{small}", muted()),
                    ),
                    cell_text(
                        &percent(p.filled, p.values),
                        &format!(
                            "{}{small}",
                            if p.has_accepted() && p.filled >= p.values {
                                ACCEPTED
                            } else {
                                ""
                            }
                        ),
                    ),
                ]);
                for i in &p.items {
                    let mark = if i.geometry > 0 {
                        run(" \u{25B3}", &format!("{}{small}", muted()))
                    } else {
                        String::new()
                    };
                    rows.push(vec![
                        para(
                            &format!("{}{mark}", run(&i.name, small)),
                            "<w:spacing w:before=\"0\" w:after=\"0\"/><w:ind w:left=\"220\"/>",
                        ),
                        cell_text(
                            &format!("{}/{}", i.present, i.required),
                            &format!("{}{small}", muted()),
                        ),
                        if i.accepted {
                            cell_text("OK", &format!("<w:b/>{ACCEPTED}{small}"))
                        } else {
                            cell_text(&percent(i.present, i.required), small)
                        },
                    ]);
                }
            }
            out.push_str(&table(&w, &rows, true, true));
        }
        out
    }

    /// A documented check: result, how it was done, colours and pictures.
    fn check_card(
        &mut self,
        c: &crate::check_docs::CheckDoc,
        pictures: &FxHashMap<String, Vec<u8>>,
    ) -> String {
        let light = run(
            &format!(" {} ", c.verdict.label_da()),
            &format!(
                "<w:b/><w:color w:val=\"FFFFFF\"/><w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{}\"/>",
                c.verdict.hex()
            ),
        );
        let mut out = para(
            &format!("{}{light}", run(&format!("{}  ", c.title), "<w:b/>")),
            &format!(
                "<w:keepNext/><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"6\" w:color=\"{RULE}\"/><w:left w:val=\"single\" w:sz=\"24\" w:space=\"6\" w:color=\"{}\"/></w:pBdr>                 <w:spacing w:before=\"240\" w:after=\"80\"/>",
                c.verdict.hex()
            ),
        );
        let rows = vec![
            ("Kontrol", c.check.clone()),
            ("Resultat", c.verdict.label_da().to_owned()),
            ("Udført af", or_dash(&c.by)),
            ("Dato", date(&c.modified)),
            ("Fundet", or_dash(&c.summary)),
        ];
        out.push_str(&self.facts(&rows, self.width * 7 / 10));
        out.push_str(&self.paragraphs(&c.about));
        out.push_str(&self.paragraphs(&c.method));
        // The colours as one small line of swatches.
        let (shown, more) = crate::check_docs::legend_shown(c);
        if !shown.is_empty() {
            let small = "<w:sz w:val=\"16\"/>";
            let mut runs = String::new();
            for l in shown {
                let [r, g, b] = l.color;
                runs.push_str(&run(
                    "■ ",
                    &format!("<w:color w:val=\"{r:02X}{g:02X}{b:02X}\"/>{small}"),
                ));
                runs.push_str(&run(&crate::check_docs::swatch_text(l), small));
                runs.push_str(&run(
                    &format!(" {}    ", l.count),
                    &format!("{}{small}", muted()),
                ));
            }
            if let Some(more) = more {
                runs.push_str(&run(&more, &format!("{}{small}", muted())));
            }
            out.push_str(&para(&runs, "<w:spacing w:before=\"80\" w:after=\"80\"/>"));
        }
        if c.table.len() > 1 {
            let columns = c.table[0].len();
            let w = widths(self.width, &vec![1.0; columns]);
            let small = "<w:sz w:val=\"16\"/>";
            let head = format!("<w:b/>{}{small}", muted());
            let rows: Vec<Vec<String>> = c
                .table
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    (0..columns)
                        .map(|k| {
                            let text = row.get(k).map_or("", String::as_str);
                            cell_text(text, if i == 0 { &head } else { small })
                        })
                        .collect()
                })
                .collect();
            out.push_str(&table(&w, &rows, true, true));
        }
        // Two pictures a row, so a check takes little room.
        let w = widths(self.width, &[1.0, 1.0]);
        let shots: Vec<&Vec<u8>> = c.pictures.iter().filter_map(|h| pictures.get(h)).collect();
        let mut rows = Vec::new();
        for pair in shots.chunks(2) {
            let mut row: Vec<String> = pair
                .iter()
                .map(|png| {
                    para(
                        &self.picture(png, w[0] - 200),
                        "<w:spacing w:before=\"0\" w:after=\"0\"/>",
                    )
                })
                .collect();
            row.resize(2, String::new());
            rows.push(row);
        }
        if !rows.is_empty() {
            out.push_str(&table(&w, &rows, false, false));
        }
        out
    }

    fn counts(&self, issues: &[&Issue]) -> String {
        let n = IssueStatus::ALL.len();
        let mut parts = vec![0.34];
        parts.extend(std::iter::repeat_n(0.66 / (n + 1) as f32, n + 1));
        let w = widths(self.width, &parts);
        let head = format!("<w:b/>{}", muted());
        let mut header = vec![cell_text("Kritikalitet", &head)];
        header.extend(
            IssueStatus::ALL
                .iter()
                .map(|s| cell_text(status_da(*s), &head)),
        );
        header.push(cell_text("I alt", &head));
        let mut rows = vec![header];
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
            let label = c402::label(p).split('–').nth(1).unwrap_or("").trim();
            let mut cells = vec![para(
                &format!("{}{}", badge(c402::code(p)), run(&format!(" {label}"), "")),
                "<w:spacing w:before=\"0\" w:after=\"0\"/>",
            )];
            cells.extend(row.iter().map(|n| cell_text(&n.to_string(), "")));
            cells.push(cell_text(&total.to_string(), "<w:b/>"));
            rows.push(cells);
        }
        table(&w, &rows, true, true)
    }

    fn issue_card(
        &mut self,
        number: usize,
        issue: &Issue,
        picture: Option<&Vec<u8>>,
        note: Option<&String>,
    ) -> String {
        let mut out = para(
            &format!(
                "{}{}",
                run(&format!("#{number} {}  ", issue.title), "<w:b/>"),
                badge(c402::code(issue.priority))
            ),
            &format!(
                "<w:keepNext/><w:pBdr><w:top w:val=\"single\" w:sz=\"4\" w:space=\"6\" w:color=\"{RULE}\"/></w:pBdr>\
                 <w:spacing w:before=\"240\" w:after=\"80\"/>"
            ),
        );
        if let Some(note) = note.map(|n| n.trim()).filter(|n| !n.is_empty()) {
            out.push_str(&para(
                &run(note, ""),
                &format!(
                    "<w:keepNext/><w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"6\" w:color=\"{ACCENT}\"/></w:pBdr>\
                     <w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"F4F6F9\"/>"
                ),
            ));
        }
        let responsible: Vec<String> = issue
            .responsible
            .iter()
            .map(|d| c402::discipline_label(*d).unwrap_or(d.label()).to_owned())
            .collect();
        let mut rules: Vec<String> = issue.findings.iter().map(|f| f.rule.clone()).collect();
        rules.sort();
        rules.dedup();
        let mut rows = vec![
            ("ID", issue.guid.clone()),
            ("Kritikalitet", c402::label(issue.priority).to_owned()),
            ("Status", status_da(issue.status).to_owned()),
            ("Ansvarlige fag", or_dash(&responsible.join(", "))),
            ("Tildelt", or_dash(&issue.assigned_to)),
            (
                "Oprettet / ændret",
                format!("{} / {}", date(&issue.created), date(&issue.modified)),
            ),
        ];
        if !rules.is_empty() {
            rows.push(("Regel", rules.join("; ")));
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
            rows.push(("Mål", measures.join(", ")));
        }
        let mut missing: Vec<String> = issue
            .findings
            .iter()
            .flat_map(|f| f.details.clone())
            .collect();
        if !missing.is_empty() {
            missing.sort();
            missing.dedup();
            rows.push(("Manglende egenskaber", missing.join(", ")));
        }
        if issue.solved_by_update {
            rows.push(("Løst", "Løst ved modelopdatering".to_owned()));
        }
        let w = widths(self.width, &[0.42, 0.58]);
        let shot = match picture {
            Some(png) => para(
                &self.picture(png, w[0] - 200),
                "<w:spacing w:before=\"0\" w:after=\"0\"/>",
            ),
            None => cell_text("Intet billede", &format!("<w:i/>{}", muted())),
        };
        let facts = self.facts(&rows, w[1] - 160);
        out.push_str(&table(&w, &[vec![shot, facts]], false, false));
        if !issue.description.trim().is_empty() {
            out.push_str(&para(&run(issue.description.trim(), ""), ""));
        }
        if !issue.components.is_empty() {
            let w = widths(self.width, &[0.04, 0.12, 0.34, 0.16, 0.09, 0.25]);
            let small = "<w:sz w:val=\"15\"/>";
            let head = format!("<w:b/>{}{small}", muted());
            let mut rows = vec![
                [
                    "",
                    "Revit ID",
                    "Objekt",
                    "IFC-klasse",
                    "Model",
                    "IFC GlobalId",
                ]
                .iter()
                .map(|h| cell_text(h, &head))
                .collect::<Vec<_>>(),
            ];
            for c in &issue.components {
                let role = match c.role {
                    ComponentRole::A => "A",
                    ComponentRole::B => "B",
                };
                let name = if c.name.is_empty() {
                    &c.ifc_class
                } else {
                    &c.name
                };
                rows.push(vec![
                    cell_text(role, small),
                    cell_text(&or_dash(&c.tag), &format!("<w:b/>{small}")),
                    cell_text(name, small),
                    cell_text(&c.ifc_class, small),
                    cell_text(&c.model, small),
                    cell_text(&c.global_id, small),
                ]);
            }
            out.push_str(&table(&w, &rows, true, true));
        }
        out
    }
}

/// Revision `index` of a report as a Word document made from `template`;
/// `watermark`: a trial print, "Foreløbigt tryk" across every page.
pub fn build(
    template: &Docx,
    page: &docx::Page,
    report: &Report,
    index: usize,
    pictures: &FxHashMap<String, Vec<u8>>,
    watermark: bool,
) -> Result<Vec<u8>, String> {
    let rev = report.revisions.get(index).ok_or("no such revision")?;
    let stamped;
    let template = if watermark {
        stamped = template.stamped(crate::reports::PRELIMINARY_DA);
        &stamped
    } else {
        template
    };
    let mut writer = Writer {
        heading1: template
            .style_id("heading 1")
            .unwrap_or_else(|| "Heading1".into()),
        heading2: template
            .style_id("heading 2")
            .unwrap_or_else(|| "Heading2".into()),
        width: (((page.width - page.left - page.right) * 20.0) as u32).max(4000),
        max_height: (page.height - page.top - page.bottom) * 0.55,
        pictures: Vec::new(),
    };
    let revisions = writer.revisions(report, &report.revisions[..=index]);
    let content = writer.content(report, rev, pictures);
    let fields = values(report, index);
    template.fill(
        &fields,
        &[("content", content), ("revisions", revisions)],
        &writer.pictures,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::default_blocks;

    #[test]
    fn the_standard_template_becomes_a_word_document() {
        let template = Docx::open(crate::report::STANDARD_TEMPLATE).unwrap();
        let layout = template.layout().unwrap();
        let report = Report {
            name: "K&K <rapport>".into(),
            revisions: vec![Revision {
                label: "A".into(),
                blocks: default_blocks(),
                snapshot: crate::reports::Snapshot {
                    information: vec![crate::reports::InfoDoc {
                        id: "lod/fill".into(),
                        title: "Information: LOD DK".into(),
                        about: "Kontrollen er foretaget efter standarden.".into(),
                        notes: vec!["En <note>".into()],
                        models: vec![crate::reports::InfoModel {
                            name: "ARK".into(),
                            elements: 3,
                            values: 6,
                            filled: 6,
                            parts: vec![crate::reports::InfoPart {
                                name: "Væg".into(),
                                elements: 3,
                                complete: 3,
                                values: 6,
                                filled: 6,
                                items: vec![crate::reports::InfoItem {
                                    name: "Dybde".into(),
                                    present: 0,
                                    required: 3,
                                    accepted: true,
                                    geometry: 0,
                                }],
                            }],
                        }],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Revision::default()
            }],
            ..Report::default()
        };
        let bytes = build(
            &template,
            &layout.page,
            &report,
            0,
            &FxHashMap::default(),
            false,
        )
        .unwrap();
        let word = Docx::open(&bytes).unwrap();
        let filled = word.layout().unwrap();
        let text: Vec<String> = filled
            .body
            .iter()
            .filter_map(|b| match b {
                docx::Block::Para(p) => Some(p.text()),
                _ => None,
            })
            .collect();
        assert!(
            text.iter()
                .any(|t| t == "K&K <rapport> · Konsistens- og kollisionskontrol")
        );
        assert!(text.iter().any(|t| t == "1 Indledning"));
        assert!(text.iter().any(|t| t == "2.1 Bygningsmodeller"));
        assert!(!text.iter().any(|t| t.contains("{{")));
        let header: String = filled
            .header
            .iter()
            .filter_map(|b| match b {
                docx::Block::Para(p) => Some(p.text()),
                _ => None,
            })
            .collect();
        assert!(header.contains("Revision A"), "{header}");
        // A trial print carries the watermark in every header.
        let bytes = build(
            &template,
            &layout.page,
            &report,
            0,
            &FxHashMap::default(),
            true,
        )
        .unwrap();
        let stamped = Docx::open(&bytes).unwrap();
        let header1 = String::from_utf8_lossy(stamped.part("word/header1.xml").unwrap());
        assert!(header1.contains("Foreløbigt tryk"), "{header1}");
        assert!(header1.contains("<v:textpath"), "{header1}");
        assert!(stamped.layout().is_ok());
        let mut report = report;
        report.revisions[0].blocks[2].new_page = true;
        let bytes = build(
            &template,
            &layout.page,
            &report,
            0,
            &FxHashMap::default(),
            false,
        )
        .unwrap();
        let word = Docx::open(&bytes).unwrap();
        let xml = String::from_utf8_lossy(word.part("word/document.xml").unwrap());
        assert_eq!(xml.matches("<w:pageBreakBefore/>").count(), 1);
    }

    /// A report with an issue, picture and comment written as Word and PDF
    /// to `OPENCHECK_SAMPLE_DIR`, to look at.
    #[test]
    #[ignore]
    fn writes_a_sample() {
        use crate::issues::{IssueComponent, IssueSource};
        use opencheck_core::check::Severity;
        use std::path::PathBuf;
        let Ok(dir) = std::env::var("OPENCHECK_SAMPLE_DIR") else {
            return;
        };
        let mut png = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png, 320, 180);
            encoder.set_color(png::ColorType::Rgb);
            let mut writer = encoder.write_header().unwrap();
            let data: Vec<u8> = (0..320 * 180)
                .flat_map(|i| [(i % 320) as u8 / 2 + 60, 120, 200])
                .collect();
            writer.write_image_data(&data).unwrap();
        }
        let issue = Issue {
            guid: "012ccfbd-0c4b-4c3c-9e72-a632665feb0e".into(),
            title: "Søjle mod ventilationskanal".into(),
            description: "Kanalen skal flyttes 200 mm op.".into(),
            status: IssueStatus::Open,
            priority: Severity::Critical,
            created: "2026-10-31T10:00:00Z".into(),
            modified: "2026-10-31T10:00:00Z".into(),
            labels: vec![],
            findings: vec![IssueSource {
                finding: "f".into(),
                ruleset: "Clash".into(),
                rule: "Elements of different models must not intersect".into(),
                kind: "Clash".into(),
                magnitude_mm: 200.0,
                ruleset_id: String::new(),
                rule_id: String::new(),
                revisions: vec![],
                solved_by_update: None,
                details: vec![],
            }],
            source: None,
            components: vec![IssueComponent {
                global_id: "0dxIwCEhH21AsezGoaFUtq".into(),
                name: "SHS_Søjle:SHS200x8:3949807".into(),
                ifc_class: "IfcColumn".into(),
                model: "kon".into(),
                role: ComponentRole::A,
                tag: "3949807".into(),
            }],
            point: [0.0; 3],
            camera: None,
            section: None,
            show_context: false,
            hidden: vec![],
            solved_by_update: false,
            responsible: vec![],
            assigned_to: "ABC".into(),
            created_by: String::new(),
            bcf: None,
        };
        let mut blocks = default_blocks();
        for b in &mut blocks {
            if b.kind == BlockKind::Clashes {
                b.notes
                    .insert(issue.guid.clone(), "Aftalt på projektmødet.".into());
            }
        }
        let report = Report {
            name: "K&K-rapport".into(),
            revisions: vec![Revision {
                label: "A".into(),
                blocks,
                snapshot: crate::reports::Snapshot {
                    project: "Skolen".into(),
                    issues: vec![issue.clone()],
                    pictures: [(issue.guid.clone(), "p1".to_owned())].into(),
                    checks: vec![crate::check_docs::CheckDoc {
                        id: "c1".into(),
                        title: "Søjler står over hinanden".into(),
                        check: "Columns above each other".into(),
                        verdict: crate::check_docs::Verdict::Remarks,
                        method: "Søjler farvet pr. etage og set fra siden.".into(),
                        summary: "40 columns".into(),
                        about: crate::check_docs::about_da(
                            &opencheck_core::visual::VisualRule::alignment(),
                        ),
                        legend: (1..=30)
                            .map(|i| crate::check_docs::Swatch {
                                label: format!("Group {i}"),
                                detail: format!("{} elements", 40 - i),
                                color: opencheck_core::visual::palette(i),
                                count: 40 - i,
                            })
                            .collect(),
                        pictures: vec!["p1".into(), "p1".into()],
                        ..Default::default()
                    }],
                    information: vec![crate::reports::InfoDoc {
                        id: "lod/fill".into(),
                        title: "Information: LOD DK".into(),
                        rule: "Fill rate per building part".into(),
                        standard: "Informationskrav, LOD DK".into(),
                        about: crate::reports::about_information_da(
                            true,
                            "Informationskrav, LOD DK",
                            &[("STR".into(), "LOD 300 DK".into())],
                            true,
                        ),
                        levels: vec![("STR".into(), "LOD 300 DK".into())],
                        models: vec![crate::reports::InfoModel {
                            name: "STR".into(),
                            elements: 368,
                            values: 1840,
                            filled: 1480,
                            parts: vec![
                                crate::reports::InfoPart {
                                    name: "Fundament".into(),
                                    elements: 243,
                                    complete: 0,
                                    values: 1215,
                                    filled: 1215,
                                    items: [
                                        "Klassifikationskode",
                                        "Type (-kode/-ID)",
                                        "Typenavn",
                                        "Længde",
                                    ]
                                    .iter()
                                    .map(|n| crate::reports::InfoItem {
                                        name: (*n).into(),
                                        present: 243,
                                        required: 243,
                                        accepted: false,
                                        geometry: 0,
                                    })
                                    .chain([
                                        crate::reports::InfoItem {
                                            name: "Lastbærende".into(),
                                            present: 0,
                                            required: 243,
                                            accepted: true,
                                            geometry: 0,
                                        },
                                        crate::reports::InfoItem {
                                            name: "Centerkote".into(),
                                            present: 243,
                                            required: 243,
                                            accepted: false,
                                            geometry: 243,
                                        },
                                    ])
                                    .collect(),
                                },
                                crate::reports::InfoPart {
                                    name: "Reces- og hulobjekter".into(),
                                    elements: 125,
                                    complete: 0,
                                    values: 625,
                                    filled: 265,
                                    items: vec![
                                        crate::reports::InfoItem {
                                            name: "Typenavn".into(),
                                            present: 125,
                                            required: 125,
                                            accepted: false,
                                            geometry: 0,
                                        },
                                        crate::reports::InfoItem {
                                            name: "Dybde".into(),
                                            present: 15,
                                            required: 125,
                                            accepted: false,
                                            geometry: 0,
                                        },
                                    ],
                                },
                            ],
                        }],
                        findings: 110,
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Revision::default()
            }],
            ..Report::default()
        };
        let mut pictures: FxHashMap<String, Vec<u8>> =
            [("p1".to_owned(), png)].into_iter().collect();
        // A wide screenshot and a small crop pasted into a text block.
        let mut report = report;
        let shot = |w: u32, h: u32, grey: u8| {
            crate::platform::encode_png(w, h, &vec![grey; (w * h * 4) as usize]).unwrap()
        };
        let mut block = Block::new(
            BlockKind::Text,
            2,
            "Bemærkning med billeder",
            "Et skærmbillede fra modellen og et lille udsnit.",
        );
        for (w, h, grey) in [(1600, 700, 200), (300, 120, 150)] {
            let p = crate::reports::store_picture(&mut pictures, shot(w, h, grey)).unwrap();
            block.pictures.push(p);
        }
        report.revisions[0].blocks.insert(4, block);
        let custom = std::env::var("OPENCHECK_SAMPLE_TEMPLATE")
            .ok()
            .map(PathBuf::from);
        let template = crate::typeset::template(custom.as_ref()).unwrap();
        let word = build(
            &template.docx,
            &template.layout.page,
            &report,
            0,
            &pictures,
            true,
        )
        .unwrap();
        std::fs::write(format!("{dir}/sample.docx"), word).unwrap();
        let mut input = crate::typeset::Input::new(&report, 0, &template, true);
        input.attach(&pictures);
        let pages = crate::typeset::compile(input).unwrap();
        std::fs::write(
            format!("{dir}/sample.pdf"),
            crate::typeset::pdf(&pages.doc).unwrap(),
        )
        .unwrap();
        // Every page as a picture too, to look at the layout.
        for i in 0..pages.doc.pages().len() {
            let image = crate::typeset::render(&pages.doc, i, 1.5).unwrap();
            let mut out = Vec::new();
            {
                let mut encoder =
                    png::Encoder::new(&mut out, image.size[0] as u32, image.size[1] as u32);
                encoder.set_color(png::ColorType::Rgba);
                let mut writer = encoder.write_header().unwrap();
                let data: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                writer.write_image_data(&data).unwrap();
            }
            std::fs::write(format!("{dir}/page{}.png", i + 1), out).unwrap();
        }
    }
}
