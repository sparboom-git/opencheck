//! Word documents (`.docx` and `.dotx`) as report templates: what a
//! template looks like (page, styles, header and footer, the text around
//! the report) read into [`Layout`], and a template filled into a Word
//! document with [`Docx::fill`].
//!
//! Only what a report template needs is read; the rest of a file is kept as
//! it is when it is filled.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use quick_xml::Reader;
use quick_xml::events::Event;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

// ---- A small XML tree ---------------------------------------------------------

/// An XML element with its qualified name (`w:p`) and attributes as written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    El(El),
    Text(String),
    /// XML written as it is (only when writing).
    Raw(String),
}

impl El {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &El> {
        self.children.iter().filter_map(|c| match c {
            Node::El(e) => Some(e),
            _ => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&El> {
        self.elements().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// The first element of that name anywhere below.
    pub fn find(&self, name: &str) -> Option<&El> {
        for e in self.elements() {
            if e.name == name {
                return Some(e);
            }
            if let Some(found) = e.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// The text of all `w:t` below, in order.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.collect_text(&mut out);
        out
    }

    fn collect_text(&self, out: &mut String) {
        for c in &self.children {
            match c {
                Node::El(e) if e.name == "w:t" => {
                    for t in &e.children {
                        if let Node::Text(t) = t {
                            out.push_str(t);
                        }
                    }
                }
                Node::El(e) => e.collect_text(out),
                _ => {}
            }
        }
    }

    /// A child's `w:val`, e.g. of `w:pStyle`.
    fn val(&self, name: &str) -> Option<&str> {
        self.child(name).and_then(|c| c.attr("w:val"))
    }

    fn write(&self, out: &mut String) {
        out.push('<');
        out.push_str(&self.name);
        for (k, v) in &self.attrs {
            out.push(' ');
            out.push_str(k);
            out.push_str("=\"");
            out.push_str(&escape(v));
            out.push('"');
        }
        if self.children.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        for c in &self.children {
            match c {
                Node::El(e) => e.write(out),
                Node::Text(t) => out.push_str(&escape(t)),
                Node::Raw(r) => out.push_str(r),
            }
        }
        out.push_str("</");
        out.push_str(&self.name);
        out.push('>');
    }
}

/// Text for XML.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // Control characters other than tab and line ends are not
            // allowed in XML 1.0; Word refuses a document with one.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn entity(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = name.strip_prefix('#')?;
            let n = match code.strip_prefix('x').or_else(|| code.strip_prefix('X')) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => code.parse().ok()?,
            };
            char::from_u32(n)
        }
    }
}

fn start(e: &quick_xml::events::BytesStart) -> Result<El, String> {
    let mut el = El {
        name: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
        ..El::default()
    };
    for a in e.attributes() {
        let a = a.map_err(|e| e.to_string())?;
        let value = a
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| e.to_string())?
            .into_owned();
        el.attrs
            .push((String::from_utf8_lossy(a.key.as_ref()).into_owned(), value));
    }
    Ok(el)
}

fn push_text(stack: &mut [El], text: &str) {
    if let Some(parent) = stack.last_mut() {
        if let Some(Node::Text(t)) = parent.children.last_mut() {
            t.push_str(text);
        } else {
            parent.children.push(Node::Text(text.to_owned()));
        }
    }
}

/// Parses an XML part into its root element.
pub fn parse(xml: &str) -> Result<El, String> {
    let mut reader = Reader::from_str(xml.trim_start_matches('\u{feff}'));
    let mut stack: Vec<El> = Vec::new();
    loop {
        match reader.read_event().map_err(|e| e.to_string())? {
            Event::Start(e) => stack.push(start(&e)?),
            Event::Empty(e) => {
                let el = start(&e)?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::El(el)),
                    None => return Ok(el),
                }
            }
            Event::End(_) => {
                let el = stack.pop().ok_or("unbalanced end tag")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(Node::El(el)),
                    None => return Ok(el),
                }
            }
            Event::Text(t) => push_text(&mut stack, &t.decode().map_err(|e| e.to_string())?),
            Event::CData(t) => push_text(&mut stack, &t.decode().map_err(|e| e.to_string())?),
            Event::GeneralRef(r) => {
                let name = r.decode().map_err(|e| e.to_string())?;
                let c = entity(&name).ok_or_else(|| format!("unknown entity &{name};"))?;
                push_text(&mut stack, c.encode_utf8(&mut [0; 4]));
            }
            Event::Eof => return Err("the XML ends inside an element".into()),
            _ => {}
        }
    }
}

