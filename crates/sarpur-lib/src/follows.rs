//! Follows (plan §3.5) and follow runs.
//!
//! A follow is one show: the series ids the user chose, plus ids added during
//! a run because their strict title key and English flag match. A run
//! refreshes the catalogue, adds those ids, and queues a scheduled library job
//! for every episode that isn't already on disk or recorded (plan §3.3).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::catalogue::{Catalogue, CatalogueItem};
use crate::db::{Db, DbError, rusqlite};
use crate::jobs::{Event, JobError, Mode, Priority, Queue};
use crate::library;
use crate::scan::Scanner;
use crate::settings;
use crate::worker::{NameMode, WorkerClient};

/// ruvsarpur's `trimSeasonNumberSuffix`: drops the first of these suffixes
/// that the title ends with, checked in this order.
fn trim_season_number_suffix(title: &str) -> &str {
    const SUFFIXES: [&str; 24] = [
        " I", " II", " III", " IV", " V", " VI", " VII", " VIII", " IX", " X", " XI", " XII", " 1",
        " 2", " 3", " 4", " 5", " 6", " 7", " 8", " 9", " 10", " 11", " 12",
    ];
    SUFFIXES
        .iter()
        .find_map(|suffix| title.strip_suffix(suffix))
        .unwrap_or(title)
}

/// The strict follow key: `NFC(lower(trimSeasonNumberSuffix(title.strip())))`.
/// It keeps parentheses, so "(með táknmálstúlkun)" stays a different show.
pub fn title_key(series_title: &str) -> String {
    trim_season_number_suffix(series_title.trim())
        .to_lowercase()
        .nfc()
        .collect()
}

/// The label the UI shows: trailing "(...)" groups removed, as
/// `normalizeSeriesLabel`. Only for display; never a key.
pub fn series_label(series_title: &str) -> String {
    let mut title = series_title.trim();
    while let Some(open) = title.ends_with(')').then(|| title.rfind('(')).flatten() {
        if title[open + 1..title.len() - 1].contains(')') {
            break;
        }
        let shorter = title[..open].trim_end();
        if shorter.is_empty() {
            break;
        }
        title = shorter;
    }
    title.to_string()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FollowSid {
    pub sid: String,
    pub series_title: String,
    /// `user` or `auto`.
    pub added_by: String,
    pub added_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Follow {
    pub id: i64,
    pub title_key: String,
    pub label: String,
    pub english_subtitled: bool,
    pub created_at: String,
    /// The sids that are followed (removed ones are left out).
    pub sids: Vec<FollowSid>,
}

#[derive(Debug, thiserror::Error)]
pub enum FollowError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("{0}")]
    NotFound(String),
    #[error("a follow run is already running")]
    AlreadyRunning,
}

fn list_follows(c: &rusqlite::Connection) -> rusqlite::Result<Vec<Follow>> {
    let mut follows: Vec<Follow> = c
        .prepare(
            "SELECT id, title_key, label, english_flag, created_at FROM follows ORDER BY label, id",
        )?
        .query_map([], |r| {
            Ok(Follow {
                id: r.get(0)?,
                title_key: r.get(1)?,
                label: r.get(2)?,
                english_subtitled: r.get(3)?,
                created_at: r.get(4)?,
                sids: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;
    let mut stmt = c.prepare(
        "SELECT follow_id, sid, series_title, added_by, added_at FROM follow_sids
         WHERE removed_at IS NULL ORDER BY added_at, sid",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            FollowSid {
                sid: r.get(1)?,
                series_title: r.get(2)?,
                added_by: r.get(3)?,
                added_at: r.get(4)?,
            },
        ))
    })?;
    for row in rows {
        let (follow_id, sid) = row?;
        if let Some(follow) = follows.iter_mut().find(|f| f.id == follow_id) {
            follow.sids.push(sid);
        }
    }
    Ok(follows)
}

