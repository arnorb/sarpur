//! Import of ruvsarpur's data (plan §3.4) from its folder: the data folder
//! of the old Docker app, `~/.ruvsarpur`, or the `--portable` folder next to
//! the old script on Windows.
//!
//! - `gui-settings.json`: settings, with paths mapped (`/data/...` becomes
//!   `<data>/...`, everything else is kept) and the old `outputDir` →
//!   `libraryRootDir` migration; `watchlistSids` become follows.
//! - `prevrecorded.log` (bare pids): `recorded` rows. The origin comes from
//!   the event log's `download_request` lines: `library` if a pid was ever
//!   requested for the library, `web` if only for the browser, else
//!   `unknown` (automatic runs only logged sids).
//! - `tvschedule.json` and `imdb-cache.json` are copied when the data folder
//!   has none, which saves the first slow catalogue fetch.
//!
//! [`preview`] shows all of it, the path mapping included, without changing
//! anything; [`apply`] does it. [`report`] lists what's recorded but missing
//! on disk and what's on disk but not recorded.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::catalogue::Catalogue;
use crate::db::{Db, DbError, rusqlite};
use crate::eventlog::{self, EventLog};
use crate::follows::Follows;
use crate::scan::Scanner;
use crate::settings;

const SETTINGS_FILE: &str = "gui-settings.json";
const RECORDED_FILE: &str = "prevrecorded.log";
const COPIED_FILES: [&str; 2] = ["tvschedule.json", "imdb-cache.json"];
/// Settings that hold a folder on the machine running the app.
const PATH_KEYS: [&str; 4] = [
    "libraryRootDir",
    "showsSubdir",
    "moviesSubdir",
    "sportsSubdir",
];
/// Settings copied as they are.
const PLAIN_KEYS: [&str; 6] = [
    "autoEnabled",
    "autoIntervalMinutes",
    "plexBaseUrl",
    "plexToken",
    "plexLibrarySectionId",
    "plexLibraryPath",
];

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Db(#[from] DbError),
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PathMapping {
    pub key: String,
    pub from: String,
    pub to: String,
    pub changed: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecordedCounts {
    pub total: usize,
    pub library: usize,
    pub web: usize,
    pub unknown: usize,
    /// Already in sarpur's `recorded`; these are left as they are.
    pub already_known: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub source: String,
    pub settings_file: bool,
    pub recorded_file: bool,
    /// Event log files read (the log and its rotated backups).
    pub event_logs: usize,
    /// The settings that will be set (passwords and tokens masked).
    pub settings: Map<String, Value>,
    /// Every folder setting: what it was and what it becomes.
    pub path_mapping: Vec<PathMapping>,
    pub follow_sids: Vec<String>,
    pub recorded: RecordedCounts,
    /// Files copied into the data folder.
    pub copies: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub preview: Preview,
    pub recorded_added: usize,
    pub follows_added: Vec<String>,
    pub warnings: Vec<String>,
}

/// Maps a folder from the old app to this one: the old Docker app's `/data`
/// is the data folder here; anything else (`/media/...`, Windows paths) is
/// kept, because the files stay where they are.
pub fn map_path(path: &str, data_dir: &Path) -> String {
    let path = path.trim();
    if path == "/data" {
        return data_dir.to_string_lossy().into_owned();
    }
    match path.strip_prefix("/data/") {
        Some(rest) => data_dir.join(rest).to_string_lossy().into_owned(),
        None => path.to_string(),
    }
}

/// Folders worth offering for import: the data folder, `~/.ruvsarpur`, and
/// the folder of the running program and its parent (where a portable old
/// install may sit). Only those holding ruvsarpur's files are returned.
pub fn candidates(data_dir: &Path) -> Vec<String> {
    let mut dirs = vec![data_dir.to_path_buf()];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".ruvsarpur"));
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        dirs.extend(exe_dir.parent().map(Path::to_path_buf));
        dirs.push(exe_dir);
    }
    let mut seen = HashSet::new();
    dirs.into_iter()
        .filter(|d| d.join(SETTINGS_FILE).is_file() || d.join(RECORDED_FILE).is_file())
        .map(|d| d.to_string_lossy().into_owned())
        .filter(|d| seen.insert(d.clone()))
        .collect()
}

