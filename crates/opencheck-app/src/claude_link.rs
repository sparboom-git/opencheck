//! The local connection Claude uses to review the findings of the open
//! project (desktop only).
//!
//! Claude Desktop starts `opencheck mcp`, an MCP server without models of
//! its own. It reads `%APPDATA%\OpenCheck\claude-link.json`, written here
//! while the connection is switched on, and sends each tool call to this
//! app over a TCP connection on 127.0.0.1, one JSON line per request:
//! `{"key": "...", "tool": "list_groups", "args": {...}}`, answered with
//! `{"ok": ...}` or `{"error": "..."}`. The calls are handled on the UI
//! thread, so Claude sees exactly what the reviewer sees, and its
//! suggestions show up in the Check tab as they come in.
//!
//! Claude reads, suggests and, when the reviewer allows it, makes issues
//! (marked as issued by Claude). It cannot approve suggestions, decide on
//! findings, or change models and rulesets. It can ask for pictures of findings: the 3D
//! view is turned to each finding in turn and captured, as for a BCF
//! export, and put back as it was.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::check::{Finding, FindingElement};
use opencheck_core::project;
use opencheck_core::triage::{
    self, Category, LINK_FILE, Lookup, Rating, Source, storey_names, type_name,
};
use opencheck_core::{Element, IfcModel};
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::{Value, json};
use web_time::Instant;

use crate::app::{IssueTarget, OpenCheckApp};
use crate::export::{CAPTURE_TIMEOUT_FRAMES, SETTLE_FRAMES, SavedView, Stage};
use crate::issues::CLAUDE;
use crate::theme;

/// Longest list a tool returns in one page.
const MAX_PAGE: usize = 100;
/// Elements within this distance of a finding are listed with it, metres.
const NEARBY: f32 = 0.5;
/// Most pictures in one call.
const MAX_PICTURES: usize = 12;
/// Width of a picture unless Claude asks for another, pixels.
const PICTURE_WIDTH: usize = 800;

/// State of the connection.
pub struct Link {
    /// The dialog is open.
    pub open: bool,
    /// Element properties may be sent to Claude.
    pub properties: bool,
    /// Claude may make issues.
    pub issues: bool,
    /// Recent calls, newest last.
    pub log: VecDeque<String>,
    pub last_call: Option<Instant>,
    pub status: String,
    /// Pictures being taken for Claude.
    pub pictures: Option<Pictures>,
    server: Option<Server>,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            open: false,
            properties: true,
            issues: true,
            log: VecDeque::new(),
            last_call: None,
            status: String::new(),
            pictures: None,
            server: None,
        }
    }
}

impl Link {
    pub fn is_on(&self) -> bool {
        self.server.is_some()
    }
}

struct Request {
    tool: String,
    args: Value,
    reply: Sender<Result<Value, String>>,
}

/// The direction a picture is taken from.
#[derive(Clone, Copy, PartialEq)]
enum Direction {
    /// From above at an angle, as the 3D view opens.
    Angle,
    /// Plan, straight from above.
    Top,
    /// From the south, north, west or east, as a parallel elevation.
    Side(f32),
    /// The direction the reviewer has the view turned to now.
    Current,
}

/// Pictures for one call: each finding is shown, the view settles, the
/// window is captured and the 3D view cut out of it.
pub struct Pictures {
    /// Finding ids; `None` is the view as it is.
    targets: Vec<Option<String>>,
    direction: Direction,
    width: usize,
    next: usize,
    stage: Stage,
    taken: Vec<Value>,
    missing: Vec<Value>,
    folder: PathBuf,
    saved: Option<SavedView>,
    reply: Sender<Result<Value, String>>,
}

struct Server {
    stop: Arc<AtomicBool>,
    requests: Receiver<Request>,
    file: PathBuf,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.file);
    }
}

/// `%APPDATA%\OpenCheck` (or the XDG config folder elsewhere).
fn app_folder() -> PathBuf {
    crate::library::folder()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn start(ctx: &egui::Context) -> Result<Server, String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let folder = app_folder();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let file = folder.join(LINK_FILE);
    let info = json!({ "port": port, "key": key, "pid": std::process::id() });
    std::fs::write(
        &file,
        serde_json::to_string_pretty(&info).unwrap_or_default(),
    )
    .map_err(|e| format!("{}: {e}", file.display()))?;

    let stop = Arc::new(AtomicBool::new(false));
    let (sender, requests) = mpsc::channel();
    let ctx = ctx.clone();
    let thread_stop = stop.clone();
    std::thread::spawn(move || {
        while !thread_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, _)) => {
                    let (sender, ctx, stop, key) = (
                        sender.clone(),
                        ctx.clone(),
                        thread_stop.clone(),
                        key.clone(),
                    );
                    std::thread::spawn(move || serve(stream, &key, &sender, &ctx, &stop));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    });
    Ok(Server {
        stop,
        requests,
        file,
    })
}

/// One connection from `opencheck mcp`: requests in, answers out.
fn serve(
    stream: TcpStream,
    key: &str,
    sender: &Sender<Request>,
    ctx: &egui::Context,
    stop: &AtomicBool,
) {
    let _ = stream.set_nonblocking(false);
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let answer = match serde_json::from_str::<Value>(&line) {
            Ok(request) if request["key"].as_str() == Some(key) => {
                let (reply, wait) = mpsc::channel();
                let sent = sender.send(Request {
                    tool: request["tool"].as_str().unwrap_or_default().to_owned(),
                    args: request["args"].clone(),
                    reply,
                });
                ctx.request_repaint();
                match sent
                    .ok()
                    .and_then(|_| wait.recv_timeout(Duration::from_secs(120)).ok())
                {
                    Some(Ok(value)) => json!({ "ok": value }),
                    Some(Err(e)) => json!({ "error": e }),
                    None => json!({ "error": "OpenCheck did not answer in time." }),
                }
            }
            Ok(_) => {
                json!({ "error": "Wrong key: switch the connection off and on in OpenCheck." })
            }
            Err(e) => json!({ "error": format!("Not a request: {e}") }),
        };
        if writeln!(writer, "{answer}")
            .and_then(|_| writer.flush())
            .is_err()
        {
            break;
        }
    }
}

