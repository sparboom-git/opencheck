//! The user's ruleset library: TOML files in a folder of their own, shared
//! by all projects (`%APPDATA%\OpenCheck\rulesets` on Windows).

use std::path::{Path, PathBuf};

use opencheck_core::check::Ruleset;

pub struct LibraryRuleset {
    pub path: PathBuf,
    pub ruleset: Result<Ruleset, String>,
}

pub fn folder() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| Path::new(&h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("OpenCheck").join("rulesets")
}

/// All rulesets in the library folder, sorted by name. Broken files are
/// listed too, with their error, so they can be fixed.
pub fn list() -> Vec<LibraryRuleset> {
    let Ok(entries) = std::fs::read_dir(folder()) else {
        return Vec::new();
    };
    let mut list: Vec<LibraryRuleset> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        })
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let ruleset = Ruleset::from_toml(&text).map_err(|e| e.to_string());
            Some(LibraryRuleset { path, ruleset })
        })
        .collect();
    list.sort_by_key(|r| {
        r.ruleset
            .as_ref()
            .map(|r| r.ruleset.name.to_lowercase())
            .unwrap_or_default()
    });
    list
}

/// Writes a ruleset into the library, replacing one with the same id.
pub fn save(ruleset: &Ruleset) -> std::io::Result<PathBuf> {
    let dir = folder();
    std::fs::create_dir_all(&dir)?;
    let existing = list().into_iter().find(|r| {
        r.ruleset
            .as_ref()
            .is_ok_and(|x| x.ruleset.id == ruleset.ruleset.id)
    });
    let path = existing.map(|r| r.path).unwrap_or_else(|| {
        let stem: String = ruleset
            .ruleset
            .id
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        dir.join(format!("{stem}.toml"))
    });
    std::fs::write(&path, ruleset.to_toml())?;
    Ok(path)
}

/// Copies ruleset files into the library after checking that they parse.
/// Returns how many were imported and a message per file that was not.
pub fn import(paths: &[PathBuf]) -> (usize, Vec<String>) {
    let mut imported = 0;
    let mut errors = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Ruleset::from_toml(&text).map_err(|e| e.to_string()))
            .and_then(|ruleset| save(&ruleset).map_err(|e| e.to_string()));
        match result {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("{name} was not imported: {}", first_line(&e))),
        }
    }
    (imported, errors)
}

/// TOML errors span several lines with a source excerpt; the first says where.
fn first_line(message: &str) -> &str {
    message
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(message)
}

pub fn delete(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)
}
