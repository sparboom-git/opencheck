//! File > Autorun: sets up checks that run without the app on a schedule.
//!
//! The dialog writes `autorun.json` (what to check, where, how often) and
//! `autorun.bat` (starts `opencheck autorun` with it) into the output folder,
//! and can create a task in Windows Task Scheduler. The configuration is a
//! snapshot of the project's checks and is also kept in the project.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;
use opencheck_core::Discipline;
use opencheck_core::autorun::{AutorunConfig, ConfigDocument, ConfigRuleset, Every, SourceFile};

use crate::app::OpenCheckApp;
use crate::theme;

const CONFIG_FILE: &str = "autorun.json";
const BAT_FILE: &str = "autorun.bat";
const DAYS: [(&str, &str); 7] = [
    ("MON", "Monday"),
    ("TUE", "Tuesday"),
    ("WED", "Wednesday"),
    ("THU", "Thursday"),
    ("FRI", "Friday"),
    ("SAT", "Saturday"),
    ("SUN", "Sunday"),
];

#[derive(Default)]
pub struct AutorunUi {
    pub open: bool,
    config: Option<AutorunConfig>,
    /// Ruleset paths chosen to run.
    chosen: BTreeSet<String>,
    /// The command line program (opencheck.exe) the task starts.
    program: String,
    status: String,
    running: Option<Child>,
    /// Whether each file's path exists, as last looked up. Looked up again
    /// when the paths change, not every frame (they may be on a network drive).
    found: (Vec<String>, Vec<bool>),
}

impl AutorunUi {
    pub fn open(&mut self) {
        self.open = true;
        self.config = None;
        self.found = (Vec::new(), Vec::new());
    }
}

/// A file to check, read from where the model was added from.
fn source_file(path: &Path, discipline: Discipline) -> SourceFile {
    SourceFile {
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        discipline,
        path: path.display().to_string(),
    }
}

/// The command line program next to the running app.
fn default_program() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("opencheck.exe")))
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "opencheck.exe".to_owned())
}

/// Starting point: the project's saved configuration, or a new one with
/// the loaded models and the enabled rulesets.
fn initial(app: &OpenCheckApp) -> (AutorunConfig, BTreeSet<String>) {
    if let Some(saved) = &app.checking.autorun {
        let chosen = saved.rulesets.iter().map(|r| r.path.clone()).collect();
        return (saved.clone(), chosen);
    }
    let name = app
        .session
        .path
        .as_ref()
        .and_then(|p| p.file_stem())
        .map_or_else(
            || "Project".to_owned(),
            |s| s.to_string_lossy().into_owned(),
        );
    let mut config = AutorunConfig::new(&name);
    config.files = project_files(app);
    let chosen = app
        .checking
        .rulesets
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.path.clone())
        .collect();
    (config, chosen)
}

/// The project's models, each read from where it was added from, in the
/// order of the model list.
fn project_files(app: &OpenCheckApp) -> Vec<SourceFile> {
    app.federation
        .ids()
        .into_iter()
        .filter_map(|id| {
            let record = app.session.records.get(&id)?;
            let path = match (&record.entry.source_path, &record.source) {
                (Some(path), _) => PathBuf::from(path),
                (None, opencheck_core::project::ModelSource::File(path)) => path.clone(),
                _ => return None,
            };
            Some(source_file(&path, record.entry.discipline))
        })
        .collect()
}

pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    if !app.autorun.open {
        return;
    }
    if app.autorun.config.is_none() {
        let (config, chosen) = initial(app);
        app.autorun.config = Some(config);
        app.autorun.chosen = chosen;
        if app.autorun.program.is_empty() {
            app.autorun.program = default_program();
        }
    }
    let paths: Vec<String> = app
        .autorun
        .config
        .as_ref()
        .map(|c| {
            c.files
                .iter()
                .map(|f| f.location(&c.source).display().to_string())
                .collect()
        })
        .unwrap_or_default();
    if app.autorun.found.0 != paths {
        let found = paths.iter().map(|p| Path::new(p).is_file()).collect();
        app.autorun.found = (paths, found);
    }
    // A run started from here: report when it ends.
    if let Some(child) = &mut app.autorun.running {
        match child.try_wait() {
            Ok(Some(status)) => {
                app.autorun.status = if status.success() {
                    format!(
                        "{}  The run finished. Open the dashboard to see it.",
                        icon::CHECK
                    )
                } else {
                    format!(
                        "{}  The run failed; see autorun.log in the output folder.",
                        icon::WARNING
                    )
                };
                app.autorun.running = None;
            }
            Ok(None) => ctx.request_repaint_after(std::time::Duration::from_millis(500)),
            Err(e) => {
                app.autorun.status = format!("The run could not be followed: {e}");
                app.autorun.running = None;
            }
        }
    }

    let mut open = true;
    let mut action = None;
    let dialog = crate::dialog::Dialog {
        id: "autorun",
        icon: icon::CLOCK_COUNTER_CLOCKWISE,
        title: "Autorun",
        size: egui::vec2(720.0, 680.0),
        min: egui::vec2(420.0, 300.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        action = form(app, ui);
    });
    if !open {
        app.autorun.open = false;
    }
    match action {
        Some(Action::Save) => {
            let _ = save(app);
        }
        Some(Action::Run) => run_now(app),
        Some(Action::Schedule) => schedule(app),
        Some(Action::Dashboard) => {
            if let Some(config) = &app.autorun.config {
                let page = Path::new(&config.output).join("dashboard.html");
                if page.exists() {
                    let _ = webbrowser::open(&page.display().to_string());
                } else {
                    app.autorun.status =
                        "There is no dashboard yet: run the checks first.".to_owned();
                }
            }
        }
        None => {}
    }
}

enum Action {
    Save,
    Run,
    Schedule,
    Dashboard,
}

fn section(ui: &mut egui::Ui, title: &str, hint: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).strong());
    if !hint.is_empty() {
        ui.label(RichText::new(hint).small().color(theme::text_muted()));
    }
    ui.add_space(2.0);
}

fn folder_field(ui: &mut egui::Ui, value: &mut String, id: &str) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value)
                .id_salt(id)
                .hint_text(r"\\server\share\project\IFC")
                .desired_width(460.0),
        );
        if ui
            .button(format!("{}  Browse…", icon::FOLDER_OPEN))
            .clicked()
            && let Some(dir) = rfd::FileDialog::new().pick_folder()
        {
            *value = dir.display().to_string();
        }
    });
}

