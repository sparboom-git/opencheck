//! The log and the crash reports (desktop only), in
//! `%APPDATA%/OpenCheck/logs`, for finding out why the app stopped.
//!
//! - `session-<start>-<pid>.log`: one per run. How the app started (version,
//!   system, graphics), the warnings and errors of the app and of the
//!   libraries it uses, the errors shown to the user, and how the run ended.
//! - `crash-<start>-<pid>.txt`: written when the program panics: what failed,
//!   where, the call stack and the last lines of the session's log.
//!
//! A run that ends without its closing line did not close normally. The next
//! start finds it and tells the user, who can send the files. Nothing is ever
//! sent from here. The files hold no model content; file names and folder
//! paths can appear in them.
//!
//! Not everything leaves a report: when the system ends the program (out of
//! memory, a fault inside the graphics driver) nothing gets to write one.
//! The missing closing line still shows that it happened.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use eframe::egui::{self, RichText};
use egui_phosphor::regular as icon;

use crate::app::OpenCheckApp;
use crate::theme;

/// Session logs kept; older ones are removed at start.
const KEEP_SESSIONS: usize = 10;
/// Crash reports kept.
const KEEP_CRASHES: usize = 20;
/// A session's log stops growing here, so a warning repeated every frame
/// cannot fill the disk.
const MAX_BYTES: u64 = 2_000_000;
/// Starts the line that closes a session's log.
const END: &str = "== end:";
/// Lines of the session's log repeated in a crash report.
const TAIL_LINES: usize = 40;

/// `%APPDATA%/OpenCheck/logs` (or the XDG config folder elsewhere).
pub fn folder() -> PathBuf {
    crate::library::folder()
        .parent()
        .map(|p| p.join("logs"))
        .unwrap_or_else(|| PathBuf::from("logs"))
}

/// A run's log file.
struct Session {
    dir: PathBuf,
    /// Start time and process id; names the session's files.
    id: String,
    /// The file and how much was written to it.
    file: Mutex<(File, u64)>,
}

impl Session {
    fn start(dir: &Path) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let id = format!(
            "{}-{}",
            opencheck_core::project::now_rfc3339().replace(':', "-"),
            std::process::id()
        );
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(format!("session-{id}.log")))?;
        Ok(Self {
            dir: dir.to_owned(),
            id,
            file: Mutex::new((file, 0)),
        })
    }

    fn log_path(&self) -> PathBuf {
        self.dir.join(format!("session-{}.log", self.id))
    }

    fn crash_path(&self) -> PathBuf {
        self.dir.join(format!("crash-{}.txt", self.id))
    }

    /// Adds a line with the time (UTC) in front.
    fn line(&self, text: &str) {
        // A panic while the lock was held must not stop the crash report.
        let mut guard = self.file.lock().unwrap_or_else(|e| e.into_inner());
        let (file, written) = &mut *guard;
        if *written > MAX_BYTES {
            return;
        }
        let line = format!("{}  {text}\n", opencheck_core::project::now_rfc3339());
        *written += line.len() as u64;
        let _ = file.write_all(line.as_bytes());
        if *written > MAX_BYTES {
            let _ = file.write_all(b"(the log is full; nothing more is written)\n");
        }
    }

    /// Writes the crash report of a panic as the panic hook gets it.
    fn panicked(&self, info: &std::panic::PanicHookInfo<'_>) -> PathBuf {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_owned());
        let location = info
            .location()
            .map_or_else(|| "(unknown)".to_owned(), ToString::to_string);
        let thread = std::thread::current();
        self.crash(
            &message,
            &location,
            thread.name().unwrap_or("(unnamed)"),
            &std::backtrace::Backtrace::force_capture().to_string(),
        )
    }

    /// Writes the crash report of a panic and returns its file.
    fn crash(&self, message: &str, location: &str, thread: &str, stack: &str) -> PathBuf {
        self.line(&format!(
            "PANIC in thread {thread} at {location}: {message}"
        ));
        let tail = std::fs::read_to_string(self.log_path()).unwrap_or_default();
        let tail: Vec<&str> = tail.lines().collect();
        let tail = tail[tail.len().saturating_sub(TAIL_LINES)..].join("\n");
        let report = format!(
            "OpenCheck crash report\n\
             \n\
             OpenCheck   {version}\n\
             System      {os} {arch}\n\
             Time (UTC)  {time}\n\
             Session     {id}\n\
             Thread      {thread}\n\
             \n\
             What failed\n  {message}\n\
             Where\n  {location}\n\
             \n\
             Call stack\n{stack}\n\
             \n\
             Last lines of the session's log\n{tail}\n\
             \n\
             ----\n\n",
            version = env!("CARGO_PKG_VERSION"),
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            time = opencheck_core::project::now_rfc3339(),
            id = self.id,
        );
        let path = self.crash_path();
        // Appended: a run can panic more than once (a job that fails, then the app).
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let _ = file.write_all(report.as_bytes());
        }
        path
    }
}

