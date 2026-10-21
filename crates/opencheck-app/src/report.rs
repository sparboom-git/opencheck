//! The issue report: one self-contained HTML file (pictures embedded) that
//! anyone can open and print to PDF. It documents a consistency and clash
//! check the way Danish practice expects: introduction, basis (models and
//! software), then the issues by properties, geometry and clashes, each with
//! a picture, its data and the GlobalIds of its objects.
//!
//! The report is written in Danish, the language of that practice.

use opencheck_core::bcf::base64;
use rustc_hash::FxHashMap;

use crate::app::OpenCheckApp;
use crate::c402;
use crate::issues::{ComponentRole, Issue, IssueStatus};

fn esc(text: &str) -> String {
    opencheck_core::bcf_file::xml_escape(text)
}

fn date(text: &str) -> String {
    text.get(..10).unwrap_or(text).to_owned()
}

fn status_da(status: IssueStatus) -> &'static str {
    match status {
        IssueStatus::Open => "Åben",
        IssueStatus::InProgress => "I gang",
        IssueStatus::Resolved => "Løst",
        IssueStatus::Closed => "Lukket",
    }
}

/// The report section an issue belongs to, from the kind of its findings.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Section {
    Properties,
    Geometry,
    Clashes,
}

impl Section {
    const ALL: [Section; 3] = [Section::Properties, Section::Geometry, Section::Clashes];

    fn of(issue: &Issue) -> Self {
        let kinds: Vec<&str> = issue.findings.iter().map(|f| f.kind.as_str()).collect();
        if kinds.iter().any(|k| *k == "Clash" || *k == "Inside") {
            Section::Clashes
        } else if kinds.iter().any(|k| *k == "Duplicate" || *k == "Clearance") {
            Section::Geometry
        } else if kinds.contains(&"Missing information") {
            Section::Properties
        } else {
            Section::Clashes
        }
    }

    fn title(self) -> &'static str {
        match self {
            Section::Properties => "Egenskaber",
            Section::Geometry => "Geometri",
            Section::Clashes => "Kollisioner",
        }
    }

    fn intro(self) -> &'static str {
        match self {
            Section::Properties => {
                "Objekter, der mangler de egenskaber, som er aftalt for projektets informationsniveau."
            }
            Section::Geometry => {
                "Geometriske forhold som dubletter af objekter og manglende respektafstande."
            }
            Section::Clashes => {
                "Utilsigtede kollisioner mellem objekter i og på tværs af fagmodellerne."
            }
        }
    }
}

