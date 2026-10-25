//! `opencheck` command line tool.
//!
//! Useful for inspecting what OpenCheck reads from a file without starting the
//! viewer, and for scripting checks later on.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
mod autorun;

use clap::{Parser, Subcommand};
use opencheck_core::check::{
    BUILTIN_RULESETS, BUILTIN_STANDARDS, CheckJob, CheckModel, FindingKind, IDS_PREFIX, Ids, Rule,
    RuleKind, Ruleset, RulesetInfo, Selector, Severity, builtin_standard, run_check,
    standard_to_ids,
};
use opencheck_core::{Federation, IfcModel, SpatialNode, load_ifc_file};

#[derive(Parser)]
#[command(name = "opencheck", version, about = "Open source IFC model checker")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Summarise one or more IFC files and how they line up as a federation.
    Info {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Number of IFC classes to list per model.
        #[arg(long, default_value_t = 12)]
        classes: usize,
    },
    /// Print the spatial structure of a model.
    Tree {
        file: PathBuf,
        /// Also list the elements under each node.
        #[arg(long)]
        elements: bool,
    },
    /// Print the attributes and property sets of one element.
    Props {
        file: PathBuf,
        /// GlobalId, or an express ID written as #123.
        element: String,
    },
    /// Run rulesets against a federation of IFC files.
    Check {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        /// Ruleset file, or the file name of a built-in ruleset
        /// (e.g. clash-interdisciplinary.toml). Repeat for several.
        #[arg(long = "ruleset", short)]
        rulesets: Vec<String>,
        /// Run all built-in rulesets.
        #[arg(long)]
        all: bool,
        /// Findings to list per rule.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Level (LOD) for information rules, e.g. 300. Default: the standard's.
        #[arg(long)]
        level: Option<String>,
        /// IDS 1.0 file to check the models against. Repeat for several.
        #[arg(long = "ids")]
        ids: Vec<PathBuf>,
    },
    /// List the built-in rulesets.
    Rulesets,
    /// Run the checks of an autorun configuration (autorun.json, made in the
    /// app): history, dashboard and log go to its output folder.
    Autorun {
        config: PathBuf,
        /// Run even when no file and no check changed since the last run.
        #[arg(long)]
        force: bool,
    },
    /// Write a built-in information standard as an IDS 1.0 file.
    ExportIds {
        /// Output file, e.g. lod-300.ids.
        output: PathBuf,
        /// Standard id. Default: the first built-in standard.
        #[arg(long)]
        standard: Option<String>,
        /// Levels to export, e.g. --level 300. Default: all levels.
        #[arg(long = "level")]
        levels: Vec<String>,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Info { files, classes } => info(&files, classes),
        Command::Tree { file, elements } => tree(&file, elements),
        Command::Props { file, element } => props(&file, &element),
        Command::Check {
            files,
            rulesets,
            all,
            limit,
            level,
            ids,
        } => check(&files, &rulesets, all, limit, level.as_deref(), &ids),
        Command::Rulesets => {
            for builtin in BUILTIN_RULESETS {
                let ruleset = Ruleset::from_toml(builtin.toml)?;
                println!("{:<40} {}", builtin.file_name, ruleset.ruleset.name);
            }
            Ok(())
        }
        Command::ExportIds {
            output,
            standard,
            levels,
        } => export_ids(&output, standard.as_deref(), &levels),
        Command::Autorun { config, force } => autorun::autorun(&config, force),
    }
}

fn export_ids(output: &PathBuf, standard: Option<&str>, levels: &[String]) -> Result<()> {
    let id = standard.unwrap_or(BUILTIN_STANDARDS[0].id);
    let Some(standard) = builtin_standard(id) else {
        let known: Vec<&str> = BUILTIN_STANDARDS.iter().map(|b| b.id).collect();
        bail!("unknown standard {id}; built in: {}", known.join(", "));
    };
    if let Some(unknown) = levels
        .iter()
        .find(|l| !standard.standard.levels.contains(l))
    {
        bail!(
            "unknown level {unknown}; the standard has {}",
            standard.standard.levels.join(", ")
        );
    }
    let export = standard_to_ids(standard, levels);
    std::fs::write(output, export.ids.to_xml())
        .with_context(|| format!("writing {}", output.display()))?;
    println!(
        "{} specifications written to {}",
        export.ids.specifications.len(),
        output.display()
    );
    if !export.warnings.is_empty() {
        println!("\nNot expressible in IDS:");
        for warning in &export.warnings {
            println!("  - {warning}");
        }
    }
    Ok(())
}

