//! Documented checks: a visual check written down as done, with how it was
//! done, what it showed (pictures of the view, the colours and how many
//! elements had each) and its result, green, orange or red. No elements are
//! kept: a check of all columns is about the columns, not each of them.
//! Reports take them in (the "Visuelle kontroller" block).

use std::sync::Arc;

use eframe::egui::{self, Color32, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::project::{self, DOCUMENTATION_PICTURES, ProjectDocuments};
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use opencheck_core::visual::{AlignMode, VisualKind, VisualRule};

use crate::app::OpenCheckApp;
use crate::theme;

/// The result of a check, as a traffic light.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    #[default]
    Approved,
    Remarks,
    Rejected,
}

impl Verdict {
    pub const ALL: [Verdict; 3] = [Verdict::Approved, Verdict::Remarks, Verdict::Rejected];

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Approved => "Approved",
            Verdict::Remarks => "Approved with remarks",
            Verdict::Rejected => "Not approved",
        }
    }

    /// As the report says it.
    pub fn label_da(self) -> &'static str {
        match self {
            Verdict::Approved => "Godkendt",
            Verdict::Remarks => "Godkendt med bemærkninger",
            Verdict::Rejected => "Ikke godkendt",
        }
    }

    /// Green, orange, red, as hex RGB.
    pub fn hex(self) -> &'static str {
        match self {
            Verdict::Approved => "2E9E4A",
            Verdict::Remarks => "E08A1E",
            Verdict::Rejected => "D64541",
        }
    }

    pub fn color(self) -> Color32 {
        let v = u32::from_str_radix(self.hex(), 16).unwrap_or(0);
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }
}

/// A colour of the check and how many elements had it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Swatch {
    pub label: String,
    pub detail: String,
    pub color: [u8; 3],
    pub count: usize,
}

/// One documented check.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckDoc {
    pub id: String,
    pub title: String,
    /// The visual check it was made from and its kind.
    pub check: String,
    pub kind: String,
    /// What the check does, written by OpenCheck from its settings.
    pub about: String,
    /// What was checked and how.
    pub method: String,
    pub verdict: Verdict,
    pub by: String,
    pub created: String,
    pub modified: String,
    /// What the check found, e.g. `12 systems (1,830 elements)`.
    pub summary: String,
    /// Elements it marked as problems.
    pub problems: usize,
    pub legend: Vec<Swatch>,
    /// The report leaves the colours out.
    pub hide_colours: bool,
    /// A table of the result (levels, rooms per storey), the first row its
    /// header.
    pub table: Vec<Vec<String>>,
    /// The models and their versions, e.g. `K09_ZKON.ifc (rev. 2)`.
    pub models: Vec<String>,
    /// Pictures of the view, PNG by content hash.
    pub pictures: Vec<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    checks: Vec<CheckDoc>,
}

/// A documented check being written or changed.
pub struct Editing {
    pub doc: CheckDoc,
    /// Its place in the list; `None` for a new one.
    pub index: Option<usize>,
    pub confirm_delete: bool,
}

/// The project's documented checks.
#[derive(Default)]
pub struct CheckDocs {
    pub list: Vec<CheckDoc>,
    /// PNG by content hash.
    pub pictures: FxHashMap<String, Vec<u8>>,
    pub editing: Option<Editing>,
    /// A picture of the view being taken: frames waited.
    pub shot: Option<u16>,
    textures: FxHashMap<String, egui::TextureHandle>,
    leftovers: crate::lenient::Leftovers,
}

/// Frames before the picture (the window drawn without dialogs) and the
/// longest wait for it.
const SETTLE: u16 = 3;
const TIMEOUT: u16 = 120;
/// Pictures of whole models want more detail than those of one issue.
const PICTURE_WIDTH: usize = 1600;

impl CheckDocs {
    /// `documentation.json` and the pictures in use.
    pub fn save_into(&self, documents: &mut ProjectDocuments) {
        if self.list.is_empty() && self.leftovers.is_empty() {
            return;
        }
        let mut json = serde_json::to_value(Saved {
            checks: self.list.clone(),
        })
        .expect("documented checks always serialise");
        self.leftovers.restore::<Saved>(&mut json);
        documents.documentation = Some(json);
        let mut used: Vec<&String> = self.list.iter().flat_map(|d| &d.pictures).collect();
        used.sort();
        used.dedup();
        documents
            .pictures
            .extend(used.into_iter().filter_map(|hash| {
                let png = self.pictures.get(hash)?;
                Some((format!("{DOCUMENTATION_PICTURES}{hash}.png"), png.clone()))
            }));
    }