/// A run that ended without closing normally.
pub struct Unclean {
    /// When it started (UTC), as in its file names.
    pub started: String,
    pub log: PathBuf,
    /// Its crash report, when the program got to write one.
    pub crash: Option<PathBuf>,
}

/// Start and process id of a session's log, from its file name.
fn session_id(path: &Path) -> Option<&str> {
    path.file_name()?
        .to_str()?
        .strip_prefix("session-")?
        .strip_suffix(".log")
}

/// The files in `dir` whose names start with `prefix`, newest first (the
/// names start with the time).
fn files(dir: &Path, prefix: &str) -> Vec<PathBuf> {
    let mut list: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(prefix))
        })
        .collect();
    list.sort();
    list.reverse();
    list
}

/// Whether the program that writes this log still runs (a second window of
/// OpenCheck), so that its missing closing line means nothing yet.
fn still_running(log: &Path) -> bool {
    #[cfg(windows)]
    {
        // The program keeps its log open; nobody else can then have it alone.
        use std::os::windows::fs::OpenOptionsExt as _;
        OpenOptions::new()
            .append(true)
            .share_mode(0)
            .open(log)
            .is_err()
    }
    #[cfg(unix)]
    {
        let Some(pid) = session_id(log).and_then(|id| id.rsplit('-').next()) else {
            return false;
        };
        std::process::Command::new("kill")
            .args(["-0", pid])
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = log;
        false
    }
}

/// The newest run that did not close normally and was not told yet. All
/// such runs get a closing line in their log, so each is told once: also one
/// that stopped while another window of OpenCheck went on and closed well.
fn unclean(dir: &Path, own: &str) -> Option<Unclean> {
    let mut newest = None;
    for log in files(dir, "session-") {
        let Some(id) = session_id(&log) else {
            continue;
        };
        if id == own || still_running(&log) {
            continue;
        }
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text.lines().any(|l| l.contains(END)) {
            continue;
        }
        if let Ok(mut file) = OpenOptions::new().append(true).open(&log) {
            let _ = writeln!(
                file,
                "{}  {END} not closed normally (found at a later start)",
                opencheck_core::project::now_rfc3339()
            );
        }
        if newest.is_some() {
            continue;
        }
        let crash = dir.join(format!("crash-{id}.txt"));
        // The time is what stands before the process id.
        let started = id.rsplit_once('-').map_or(id, |(time, _)| time).to_owned();
        newest = Some(Unclean {
            started,
            crash: crash.is_file().then_some(crash),
            log,
        });
    }
    newest
}

/// Removes all but the newest logs and crash reports.
fn prune(dir: &Path) {
    for (prefix, keep) in [("session-", KEEP_SESSIONS), ("crash-", KEEP_CRASHES)] {
        for old in files(dir, prefix).into_iter().skip(keep) {
            let _ = std::fs::remove_file(old);
        }
    }
}

static SESSION: OnceLock<Session> = OnceLock::new();
static PREVIOUS: Mutex<Option<Unclean>> = Mutex::new(None);

/// Warnings and errors of everything, and what the app itself tells.
struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() <= log::Level::Info && metadata.target().starts_with("opencheck"))
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata())
            && let Some(session) = SESSION.get()
        {
            session.line(&format!(
                "{:<5} {}: {}",
                record.level(),
                record.target(),
                record.args()
            ));
        }
    }

    fn flush(&self) {}
}

/// Starts the session's log and the crash reports. Call first in `main`.
/// Without a folder to write in the app runs as before, without them.
pub fn init() {
    let dir = folder();
    let Ok(session) = Session::start(&dir) else {
        return;
    };
    let previous = unclean(&dir, &session.id);
    prune(&dir);
    if SESSION.set(session).is_err() {
        return;
    }
    *PREVIOUS.lock().unwrap_or_else(|e| e.into_inner()) = previous;

    if log::set_logger(&Logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if let Some(session) = SESSION.get() {
            session.panicked(info);
        }
        // And as before: on the console, where there is one.
        default(info);
    }));

    log::info!(
        "OpenCheck {} started on {} {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    // Asking the system for its version takes a moment; not on the way to the window.
    std::thread::spawn(|| {
        if let Some(version) = system_version() {
            log::info!("system: {version}");
        }
    });
}