/// An XML part written back, with its declaration.
pub fn write(root: &El) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\r\n");
    root.write(&mut out);
    out
}

// ---- The package ----------------------------------------------------------------

/// A Word file's parts by name.
#[derive(Clone, Debug, Default)]
pub struct Docx {
    parts: BTreeMap<String, Vec<u8>>,
}

const MAIN: &str = "word/document.xml";

/// A part's relationships: id → target part.
fn relationships(rels: Option<&El>, from: &str) -> BTreeMap<String, String> {
    let dir = from.rsplit_once('/').map_or("", |(d, _)| d);
    rels.map(|r| {
        r.children_named("Relationship")
            .filter(|r| r.attr("TargetMode") != Some("External"))
            .filter_map(|r| Some((r.attr("Id")?.to_owned(), resolve(dir, r.attr("Target")?))))
            .collect()
    })
    .unwrap_or_default()
}

/// A relationship target relative to a folder, as a part name.
fn resolve(dir: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_owned();
    }
    let mut path: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
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

fn rels_name(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

impl Docx {
    /// Reads a `.docx` or `.dotx` file.
    pub fn open(bytes: &[u8]) -> Result<Self, String> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| format!("not a Word file: {e}"))?;
        let mut parts = BTreeMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
            if entry.is_dir() {
                continue;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            parts.insert(name, data);
        }
        if !parts.contains_key(MAIN) {
            return Err("not a Word document (no word/document.xml)".into());
        }
        Ok(Self { parts })
    }

    fn xml(&self, name: &str) -> Option<El> {
        self.parts
            .get(name)
            .and_then(|b| parse(&String::from_utf8_lossy(b)).ok())
    }

    fn rels(&self, part: &str) -> BTreeMap<String, String> {
        relationships(self.xml(&rels_name(part)).as_ref(), part)
    }

    /// The bytes of a part, e.g. a picture.
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        self.parts.get(name).map(Vec::as_slice)
    }

    fn to_bytes(&self) -> Result<Vec<u8>, String> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        // The content types first, as Word writes them.
        let order = self
            .parts
            .iter()
            .filter(|(n, _)| *n == "[Content_Types].xml")
            .chain(
                self.parts
                    .iter()
                    .filter(|(n, _)| *n != "[Content_Types].xml"),
            );
        for (name, bytes) in order {
            zip.start_file(name.as_str(), options)
                .map_err(|e| e.to_string())?;
            zip.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
    }

    // ---- What the template looks like --------------------------------------------

    /// The template read: page, styles, header and footer and its text.
    pub fn layout(&self) -> Result<Layout, String> {
        let document = self.xml(MAIN).ok_or("word/document.xml cannot be read")?;
        let body = document.child("w:body").ok_or("the document has no body")?;
        let rels = self.rels(MAIN);
        let styles = self.styles();
        let mut layout = Layout::default();
        let mut reader = PartReader {
            docx: self,
            rels: &rels,
            media: &mut layout.media,
            styles: &styles,
        };
        layout.body = reader.blocks(body);

        let section = body
            .child("w:sectPr")
            .or_else(|| body.find("w:sectPr"))
            .cloned()
            .unwrap_or_default();
        let twips = |e: Option<&El>, a: &str, default: f32| {
            e.and_then(|e| e.attr(a))
                .and_then(|v| v.parse::<f32>().ok())
                .map_or(default, |v| v / 20.0)
        };
        let size = section.child("w:pgSz");
        let margin = section.child("w:pgMar");
        layout.page = Page {
            width: twips(size, "w:w", 595.3),
            height: twips(size, "w:h", 841.9),
            top: twips(margin, "w:top", 72.0).abs(),
            bottom: twips(margin, "w:bottom", 72.0).abs(),
            left: twips(margin, "w:left", 72.0),
            right: twips(margin, "w:right", 72.0),
            header: twips(margin, "w:header", 35.4),
            footer: twips(margin, "w:footer", 35.4),
        };
        layout.title_page = section
            .child("w:titlePg")
            .is_some_and(|t| !matches!(t.attr("w:val"), Some("0" | "false")));
        for (kind, slot) in [("w:headerReference", 0), ("w:footerReference", 1)] {
            for reference in section.children_named(kind) {
                let Some(target) = reference.attr("r:id").and_then(|id| rels.get(id)) else {
                    continue;
                };
                let Some(root) = self.xml(target) else {
                    continue;
                };
                let part_rels = self.rels(target);
                let mut reader = PartReader {
                    docx: self,
                    rels: &part_rels,
                    media: &mut layout.media,
                    styles: &styles,
                };
                let blocks = reader.blocks(&root);
                let first = reference.attr("w:type") == Some("first");
                match (slot, first) {
                    (0, false) => layout.header = blocks,
                    (0, true) => layout.header_first = Some(blocks),
                    (_, false) => layout.footer = blocks,
                    (_, true) => layout.footer_first = Some(blocks),
                }
            }
        }
        layout.styles = styles;
        Ok(layout)
    }

    /// The theme's body and heading fonts.
    fn theme_fonts(&self) -> (Option<String>, Option<String>) {
        let rels = self.rels(MAIN);
        let theme = rels
            .values()
            .find(|t| t.contains("theme/"))
            .and_then(|t| self.xml(t));
        let font = |kind: &str| {
            theme
                .as_ref()
                .and_then(|t| t.find(kind))
                .and_then(|f| f.child("a:latin"))
                .and_then(|l| l.attr("typeface"))
                .filter(|f| !f.is_empty())
                .map(str::to_owned)
        };
        (font("a:minorFont"), font("a:majorFont"))
    }

    fn styles(&self) -> Styles {
        let mut styles = Styles::default();
        let Some(root) = self.xml("word/styles.xml") else {
            return styles;
        };
        let (minor, major) = self.theme_fonts();
        styles.minor_font = minor.clone();
        styles.major_font = major.clone();
        let fonts = ThemeFonts { minor, major };
        if let Some(defaults) = root.child("w:docDefaults") {
            if let Some(r) = defaults.find("w:rPr") {
                styles.defaults.run = run_format(r, &fonts);
            }
            if let Some(p) = defaults.find("w:pPr") {
                styles.defaults.para = para_format(p);
            }
        }
        // Each style with what it adds; the chains are resolved below.
        let mut raw: BTreeMap<String, (Option<String>, Style)> = BTreeMap::new();
        for s in root.children_named("w:style") {
            let Some(id) = s.attr("w:styleId") else {
                continue;
            };
            let style = Style {
                name: s.val("w:name").unwrap_or(id).to_owned(),
                kind: s.attr("w:type").unwrap_or("paragraph").to_owned(),
                run: s
                    .child("w:rPr")
                    .map(|r| run_format(r, &fonts))
                    .unwrap_or_default(),
                para: s.child("w:pPr").map(para_format).unwrap_or_default(),
            };
            if s.attr("w:default") == Some("1") && style.kind == "paragraph" {
                styles.default_para = Some(id.to_owned());
            }
            raw.insert(
                id.to_owned(),
                (s.val("w:basedOn").map(str::to_owned), style),
            );
        }
        for id in raw.keys() {
            let mut chain = vec![id.clone()];
            while let Some(base) = raw.get(chain.last().unwrap()).and_then(|(b, _)| b.clone()) {
                if chain.contains(&base) || chain.len() > 20 {
                    break;
                }
                chain.push(base);
            }
            let mut style = raw[id].1.clone();
            style.run = RunFormat::default();
            style.para = ParaFormat::default();
            for link in chain.iter().rev() {
                if let Some((_, s)) = raw.get(link) {
                    style.run = style.run.over(&s.run);
                    style.para = style.para.over(&s.para);
                }
            }
            styles.by_id.insert(id.clone(), style);
        }
        styles
    }

    // ---- Filling ----------------------------------------------------------------

    /// The id of the paragraph style with this name (e.g. "heading 1"),
    /// or of the first with this id.
    pub fn style_id(&self, name: &str) -> Option<String> {
        let styles = self.styles();
        styles
            .by_id
            .iter()
            .find(|(_, s)| s.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| id.clone())
    }

    /// A Word document from the template: `{{field}}` texts replaced in the
    /// body, headers and footers; a paragraph holding only `{{name}}` of a
    /// part in `parts` replaced by that part's XML (body elements); and
    /// `pictures` added as `word/media/<name>`, related to the body by the
    /// ids [`picture_rel`] gives.
    pub fn fill(
        &self,
        fields: &[(&str, String)],
        parts: &[(&str, String)],
        pictures: &[(String, Vec<u8>)],
    ) -> Result<Vec<u8>, String> {
        let mut out = self.clone();
        let names: Vec<String> = self
            .parts
            .keys()
            .filter(|n| {
                *n == MAIN
                    || (n.starts_with("word/header") || n.starts_with("word/footer"))
                        && n.ends_with(".xml")
            })
            .cloned()
            .collect();
        for name in names {
            let Some(mut root) = self.xml(&name) else {
                continue;
            };
            fill_element(&mut root, fields, if name == MAIN { parts } else { &[] });
            out.parts.insert(name, write(&root).into_bytes());
        }
        // The document's title and the like may hold fields too.
        if let Some(mut core) = self.xml("docProps/core.xml") {
            replace_all_texts(&mut core, fields);
            out.parts
                .insert("docProps/core.xml".into(), write(&core).into_bytes());
        }
        // The pictures and their relationships.
        if !pictures.is_empty() {
            let rels_part = rels_name(MAIN);
            let mut rels = self.xml(&rels_part).unwrap_or_else(|| El {
                name: "Relationships".into(),
                attrs: vec![(
                    "xmlns".into(),
                    "http://schemas.openxmlformats.org/package/2006/relationships".into(),
                )],
                children: vec![],
            });
            for (i, (name, bytes)) in pictures.iter().enumerate() {
                out.parts
                    .insert(format!("word/media/{name}"), bytes.clone());
                rels.children.push(Node::El(El {
                    name: "Relationship".into(),
                    attrs: vec![
                        ("Id".into(), picture_rel(i)),
                        (
                            "Type".into(),
                            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image".into(),
                        ),
                        ("Target".into(), format!("media/{name}")),
                    ],
                    children: vec![],
                }));
            }
            out.parts.insert(rels_part, write(&rels).into_bytes());
        }
        // A template (.dotx) becomes a document; PNG gets a content type.
        if let Some(mut types) = self.xml("[Content_Types].xml") {
            for e in types.children.iter_mut() {
                if let Node::El(e) = e {
                    for (k, v) in e.attrs.iter_mut() {
                        if k == "ContentType" && v.contains("wordprocessingml.template.main+xml") {
                            *v = v.replace("template.main", "document.main");
                        }
                    }
                }
            }
            let has_png = types.children_named("Default").any(|d| {
                d.attr("Extension")
                    .is_some_and(|x| x.eq_ignore_ascii_case("png"))
            });
            if !has_png && !pictures.is_empty() {
                types.children.insert(
                    0,
                    Node::El(El {
                        name: "Default".into(),
                        attrs: vec![
                            ("Extension".into(), "png".into()),
                            ("ContentType".into(), "image/png".into()),
                        ],
                        children: vec![],
                    }),
                );
            }
            out.parts
                .insert("[Content_Types].xml".into(), write(&types).into_bytes());
        }
        out.to_bytes()
    }
}