    /// Reads the documented checks of an opened project.
    pub fn load(documents: &ProjectDocuments) -> (Self, Vec<String>) {
        let (saved, leftovers, errors) = match &documents.documentation {
            Some(json) => {
                crate::lenient::read::<Saved>(json.clone(), &["checks"], "documentation.json")
            }
            None => Default::default(),
        };
        let pictures = documents
            .pictures
            .iter()
            .filter_map(|(name, png)| {
                let hash = name
                    .strip_prefix(DOCUMENTATION_PICTURES)?
                    .strip_suffix(".png")?;
                Some((hash.to_owned(), png.clone()))
            })
            .collect();
        (
            Self {
                list: saved.checks,
                pictures,
                leftovers,
                ..Self::default()
            },
            errors,
        )
    }

    fn texture(&mut self, ctx: &egui::Context, hash: &str) -> Option<egui::TextureHandle> {
        if let Some(t) = self.textures.get(hash) {
            return Some(t.clone());
        }
        let image = crate::report_ui::png_image(self.pictures.get(hash)?)?;
        let texture = ctx.load_texture(
            format!("check-doc-{hash}"),
            image,
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert(hash.to_owned(), texture.clone());
        Some(texture)
    }
}

impl OpenCheckApp {
    /// Starts documenting the visual check shown, with a picture of the view.
    pub fn document_visual_check(&mut self, ctx: &egui::Context) {
        // One at a time: an open one is kept.
        if self.check_docs.editing.is_some() {
            return;
        }
        let Some(index) = self.visual.selected else {
            return;
        };
        let (Some(result), Some(rule)) = (
            self.visual.results.get(&index),
            self.checking.visual.get(index),
        ) else {
            return;
        };
        let outcome = &result.outcome;
        let mut legend: Vec<Swatch> = outcome
            .categories
            .iter()
            .map(|c| Swatch {
                label: c.label.clone(),
                detail: c.detail.clone(),
                color: c.color,
                count: c.elements.len(),
            })
            .collect();
        // Problems by colour, as the view shows them.
        let mut problems: Vec<Swatch> = Vec::new();
        for p in &outcome.problems {
            match problems.iter_mut().find(|s| s.color == p.color) {
                Some(s) => s.count += 1,
                None => problems.push(Swatch {
                    label: outcome.problem_title.clone(),
                    detail: String::new(),
                    color: p.color,
                    count: 1,
                }),
            }
        }
        legend.extend(problems);
        let models = self
            .federation
            .iter()
            .map(|(id, model)| {
                let entry = self.session.records.get(&id).map(|r| &r.entry);
                let file = entry
                    .and_then(|e| e.source_path.as_deref())
                    .and_then(|p| std::path::Path::new(p).file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| model.short_name.clone());
                format!("{file} (rev. {})", entry.map_or(1, |e| e.revision))
            })
            .collect();
        let now = project::now_rfc3339();
        let doc = CheckDoc {
            id: crate::issues::new_guid("check"),
            title: rule.name.clone(),
            check: rule.name.clone(),
            kind: rule.kind.check_type().to_owned(),
            about: about_da(rule),
            method: String::new(),
            verdict: if outcome.problems.is_empty() {
                Verdict::Approved
            } else {
                Verdict::Remarks
            },
            by: self.issues.report.author.clone(),
            created: now.clone(),
            modified: now,
            summary: outcome.summary.clone(),
            problems: outcome.problems.len(),
            legend,
            // A table says it better than colours without elements.
            hide_colours: !outcome.table.is_empty(),
            table: outcome.table.clone(),
            models,
            pictures: Vec::new(),
        };
        self.check_docs.editing = Some(Editing {
            doc,
            index: None,
            confirm_delete: false,
        });
        self.take_view_picture(ctx);
    }

