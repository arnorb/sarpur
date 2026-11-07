//! The download queue: one ffmpeg at a time, interactive jobs before scheduled
//! ones, FIFO within a priority. Jobs live in the `jobs` table; changes are
//! broadcast as [`Event`]s.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast, mpsc, watch};

use crate::db::{Db, DbError, rusqlite};
use crate::library;
use crate::media::{self, Download, Outcome, Stop};
use crate::settings;
use crate::worker::{NameMode, Plan, WorkerClient};

/// Finished browser downloads are kept this long.
pub const WEB_RETENTION: Duration = Duration::from_secs(24 * 3600);
/// Used for the disk-space estimate when the catalogue has no duration.
const UNKNOWN_DURATION_SECS: u64 = 3600;
pub const ALREADY_IN_LIBRARY: &str = "Already in the library";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Web,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Interactive,
    Scheduled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

macro_rules! text_enum {
    ($ty:ty { $($variant:ident => $text:literal),* $(,)? }) => {
        impl $ty {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),* }
            }
            fn from_db(text: &str) -> rusqlite::Result<Self> {
                match text {
                    $($text => Ok(Self::$variant),)*
                    other => Err(rusqlite::Error::InvalidColumnType(
                        0,
                        format!("unknown value {other}"),
                        rusqlite::types::Type::Text,
                    )),
                }
            }
        }
    };
}
text_enum!(Mode { Web => "web", Library => "library" });
text_enum!(Priority { Interactive => "interactive", Scheduled => "scheduled" });
text_enum!(State {
    Queued => "queued",
    Running => "running",
    Completed => "completed",
    Failed => "failed",
    Cancelled => "cancelled",
});

impl State {
    pub fn is_finished(self) -> bool {
        matches!(self, State::Completed | State::Failed | State::Cancelled)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct JobFile {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub client_id: Option<String>,
    pub pid: String,
    pub title: String,
    pub mode: Mode,
    pub priority: Priority,
    pub state: State,
    pub progress: u8,
    pub message: Option<String>,
    #[serde(skip)]
    pub part_path: Option<String>,
    pub final_path: Option<String>,
    pub files: Vec<JobFile>,
    pub ffmpeg_exit: Option<i32>,
    pub error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

/// What the SSE stream carries.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    /// Any job change; sent to everyone.
    Job { job: Box<Job> },
    /// A finished browser download; sent only to the browser that asked for it.
    /// `client_id` is routing, not security.
    FileReady {
        #[serde(rename = "clientId")]
        client_id: String,
        #[serde(rename = "jobId")]
        job_id: String,
        name: String,
        url: String,
    },
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::Job { .. } => "job",
            Event::FileReady { .. } => "file_ready",
        }
    }

    /// Whether an SSE subscriber with `client_id` gets this event.
    pub fn visible_to(&self, client_id: Option<&str>) -> bool {
        match self {
            Event::Job { .. } => true,
            Event::FileReady {
                client_id: target, ..
            } => client_id == Some(target.as_str()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("{0}")]
    Invalid(String),
}

/// Free bytes available at a path (mockable for tests).
pub type FreeSpaceFn = Arc<dyn Fn(&Path) -> io::Result<u64> + Send + Sync>;

/// Free space on the filesystem holding `path`, or its nearest existing parent.
pub fn real_free_space(path: &Path) -> io::Result<u64> {
    let existing = path
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(Path::new("."));
    fs4::available_space(existing)
}

pub struct QueueConfig {
    pub data_dir: PathBuf,
    pub ffmpeg: String,
    pub dry_run: bool,
    pub stop_grace: Duration,
    pub free_space: FreeSpaceFn,
}

#[derive(Clone)]
pub struct Queue {
    inner: Arc<Inner>,
}

struct Inner {
    db: Db,
    worker: WorkerClient,
    cfg: QueueConfig,
    events: broadcast::Sender<Event>,
    wake: Notify,
    /// The job the runner is working on, and how to stop it.
    running: Mutex<Option<(String, watch::Sender<Option<Stop>>)>>,
    shutdown: watch::Sender<bool>,
}

const JOB_COLUMNS: &str = "id, client_id, pid, title, mode, priority, state, progress, message, \
     part_path, final_path, files, ffmpeg_exit, error, created_at, started_at, finished_at";

fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    let id: String = row.get(0)?;
    let files: String = row.get(11)?;
    let names: Vec<String> = serde_json::from_str(&files).unwrap_or_default();
    let files = names
        .into_iter()
        .map(|name| JobFile {
            url: file_url(&id, &name),
            name,
        })
        .collect();
    Ok(Job {
        client_id: row.get(1)?,
        pid: row.get(2)?,
        title: row.get(3)?,
        mode: Mode::from_db(&row.get::<_, String>(4)?)?,
        priority: Priority::from_db(&row.get::<_, String>(5)?)?,
        state: State::from_db(&row.get::<_, String>(6)?)?,
        progress: row.get(7)?,
        message: row.get(8)?,
        part_path: row.get(9)?,
        final_path: row.get(10)?,
        files,
        ffmpeg_exit: row.get(12)?,
        error: row.get(13)?,
        created_at: row.get(14)?,
        started_at: row.get(15)?,
        finished_at: row.get(16)?,
        id,
    })
}

/// `/api/jobs/{id}/files/{name}` with the name percent-encoded.
pub fn file_url(job_id: &str, name: &str) -> String {
    format!("/api/jobs/{job_id}/files/{}", percent_encode(name))
}

/// Percent-encodes everything except RFC 3986 unreserved characters.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn get_job(c: &rusqlite::Connection, id: &str) -> rusqlite::Result<Option<Job>> {
    let mut stmt = c.prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"))?;
    let mut rows = stmt.query_map([id], job_from_row)?;
    rows.next().transpose()
}