/// Adds `sid` to the follow for its key and flag, creating the follow if
/// needed. A removed sid is followed again. Returns the follow id.
fn insert_sid(
    c: &rusqlite::Connection,
    sid: &str,
    series_title: &str,
    english: bool,
    added_by: &str,
) -> rusqlite::Result<i64> {
    let key = title_key(series_title);
    let now = crate::now_iso();
    c.execute(
        "INSERT INTO follows (title_key, english_flag, label, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (title_key, english_flag) DO NOTHING",
        rusqlite::params![key, english, series_label(series_title), now],
    )?;
    let follow_id: i64 = c.query_row(
        "SELECT id FROM follows WHERE title_key = ?1 AND english_flag = ?2",
        rusqlite::params![key, english],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO follow_sids (follow_id, sid, series_title, english_flag, added_by, added_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (follow_id, sid) DO UPDATE SET removed_at = NULL,
             added_by = CASE WHEN excluded.added_by = 'user' THEN 'user' ELSE added_by END",
        rusqlite::params![follow_id, sid, series_title, english, added_by, now],
    )?;
    Ok(follow_id)
}

pub async fn list(db: &Db) -> Result<Vec<Follow>, DbError> {
    db.call(|c| list_follows(c)).await
}

/// Follows `sid` for the user.
pub async fn add(db: &Db, sid: &str, series_title: &str, english: bool) -> Result<Follow, DbError> {
    let (sid, title) = (sid.to_string(), series_title.to_string());
    db.call(move |c| {
        let tx = c.transaction()?;
        let id = insert_sid(&tx, &sid, &title, english, "user")?;
        tx.commit()?;
        Ok(list_follows(c)?
            .into_iter()
            .find(|f| f.id == id)
            .expect("inserted follow"))
    })
    .await
}

/// Stops following a show. Returns false for an unknown id.
pub async fn remove(db: &Db, id: i64) -> Result<bool, DbError> {
    db.call(move |c| Ok(c.execute("DELETE FROM follows WHERE id = ?1", [id])? > 0))
        .await
}

/// Stops following one sid of a show. It is kept as removed, so a run doesn't
/// add it back; a follow left with no sids is deleted. Returns the follow as
/// it is now (`None` if deleted), or `NotFound`.
pub async fn remove_sid(db: &Db, id: i64, sid: &str) -> Result<Option<Follow>, FollowError> {
    let sid = sid.to_string();
    let result = db
        .call(move |c| {
            let tx = c.transaction()?;
            let changed = tx.execute(
                "UPDATE follow_sids SET removed_at = ?3
                 WHERE follow_id = ?1 AND sid = ?2 AND removed_at IS NULL",
                rusqlite::params![id, sid, crate::now_iso()],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            let left: i64 = tx.query_row(
                "SELECT count(*) FROM follow_sids WHERE follow_id = ?1 AND removed_at IS NULL",
                [id],
                |r| r.get(0),
            )?;
            if left == 0 {
                tx.execute("DELETE FROM follows WHERE id = ?1", [id])?;
            }
            tx.commit()?;
            Ok(Some(list_follows(c)?.into_iter().find(|f| f.id == id)))
        })
        .await?;
    result.ok_or_else(|| FollowError::NotFound(format!("follow {id} has no sid")))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AddedSid {
    pub follow_id: i64,
    pub sid: String,
    pub series_title: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledEpisode {
    pub pid: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// (a) a library file for the pid is on disk.
    pub on_disk: usize,
    /// (b) a file matches the expected name as a prefix.
    pub name_match: usize,
    /// (c) recorded with origin `library` or `unknown`, not forgotten.
    pub recorded: usize,
    /// Already queued or running.
    pub in_queue: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    pub dry_run: bool,
    pub follows: usize,
    pub episodes: usize,
    pub added_sids: Vec<AddedSid>,
    pub scheduled: Vec<ScheduledEpisode>,
    pub skipped: Skipped,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub running: bool,
    pub last_run_at: Option<String>,
    /// `ok`, `warning` or `error`.
    pub last_run_state: Option<&'static str>,
    pub last_run_message: Option<String>,
    pub last_report: Option<RunReport>,
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    /// Fetch the catalogue from RÚV first.
    pub refresh: bool,
    /// Report what would be scheduled; add and queue nothing.
    pub dry_run: bool,
}

pub struct Follows {
    db: Db,
    worker: WorkerClient,
    catalogue: Arc<Catalogue>,
    queue: Queue,
    scanner: Arc<Scanner>,
    data_dir: PathBuf,
    status: Mutex<RunStatus>,
    run_lock: tokio::sync::Mutex<()>,
    last_started: Mutex<Option<Instant>>,
}

/// What the run found out about one batch of pids, from the database.
struct Known {
    recorded: HashSet<String>,
    library_paths: HashMap<String, Vec<String>>,
    in_queue: HashSet<String>,
}

fn known(c: &rusqlite::Connection, pids: &[String]) -> rusqlite::Result<Known> {
    let mut recorded = HashSet::new();
    let mut library_paths: HashMap<String, Vec<String>> = HashMap::new();
    let mut in_queue = HashSet::new();
    let mut rec = c.prepare(
        "SELECT 1 FROM recorded WHERE pid = ?1 AND origin IN ('library', 'unknown')
         AND forgotten_at IS NULL",
    )?;
    let mut lib = c.prepare("SELECT path FROM library WHERE pid = ?1")?;
    let mut job = c.prepare(
        "SELECT 1 FROM jobs WHERE pid = ?1 AND mode = 'library' AND state IN ('queued', 'running')",
    )?;
    for pid in pids {
        if rec.exists([pid])? {
            recorded.insert(pid.clone());
        }
        let paths = lib
            .query_map([pid], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if !paths.is_empty() {
            library_paths.insert(pid.clone(), paths);
        }
        if job.exists([pid])? {
            in_queue.insert(pid.clone());
        }
    }
    Ok(Known {
        recorded,
        library_paths,
        in_queue,
    })
}

impl Follows {
    pub fn new(
        db: Db,
        worker: WorkerClient,
        catalogue: Arc<Catalogue>,
        queue: Queue,
        scanner: Arc<Scanner>,
        data_dir: PathBuf,
    ) -> Follows {
        Follows {
            db,
            worker,
            catalogue,
            queue,
            scanner,
            data_dir,
            status: Mutex::new(RunStatus::default()),
            run_lock: tokio::sync::Mutex::new(()),
            last_started: Mutex::new(None),
        }
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn status(&self) -> RunStatus {
        self.status.lock().unwrap().clone()
    }

    fn set_status(&self, change: impl FnOnce(&mut RunStatus)) {
        let status = {
            let mut status = self.status.lock().unwrap();
            change(&mut status);
            status.clone()
        };
        self.queue.emit(Event::Follows {
            status: Box::new(status),
        });
    }

    /// Follows `sid` for the user, looking it up in the catalogue or, when it
    /// isn't there, on RÚV.
    pub async fn follow_sid(&self, sid: &str) -> Result<Follow, FollowError> {
        let sid = sid.trim();
        let info = match self.catalogue.series_index().remove(sid) {
            Some(info) => (info.series_title, info.english_subtitled),
            None => {
                let items = self
                    .worker
                    .series(sid)
                    .await
                    .map_err(|e| FollowError::NotFound(format!("series {sid}: {e}")))?;
                let item = items
                    .iter()
                    .find_map(CatalogueItem::from_worker)
                    .ok_or_else(|| FollowError::NotFound(format!("series {sid} not found")))?;
                let title = if item.series_title.trim().is_empty() {
                    item.title
                } else {
                    item.series_title
                };
                (title, item.english_subtitled)
            }
        };
        Ok(add(&self.db, sid, &info.0, info.1).await?)
    }

    /// One follow run. Fails only when another run is in progress; problems
    /// along the way end up in the status and the report's warnings.
    pub async fn run(&self, opts: RunOptions) -> Result<RunReport, FollowError> {
        let Ok(_guard) = self.run_lock.try_lock() else {
            return Err(FollowError::AlreadyRunning);
        };
        *self.last_started.lock().unwrap() = Some(Instant::now());
        self.set_status(|s| {
            s.running = true;
            s.last_run_at = Some(crate::now_iso());
        });
        let (state, message, report) = match self.run_inner(opts).await {
            Ok(report) => {
                let state = if report.warnings.is_empty() {
                    "ok"
                } else {
                    "warning"
                };
                let verb = if opts.dry_run {
                    "Would download"
                } else {
                    "Queued"
                };
                let message = format!(
                    "{verb} {} from {}",
                    count(report.scheduled.len(), "episode", "episodes"),
                    count(report.follows, "followed show", "followed shows")
                );
                (state, message, Some(report))
            }
            Err(RunProblem::Warning(message)) => ("warning", message, None),
            Err(RunProblem::Error(message)) => ("error", message, None),
        };
        match state {
            "ok" => tracing::info!("follow run: {message}"),
            _ => tracing::warn!("follow run: {message}"),
        }
        self.set_status(|s| {
            s.running = false;
            s.last_run_state = Some(state);
            s.last_run_message = Some(message);
            s.last_report = report.clone();
        });
        Ok(report.unwrap_or_else(|| RunReport {
            dry_run: opts.dry_run,
            ..RunReport::default()
        }))
    }

    async fn run_inner(&self, opts: RunOptions) -> Result<RunReport, RunProblem> {
        let mut report = RunReport {
            dry_run: opts.dry_run,
            ..RunReport::default()
        };
        self.catalogue.wait_idle().await;
        if opts.refresh {
            self.catalogue.load(&self.worker, true).await;
            let status = self.catalogue.status();
            if status.state == "failed" {
                return Err(RunProblem::Warning(format!(
                    "Could not refresh the catalogue from RÚV; trying again next run ({})",
                    status.error.unwrap_or_default()
                )));
            }
        }
        // A failed refresh keeps the items loaded before it.
        if self.catalogue.status().loaded_at.is_none() {
            return Err(RunProblem::Warning(
                "The catalogue isn't loaded yet; trying again next run".into(),
            ));
        }

        let follows = list(&self.db).await.map_err(RunProblem::error)?;
        report.follows = follows.len();
        if follows.is_empty() {
            return Ok(report);
        }

        // New sids whose strict key and English flag match a follow.
        let index = self.catalogue.series_index();
        let all_known: HashSet<(i64, String)> = self
            .db
            .call(|c| {
                c.prepare("SELECT follow_id, sid FROM follow_sids")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .await
            .map_err(RunProblem::error)?;
        let follow_by_key: HashMap<(String, bool), i64> = follows
            .iter()
            .map(|f| ((f.title_key.clone(), f.english_subtitled), f.id))
            .collect();
        let mut active: HashSet<String> = follows
            .iter()
            .flat_map(|f| f.sids.iter().map(|s| s.sid.clone()))
            .collect();
        let mut sids: Vec<_> = index.iter().collect();
        sids.sort_by(|a, b| a.0.cmp(b.0));
        for (sid, info) in sids {
            let key = (title_key(&info.series_title), info.english_subtitled);
            let Some(&follow_id) = follow_by_key.get(&key) else {
                continue;
            };
            if all_known.contains(&(follow_id, sid.clone())) {
                continue;
            }
            report.added_sids.push(AddedSid {
                follow_id,
                sid: sid.clone(),
                series_title: info.series_title.clone(),
            });
            active.insert(sid.clone());
        }
        if !opts.dry_run && !report.added_sids.is_empty() {
            let added = report.added_sids.clone();
            let english: HashMap<String, bool> = added
                .iter()
                .map(|a| (a.sid.clone(), index[&a.sid].english_subtitled))
                .collect();
            self.db
                .call(move |c| {
                    let tx = c.transaction()?;
                    for a in &added {
                        insert_sid(&tx, &a.sid, &a.series_title, english[&a.sid], "auto")?;
                    }
                    tx.commit()
                })
                .await
                .map_err(RunProblem::error)?;
        }
        for added in &report.added_sids {
            tracing::info!(
                "follow: {} sid {} ({})",
                if opts.dry_run { "would add" } else { "added" },
                added.sid,
                added.series_title
            );
        }

        // Followed sids that aren't in the catalogue: fetch them from RÚV.
        let mut missing: Vec<&String> = active.iter().filter(|s| !index.contains_key(*s)).collect();
        missing.sort();
        let mut fetched = false;
        for sid in missing {
            match self.worker.series(sid).await {
                Ok(items) => fetched |= !items.is_empty(),
                Err(e) => report
                    .warnings
                    .push(format!("Could not fetch series {sid}: {e}")),
            }
        }
        if fetched {
            self.catalogue.load(&self.worker, false).await;
        }

        // (a) reads the library table, so bring it up to date first.
        if let Err(e) = self.scanner.scan().await {
            report
                .warnings
                .push(format!("The library scan failed: {e}"));
        }

        // Oldest first, so a show's episodes arrive in order.
        let mut episodes = self.catalogue.items_for_sids(&active);
        episodes.reverse();
        report.episodes = episodes.len();
        let pids: Vec<String> = episodes.iter().map(|e| e.pid.clone()).collect();
        let known = self
            .db
            .call(move |c| known(c, &pids))
            .await
            .map_err(RunProblem::error)?;

        let mut candidates = Vec::new();
        for episode in episodes {
            if known.in_queue.contains(&episode.pid) {
                report.skipped.in_queue += 1;
            } else if known.recorded.contains(&episode.pid) {
                report.skipped.recorded += 1;
            } else if let Some(paths) = known.library_paths.get(&episode.pid) {
                let paths = paths.clone();
                let exists = tokio::task::spawn_blocking(move || {
                    paths.iter().any(|p| Path::new(p).is_file())
                })
                .await
                .unwrap_or(false);
                if exists {
                    report.skipped.on_disk += 1;
                } else {
                    candidates.push(episode);
                }
            } else {
                candidates.push(episode);
            }
        }
        if candidates.is_empty() {
            return Ok(report);
        }

        let settings = settings::load(&self.db, &self.data_dir)
            .await
            .map_err(RunProblem::error)?;
        let pids: Vec<String> = candidates.iter().map(|e| e.pid.clone()).collect();
        let names = self
            .worker
            .names(&pids, NameMode::Plex)
            .await
            .map_err(|e| RunProblem::Warning(format!("Could not name the episodes: {e}")))?;
        let names: HashMap<String, _> = names.into_iter().map(|n| (n.pid.clone(), n)).collect();
        for episode in candidates {
            let Some(relative) = names
                .get(&episode.pid)
                .and_then(|n| n.relative_path.clone())
            else {
                report
                    .warnings
                    .push(format!("{} is not in the schedule", episode.pid));
                continue;
            };
            let content_type = names[&episode.pid]
                .content_type
                .clone()
                .unwrap_or_else(|| episode.content_type.to_string());
            let expected =
                library::output_dir(&settings, &content_type, &self.data_dir).join(&relative);
            let check = expected.clone();
            let existing = tokio::task::spawn_blocking(move || library::find_existing(&check))
                .await
                .map_err(RunProblem::error)?;
            match existing {
                Ok(Some(_)) => {
                    report.skipped.name_match += 1;
                    continue;
                }
                Ok(None) => {}
                Err(e) => {
                    report
                        .warnings
                        .push(format!("Could not check {}: {e}", expected.display()));
                    continue;
                }
            }
            let path = expected.to_string_lossy().into_owned();
            if opts.dry_run {
                tracing::info!("dry run: would download {} to {path}", episode.pid);
            } else {
                match self
                    .queue
                    .enqueue(
                        &episode.pid,
                        &episode.title,
                        Mode::Library,
                        Priority::Scheduled,
                        None,
                    )
                    .await
                {
                    Ok(_) => {}
                    Err(JobError::Invalid(m)) => {
                        report.warnings.push(format!("{}: {m}", episode.pid));
                        continue;
                    }
                    Err(JobError::Db(e)) => return Err(RunProblem::error(e)),
                }
            }
            report.scheduled.push(ScheduledEpisode {
                pid: episode.pid,
                title: episode.title,
                path,
            });
        }
        Ok(report)
    }

    /// Runs follow runs on the settings' interval (at least 5 minutes) while
    /// `autoEnabled` is on. The first run comes shortly after startup.
    pub fn spawn_scheduler(self: &Arc<Self>, dry_run: bool) -> tokio::task::JoinHandle<()> {
        let follows = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(30));
            loop {
                tick.tick().await;
                let settings = match settings::load(&follows.db, &follows.data_dir).await {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("scheduler: {e}");
                        continue;
                    }
                };
                if !settings.auto_enabled {
                    continue;
                }
                let interval =
                    Duration::from_secs(60 * u64::from(settings.auto_interval_minutes.max(5)));
                let due = follows
                    .last_started
                    .lock()
                    .unwrap()
                    .is_none_or(|t| t.elapsed() >= interval);
                if due {
                    let _ = follows
                        .run(RunOptions {
                            refresh: true,
                            dry_run,
                        })
                        .await;
                }
            }
        })
    }
}

enum RunProblem {
    /// Expected trouble (RÚV down); the next run tries again.
    Warning(String),
    Error(String),
}

impl RunProblem {
    fn error(e: impl std::fmt::Display) -> RunProblem {
        RunProblem::Error(e.to_string())
    }
}

/// "1 episode", "3 episodes".
fn count(n: usize, one: &str, other: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { other })
}

#[cfg(test)]
mod tests;