/// The relationship id of picture `i` added by [`Docx::fill`].
pub fn picture_rel(i: usize) -> String {
    format!("rIdOpenCheck{i}")
}

/// Joins the runs of a paragraph so no `{{field}}` is split over runs, as
/// Word often does after editing.
fn join_split_fields(p: &mut El) {
    let runs: Vec<usize> = p
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Node::El(e) if e.name == "w:r" && e.child("w:t").is_some()))
        .map(|(i, _)| i)
        .collect();
    let text_of = |p: &El, i: usize| match &p.children[i] {
        Node::El(e) => e.text(),
        _ => String::new(),
    };
    let open = |t: &str| t.matches("{{").count() > t.matches("}}").count() || t.ends_with('{');
    let mut k = 0;
    while k < runs.len() {
        let mut text = text_of(p, runs[k]);
        if !open(&text) {
            k += 1;
            continue;
        }
        let mut j = k + 1;
        while j < runs.len() && open(&text) {
            text.push_str(&text_of(p, runs[j]));
            set_run_text(&mut p.children[runs[j]], "");
            j += 1;
        }
        set_run_text(&mut p.children[runs[k]], &text);
        k = j;
    }
}

fn set_run_text(run: &mut Node, text: &str) {
    let Node::El(run) = run else {
        return;
    };
    let mut first = true;
    run.children.retain_mut(|c| match c {
        Node::El(e) if e.name == "w:t" => {
            if first {
                first = false;
                e.attrs.retain(|(k, _)| k != "xml:space");
                e.attrs.push(("xml:space".into(), "preserve".into()));
                e.children = if text.is_empty() {
                    vec![]
                } else {
                    vec![Node::Text(text.to_owned())]
                };
                true
            } else {
                false
            }
        }
        _ => true,
    });
}