fn save_job(c: &rusqlite::Connection, job: &Job) -> rusqlite::Result<()> {
    let files: Vec<&str> = job.files.iter().map(|f| f.name.as_str()).collect();
    c.execute(
        "UPDATE jobs SET title = ?2, state = ?3, progress = ?4, message = ?5, part_path = ?6,
             final_path = ?7, files = ?8, ffmpeg_exit = ?9, error = ?10, started_at = ?11,
             finished_at = ?12
         WHERE id = ?1",
        rusqlite::params![
            job.id,
            job.title,
            job.state.as_str(),
            job.progress,
            job.message,
            job.part_path,
            job.final_path,
            serde_json::to_string(&files).unwrap_or_else(|_| "[]".into()),
            job.ffmpeg_exit,
            job.error,
            job.started_at,
            job.finished_at,
        ],
    )?;
    Ok(())
}

/// The next job to run: interactive before scheduled, then FIFO.
fn next_queued(c: &rusqlite::Connection) -> rusqlite::Result<Option<Job>> {
    let mut stmt = c.prepare(&format!(
        "SELECT {JOB_COLUMNS} FROM jobs WHERE state = 'queued'
         ORDER BY priority = 'interactive' DESC, queue_order LIMIT 1"
    ))?;
    let mut rows = stmt.query_map([], job_from_row)?;
    rows.next().transpose()
}

impl Queue {
    pub fn new(db: Db, worker: WorkerClient, cfg: QueueConfig) -> Queue {
        let (events, _) = broadcast::channel(256);
        let (shutdown, _) = watch::channel(false);
        Queue {
            inner: Arc::new(Inner {
                db,
                worker,
                cfg,
                events,
                wake: Notify::new(),
                running: Mutex::new(None),
                shutdown,
            }),
        }
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.cfg.data_dir
    }

