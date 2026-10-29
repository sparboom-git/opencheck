//! The `.opencheck` project file.
//!
//! A project is a ZIP archive with plain, documented content (see
//! `docs/file-format.md`):
//!
//! ```text
//! manifest.json        format version, timestamps, the list of models
//! models/<name>.ifc    the IFC files, byte for byte as delivered
//! rulesets/<name>.toml rulesets used by the project
//! requirements/<name>.ids IDS files that information rules refer to
//! view.json            saved view state of the application
//! ```
//!
//! IFC files are never rewritten. When a project is saved again, entries that
//! did not change are copied over still compressed, so saving stays fast even
//! for large federations.

use std::fs::File;
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::Discipline;

pub const PROJECT_EXTENSION: &str = "opencheck";
pub const FORMAT_NAME: &str = "opencheck-project";
pub const FORMAT_VERSION: u32 = 1;

const MANIFEST: &str = "manifest.json";
const VIEW: &str = "view.json";
const CHECKING: &str = "checking.json";
const ISSUES: &str = "issues.json";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("not a valid OpenCheck project: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("invalid project data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Format(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub format_version: u32,
    /// Application that wrote the file, e.g. `OpenCheck 0.1.0`.
    pub generator: String,
    /// RFC 3339 timestamps.
    pub created: String,
    pub modified: String,
    pub models: Vec<ModelEntry>,
    /// Archive paths of the rulesets, e.g. `rulesets/mep-vs-structure.toml`.
    #[serde(default)]
    pub rulesets: Vec<String>,
    /// Archive paths of IDS files, e.g. `requirements/delivery.ids`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<String>,
}

/// One IFC file in the project.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelEntry {
    /// Archive path, e.g. `models/ARK.ifc`.
    pub file: String,
    /// Short name shown in the application.
    pub name: String,
    pub discipline: Discipline,
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Where the IFC file was imported from, for reference only.
    #[serde(default)]
    pub source_path: Option<String>,
    /// SHA-256 of the IFC file, lowercase hex.
    pub sha256: String,
    /// Starts at 1 and increases each time the model is replaced by a new version.
    #[serde(default = "default_revision")]
    pub revision: u32,
    pub added: String,
    pub updated: String,
}

fn default_true() -> bool {
    true
}

fn default_revision() -> u32 {
    1
}

/// A text file stored in the project: a ruleset or an IDS file.
#[derive(Debug, Clone, PartialEq)]
pub struct RulesetFile {
    /// Archive path, e.g. `rulesets/clash.toml` or `requirements/x.ids`.
    pub path: String,
    pub text: String,
}

/// Everything in a project except the IFC files, which are read on demand.
#[derive(Debug, Clone)]
pub struct ProjectContents {
    pub manifest: Manifest,
    pub rulesets: Vec<RulesetFile>,
    /// IDS files (`requirements/*.ids`).
    pub requirements: Vec<RulesetFile>,
    pub documents: ProjectDocuments,
}

/// Application state stored as JSON documents, kept untyped so the core does
/// not depend on UI types.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProjectDocuments {
    /// `view.json`: camera, hidden elements, section, measurements.
    pub view: Option<serde_json::Value>,
    /// `checking.json`: ruleset settings and decisions on findings.
    pub checking: Option<serde_json::Value>,
    /// `issues.json`: the selected issues, modelled on BCF topics.
    pub issues: Option<serde_json::Value>,
}

/// Where the bytes of a model come from when the project is saved.
#[derive(Debug, Clone, PartialEq)]
pub enum ModelSource {
    /// An IFC file on disk that is not yet part of the project.
    File(PathBuf),
    /// An entry in an existing project file.
    Project { path: PathBuf, entry: String },
    /// IFC bytes held in memory, where there are no files (the browser).
    Memory { name: String, bytes: Arc<[u8]> },
}

/// A model to write into a project.
#[derive(Debug, Clone)]
pub struct ProjectModel {
    /// Metadata; `file` is assigned when saving.
    pub entry: ModelEntry,
    pub source: ModelSource,
}

/// A time as RFC 3339 in UTC, to the second.
pub fn rfc3339(time: std::time::SystemTime) -> String {
    humantime::format_rfc3339_seconds(time).to_string()
}