/// Handles the calls that came in since the last frame.
pub fn poll(app: &mut OpenCheckApp) {
    let Some(server) = &app.claude.server else {
        return;
    };
    let requests: Vec<Request> = server.requests.try_iter().collect();
    for request in requests {
        if matches!(request.tool.as_str(), "finding_pictures" | "view_picture") {
            let note = match start_pictures(app, &request) {
                Ok(()) => continue,
                Err(e) => {
                    let _ = request.reply.send(Err(e.clone()));
                    format!("{} failed: {e}", request.tool)
                }
            };
            log(app, note);
            continue;
        }
        let result = handle(app, &request.tool, &request.args);
        let note = match &result {
            Ok(_) => request.tool.clone(),
            Err(e) => format!("{} failed: {e}", request.tool),
        };
        log(app, note);
        let _ = request.reply.send(result);
    }
}

/// Notes a call in the dialog's list.
fn log(app: &mut OpenCheckApp, note: String) {
    let link = &mut app.claude;
    link.last_call = Some(Instant::now());
    link.log.push_back(format!("{}  {note}", chrono_like_now()));
    while link.log.len() > 200 {
        link.log.pop_front();
    }
}

// ---- Pictures --------------------------------------------------------------

fn start_pictures(app: &mut OpenCheckApp, request: &Request) -> Result<(), String> {
    let args = &request.args;
    if app.is_capturing() {
        return Err("OpenCheck is taking other pictures: try again in a moment.".to_owned());
    }
    let targets: Vec<Option<String>> = if request.tool == "view_picture" {
        vec![None]
    } else {
        need_report(app)?;
        let ids: Vec<String> = args["ids"]
            .as_array()
            .ok_or("give the finding ids")?
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect();
        if ids.is_empty() {
            return Err("give the finding ids".to_owned());
        }
        if ids.len() > MAX_PICTURES {
            return Err(format!(
                "at most {MAX_PICTURES} pictures per call: ask for the rest in another call"
            ));
        }
        if let Some(unknown) = ids.iter().find(|id| app.checking.find(id).is_none()) {
            return Err(format!("no finding {unknown}"));
        }
        ids.into_iter().map(Some).collect()
    };
    let direction = match args["view"].as_str().unwrap_or("angle") {
        "angle" => Direction::Angle,
        "top" => Direction::Top,
        "south" => Direction::Side(0.0),
        "north" => Direction::Side(std::f32::consts::PI),
        "west" => Direction::Side(std::f32::consts::FRAC_PI_2),
        "east" => Direction::Side(-std::f32::consts::FRAC_PI_2),
        "current" => Direction::Current,
        other => return Err(format!("unknown view {other}")),
    };
    let width = args["width"]
        .as_u64()
        .map_or(PICTURE_WIDTH, |w| w as usize)
        .clamp(320, 1600);
    let folder = picture_folder(app);
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    app.claude.pictures = Some(Pictures {
        targets,
        direction,
        width,
        next: 0,
        stage: Stage::Show,
        taken: Vec::new(),
        missing: Vec::new(),
        folder,
        saved: None,
        reply: request.reply.clone(),
    });
    log(app, format!("{}: taking pictures", request.tool));
    Ok(())
}

/// Where pictures are saved: next to the project file, or in the temporary
/// folder while the project is not saved.
fn picture_folder(app: &OpenCheckApp) -> PathBuf {
    match &app.session.path {
        Some(path) => {
            let stem = path.file_stem().unwrap_or_default().to_string_lossy();
            path.with_file_name(format!("{stem} pictures"))
        }
        None => std::env::temp_dir().join("OpenCheck pictures"),
    }
}