/// A ruleset with one information rule for an IDS file, and the document.
fn ids_ruleset(path: &PathBuf) -> Result<(Ruleset, (String, Arc<Ids>))> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let ids = Ids::from_xml(&text).with_context(|| format!("IDS file {}", path.display()))?;
    let key = path.display().to_string();
    let ruleset = Ruleset {
        ruleset: RulesetInfo {
            id: format!("ids.{key}"),
            name: format!("IDS: {}", ids.title()),
            description: ids.info.description.clone(),
            version: "1".to_owned(),
            author: String::new(),
        },
        rules: vec![Rule {
            id: "ids".to_owned(),
            name: "Elements meet the IDS specifications".to_owned(),
            description: String::new(),
            kind: RuleKind::Information,
            severity: Severity::Moderate,
            tolerance_mm: 10.0,
            clearance_mm: 0.0,
            between: Default::default(),
            a: Selector::default(),
            b: None,
            standard: format!("{IDS_PREFIX}{key}"),
            zone: None,
            script: String::new(),
            params: Default::default(),
        }],
    };
    Ok((ruleset, (key, Arc::new(ids))))
}

fn load_ruleset(spec: &str) -> Result<Ruleset> {
    let text = match BUILTIN_RULESETS.iter().find(|b| b.file_name == spec) {
        Some(builtin) => builtin.toml.to_owned(),
        None => std::fs::read_to_string(spec).with_context(|| format!("reading {spec}"))?,
    };
    Ruleset::from_toml(&text).with_context(|| format!("ruleset {spec}"))
}

fn check(
    files: &[PathBuf],
    specs: &[String],
    all: bool,
    limit: usize,
    level: Option<&str>,
    ids_files: &[PathBuf],
) -> Result<()> {
    let mut rulesets = Vec::new();
    let mut documents = Vec::new();
    for path in ids_files {
        let (ruleset, document) = ids_ruleset(path)?;
        rulesets.push(ruleset);
        documents.push(document);
    }
    if all {
        for builtin in BUILTIN_RULESETS {
            rulesets.push(Ruleset::from_toml(builtin.toml)?);
        }
    }
    for spec in specs {
        rulesets.push(load_ruleset(spec)?);
    }
    if rulesets.is_empty() {
        bail!("give at least one --ruleset or --ids, or --all");
    }

    let mut federation = Federation::new();
    for path in files {
        let model = load(path)?;
        println!("{:<28} {}", model.short_name, model.discipline);
        federation.add(model);
    }
    let models: Vec<CheckModel> = federation
        .ids()
        .into_iter()
        .map(|id| CheckModel {
            id,
            model: federation.shared(id).expect("id from the federation"),
            transform: federation
                .model_transform(id)
                .expect("id from the federation"),
            mask: None,
        })
        .collect();
    let jobs: Vec<CheckJob> = rulesets
        .into_iter()
        .map(|ruleset| CheckJob {
            ruleset,
            models: federation.ids(),
            levels: level
                .map(|l| {
                    federation
                        .ids()
                        .into_iter()
                        .map(|id| (id, l.to_owned()))
                        .collect()
                })
                .unwrap_or_default(),
            ids: documents.clone(),
            standards: Vec::new(),
        })
        .collect();

    let report = run_check(&models, &jobs, &|_| {});
    let name_of = |id| {
        federation
            .get(id)
            .map(|m| m.short_name.clone())
            .unwrap_or_default()
    };
    println!();
    for ruleset in &report.rulesets {
        println!("{}", ruleset.info.name);
        for rule in &ruleset.rules {
            println!(
                "  [{}] {}: {} findings ({} pairs, {:.0} ms)",
                rule.rule.severity.label(),
                rule.rule.name,
                rule.findings.len(),
                rule.pairs_tested,
                rule.duration.as_secs_f64() * 1000.0
            );
            if let Some(info) = &rule.information {
                for note in &info.notes {
                    println!("    ! {note}");
                }
                for part in &info.parts {
                    println!(
                        "    {:<36} {:>5} elements, {:>5} complete",
                        part.name, part.elements, part.complete
                    );
                    for item in &part.items {
                        println!(
                            "        {:<32} {:>5} / {:<5} {:>4.0} %",
                            item.name,
                            item.present,
                            item.required,
                            100.0 * item.present as f64 / item.required.max(1) as f64
                        );
                        if !item.found_in.is_empty() {
                            let places: Vec<String> = item
                                .found_in
                                .iter()
                                .take(3)
                                .map(|(place, n)| format!("{place} ({n})"))
                                .collect();
                            println!("            found in {}", places.join(", "));
                        }
                    }
                }
            }
            for note in rule.notes.iter().take(limit.max(5)) {
                println!("    note: {note}");
            }
            for f in rule.findings.iter().take(limit) {
                if f.kind != FindingKind::MissingInformation && !f.details.is_empty() {
                    let other = if f.is_single() {
                        String::new()
                    } else {
                        format!(
                            "  <->  {} [{}] {}",
                            f.b.name,
                            f.b.ifc_class,
                            name_of(f.b.model)
                        )
                    };
                    println!(
                        "    {:<9} {} [{}] {}{other}
              {}",
                        f.kind.label(),
                        f.a.name,
                        f.a.ifc_class,
                        name_of(f.a.model),
                        f.details.join(" · ")
                    );
                    continue;
                }
                if f.is_single() {
                    println!(
                        "    {:<9} {} [{}] {} ({}): missing {}",
                        f.kind.label(),
                        f.a.name,
                        f.a.ifc_class,
                        name_of(f.a.model),
                        f.category,
                        f.details.join(", ")
                    );
                    continue;
                }
                println!(
                    "    {:<9} {:>7.0} mm  {} [{}] {}  <->  {} [{}] {}",
                    f.kind.label(),
                    f.magnitude * 1000.0,
                    f.a.name,
                    f.a.ifc_class,
                    name_of(f.a.model),
                    f.b.name,
                    f.b.ifc_class,
                    name_of(f.b.model),
                );
            }
            if rule.findings.len() > limit {
                println!("    … {} more", rule.findings.len() - limit);
            }
        }
    }
    println!(
        "\n{} findings in {:.0} ms",
        report.finding_count(),
        report.duration.as_secs_f64() * 1000.0
    );
    Ok(())
}