fn form(app: &mut OpenCheckApp, ui: &mut egui::Ui) -> Option<Action> {
    let mut action = None;
    let rulesets: Vec<(String, String, bool)> = app
        .checking
        .rulesets
        .iter()
        .map(|r| (r.path.clone(), r.ruleset.ruleset.name.clone(), r.enabled))
        .collect();
    let project = project_files(app);
    let visual: Vec<String> = app
        .checking
        .visual
        .iter()
        .filter(|r| r.enabled)
        .map(|r| r.name.clone())
        .collect();
    let state = &mut app.autorun;
    let found = &state.found.1;
    let config = state.config.as_mut().expect("set above");

    ui.label(
        RichText::new(
            "Runs the checks without opening OpenCheck, on a schedule: reads the IFC files below, \
             keeps every run in a history and makes a dashboard that shows what is new and what \
             was solved since the run before.",
        )
        .color(theme::text_muted()),
    );

    section(ui, "Project name", "Shown in the dashboard.");
    ui.text_edit_singleline(&mut config.name);

    section(
        ui,
        "IFC files",
        "Only these files are checked, each read from its own path; other IFC files in the same \
         folders are left alone. By default each model is read from where it was added from. \
         Use UNC paths (\\\\server\\share\\…): drive letters may differ between machines.",
    );
    let mut remove = None;
    let mut browse = None;
    egui::Grid::new("autorun_files")
        .num_columns(4)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for (i, file) in config.files.iter_mut().enumerate() {
                egui::ComboBox::from_id_salt(("autorun_discipline", i))
                    .width(120.0)
                    .selected_text(file.discipline.label())
                    .show_ui(ui, |ui| {
                        for d in Discipline::ALL {
                            ui.selectable_value(&mut file.discipline, d, d.label());
                        }
                    });
                let exists = found.get(i).copied().unwrap_or(true);
                let edit = ui
                    .add(
                        egui::TextEdit::singleline(&mut file.path)
                            .id_salt(("autorun_path", i))
                            .desired_width(440.0)
                            .text_color_opt((!exists).then(theme::danger)),
                    )
                    .on_hover_text(if exists {
                        "Where the file is read from"
                    } else {
                        "No file at this path now"
                    });
                if edit.changed() {
                    // The file name names the file in the history.
                    if let Some(name) = Path::new(&file.path).file_name() {
                        file.name = name.to_string_lossy().into_owned();
                    }
                }
                if ui
                    .button(format!("{}  Browse…", icon::FOLDER_OPEN))
                    .on_hover_text("Choose the file")
                    .clicked()
                {
                    browse = Some(i);
                }
                if ui
                    .small_button(icon::X)
                    .on_hover_text("Leave out")
                    .clicked()
                {
                    remove = Some(i);
                }
                ui.end_row();
            }
        });
    if let Some(i) = remove {
        config.files.remove(i);
    }
    if let Some(i) = browse {
        let start = Path::new(&config.files[i].path)
            .parent()
            .map(Path::to_path_buf);
        let mut dialog = rfd::FileDialog::new().add_filter("IFC model", &["ifc", "IFC"]);
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            let discipline = config.files[i].discipline;
            config.files[i] = source_file(&path, discipline);
        }
    }
    if config.files.is_empty() {
        ui.label(
            RichText::new("No files: add the IFC files to check.")
                .small()
                .color(theme::danger()),
        );
    }
    ui.horizontal(|ui| {
        if ui
            .button(format!("{}  Add IFC file…", icon::PLUS))
            .clicked()
            && let Some(paths) = rfd::FileDialog::new()
                .add_filter("IFC model", &["ifc", "IFC"])
                .pick_files()
        {
            for path in paths {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                config.files.push(source_file(
                    &path,
                    Discipline::guess(&name, std::iter::empty()),
                ));
            }
        }
        if ui
            .button(format!(
                "{}  Use the project's models",
                icon::ARROWS_CLOCKWISE
            ))
            .on_hover_text("The models loaded now, each from where it was added from")
            .clicked()
        {
            config.files = project.clone();
        }
    });

    section(
        ui,
        "Checks",
        "The project's rulesets to run. Their current settings are copied when you save; save again after changing them.",
    );
    if rulesets.is_empty() {
        ui.label(
            RichText::new("The project has no rulesets: add them in the Check tab first.")
                .small()
                .color(theme::danger()),
        );
    }
    for (path, name, _) in &rulesets {
        let mut on = state.chosen.contains(path);
        if ui.checkbox(&mut on, name).changed() {
            if on {
                state.chosen.insert(path.clone());
            } else {
                state.chosen.remove(path);
            }
        }
    }

    if !visual.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(format!("Visual checks: {}", visual.join(", "))).small())
            .on_hover_text("The visual checks switched on in the Visual Check tab run too; their problems count as findings.");
    }

    section(
        ui,
        "Output folder",
        "History (one JSON file per run), dashboard.html, autorun.log and the lock file go here; on the network drive, everyone can open the dashboard.",
    );
    folder_field(ui, &mut config.output, "autorun_output");

    section(ui, "When", "");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut config.schedule.every, Every::Daily, "Every day");
        ui.selectable_value(&mut config.schedule.every, Every::Weekly, "Every week on");
        if config.schedule.every == Every::Weekly {
            egui::ComboBox::from_id_salt("autorun_day")
                .selected_text(
                    DAYS.iter()
                        .find(|(k, _)| *k == config.schedule.day)
                        .map_or("Monday", |(_, n)| *n),
                )
                .show_ui(ui, |ui| {
                    for (key, name) in DAYS {
                        ui.selectable_value(&mut config.schedule.day, key.to_owned(), name);
                    }
                });
        }
        ui.label("at");
        ui.add(egui::TextEdit::singleline(&mut config.schedule.time).desired_width(54.0));
    });

    section(
        ui,
        "Program",
        "The command line program the task starts. On a machine that is always on (e.g. a PAM machine), put OpenCheck where that machine can reach it.",
    );
    ui.add(egui::TextEdit::singleline(&mut state.program).desired_width(560.0));

    ui.add_space(12.0);
    let valid = !config.files.is_empty()
        && !config.output.trim().is_empty()
        && (!state.chosen.is_empty() || !visual.is_empty())
        && valid_time(&config.schedule.time);
    let running = state.running.is_some();
    ui.horizontal_wrapped(|ui| {
        let save = egui::Button::new(
            RichText::new(format!("{}  Save configuration", icon::FLOPPY_DISK))
                .strong()
                .color(theme::on_accent()),
        )
        .fill(theme::accent());
        if ui
            .add_enabled(valid, save)
            .on_hover_text("Writes autorun.json and autorun.bat into the output folder")
            .on_disabled_hover_text("Add the IFC files, fill in the output folder, a time (HH:MM) and choose at least one check")
            .clicked()
        {
            action = Some(Action::Save);
        }
        if ui
            .add_enabled(valid && !running, egui::Button::new(format!("{}  Run now", icon::PLAY)))
            .on_hover_text("Saves, then runs once in the background")
            .clicked()
        {
            action = Some(Action::Run);
        }
        if ui
            .add_enabled(
                valid,
                egui::Button::new(format!("{}  Create scheduled task", icon::CALENDAR_PLUS)),
            )
            .on_hover_text("Saves, then adds the task to Windows Task Scheduler on this machine")
            .clicked()
        {
            action = Some(Action::Schedule);
        }
        if ui
            .button(format!("{}  Open dashboard", icon::CHART_LINE_UP))
            .clicked()
        {
            action = Some(Action::Dashboard);
        }
    });
    if running {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Running the checks…");
        });
    }
    if !state.status.is_empty() {
        ui.add_space(4.0);
        ui.label(RichText::new(&state.status).small());
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "The scheduled task runs on the machine it is created on: that machine must be on \
             and reach the network drive at the time. A lock file keeps two machines from \
             checking the project at the same time.",
        )
        .small()
        .color(theme::text_muted()),
    );
    action
}