    pub fn web_dir(&self) -> PathBuf {
        self.inner.cfg.data_dir.join("web")
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    fn emit(&self, event: Event) {
        let _ = self.inner.events.send(event);
    }

    fn emit_job(&self, job: &Job) {
        self.emit(Event::Job {
            job: Box::new(job.clone()),
        });
    }

    async fn save(&self, job: &Job) -> Result<(), DbError> {
        let copy = job.clone();
        self.inner.db.call(move |c| save_job(c, &copy)).await?;
        self.emit_job(job);
        Ok(())
    }

    /// Startup recovery for jobs left `running` by a crash or shutdown: their
    /// `.part` file is deleted, interactive jobs go back in the queue and
    /// scheduled jobs fail with "interrupted".
    pub async fn recover(&self) -> Result<usize, DbError> {
        let now = crate::now_iso();
        let jobs = self
            .inner
            .db
            .call(|c| {
                let mut stmt = c.prepare(&format!(
                    "SELECT {JOB_COLUMNS} FROM jobs WHERE state = 'running'"
                ))?;
                let rows = stmt
                    .query_map([], job_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await?;
        for mut job in jobs.iter().cloned() {
            if let Some(part) = job.part_path.take() {
                media::remove_if_exists(Path::new(&part));
            }
            match job.priority {
                Priority::Interactive => {
                    job.state = State::Queued;
                    job.progress = 0;
                    job.message = Some("Requeued after a restart".into());
                    job.started_at = None;
                    job.final_path = None;
                }
                Priority::Scheduled => {
                    job.state = State::Failed;
                    job.message = Some("Interrupted".into());
                    job.error = Some("interrupted".into());
                    job.finished_at = Some(now.clone());
                }
            }
            tracing::info!(
                "recovered job {} ({}) as {}",
                job.id,
                job.pid,
                job.state.as_str()
            );
            self.save(&job).await?;
        }
        self.inner.wake.notify_one();
        Ok(jobs.len())
    }

    pub async fn enqueue(
        &self,
        pid: &str,
        title: &str,
        mode: Mode,
        priority: Priority,
        client_id: Option<String>,
    ) -> Result<Job, JobError> {
        let pid = pid.trim().to_string();
        if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(JobError::Invalid(
                "pid must be a non-empty alphanumeric id".into(),
            ));
        }
        let id = uuid::Uuid::new_v4().simple().to_string();
        let title = if title.trim().is_empty() {
            pid.clone()
        } else {
            title.to_string()
        };
        let created = crate::now_iso();
        let job = self
            .inner
            .db
            .call(move |c| {
                c.execute(
                    "INSERT INTO jobs (id, queue_order, client_id, pid, title, mode, priority, state, message, created_at)
                     VALUES (?1, (SELECT COALESCE(MAX(queue_order), 0) + 1 FROM jobs), ?2, ?3, ?4, ?5, ?6, 'queued', 'Queued', ?7)",
                    rusqlite::params![id, client_id, pid, title, mode.as_str(), priority.as_str(), created],
                )?;
                get_job(c, &id)
            })
            .await?
            .expect("inserted job");
        self.emit_job(&job);
        self.inner.wake.notify_one();
        Ok(job)
    }

    /// All jobs, newest first.
    pub async fn list(&self) -> Result<Vec<Job>, DbError> {
        self.inner
            .db
            .call(|c| {
                let mut stmt = c.prepare(&format!(
                    "SELECT {JOB_COLUMNS} FROM jobs ORDER BY queue_order DESC"
                ))?;
                let rows = stmt
                    .query_map([], job_from_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    pub async fn get(&self, id: &str) -> Result<Option<Job>, DbError> {
        let id = id.to_string();
        self.inner.db.call(move |c| get_job(c, &id)).await
    }

    /// DELETE semantics: a queued job is cancelled, a running one is stopped
    /// (and becomes cancelled when ffmpeg has stopped), and a finished one is
    /// removed from the list (with its browser-download folder). Returns the
    /// job as it is now, `Ok(None)` if it was removed, or `Err` if unknown.
    pub async fn cancel_or_remove(&self, id: &str) -> Result<Option<Job>, JobError> {
        let Some(mut job) = self.get(id).await? else {
            return Err(JobError::Invalid(format!("no job {id}")));
        };
        match job.state {
            State::Queued => {
                let claim_id = job.id.clone();
                let changed = self
                    .inner
                    .db
                    .call(move |c| {
                        c.execute(
                            "UPDATE jobs SET state = 'cancelled', message = 'Cancelled', finished_at = ?2
                             WHERE id = ?1 AND state = 'queued'",
                            (&claim_id, crate::now_iso()),
                        )
                    })
                    .await?;
                if changed == 0 {
                    // The runner picked it up in the meantime.
                    return Box::pin(self.cancel_or_remove(id)).await;
                }
                job = self.get(id).await?.expect("job exists");
                self.emit_job(&job);
                Ok(Some(job))
            }
            State::Running => {
                if let Some((running_id, stop)) = self.inner.running.lock().unwrap().as_ref() {
                    if running_id == id {
                        let _ = stop.send(Some(Stop::Cancel));
                    }
                }
                job.message = Some("Stopping".into());
                self.emit_job(&job);
                Ok(Some(job))
            }
            _ => {
                let remove_id = job.id.clone();
                self.inner
                    .db
                    .call(move |c| c.execute("DELETE FROM jobs WHERE id = ?1", [&remove_id]))
                    .await?;
                if job.mode == Mode::Web {
                    let _ = std::fs::remove_dir_all(self.web_dir().join(&job.id));
                }
                Ok(None)
            }
        }
    }

    /// Puts a finished job back at the end of the queue.
    pub async fn retry(&self, id: &str) -> Result<Job, JobError> {
        let Some(mut job) = self.get(id).await? else {
            return Err(JobError::Invalid(format!("no job {id}")));
        };
        if !job.state.is_finished() {
            return Err(JobError::Invalid(format!(
                "job {id} is {}",
                job.state.as_str()
            )));
        }
        if job.mode == Mode::Web {
            let _ = std::fs::remove_dir_all(self.web_dir().join(&job.id));
        }
        job.state = State::Queued;
        job.progress = 0;
        job.message = Some("Queued".into());
        job.part_path = None;
        job.final_path = None;
        job.files.clear();
        job.ffmpeg_exit = None;
        job.error = None;
        job.started_at = None;
        job.finished_at = None;
        self.save(&job).await?;
        let retry_id = job.id.clone();
        self.inner
            .db
            .call(move |c| {
                c.execute(
                    "UPDATE jobs SET queue_order = (SELECT MAX(queue_order) + 1 FROM jobs) WHERE id = ?1",
                    [&retry_id],
                )
            })
            .await?;
        self.inner.wake.notify_one();
        Ok(job)
    }

    /// Runs queued jobs one at a time until [`Queue::shutdown`].
    pub fn spawn_runner(&self) -> tokio::task::JoinHandle<()> {
        let queue = self.clone();
        tokio::spawn(async move {
            let mut shutdown = queue.inner.shutdown.subscribe();
            loop {
                if *shutdown.borrow() {
                    break;
                }
                match queue.inner.db.call(|c| next_queued(c)).await {
                    Ok(Some(job)) => queue.run_job(job).await,
                    Ok(None) => {
                        tokio::select! {
                            _ = queue.inner.wake.notified() => {}
                            _ = shutdown.changed() => {}
                        }
                    }
                    Err(e) => {
                        tracing::error!("queue: {e}");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        })
    }

    /// Stops the runner. A running ffmpeg is stopped and its job left
    /// `running`, so startup recovery deletes the `.part` and requeues it.
    pub async fn shutdown(&self, runner: tokio::task::JoinHandle<()>) {
        let _ = self.inner.shutdown.send(true);
        if let Some((_, stop)) = self.inner.running.lock().unwrap().as_ref() {
            let _ = stop.send(Some(Stop::Shutdown));
        }
        let _ = runner.await;
    }

    async fn run_job(&self, mut job: Job) {
        let (stop_tx, mut stop_rx) = watch::channel(None);
        *self.inner.running.lock().unwrap() = Some((job.id.clone(), stop_tx));
        if *self.inner.shutdown.borrow() {
            self.inner.running.lock().unwrap().take();
            return;
        }
        let claim_id = job.id.clone();
        let started = crate::now_iso();
        let started_copy = started.clone();
        let claimed = self
            .inner
            .db
            .call(move |c| {
                c.execute(
                    "UPDATE jobs SET state = 'running', started_at = ?2, message = 'Starting'
                     WHERE id = ?1 AND state = 'queued'",
                    (&claim_id, &started_copy),
                )
            })
            .await;
        if !matches!(claimed, Ok(1)) {
            self.inner.running.lock().unwrap().take();
            return;
        }
        job.state = State::Running;
        job.started_at = Some(started);
        job.message = Some("Starting".into());
        self.emit_job(&job);

        let result = self.execute(&mut job, &mut stop_rx).await;
        self.inner.running.lock().unwrap().take();
        let stopped_for_shutdown = matches!(result, Ok(Some(Stop::Shutdown)));
        if stopped_for_shutdown {
            tracing::info!(
                "job {} stopped for shutdown; it resumes on the next start",
                job.id
            );
            return;
        }
        match result {
            Ok(Some(Stop::Cancel)) => {
                job.state = State::Cancelled;
                job.message = Some("Cancelled".into());
                job.part_path = None;
            }
            Ok(_) => {}
            Err(message) => {
                tracing::warn!("job {} ({}) failed: {message}", job.id, job.pid);
                job.state = State::Failed;
                job.message = Some("Failed".into());
                job.error = Some(message);
                job.part_path = None;
            }
        }
        job.finished_at = Some(crate::now_iso());
        if job.mode == Mode::Web && job.state != State::Completed {
            let _ = std::fs::remove_dir_all(self.web_dir().join(&job.id));
        }
        if let Err(e) = self.save(&job).await {
            tracing::error!("could not save job {}: {e}", job.id);
        }
        if job.state == State::Completed {
            if let (Mode::Web, Some(client_id), Some(file)) =
                (job.mode, job.client_id.clone(), job.files.first())
            {
                self.emit(Event::FileReady {
                    client_id,
                    job_id: job.id.clone(),
                    name: file.name.clone(),
                    url: file.url.clone(),
                });
            }
        }
    }

    /// Does the work of one job. `Ok(Some(stop))` when stopped; `Ok(None)` when
    /// `job` was completed; `Err(message)` on failure.
    async fn execute(
        &self,
        job: &mut Job,
        stop_rx: &mut watch::Receiver<Option<Stop>>,
    ) -> Result<Option<Stop>, String> {
        let cfg = &self.inner.cfg;
        let settings = settings::load(&self.inner.db, &cfg.data_dir)
            .await
            .map_err(|e| e.to_string())?;
        let name_mode = match job.mode {
            Mode::Web => NameMode::Web,
            Mode::Library => NameMode::Plex,
        };

        let plan: Plan = tokio::select! {
            plan = self.inner.worker.plan(&job.pid, name_mode, settings.quality) => plan.map_err(|e| e.to_string())?,
            stop = wait_for_stop(stop_rx) => return Ok(Some(stop)),
        };
        job.title = plan.title.clone();

        let final_path = match job.mode {
            Mode::Library => library::output_dir(&settings, &plan.content_type, &cfg.data_dir)
                .join(&plan.relative_path),
            Mode::Web => {
                let name = Path::new(&plan.relative_path)
                    .file_name()
                    .ok_or_else(|| format!("bad file name {:?}", plan.relative_path))?;
                self.web_dir().join(&job.id).join(name)
            }
        };
        job.final_path = Some(final_path.to_string_lossy().into_owned());

        if job.mode == Mode::Library {
            if let Some(existing) =
                library::find_existing(&final_path).map_err(|e| e.to_string())?
            {
                tracing::info!(
                    "{} is already in the library: {}",
                    job.pid,
                    existing.display()
                );
                job.final_path = Some(existing.to_string_lossy().into_owned());
                self.complete(job, ALREADY_IN_LIBRARY, &existing, &plan)
                    .await;
                return Ok(None);
            }
        }

        let duration = plan.duration_seconds.unwrap_or(UNKNOWN_DURATION_SECS);
        let quality = plan
            .quality
            .as_deref()
            .and_then(settings::Quality::parse)
            .unwrap_or(settings.quality);
        let estimate = duration * quality.bits_per_second() / 8;
        let target_dir = final_path.parent().unwrap_or(Path::new("."));
        let free = (cfg.free_space)(target_dir).map_err(|e| {
            format!(
                "could not check free disk space at {}: {e}",
                target_dir.display()
            )
        })?;
        if free < estimate.saturating_mul(2) {
            return Err(format!(
                "Not enough disk space at {}: {} MB free, need at least {} MB (twice the estimated {} MB)",
                target_dir.display(),
                free / 1_000_000,
                estimate * 2 / 1_000_000,
                estimate / 1_000_000
            ));
        }

        if cfg.dry_run {
            tracing::info!(
                "dry run: would download {} ({}) from {} to {}",
                job.pid,
                plan.title,
                plan.playlist_url,
                final_path.display()
            );
            job.state = State::Completed;
            job.progress = 100;
            job.message = Some("dry run".into());
            return Ok(None);
        }

        std::fs::create_dir_all(target_dir)
            .map_err(|e| format!("could not create {}: {e}", target_dir.display()))?;
        job.part_path = Some(media::part_path(&final_path).to_string_lossy().into_owned());
        job.message = Some("Downloading".into());
        self.save(job).await.map_err(|e| e.to_string())?;

        let download = Download {
            ffmpeg: &cfg.ffmpeg,
            playlist_url: &plan.playlist_url,
            audio_url: plan.audio_url.as_deref(),
            metadata: &plan.metadata,
            final_path: &final_path,
            duration_secs: plan.duration_seconds.map(|d| d as f64),
            stop_grace: cfg.stop_grace,
        };
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let run = media::download(&download, progress_tx, stop_rx);
        tokio::pin!(run);
        let outcome = loop {
            tokio::select! {
                outcome = &mut run => break outcome,
                Some(pct) = progress_rx.recv() => {
                    job.progress = pct;
                    if let Err(e) = self.save(job).await {
                        tracing::warn!("could not save progress: {e}");
                    }
                }
            }
        };
        match outcome.map_err(|e| format!("could not run ffmpeg ({}): {e}", cfg.ffmpeg))? {
            Outcome::Completed => {
                job.part_path = None;
                self.complete(job, "Downloaded", &final_path, &plan).await;
                Ok(None)
            }
            Outcome::Failed { exit, message } => {
                job.ffmpeg_exit = exit;
                Err(message)
            }
            Outcome::Stopped(stop) => Ok(Some(stop)),
        }
    }

    /// Marks `job` completed with the file at `path`, and records it.
    async fn complete(&self, job: &mut Job, message: &str, path: &Path, plan: &Plan) {
        job.state = State::Completed;
        job.progress = 100;
        job.message = Some(message.into());
        if job.mode == Mode::Web {
            if let Some(name) = path.file_name() {
                let name = name.to_string_lossy().into_owned();
                job.files = vec![JobFile {
                    url: file_url(&job.id, &name),
                    name,
                }];
            }
        }
        let pid = job.pid.clone();
        let mode = job.mode;
        let path_text = path.to_string_lossy().into_owned();
        let meta = std::fs::metadata(path).ok();
        let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
        let mtime = meta
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let tagged = plan
            .metadata
            .iter()
            .any(|(k, v)| k == "comment" && v.starts_with("ruvinfo:"));
        let already = message == ALREADY_IN_LIBRARY;
        let now = crate::now_iso();
        let result = self
            .inner
            .db
            .call(move |c| {
                match mode {
                    // Browser downloads never mark a pid as in the library.
                    Mode::Web => {
                        c.execute(
                            "INSERT INTO recorded (pid, origin, recorded_at) VALUES (?1, 'web', ?2)
                             ON CONFLICT (pid) DO NOTHING",
                            (&pid, &now),
                        )?;
                    }
                    Mode::Library => {
                        c.execute(
                            "INSERT INTO recorded (pid, origin, recorded_at) VALUES (?1, 'library', ?2)
                             ON CONFLICT (pid) DO UPDATE SET origin = 'library',
                                 recorded_at = excluded.recorded_at, forgotten_at = NULL",
                            (&pid, &now),
                        )?;
                        let source = if already || !tagged { "name" } else { "tag" };
                        c.execute(
                            "INSERT INTO library (path, pid, size, mtime, source, verified_at)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                             ON CONFLICT (path) DO UPDATE SET pid = excluded.pid, size = excluded.size,
                                 mtime = excluded.mtime, source = excluded.source,
                                 verified_at = excluded.verified_at",
                            rusqlite::params![path_text, pid, size, mtime, source, now],
                        )?;
                    }
                }
                Ok(())
            })
            .await;
        if let Err(e) = result {
            tracing::warn!("could not record {}: {e}", job.pid);
        }
    }

    /// Deletes browser-download folders older than [`WEB_RETENTION`] at `now`,
    /// skipping queued and running jobs. Returns how many were removed.
    pub async fn sweep_web(&self, now: SystemTime) -> Result<usize, DbError> {
        let active: Vec<String> = self
            .inner
            .db
            .call(|c| {
                let mut stmt =
                    c.prepare("SELECT id FROM jobs WHERE state IN ('queued', 'running')")?;
                let rows = stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await?;
        let Ok(entries) = std::fs::read_dir(self.web_dir()) else {
            return Ok(0);
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if active.contains(&name) {
                continue;
            }
            let modified = entry.metadata().and_then(|m| m.modified()).unwrap_or(now);
            if now.duration_since(modified).unwrap_or_default() < WEB_RETENTION {
                continue;
            }
            let path = entry.path();
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => removed += 1,
                Err(e) => tracing::warn!("could not sweep {}: {e}", path.display()),
            }
        }
        if removed > 0 {
            tracing::info!("swept {removed} expired browser download(s)");
        }
        Ok(removed)
    }
}

async fn wait_for_stop(stop: &mut watch::Receiver<Option<Stop>>) -> Stop {
    loop {
        if let Some(reason) = *stop.borrow_and_update() {
            return reason;
        }
        if stop.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

#[cfg(test)]
mod tests;
