//! The disk scan (plan §3.2): finds the `.mp4` files in the library folders,
//! reads their `©cmt` tag (`ruvinfo:{pid}:{sid}`, written by ruvsarpur and by
//! sarpur's ffmpeg metadata) and keeps the `library` table in step with it.
//!
//! Tags are only read for new or changed files: each file's last result is
//! cached in `scan_cache` by `(path, size, mtime)`. File system work runs on
//! the blocking pool, and the database is only held for the short reads and
//! the final write.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};

use serde::Serialize;

use crate::db::{Db, rusqlite};
use crate::library;
use crate::settings;

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub running: bool,
    pub last_scan_at: Option<String>,
    /// The folders scanned (the library's category folders that exist).
    pub roots: Vec<String>,
    /// `.mp4` files found.
    pub files: usize,
    /// Files with a `ruvinfo` tag.
    pub tagged: usize,
    /// Files whose tag was read this time (new or changed).
    pub read: usize,
    /// `library` rows removed because their file is gone or lost its tag.
    pub removed: usize,
    /// Folders that couldn't be listed plus files whose tag couldn't be read.
    pub errors: usize,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

/// One `.mp4` found on disk.
#[derive(Debug, Clone)]
struct Found {
    path: String,
    size: i64,
    mtime: i64,
}

/// What the tag says (or why it couldn't be read).
#[derive(Debug, Clone, Default, PartialEq)]
struct TagInfo {
    pid: Option<String>,
    sid: Option<String>,
    error: Option<String>,
}

/// Parses `ruvinfo:{pid}:{sid}`.
pub fn parse_ruvinfo(comment: &str) -> Option<(String, String)> {
    let mut parts = comment.trim().split(':');
    if parts.next()? != "ruvinfo" {
        return None;
    }
    let pid = parts.next()?.trim();
    let sid = parts.next()?.trim();
    (!pid.is_empty()).then(|| (pid.to_string(), sid.to_string()))
}

fn read_tag(path: &Path) -> TagInfo {
    match mp4ameta::Tag::read_from_path(path) {
        Ok(tag) => match tag.comment().and_then(parse_ruvinfo) {
            Some((pid, sid)) => TagInfo {
                pid: Some(pid),
                sid: Some(sid),
                error: None,
            },
            None => TagInfo::default(),
        },
        Err(e) => TagInfo {
            error: Some(e.to_string()),
            ..TagInfo::default()
        },
    }
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Lists every `*.mp4` under `dir` (symlinks are not followed).
fn walk(dir: &Path, found: &mut Vec<Found>, errors: &mut usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!("scan: could not list {}: {e}", dir.display());
            *errors += 1;
            return;
        }
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, found, errors),
            Ok(t) if t.is_file() => {
                let is_mp4 = path
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("mp4"));
                if !is_mp4 {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    found.push(Found {
                        path: path.to_string_lossy().into_owned(),
                        size: meta.len() as i64,
                        mtime: mtime_secs(&meta),
                    });
                }
            }
            _ => {}
        }
    }
}

/// The library's category folders, without duplicates or folders nested in
/// another one.
pub fn roots(settings: &settings::Settings, data_dir: &Path) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = ["show", "movie_or_docu", "sport"]
        .iter()
        .map(|t| library::output_dir(settings, t, data_dir))
        .collect();
    roots.sort();
    roots.dedup();
    let all = roots.clone();
    roots.retain(|r| !all.iter().any(|other| other != r && r.starts_with(other)));
    roots
}

fn under_any(path: &str, roots: &[PathBuf]) -> bool {
    let path = Path::new(path);
    roots.iter().any(|r| path.starts_with(r))
}

pub struct Scanner {
    db: Db,
    data_dir: PathBuf,
    lock: tokio::sync::Mutex<()>,
    status: Mutex<ScanStatus>,
}

impl Scanner {
    pub fn new(db: Db, data_dir: PathBuf) -> Scanner {
        Scanner {
            db,
            data_dir,
            lock: tokio::sync::Mutex::new(()),
            status: Mutex::new(ScanStatus::default()),
        }
    }

    pub fn status(&self) -> ScanStatus {
        self.status.lock().unwrap().clone()
    }

    /// Scans the library folders. A scan that is already running is waited
    /// for, and then this one runs (a warm scan is cheap).
    pub async fn scan(&self) -> Result<ScanStatus, String> {
        let _guard = self.lock.lock().await;
        self.status.lock().unwrap().running = true;
        let start = Instant::now();
        let result = self.scan_inner().await;
        let mut status = self.status.lock().unwrap();
        match result {
            Ok(mut done) => {
                done.elapsed_ms = start.elapsed().as_millis() as u64;
                done.last_scan_at = Some(crate::now_iso());
                tracing::info!(
                    "scan: {} files ({} tagged, {} read, {} removed, {} errors) in {} ms",
                    done.files,
                    done.tagged,
                    done.read,
                    done.removed,
                    done.errors,
                    done.elapsed_ms
                );
                *status = done;
                Ok(status.clone())
            }
            Err(e) => {
                tracing::warn!("scan failed: {e}");
                status.running = false;
                status.error = Some(e.clone());
                Err(e)
            }
        }
    }

