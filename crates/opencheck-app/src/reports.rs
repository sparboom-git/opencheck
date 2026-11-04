//! Reports: the descriptive documentation of a consistency and clash check
//! in words and pictures, built up in the Report tab.
//!
//! A report has revisions (A, B, … or the project's own labels). A revision
//! holds a frozen snapshot of the data it shows (issues with their
//! pictures, models, rulesets) and the blocks the report is made of. The
//! newest revision is a draft whose texts and choices can be changed;
//! issuing it locks it. New data from a later check always makes a new
//! revision, so an issued revision reads the same forever.
//!
//! Stored in the project as `reports.json`, with the pictures as PNG files
//! named by their content hash, so a picture shared by revisions is kept
//! once.

use std::collections::BTreeMap;

use opencheck_core::Discipline;
use opencheck_core::project::{self, ProjectDocuments, REPORT_PICTURES};
use opencheck_core::triage::Category;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use crate::app::OpenCheckApp;
use crate::c402;
use crate::checking::category_of;
use crate::issues::{Issue, IssueStatus};

/// The fields of the introduction, kept per report and carried to the next
/// revision.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Intro {
    pub prepared_by: String,
    /// Optional: left out of the report when empty.
    pub checked_by: String,
    pub company: String,
    /// Own discipline(s) the report covers.
    pub disciplines: String,
    pub phase: String,
}

/// A locked revision's issue: who issued it, when, and what changed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Issued {
    pub date: String,
    pub by: String,
    pub note: String,
}

/// A model file as it was when the revision was made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SnapModel {
    pub file: String,
    pub model: String,
    pub discipline: String,
    pub revision: u32,
    /// When the file was written (its IFC header), `YYYY-MM-DD HH:MM`.
    pub date: String,
    /// When this revision of it was added to the project.
    pub imported: String,
    /// The company that made it; typed in the report.
    pub company: String,
}

/// The data a revision shows, frozen when it was made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub taken: String,
    pub project: String,
    pub opencheck: String,
    pub models: Vec<SnapModel>,
    /// The rulesets that ran.
    pub rulesets: Vec<String>,
    /// The project's issues, with Revit ids and assignees filled in.
    pub issues: Vec<Issue>,
    /// Picture per issue GUID, by content hash.
    pub pictures: BTreeMap<String, String>,
    /// The documented visual checks; their pictures are among the report's.
    pub checks: Vec<crate::check_docs::CheckDoc>,
    /// The information checks (LOD) of the last run, with their fill rates.
    pub information: Vec<InfoDoc>,
}

