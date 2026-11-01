//! Typesetting reports with Typst: the PDF that is exported and the pages
//! the Report tab's preview shows, made from a Word template. Everything
//! Typst reads comes from memory: the report's files, the template's
//! pictures, the fonts built into the app and, on the desktop, the
//! computer's fonts a template names.

use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use eframe::egui;
use opencheck_core::docx::{Docx, Layout};
use rustc_hash::FxHashMap;
use typst::diag::{FileError, FileResult, Severity, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration, Label};
use typst::introspection::Introspector;
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::{LazyHash, PicoStr};
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;

use crate::app::OpenCheckApp;
use crate::reports::Report;

/// The app's own fonts, used when a template's font is not at hand.
static OWN_FONTS: LazyLock<Vec<Font>> = LazyLock::new(|| {
    [
        &include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")[..],
        &include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")[..],
        &include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")[..],
    ]
    .into_iter()
    .flat_map(|data| Font::iter(Bytes::new(data)))
    .collect()
});

static LIBRARY: LazyLock<LazyHash<Library>> = LazyLock::new(|| LazyHash::new(Library::default()));

/// The computer's fonts: family (lower case), file and index in it. Read
/// once, the first time a template names a font.
#[cfg(not(target_arch = "wasm32"))]
static SYSTEM_FONTS: LazyLock<Vec<(String, PathBuf, u32)>> = LazyLock::new(|| {
    let mut folders = vec![
        PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into()))
            .join("Fonts"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        folders.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
    }
    folders.push(PathBuf::from("/usr/share/fonts"));
    folders.push(PathBuf::from("/Library/Fonts"));
    let mut found = Vec::new();
    let mut stack = folders;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let font_file = path.extension().is_some_and(|x| {
                matches!(
                    x.to_string_lossy().to_ascii_lowercase().as_str(),
                    "ttf" | "otf" | "ttc" | "otc"
                )
            });
            // Very large files are East Asian fonts reports do not use.
            let small = entry.metadata().is_ok_and(|m| m.len() < 40_000_000);
            if !font_file || !small {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            for (i, info) in typst::text::FontInfo::iter(&data).enumerate() {
                found.push((info.family.to_lowercase(), path.clone(), i as u32));
            }
        }
    }
    found
});

