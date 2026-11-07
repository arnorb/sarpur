//! The NDJSON event log, `<data>/download-events.log`. Same file name, line
//! shape (`{timestampUtc, event, status, details}`, sorted keys) and rotation
//! (5 MB, 5 backups) as ruvsarpur's `log_download_event`, so tools that read
//! the old log keep working and the import can read both.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::jobs::{Event, Queue, State};

pub const FILE_NAME: &str = "download-events.log";
const MAX_BYTES: u64 = 5 * 1024 * 1024;
const BACKUPS: usize = 5;

#[derive(Clone)]
pub struct EventLog {
    path: PathBuf,
    max_bytes: u64,
    lock: Arc<Mutex<()>>,
}

/// `YYYY-MM-DDTHH:MM:SS.ffffffZ`, as Python's `utcnow().isoformat() + "Z"`.
fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let iso = crate::iso_from_unix(now.as_secs() as i64);
    format!("{}.{:06}Z", iso.trim_end_matches('Z'), now.subsec_micros())
}

impl EventLog {
    pub fn new(data_dir: &Path) -> EventLog {
        EventLog {
            path: data_dir.join(FILE_NAME),
            max_bytes: MAX_BYTES,
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line. Never fails the caller; problems are logged.
    pub async fn log(&self, event: &str, status: &str, details: Value) {
        // serde_json's maps are sorted, as Python's `sort_keys=True`.
        let line = json!({
            "timestampUtc": timestamp(),
            "event": event,
            "status": status,
            "details": details,
        })
        .to_string();
        let log = self.clone();
        let result = tokio::task::spawn_blocking(move || log.append(&line)).await;
        if let Ok(Err(e)) | Err(e) = result.map_err(std::io::Error::other) {
            tracing::warn!("event log {}: {e}", self.path.display());
        }
    }

    fn append(&self, line: &str) -> std::io::Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|p| p.into_inner());
        let size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if size > 0 && size + line.len() as u64 + 1 > self.max_bytes {
            self.rotate()?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{line}")
    }

    /// `log` → `log.1` → … → `log.5`; the oldest is dropped.
    fn rotate(&self) -> std::io::Result<()> {
        let numbered = |n: usize| PathBuf::from(format!("{}.{n}", self.path.display()));
        let _ = std::fs::remove_file(numbered(BACKUPS));
        for n in (1..BACKUPS).rev() {
            if numbered(n).exists() {
                std::fs::rename(numbered(n), numbered(n + 1))?;
            }
        }
        std::fs::rename(&self.path, numbered(1))
    }
}

fn mode(job: &crate::jobs::Job) -> &'static str {
    job.mode.as_str()
}

/// Writes queue, follow-run and Plex events to the log: `download_request`
/// when a job is queued (again), `download` when it finishes, `follow_run`
/// when a run ends and `plex_refresh`.
pub fn spawn_logger(queue: &Queue, log: EventLog) -> tokio::task::JoinHandle<()> {
    let mut events = queue.subscribe();
    tokio::spawn(async move {
        // The last state logged per job, so progress updates aren't logged.
        let mut logged: HashMap<String, State> = HashMap::new();
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("event log missed {n} events");
                    continue;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match event {
                Event::Job { job } => {
                    let interesting = matches!(job.state, State::Queued) || job.state.is_finished();
                    if !interesting || logged.get(&job.id) == Some(&job.state) {
                        continue;
                    }
                    logged.insert(job.id.clone(), job.state);
                    if job.state == State::Queued {
                        log.log(
                            "download_request",
                            "queued",
                            json!({
                                "jobId": job.id, "pid": job.pid, "mode": mode(&job),
                                "priority": job.priority.as_str(),
                            }),
                        )
                        .await;
                    } else {
                        log.log(
                            "download",
                            job.state.as_str(),
                            json!({
                                "jobId": job.id, "pid": job.pid, "mode": mode(&job),
                                "priority": job.priority.as_str(), "title": job.title,
                                "message": job.message, "finalPath": job.final_path,
                                "error": job.error,
                            }),
                        )
                        .await;
                    }
                }
                Event::Follows { status } if !status.running => {
                    let report = status.last_report.as_ref();
                    log.log(
                        "follow_run",
                        status.last_run_state.unwrap_or("ok"),
                        json!({
                            "message": status.last_run_message,
                            "dryRun": report.map(|r| r.dry_run),
                            "scheduled": report.map(|r| r.scheduled.iter().map(|e| e.pid.clone()).collect::<Vec<_>>()),
                            "addedSids": report.map(|r| r.added_sids.iter().map(|a| a.sid.clone()).collect::<Vec<_>>()),
                            "warnings": report.map(|r| r.warnings.clone()),
                        }),
                    )
                    .await;
                }
                Event::Plex { result } => {
                    log.log(
                        "plex_refresh",
                        result.state,
                        json!({ "message": result.message }),
                    )
                    .await;
                }
                _ => {}
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn writes_sorted_lines_and_rotates() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = EventLog::new(dir.path());
        log.log(
            "download_request",
            "queued",
            json!({"pid": "p1", "mode": "web"}),
        )
        .await;
        let text = std::fs::read_to_string(log.path()).unwrap();
        assert!(
            text.starts_with(r#"{"details":{"mode":"web","pid":"p1"},"event":"download_request","status":"queued","timestampUtc":""#),
            "{text}"
        );
        assert!(text.trim_end().ends_with("Z\"}"), "{text}");

        log.max_bytes = 300;
        for n in 0..20 {
            log.log("download", "completed", json!({"n": n})).await;
        }
        let base = log.path().to_string_lossy().into_owned();
        assert!(Path::new(&format!("{base}.5")).exists());
        assert!(!Path::new(&format!("{base}.6")).exists());
        let last = lines(log.path());
        assert_eq!(last.last().unwrap()["details"]["n"], 19);
        assert!(std::fs::metadata(log.path()).unwrap().len() <= 300);
    }
}