fn valid_time(text: &str) -> bool {
    let Some((h, m)) = text.split_once(':') else {
        return false;
    };
    h.len() == 2
        && m.len() == 2
        && h.parse::<u32>().is_ok_and(|h| h < 24)
        && m.parse::<u32>().is_ok_and(|m| m < 60)
}

/// Copies the chosen checks into the configuration, writes it and the bat
/// file, and keeps it in the project. Returns the bat file's path.
fn save(app: &mut OpenCheckApp) -> Option<PathBuf> {
    let checking = &app.checking;
    let state = &mut app.autorun;
    let config = state.config.as_mut()?;
    config.rulesets = checking
        .rulesets
        .iter()
        .filter(|r| state.chosen.contains(&r.path))
        .map(|r| ConfigRuleset {
            path: r.path.clone(),
            toml: r.text.clone(),
        })
        .collect();
    config.visual = checking
        .visual
        .iter()
        .filter(|r| r.enabled)
        .cloned()
        .collect();
    config.ids = checking
        .requirements
        .iter()
        .map(|r| ConfigDocument {
            path: r.path.clone(),
            text: r.text.clone(),
        })
        .collect();
    let changes = checking.overrides();
    config.information_changes = (!changes.is_empty()).then(|| changes.to_toml());
    config.level = Some(checking.project_level());

    let output = PathBuf::from(&config.output);
    let result = std::fs::create_dir_all(&output)
        .and_then(|()| std::fs::write(output.join(CONFIG_FILE), config.to_json()))
        .and_then(|()| {
            std::fs::write(
                output.join(BAT_FILE),
                bat_file(&state.program, &output.join(CONFIG_FILE), &config.name),
            )
        });
    let saved = config.clone();
    match result {
        Ok(()) => {
            state.status = format!(
                "{}  Saved {} and {} in {}",
                icon::CHECK,
                CONFIG_FILE,
                BAT_FILE,
                output.display()
            );
            app.checking.autorun = Some(saved);
            app.session.dirty = true;
            Some(output.join(BAT_FILE))
        }
        Err(e) => {
            state.status = format!(
                "{}  Could not write to {}: {e}",
                icon::WARNING,
                output.display()
            );
            None
        }
    }
}