    async fn scan_inner(&self) -> Result<ScanStatus, String> {
        let settings = settings::load(&self.db, &self.data_dir)
            .await
            .map_err(|e| e.to_string())?;
        let roots = roots(&settings, &self.data_dir);
        let walk_roots = roots.clone();
        let (found, walk_errors, existing_roots) = tokio::task::spawn_blocking(move || {
            let mut found = Vec::new();
            let mut errors = 0;
            let mut existing = Vec::new();
            for root in &walk_roots {
                // A category folder that doesn't exist yet is just empty.
                if root.is_dir() {
                    existing.push(root.clone());
                    walk(root, &mut found, &mut errors);
                }
            }
            (found, errors, existing)
        })
        .await
        .map_err(|e| e.to_string())?;

        let cache: HashMap<String, (i64, i64, TagInfo)> = self
            .db
            .call(|c| {
                c.prepare("SELECT path, size, mtime, pid, sid, error FROM scan_cache")?
                    .query_map([], |r| {
                        Ok((
                            r.get(0)?,
                            (
                                r.get(1)?,
                                r.get(2)?,
                                TagInfo {
                                    pid: r.get(3)?,
                                    sid: r.get(4)?,
                                    error: r.get(5)?,
                                },
                            ),
                        ))
                    })?
                    .collect()
            })
            .await
            .map_err(|e| e.to_string())?;

        let changed: Vec<Found> = found
            .iter()
            .filter(|f| {
                cache
                    .get(&f.path)
                    .is_none_or(|(size, mtime, _)| (*size, *mtime) != (f.size, f.mtime))
            })
            .cloned()
            .collect();
        let read = tokio::task::spawn_blocking(move || {
            changed
                .into_iter()
                .map(|f| {
                    let tag = read_tag(Path::new(&f.path));
                    (f, tag)
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| e.to_string())?;

        let mut tags: HashMap<String, TagInfo> = found
            .iter()
            .filter_map(|f| {
                cache
                    .get(&f.path)
                    .map(|(_, _, t)| (f.path.clone(), t.clone()))
            })
            .collect();
        for (f, tag) in &read {
            tags.insert(f.path.clone(), tag.clone());
        }
        let status = ScanStatus {
            running: false,
            roots: roots
                .iter()
                .map(|r| r.to_string_lossy().into_owned())
                .collect(),
            files: found.len(),
            tagged: tags.values().filter(|t| t.pid.is_some()).count(),
            read: read.len(),
            errors: walk_errors + tags.values().filter(|t| t.error.is_some()).count(),
            ..ScanStatus::default()
        };

        let now = crate::now_iso();
        let removed = self
            .db
            .call(move |c| write_results(c, &found, &read, &tags, &existing_roots, &now))
            .await
            .map_err(|e| e.to_string())?;
        Ok(ScanStatus { removed, ..status })
    }
}

/// Stores the scan's results in one transaction. Rows under a scanned folder
/// whose file is gone are dropped from both tables, as are `tag` rows whose
/// file lost its tag. Returns how many `library` rows were removed.
fn write_results(
    c: &mut rusqlite::Connection,
    found: &[Found],
    read: &[(Found, TagInfo)],
    tags: &HashMap<String, TagInfo>,
    roots: &[PathBuf],
    now: &str,
) -> rusqlite::Result<usize> {
    let tx = c.transaction()?;
    {
        let mut upsert_cache = tx.prepare(
            "INSERT INTO scan_cache (path, size, mtime, pid, sid, error) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (path) DO UPDATE SET size = excluded.size, mtime = excluded.mtime,
                 pid = excluded.pid, sid = excluded.sid, error = excluded.error",
        )?;
        for (f, tag) in read {
            upsert_cache.execute(rusqlite::params![
                f.path, f.size, f.mtime, tag.pid, tag.sid, tag.error
            ])?;
        }
        let mut upsert_library = tx.prepare(
            "INSERT INTO library (path, pid, size, mtime, source, verified_at)
             VALUES (?1, ?2, ?3, ?4, 'tag', ?5)
             ON CONFLICT (path) DO UPDATE SET pid = excluded.pid, size = excluded.size,
                 mtime = excluded.mtime, source = 'tag', verified_at = excluded.verified_at",
        )?;
        for f in found {
            if let Some(pid) = tags.get(&f.path).and_then(|t| t.pid.as_ref()) {
                upsert_library.execute(rusqlite::params![f.path, pid, f.size, f.mtime, now])?;
            }
        }
    }

    let present: HashSet<&str> = found.iter().map(|f| f.path.as_str()).collect();
    let cached: Vec<String> = tx
        .prepare("SELECT path FROM scan_cache")?
        .query_map([], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    for path in cached {
        if under_any(&path, roots) && !present.contains(path.as_str()) {
            tx.execute("DELETE FROM scan_cache WHERE path = ?1", [&path])?;
        }
    }
    let rows: Vec<(String, String)> = tx
        .prepare("SELECT path, source FROM library")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut removed = 0;
    for (path, source) in rows {
        let gone = under_any(&path, roots) && !present.contains(path.as_str());
        let untagged = source == "tag"
            && present.contains(path.as_str())
            && tags
                .get(&path)
                .is_some_and(|t| t.pid.is_none() && t.error.is_none());
        if gone || untagged {
            removed += tx.execute("DELETE FROM library WHERE path = ?1", [&path])?;
        }
    }
    tx.commit()?;
    Ok(removed)
}

#[cfg(test)]
pub(crate) mod tests;