/// The fonts for a template: the app's own and the computer's of the
/// families it names.
fn fonts_for(families: &[String]) -> Vec<Font> {
    #[cfg_attr(target_arch = "wasm32", allow(unused_mut))]
    let mut fonts = OWN_FONTS.clone();
    #[cfg(not(target_arch = "wasm32"))]
    {
        static LOADED: LazyLock<Mutex<FxHashMap<PathBuf, Bytes>>> = LazyLock::new(Default::default);
        let wanted: Vec<String> = families
            .iter()
            .map(|f| f.to_lowercase())
            .filter(|f| !f.starts_with("ibm plex"))
            .collect();
        if wanted.is_empty() {
            return fonts;
        }
        let Ok(mut loaded) = LOADED.lock() else {
            return fonts;
        };
        for (family, path, index) in SYSTEM_FONTS.iter() {
            if !wanted.contains(family) {
                continue;
            }
            let bytes = match loaded.get(path) {
                Some(b) => b.clone(),
                None => match std::fs::read(path) {
                    Ok(data) => {
                        let b = Bytes::new(data);
                        loaded.insert(path.clone(), b.clone());
                        b
                    }
                    Err(_) => continue,
                },
            };
            if let Some(font) = Font::new(bytes, *index) {
                fonts.push(font);
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    let _ = families;
    fonts
}

/// The fonts' book. The app's semibold calls itself "IBM Plex Sans SmBld";
/// as part of "IBM Plex Sans" it is the bold of the regular.
fn book(fonts: &[Font]) -> FontBook {
    FontBook::from_infos(fonts.iter().map(|f| {
        let mut info = f.info().clone();
        if info.family == "IBM Plex Sans SmBld" {
            info.family = "IBM Plex Sans".into();
        }
        info
    }))
}

/// A report template read: the Word file and what it looks like.
pub struct Template {
    pub docx: Docx,
    pub layout: Layout,
}

/// Reads the user's template, or the standard one when `None` (always in
/// the browser).
pub fn template(path: Option<&PathBuf>) -> Result<Arc<Template>, String> {
    let bytes = match path.filter(|_| !crate::platform::IS_WEB) {
        Some(path) => std::fs::read(path)
            .map_err(|e| format!("Could not read the report template {}: {e}", path.display()))?,
        None => crate::report::STANDARD_TEMPLATE.to_vec(),
    };
    let docx = Docx::open(&bytes).map_err(|e| format!("The report template: {e}"))?;
    let layout = docx
        .layout()
        .map_err(|e| format!("The report template: {e}"))?;
    Ok(Arc::new(Template { docx, layout }))
}

/// What a report is typeset from: the template made into Typst and the
/// files it uses.
#[derive(Clone)]
pub struct Input {
    main: String,
    files: FxHashMap<String, Bytes>,
    /// The hashes of the pictures used; their bytes are added by
    /// [`Input::attach`] just before typesetting.
    pictures: Vec<String>,
    /// The font families the template names.
    families: Vec<String>,
}

impl Input {
    /// Revision `index` of a report with a template.
    pub fn new(report: &Report, index: usize, template: &Template) -> Self {
        let main = crate::report::main(&template.layout, report, index);
        let (parts, used) = crate::report::parts(report, index);
        let mut files: FxHashMap<String, Bytes> = parts
            .into_iter()
            .map(|(name, text)| (format!("/{name}"), Bytes::from_string(text)))
            .collect();
        for (part, bytes) in &template.layout.media {
            files.insert(format!("/{part}"), Bytes::new(bytes.clone()));
        }
        Self {
            main,
            files,
            pictures: used,
            families: crate::report::fonts(&template.layout),
        }
    }

    /// Adds the pictures used, PNG by hash.
    pub fn attach(&mut self, pictures: &FxHashMap<String, Vec<u8>>) {
        for hash in &self.pictures {
            if let Some(png) = pictures.get(hash) {
                self.files
                    .insert(crate::report::picture_path(hash), Bytes::new(png.clone()));
            }
        }
    }

    /// A fingerprint, to tell whether the report changed.
    fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = rustc_hash::FxHasher::default();
        self.main.hash(&mut h);
        let mut names: Vec<&String> = self.files.keys().collect();
        names.sort();
        for name in names {
            name.hash(&mut h);
            if name.ends_with(".typ") {
                self.files[name].as_slice().hash(&mut h);
            }
        }
        self.pictures.hash(&mut h);
        h.finish()
    }
}

struct Doc {
    input: Input,
    main: FileId,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

fn file_id(path: &str) -> Option<FileId> {
    VirtualPath::new(path)
        .ok()
        .map(|p| RootedPath::new(VirtualRoot::Project, p).intern())
}

impl World for Doc {
    fn library(&self) -> &LazyHash<Library> {
        &LIBRARY
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main {
            return Ok(Source::new(id, self.input.main.clone()));
        }
        let bytes = self.file(id)?;
        let text = std::str::from_utf8(bytes.as_slice()).map_err(|_| FileError::InvalidUtf8)?;
        Ok(Source::new(id, text.to_owned()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let path = id.vpath().get_with_slash().to_owned();
        self.input
            .files
            .get(&path)
            .cloned()
            .ok_or_else(|| FileError::NotFound(path.into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        let now = opencheck_core::project::now_rfc3339();
        let part = |r: std::ops::Range<usize>| now.get(r).and_then(|t| t.parse::<u16>().ok());
        Datetime::from_ymd(
            part(0..4)?.into(),
            part(5..7)?.try_into().ok()?,
            part(8..10)?.try_into().ok()?,
        )
    }
}

/// A report set in pages.
pub struct Typeset {
    pub doc: PagedDocument,
    /// Where each block starts: page (from 0) and points from its top.
    pub anchors: Vec<Option<(usize, f32)>>,
}

pub type Pages = Arc<Typeset>;

impl Typeset {
    fn new(doc: PagedDocument) -> Self {
        let introspector = doc.introspector().clone();
        let anchors = (0..)
            .map_while(|i| {
                let label = Label::new(PicoStr::intern(&crate::report::anchor(i)))?;
                let content = introspector.query_label(label).ok()?;
                Some(
                    content
                        .location()
                        .and_then(|l| introspector.position(l))
                        .map(|p| (p.page.get() - 1, p.point.y.to_pt() as f32)),
                )
            })
            .collect();
        Self { doc, anchors }
    }

    /// The block shown at a place on a page: the last one starting before it.
    pub fn block_at(&self, page: usize, y: f32) -> Option<usize> {
        self.anchors
            .iter()
            .enumerate()
            .filter_map(|(i, a)| a.map(|a| (i, a)))
            .filter(|(_, (p, top))| (*p, *top) <= (page, y))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(i, _)| i)
    }
}

/// Typesets a report; the errors as readable lines.
pub fn compile(input: Input) -> Result<Pages, String> {
    let main = file_id("/main.typ").ok_or("Bad file name")?;
    let fonts = fonts_for(&input.families);
    let doc = Doc {
        input,
        main,
        book: LazyHash::new(book(&fonts)),
        fonts,
    };
    let result = typst::compile::<PagedDocument>(&doc);
    // Keep the cache of earlier runs small.
    typst::comemo::evict(10);
    result
        .output
        .map(|doc| Arc::new(Typeset::new(doc)))
        .map_err(|errors| explain(&doc, errors.as_slice()))
}

/// The errors of a run, with the report text they come from.
fn explain(doc: &Doc, errors: &[SourceDiagnostic]) -> String {
    errors
        .iter()
        .filter(|e| e.severity == Severity::Error)
        .take(5)
        .map(|e| {
            let near = doc.range(e.span).zip(e.span.id()).and_then(|(range, id)| {
                let text = doc.source(id).ok()?;
                text.text()
                    .get(range)
                    .map(|t| t.chars().take(60).collect::<String>())
            });
            match near {
                Some(near) => format!("{} ({near})", e.message),
                None => e.message.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// The PDF of a typeset report.
pub fn pdf(pages: &PagedDocument) -> Result<Vec<u8>, String> {
    typst_pdf::pdf(pages, &typst_pdf::PdfOptions::default()).map_err(|errors| {
        errors
            .iter()
            .map(|e| e.message.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })
}

/// One page as a picture, `scale` pixels per point.
pub fn render(pages: &PagedDocument, page: usize, scale: f32) -> Option<egui::ColorImage> {
    let page = pages.pages().get(page)?;
    let pixmap = typst_render::render(
        page,
        &typst_render::RenderOptions {
            pixel_per_pt: (scale as f64).into(),
            render_bleed: false,
        },
    );
    Some(egui::ColorImage::from_rgba_premultiplied(
        [pixmap.width() as usize, pixmap.height() as usize],
        pixmap.data(),
    ))
}

// ---- The page preview --------------------------------------------------------

/// A finished run: the fingerprint it was made from and the pages.
type Finished = (u64, Result<Pages, String>);

type TemplateRead = (
    Option<PathBuf>,
    Option<std::time::SystemTime>,
    Result<Arc<Template>, String>,
);

/// Typesetting for the preview, in the background, a moment after the
/// last change.
#[derive(Default)]
pub struct Preview {
    /// The fingerprint of what is shown or being typeset.
    key: u64,
    /// When the report last changed, while waiting to typeset it.
    changed: Option<f64>,
    busy: bool,
    done: Arc<Mutex<Option<Finished>>>,
    pub pages: Option<Pages>,
    pub error: Option<String>,
    /// The template last read: its file, when that changed, and it.
    template: Option<TemplateRead>,
    /// Pictures of the pages, made as they come into view.
    pub textures: FxHashMap<usize, egui::TextureHandle>,
}

/// Pixels per point the pages are drawn with.
pub const SCALE: f32 = 1.6;

/// The quiet time after a change before the pages are set again.
const DEBOUNCE: f64 = 0.5;

impl Preview {
    /// The template, read again when its file changed.
    fn template(&mut self, path: Option<&PathBuf>) -> (Result<Arc<Template>, String>, u64) {
        let modified = path
            .filter(|_| !crate::platform::IS_WEB)
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        let fresh = self
            .template
            .as_ref()
            .is_some_and(|(p, m, _)| p.as_ref() == path && *m == modified);
        if !fresh {
            self.template = Some((path.cloned(), modified, template(path)));
        }
        let stamp = {
            use std::hash::{Hash, Hasher};
            let mut h = rustc_hash::FxHasher::default();
            path.hash(&mut h);
            modified.hash(&mut h);
            h.finish()
        };
        let result = self.template.as_ref().map(|(_, _, t)| t.clone());
        (result.unwrap_or_else(|| Err("no template".into())), stamp)
    }

    /// Brings the pages up to date with the report shown; call every frame
    /// the preview is visible.
    pub fn update(app: &mut OpenCheckApp, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some((key, result)) = app
            .reports
            .preview
            .done
            .lock()
            .ok()
            .and_then(|mut d| d.take())
        {
            let preview = &mut app.reports.preview;
            preview.busy = false;
            if key == preview.key {
                preview.textures.clear();
                match result {
                    Ok(pages) => {
                        preview.pages = Some(pages);
                        preview.error = None;
                    }
                    Err(e) => preview.error = Some(e),
                }
            }
        }
        let path = app.report_template.clone();
        let (template, stamp) = app.reports.preview.template(path.as_ref());
        let template = match template {
            Ok(t) => t,
            Err(e) => {
                app.reports.preview.error = Some(e);
                return;
            }
        };
        let Some((report, shown)) = app.reports.shown() else {
            return;
        };
        let mut input = Input::new(report, shown, &template);
        let key = input.key() ^ stamp;
        let preview = &mut app.reports.preview;
        if key != preview.key {
            preview.key = key;
            // The first time at once, after that when the typing stops.
            preview.changed = Some(if preview.pages.is_none() {
                now - DEBOUNCE
            } else {
                now
            });
        }
        if let Some(since) = preview.changed {
            if preview.busy || now - since < DEBOUNCE {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            preview.changed = None;
            preview.busy = true;
            input.attach(&app.reports.pictures);
            let done = preview.done.clone();
            let ctx = ctx.clone();
            crate::platform::spawn(move || {
                let result = compile(input);
                if let Ok(mut d) = done.lock() {
                    *d = Some((key, result));
                }
                ctx.request_repaint();
            });
        }
    }

    /// Whether the pages are being set or about to be.
    pub fn working(&self) -> bool {
        self.busy || self.changed.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{Report, Revision, default_blocks};

    #[test]
    fn the_standard_template_sets_a_report() {
        let report = Report {
            name: "K&K-rapport \"#1\" [x]".into(),
            revisions: vec![Revision {
                label: "A".into(),
                blocks: default_blocks(),
                snapshot: crate::reports::Snapshot {
                    checks: vec![crate::check_docs::CheckDoc {
                        id: "c1".into(),
                        title: "Levels".into(),
                        verdict: crate::check_docs::Verdict::Rejected,
                        about: "Kontrollen stiller etagerne op.".into(),
                        table: vec![
                            vec!["Kote".into(), "ARK".into(), "KON".into()],
                            vec!["+31.650".into(), "E0".into(), "–".into()],
                            vec!["+35.000".into(), "K0.5 [x] #y".into()],
                        ],
                        ..Default::default()
                    }],
                    ..Default::default()
                },
                ..Revision::default()
            }],
            ..Report::default()
        };
        let template = template(None).unwrap();
        let pages = compile(Input::new(&report, 0, &template)).expect("typesets");
        assert!(!pages.doc.pages().is_empty());
        assert_eq!(pages.anchors.len(), default_blocks().len());
        assert!(pages.anchors.iter().all(Option::is_some));
        assert_eq!(pages.block_at(0, 0.0), None);
        let pdf = pdf(&pages.doc).expect("PDF");
        assert!(pdf.starts_with(b"%PDF"));
        assert!(render(&pages.doc, 0, 0.5).is_some());
    }
}