    /// Opens a documented check to change it.
    pub fn edit_check_doc(&mut self, index: usize) {
        if let Some(doc) = self.check_docs.list.get(index) {
            let mut doc = doc.clone();
            // Documented before OpenCheck described its checks.
            if doc.about.is_empty()
                && let Some(rule) = self.checking.visual.iter().find(|r| r.name == doc.check)
            {
                doc.about = about_da(rule);
            }
            self.check_docs.editing = Some(Editing {
                doc,
                index: Some(index),
                confirm_delete: false,
            });
        }
    }

    /// Takes a picture of the 3D view as it is now, for the check being
    /// documented.
    pub fn take_view_picture(&mut self, ctx: &egui::Context) {
        if self.capture.is_none() && self.check_docs.shot.is_none() {
            self.check_docs.shot = Some(0);
            ctx.request_repaint();
        }
    }

    /// One step of taking the picture; called every frame.
    pub fn step_view_picture(&mut self, ctx: &egui::Context) {
        let Some(waited) = self.check_docs.shot else {
            return;
        };
        ctx.request_repaint();
        if waited < SETTLE {
            self.check_docs.shot = Some(waited + 1);
            return;
        }
        if waited == SETTLE {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.check_docs.shot = Some(waited + 1);
            return;
        }
        let image: Option<Arc<egui::ColorImage>> = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            self.check_docs.shot = None;
            if let Some(png) = crate::export::crop_png(
                &image,
                self.viewport_rect,
                ctx.pixels_per_point(),
                PICTURE_WIDTH,
            ) {
                let hash = project::sha256_hex(&png);
                self.check_docs.pictures.insert(hash.clone(), png);
                if let Some(editing) = &mut self.check_docs.editing {
                    editing.doc.pictures.push(hash);
                }
            }
        } else if waited >= TIMEOUT {
            self.check_docs.shot = None;
            self.errors
                .push("The picture of the view could not be taken.".into());
        } else {
            self.check_docs.shot = Some(waited + 1);
        }
    }
}

/// The colours a report shows of a check: none when left out, at most
/// [`LEGEND_SHOWN`], and how many more there are.
pub fn legend_shown(doc: &CheckDoc) -> (Vec<&Swatch>, Option<String>) {
    if doc.hide_colours {
        return (Vec::new(), None);
    }
    let shown: Vec<&Swatch> = doc.legend.iter().take(LEGEND_SHOWN).collect();
    let rest = doc.legend.len().saturating_sub(LEGEND_SHOWN);
    let more = (rest > 0).then(|| format!("og {rest} farver mere"));
    (shown, more)
}

/// Most colours a report lists of one check.
const LEGEND_SHOWN: usize = 16;

/// A colour's name with what it stands for, when that says more than the
/// count, e.g. `> 20 ‰`.
pub fn swatch_text(s: &Swatch) -> String {
    let detail = s.detail.trim();
    if detail.is_empty() || detail.contains("element") {
        s.label.clone()
    } else {
        format!("{} ({detail})", s.label)
    }
}