/// What was read from the source folder.
struct Source {
    preview: Preview,
    /// The settings patch for [`settings::update`], unmasked.
    patch: Map<String, Value>,
    /// pid → origin.
    recorded: BTreeMap<String, &'static str>,
}

/// `pid → modes` from `download_request` lines in the event logs.
fn request_modes(
    dir: &Path,
    warnings: &mut Vec<String>,
) -> (HashMap<String, HashSet<String>>, usize) {
    let mut modes: HashMap<String, HashSet<String>> = HashMap::new();
    let mut files = 0;
    let base = dir.join(eventlog::FILE_NAME);
    let mut paths = vec![base.clone()];
    paths.extend((1..=5).map(|n| PathBuf::from(format!("{}.{n}", base.display()))));
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        files += 1;
        let mut bad = 0;
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(value) = serde_json::from_str::<Value>(line) else {
                bad += 1;
                continue;
            };
            if value["event"] != "download_request" {
                continue;
            }
            let details = &value["details"];
            if let (Some(pid), Some(mode)) = (details["pid"].as_str(), details["mode"].as_str()) {
                modes
                    .entry(pid.trim().to_string())
                    .or_default()
                    .insert(mode.to_string());
            }
        }
        if bad > 0 {
            warnings.push(format!(
                "{}: skipped {bad} unreadable line(s)",
                path.display()
            ));
        }
    }
    (modes, files)
}