/// Replaces `{{name}}` by its value in a text.
pub fn substitute(text: &str, fields: &[(&str, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let name = after[..end].trim();
        match fields.iter().find(|(k, _)| *k == name) {
            Some((_, v)) => out.push_str(v),
            None => out.push_str(&rest[start..start + end + 4]),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// The name of the part a paragraph stands for: `{{content}}` alone in it.
fn part_marker(p: &El) -> Option<String> {
    let text = p.text();
    let name = text.trim().strip_prefix("{{")?.strip_suffix("}}")?.trim();
    (!name.contains("{{")).then(|| name.to_owned())
}

fn fill_element(el: &mut El, fields: &[(&str, String)], parts: &[(&str, String)]) {
    for child in el.children.iter_mut() {
        let Node::El(e) = child else {
            continue;
        };
        if e.name == "w:p" {
            join_split_fields(e);
            if let Some(name) = part_marker(e)
                && let Some((_, xml)) = parts.iter().find(|(n, _)| *n == name)
            {
                *child = Node::Raw(xml.clone());
                continue;
            }
            replace_in_texts(e, fields);
        } else {
            fill_element(e, fields, parts);
        }
    }
}

fn replace_all_texts(el: &mut El, fields: &[(&str, String)]) {
    for c in el.children.iter_mut() {
        match c {
            Node::Text(t) if t.contains("{{") => *t = substitute(t, fields),
            Node::El(e) => replace_all_texts(e, fields),
            _ => {}
        }
    }
}

fn replace_in_texts(el: &mut El, fields: &[(&str, String)]) {
    for c in el.children.iter_mut() {
        match c {
            Node::Text(t) if el.name == "w:t" && t.contains("{{") => {
                *t = substitute(t, fields);
            }
            Node::El(e) => replace_in_texts(e, fields),
            _ => {}
        }
    }
}

// ---- The layout model ---------------------------------------------------------

/// Page size and margins in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Page {
    pub width: f32,
    pub height: f32,
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
    /// From the top edge to the header, and the bottom edge to the footer.
    pub header: f32,
    pub footer: f32,
}

/// What a template looks like.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    pub page: Page,
    pub styles: Styles,
    /// A different header and footer on the first page.
    pub title_page: bool,
    pub header: Vec<Block>,
    pub header_first: Option<Vec<Block>>,
    pub footer: Vec<Block>,
    pub footer_first: Option<Vec<Block>>,
    /// The text of the document; a paragraph `{{content}}` marks the report.
    pub body: Vec<Block>,
    /// Pictures used, by part name.
    pub media: BTreeMap<String, Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Para(Para),
    Table(Table),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Para {
    pub style: Option<String>,
    pub format: ParaFormat,
    pub runs: Vec<Run>,
}

impl Para {
    /// The paragraph's text, fields included as they read.
    pub fn text(&self) -> String {
        self.runs
            .iter()
            .filter_map(|r| match r {
                Run::Text(t, _) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Text(String, RunFormat),
    Tab,
    Break,
    PageBreak,
    /// The page number.
    Page(RunFormat),
    /// The number of pages.
    Pages(RunFormat),
    /// A picture by part name, its size in points.
    Picture {
        part: String,
        width: f32,
        height: f32,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Column widths in points.
    pub widths: Vec<f32>,
    pub rows: Vec<Vec<Vec<Block>>>,
}

/// Character formatting; `None` where it is inherited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunFormat {
    pub font: Option<String>,
    /// Points.
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub caps: Option<bool>,
    /// Hex RGB, e.g. "1F3864".
    pub color: Option<String>,
}

impl RunFormat {
    /// `top` over `self`: what `top` sets wins.
    pub fn over(&self, top: &RunFormat) -> RunFormat {
        RunFormat {
            font: top.font.clone().or_else(|| self.font.clone()),
            size: top.size.or(self.size),
            bold: top.bold.or(self.bold),
            italic: top.italic.or(self.italic),
            underline: top.underline.or(self.underline),
            caps: top.caps.or(self.caps),
            color: top.color.clone().or_else(|| self.color.clone()),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

/// Paragraph formatting; `None` where it is inherited.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParaFormat {
    pub align: Option<Align>,
    /// Points.
    pub before: Option<f32>,
    pub after: Option<f32>,
    /// A line under the paragraph: width in points and colour.
    pub rule_below: Option<(f32, String)>,
    pub page_break_before: Option<bool>,
}

impl ParaFormat {
    pub fn over(&self, top: &ParaFormat) -> ParaFormat {
        ParaFormat {
            align: top.align.or(self.align),
            before: top.before.or(self.before),
            after: top.after.or(self.after),
            rule_below: top.rule_below.clone().or_else(|| self.rule_below.clone()),
            page_break_before: top.page_break_before.or(self.page_break_before),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Style {
    /// Word's name, e.g. "heading 1" (also in a Danish Word).
    pub name: String,
    /// "paragraph", "character", "table" …
    pub kind: String,
    pub run: RunFormat,
    pub para: ParaFormat,
}

/// The template's styles, each with its inherited formatting resolved.
#[derive(Clone, Debug, Default)]
pub struct Styles {
    /// The theme's body and heading fonts.
    pub minor_font: Option<String>,
    pub major_font: Option<String>,
    pub defaults: Style,
    pub default_para: Option<String>,
    pub by_id: BTreeMap<String, Style>,
}

impl Styles {
    /// A paragraph style's formatting with the document defaults below it.
    pub fn para_style(&self, id: Option<&str>) -> Style {
        let id = id.or(self.default_para.as_deref());
        let base = &self.defaults;
        match id.and_then(|id| self.by_id.get(id)) {
            Some(s) => Style {
                name: s.name.clone(),
                kind: s.kind.clone(),
                run: base.run.over(&s.run),
                para: base.para.over(&s.para),
            },
            None => base.clone(),
        }
    }

    /// The paragraph style named so (e.g. "heading 1"), resolved.
    pub fn named(&self, name: &str) -> Option<Style> {
        let id = self
            .by_id
            .iter()
            .find(|(_, s)| s.name.eq_ignore_ascii_case(name))
            .map(|(id, _)| id.clone())?;
        Some(self.para_style(Some(&id)))
    }

    /// Every font named in the styles.
    pub fn fonts(&self) -> Vec<String> {
        let mut list: Vec<String> = self
            .by_id
            .values()
            .chain(std::iter::once(&self.defaults))
            .filter_map(|s| s.run.font.clone())
            .collect();
        list.sort();
        list.dedup();
        list
    }
}

struct ThemeFonts {
    minor: Option<String>,
    major: Option<String>,
}

fn on(e: &El) -> bool {
    !matches!(e.attr("w:val"), Some("0" | "false" | "off" | "none"))
}

fn run_format(r: &El, theme: &ThemeFonts) -> RunFormat {
    let font = r.child("w:rFonts").and_then(|f| {
        f.attr("w:ascii")
            .or_else(|| f.attr("w:hAnsi"))
            .map(str::to_owned)
            .or_else(
                || match f.attr("w:asciiTheme").or_else(|| f.attr("w:hAnsiTheme")) {
                    Some(t) if t.starts_with("major") => theme.major.clone(),
                    Some(_) => theme.minor.clone(),
                    None => None,
                },
            )
    });
    RunFormat {
        font,
        size: r
            .val("w:sz")
            .and_then(|v| v.parse::<f32>().ok())
            .map(|v| v / 2.0),
        bold: r.child("w:b").map(on),
        italic: r.child("w:i").map(on),
        underline: r.child("w:u").map(on),
        caps: r.child("w:caps").map(on),
        color: r
            .val("w:color")
            .filter(|c| c.len() == 6 && c.chars().all(|c| c.is_ascii_hexdigit()))
            .map(str::to_owned),
    }
}

fn para_format(p: &El) -> ParaFormat {
    let spacing = p.child("w:spacing");
    let points = |a: &str| {
        spacing
            .and_then(|s| s.attr(a))
            .and_then(|v| v.parse::<f32>().ok())
            .map(|v| v / 20.0)
    };
    let rule_below = p
        .child("w:pBdr")
        .and_then(|b| b.child("w:bottom"))
        .and_then(|b| {
            if !on(b) {
                return None;
            }
            // Eighths of a point.
            let width = b
                .attr("w:sz")
                .and_then(|v| v.parse::<f32>().ok())
                .map_or(0.5, |v| v / 8.0);
            let color = b
                .attr("w:color")
                .filter(|c| c.len() == 6)
                .unwrap_or("000000");
            Some((width, color.to_owned()))
        });
    ParaFormat {
        align: p.val("w:jc").map(|j| match j {
            "center" => Align::Center,
            "right" | "end" => Align::Right,
            "both" | "distribute" => Align::Justify,
            _ => Align::Left,
        }),
        before: points("w:before"),
        after: points("w:after"),
        rule_below,
        page_break_before: p.child("w:pageBreakBefore").map(on),
    }
}

/// Reads the blocks of a part (body, header, footer, table cell).
struct PartReader<'a> {
    docx: &'a Docx,
    rels: &'a BTreeMap<String, String>,
    media: &'a mut BTreeMap<String, Vec<u8>>,
    styles: &'a Styles,
}

#[derive(PartialEq)]
enum FieldState {
    Normal,
    Instruction(String),
    /// The shown result of a page field, replaced by the field itself.
    SkipResult,
}

impl PartReader<'_> {
    fn blocks(&mut self, el: &El) -> Vec<Block> {
        let mut out = Vec::new();
        for e in el.elements() {
            match e.name.as_str() {
                "w:p" => out.push(Block::Para(self.para(e))),
                "w:tbl" => out.push(Block::Table(self.table(e))),
                // Content controls and the like hold paragraphs too.
                "w:sdt" => {
                    if let Some(content) = e.child("w:sdtContent") {
                        out.extend(self.blocks(content));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn table(&mut self, t: &El) -> Table {
        let widths = t
            .child("w:tblGrid")
            .map(|g| {
                g.children_named("w:gridCol")
                    .map(|c| {
                        c.attr("w:w")
                            .and_then(|v| v.parse::<f32>().ok())
                            .map_or(0.0, |v| v / 20.0)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let rows = t
            .children_named("w:tr")
            .map(|r| r.children_named("w:tc").map(|c| self.blocks(c)).collect())
            .collect();
        Table { widths, rows }
    }

    fn para(&mut self, p: &El) -> Para {
        let mut para = Para {
            style: p
                .child("w:pPr")
                .and_then(|pr| pr.val("w:pStyle"))
                .map(str::to_owned),
            format: p.child("w:pPr").map(para_format).unwrap_or_default(),
            runs: Vec::new(),
        };
        let mut joined = p.clone();
        join_split_fields(&mut joined);
        let mut state = FieldState::Normal;
        self.runs(&joined, &mut para.runs, &mut state);
        para
    }

    fn runs(&mut self, el: &El, out: &mut Vec<Run>, state: &mut FieldState) {
        for e in el.elements() {
            match e.name.as_str() {
                "w:r" => self.run(e, out, state),
                "w:fldSimple" => {
                    let instr = e.attr("w:instr").unwrap_or_default().trim().to_uppercase();
                    let format = e
                        .find("w:rPr")
                        .map(|r| run_format(r, &self.theme()))
                        .unwrap_or_default();
                    if instr.starts_with("PAGE") {
                        out.push(Run::Page(format));
                    } else if instr.starts_with("NUMPAGES") || instr.starts_with("SECTIONPAGES") {
                        out.push(Run::Pages(format));
                    } else {
                        self.runs(e, out, state);
                    }
                }
                "w:hyperlink" | "w:smartTag" | "w:ins" | "w:sdt" | "w:sdtContent" => {
                    self.runs(e, out, state)
                }
                _ => {}
            }
        }
    }

    fn theme(&self) -> ThemeFonts {
        ThemeFonts {
            minor: self.styles.minor_font.clone(),
            major: self.styles.major_font.clone(),
        }
    }

    fn run(&mut self, r: &El, out: &mut Vec<Run>, state: &mut FieldState) {
        let mut format = RunFormat::default();
        if let Some(rpr) = r.child("w:rPr") {
            if let Some(style) = rpr.val("w:rStyle").and_then(|id| self.styles.by_id.get(id)) {
                format = style.run.clone();
            }
            format = format.over(&run_format(rpr, &self.theme()));
        }
        for e in r.elements() {
            match e.name.as_str() {
                "w:fldChar" => match e.attr("w:fldCharType") {
                    Some("begin") => *state = FieldState::Instruction(String::new()),
                    Some("separate") => {
                        if let FieldState::Instruction(instr) = &*state {
                            let instr = instr.trim().to_uppercase();
                            if instr.starts_with("PAGE") {
                                out.push(Run::Page(format.clone()));
                                *state = FieldState::SkipResult;
                            } else if instr.starts_with("NUMPAGES")
                                || instr.starts_with("SECTIONPAGES")
                            {
                                out.push(Run::Pages(format.clone()));
                                *state = FieldState::SkipResult;
                            } else {
                                *state = FieldState::Normal;
                            }
                        }
                    }
                    _ => *state = FieldState::Normal,
                },
                "w:instrText" => {
                    if let FieldState::Instruction(instr) = state {
                        instr.push_str(&text_of(e));
                    }
                }
                _ if *state != FieldState::Normal => {}
                "w:t" => {
                    let text = text_of(e);
                    if !text.is_empty() {
                        out.push(Run::Text(text, format.clone()));
                    }
                }
                "w:tab" | "w:ptab" => out.push(Run::Tab),
                "w:br" | "w:cr" => out.push(if e.attr("w:type") == Some("page") {
                    Run::PageBreak
                } else {
                    Run::Break
                }),
                "w:drawing" => {
                    if let Some(picture) = self.picture(e) {
                        out.push(picture);
                    }
                }
                "w:pict" => {
                    if let Some(picture) = self.vml_picture(e) {
                        out.push(picture);
                    }
                }
                _ => {}
            }
        }
    }

    fn picture(&mut self, drawing: &El) -> Option<Run> {
        let extent = drawing.find("wp:extent")?;
        let emu = |a: &str| {
            extent
                .attr(a)
                .and_then(|v| v.parse::<f32>().ok())
                .map_or(0.0, |v| v / 12700.0)
        };
        let id = drawing.find("a:blip")?.attr("r:embed")?;
        let part = self.rels.get(id)?.clone();
        self.keep(&part)?;
        Some(Run::Picture {
            part,
            width: emu("cx"),
            height: emu("cy"),
        })
    }

    fn vml_picture(&mut self, pict: &El) -> Option<Run> {
        let id = pict.find("v:imagedata")?.attr("r:id")?;
        let part = self.rels.get(id)?.clone();
        let style = pict
            .find("v:shape")
            .and_then(|s| s.attr("style"))
            .unwrap_or("");
        let length = |key: &str| {
            style.split(';').find_map(|kv| {
                let (k, v) = kv.split_once(':')?;
                (k.trim() == key).then(|| v.trim().trim_end_matches("pt").parse::<f32>().ok())?
            })
        };
        self.keep(&part)?;
        Some(Run::Picture {
            part,
            width: length("width").unwrap_or(100.0),
            height: length("height").unwrap_or(50.0),
        })
    }

    fn keep(&mut self, part: &str) -> Option<()> {
        let bytes = self.docx.parts.get(part)?;
        self.media.insert(part.to_owned(), bytes.clone());
        Some(())
    }
}

fn text_of(e: &El) -> String {
    e.children
        .iter()
        .filter_map(|c| match c {
            Node::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

/// Width and height in pixels of a PNG.
pub fn png_size(png: &[u8]) -> Option<(u32, u32)> {
    if png.len() < 24 || &png[1..4] != b"PNG" {
        return None;
    }
    let w = u32::from_be_bytes(png[16..20].try_into().ok()?);
    let h = u32::from_be_bytes(png[20..24].try_into().ok()?);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_drops_characters_xml_does_not_allow() {
        assert_eq!(
            escape("a\u{0B}b & <c> \"d\""),
            "ab &amp; &lt;c&gt; &quot;d&quot;"
        );
        assert_eq!(escape("tab\tline\nend\r"), "tab\tline\nend\r");
        assert_eq!(escape("Ø160 \u{1F}x\u{0}"), "Ø160 x");
    }

    #[test]
    fn xml_round_trip_keeps_text_and_attributes() {
        let xml = r#"<w:p a="1 &amp; 2"><w:r><w:t xml:space="preserve"> x &lt; y </w:t></w:r><w:br/></w:p>"#;
        let root = parse(xml).unwrap();
        assert_eq!(root.attr("a"), Some("1 & 2"));
        assert_eq!(root.text(), " x < y ");
        let mut out = String::new();
        root.write(&mut out);
        assert_eq!(out, xml);
    }

    #[test]
    fn fields_split_over_runs_are_joined() {
        let xml = "<w:p><w:r><w:t>Projekt: {{pro</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t>ject}} og {</w:t></w:r><w:r><w:t>{report}}</w:t></w:r></w:p>";
        let mut p = parse(xml).unwrap();
        join_split_fields(&mut p);
        let runs: Vec<String> = p.children_named("w:r").map(El::text).collect();
        assert_eq!(runs[0], "Projekt: {{project}} og {{report}}");
        assert!(runs[1..].iter().all(String::is_empty));
        replace_in_texts(
            &mut p,
            &[("project", "Skolen".into()), ("report", "K&K".into())],
        );
        assert_eq!(p.text(), "Projekt: Skolen og K&K");
    }

    #[test]
    fn substitutes_known_fields_only() {
        let fields = [("a", "1".to_owned())];
        assert_eq!(
            substitute("{{a}}-{{ a }}-{{b}}-{{", &fields),
            "1-1-{{b}}-{{"
        );
    }

    #[test]
    fn relationship_targets_resolve_to_parts() {
        assert_eq!(resolve("word", "media/image1.png"), "word/media/image1.png");
        assert_eq!(
            resolve("word", "../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(resolve("word", "/word/media/x.png"), "word/media/x.png");
        assert_eq!(
            rels_name("word/document.xml"),
            "word/_rels/document.xml.rels"
        );
    }

    #[test]
    fn png_size_reads_the_header() {
        let mut png = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10, 0, 0, 0, 13];
        png.extend(b"IHDR");
        png.extend(300u32.to_be_bytes());
        png.extend(200u32.to_be_bytes());
        assert_eq!(png_size(&png), Some((300, 200)));
    }
}