fn load(path: &PathBuf) -> Result<IfcModel> {
    load_ifc_file(path).with_context(|| format!("loading {}", path.display()))
}

fn info(files: &[PathBuf], classes: usize) -> Result<()> {
    let mut federation = Federation::new();
    for path in files {
        let model = load(path)?;
        print_model_summary(&model, classes);
        federation.add(model);
    }
    if federation.len() > 1 {
        let b = federation.bounds();
        let o = federation.origin();
        println!("Federation of {} models", federation.len());
        println!("  origin        {:.3}, {:.3}, {:.3}", o.x, o.y, o.z);
        println!(
            "  extent        {:.2} x {:.2} x {:.2} m",
            b.size().x,
            b.size().y,
            b.size().z
        );
        for (id, model) in federation.iter() {
            let t = federation.model_transform(id).expect("id from iter").w_axis;
            println!(
                "  {:<24} offset {:>9.3}, {:>9.3}, {:>9.3}",
                model.short_name, t.x, t.y, t.z
            );
        }
    }
    Ok(())
}

fn print_model_summary(model: &IfcModel, classes: usize) {
    let with_geometry = model.elements.iter().filter(|e| e.has_geometry()).count();
    println!("{}", model.short_name);
    println!("  schema        {}", model.schema);
    println!("  discipline    {}", model.discipline);
    if let Some(project) = &model.project_name {
        println!("  project       {project}");
    }
    if let Some(app) = &model.authoring_application {
        println!("  application   {app}");
    }
    println!("  unit scale    {} m per unit", model.length_unit_scale);
    println!(
        "  entities      {} ({} elements, {} with geometry)",
        model.entity_count,
        model.elements.len(),
        with_geometry
    );
    println!(
        "  geometry      {} meshes, {} triangles",
        model.meshes.len(),
        model.triangle_count()
    );
    if !model.bounds.is_empty() {
        let s = model.bounds.size();
        println!("  size          {:.2} x {:.2} x {:.2} m", s.x, s.y, s.z);
    }
    println!(
        "  load time     {:.0} ms geometry, {:.0} ms semantics ({:.1} MB)",
        model.stats.geometry_time.as_secs_f64() * 1000.0,
        model.stats.semantics_time.as_secs_f64() * 1000.0,
        model.stats.file_size as f64 / 1_048_576.0
    );
    let storeys = model.storeys();
    if !storeys.is_empty() {
        println!("  storeys");
        for s in storeys {
            let elevation = s
                .elevation
                .map(|e| format!("{:>8.2}", e * model.length_unit_scale))
                .unwrap_or_else(|| "       ?".to_owned());
            println!(
                "    {elevation} m  {} ({} elements)",
                s.label(),
                s.element_count()
            );
        }
    }
    println!("  classes");
    for (class, count) in model.class_histogram().into_iter().take(classes) {
        println!("    {count:>6}  {class}");
    }
    println!();
}