/// An information check as it was when the revision was made: how it was
/// done and the fill rate per model, building part and item.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoDoc {
    /// `<ruleset id>/<rule id>`.
    pub id: String,
    /// The ruleset's name, e.g. `Information: LOD DK`.
    pub title: String,
    pub rule: String,
    /// The standard or IDS file checked against.
    pub standard: String,
    /// How the check was done, written by OpenCheck in Danish.
    pub about: String,
    /// Model name and the level it was checked at, e.g. `LOD 300 DK`.
    pub levels: Vec<(String, String)>,
    /// Problems that concern no single element.
    pub notes: Vec<String>,
    pub models: Vec<InfoModel>,
    /// Elements with missing information.
    pub findings: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoModel {
    pub name: String,
    pub elements: usize,
    pub values: usize,
    pub filled: usize,
    pub parts: Vec<InfoPart>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoPart {
    pub name: String,
    pub elements: usize,
    pub complete: usize,
    pub values: usize,
    pub filled: usize,
    pub items: Vec<InfoItem>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InfoItem {
    pub name: String,
    pub present: usize,
    pub required: usize,
    /// Missing on some elements, but accepted so by the project.
    pub accepted: bool,
    /// Elements where the value follows from the geometry and placement,
    /// not from a property on the object.
    pub geometry: usize,
}

/// The note under an information check whose items include some read from
/// the geometry; the report marks those items with a triangle.
pub const GEOMETRY_NOTE_DA: &str = "Egenskaber markeret med en trekant er aflæst af modellens \
    geometri og placering iht. bygningsdelsspecifikationen og ikke af en parameter på objektet.";

impl InfoDoc {
    /// Some items are read from the geometry.
    pub fn has_geometry(&self) -> bool {
        self.models
            .iter()
            .flat_map(|m| &m.parts)
            .flat_map(|p| &p.items)
            .any(|i| i.geometry > 0)
    }
}

/// The share of values filled in, 0 to 1.
pub fn fill_rate(filled: usize, values: usize) -> f64 {
    filled as f64 / values.max(1) as f64
}

/// The scope of an information check in a report, e.g. `368 elementer i 2
/// modeller, 110 med manglende information`.
pub fn scope_da(elements: usize, models: usize, findings: usize) -> String {
    format!(
        "{elements} elementer i {models} {}, {findings} med manglende information",
        if models == 1 { "model" } else { "modeller" }
    )
}

/// A fill rate as shown in reports, e.g. `80 %`.
pub fn percent(filled: usize, values: usize) -> String {
    format!("{:.0} %", fill_rate(filled, values) * 100.0)
}

impl InfoModel {
    pub fn has_accepted(&self) -> bool {
        self.parts.iter().any(InfoPart::has_accepted)
    }
}

impl InfoPart {
    pub fn has_accepted(&self) -> bool {
        self.items.iter().any(|i| i.accepted)
    }
}

/// How an information check was done, for the report.
pub fn about_information_da(
    adaptable: bool,
    standard: &str,
    levels: &[(String, String)],
    accepted: bool,
) -> String {
    let mut text = if adaptable {
        let mut distinct: Vec<&str> = levels.iter().map(|(_, l)| l.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        let at = match distinct.as_slice() {
            [] => String::new(),
            [one] => format!(" Modellerne er kontrolleret på niveau {one}."),
            _ => format!(
                " Modellerne er kontrolleret på følgende niveauer: {}.",
                levels
                    .iter()
                    .map(|(m, l)| format!("{m} {l}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        format!(
            "Kontrollen er foretaget efter {standard}. Hvert element i modellerne er henført til \
             en bygningsdelstype ud fra sin IFC-klasse og sine egenskaber, og for hvert element \
             er det kontrolleret, om de egenskaber, bygningsdelstypen skal bære på det aftalte \
             informationsniveau, er udfyldt. En egenskab regnes som udfyldt, når den har en \
             værdi i mindst ét af de steder i modellen, standarden læser den fra.{at}"
        )
    } else {
        format!(
            "Kontrollen er foretaget efter IDS-filen {standard}. For hver specifikation i filen \
             er det kontrolleret, at de elementer, den gælder for, opfylder dens krav."
        )
    };
    text.push_str(
        "\n\nUdfyldningsgraden er andelen af de krævede værdier, der er udfyldt, opgjort pr. \
         bygningsmodel og bygningsdelstype. Under hver bygningsdelstype er anført, hvor mange \
         af dens elementer der har hver egenskab.",
    );
    if accepted {
        text.push_str(
            " Egenskaber, som det på projektet er aftalt ikke indgår i modellerne, er markeret \
             som accepteret og regnes som i orden.",
        );
    }
    text
}

/// What a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    /// The introduction's fields and a text.
    Intro,
    Text,
    /// The model files the check is based on.
    Models,
    /// How the check was done: software, rulesets, method.
    Software,
    /// Issues about properties, geometry or clashes, with a text.
    Properties,
    Geometry,
    Clashes,
    /// Issues chosen by filter.
    Issues,
    /// Documented visual checks.
    Visual,
    /// The information checks (LOD) with their fill rates.
    Information,
}

impl BlockKind {
    pub fn label(self) -> &'static str {
        match self {
            BlockKind::Intro => "Indledning",
            BlockKind::Text => "Tekst (frit afsnit)",
            BlockKind::Models => "Bygningsmodeller",
            BlockKind::Software => "Software",
            BlockKind::Properties => "Afsnit: Egenskaber",
            BlockKind::Geometry => "Afsnit: Geometri",
            BlockKind::Clashes => "Afsnit: Kollisioner",
            BlockKind::Issues => "Issues",
            BlockKind::Visual => "Afsnit: Visuelle kontroller",
            BlockKind::Information => "Afsnit: Informationsniveau",
        }
    }

    /// Blocks that list issues.
    pub fn shows_issues(self) -> bool {
        matches!(
            self,
            BlockKind::Properties | BlockKind::Geometry | BlockKind::Clashes | BlockKind::Issues
        )
    }

    /// The blocks a user can add, in the order offered.
    pub const ADDABLE: [BlockKind; 10] = [
        BlockKind::Text,
        BlockKind::Intro,
        BlockKind::Models,
        BlockKind::Software,
        BlockKind::Properties,
        BlockKind::Geometry,
        BlockKind::Clashes,
        BlockKind::Information,
        BlockKind::Visual,
        BlockKind::Issues,
    ];
}

/// Which issues a block shows; an empty list lets everything through.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IssueFilter {
    /// Ruleset names.
    pub rulesets: Vec<String>,
    pub categories: Vec<Category>,
    pub statuses: Vec<IssueStatus>,
    /// Responsible disciplines.
    pub disciplines: Vec<Discipline>,
    /// Issues left out by hand.
    pub excluded: Vec<String>,
}

impl IssueFilter {
    pub fn matches(&self, issue: &Issue) -> bool {
        (self.rulesets.is_empty()
            || issue
                .findings
                .iter()
                .any(|f| self.rulesets.contains(&f.ruleset)))
            && (self.categories.is_empty()
                || self.categories.contains(&category_of(issue.priority)))
            && (self.statuses.is_empty() || self.statuses.contains(&issue.status))
            && (self.disciplines.is_empty()
                || issue
                    .responsible
                    .iter()
                    .any(|d| self.disciplines.contains(d)))
            && !self.excluded.contains(&issue.guid)
    }
}

/// A part of a report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Block {
    pub kind: BlockKind,
    /// 1: a chapter; 2: a section in it.
    pub level: u8,
    pub heading: String,
    /// The block starts on a new page.
    pub new_page: bool,
    /// The block's text: the whole of a text block, the comment above the
    /// data of the others.
    pub body: String,
    pub filter: IssueFilter,
    /// A comment per issue (GUID), in this report only.
    pub notes: BTreeMap<String, String>,
    /// Pictures added by hand, shown after the text.
    pub pictures: Vec<BlockPicture>,
}

/// A picture pasted into a block: among the report's pictures by its hash,
/// with its size in pixels so it can be laid out without decoding it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlockPicture {
    pub hash: String,
    pub width: u32,
    pub height: u32,
}

/// The size a block picture is shown at, in the units of `text_width` (pt
/// on paper, points on screen): as wide as the text column, a small one at
/// its own size but at least half the column, and a tall one no higher
/// than `max_height`, so a screenshot is neither huge nor tiny.
pub fn picture_box(pixels: (u32, u32), text_width: f32, max_height: f32) -> (f32, f32) {
    let (pw, ph) = (pixels.0.max(1) as f32, pixels.1.max(1) as f32);
    // A screen pixel is 0.75 pt at 96 dpi.
    let mut w = (pw * 0.75).clamp(text_width * 0.5, text_width);
    let mut h = w * ph / pw;
    if h > max_height {
        h = max_height;
        w = h * pw / ph;
    }
    (w, h)
}

/// Keeps a PNG among the report pictures and describes it for a block.
/// None when the bytes are not a PNG.
pub fn store_picture(
    pictures: &mut FxHashMap<String, Vec<u8>>,
    png: Vec<u8>,
) -> Option<BlockPicture> {
    let (width, height) = opencheck_core::docx::png_size(&png)?;
    let hash = project::sha256_hex(&png);
    pictures.entry(hash.clone()).or_insert(png);
    Some(BlockPicture {
        hash,
        width,
        height,
    })
}

impl Default for Block {
    fn default() -> Self {
        Self {
            kind: BlockKind::Text,
            level: 1,
            heading: String::new(),
            new_page: false,
            body: String::new(),
            filter: IssueFilter::default(),
            notes: BTreeMap::new(),
            pictures: Vec::new(),
        }
    }
}

impl Block {
    pub fn new(kind: BlockKind, level: u8, heading: &str, body: &str) -> Self {
        Self {
            kind,
            level,
            heading: heading.to_owned(),
            body: body.to_owned(),
            ..Self::default()
        }
    }

    /// The issues the block shows, most critical first, then in list order.
    pub fn issues<'a>(&self, snapshot: &'a Snapshot) -> Vec<&'a Issue> {
        let mut list: Vec<(usize, &Issue)> = snapshot
            .issues
            .iter()
            .enumerate()
            .filter(|(_, i)| match self.kind {
                BlockKind::Properties | BlockKind::Geometry | BlockKind::Clashes => {
                    section_of(i) == self.kind && self.filter.matches(i)
                }
                BlockKind::Issues => self.filter.matches(i),
                _ => false,
            })
            .collect();
        list.sort_by_key(|(n, i)| (i.priority, *n));
        list.into_iter().map(|(_, i)| i).collect()
    }

    /// The documented checks the block shows: all but those left out.
    pub fn checks<'a>(&self, snapshot: &'a Snapshot) -> Vec<&'a crate::check_docs::CheckDoc> {
        if self.kind != BlockKind::Visual {
            return Vec::new();
        }
        snapshot
            .checks
            .iter()
            .filter(|c| !self.filter.excluded.contains(&c.id))
            .collect()
    }

    /// The information checks the block shows: all but those left out.
    pub fn information<'a>(&self, snapshot: &'a Snapshot) -> Vec<&'a InfoDoc> {
        if self.kind != BlockKind::Information {
            return Vec::new();
        }
        snapshot
            .information
            .iter()
            .filter(|d| !self.filter.excluded.contains(&d.id))
            .collect()
    }
}

