//! `opencheck autorun <config>`: one scheduled run without the app.
//!
//! Takes the lock file, waits until the IFC files are completely copied,
//! checks whether they changed since the last run, runs the checks, keeps
//! the run as a JSON file in the history and writes the dashboard.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use opencheck_core::autorun::{
    AutorunConfig, FORMAT, RunRecord, dashboard_data, dashboard_html, file_facts, missing_per_file,
    run_findings,
};
use opencheck_core::check::{
    CheckJob, CheckModel, Ids, Ruleset, StandardOverrides, builtin_standard, run_check,
};
use opencheck_core::project::sha256_hex;
use opencheck_core::{Federation, IfcModel, ModelId, load_ifc_bytes};
use serde::{Deserialize, Serialize};

/// A lock older than this was left by a run that stopped; it is taken over.
const STALE_LOCK: Duration = Duration::from_secs(12 * 3600);
/// How often a file is measured again while it is still being copied.
const SETTLE_TRIES: u32 = 30;

const HISTORY: &str = "history";
const LOCK: &str = "autorun.lock";
const STATE: &str = "state.json";
const LOG: &str = "autorun.log";
pub const DASHBOARD: &str = "dashboard.html";

/// The log next to the history; every line also goes to the console.
struct Log(Option<File>);

impl Log {
    fn open(output: &Path) -> Self {
        Self(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(output.join(LOG))
                .ok(),
        )
    }