/// The version of the operating system, as it names itself.
fn system_version() -> Option<String> {
    #[cfg(windows)]
    let output = {
        use std::os::windows::process::CommandExt as _;
        // No console window flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("cmd")
            .args(["/c", "ver"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?
            .stdout
    };
    #[cfg(target_os = "macos")]
    let output = std::process::Command::new("sw_vers").output().ok()?.stdout;
    #[cfg(not(any(windows, target_os = "macos")))]
    let output = std::fs::read_to_string("/etc/os-release")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))?
        .trim_matches('"')
        .as_bytes()
        .to_vec();
    let text = String::from_utf8_lossy(&output);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// Closes the session's log: the app was closed, it did not stop.
pub fn closed() {
    if let Some(session) = SESSION.get() {
        session.line(&format!("{END} closed normally"));
    }
}

/// The last run, when it did not close normally; once.
pub fn take_previous() -> Option<Unclean> {
    PREVIOUS.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Opens the folder of the logs in the system's file manager, with `file`
/// marked where the file manager can.
pub fn reveal(file: Option<&Path>) {
    let dir = folder();
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(windows)]
    let command = {
        let mut command = std::process::Command::new("explorer");
        match file {
            Some(file) => command.arg("/select,").arg(file),
            None => command.arg(&dir),
        };
        command
    };
    #[cfg(not(windows))]
    let command = {
        let _ = file;
        let mut command = std::process::Command::new(if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        });
        command.arg(&dir);
        command
    };
    let mut command = command;
    if let Err(e) = command.spawn() {
        log::warn!("could not open the log folder: {e}");
    }
}

/// What the files hold and that they stay here, for the dialogs.
const WHAT_IS_IN_THEM: &str = "The files hold no model content, but file names and folder paths \
                               can appear in them. Nothing is sent from OpenCheck: you send them \
                               yourself.";

/// Tells that the last run did not close normally, once, at the start.
pub fn window(app: &mut OpenCheckApp, ctx: &egui::Context) {
    let Some(previous) = &app.unclean else {
        return;
    };
    let mut open = true;
    let mut close = false;
    let dialog = crate::dialog::Dialog {
        id: "unclean_exit",
        icon: icon::WARNING,
        title: "OpenCheck did not close normally last time",
        size: egui::vec2(480.0, 280.0),
        min: egui::vec2(340.0, 200.0),
        scroll: crate::dialog::Scroll::Both,
    };
    crate::dialog::show(ctx, &dialog, &mut open, |ui| {
        ui.label(format!(
            "The session started {} (UTC) stopped without being closed.",
            previous.started
        ));
        ui.add_space(6.0);
        match &previous.crash {
            Some(crash) => {
                ui.label("A crash report was saved:");
                ui.label(
                    RichText::new(crash.file_name().unwrap_or_default().to_string_lossy())
                        .monospace(),
                );
            }
            None => {
                ui.label(
                    "Nothing was recorded about why: the system may have ended the program \
                     (out of memory, a fault in the graphics driver, a shutdown). The session's \
                     log is saved:",
                );
                ui.label(
                    RichText::new(
                        previous
                            .log
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy(),
                    )
                    .monospace(),
                );
            }
        }
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "When you report the problem, send the file along with a note of what you were \
                 doing. {WHAT_IS_IN_THEM}"
            ))
            .small()
            .color(theme::text_muted()),
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .button(format!("{}  Show the file", icon::FOLDER_OPEN))
                .clicked()
            {
                reveal(Some(previous.crash.as_deref().unwrap_or(&previous.log)));
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
    });
    if !open || close {
        app.unclean = None;
    }
}