const STYLE: &str = r#"
:root { --ink:#20252e; --muted:#6b7380; --line:#dfe3ea; --accent:#2b7ff6;
        --c3:#d64541; --c2:#e08a1e; --c1:#7a8594; }
* { box-sizing: border-box; }
body { font-family: "Segoe UI", Arial, sans-serif; color: var(--ink); margin: 0;
       background: #f4f6f9; line-height: 1.45; font-size: 14px; }
main { max-width: 980px; margin: 0 auto; background: #fff; padding: 40px 48px; }
h1 { font-size: 26px; margin: 0 0 4px; }
h2 { font-size: 19px; margin: 32px 0 10px; padding-bottom: 6px; border-bottom: 2px solid var(--ink); }
h3 { font-size: 16px; margin: 22px 0 8px; }
.sub { color: var(--muted); margin: 0 0 24px; font-size: 16px; }
table { border-collapse: collapse; width: 100%; margin: 6px 0 12px; }
th, td { text-align: left; vertical-align: top; padding: 5px 8px; border-bottom: 1px solid var(--line); }
th { color: var(--muted); font-weight: 600; white-space: nowrap; }
table.facts th { width: 160px; }
.mono { font-family: Consolas, "Courier New", monospace; font-size: 12.5px; }
.badge { display: inline-block; padding: 1px 8px; border-radius: 10px; font-weight: 700;
         font-size: 12px; color: #fff; }
.C3 { background: var(--c3); } .C2 { background: var(--c2); } .C1 { background: var(--c1); }
.issue { border: 1px solid var(--line); border-radius: 8px; padding: 16px 18px; margin: 14px 0;
         page-break-inside: avoid; break-inside: avoid; }
.issue h4 { margin: 0 0 10px; font-size: 15px; }
.issue .top { display: flex; gap: 16px; align-items: flex-start; }
.issue .top .facts { flex: 1; margin-top: 0; }
.issue .shot { flex: none; width: 300px; }
.issue .shot img { width: 300px; border-radius: 6px; border: 1px solid var(--line); display: block; }
.noimg { flex: none; width: 300px; height: 170px; border: 1px dashed var(--line); border-radius: 6px;
         color: var(--muted); font-style: italic; display: flex; align-items: center; justify-content: center; }
.revit { font-weight: 700; }
@media (max-width: 720px) { .issue .top { flex-direction: column; } }
.desc { white-space: pre-wrap; margin: 6px 0 10px; }
.counts td, .counts th { text-align: center; } .counts th:first-child, .counts td:first-child { text-align: left; }
.muted { color: var(--muted); }
footer { margin-top: 36px; color: var(--muted); font-size: 12px; }
@media print { body { background: #fff; } main { padding: 0; max-width: none; }
               h2 { page-break-after: avoid; } }
"#;

/// Builds the report for the given issues (in list order) with their pictures.
pub fn build(app: &OpenCheckApp, guids: &[String], images: &FxHashMap<String, Vec<u8>>) -> String {
    let settings = &app.issues.report;
    let link = app.issues.bcf.as_ref();
    let today = opencheck_core::project::now_rfc3339();
    let issues: Vec<(usize, &Issue)> = app
        .issues
        .list
        .iter()
        .enumerate()
        .filter(|(_, i)| guids.contains(&i.guid))
        .map(|(n, i)| (n + 1, i))
        .collect();
    let or_dash = |s: &str| {
        if s.trim().is_empty() {
            "–".to_owned()
        } else {
            esc(s.trim())
        }
    };
    let project_name = if settings.project.trim().is_empty() {
        app.session
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Projekt".to_owned())
    } else {
        settings.project.trim().to_owned()
    };

    let mut h = String::new();
    h.push_str(&format!(
        "<!doctype html><html lang=\"da\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <title>{} – konsistens- og kollisionskontrol</title><style>{STYLE}</style></head><body><main>",
        esc(&project_name)
    ));
    h.push_str(&format!(
        "<h1>{}</h1><p class=\"sub\">Dokumentation for konsistens- og kollisionskontrol</p>",
        esc(&project_name)
    ));

    // 1 Indledning
    h.push_str("<h2>1 Indledning</h2><table class=\"facts\">");
    for (label, value) in [
        ("Udarbejdet af", or_dash(&settings.author)),
        ("Firma", or_dash(&settings.company)),
        ("Dato", date(&today)),
        ("Revision", or_dash(&settings.revision)),
        ("Fag", or_dash(&settings.disciplines)),
        ("Fase", or_dash(&settings.phase)),
    ] {
        h.push_str(&format!("<tr><th>{label}</th><td>{value}</td></tr>"));
    }
    h.push_str("</table>");
    h.push_str(
        "<p>Rapporten dokumenterer en konsistens- og kollisionskontrol af projektets digitale \
         bygningsmodeller og de emner, der er udvalgt til opfølgning. Hvert emne er vist med \
         billede, kritikalitet (C1–C3), ansvarlige fag og de berørte objekter med deres IFC \
         GlobalId, så objekterne kan genfindes i modellerne. Emnerne udveksles desuden \
         digitalt som BCF.</p>",
    );

    // 2 Grundlag
    h.push_str("<h2>2 Grundlag</h2><h3>2.1 Bygningsmodeller</h3>");
    h.push_str(
        "<table><tr><th>Bygningsmodel (filnavn)</th><th>Fag</th><th>Revision</th>\
         <th>Opdateret</th><th>Kontrolsum (SHA-256)</th></tr>",
    );
    for (id, model) in app.federation.iter() {
        let record = app.session.records.get(&id);
        let file = model
            .source_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .or_else(|| record.and_then(|r| r.entry.source_path.clone()))
            .unwrap_or_else(|| model.short_name.clone());
        let (revision, updated, hash) = record
            .map(|r| {
                (
                    r.entry.revision.to_string(),
                    date(&r.entry.updated),
                    r.entry.sha256.get(..12).unwrap_or("").to_owned(),
                )
            })
            .unwrap_or_default();
        h.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{revision}</td><td>{updated}</td><td class=\"mono\">{hash}…</td></tr>",
            esc(&file),
            esc(model.discipline.label())
        ));
    }
    h.push_str("</table><h3>2.2 Software</h3>");
    let rulesets: Vec<String> = app
        .checking
        .rulesets
        .iter()
        .filter(|r| r.enabled)
        .map(|r| esc(&r.ruleset.ruleset.name))
        .collect();
    h.push_str(&format!(
        "<p>Kontrollen er foretaget i OpenCheck {} med følgende regelsæt: {}.</p>",
        env!("CARGO_PKG_VERSION"),
        if rulesets.is_empty() {
            "–".to_owned()
        } else {
            rulesets.join(", ")
        }
    ));
    if !settings.method.trim().is_empty() {
        h.push_str(&format!(
            "<p class=\"desc\">{}</p>",
            esc(settings.method.trim())
        ));
    }

    // 3 Kontrollen
    h.push_str("<h2>3 Konsistens- og kollisionskontrollen</h2><h3>3.1 Generelle bemærkninger</h3>");
    if settings.remarks.trim().is_empty() {
        h.push_str("<p class=\"muted\">Ingen generelle bemærkninger.</p>");
    } else {
        h.push_str(&format!(
            "<p class=\"desc\">{}</p>",
            esc(settings.remarks.trim())
        ));
    }

    // Overview: criticality against status.
    h.push_str("<h3>Overblik</h3><table class=\"counts\"><tr><th>Kritikalitet</th>");
    for s in IssueStatus::ALL {
        h.push_str(&format!("<th>{}</th>", status_da(s)));
    }
    h.push_str("<th>I alt</th></tr>");
    for p in c402::LEVELS {
        let row: Vec<usize> = IssueStatus::ALL
            .iter()
            .map(|s| {
                issues
                    .iter()
                    .filter(|(_, i)| i.priority == p && i.status == *s)
                    .count()
            })
            .collect();
        h.push_str(&format!(
            "<tr><td><span class=\"badge {}\">{}</span> {}</td>",
            c402::code(p),
            c402::code(p),
            esc(c402::label(p).split('–').nth(1).unwrap_or("").trim())
        ));
        for n in &row {
            h.push_str(&format!("<td>{n}</td>"));
        }
        h.push_str(&format!(
            "<td><b>{}</b></td></tr>",
            row.iter().sum::<usize>()
        ));
    }
    h.push_str("</table>");
    // Per responsible discipline.
    let mut per_discipline: Vec<(String, usize)> = Vec::new();
    for (_, issue) in &issues {
        for d in &issue.responsible {
            let name = c402::discipline_label(*d).unwrap_or(d.label()).to_owned();
            match per_discipline.iter_mut().find(|(n, _)| *n == name) {
                Some((_, count)) => *count += 1,
                None => per_discipline.push((name, 1)),
            }
        }
    }
    if !per_discipline.is_empty() {
        h.push_str("<table class=\"counts\"><tr><th>Ansvarligt fag</th><th>Emner</th></tr>");
        for (name, n) in &per_discipline {
            h.push_str(&format!("<tr><td>{}</td><td>{n}</td></tr>", esc(name)));
        }
        h.push_str("</table>");
    }

    // Revit's element id: stored on the component, or read from the model.
    let revit_id = |c: &crate::issues::IssueComponent| -> String {
        if !c.tag.is_empty() {
            return c.tag.clone();
        }
        app.federation
            .iter()
            .filter(|(_, m)| m.short_name == c.model)
            .find_map(|(_, m)| m.element_by_global_id(&c.global_id))
            .map(crate::issues::element_id)
            .unwrap_or_default()
    };

    // The issues per section.
    for (number, section) in Section::ALL.into_iter().enumerate() {
        let mut in_section: Vec<&(usize, &Issue)> = issues
            .iter()
            .filter(|(_, i)| Section::of(i) == section)
            .collect();
        h.push_str(&format!("<h3>3.{} {}</h3>", number + 2, section.title()));
        if in_section.is_empty() {
            h.push_str("<p class=\"muted\">Ingen udvalgte emner.</p>");
            continue;
        }
        h.push_str(&format!("<p>{}</p>", section.intro()));
        // Most critical first, then in list order.
        in_section.sort_by_key(|(n, i)| (i.priority, *n));
        for (n, issue) in in_section {
            issue_card(&mut h, *n, issue, images.get(&issue.guid), link, &revit_id);
        }
    }

    h.push_str(&format!(
        "<footer>Genereret af OpenCheck {} den {}. Objekter kan genfindes i Revit via deres Revit ID \
         (Element ID, fx med Select by ID) og i andre programmer via deres IFC GlobalId; \
         emnernes ID svarer til BCF-emnernes GUID.</footer></main></body></html>",
        env!("CARGO_PKG_VERSION"),
        date(&today)
    ));
    h
}

fn issue_card(
    h: &mut String,
    number: usize,
    issue: &Issue,
    image: Option<&Vec<u8>>,
    link: Option<&crate::issues::BcfProjectLink>,
    revit_id: &dyn Fn(&crate::issues::IssueComponent) -> String,
) {
    let code = c402::code(issue.priority);
    h.push_str(&format!(
        "<div class=\"issue\"><h4>#{number} {} <span class=\"badge {code}\">{code}</span></h4>",
        esc(&issue.title)
    ));
    // A small orientation picture next to the facts; a click shows it full size.
    h.push_str("<div class=\"top\">");
    match image {
        Some(png) => {
            let data = format!("data:image/png;base64,{}", base64(png));
            h.push_str(&format!(
                "<div class=\"shot\"><a href=\"{data}\" target=\"_blank\" title=\"Vis i fuld størrelse\"><img alt=\"Emne {number}\" src=\"{data}\"></a></div>"
            ));
        }
        None => h.push_str("<div class=\"noimg\">Intet billede</div>"),
    }
    let responsible: Vec<String> = issue
        .responsible
        .iter()
        .map(|d| c402::discipline_label(*d).unwrap_or(d.label()).to_owned())
        .collect();
    let assigned = issue.effective_assignee(link).unwrap_or("");
    let rules: Vec<String> = {
        let mut list: Vec<String> = issue.findings.iter().map(|f| f.rule.clone()).collect();
        list.sort();
        list.dedup();
        list
    };
    h.push_str("<table class=\"facts\">");
    let mut row = |label: &str, value: String| {
        h.push_str(&format!("<tr><th>{label}</th><td>{value}</td></tr>"));
    };
    row(
        "ID",
        format!("<span class=\"mono\">{}</span>", esc(&issue.guid)),
    );
    row("Kritikalitet", esc(c402::label(issue.priority)));
    row("Status", status_da(issue.status).to_owned());
    row(
        "Ansvarlige fag",
        if responsible.is_empty() {
            "–".to_owned()
        } else {
            esc(&responsible.join(", "))
        },
    );
    row(
        "Tildelt",
        if assigned.is_empty() {
            "–".to_owned()
        } else {
            esc(assigned)
        },
    );
    row(
        "Oprettet / ændret",
        format!("{} / {}", date(&issue.created), date(&issue.modified)),
    );
    if !rules.is_empty() {
        row("Regel", esc(&rules.join("; ")));
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
        row("Mål", esc(&measures.join(", ")));
    }
    let missing: Vec<String> = issue
        .findings
        .iter()
        .flat_map(|f| f.details.clone())
        .collect();
    if !missing.is_empty() {
        let mut missing = missing;
        missing.sort();
        missing.dedup();
        row("Manglende egenskaber", esc(&missing.join(", ")));
    }
    if issue.solved_by_update {
        row("Løst", "Løst ved modelopdatering".to_owned());
    }
    if let Some(bcf) = &issue.bcf {
        row(
            "BCF-server",
            format!(
                "Status: {} (synkroniseret {})",
                esc(&bcf.remote_status),
                date(&bcf.synced)
            ),
        );
    }
    h.push_str("</table></div>");
    if !issue.description.trim().is_empty() {
        h.push_str(&format!(
            "<p class=\"desc\">{}</p>",
            esc(issue.description.trim())
        ));
    }
    if !issue.components.is_empty() {
        h.push_str(
            "<table><tr><th></th><th>Revit ID</th><th>Objekt</th><th>IFC-klasse</th><th>Model</th><th>IFC GlobalId</th></tr>",
        );
        for c in &issue.components {
            let role = match c.role {
                ComponentRole::A => "A",
                ComponentRole::B => "B",
            };
            let element_id = revit_id(c);
            h.push_str(&format!(
                "<tr><td>{role}</td><td class=\"mono revit\">{}</td><td>{}</td><td>{}</td><td>{}</td><td class=\"mono\">{}</td></tr>",
                if element_id.is_empty() { "–".to_owned() } else { esc(&element_id) },
                esc(if c.name.is_empty() { &c.ifc_class } else { &c.name }),
                esc(&c.ifc_class),
                esc(&c.model),
                esc(&c.global_id)
            ));
        }
        h.push_str("</table>");
    }
    h.push_str("</div>");
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn escapes_markup_in_issue_text() {
        assert_eq!(esc("<b>&\"x\""), "&lt;b&gt;&amp;&quot;x&quot;");
        assert_eq!(date("2026-10-20T10:00:00Z"), "2026-10-20");
    }
}