pub fn now_rfc3339() -> String {
    // web_time's clock also works in the browser; humantime wants std's type.
    let since_epoch = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .unwrap_or_default();
    humantime::format_rfc3339_seconds(std::time::UNIX_EPOCH + since_epoch).to_string()
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Reads the manifest, rulesets and view state of a project.
pub fn read_project(path: &Path) -> Result<ProjectContents, ProjectError> {
    read_contents(ZipArchive::new(BufReader::new(File::open(path)?))?)
}

/// [`read_project`] for a project held in memory.
pub fn read_project_bytes(bytes: &[u8]) -> Result<ProjectContents, ProjectError> {
    read_contents(ZipArchive::new(Cursor::new(bytes))?)
}

fn read_contents<R: Read + Seek>(
    mut archive: ZipArchive<R>,
) -> Result<ProjectContents, ProjectError> {
    let manifest: Manifest = serde_json::from_reader(archive.by_name(MANIFEST)?)?;
    if manifest.format != FORMAT_NAME {
        return Err(ProjectError::Format(format!(
            "unknown format '{}'",
            manifest.format
        )));
    }
    if manifest.format_version > FORMAT_VERSION {
        return Err(ProjectError::Format(format!(
            "the project was saved by a newer OpenCheck (format {}); please update",
            manifest.format_version
        )));
    }
    let mut read_text = |paths: &[String]| -> Result<Vec<RulesetFile>, ProjectError> {
        let mut files = Vec::new();
        for path in paths {
            let mut text = String::new();
            archive.by_name(path)?.read_to_string(&mut text)?;
            files.push(RulesetFile {
                path: path.clone(),
                text,
            });
        }
        Ok(files)
    };
    let rulesets = read_text(&manifest.rulesets)?;
    let requirements = read_text(&manifest.requirements)?;
    let documents = ProjectDocuments {
        view: read_json(&mut archive, VIEW)?,
        checking: read_json(&mut archive, CHECKING)?,
        issues: read_json(&mut archive, ISSUES)?,
    };
    Ok(ProjectContents {
        manifest,
        rulesets,
        requirements,
        documents,
    })
}

fn read_json<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
) -> Result<Option<serde_json::Value>, ProjectError> {
    match archive.by_name(name) {
        Ok(file) => Ok(Some(serde_json::from_reader(file)?)),
        Err(zip::result::ZipError::FileNotFound) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reads the bytes of one IFC file from a project.
pub fn read_project_model(path: &Path, entry: &str) -> Result<Vec<u8>, ProjectError> {
    read_entry(ZipArchive::new(BufReader::new(File::open(path)?))?, entry)
}

/// [`read_project_model`] for a project held in memory.
pub fn read_project_model_bytes(project: &[u8], entry: &str) -> Result<Vec<u8>, ProjectError> {
    read_entry(ZipArchive::new(Cursor::new(project))?, entry)
}

fn read_entry<R: Read + Seek>(
    mut archive: ZipArchive<R>,
    entry: &str,
) -> Result<Vec<u8>, ProjectError> {
    let mut file = archive.by_name(entry)?;
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Writes a project. The file is written next to `path` and moved into place
/// when complete, so a failed save never damages the previous version.
///
/// Returns the manifest that was written, with archive paths filled in.
pub fn save_project(
    path: &Path,
    models: &[ProjectModel],
    rulesets: &[RulesetFile],
    requirements: &[RulesetFile],
    documents: &ProjectDocuments,
    created: Option<&str>,
) -> Result<Manifest, ProjectError> {
    let temp = temp_path(path);
    let result = File::create(&temp)
        .map_err(ProjectError::from)
        .and_then(|file| {
            write_archive(
                BufWriter::new(file),
                models,
                rulesets,
                requirements,
                documents,
                created,
            )
        })
        .and_then(|(manifest, mut writer)| {
            writer.flush()?;
            Ok(manifest)
        });
    match result {
        Ok(manifest) => {
            std::fs::rename(&temp, path)?;
            Ok(manifest)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            Err(e)
        }
    }
}

/// [`save_project`] into memory: the bytes of the `.opencheck` file.
pub fn save_project_bytes(
    models: &[ProjectModel],
    rulesets: &[RulesetFile],
    requirements: &[RulesetFile],
    documents: &ProjectDocuments,
    created: Option<&str>,
) -> Result<(Vec<u8>, Manifest), ProjectError> {
    let (manifest, cursor) = write_archive(
        Cursor::new(Vec::new()),
        models,
        rulesets,
        requirements,
        documents,
        created,
    )?;
    Ok((cursor.into_inner(), manifest))
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".saving");
    path.with_file_name(name)
}

fn write_archive<W: Write + Seek>(
    target: W,
    models: &[ProjectModel],
    rulesets: &[RulesetFile],
    requirements: &[RulesetFile],
    documents: &ProjectDocuments,
    created: Option<&str>,
) -> Result<(Manifest, W), ProjectError> {
    let mut zip = ZipWriter::new(target);
    let now = now_rfc3339();
    let text_options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut used_names: Vec<String> = Vec::new();
    let mut entries = Vec::with_capacity(models.len());
    for model in models {
        let file = unique_entry_name(&model.entry.name, &mut used_names);
        match &model.source {
            ModelSource::Project { path, entry } => {
                // Copy the compressed bytes as they are.
                let mut archive = ZipArchive::new(BufReader::new(File::open(path)?))?;
                let source = archive.by_name(entry)?;
                zip.raw_copy_file_rename(source, &file)?;
            }
            ModelSource::File(path) => {
                let size = std::fs::metadata(path)?.len();
                let options = SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(3))
                    .large_file(size >= u32::MAX as u64);
                zip.start_file(&file, options)?;
                io::copy(&mut BufReader::new(File::open(path)?), &mut zip)?;
            }
            ModelSource::Memory { bytes, .. } => {
                let options = SimpleFileOptions::default()
                    .compression_method(CompressionMethod::Deflated)
                    .compression_level(Some(3))
                    .large_file(bytes.len() as u64 >= u32::MAX as u64);
                zip.start_file(&file, options)?;
                zip.write_all(bytes)?;
            }
        }
        entries.push(ModelEntry {
            file,
            ..model.entry.clone()
        });
    }

    for file in rulesets.iter().chain(requirements) {
        zip.start_file(&file.path, text_options)?;
        zip.write_all(file.text.as_bytes())?;
    }
    for (name, document) in [
        (VIEW, &documents.view),
        (CHECKING, &documents.checking),
        (ISSUES, &documents.issues),
    ] {
        if let Some(value) = document {
            zip.start_file(name, text_options)?;
            serde_json::to_writer_pretty(&mut zip, value)?;
        }
    }

    let manifest = Manifest {
        format: FORMAT_NAME.to_owned(),
        format_version: FORMAT_VERSION,
        generator: format!("OpenCheck {}", env!("CARGO_PKG_VERSION")),
        created: created.map_or_else(|| now.clone(), str::to_owned),
        modified: now,
        models: entries,
        rulesets: rulesets.iter().map(|r| r.path.clone()).collect(),
        requirements: requirements.iter().map(|r| r.path.clone()).collect(),
    };
    // The manifest goes last so a reader that finds it knows the archive is complete.
    zip.start_file(MANIFEST, text_options)?;
    serde_json::to_writer_pretty(&mut zip, &manifest)?;
    let writer = zip.finish()?;
    Ok((manifest, writer))
}

/// `models/<name>.ifc`, with characters that are awkward in archives replaced
/// and a number appended when two models share a name.
fn unique_entry_name(name: &str, used: &mut Vec<String>) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let clean = clean.trim().trim_matches('.');
    let base = if clean.is_empty() { "model" } else { clean };
    let mut candidate = format!("models/{base}.ifc");
    let mut n = 2;
    while used.iter().any(|u| u.eq_ignore_ascii_case(&candidate)) {
        candidate = format!("models/{base} ({n}).ifc");
        n += 1;
    }
    used.push(candidate.clone());
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    const IFC: &[u8] = b"ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1=IFCPROJECT('0',$,'P',$,$,$,$,$,$);\nENDSEC;\nEND-ISO-10303-21;\n";

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("opencheck-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(name: &str) -> ModelEntry {
        ModelEntry {
            file: String::new(),
            name: name.to_owned(),
            discipline: Discipline::Structural,
            visible: true,
            source_path: None,
            sha256: sha256_hex(IFC),
            revision: 1,
            added: now_rfc3339(),
            updated: now_rfc3339(),
        }
    }

    #[test]
    fn round_trip_and_resave_in_place() {
        let dir = temp_dir("roundtrip");
        let ifc = dir.join("K model.ifc");
        std::fs::write(&ifc, IFC).unwrap();
        let project = dir.join("test.opencheck");
        let ruleset = RulesetFile {
            path: "rulesets/clash.toml".into(),
            text: "name = \"x\"\n".into(),
        };
        let documents = ProjectDocuments {
            view: Some(serde_json::json!({ "xray": true })),
            checking: Some(serde_json::json!({ "decisions": [] })),
            issues: Some(serde_json::json!({ "issues": [] })),
        };

        let models = vec![
            ProjectModel {
                entry: entry("K/model"),
                source: ModelSource::File(ifc.clone()),
            },
            ProjectModel {
                entry: entry("K/model"),
                source: ModelSource::File(ifc),
            },
        ];
        let ids = RulesetFile {
            path: "requirements/delivery.ids".into(),
            text: "<ids/>".into(),
        };
        let written = save_project(
            &project,
            &models,
            std::slice::from_ref(&ruleset),
            std::slice::from_ref(&ids),
            &documents,
            None,
        )
        .unwrap();
        assert_eq!(written.models[0].file, "models/K_model.ifc");
        assert_eq!(written.models[1].file, "models/K_model (2).ifc");

        let read = read_project(&project).unwrap();
        assert_eq!(read.manifest.models, written.models);
        assert_eq!(read.rulesets, vec![ruleset.clone()]);
        assert_eq!(read.requirements, vec![ids]);
        assert_eq!(read.documents, documents);
        assert_eq!(read.manifest.models[0].discipline, Discipline::Structural);
        assert_eq!(
            read_project_model(&project, "models/K_model.ifc").unwrap(),
            IFC
        );

        // Save over the same file, copying the model from the project itself.
        let again = vec![ProjectModel {
            entry: read.manifest.models[1].clone(),
            source: ModelSource::Project {
                path: project.clone(),
                entry: "models/K_model (2).ifc".into(),
            },
        }];
        save_project(
            &project,
            &again,
            &[],
            &[],
            &ProjectDocuments::default(),
            Some(&read.manifest.created),
        )
        .unwrap();
        let reread = read_project(&project).unwrap();
        assert_eq!(reread.manifest.models.len(), 1);
        assert_eq!(reread.manifest.created, read.manifest.created);
        let entry = &reread.manifest.models[0].file;
        assert_eq!(read_project_model(&project, entry).unwrap(), IFC);
        assert_eq!(reread.documents, ProjectDocuments::default());
        assert!(reread.requirements.is_empty());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn round_trip_in_memory() {
        let models = vec![ProjectModel {
            entry: entry("ARK"),
            source: ModelSource::Memory {
                name: "ARK.ifc".into(),
                bytes: Arc::from(IFC),
            },
        }];
        let documents = ProjectDocuments {
            issues: Some(serde_json::json!({ "issues": [] })),
            ..Default::default()
        };
        let (bytes, written) = save_project_bytes(&models, &[], &[], &documents, None).unwrap();
        let read = read_project_bytes(&bytes).unwrap();
        assert_eq!(read.manifest.models, written.models);
        assert_eq!(read.documents, documents);
        assert_eq!(
            read_project_model_bytes(&bytes, &written.models[0].file).unwrap(),
            IFC
        );
    }

    #[test]
    fn rejects_other_zip_files() {
        let dir = temp_dir("reject");
        let path = dir.join("other.opencheck");
        let mut zip = ZipWriter::new(File::create(&path).unwrap());
        zip.start_file(MANIFEST, SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"format":"something-else","format_version":1,"generator":"","created":"","modified":"","models":[]}"#).unwrap();
        zip.finish().unwrap();
        assert!(matches!(read_project(&path), Err(ProjectError::Format(_))));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