fn tree(path: &PathBuf, list_elements: bool) -> Result<()> {
    let model = load(path)?;
    let Some(root) = &model.spatial_root else {
        bail!("{} has no IfcProject", path.display());
    };
    print_node(&model, root, 0, list_elements);
    let orphans = model
        .elements
        .iter()
        .filter(|e| e.container.is_none() && e.parent.is_none())
        .count();
    if orphans > 0 {
        println!("({orphans} elements are not placed in the spatial structure)");
    }
    Ok(())
}

fn print_node(model: &IfcModel, node: &SpatialNode, depth: usize, list_elements: bool) {
    let indent = "  ".repeat(depth);
    println!(
        "{indent}{} [{}] {} elements",
        node.label(),
        node.ifc_class,
        node.element_count()
    );
    if list_elements {
        for id in &node.elements {
            print_element_line(model, *id, depth + 1);
        }
    }
    for child in &node.children {
        print_node(model, child, depth + 1, list_elements);
    }
}

fn print_element_line(model: &IfcModel, id: u32, depth: usize) {
    let Some(element) = model.element(id) else {
        return;
    };
    println!(
        "{}- {} [{}] {}",
        "  ".repeat(depth),
        element.label(),
        element.ifc_class,
        element.global_id
    );
    for part in &element.parts {
        print_element_line(model, *part, depth + 1);
    }
}

fn props(path: &PathBuf, key: &str) -> Result<()> {
    let model = load(path)?;
    let element = match key.strip_prefix('#') {
        Some(id) => model.element(id.parse().context("express ID must be a number")?),
        None => model.element_by_global_id(key),
    };
    let Some(element) = element else {
        bail!("no element {key} in {}", path.display());
    };

    println!("{} [{}]", element.label(), element.ifc_class);
    let row = |k: &str, v: &str| println!("  {k:<16} {v}");
    row("GlobalId", &element.global_id);
    row("Express ID", &format!("#{}", element.express_id));
    if !element.bounds.is_empty() {
        let (min, size) = (element.bounds.min, element.bounds.size());
        row(
            "Bounds",
            &format!(
                "{:.3} x {:.3} x {:.3} m at ({:.3}, {:.3}, {:.3})",
                size.x, size.y, size.z, min.x, min.y, min.z
            ),
        );
    }
    for (k, v) in [
        ("Description", &element.description),
        ("ObjectType", &element.object_type),
        ("Tag", &element.tag),
        ("PredefinedType", &element.predefined_type),
    ] {
        if let Some(v) = v {
            row(k, v);
        }
    }
    if let Some((_, type_name)) = &element.type_object {
        row("Type", type_name);
    }
    if let Some(container) = element.container {
        let label = find_node(model.spatial_root.as_ref(), container)
            .map(|n| n.label().to_owned())
            .unwrap_or_else(|| format!("#{container}"));
        row("Container", &label);
    }
    if let Some(type_class) = &element.type_class {
        row("Type class", type_class);
    }
    if element.occurrence_class() != element.ifc_class {
        row("Acts as", element.occurrence_class());
    }
    for c in &element.classifications {
        row(
            "Classification",
            format!("{} {} {}", c.system, c.code, c.name).trim(),
        );
    }
    if !element.materials.is_empty() {
        row("Materials", &element.materials.join(", "));
    }
    if !element.systems.is_empty() {
        row("Systems", &element.systems.join(", "));
    }

    for pset in model.property_sets_of(element) {
        println!("  {}", pset.name);
        for p in &pset.properties {
            let unit = p.value_type.as_deref().unwrap_or("");
            println!("    {:<32} {:<24} {unit}", p.name, p.value.to_string());
        }
    }
    Ok(())
}

fn find_node(node: Option<&SpatialNode>, id: u32) -> Option<&SpatialNode> {
    let node = node?;
    if node.express_id == id {
        return Some(node);
    }
    node.children.iter().find_map(|c| find_node(Some(c), id))
}