/// A finding id as a file name.
fn file_stem(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// One step of the pictures; called every frame, also while the windows are
/// hidden for the pictures.
pub fn step_pictures(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let Some(job) = &mut app.claude.pictures else {
        return;
    };
    ctx.request_repaint();
    match job.stage {
        Stage::Show => {
            if job.next >= job.targets.len() {
                finish_pictures(app);
                return;
            }
            if ctx.input(|i| i.viewport().minimized) == Some(true) {
                let job = app.claude.pictures.take().expect("checked above");
                if let Some(saved) = job.saved {
                    app.restore_view(saved);
                }
                let _ = job.reply.send(Err(
                    "OpenCheck's window is minimized: restore it so the 3D view can be captured."
                        .to_owned(),
                ));
                return;
            }
            job.stage = Stage::Settle(0);
            let target = job.targets[job.next].clone();
            let direction = job.direction;
            if job.saved.is_none() {
                let saved = app.save_view();
                if let Some(job) = &mut app.claude.pictures {
                    job.saved = Some(saved);
                }
            }
            if let Some(id) = target {
                turn(app, direction);
                app.show_finding(&id, true);
            }
        }
        Stage::Settle(n) if n < SETTLE_FRAMES => job.stage = Stage::Settle(n + 1),
        Stage::Settle(_) => {
            job.stage = Stage::Requested(0);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        Stage::Requested(waited) => {
            let image: Option<Arc<egui::ColorImage>> = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let png = crate::export::crop_png(
                    &image,
                    app.viewport_rect,
                    ctx.pixels_per_point(),
                    job.width,
                );
                let target = job.targets[job.next].clone();
                let name = target
                    .as_deref()
                    .map_or_else(|| "view".to_owned(), file_stem);
                let file = job.folder.join(format!("{name}.png"));
                match png.map(|png| std::fs::write(&file, &png).map(|()| png)) {
                    Some(Ok(png)) => job.taken.push(json!({
                        "finding": target,
                        "file": file.display().to_string(),
                        "png_base64": opencheck_core::bcf::base64(&png),
                    })),
                    Some(Err(e)) => job.missing.push(json!({
                        "finding": target,
                        "why": format!("{}: {e}", file.display()),
                    })),
                    None => job.missing.push(json!({
                        "finding": target,
                        "why": "the 3D view is not on screen",
                    })),
                }
                job.next += 1;
                job.stage = Stage::Show;
            } else if waited >= CAPTURE_TIMEOUT_FRAMES {
                let target = job.targets[job.next].clone();
                job.missing.push(json!({
                    "finding": target,
                    "why": "the window could not be captured",
                }));
                job.next += 1;
                job.stage = Stage::Show;
            } else {
                job.stage = Stage::Requested(waited + 1);
            }
        }
    }
}

/// Turns the camera for a picture; the finding is framed after.
fn turn(app: &mut OpenCheckApp, direction: Direction) {
    let home = crate::camera::Camera::default();
    let (yaw, pitch, ortho) = match direction {
        Direction::Current => return,
        Direction::Angle => (home.yaw, home.pitch, false),
        Direction::Top => (0.0, -89.9_f32.to_radians(), true),
        Direction::Side(yaw) => (yaw, 0.0, true),
    };
    app.camera.yaw = yaw;
    app.camera.pitch = pitch;
    app.camera.ortho = ortho;
}

fn finish_pictures(app: &mut OpenCheckApp) {
    let Some(job) = app.claude.pictures.take() else {
        return;
    };
    if let Some(saved) = job.saved {
        app.restore_view(saved);
    }
    let count = job.taken.len();
    let _ = job.reply.send(Ok(json!({
        "pictures": job.taken,
        "missing": job.missing,
        "folder": job.folder.display().to_string(),
        "colours": "The finding's first element (a) is orange, the second (b) blue; the point of the finding has a marker. Other elements are drawn faint.",
    })));
    log(app, format!("pictures: {count} taken"));
}

/// Where a finding is: in the frame OpenCheck shows the models in (metres
/// from the first model's origin) and in the models' IFC coordinates.
fn location(app: &OpenCheckApp, f: &Finding) -> Value {
    let round = |v: f64| (v * 1000.0).round() / 1000.0;
    let shown = f.point.as_dvec3();
    let ifc = shown + app.federation.origin();
    let point = [shown.x, shown.y, shown.z].map(round);
    let ifc_point = [ifc.x, ifc.y, ifc.z].map(round);
    json!({ "point_m": point, "ifc_point_m": ifc_point })
}

/// The time of day, `HH:MM:SS`, without a time zone library.
fn chrono_like_now() -> String {
    let now = project::now_rfc3339();
    now.get(11..19).unwrap_or_default().to_owned()
}

// ---- The tools -------------------------------------------------------------

fn handle(app: &mut OpenCheckApp, tool: &str, args: &Value) -> Result<Value, String> {
    match tool {
        "project_info" => Ok(project_info(app)),
        "triage_rules" => Ok(json!({
            "rules": app.checking.triage.rules,
            "examples": app.checking.triage.examples,
        })),
        "list_groups" => list_groups(app, args),
        "list_findings" => list_findings(app, args),
        "get_finding" => get_finding(app, args),
        "get_element" => get_element(app, args),
        "rate" => rate(app, args),
        "list_issues" => list_issues(app, args),
        "create_issues" => create_issues(app, args),
        "summary" => Ok(summary(app)),
        other => Err(format!("unknown tool {other}")),
    }
}

fn need_report(app: &OpenCheckApp) -> Result<(), String> {
    if app.checking.report.is_none() {
        Err("No check results yet: run the check in OpenCheck first.".to_owned())
    } else {
        Ok(())
    }
}

fn project_info(app: &OpenCheckApp) -> Value {
    let models: Vec<Value> = app
        .federation
        .iter()
        .map(|(_, m)| {
            json!({
                "name": m.short_name,
                "discipline": m.discipline.label(),
                "schema": m.schema,
                "elements": m.elements.len(),
            })
        })
        .collect();
    let rulesets: Vec<Value> = app
        .checking
        .report
        .as_ref()
        .map(|report| {
            report
                .rulesets
                .iter()
                .map(|rs| {
                    json!({
                        "ruleset": rs.info.name,
                        "rules": rs.rules.iter().map(|r| json!({
                            "rule": r.rule.name,
                            "type": format!("{:?}", r.rule.kind).to_lowercase(),
                            "severity": r.rule.severity.label(),
                            "findings": r.findings.len(),
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    json!({
        "project": app.session.path.as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "Not saved yet".to_owned(), |s| s.to_string_lossy().into_owned()),
        "models": models,
        "results": rulesets,
        "findings": app.checking.report.as_ref().map_or(0, |r| r.finding_count()),
        "unrated": unrated(app).len(),
        "triage_file": {
            "rules": !app.checking.triage.rules.trim().is_empty(),
            "examples": app.checking.triage.examples.len(),
        },
        "properties_allowed": app.claude.properties,
        "issues_allowed": app.claude.issues,
        "issues": app.issues.list.len(),
        "categories": {
            "C1": "Not critical: no effect on quality, cost or time. Noise, agreed and theoretical overlaps. Needs no action.",
            "C2": "Critical: affects quality, cost, time, buildability or construction.",
            "C3": "Very critical: large effect, to be handled before other issues.",
        },
    })
}

/// Findings without a decision, an issue or a suggestion.
fn unrated(app: &OpenCheckApp) -> FxHashSet<String> {
    let Some(report) = &app.checking.report else {
        return FxHashSet::default();
    };
    report
        .rulesets
        .iter()
        .flat_map(|r| &r.rules)
        .flat_map(|r| &r.findings)
        .filter(|f| is_unrated(app, &f.id))
        .map(|f| f.id.clone())
        .collect()
}

fn is_unrated(app: &OpenCheckApp, id: &str) -> bool {
    !app.checking.ratings.contains_key(id)
        && !app.checking.decisions.contains_key(id)
        && app.issues.for_finding(id).is_none()
}

fn page(args: &Value, default: usize) -> (usize, usize) {
    let offset = args["offset"].as_u64().unwrap_or(0) as usize;
    let limit = (args["limit"].as_u64().unwrap_or(default as u64) as usize).clamp(1, MAX_PAGE);
    (offset, limit)
}

fn rating_json(r: &Rating) -> Value {
    json!({
        "category": r.category.code(),
        "noise": r.noise,
        "confidence": r.confidence,
        "reason": r.reason,
        "by": r.source.label(),
        "approved": r.approved.map(|c| c.code()),
    })
}

/// Rule names by (ruleset id, rule id).
fn rule_names(app: &OpenCheckApp) -> FxHashMap<(String, String), String> {
    let mut out = FxHashMap::default();
    if let Some(report) = &app.checking.report {
        for rs in &report.rulesets {
            for r in &rs.rules {
                out.insert(
                    (rs.info.id.clone(), r.rule.id.clone()),
                    format!("{} / {}", rs.info.name, r.rule.name),
                );
            }
        }
    }
    out
}

fn list_groups(app: &OpenCheckApp, args: &Value) -> Result<Value, String> {
    need_report(app)?;
    let report = app.checking.report.clone().expect("checked above");
    let only_unrated = args["unrated"].as_bool().unwrap_or(true);
    let rule = args["rule"].as_str().map(str::to_lowercase);
    let lookup = Lookup::new(&app.federation);
    let names = rule_names(app);
    let open = unrated(app);
    let groups = triage::groups(&report, &lookup, |f| !only_unrated || open.contains(&f.id));
    let groups: Vec<_> = groups
        .into_iter()
        .filter(|g| {
            rule.as_ref().is_none_or(|r| {
                names
                    .get(&(g.ruleset.clone(), g.rule.clone()))
                    .is_some_and(|n| n.to_lowercase().contains(r))
            })
        })
        .collect();
    let (offset, limit) = page(args, 50);
    let total = groups.len();
    let list: Vec<Value> = groups
        .iter()
        .skip(offset)
        .take(limit)
        .map(|g| {
            // What the findings in the group were suggested so far.
            let mut suggested: FxHashMap<String, usize> = FxHashMap::default();
            for id in &g.findings {
                if let Some(r) = app.checking.ratings.get(id) {
                    let key = format!(
                        "{}{} by {}",
                        r.category.code(),
                        if r.noise { " noise" } else { "" },
                        r.source.label()
                    );
                    *suggested.entry(key).or_insert(0) += 1;
                }
            }
            json!({
                "id": g.id,
                "rule": names.get(&(g.ruleset.clone(), g.rule.clone())),
                "a": { "class": g.a.class, "type": g.a.type_name },
                "b": { "class": g.b.class, "type": g.b.type_name },
                "systems": g.systems.iter().take(8).map(|(s, n)| format!("{s} ({n})")).collect::<Vec<_>>(),
                "storeys": g.storeys.iter().take(8).map(|(s, n)| format!("{s} ({n})")).collect::<Vec<_>>(),
                "findings": g.findings.len(),
                "depth_mm": { "min": g.depth_mm[0].round(), "median": g.depth_mm[1].round(), "max": g.depth_mm[2].round() },
                "suggested": suggested,
                "examples": g.findings.iter().take(5).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "total": total, "offset": offset, "groups": list }))
}

fn short(lookup: &Lookup, e: &FindingElement) -> String {
    let s = lookup.side(e);
    let mut text = format!("{} \"{}\"", s.class, e.name);
    if !s.system.is_empty() {
        text.push_str(&format!(" ({})", s.system));
    }
    text
}

fn list_findings(app: &OpenCheckApp, args: &Value) -> Result<Value, String> {
    need_report(app)?;
    let report = app.checking.report.clone().expect("checked above");
    let lookup = Lookup::new(&app.federation);
    let names = rule_names(app);
    let status = args["status"].as_str().unwrap_or("unrated");
    let in_group: Option<FxHashSet<String>> = match args["group"].as_str() {
        Some(gid) => {
            let groups = triage::groups(&report, &lookup, |_| true);
            let group = groups
                .into_iter()
                .find(|g| g.id == gid)
                .ok_or_else(|| format!("no group {gid}"))?;
            Some(group.findings.into_iter().collect())
        }
        None => None,
    };
    let rule = args["rule"].as_str().map(str::to_lowercase);
    let storey = args["storey"].as_str();
    let mut rows = Vec::new();
    for rs in &report.rulesets {
        for r in &rs.rules {
            let name = names
                .get(&(rs.info.id.clone(), r.rule.id.clone()))
                .cloned()
                .unwrap_or_default();
            if rule
                .as_ref()
                .is_some_and(|x| !name.to_lowercase().contains(x))
            {
                continue;
            }
            for f in &r.findings {
                if in_group.as_ref().is_some_and(|g| !g.contains(&f.id)) {
                    continue;
                }
                let rating = app.checking.ratings.get(&f.id);
                let keep = match status {
                    "unrated" => is_unrated(app, &f.id),
                    "proposed" => rating.is_some_and(Rating::is_proposed),
                    "approved" => rating.is_some_and(|r| !r.is_proposed()),
                    _ => true,
                };
                let at = lookup.storey(&f.a);
                if !keep || storey.is_some_and(|s| at != s) {
                    continue;
                }
                rows.push((name.clone(), f, at, rating));
            }
        }
    }
    let (offset, limit) = page(args, 50);
    let list: Vec<Value> = rows
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(rule, f, storey, rating)| {
            json!({
                "id": f.id,
                "rule": rule,
                "a": short(&lookup, &f.a),
                "b": if f.is_single() { Value::Null } else { json!(short(&lookup, &f.b)) },
                "depth_mm": (f.magnitude * 1000.0).round(),
                "storey": storey,
                "ifc_point_m": location(app, f)["ifc_point_m"],
                "rating": rating.map(rating_json),
            })
        })
        .collect();
    Ok(json!({ "total": rows.len(), "offset": offset, "findings": list }))
}

/// A size in metres as whole millimetres.
fn mm3(size: glam::Vec3) -> [f32; 3] {
    [size.x, size.y, size.z].map(|v| (v * 1000.0).round())
}

fn element_json(app: &OpenCheckApp, model: &IfcModel, element: &Element, storey: &str) -> Value {
    let size = element.bounds.size();
    json!({
        "model": model.short_name,
        "discipline": model.discipline.label(),
        "global_id": element.global_id,
        "class": element.ifc_class,
        "kind": element.occurrence_class(),
        "name": element.name,
        "type": type_name(element),
        "predefined_type": element.predefined_type,
        "systems": element.systems.iter().map(|s| json!({
            "name": s,
            "kind": model.system_info.get(s),
        })).collect::<Vec<_>>(),
        "materials": element.materials,
        "storey": storey,
        "size_mm": mm3(size),
        "properties": if app.claude.properties { "use get_element" } else { "not shared" },
    })
}

fn get_finding(app: &OpenCheckApp, args: &Value) -> Result<Value, String> {
    let id = args["id"].as_str().ok_or("give the finding's id")?;
    let (ruleset, rule, f) = app
        .checking
        .find(id)
        .ok_or_else(|| format!("no finding {id}"))?;
    let lookup = Lookup::new(&app.federation);
    let side = |e: &FindingElement| -> Value {
        match (app.federation.get(e.model), lookup.element(e)) {
            (Some(model), Some(element)) => element_json(app, model, element, &lookup.storey(e)),
            _ => json!({ "class": e.ifc_class, "name": e.name }),
        }
    };
    let size = f.region.size();
    Ok(json!({
        "id": f.id,
        "kind": f.kind.label(),
        "ruleset": ruleset.info.name,
        "rule": rule.rule.name,
        "rule_severity": rule.rule.severity.label(),
        "measure": crate::results_ui::magnitude_text(f),
        "depth_mm": (f.magnitude * 1000.0).round(),
        "region_mm": mm3(size),
        "location": location(app, f),
        "details": f.details,
        "a": side(&f.a),
        "b": if f.is_single() { Value::Null } else { side(&f.b) },
        "nearby": nearby(app, f),
        "decision": app.checking.decisions.get(id).map(|d| format!("{d:?}").to_lowercase()),
        "issue": app.issues.for_finding(id).map(|i| i.title.clone()),
        "rating": app.checking.ratings.get(id).map(rating_json),
    }))
}

/// Other elements close to a finding: what surrounds it.
fn nearby(app: &OpenCheckApp, f: &Finding) -> Vec<Value> {
    let mut found = Vec::new();
    let reach = glam::Vec3::splat(NEARBY);
    for (id, model) in app.federation.iter() {
        let Some(transform) = app.federation.model_transform(id).map(|m| m.as_dmat4()) else {
            continue;
        };
        let storeys = storey_names(model);
        for e in &model.elements {
            if !e.has_geometry()
                || (id == f.a.model && e.express_id == f.a.express_id)
                || (id == f.b.model && e.express_id == f.b.express_id)
                || crate::scene::is_space_class(&e.ifc_class)
            {
                continue;
            }
            let b = e.bounds.transformed(&transform);
            if f.point.cmpge(b.min - reach).all() && f.point.cmple(b.max + reach).all() {
                found.push(json!({
                    "model": model.short_name,
                    "class": e.occurrence_class(),
                    "name": e.name,
                    "type": type_name(e),
                    "system": e.systems.first(),
                    "storey": storeys.get(&e.express_id),
                }));
                if found.len() >= 12 {
                    return found;
                }
            }
        }
    }
    found
}

fn get_element(app: &OpenCheckApp, args: &Value) -> Result<Value, String> {
    let gid = args["global_id"]
        .as_str()
        .ok_or("give the element's global_id")?;
    let wanted = args["model"].as_str();
    for (_, model) in app.federation.iter() {
        if wanted.is_some_and(|w| w != model.short_name) {
            continue;
        }
        let Some(element) = model.elements.iter().find(|e| e.global_id == gid) else {
            continue;
        };
        let storeys = storey_names(model);
        let storey = storeys
            .get(&element.express_id)
            .cloned()
            .unwrap_or_default();
        let mut value = element_json(app, model, element, &storey);
        if app.claude.properties {
            let mut sets = serde_json::Map::new();
            for pid in &element.property_sets {
                if let Some(set) = model.property_sets.get(pid) {
                    let props: serde_json::Map<String, Value> = set
                        .properties
                        .iter()
                        .map(|p| (p.name.clone(), json!(p.value.to_string())))
                        .collect();
                    sets.insert(set.name.clone(), Value::Object(props));
                }
            }
            value["properties"] = Value::Object(sets);
            value["attributes"] = element
                .attributes
                .iter()
                .map(|(n, v)| (n.clone(), json!(v.to_string())))
                .collect::<serde_json::Map<_, _>>()
                .into();
            value["classifications"] = element
                .classifications
                .iter()
                .map(|c| json!({ "system": c.system, "code": c.code, "name": c.name }))
                .collect::<Vec<_>>()
                .into();
        }
        return Ok(value);
    }
    Err(format!("no element with GlobalId {gid}"))
}

fn rate(app: &mut OpenCheckApp, args: &Value) -> Result<Value, String> {
    need_report(app)?;
    let items = args["ratings"].as_array().ok_or(
        "give \"ratings\": a list of {finding or group, category, noise, confidence, reason}",
    )?;
    let now = project::now_rfc3339();
    // Every item is checked before any is applied: a bad item in the batch
    // leaves the project as it was, and Claude can send the batch again.
    let mut batch: Vec<(Vec<String>, Rating)> = Vec::with_capacity(items.len());
    let mut unknown = Vec::new();
    let mut groups: Option<FxHashMap<String, Vec<String>>> = None;
    for (n, item) in items.iter().enumerate() {
        let category = item["category"]
            .as_str()
            .and_then(Category::parse)
            .ok_or_else(|| format!("rating {}: needs a category: C1, C2 or C3", n + 1))?;
        let noise = item["noise"].as_bool().unwrap_or(false);
        let rating = Rating {
            category: if noise { Category::C1 } else { category },
            noise,
            confidence: item["confidence"].as_f64().unwrap_or(0.5).clamp(0.0, 1.0) as f32,
            reason: item["reason"]
                .as_str()
                .unwrap_or_default()
                .trim()
                .to_owned(),
            source: Source::Claude,
            approved: None,
            group: item["group"].as_str().unwrap_or_default().to_owned(),
            image: item["image"].as_bool().unwrap_or(false),
            at: now.clone(),
        };
        if rating.reason.is_empty() {
            return Err(format!("rating {}: needs a reason", n + 1));
        }
        let targets: Vec<String> = if let Some(id) = item["finding"].as_str() {
            vec![id.to_owned()]
        } else if let Some(gid) = item["group"].as_str() {
            let groups = groups.get_or_insert_with(|| {
                let report = app.checking.report.as_ref().expect("checked above");
                let lookup = Lookup::new(&app.federation);
                triage::groups(report, &lookup, |_| true)
                    .into_iter()
                    .map(|g| (g.id, g.findings))
                    .collect()
            });
            match groups.get(gid) {
                Some(list) => list.clone(),
                None => {
                    unknown.push(gid.to_owned());
                    continue;
                }
            }
        } else {
            return Err(format!("rating {}: needs a finding or a group id", n + 1));
        };
        batch.push((targets, rating));
    }
    let (mut rated, mut kept) = (0, 0);
    for (targets, rating) in batch {
        for id in targets {
            if app.checking.find(&id).is_none() {
                unknown.push(id);
                continue;
            }
            // Approved suggestions and decided findings stay as they are.
            let settled = app
                .checking
                .ratings
                .get(&id)
                .is_some_and(|r| !r.is_proposed())
                || app.checking.decisions.contains_key(&id)
                || app.issues.for_finding(&id).is_some();
            if settled {
                kept += 1;
                continue;
            }
            app.checking.ratings.insert(id, rating.clone());
            rated += 1;
        }
    }
    if rated > 0 {
        app.session.dirty = true;
    }
    Ok(json!({
        "rated": rated,
        "left_as_decided": kept,
        "unknown": unknown,
        "unrated_left": unrated(app).len(),
    }))
}

fn issue_json(index: usize, issue: &crate::issues::Issue) -> Value {
    json!({
        "number": index + 1,
        "id": issue.guid,
        "title": issue.title,
        "status": issue.status.label(),
        "category": crate::c402::code(issue.priority),
        "responsible": issue.responsible.iter().map(|d| d.code()).collect::<Vec<_>>(),
        "issued_by": issue.created_by,
        "created": issue.created,
        "findings": issue.findings.iter().map(|f| f.finding.clone()).collect::<Vec<_>>(),
    })
}

fn list_issues(app: &OpenCheckApp, args: &Value) -> Result<Value, String> {
    let by = args["issued_by"].as_str();
    let rows: Vec<Value> = app
        .issues
        .list
        .iter()
        .enumerate()
        .filter(|(_, i)| by.is_none_or(|b| i.created_by.eq_ignore_ascii_case(b)))
        .map(|(n, i)| issue_json(n, i))
        .collect();
    let (offset, limit) = page(args, 50);
    let total = rows.len();
    let list: Vec<Value> = rows.into_iter().skip(offset).take(limit).collect();
    Ok(json!({ "total": total, "offset": offset, "issues": list }))
}

/// Makes issues of findings, as issued by Claude.
fn create_issues(app: &mut OpenCheckApp, args: &Value) -> Result<Value, String> {
    if !app.claude.issues {
        return Err(
            "The reviewer has not allowed Claude to make issues (File → Claude connection). \
             Suggest categories with rate instead."
                .to_owned(),
        );
    }
    need_report(app)?;
    let items = args["issues"].as_array().ok_or(
        "give \"issues\": a list of {findings or group, category, title, description, bundle}",
    )?;
    // Every item is checked before any issue is made.
    let mut batch = Vec::with_capacity(items.len());
    for (n, item) in items.iter().enumerate() {
        let category = item["category"]
            .as_str()
            .and_then(Category::parse)
            .ok_or_else(|| format!("issue {}: needs a category: C1, C2 or C3", n + 1))?;
        let mut ids: Vec<String> = item["findings"]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(gid) = item["group"].as_str() {
            let report = app.checking.report.as_ref().expect("checked above");
            let lookup = Lookup::new(&app.federation);
            let group = triage::groups(report, &lookup, |_| true)
                .into_iter()
                .find(|g| g.id == gid)
                .ok_or_else(|| format!("issue {}: no group {gid}", n + 1))?;
            ids.extend(group.findings);
        }
        if ids.is_empty() {
            return Err(format!("issue {}: give its findings or a group", n + 1));
        }
        if let Some(unknown) = ids.iter().find(|id| app.checking.find(id).is_none()) {
            return Err(format!("issue {}: no finding {unknown}", n + 1));
        }
        let text = |key: &str| item[key].as_str().unwrap_or_default().trim().to_owned();
        batch.push((
            ids,
            category,
            text("title"),
            text("description"),
            item["bundle"].as_bool().unwrap_or(true),
        ));
    }

    // Making issues clears the ticked findings: the reviewer's stay ticked.
    let ticked = app.checking.marked.clone();
    let mut made = Vec::new();
    let mut skipped = Vec::new();
    for (ids, category, title, description, bundle) in batch {
        // The reviewer's decisions and existing issues stand.
        let (free, taken): (Vec<String>, Vec<String>) = ids.into_iter().partition(|id| {
            app.issues.for_finding(id).is_none() && !app.checking.decisions.contains_key(id)
        });
        skipped.extend(taken);
        if free.is_empty() {
            continue;
        }
        let before: FxHashSet<String> = app.issues.list.iter().map(|i| i.guid.clone()).collect();
        let target = if bundle {
            IssueTarget::BundleFramed
        } else {
            IssueTarget::EachFramed
        };
        app.add_issues(&free, target);
        for (index, issue) in app.issues.list.iter_mut().enumerate() {
            if before.contains(&issue.guid) {
                continue;
            }
            issue.created_by = CLAUDE.to_owned();
            issue.priority = category.severity();
            if !title.is_empty() && (bundle || free.len() == 1) {
                issue.title = title.clone();
            }
            if !description.is_empty() {
                issue.description = description.clone();
            }
            made.push(issue_json(index, issue));
        }
    }
    app.checking.marked = ticked
        .into_iter()
        .filter(|id| app.issues.for_finding(id).is_none())
        .collect();
    app.refresh_finding_focus();
    if !made.is_empty() {
        app.session.dirty = true;
    }
    Ok(json!({
        "made": made,
        "skipped": skipped,
        "note": "Findings already in an issue, or decided on by the reviewer, were skipped.",
    }))
}

fn summary(app: &OpenCheckApp) -> Value {
    let mut by: FxHashMap<String, usize> = FxHashMap::default();
    let (mut sure, mut likely, mut unsure) = (0, 0, 0);
    for r in app.checking.ratings.values().filter(|r| r.is_proposed()) {
        let key = format!(
            "{}{} by {}",
            r.category.code(),
            if r.noise { " noise" } else { "" },
            r.source.label()
        );
        *by.entry(key).or_insert(0) += 1;
        match r.confidence {
            c if c >= 0.9 => sure += 1,
            c if c >= 0.6 => likely += 1,
            _ => unsure += 1,
        }
    }
    let approved = app
        .checking
        .ratings
        .values()
        .filter(|r| !r.is_proposed())
        .count();
    json!({
        "findings": app.checking.report.as_ref().map_or(0, |r| r.finding_count()),
        "suggested": by,
        "confidence": { "sure (0.9+)": sure, "likely (0.6-0.9)": likely, "for the reviewer (<0.6)": unsure },
        "approved": approved,
        "unrated": unrated(app).len(),
    })
}

// ---- Claude Desktop set-up ------------------------------------------------

/// Claude Desktop's configuration files: every one that exists, newest
/// installs first. Recent versions keep it in
/// `%LOCALAPPDATA%\Claude-Data\claude_<id>\`; older ones in
/// `%APPDATA%\Claude\`, or in the Microsoft Store package's copy of it.
/// With none, the classic place.
fn claude_configs() -> Vec<PathBuf> {
    const FILE: &str = "claude_desktop_config.json";
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let mut found = Vec::new();
    if let Some(local) = env("LOCALAPPDATA") {
        if let Ok(entries) = std::fs::read_dir(local.join("Claude-Data")) {
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("claude"))
                .map(|e| e.path())
                .filter(|d| d.is_dir())
                .collect();
            dirs.sort();
            found.extend(dirs.into_iter().map(|d| d.join(FILE)));
        }
        if let Ok(entries) = std::fs::read_dir(local.join("Packages")) {
            for entry in entries.flatten() {
                let store = entry
                    .path()
                    .join("LocalCache")
                    .join("Roaming")
                    .join("Claude");
                if entry.file_name().to_string_lossy().starts_with("Claude_") && store.exists() {
                    found.push(store.join(FILE));
                }
            }
        }
    }
    let classic = env("APPDATA").unwrap_or_default().join("Claude");
    if classic.exists() || found.is_empty() {
        found.push(classic.join(FILE));
    }
    found
}

/// Adds OpenCheck's MCP server to Claude Desktop, keeping its other
/// settings, in every configuration file it may read. A file that cannot
/// be read as JSON is left alone and reported, and a copy of every file
/// is kept next to it as `.json.bak` before it is written.
fn set_up_claude_desktop() -> Result<Vec<PathBuf>, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let cli = exe
        .parent()
        .map(|d| {
            d.join(if cfg!(windows) {
                "opencheck.exe"
            } else {
                "opencheck"
            })
        })
        .filter(|p| p.exists())
        .ok_or("opencheck.exe was not found next to OpenCheck")?;
    // Every file is read and checked before any is written, so a broken
    // one stops the whole step and nothing is half done.
    let mut updated: Vec<(PathBuf, String)> = Vec::new();
    for path in claude_configs() {
        let config = match std::fs::read_to_string(&path) {
            Ok(text) => with_opencheck_server(&text, &cli)
                .map_err(|e| format!("{}: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                with_opencheck_server("{}", &cli).expect("an empty settings file")
            }
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        updated.push((path, config));
    }
    let mut written = Vec::new();
    for (path, config) in updated {
        if path.exists() {
            let backup = path.with_extension("json.bak");
            std::fs::copy(&path, &backup)
                .map_err(|e| format!("copying {} to {}: {e}", path.display(), backup.display()))?;
        } else if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, config).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
    }
    Ok(written)
}

/// The text of a Claude Desktop settings file with OpenCheck's MCP server
/// added (or replaced) and everything else, and its order, kept. Fails on a
/// file that is not a JSON object.
fn with_opencheck_server(text: &str, cli: &std::path::Path) -> Result<String, String> {
    let mut config: Value = serde_json::from_str(text)
        .map_err(|e| format!("not valid JSON ({e}); fix or remove the file and try again"))?;
    if !config.is_object() {
        return Err("not a settings file (not a JSON object)".to_owned());
    }
    if !config["mcpServers"].is_object() {
        config["mcpServers"] = json!({});
    }
    config["mcpServers"]["opencheck"] = json!({
        "command": cli.to_string_lossy(),
        "args": ["mcp"],
    });
    serde_json::to_string_pretty(&config).map_err(|e| e.to_string())
}

/// The triage skill for Claude Desktop, from the repository.
const SKILL: &str = include_str!("../../../skills/opencheck-triage/SKILL.md");

// ---- The dialog -------------------------------------------------------------

pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    poll(app);
    if app.claude.is_on() {
        // Keep answering while the window is in the background.
        ctx.request_repaint_after(Duration::from_millis(250));
    }
    if !app.claude.open {
        return;
    }
    let mut open = true;
    let dialog = crate::dialog::Dialog {
        id: "claude_link",
        icon: icon::SPARKLE,
        title: "Claude connection",
        size: egui::vec2(560.0, 560.0),
        min: egui::vec2(380.0, 300.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| form(app, ui, ctx));
    if !open {
        app.claude.open = false;
    }
}

fn form(app: &mut OpenCheckApp, ui: &mut egui::Ui, ctx: &egui::Context) {
    ui.label(
        RichText::new(
            "Lets Claude Desktop review the findings of the open project: it reads them and \
             suggests a category (C1–C3) with how sure it is and why. You approve the \
             suggestions in the Check tab. If you allow it, Claude also makes issues, \
             shown as issued by Claude. Claude cannot approve, decide on findings or change \
             models and rulesets. It can ask for pictures of findings: the 3D view turns to \
             each in turn and comes back to yours.",
        )
        .color(theme::text_muted()),
    );
    ui.add_space(8.0);
    let mut on = app.claude.is_on();
    if ui
        .checkbox(&mut on, RichText::new("Allow Claude to connect").strong())
        .changed()
    {
        if on {
            match start(ctx) {
                Ok(server) => {
                    app.claude.server = Some(server);
                    app.claude.status = "Waiting for Claude.".to_owned();
                }
                Err(e) => app.claude.status = format!("Could not start: {e}"),
            }
        } else {
            app.claude.server = None;
            app.claude.status = "Switched off.".to_owned();
        }
    }
    let status = match app.claude.last_call {
        Some(t) if app.claude.is_on() => {
            format!("Last call from Claude {} s ago.", t.elapsed().as_secs())
        }
        _ => app.claude.status.clone(),
    };
    if !status.is_empty() {
        ui.label(RichText::new(status).small().color(theme::text_muted()));
    }
    ui.checkbox(&mut app.claude.issues, "Let Claude make issues")
        .on_hover_text(
            "Issues Claude makes are shown as issued by Claude; filter the issue list to see them",
        );
    ui.checkbox(&mut app.claude.properties, "Share element properties")
        .on_hover_text(
            "Off: Claude sees classes, names, types, systems and sizes, but no property sets",
        );
    ui.label(
        RichText::new(
            "What Claude reads (names, systems, properties, pictures of the 3D view) is sent \
             to Anthropic. Check that the project allows it.",
        )
        .small()
        .color(theme::text_muted()),
    );

    ui.add_space(12.0);
    ui.label(RichText::new("Set up once").strong());
    ui.label(
        RichText::new(
            "Quit Claude Desktop completely (also in the notification area) before adding \
             OpenCheck: it writes its settings file over while it runs.",
        )
        .small()
        .color(theme::text_muted()),
    );
    ui.horizontal_wrapped(|ui| {
        if ui
            .button(format!("{}  Add OpenCheck to Claude Desktop", icon::PLUG))
            .on_hover_text(
                "Adds the opencheck MCP server to claude_desktop_config.json, keeping its other \
                 settings; a copy of the file is kept as claude_desktop_config.json.bak. Quit \
                 Claude Desktop completely first: it writes the file over while it runs.",
            )
            .clicked()
        {
            app.claude.status = match set_up_claude_desktop() {
                Ok(paths) => format!(
                    "Added to {}. Now start Claude Desktop, check Settings → Developer, and start a new chat or task.",
                    paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(" and ")
                ),
                Err(e) => format!("Could not set up Claude Desktop: {e}"),
            };
        }
        if ui
            .button(format!("{}  Save the triage skill…", icon::DOWNLOAD_SIMPLE))
            .on_hover_text("A zip file to add in Claude Desktop under Settings → Capabilities → Skills")
            .clicked()
            && let Some(path) = rfd::FileDialog::new()
                .set_file_name("opencheck-triage.zip")
                .add_filter("Zip", &["zip"])
                .save_file()
        {
            app.claude.status = match std::fs::write(&path, triage::skill_zip(SKILL)) {
                Ok(()) => format!("Saved {}.", path.display()),
                Err(e) => format!("Could not save: {e}"),
            };
        }
    });

    ui.add_space(12.0);
    ui.label(RichText::new("Calls").strong());
    if app.claude.log.is_empty() {
        ui.label(
            RichText::new("None yet.")
                .small()
                .color(theme::text_muted()),
        );
    }
    for line in app.claude.log.iter().rev().take(40) {
        ui.label(RichText::new(line).small().monospace());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_desktop_settings_keep_their_other_entries_and_order() {
        let cli = std::path::Path::new(r"C:\Tools\opencheck.exe");
        let before = r#"{
  "theme": "dark",
  "mcpServers": {
    "zeta": { "command": "zeta" },
    "alpha": { "command": "alpha" }
  }
}"#;
        let after = with_opencheck_server(before, cli).unwrap();
        let value: Value = serde_json::from_str(&after).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["mcpServers"]["zeta"]["command"], "zeta");
        assert_eq!(value["mcpServers"]["alpha"]["command"], "alpha");
        assert_eq!(value["mcpServers"]["opencheck"]["args"], json!(["mcp"]));
        assert_eq!(
            value["mcpServers"]["opencheck"]["command"],
            cli.to_string_lossy().as_ref()
        );
        // The file's own order is kept: theme first, zeta before alpha.
        let (theme, zeta, alpha) = (
            after.find("\"theme\"").unwrap(),
            after.find("\"zeta\"").unwrap(),
            after.find("\"alpha\"").unwrap(),
        );
        assert!(theme < zeta && zeta < alpha);

        // Adding it again replaces the entry instead of doubling it.
        let again = with_opencheck_server(&after, cli).unwrap();
        assert_eq!(again.matches("\"opencheck\"").count(), 1);

        // An empty file gets just the server.
        let fresh: Value =
            serde_json::from_str(&with_opencheck_server("{}", cli).unwrap()).unwrap();
        assert!(fresh["mcpServers"]["opencheck"].is_object());
    }

    #[test]
    fn broken_claude_desktop_settings_are_refused_not_replaced() {
        let cli = std::path::Path::new("opencheck.exe");
        assert!(with_opencheck_server("{ \"mcpServers\": { }, }", cli).is_err());
        assert!(with_opencheck_server("[]", cli).is_err());
        assert!(with_opencheck_server("", cli).is_err());
    }
}