    fn line(&mut self, text: &str) {
        let line = format!("{} {text}", Local::now().format("%Y-%m-%d %H:%M:%S"));
        println!("{line}");
        if let Some(file) = &mut self.0 {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LockFile {
    machine: String,
    started: String,
}

/// Removes the lock when the run ends, however it ends.
struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A file to check: name, path, size and last change.
type Found = (String, PathBuf, u64, String);
/// Size and last change of a file.
type Stamp = (u64, String);

/// What the last run saw of each file, to tell changed files from unchanged.
#[derive(Default, Serialize, Deserialize)]
struct State {
    files: BTreeMap<String, SeenFile>,
    /// Hash of the configuration, so changed checks also count as a change.
    config: String,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct SeenFile {
    size: u64,
    modified: String,
    hash: String,
}

fn machine() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_owned())
}

fn local(time: SystemTime) -> String {
    DateTime::<Local>::from(time)
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

pub fn autorun(config_path: &Path, force: bool) -> Result<()> {
    let text = fs::read_to_string(config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = AutorunConfig::from_json(&text)
        .map_err(|e| anyhow::anyhow!("{}: {e}", config_path.display()))?;
    let output = PathBuf::from(&config.output);
    fs::create_dir_all(output.join(HISTORY))
        .with_context(|| format!("creating {}", output.display()))?;
    let mut log = Log::open(&output);
    log.line(&format!("Autorun of \"{}\" on {}", config.name, machine()));

    // Only one machine runs a project at a time.
    let lock_path = output.join(LOCK);
    if let Ok(existing) = fs::read_to_string(&lock_path) {
        let age = fs::metadata(&lock_path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| m.elapsed().ok())
            .unwrap_or_default();
        let who = serde_json::from_str::<LockFile>(&existing)
            .map(|l| format!("{} since {}", l.machine, l.started))
            .unwrap_or_else(|_| "another run".to_owned());
        if age < STALE_LOCK {
            log.line(&format!("Skipped: the project is being checked by {who}."));
            return Ok(());
        }
        log.line(&format!("Taking over a lock left by {who}."));
    }
    let lock = LockFile {
        machine: machine(),
        started: local(SystemTime::now()),
    };
    fs::write(&lock_path, serde_json::to_string_pretty(&lock)?)
        .with_context(|| format!("writing {}", lock_path.display()))?;
    let _guard = LockGuard(lock_path);

    let result = run(&config, &output, force, &mut log);
    if let Err(e) = &result {
        log.line(&format!("Failed: {e:#}"));
    }
    result
}

fn run(config: &AutorunConfig, output: &Path, force: bool, log: &mut Log) -> Result<()> {
    let files = source_files(config, log)?;
    if files.is_empty() {
        bail!("none of the IFC files in the configuration could be found");
    }
    let files = settle(files, Duration::from_secs(config.settle_seconds), log);

    // Version check: size and date first, the hash only for files that changed.
    let state_path = output.join(STATE);
    let state: State = fs::read_to_string(&state_path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let config_hash = sha256_hex(config.to_json().as_bytes());
    let mut seen: BTreeMap<String, SeenFile> = BTreeMap::new();
    let mut changed = Vec::new();
    for (name, _, size, modified) in &files {
        let previous = state.files.get(name);
        let same_stamp = previous.is_some_and(|p| p.size == *size && p.modified == *modified);
        if !same_stamp {
            changed.push(name.clone());
        }
        seen.insert(
            name.clone(),
            SeenFile {
                size: *size,
                modified: modified.clone(),
                hash: previous.map(|p| p.hash.clone()).unwrap_or_default(),
            },
        );
    }
    let removed: Vec<&String> = state
        .files
        .keys()
        .filter(|k| !seen.contains_key(*k))
        .collect();
    let config_changed = state.config != config_hash;
    if changed.is_empty() && removed.is_empty() && !config_changed && !force {
        log.line("No file and no check changed since the last run; nothing to do.");
        write_dashboard(config, output, log)?;
        return Ok(());
    }

    // Read every file once: for its hash and for the check.
    let loaded: Vec<Result<(String, Vec<u8>, IfcModel)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .iter()
            .map(|(name, path, _, _)| {
                scope.spawn(move || -> Result<(String, Vec<u8>, IfcModel)> {
                    let bytes =
                        fs::read(path).with_context(|| format!("reading {}", path.display()))?;
                    let model =
                        load_ifc_bytes(name, &bytes).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
                    Ok((name.clone(), bytes, model))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .unwrap_or_else(|_| bail!("a model could not be read"))
            })
            .collect()
    });

    let mut federation = Federation::new();
    let mut by_id: BTreeMap<ModelId, String> = BTreeMap::new();
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut really_changed = 0;
    for result in loaded {
        let (name, bytes, mut model) = match result {
            Ok(x) => x,
            Err(e) => {
                log.line(&format!("Left out: {e:#}"));
                continue;
            }
        };
        let hash = sha256_hex(&bytes);
        if let Some(entry) = seen.get_mut(&name) {
            if changed.contains(&name) && entry.hash != hash {
                really_changed += 1;
            }
            entry.hash = hash.clone();
        }
        hashes.insert(name.clone(), hash);
        if let Some(discipline) = config.discipline_of(&name) {
            model.discipline = discipline;
        }
        log.line(&format!(
            "{name}: {} elements, {}{}",
            model.elements.len(),
            model.discipline.code(),
            if changed.contains(&name) {
                ", changed"
            } else {
                ""
            }
        ));
        let id = federation.add(model);
        by_id.insert(id, name);
    }
    if by_id.is_empty() {
        bail!("none of the IFC files could be read");
    }
    if really_changed == 0 && removed.is_empty() && !config_changed && !force {
        // Dates changed, contents did not (a file copied again).
        log.line("The files were copied again but their contents are the same; nothing to do.");
        save_state(&state_path, &seen, &config_hash)?;
        write_dashboard(config, output, log)?;
        return Ok(());
    }

    let report = check(config, &federation)?;
    let model_info = |id: ModelId| {
        (
            federation
                .get(id)
                .map(|m| m.discipline.code().to_owned())
                .unwrap_or_default(),
            by_id.get(&id).cloned().unwrap_or_default(),
        )
    };
    let findings = run_findings(&report, &model_info);
    let missing = missing_per_file(&findings);
    let has_information = findings.iter().any(|f| f.check_type == "information")
        || config
            .rulesets
            .iter()
            .any(|r| r.toml.contains("type = \"information\""));
    let facts = by_id
        .iter()
        .filter_map(|(id, name)| {
            let model = federation.get(*id)?;
            let seen = seen.get(name)?;
            Some(file_facts(
                model,
                name,
                seen.size,
                hashes.get(name).map_or("", String::as_str),
                &seen.modified,
                has_information.then(|| missing.get(name).copied().unwrap_or(0)),
            ))
        })
        .collect();
    let started = Local::now();
    let record = RunRecord {
        format: FORMAT,
        run_id: started.format("%Y-%m-%dT%H:%M:%S").to_string(),
        machine: machine(),
        generator: format!("OpenCheck {}", env!("CARGO_PKG_VERSION")),
        files: facts,
        findings,
    };

    // History files are never overwritten.
    let history = output.join(HISTORY);
    let mut path = history.join(format!("run_{}.json", started.format("%Y-%m-%d_%H%M")));
    if path.exists() {
        path = history.join(format!("run_{}.json", started.format("%Y-%m-%d_%H%M%S")));
    }
    fs::write(&path, serde_json::to_string_pretty(&record)?)
        .with_context(|| format!("writing {}", path.display()))?;
    log.line(&format!(
        "{} findings in {:.1} s; kept as {}",
        record.findings.len(),
        report.duration.as_secs_f32(),
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    save_state(&state_path, &seen, &config_hash)?;
    write_dashboard(config, output, log)
}

/// The IFC files to check: the listed ones, each from its own path. Other
/// files in the same folders are left alone, unless the configuration asks
/// for new files in its `source` folder. (name, path, size, modified)
fn source_files(config: &AutorunConfig, log: &mut Log) -> Result<Vec<Found>> {
    let mut out = Vec::new();
    for f in &config.files {
        let path = f.location(&config.source);
        if path.is_file() {
            out.push((f.name.clone(), path, 0, String::new()));
        } else {
            log.line(&format!("Missing: {} ({}).", f.name, path.display()));
        }
    }
    if config.new_files && !config.source.is_empty() {
        let dir = Path::new(&config.source);
        let entries =
            fs::read_dir(dir).with_context(|| format!("reading the folder {}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.to_ascii_lowercase().ends_with(".ifc")
                || !path.is_file()
                || out.iter().any(|(n, ..)| n.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            log.line(&format!(
                "New file {name}: checked with a guessed discipline."
            ));
            out.push((name, path, 0, String::new()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// Waits until each file's size stops changing and it can be opened, so a
/// file still being copied is not read. Files that never settle are left out.
fn settle(files: Vec<Found>, wait: Duration, log: &mut Log) -> Vec<Found> {
    let stamp = |path: &Path| -> Option<Stamp> {
        let m = fs::metadata(path).ok()?;
        Some((m.len(), local(m.modified().ok()?)))
    };
    let mut pending: Vec<(String, PathBuf, Option<Stamp>)> = files
        .into_iter()
        .map(|(name, path, ..)| {
            let s = stamp(&path);
            (name, path, s)
        })
        .collect();
    let mut ready = Vec::new();
    for _ in 0..SETTLE_TRIES {
        if pending.is_empty() {
            break;
        }
        std::thread::sleep(wait);
        let mut still = Vec::new();
        for (name, path, before) in pending {
            let now = stamp(&path);
            let readable = File::open(&path).is_ok();
            match (&before, &now) {
                (Some(b), Some(n)) if b == n && readable => {
                    ready.push((name, path, n.0, n.1.clone()));
                }
                _ => still.push((name, path, now)),
            }
        }
        pending = still;
    }
    for (name, ..) in pending {
        log.line(&format!("Left out: {name} was still being written."));
    }
    ready.sort_by(|a, b| a.0.cmp(&b.0));
    ready
}

fn check(
    config: &AutorunConfig,
    federation: &Federation,
) -> Result<opencheck_core::check::CheckReport> {
    let mut rulesets = Vec::new();
    for r in &config.rulesets {
        rulesets.push(Ruleset::from_toml(&r.toml).with_context(|| format!("ruleset {}", r.path))?);
    }
    if rulesets.is_empty() {
        bail!("the configuration has no rulesets");
    }
    let mut ids = Vec::new();
    for d in &config.ids {
        let doc = Ids::from_xml(&d.text).map_err(|e| anyhow::anyhow!("{}: {e}", d.path))?;
        ids.push((d.path.clone(), Arc::new(doc)));
    }
    let mut standards = Vec::new();
    if let Some(text) = &config.information_changes {
        let changes = StandardOverrides::from_toml(text).context("information changes")?;
        if let Some(base) = builtin_standard(&changes.base) {
            standards.push(Arc::new(changes.apply(base).0));
        }
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
            levels: config
                .level
                .iter()
                .flat_map(|l| federation.ids().into_iter().map(move |id| (id, l.clone())))
                .collect(),
            ids: ids.clone(),
            standards: standards.clone(),
        })
        .collect();
    Ok(run_check(&models, &jobs, &|_| {}))
}

fn save_state(path: &Path, files: &BTreeMap<String, SeenFile>, config: &str) -> Result<()> {
    let state = State {
        files: files.clone(),
        config: config.to_owned(),
    };
    fs::write(path, serde_json::to_string_pretty(&state)?)
        .with_context(|| format!("writing {}", path.display()))
}

/// Writes the dashboard from all runs in the history. It is written to a
/// temporary name first, so nobody opens a half-written page.
fn write_dashboard(config: &AutorunConfig, output: &Path, log: &mut Log) -> Result<()> {
    let mut runs: Vec<RunRecord> = Vec::new();
    for entry in fs::read_dir(output.join(HISTORY))?.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            match fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<RunRecord>(&t).ok())
            {
                Some(run) => runs.push(run),
                None => log.line(&format!(
                    "Skipped unreadable history file {}",
                    path.display()
                )),
            }
        }
    }
    runs.sort_by(|a, b| a.run_id.cmp(&b.run_id));
    let data = dashboard_data(
        &config.name,
        &Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        &runs,
    );
    let html = dashboard_html(&data);
    let target = output.join(DASHBOARD);
    let temp = output.join(format!("{DASHBOARD}.writing"));
    fs::write(&temp, html).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, &target).with_context(|| format!("replacing {}", target.display()))?;
    log.line(&format!(
        "Dashboard updated: {} ({} runs)",
        target.display(),
        runs.len()
    ));
    Ok(())
}