fn bat_file(program: &str, config: &Path, name: &str) -> String {
    // Windows wants CRLF line ends in batch files.
    [
        "@echo off".to_owned(),
        format!("rem OpenCheck autorun for {name}. Made by OpenCheck; the settings are in {CONFIG_FILE}."),
        format!("\"{program}\" autorun \"{}\"", config.display()),
        String::new(),
    ]
    .join("\r\n")
}

fn run_now(app: &mut OpenCheckApp) {
    if save(app).is_none() {
        return;
    }
    let Some(config) = &app.autorun.config else {
        return;
    };
    let config_path = Path::new(&config.output).join(CONFIG_FILE);
    let mut command = Command::new(&app.autorun.program);
    command.arg("autorun").arg(&config_path).arg("--force");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window over the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    match command.spawn() {
        Ok(child) => {
            app.autorun.running = Some(child);
            app.autorun.status = "Running; the log is autorun.log in the output folder.".to_owned();
        }
        Err(e) => {
            app.autorun.status = format!(
                "{}  Could not start {}: {e}",
                icon::WARNING,
                app.autorun.program
            );
        }
    }
}

/// Adds (or replaces) the task in Windows Task Scheduler. The task starts
/// the program itself, not the batch file next to the settings: that one
/// lies on the shared drive, where anyone who can write there could change
/// what runs under this account.
fn schedule(app: &mut OpenCheckApp) {
    if save(app).is_none() {
        return;
    }
    let Some(config) = &app.autorun.config else {
        return;
    };
    let program = &app.autorun.program;
    if program.contains('"') {
        app.autorun.status = format!("{}  The program path cannot contain quotes.", icon::WARNING);
        return;
    }
    let config_path = Path::new(&config.output).join(CONFIG_FILE);
    let command = format!("\"{program}\" autorun \"{}\"", config_path.display());
    // Task Scheduler's limit for the command.
    if command.chars().count() > 261 {
        app.autorun.status = format!(
            "{}  The paths are too long for Task Scheduler (over 261 characters);              use a shorter output folder.",
            icon::WARNING
        );
        return;
    }
    let name = format!(
        "OpenCheck autorun - {}",
        config.name.replace(['\\', '/', ':'], "-")
    );
    let mut args: Vec<String> = vec![
        "/Create".into(),
        "/F".into(),
        "/TN".into(),
        name.clone(),
        "/TR".into(),
        command,
        "/ST".into(),
        config.schedule.time.clone(),
    ];
    match config.schedule.every {
        Every::Daily => args.extend(["/SC".into(), "DAILY".into()]),
        Every::Weekly => args.extend([
            "/SC".into(),
            "WEEKLY".into(),
            "/D".into(),
            config.schedule.day.clone(),
        ]),
    }
    let mut command = Command::new("schtasks");
    command.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing over the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    app.autorun.status = match command.output() {
        Ok(out) if out.status.success() => format!(
            "{}  Task \"{name}\" created on this machine. Change or remove it in Task Scheduler.",
            icon::CHECK
        ),
        Ok(out) => format!(
            "{}  Task Scheduler refused: {}",
            icon::WARNING,
            String::from_utf8_lossy(&out.stderr).trim()
        ),
        Err(e) => format!("{}  Could not start schtasks: {e}", icon::WARNING),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_checked() {
        assert!(valid_time("06:00"));
        assert!(valid_time("23:59"));
        assert!(!valid_time("6:00"));
        assert!(!valid_time("24:00"));
        assert!(!valid_time("06"));
    }

    #[test]
    fn the_bat_file_quotes_paths() {
        let bat = bat_file(
            r"C:\Program Files\OpenCheck\opencheck.exe",
            Path::new(r"\\server\share\out\autorun.json"),
            "P",
        );
        assert!(bat.contains(
            "\"C:\\Program Files\\OpenCheck\\opencheck.exe\" autorun \"\\\\server\\share\\out\\autorun.json\""
        ));
        assert!(bat.contains("\r\n"));
    }
}