/// What a visual check does, in the report's words, with its settings.
pub fn about_da(rule: &VisualRule) -> String {
    match &rule.kind {
        VisualKind::Alignment(r) => {
            let what = match (r.walls, r.columns) {
                (true, true) => "vægge og søjler",
                (true, false) => "vægge",
                _ => "søjler",
            };
            let walls = match r.mode {
                AlignMode::Centerline => "Vægge sammenlignes ud fra deres centerlinje",
                AlignMode::Face => {
                    "Vægge sammenlignes ud fra deres sideflader, så også vægge med forskellig \
                     tykkelse kan flugte"
                }
            };
            format!(
                "Kontrollen farver {what}, der står i forlængelse af hinanden fra etage til etage; \
                 elementer med samme farve står over hinanden. {walls}, søjler ud fra deres \
                 centerpunkt. {}",
                if r.tolerance_mm > 0.0 {
                    format!(
                        "En forskydning på op til {:.0} mm regnes som at flugte.",
                        r.tolerance_mm
                    )
                } else {
                    "Kun elementer, der flugter præcist, regnes som at stå over hinanden."
                        .to_owned()
                }
            )
        }
        VisualKind::Dimensions(r) => {
            let what = match (r.pipes, r.ducts) {
                (true, true) => "rør- og kanalsystemerne",
                (true, false) => "rørsystemerne",
                _ => "kanalsystemerne",
            };
            format!(
                "Kontrollen følger {what} fra deres start og farver hvert system for sig. Den \
                 markerer steder, hvor dimensionen bliver mere end {:.0} % større igen væk fra \
                 starten. Elementer uden forbindelser kobles sammen, hvor deres ender ligger \
                 inden for {:.0} mm af hinanden.",
                r.tolerance_percent, r.connect_mm
            )
        }
        VisualKind::DrainSlope(r) => {
            let limits: Vec<String> = r
                .limits
                .iter()
                .map(|l| format!("{:.1} ‰ op til Ø{:.0}", l.min_permille, l.up_to_mm))
                .collect();
            let limits = if limits.is_empty() {
                String::new()
            } else {
                format!(" ({})", limits.join(", "))
            };
            format!(
                "Kontrollen finder afløbsledningerne og farver dem efter deres fald. Den markerer \
                 ledninger, der ligger fladt eller falder den forkerte vej. Mindste fald er \
                 {:.1} ‰{limits}, med en tolerance på {:.1} ‰.",
                r.min_permille, r.tolerance_permille
            )
        }
        VisualKind::Storeys(r) => {
            let exempt = if r.exempt.is_empty() {
                String::new()
            } else {
                format!(" {} kan stå på alle etager.", r.exempt.join(", "))
            };
            format!(
                "Kontrollen farver elementerne efter den etage, de er tilknyttet i modellen, og \
                 markerer elementer, der står på en anden etage end den, de tilhører.{}{exempt}",
                if r.tolerance_mm > 0.0 {
                    format!(
                        " Et element må række op til {:.0} mm ned under sin etage.",
                        r.tolerance_mm
                    )
                } else {
                    String::new()
                }
            )
        }
        VisualKind::Rooms(r) => format!(
            "Kontrollen viser rummene grønne og kontrollerer, at der er et rum alle steder, hvor \
             væggene lukker et område inde. Væggene, dørene, vinduerne og søjlerne skæres {:.0} mm \
             over hver etages gulv; et indelukket område på mindst {:.1} m² uden rum markeres \
             med røde vægge omkring. Åbninger smallere end {:.0} mm regnes som lukkede.",
            r.cut_height_mm, r.min_area_m2, r.gap_mm
        ),
        VisualKind::Levels(r) if !r.master.is_empty() => format!(
            "Kontrollen stiller alle modellers etager op efter kote og holder dem op mod \
             mastermodellen {}: en etage afviger, når masteren har dens navn i en anden kote, \
             et andet navn i dens kote, eller ingen af delene. Etager inden for {:.0} mm af \
             hinanden regnes som samme niveau.",
            r.master, r.tolerance_mm
        ),
        VisualKind::Levels(r) => format!(
            "Kontrollen stiller alle modellers etager op efter kote og kontrollerer, at hvert \
             niveau har samme navn og samme kote i alle modeller. Etager inden for {:.0} mm af \
             hinanden regnes som samme niveau.",
            r.tolerance_mm
        ),
    }
}

fn dot(ui: &mut egui::Ui, color: Color32, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
}

fn date(text: &str) -> &str {
    text.get(..10).unwrap_or(text)
}

/// The documented checks, in the Visual Check tab: a click opens one.
pub fn list(app: &mut OpenCheckApp, ui: &mut egui::Ui) {
    crate::checking_ui::section_title(
        ui,
        &format!("Documented checks  ·  {}", app.check_docs.list.len()),
    );
    if app.check_docs.list.is_empty() {
        ui.label(
            RichText::new(
                "Document a check to show it was done: Analyse it, then Document check… on the right.",
            )
            .small()
            .color(theme::text_muted()),
        );
        return;
    }
    let mut open = None;
    for (i, doc) in app.check_docs.list.iter().enumerate() {
        let response = ui
            .horizontal(|ui| {
                dot(ui, doc.verdict.color(), 10.0);
                ui.add(egui::Label::new(&doc.title).truncate().selectable(false));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{}  {}",
                            date(&doc.modified),
                            if doc.pictures.is_empty() {
                                String::new()
                            } else {
                                format!("{} {}", icon::IMAGE, doc.pictures.len())
                            }
                        ))
                        .small()
                        .color(theme::text_muted()),
                    );
                });
            })
            .response;
        if ui
            .interact(response.rect, response.id.with("doc"), egui::Sense::click())
            .on_hover_text(format!("{} — click to open", doc.verdict.label()))
            .clicked()
        {
            open = Some(i);
        }
    }
    if let Some(i) = open {
        app.edit_check_doc(i);
    }
}