/// The section an issue belongs to, from the kind of its findings.
pub fn section_of(issue: &Issue) -> BlockKind {
    let kinds: Vec<&str> = issue.findings.iter().map(|f| f.kind.as_str()).collect();
    if kinds.iter().any(|k| *k == "Clash" || *k == "Inside") {
        BlockKind::Clashes
    } else if kinds.iter().any(|k| *k == "Duplicate" || *k == "Clearance") {
        BlockKind::Geometry
    } else if kinds.contains(&"Missing information") {
        BlockKind::Properties
    } else {
        BlockKind::Clashes
    }
}

/// The blocks a new report starts with, after C402's outline: introduction,
/// basis (models, software), the check (general remarks, properties,
/// geometry, clashes) and a conclusion.
pub fn default_blocks() -> Vec<Block> {
    vec![
        Block::new(
            BlockKind::Intro,
            1,
            "Indledning",
            "Rapporten dokumenterer kontrollen af bygningsmodellerne for de nævnte fag, både \
             inden for hvert fag og mellem fagene. Den gennemgår de vigtigste fund. Alle emner \
             er desuden udvekslet som BCF.",
        ),
        Block::new(BlockKind::Text, 1, "Grundlag", ""),
        Block::new(
            BlockKind::Models,
            2,
            "Bygningsmodeller",
            "Kontrollen er udført på bygningsmodellerne i de versioner, der er vist herunder.",
        ),
        Block::new(BlockKind::Software, 2, "Software", ""),
        Block::new(
            BlockKind::Text,
            1,
            "Konsistens- og kollisionskontrollen",
            "",
        ),
        Block::new(BlockKind::Text, 2, "Generelle bemærkninger", ""),
        Block::new(BlockKind::Properties, 2, "Egenskaber", ""),
        Block::new(
            BlockKind::Information,
            2,
            "Informationsniveau",
            "Modellernes informationsniveau er kontrolleret som beskrevet herunder.",
        ),
        Block::new(BlockKind::Geometry, 2, "Geometri", ""),
        Block::new(BlockKind::Clashes, 2, "Kollisioner", ""),
        Block::new(
            BlockKind::Visual,
            2,
            "Visuelle kontroller",
            "Følgende kontroller er foretaget visuelt i bygningsmodellerne.",
        ),
        Block::new(BlockKind::Text, 1, "Konklusion", ""),
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Revision {
    /// "A", "B", … or the project's own label; empty until the first issue
    /// makes it "A".
    pub label: String,
    /// The phase the revision belongs to, by its code (see [`PHASES`]).
    pub phase: String,
    /// The date printed in the report, as typed; empty: the date of issue,
    /// or of creation while a draft.
    pub date: String,
    pub created: String,
    /// Set when issued: then nothing in it changes any more.
    pub issued: Option<Issued>,
    pub blocks: Vec<Block>,
    pub snapshot: Snapshot,
    /// The introduction and the report's name as they were when the
    /// revision was issued, so it reads the same afterwards; the draft
    /// follows the report's.
    pub intro: Option<Intro>,
    pub name: Option<String>,
}

impl Revision {
    pub fn is_locked(&self) -> bool {
        self.issued.is_some()
    }

    /// The label to show: "–" while there is none.
    pub fn label_text(&self) -> &str {
        let label = self.label.trim();
        if label.is_empty() { "–" } else { label }
    }

    /// The date printed: as typed, else the date of issue, else of creation.
    pub fn date_text(&self) -> String {
        let typed = self.date.trim();
        if !typed.is_empty() {
            return typed.to_owned();
        }
        let when = self.issued.as_ref().map_or(&self.created, |i| &i.date);
        when.get(..10).unwrap_or(when).to_owned()
    }
}

/// The watermark of a trial print.
pub const PRELIMINARY_DA: &str = "Foreløbigt tryk";

/// The phases a report goes through, code and name, in order.
pub const PHASES: &[(&str, &str)] = &[
    ("Disp", "Dispositionsforslag"),
    ("PF", "Projektforslag"),
    ("MP", "Myndighedsprojekt"),
    ("UBP", "Udbudsprojekt"),
    ("UFP", "Udførelsesprojekt"),
    ("SU", "Som udført"),
];

/// The name of a phase code, e.g. "Projektforslag (PF)"; a code not in
/// [`PHASES`] as it is, and "" for none.
pub fn phase_name(code: &str) -> String {
    let code = code.trim();
    match PHASES.iter().find(|(c, _)| c.eq_ignore_ascii_case(code)) {
        Some((c, name)) => format!("{name} ({c})"),
        None => code.to_owned(),
    }
}

impl Report {
    /// The introduction a revision shows: its own, frozen when it was
    /// issued, else the report's.
    pub fn intro_for<'a>(&'a self, rev: &'a Revision) -> &'a Intro {
        rev.intro.as_ref().unwrap_or(&self.intro)
    }

    /// The name a revision shows, frozen when it was issued.
    pub fn name_for<'a>(&'a self, rev: &'a Revision) -> &'a str {
        rev.name.as_deref().unwrap_or(&self.name)
    }

    /// The phase code of a revision: its own, else the introduction's
    /// (reports from before phases were kept per revision).
    pub fn phase_for<'a>(&'a self, rev: &'a Revision) -> &'a str {
        let phase = rev.phase.trim();
        if phase.is_empty() {
            self.intro_for(rev).phase.trim()
        } else {
            phase
        }
    }

    /// Issues the current draft: it is locked, and the introduction and
    /// the name are frozen with it. A draft without a label becomes
    /// revision A. False when there is no draft.
    pub fn issue_draft(&mut self, issued: Issued) -> bool {
        let intro = self.intro.clone();
        let name = self.name.clone();
        match self.revisions.last_mut() {
            Some(rev) if !rev.is_locked() => {
                if rev.label.trim().is_empty() {
                    rev.label = "A".to_owned();
                }
                rev.intro = Some(intro);
                rev.name = Some(name);
                rev.issued = Some(issued);
                true
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Report {
    pub id: String,
    pub name: String,
    pub intro: Intro,
    /// Oldest first; the last is the current one.
    pub revisions: Vec<Revision>,
}

/// The label after `label`: B after A, 2 after 1, P03 after P02; else the
/// same with "+".
pub fn next_label(label: &str) -> String {
    let label = label.trim();
    if label.is_empty() {
        return "A".to_owned();
    }
    let digits = label.len() - label.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        let (head, tail) = label.split_at(label.len() - digits);
        if let Ok(n) = tail.parse::<u64>() {
            return format!("{head}{:0width$}", n + 1, width = digits);
        }
    }
    let mut chars: Vec<char> = label.chars().collect();
    if let Some(last) = chars.last_mut()
        && last.is_ascii_alphabetic()
        && !matches!(last, 'z' | 'Z')
    {
        *last = (*last as u8 + 1) as char;
        return chars.into_iter().collect();
    }
    format!("{label}+")
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct SavedReports {
    reports: Vec<Report>,
}

/// What a revision is made from when its pictures are ready.
#[derive(Debug, Clone, PartialEq)]
pub enum NewRevision {
    /// A new report with this name, phase and first revision label (may be
    /// empty: none until issued).
    Report {
        name: String,
        label: String,
        phase: String,
    },
    /// The next revision of a report; `replace`: the draft's data is
    /// renewed instead; `phase`: a new phase starts, without a label.
    Next {
        report: String,
        replace: bool,
        phase: Option<String>,
    },
}

/// The reports of the project and the Report tab's state.
#[derive(Default)]
pub struct Reports {
    pub list: Vec<Report>,
    /// The report shown.
    pub selected: usize,
    /// The revision shown, by index; `None` the newest.
    pub revision: Option<usize>,
    /// The block being edited.
    pub block: Option<usize>,
    /// A block the preview should scroll to, once.
    pub scroll_to: Option<usize>,
    /// PNG pictures by content hash.
    pub pictures: FxHashMap<String, Vec<u8>>,
    pub dialog: Option<crate::report_ui::Dialog>,
    /// Exports and the preview carry a "Foreløbigt tryk" watermark.
    pub preliminary: bool,
    /// Pictures on the GPU for the preview, by hash.
    pub textures: FxHashMap<String, eframe::egui::TextureHandle>,
    /// The revision shown set in pages.
    pub preview: crate::typeset::Preview,
    leftovers: crate::lenient::Leftovers,
}

impl Reports {
    /// The documents to save: `reports.json` and the pictures in use.
    pub fn save_into(&self, documents: &mut ProjectDocuments) {
        if self.list.is_empty() && self.leftovers.is_empty() {
            return;
        }
        let mut json = serde_json::to_value(SavedReports {
            reports: self.list.clone(),
        })
        .expect("reports always serialise");
        self.leftovers.restore::<SavedReports>(&mut json);
        documents.reports = Some(json);
        let mut used: Vec<&String> = self
            .list
            .iter()
            .flat_map(|r| &r.revisions)
            .flat_map(|r| {
                r.snapshot
                    .pictures
                    .values()
                    .chain(r.snapshot.checks.iter().flat_map(|c| &c.pictures))
                    .chain(
                        r.blocks
                            .iter()
                            .flat_map(|b| b.pictures.iter().map(|p| &p.hash)),
                    )
            })
            .collect();
        used.sort();
        used.dedup();
        documents
            .pictures
            .extend(used.into_iter().filter_map(|hash| {
                let png = self.pictures.get(hash)?;
                Some((format!("{REPORT_PICTURES}{hash}.png"), png.clone()))
            }));
    }

    /// Reads the reports of an opened project; reports that cannot be read
    /// are left out with a message, and kept in the file.
    pub fn load(documents: &ProjectDocuments) -> (Self, Vec<String>) {
        let (saved, leftovers, errors) = match &documents.reports {
            Some(json) => {
                crate::lenient::read::<SavedReports>(json.clone(), &["reports"], "reports.json")
            }
            None => Default::default(),
        };
        let pictures = documents
            .pictures
            .iter()
            .filter_map(|(name, png)| {
                let hash = name.strip_prefix(REPORT_PICTURES)?.strip_suffix(".png")?;
                Some((hash.to_owned(), png.clone()))
            })
            .collect();
        (
            Self {
                list: saved.reports,
                pictures,
                leftovers,
                ..Self::default()
            },
            errors,
        )
    }

    pub fn current(&self) -> Option<&Report> {
        self.list.get(self.selected)
    }

    /// The revision shown of the current report.
    pub fn shown(&self) -> Option<(&Report, usize)> {
        let report = self.current()?;
        let last = report.revisions.len().checked_sub(1)?;
        Some((report, self.revision.unwrap_or(last).min(last)))
    }
}

impl OpenCheckApp {
    /// Takes pictures of every issue and then makes the revision. Without
    /// issues the revision is made at once.
    pub fn request_revision(&mut self, what: NewRevision, ctx: &eframe::egui::Context) {
        let issues: Vec<String> = self.issues.list.iter().map(|i| i.guid.clone()).collect();
        if issues.is_empty() {
            self.make_revision(what, &FxHashMap::default());
            return;
        }
        self.start_capture(crate::export::ExportKind::Revision(what), issues, ctx);
    }

    /// Freezes the data shown now, with these pictures (PNG by issue GUID).
    fn snapshot(
        &mut self,
        images: &FxHashMap<String, Vec<u8>>,
        previous: Option<&Snapshot>,
    ) -> Snapshot {
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
        let models = self
            .federation
            .iter()
            .map(|(id, model)| {
                let entry = self.session.records.get(&id).map(|r| &r.entry);
                let file = entry
                    .and_then(|e| e.source_path.as_deref())
                    .and_then(|p| std::path::Path::new(p).file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .or_else(|| {
                        model
                            .source_path
                            .as_deref()
                            .and_then(|p| p.file_name())
                            .map(|n| n.to_string_lossy().into_owned())
                    })
                    .unwrap_or_else(|| format!("{}.ifc", model.short_name));
                // The company typed for this model before goes with it.
                let company = previous
                    .and_then(|p| p.models.iter().find(|m| m.model == model.short_name))
                    .map(|m| m.company.clone())
                    .unwrap_or_default();
                SnapModel {
                    file,
                    model: model.short_name.clone(),
                    discipline: c402::discipline_label(model.discipline)
                        .unwrap_or(model.discipline.label())
                        .to_owned(),
                    revision: entry.map_or(1, |e| e.revision),
                    date: model
                        .file_timestamp
                        .as_deref()
                        .map(|s| s.get(..16).unwrap_or(s).replace('T', " "))
                        .unwrap_or_default(),
                    imported: entry
                        .map(|e| e.updated.get(..10).unwrap_or(&e.updated).to_owned())
                        .unwrap_or_default(),
                    company,
                }
            })
            .collect();
        let rulesets = self
            .checking
            .rulesets
            .iter()
            .filter(|r| r.enabled)
            .map(|r| r.ruleset.ruleset.name.clone())
            .collect();
        let link = self.issues.bcf.clone();
        let issues: Vec<Issue> = self
            .issues
            .list
            .iter()
            .map(|issue| {
                let mut issue = issue.clone();
                for c in &mut issue.components {
                    if c.tag.is_empty() {
                        c.tag = self.revit_id(c);
                    }
                }
                issue.assigned_to = issue
                    .effective_assignee(link.as_ref())
                    .unwrap_or("")
                    .to_owned();
                issue
            })
            .collect();
        let mut pictures = BTreeMap::new();
        for (guid, png) in images {
            let hash = project::sha256_hex(png);
            self.reports
                .pictures
                .entry(hash.clone())
                .or_insert_with(|| png.clone());
            pictures.insert(guid.clone(), hash);
        }
        let information = self.information_docs();
        let checks = self.check_docs.list.clone();
        for hash in checks.iter().flat_map(|c| &c.pictures) {
            if let Some(png) = self.check_docs.pictures.get(hash) {
                self.reports
                    .pictures
                    .entry(hash.clone())
                    .or_insert_with(|| png.clone());
            }
        }
        Snapshot {
            taken: project::now_rfc3339(),
            project,
            opencheck: env!("CARGO_PKG_VERSION").to_owned(),
            models,
            rulesets,
            issues,
            pictures,
            checks,
            information,
        }
    }

    /// The information checks of the last run, frozen for a report.
    fn information_docs(&self) -> Vec<InfoDoc> {
        let Some(report) = &self.checking.report else {
            return Vec::new();
        };
        let mut docs = Vec::new();
        for rs in &report.rulesets {
            for rule in &rs.rules {
                let Some(summary) = &rule.information else {
                    continue;
                };
                let adaptable = crate::checking::uses_levels(&rule.rule);
                let levels: Vec<(String, String)> = if adaptable {
                    summary
                        .models
                        .iter()
                        .filter(|m| !m.parts.is_empty())
                        .map(|m| {
                            (
                                m.name.clone(),
                                crate::checking::level_label(&self.checking.level_of(m.model)),
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                let models: Vec<InfoModel> = summary
                    .models
                    .iter()
                    .filter(|m| !m.parts.is_empty())
                    .map(|m| InfoModel {
                        name: m.name.clone(),
                        elements: m.elements(),
                        values: m.parts.iter().map(|p| p.values).sum(),
                        filled: m.parts.iter().map(|p| p.filled).sum(),
                        parts: m
                            .parts
                            .iter()
                            .map(|p| InfoPart {
                                name: p.name.clone(),
                                elements: p.elements,
                                complete: p.complete,
                                values: p.values,
                                filled: p.filled,
                                items: p
                                    .items
                                    .iter()
                                    .map(|i| InfoItem {
                                        name: i.name.clone(),
                                        present: i.present,
                                        required: i.required,
                                        accepted: i.accepted && i.present < i.required,
                                        geometry: i
                                            .found_in
                                            .iter()
                                            .filter(|(place, _)| place == "Geometry")
                                            .map(|(_, n)| n)
                                            .sum(),
                                    })
                                    .collect(),
                            })
                            .collect(),
                    })
                    .collect();
                let accepted = models.iter().any(InfoModel::has_accepted);
                docs.push(InfoDoc {
                    id: format!("{}/{}", rs.info.id, rule.rule.id),
                    title: rs.info.name.clone(),
                    rule: rule.rule.name.clone(),
                    standard: summary.standard.clone(),
                    about: about_information_da(adaptable, &summary.standard, &levels, accepted),
                    levels,
                    notes: summary.notes.clone(),
                    models,
                    findings: rule.findings.len(),
                });
            }
        }
        docs
    }

    /// Makes the revision asked for, once its pictures are taken.
    pub fn make_revision(&mut self, what: NewRevision, images: &FxHashMap<String, Vec<u8>>) {
        let now = project::now_rfc3339();
        match what {
            NewRevision::Report { name, label, phase } => {
                let snapshot = self.snapshot(images, None);
                let settings = &self.issues.report;
                let report = Report {
                    id: crate::issues::new_guid("report"),
                    name,
                    intro: Intro {
                        prepared_by: settings.author.clone(),
                        checked_by: String::new(),
                        company: settings.company.clone(),
                        disciplines: settings.disciplines.clone(),
                        phase: String::new(),
                    },
                    revisions: vec![Revision {
                        label: label.trim().to_owned(),
                        phase: phase.trim().to_owned(),
                        date: String::new(),
                        created: now,
                        issued: None,
                        intro: None,
                        name: None,
                        blocks: default_blocks(),
                        snapshot,
                    }],
                };
                self.reports.list.push(report);
                self.reports.selected = self.reports.list.len() - 1;
            }
            NewRevision::Next {
                report,
                replace,
                phase,
            } => {
                let Some(index) = self.reports.list.iter().position(|r| r.id == report) else {
                    return;
                };
                let Some(last) = self.reports.list[index].revisions.last().cloned() else {
                    return;
                };
                let last_phase = self.reports.list[index].phase_for(&last).to_owned();
                let snapshot = self.snapshot(images, Some(&last.snapshot));
                let revisions = &mut self.reports.list[index].revisions;
                if replace && !last.is_locked() {
                    let draft = revisions.last_mut().expect("checked above");
                    draft.snapshot = snapshot;
                    draft.created = now;
                } else {
                    // A new phase starts without a label; the next revision
                    // of the same phase follows the last label.
                    let (label, phase) = match phase {
                        Some(phase) => (String::new(), phase.trim().to_owned()),
                        None => (next_label(&last.label), last_phase),
                    };
                    revisions.push(Revision {
                        label,
                        phase,
                        date: String::new(),
                        created: now,
                        issued: None,
                        intro: None,
                        name: None,
                        blocks: last.blocks.clone(),
                        snapshot,
                    });
                }
                self.reports.selected = index;
            }
        }
        self.reports.revision = None;
        self.reports.block = None;
        self.session.dirty = true;
        self.notify("Report revision made with the latest data.".to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_labels() {
        assert_eq!(next_label("A"), "B");
        assert_eq!(next_label("b"), "c");
        assert_eq!(next_label("P02"), "P03");
        assert_eq!(next_label("1.9"), "1.10");
        assert_eq!(next_label("Z"), "Z+");
        assert_eq!(next_label(""), "A");
    }

    #[test]
    fn block_pictures_fit_the_text_column() {
        // A wide screenshot: the width of the column.
        assert_eq!(picture_box((1600, 800), 450.0, 400.0), (450.0, 225.0));
        // A small crop: its own size, at least half the column.
        assert_eq!(picture_box((400, 100), 450.0, 400.0), (300.0, 75.0));
        assert_eq!(picture_box((100, 100), 450.0, 400.0), (225.0, 225.0));
        // A tall one: no higher than allowed.
        let (w, h) = picture_box((600, 2400), 450.0, 400.0);
        assert_eq!(h, 400.0);
        assert_eq!(w, 100.0);
        let mut store = FxHashMap::default();
        assert!(store_picture(&mut store, vec![1, 2, 3]).is_none());
        let png = crate::platform::encode_png(3, 2, &[0; 24]).unwrap();
        let p = store_picture(&mut store, png).unwrap();
        assert_eq!((p.width, p.height), (3, 2));
        assert!(store.contains_key(&p.hash));
    }

    #[test]
    fn issuing_a_draft_without_a_label_makes_it_a() {
        let mut report = Report {
            revisions: vec![Revision {
                phase: "PF".into(),
                ..Revision::default()
            }],
            ..Report::default()
        };
        assert_eq!(report.revisions[0].label_text(), "–");
        assert!(report.issue_draft(Issued {
            date: "2026-11-04T10:00:00Z".into(),
            ..Default::default()
        }));
        assert_eq!(report.revisions[0].label, "A");
        assert_eq!(report.revisions[0].date_text(), "2026-11-04");
        assert_eq!(report.phase_for(&report.revisions[0]), "PF");
        assert!(!report.issue_draft(Issued::default()));
        let mut typed = report.revisions[0].clone();
        typed.date = "04.11.2026".into();
        assert_eq!(typed.date_text(), "04.11.2026");
        assert_eq!(phase_name("pf"), "Projektforslag (PF)");
        assert_eq!(phase_name("Egen"), "Egen");
    }

    #[test]
    fn reports_round_trip_with_their_pictures() {
        let mut reports = Reports::default();
        reports.pictures.insert("abc".into(), vec![1, 2, 3]);
        reports.pictures.insert("unused".into(), vec![9]);
        let mut snapshot = Snapshot::default();
        snapshot.pictures.insert("guid-1".into(), "abc".into());
        reports.list.push(Report {
            id: "r1".into(),
            name: "K&K".into(),
            intro: Intro::default(),
            revisions: vec![Revision {
                label: "A".into(),
                blocks: default_blocks(),
                snapshot,
                ..Revision::default()
            }],
        });
        let mut documents = ProjectDocuments::default();
        reports.save_into(&mut documents);
        assert_eq!(
            documents.pictures.len(),
            1,
            "only pictures in use are saved"
        );
        let (back, errors) = Reports::load(&documents);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(back.list, reports.list);
        assert_eq!(back.pictures.get("abc"), Some(&vec![1, 2, 3]));
    }

    #[test]
    fn default_blocks_follow_c402() {
        let headings: Vec<String> = default_blocks().into_iter().map(|b| b.heading).collect();
        assert_eq!(headings.first().map(String::as_str), Some("Indledning"));
        assert!(headings.contains(&"Bygningsmodeller".to_owned()));
        assert!(headings.contains(&"Kollisioner".to_owned()));
    }

    #[test]
    fn filters_select_issues() {
        let issue: Issue = serde_json::from_value(serde_json::json!({
            "guid": "g1", "title": "Duct vs Beam", "status": "open", "priority": "critical",
            "created": "", "modified": "", "components": [], "point": [0.0, 0.0, 0.0],
            "findings": [{"finding": "f", "ruleset": "Clash", "rule": "r", "kind": "Clash",
                          "magnitude_mm": 10.0, "ruleset_id": "x", "rule_id": "y"}],
            "responsible": ["structural"]
        }))
        .unwrap();
        let mut filter = IssueFilter::default();
        assert!(filter.matches(&issue));
        filter.categories = vec![Category::C2];
        assert!(!filter.matches(&issue));
        filter.categories = vec![Category::C3];
        filter.rulesets = vec!["Clash".into()];
        assert!(filter.matches(&issue));
        filter.excluded = vec!["g1".into()];
        assert!(!filter.matches(&issue));
        assert_eq!(section_of(&issue), BlockKind::Clashes);
    }
}