/// The part of the Settings dialog about the logs.
pub fn section(ui: &mut egui::Ui) {
    ui.label(RichText::new(format!("{}  Logs and crash reports", icon::BUG)).strong());
    ui.add_space(4.0);
    ui.label(
        RichText::new(format!(
            "OpenCheck keeps a log of each of its last {KEEP_SESSIONS} sessions (warnings and \
             errors), and writes a crash report when it fails. If something goes wrong, send \
             the newest files along when you report it. {WHAT_IS_IN_THEM}"
        ))
        .small()
        .color(theme::text_muted()),
    );
    ui.add_space(8.0);
    if ui
        .button(format!("{}  Open the log folder", icon::FOLDER_OPEN))
        .on_hover_text(folder().display().to_string())
        .clicked()
    {
        reveal(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own under the system's temporary one.
    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("opencheck-logs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_crash_report_tells_what_failed_and_where() {
        let dir = temp("crash");
        let session = Session::start(&dir).unwrap();
        session.line("INFO  opencheck_app: before");
        let path = session.crash(
            "index out of bounds",
            "src/scene.rs:12:5",
            "main",
            "0: frame",
        );
        let report = std::fs::read_to_string(&path).unwrap();
        assert!(report.contains("index out of bounds"));
        assert!(report.contains("src/scene.rs:12:5"));
        assert!(report.contains(env!("CARGO_PKG_VERSION")));
        // The log's last lines are in it, the panic itself too.
        assert!(report.contains("opencheck_app: before"));
        assert!(report.contains("PANIC in thread main"));
        // A second panic of the run is added to the same file.
        session.crash("again", "src/app.rs:1:1", "job", "");
        let report = std::fs::read_to_string(&path).unwrap();
        assert_eq!(report.matches("OpenCheck crash report").count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_panic_is_reported_through_the_hook() {
        let dir = temp("hook");
        let session = std::sync::Arc::new(Session::start(&dir).unwrap());
        let in_hook = session.clone();
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Only this test's own panic: others run at the same time.
            if std::thread::current().name() == Some("logs-hook-test") {
                in_hook.panicked(info);
            }
        }));
        let failed = std::thread::Builder::new()
            .name("logs-hook-test".to_owned())
            .spawn(|| panic!("the wall {} fell", 7))
            .unwrap()
            .join();
        std::panic::set_hook(before);
        assert!(failed.is_err());
        let report = std::fs::read_to_string(session.crash_path()).unwrap();
        assert!(report.contains("the wall 7 fell"), "{report}");
        assert!(report.contains("logs.rs"), "{report}");
        assert!(report.contains("logs-hook-test"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_run_without_its_closing_line_is_told_once() {
        let dir = temp("unclean");
        // A run that did not close (its process is gone), with runs that
        // closed before and after it.
        for day in ["01", "03"] {
            std::fs::write(
                dir.join(format!("session-2026-01-{day}T10-00-00Z-999999991.log")),
                format!("t  {END} closed normally\n"),
            )
            .unwrap();
        }
        let stopped = dir.join("session-2026-01-02T10-00-00Z-999999992.log");
        std::fs::write(&stopped, "t  INFO started\n").unwrap();
        std::fs::write(dir.join("crash-2026-01-02T10-00-00Z-999999992.txt"), "x").unwrap();

        let found = unclean(&dir, "2026-01-04T10-00-00Z-1").expect("the run that stopped");
        assert_eq!(found.started, "2026-01-02T10-00-00Z");
        assert_eq!(found.log, stopped);
        assert!(found.crash.is_some());
        // Told once: its log is closed now.
        assert!(unclean(&dir, "2026-01-04T10-00-00Z-1").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_run_that_closed_is_not_told() {
        let dir = temp("clean");
        // The run before the last one stopped; that was told back then.
        std::fs::write(
            dir.join("session-2026-01-01T10-00-00Z-999999991.log"),
            format!("t  {END} not closed normally\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("session-2026-01-02T10-00-00Z-999999992.log"),
            format!("t  {END} closed normally\n"),
        )
        .unwrap();
        assert!(unclean(&dir, "2026-01-04T10-00-00Z-1").is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_own_and_running_sessions_are_left_alone() {
        let dir = temp("running");
        // This test's own session keeps its file open, as a running app does.
        let session = Session::start(&dir).unwrap();
        assert!(unclean(&dir, &session.id).is_none());
        assert!(still_running(&session.log_path()));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn old_files_are_removed() {
        let dir = temp("prune");
        for day in 10..30 {
            std::fs::write(
                dir.join(format!("session-2026-01-{day}T10-00-00Z-1.log")),
                "",
            )
            .unwrap();
        }
        prune(&dir);
        let left = files(&dir, "session-");
        assert_eq!(left.len(), KEEP_SESSIONS);
        assert!(left[0].ends_with("session-2026-01-29T10-00-00Z-1.log"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_full_log_stops_growing() {
        let dir = temp("full");
        let session = Session::start(&dir).unwrap();
        let long = "x".repeat(100_000);
        for _ in 0..40 {
            session.line(&long);
        }
        let size = std::fs::metadata(session.log_path()).unwrap().len();
        assert!(size < MAX_BYTES + 200_000, "{size}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