/// The dialog for writing a documented check down.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if app.check_docs.editing.is_none() {
        return;
    }
    let mut open = true;
    let dialog = crate::dialog::Dialog {
        id: "check_doc",
        icon: icon::CLIPBOARD_TEXT,
        title: "Document check",
        size: egui::vec2(560.0, 640.0),
        min: egui::vec2(380.0, 320.0),
        scroll: crate::dialog::Scroll::Both,
    };
    let mut action = None;
    crate::dialog::show(ctx, &dialog, &mut open, |ui| action = contents(app, ui));
    match action {
        Some(Action::Save) => {
            if let Some(editing) = app.check_docs.editing.take() {
                let mut doc = editing.doc;
                doc.modified = project::now_rfc3339();
                match editing.index {
                    Some(i) if i < app.check_docs.list.len() => app.check_docs.list[i] = doc,
                    _ => app.check_docs.list.push(doc),
                }
                app.session.dirty = true;
            }
        }
        Some(Action::Delete) => {
            if let Some(i) = app.check_docs.editing.take().and_then(|e| e.index)
                && i < app.check_docs.list.len()
            {
                app.check_docs.list.remove(i);
                app.session.dirty = true;
            }
        }
        Some(Action::Picture) => app.take_view_picture(ctx),
        None if !open => app.check_docs.editing = None,
        None => {}
    }
}

enum Action {
    Save,
    Delete,
    Picture,
}

