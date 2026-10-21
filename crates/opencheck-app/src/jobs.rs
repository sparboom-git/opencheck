//! Background work: loading models, opening and saving projects.
//!
//! Every job runs on its own thread and reports back over a channel, so the
//! window never freezes. Each job belongs to a session generation; results
//! from a session the user has since closed (New / Open) are dropped.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use opencheck_core::project::{
    self, Manifest, ModelEntry, ModelSource, ProjectContents, ProjectDocuments, ProjectModel,
    RulesetFile,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use opencheck_core::check::{CheckJob, CheckModel, CheckReport};
use opencheck_core::{IfcModel, ModelId};

pub type JobId = u64;

pub enum JobResult {
    ModelLoaded {
        source: ModelSource,
        /// Metadata stored in the project, when the model comes from one.
        entry: Option<ModelEntry>,
        /// The model this file is a newer version of.
        replaces: Option<ModelId>,
        result: Result<Box<IfcModel>, String>,
    },
    ProjectOpened {
        path: PathBuf,
        result: Result<ProjectContents, String>,
    },
    /// All models of an opened project have been sent.
    ProjectModelsDone,
    Saved {
        path: PathBuf,
        /// Models in the order they were written.
        ids: Vec<ModelId>,
        result: Result<Manifest, String>,
    },
    CheckDone {
        report: Box<CheckReport>,
    },
}

/// Everything a background save needs, captured on the UI thread.
pub struct SaveRequest {
    pub path: PathBuf,
    /// Model ids in the same order as `models`.
    pub ids: Vec<ModelId>,
    pub models: Vec<ProjectModel>,
    pub rulesets: Vec<RulesetFile>,
    /// IDS files of the project.
    pub requirements: Vec<RulesetFile>,
    pub documents: ProjectDocuments,
    pub created: Option<String>,
}

struct Message {
    generation: u64,
    job: JobId,
    /// True for the last message of a job.
    finished: bool,
    result: JobResult,
}

pub struct Jobs {
    sender: Sender<Message>,
    receiver: Receiver<Message>,
    generation: u64,
    next_id: JobId,
    running: Vec<(JobId, String)>,
}

impl Default for Jobs {
    fn default() -> Self {
        let (sender, receiver) = channel();
        Self {
            sender,
            receiver,
            generation: 0,
            next_id: 0,
            running: Vec::new(),
        }
    }
}

/// Sends messages for one job.
struct Reporter {
    sender: Sender<Message>,
    generation: u64,
    job: JobId,
    ctx: egui::Context,
}

impl Reporter {
    fn send(&self, result: JobResult, finished: bool) {
        // The receiver only disappears when the app is closing.
        let _ = self.sender.send(Message {
            generation: self.generation,
            job: self.job,
            finished,
            result,
        });
        self.ctx.request_repaint();
    }
}

impl Jobs {
    fn start(&mut self, label: String, ctx: &egui::Context) -> Reporter {
        let job = self.next_id;
        self.next_id += 1;
        self.running.push((job, label));
        Reporter {
            sender: self.sender.clone(),
            generation: self.generation,
            job,
            ctx: ctx.clone(),
        }
    }

    /// Forgets all running jobs; their results will be ignored.
    pub fn new_session(&mut self) {
        self.generation += 1;
        self.running.clear();
    }

    /// Loads an IFC file; with `replaces` it becomes the next version of that model.
    pub fn load_file(&mut self, path: PathBuf, replaces: Option<ModelId>, ctx: &egui::Context) {
        let verb = if replaces.is_some() {
            "Updating"
        } else {
            "Loading"
        };
        let reporter = self.start(format!("{verb} {}", file_name(&path)), ctx);
        std::thread::spawn(move || {
            let result = opencheck_core::load_ifc_file(&path)
                .map(Box::new)
                .map_err(|e| e.to_string());
            reporter.send(
                JobResult::ModelLoaded {
                    source: ModelSource::File(path),
                    entry: None,
                    replaces,
                    result,
                },
                true,
            );
        });
    }

    /// Reads the project, then its models one by one in manifest order, so the
    /// federation is built in the same order every time.
    pub fn open_project(&mut self, path: PathBuf, ctx: &egui::Context) {
        let reporter = self.start(format!("Opening {}", file_name(&path)), ctx);
        std::thread::spawn(move || {
            let contents = match project::read_project(&path) {
                Ok(c) => c,
                Err(e) => {
                    reporter.send(
                        JobResult::ProjectOpened {
                            path,
                            result: Err(e.to_string()),
                        },
                        true,
                    );
                    return;
                }
            };
            let entries = contents.manifest.models.clone();
            reporter.send(
                JobResult::ProjectOpened {
                    path: path.clone(),
                    result: Ok(contents),
                },
                false,
            );
            for entry in entries {
                let result = project::read_project_model(&path, &entry.file)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        opencheck_core::load_ifc_bytes(&format!("{}.ifc", entry.name), &bytes)
                            .map(Box::new)
                            .map_err(|e| e.to_string())
                    });
                reporter.send(
                    JobResult::ModelLoaded {
                        source: ModelSource::Project {
                            path: path.clone(),
                            entry: entry.file.clone(),
                        },
                        entry: Some(entry),
                        replaces: None,
                        result,
                    },
                    false,
                );
            }
            reporter.send(JobResult::ProjectModelsDone, true);
        });
    }

    pub fn save_project(&mut self, request: SaveRequest, ctx: &egui::Context) {
        let reporter = self.start(format!("Saving {}", file_name(&request.path)), ctx);
        std::thread::spawn(move || {
            let SaveRequest {
                path,
                ids,
                models,
                rulesets,
                requirements,
                documents,
                created,
            } = request;
            let result = project::save_project(
                &path,
                &models,
                &rulesets,
                &requirements,
                &documents,
                created.as_deref(),
            )
            .map_err(|e| e.to_string());
            reporter.send(JobResult::Saved { path, ids, result }, true);
        });
    }

    /// Runs a check; `progress` counts thousandths.
    pub fn run_check(
        &mut self,
        models: Vec<CheckModel>,
        jobs: Vec<CheckJob>,
        progress: Arc<AtomicU32>,
        ctx: &egui::Context,
    ) {
        let reporter = self.start("Checking".to_owned(), ctx);
        std::thread::spawn(move || {
            let ctx = reporter.ctx.clone();
            let report = opencheck_core::check::run_check(&models, &jobs, &|fraction| {
                progress.store((fraction * 1000.0) as u32, Ordering::Relaxed);
                ctx.request_repaint();
            });
            reporter.send(
                JobResult::CheckDone {
                    report: Box::new(report),
                },
                true,
            );
        });
    }

    /// Results of the current session since the last call.
    pub fn poll(&mut self) -> Vec<JobResult> {
        let mut results = Vec::new();
        for message in self.receiver.try_iter() {
            if message.generation != self.generation {
                continue;
            }
            if message.finished {
                self.running.retain(|(id, _)| *id != message.job);
            }
            results.push(message.result);
        }
        results
    }

    pub fn is_busy(&self) -> bool {
        !self.running.is_empty()
    }

    pub fn labels(&self) -> impl Iterator<Item = &str> {
        self.running.iter().map(|(_, label)| label.as_str())
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