fn read_source(dir: &Path, data_dir: &Path) -> Result<Source, ImportError> {
    if !dir.is_dir() {
        return Err(ImportError::Invalid(format!(
            "{} is not a folder",
            dir.display()
        )));
    }
    let mut preview = Preview {
        source: dir.to_string_lossy().into_owned(),
        ..Preview::default()
    };
    let mut patch = Map::new();

    let settings_path = dir.join(SETTINGS_FILE);
    if let Ok(text) = std::fs::read_to_string(&settings_path) {
        preview.settings_file = true;
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(old)) => {
                let text_of = |key: &str| {
                    old.get(key)
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or("")
                        .to_string()
                };
                for key in PATH_KEYS {
                    let mut from = text_of(key);
                    // gui_api: keep the single-folder setup working.
                    if key == "libraryRootDir" && from.is_empty() {
                        from = text_of("outputDir");
                    }
                    if from.is_empty() && !old.contains_key(key) {
                        continue;
                    }
                    let to = map_path(&from, data_dir);
                    preview.path_mapping.push(PathMapping {
                        key: key.into(),
                        changed: to != from,
                        from,
                        to: to.clone(),
                    });
                    patch.insert(key.into(), Value::String(to));
                }
                for key in PLAIN_KEYS {
                    if let Some(value) = old.get(key) {
                        let value = match (key, value) {
                            ("autoIntervalMinutes", v) => json!(
                                v.as_f64()
                                    .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
                                    .map(|n| n.max(5.0) as u64)
                                    .unwrap_or(60)
                            ),
                            ("autoEnabled", v) => json!(v.as_bool().unwrap_or(false)),
                            (_, v) => json!(v.as_str().map(str::trim).unwrap_or_default()),
                        };
                        patch.insert(key.into(), value);
                    }
                }
                preview.follow_sids = old
                    .get("watchlistSids")
                    .and_then(Value::as_array)
                    .map(|sids| {
                        let mut seen = HashSet::new();
                        sids.iter()
                            .filter_map(|s| match s {
                                Value::String(s) => Some(s.trim().to_string()),
                                Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                            .filter(|s| !s.is_empty() && seen.insert(s.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
            }
            _ => preview.warnings.push(format!(
                "{} isn't a JSON object; settings skipped",
                settings_path.display()
            )),
        }
    }
    preview.settings = patch
        .iter()
        .map(|(k, v)| {
            let secret =
                k.to_lowercase().contains("token") && v.as_str().is_some_and(|s| !s.is_empty());
            (
                k.clone(),
                if secret {
                    json!("••••")
                } else {
                    v.clone()
                },
            )
        })
        .collect();

    let (modes, event_logs) = request_modes(dir, &mut preview.warnings);
    preview.event_logs = event_logs;
    let mut recorded = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(dir.join(RECORDED_FILE)) {
        preview.recorded_file = true;
        for pid in text.lines().map(str::trim).filter(|p| !p.is_empty()) {
            let origin = match modes.get(pid) {
                Some(m) if m.contains("library") => "library",
                Some(m) if m.contains("web") => "web",
                _ => "unknown",
            };
            recorded.insert(pid.to_string(), origin);
        }
    }
    preview.recorded.total = recorded.len();
    for origin in recorded.values() {
        match *origin {
            "library" => preview.recorded.library += 1,
            "web" => preview.recorded.web += 1,
            _ => preview.recorded.unknown += 1,
        }
    }

    let same_folder = std::fs::canonicalize(dir).ok() == std::fs::canonicalize(data_dir).ok();
    if !same_folder {
        preview.copies = COPIED_FILES
            .iter()
            .filter(|f| dir.join(f).is_file() && !data_dir.join(f).exists())
            .map(|f| f.to_string())
            .collect();
    }
    if !preview.settings_file && !preview.recorded_file {
        return Err(ImportError::Invalid(format!(
            "{} has neither {SETTINGS_FILE} nor {RECORDED_FILE}",
            dir.display()
        )));
    }
    Ok(Source {
        preview,
        patch,
        recorded,
    })
}

async fn read(dir: &Path, data_dir: &Path, db: &Db) -> Result<Source, ImportError> {
    let (dir, data) = (dir.to_path_buf(), data_dir.to_path_buf());
    let mut source = tokio::task::spawn_blocking(move || read_source(&dir, &data))
        .await
        .map_err(|e| ImportError::Invalid(e.to_string()))??;
    let pids: Vec<String> = source.recorded.keys().cloned().collect();
    source.preview.recorded.already_known = db
        .call(move |c| {
            let mut stmt = c.prepare("SELECT 1 FROM recorded WHERE pid = ?1")?;
            let mut n = 0;
            for pid in &pids {
                n += usize::from(stmt.exists([pid])?);
            }
            Ok(n)
        })
        .await?;
    Ok(source)
}

/// What an import of `dir` would do. Changes nothing.
pub async fn preview(dir: &Path, data_dir: &Path, db: &Db) -> Result<Preview, ImportError> {
    Ok(read(dir, data_dir, db).await?.preview)
}

pub struct Services<'a> {
    pub db: &'a Db,
    pub data_dir: &'a Path,
    pub follows: &'a Follows,
    pub catalogue: &'a Catalogue,
    pub ruv: &'a crate::ruv::Ruv,
    pub log: &'a EventLog,
}

/// Imports `dir`: settings, recorded pids (existing rows are kept), follows,
/// and the catalogue files.
pub async fn apply(dir: &Path, svc: Services<'_>) -> Result<Applied, ImportError> {
    let source = read(dir, svc.data_dir, svc.db).await?;
    let mut applied = Applied {
        preview: source.preview.clone(),
        ..Applied::default()
    };

    for file in &source.preview.copies {
        if let Err(e) = tokio::fs::copy(dir.join(file), svc.data_dir.join(file)).await {
            applied.warnings.push(format!("Could not copy {file}: {e}"));
        }
    }
    if source.preview.copies.iter().any(|f| f == "tvschedule.json") {
        svc.catalogue.wait_idle().await;
        svc.catalogue.load(svc.ruv, false).await;
    }

    if !source.patch.is_empty() {
        settings::update(svc.db, svc.data_dir, &Value::Object(source.patch.clone()))
            .await
            .map_err(|e| ImportError::Invalid(format!("settings: {e}")))?;
    }

    let rows: Vec<(String, &'static str)> = source.recorded.into_iter().collect();
    applied.recorded_added = svc
        .db
        .call(move |c| {
            let tx = c.transaction()?;
            let now = crate::now_iso();
            let mut added = 0;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO recorded (pid, origin, recorded_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT (pid) DO NOTHING",
                )?;
                for (pid, origin) in &rows {
                    added += insert.execute(rusqlite::params![pid, origin, now])?;
                }
            }
            tx.commit()?;
            Ok(added)
        })
        .await?;

    for sid in &source.preview.follow_sids {
        match svc.follows.follow_sid(sid).await {
            Ok(_) => applied.follows_added.push(sid.clone()),
            Err(e) => applied
                .warnings
                .push(format!("Could not follow series {sid}: {e}")),
        }
    }

    svc.log
        .log(
            "import",
            if applied.warnings.is_empty() {
                "succeeded"
            } else {
                "warning"
            },
            json!({
                "source": applied.preview.source,
                "recordedAdded": applied.recorded_added,
                "followsAdded": applied.follows_added,
                "pathMapping": applied.preview.path_mapping,
                "copies": applied.preview.copies,
                "warnings": applied.warnings,
            }),
        )
        .await;
    tracing::info!(
        "imported {}: {} recorded pid(s), {} follow(s)",
        applied.preview.source,
        applied.recorded_added,
        applied.follows_added.len()
    );
    Ok(applied)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MissingEpisode {
    pub pid: String,
    pub origin: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UntrackedFile {
    pub pid: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Recorded for the library (or unknown) with no file on disk: probably
    /// deleted on purpose. Follow runs skip these until "Allow re-download".
    pub recorded_but_missing: Vec<MissingEpisode>,
    /// Tagged files in the library that aren't recorded.
    pub on_disk_not_recorded: Vec<UntrackedFile>,
    pub scan_error: Option<String>,
}

/// `(pid, origin)` or `(pid, path)` rows.
type Pairs = Vec<(String, String)>;

/// Scans the library, then compares it with `recorded` (§3.4's report).
pub async fn report(db: &Db, scanner: &Scanner, catalogue: &Catalogue) -> Result<Report, DbError> {
    let scan_error = scanner.scan().await.err();
    let (recorded, library): (Pairs, Pairs) = db
        .call(|c| {
            let recorded = c
                .prepare(
                    "SELECT pid, origin FROM recorded
                     WHERE origin IN ('library', 'unknown') AND forgotten_at IS NULL ORDER BY pid",
                )?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            let library = c
                .prepare("SELECT pid, path FROM library ORDER BY path")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok((recorded, library))
        })
        .await?;
    let library = tokio::task::spawn_blocking(move || {
        library
            .into_iter()
            .filter(|(_, path)| Path::new(path).is_file())
            .collect::<Vec<_>>()
    })
    .await?;
    let on_disk: HashSet<&str> = library.iter().map(|(pid, _)| pid.as_str()).collect();
    let recorded_pids: HashSet<&str> = recorded.iter().map(|(pid, _)| pid.as_str()).collect();
    Ok(Report {
        recorded_but_missing: recorded
            .iter()
            .filter(|(pid, _)| !on_disk.contains(pid.as_str()))
            .map(|(pid, origin)| MissingEpisode {
                pid: pid.clone(),
                origin: origin.clone(),
                title: catalogue.get(pid).map(|i| i.title),
            })
            .collect(),
        on_disk_not_recorded: library
            .iter()
            .filter(|(pid, _)| !recorded_pids.contains(pid.as_str()))
            .map(|(pid, path)| UntrackedFile {
                pid: pid.clone(),
                path: path.clone(),
            })
            .collect(),
        scan_error,
    })
}

#[cfg(test)]
mod tests;