fn contents(app: &mut OpenCheckApp, ui: &mut egui::Ui) -> Option<Action> {
    let mut action = None;
    let ctx = ui.ctx().clone();
    let docs = &mut app.check_docs;
    let editing = docs.editing.as_mut()?;
    let doc = &mut editing.doc;
    egui::Grid::new("check_doc_fields")
        .num_columns(2)
        .spacing([10.0, 8.0])
        .show(ui, |ui| {
            ui.label("Title");
            ui.add(egui::TextEdit::singleline(&mut doc.title).desired_width(f32::INFINITY));
            ui.end_row();
            ui.label("Result");
            ui.horizontal_wrapped(|ui| {
                for v in Verdict::ALL {
                    let on = doc.verdict == v;
                    let text = RichText::new(v.label());
                    let response = ui
                        .horizontal(|ui| {
                            dot(ui, v.color(), 12.0);
                            ui.add(egui::Button::selectable(
                                on,
                                if on { text.strong() } else { text },
                            ))
                        })
                        .inner;
                    if response.clicked() {
                        doc.verdict = v;
                    }
                }
            });
            ui.end_row();
            ui.label("Checked by");
            ui.add(egui::TextEdit::singleline(&mut doc.by).desired_width(200.0));
            ui.end_row();
        });
    ui.add_space(6.0);
    ui.label("What the check does")
        .on_hover_text("Written by OpenCheck from the check's settings; change it if you like");
    ui.add(
        egui::TextEdit::multiline(&mut doc.about)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(4.0);
    ui.label("What was checked, and how");
    ui.add(
        egui::TextEdit::multiline(&mut doc.method)
            .hint_text(
                "E.g. seen from all four sides; the red walls on level 3 are in issue #12. \
                 Blank lines start a new paragraph.",
            )
            .desired_rows(4)
            .desired_width(f32::INFINITY),
    );
    ui.add_space(8.0);

    // What the check showed.
    ui.label(
        RichText::new(format!(
            "{}  ·  {}  ·  {}",
            doc.check,
            doc.summary,
            date(&doc.created)
        ))
        .small()
        .color(theme::text_muted()),
    );
    if !doc.models.is_empty() {
        ui.label(
            RichText::new(doc.models.join(", "))
                .small()
                .color(theme::text_muted()),
        );
    }
    let mut show = !doc.hide_colours;
    ui.checkbox(&mut show, "Show the colours in the report")
        .on_hover_text("Leave them out when the pictures say enough");
    doc.hide_colours = !show;
    egui::CollapsingHeader::new(format!("Colours  ·  {}", doc.legend.len()))
        .id_salt("check_doc_legend")
        .show(ui, |ui| {
            for s in &doc.legend {
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                    let [r, g, b] = s.color;
                    ui.painter()
                        .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                    ui.label(&s.label);
                    ui.label(
                        RichText::new(format!("{}  ·  {}", s.detail, s.count))
                            .small()
                            .color(theme::text_muted()),
                    );
                });
            }
        });
    ui.add_space(8.0);

    // The pictures.
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("Pictures  ·  {}", doc.pictures.len())).strong());
        if ui
            .button(format!("{}  Add picture of the view", icon::CAMERA))
            .on_hover_text(
                "Turn and zoom the 3D view first: the picture is what it shows, with its colours",
            )
            .clicked()
        {
            action = Some(Action::Picture);
        }
    });
    let mut remove = None;
    let hashes = doc.pictures.clone();
    ui.horizontal_wrapped(|ui| {
        for (i, hash) in hashes.iter().enumerate() {
            ui.vertical(|ui| {
                if let Some(texture) = docs.texture(&ctx, hash) {
                    let size = texture.size_vec2();
                    let width = 160.0;
                    ui.add(
                        egui::Image::new(&texture)
                            .fit_to_exact_size(egui::vec2(width, width * size.y / size.x.max(1.0))),
                    );
                }
                if ui
                    .small_button(format!("{}  Remove", icon::TRASH))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
        }
    });
    let Some(editing) = docs.editing.as_mut() else {
        return action;
    };
    if let Some(i) = remove {
        editing.doc.pictures.remove(i);
    }
    ui.add_space(10.0);
    ui.separator();
    ui.horizontal(|ui| {
        if ui
            .button(RichText::new(format!("{}  Save", icon::CHECK)).strong())
            .clicked()
        {
            action = Some(Action::Save);
        }
        if editing.index.is_some() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if editing.confirm_delete {
                    if ui
                        .button(
                            RichText::new(format!("{}  Delete for good", icon::TRASH))
                                .color(theme::danger()),
                        )
                        .clicked()
                    {
                        action = Some(Action::Delete);
                    }
                    if ui.button("Keep it").clicked() {
                        editing.confirm_delete = false;
                    }
                } else if ui.button(format!("{}  Delete…", icon::TRASH)).clicked() {
                    editing.confirm_delete = true;
                }
            });
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_are_described_with_their_settings() {
        let about = about_da(&VisualRule::alignment());
        assert!(about.contains("vægge og søjler"));
        assert!(about.contains("flugter præcist"));
        let about = about_da(&VisualRule::storeys());
        assert!(about.contains("anden etage"), "{about}");
    }

    #[test]
    fn long_legends_are_cut_short_or_left_out() {
        let mut doc = CheckDoc {
            legend: (0..30)
                .map(|i| Swatch {
                    label: format!("G{i}"),
                    detail: "3 elements".into(),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let (shown, more) = legend_shown(&doc);
        assert_eq!(shown.len(), LEGEND_SHOWN);
        assert_eq!(more.as_deref(), Some("og 14 farver mere"));
        assert_eq!(swatch_text(shown[0]), "G0");
        doc.hide_colours = true;
        assert!(legend_shown(&doc).0.is_empty());
    }

    #[test]
    fn documented_checks_round_trip_with_pictures() {
        let mut docs = CheckDocs::default();
        docs.pictures.insert("h1".into(), vec![1, 2, 3]);
        docs.pictures.insert("unused".into(), vec![9]);
        docs.list.push(CheckDoc {
            id: "c1".into(),
            title: "Columns above each other".into(),
            verdict: Verdict::Remarks,
            pictures: vec!["h1".into()],
            ..CheckDoc::default()
        });
        let mut documents = ProjectDocuments::default();
        docs.save_into(&mut documents);
        assert_eq!(
            documents.pictures.len(),
            1,
            "only pictures in use are saved"
        );
        let (loaded, errors) = CheckDocs::load(&documents);
        assert!(errors.is_empty());
        assert_eq!(loaded.list, docs.list);
        assert_eq!(loaded.pictures.get("h1"), Some(&vec![1, 2, 3]));
    }
}
