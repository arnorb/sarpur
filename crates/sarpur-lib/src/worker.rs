//! Client for the Python worker (`python ruvsarpur.py --json <command>`).
//! One child process per command, run in the data folder (so the worker's
//! `tvschedule.json` and `imdb-cache.json` live there), with a timeout.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::settings::Quality;

/// Timeouts per command. A full catalogue refresh walks every series on RÚV.
pub const QUICK_TIMEOUT: Duration = Duration::from_secs(60);
pub const CATALOGUE_TIMEOUT: Duration = Duration::from_secs(120);
pub const REFRESH_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("could not start the worker ({program}): {source}")]
    Spawn {
        program: String,
        source: std::io::Error,
    },
    #[error("the worker timed out after {0:?}")]
    Timeout(Duration),
    #[error("{0}")]
    Failed(String),
    #[error("the worker wrote invalid output: {0}")]
    BadOutput(String),
}

#[derive(Debug, Clone)]
pub struct WorkerClient {
    python: String,
    script: PathBuf,
    cwd: PathBuf,
}

/// The worker's `plan` output: everything needed for one download.
#[derive(Debug, Clone, Deserialize)]
pub struct Plan {
    pub pid: String,
    pub sid: String,
    pub title: String,
    pub relative_path: String,
    pub content_type: String,
    pub playlist_url: String,
    pub audio_url: Option<String>,
    pub quality: Option<String>,
    pub duration_seconds: Option<u64>,
    pub metadata: Vec<(String, String)>,
    #[serde(default)]
    pub subtitles: Value,
    #[serde(default)]
    pub images: Images,
}

/// Artwork URLs for a download (`portrait_image`, `series_image`, `episode_image`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct Images {
    pub portrait: Option<String>,
    pub series: Option<String>,
    pub episode: Option<String>,
}

/// One line of the worker's `names` output.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Name {
    pub pid: String,
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub not_found: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMode {
    Web,
    Plex,
}

impl WorkerClient {
    pub fn new(
        python: impl Into<String>,
        script: impl Into<PathBuf>,
        cwd: impl Into<PathBuf>,
    ) -> Self {
        WorkerClient {
            python: python.into(),
            script: script.into(),
            cwd: cwd.into(),
        }
    }

    pub fn script(&self) -> &Path {
        &self.script
    }

    /// Runs one command and returns its JSON lines. A final `{"error": ...}`
    /// line or a non-zero exit becomes [`WorkerError::Failed`].
    pub async fn run(&self, args: &[&str], timeout: Duration) -> Result<Vec<Value>, WorkerError> {
        let mut cmd = Command::new(&self.python);
        cmd.arg(&self.script)
            .arg("--json")
            .arg("--portable")
            .args(args)
            .current_dir(&self.cwd)
            .env("PYTHONIOENCODING", "utf-8")
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = cmd.spawn().map_err(|source| WorkerError::Spawn {
            program: self.python.clone(),
            source,
        })?;
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let command = args.first().copied().unwrap_or("").to_string();

        let stderr_task = tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut tail = Vec::new();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!(target: "sarpur::worker", "{command}: {line}");
                tail.push(line);
                if tail.len() > 5 {
                    tail.remove(0);
                }
            }
            tail
        });

        let work = async {
            let mut values = Vec::new();
            let mut lines = BufReader::new(stdout).lines();
            while let Some(line) = lines
                .next_line()
                .await
                .map_err(|e| WorkerError::BadOutput(e.to_string()))?
            {
                if line.trim().is_empty() {
                    continue;
                }
                let value: Value = serde_json::from_str(&line)
                    .map_err(|e| WorkerError::BadOutput(format!("{e}: {line:.200}")))?;
                values.push(value);
            }
            let status = child
                .wait()
                .await
                .map_err(|e| WorkerError::Failed(e.to_string()))?;
            Ok::<_, WorkerError>((values, status))
        };
        let (values, status) = match tokio::time::timeout(timeout, work).await {
            Ok(result) => result?,
            Err(_) => return Err(WorkerError::Timeout(timeout)),
        };
        let stderr_tail = stderr_task.await.unwrap_or_default();

        if let Some(message) = values
            .last()
            .and_then(|v| v.get("error"))
            .and_then(Value::as_str)
        {
            return Err(WorkerError::Failed(message.to_string()));
        }
        if !status.success() {
            let mut message = format!("the worker exited with {status}");
            if !stderr_tail.is_empty() {
                message.push_str(": ");
                message.push_str(&stderr_tail.join(" | "));
            }
            return Err(WorkerError::Failed(message));
        }
        Ok(values)
    }

    pub async fn plan(
        &self,
        pid: &str,
        mode: NameMode,
        quality: Quality,
    ) -> Result<Plan, WorkerError> {
        let mode = match mode {
            NameMode::Web => "web",
            NameMode::Plex => "plex",
        };
        let mut values = self
            .run(
                &["plan", pid, "--mode", mode, "--quality", quality.as_str()],
                QUICK_TIMEOUT,
            )
            .await?;
        let value = values
            .pop()
            .ok_or_else(|| WorkerError::BadOutput("plan returned nothing".into()))?;
        serde_json::from_value(value).map_err(|e| WorkerError::BadOutput(e.to_string()))
    }

    /// The `plan` file names of many pids, in batches.
    pub async fn names(&self, pids: &[String], mode: NameMode) -> Result<Vec<Name>, WorkerError> {
        let mode = match mode {
            NameMode::Web => "web",
            NameMode::Plex => "plex",
        };
        let mut names = Vec::with_capacity(pids.len());
        for chunk in pids.chunks(400) {
            let mut args = vec!["names", "--mode", mode];
            args.extend(chunk.iter().map(String::as_str));
            for value in self.run(&args, QUICK_TIMEOUT).await? {
                names.push(
                    serde_json::from_value(value)
                        .map_err(|e| WorkerError::BadOutput(e.to_string()))?,
                );
            }
        }
        Ok(names)
    }

    /// All episodes of one series from RÚV, merged into the saved schedule so
    /// `plan` can find them.
    pub async fn series(&self, sid: &str) -> Result<Vec<Value>, WorkerError> {
        self.run(&["series", sid, "--save"], QUICK_TIMEOUT).await
    }

    /// The schedule items. With `refresh`, the worker fetches the catalogue
    /// from RÚV first (takes minutes).
    pub async fn catalogue(&self, refresh: bool) -> Result<Vec<Value>, WorkerError> {
        if refresh {
            self.run(
                &["catalogue", "--refresh", "--incremental"],
                REFRESH_TIMEOUT,
            )
            .await
        } else {
            self.run(&["catalogue"], CATALOGUE_TIMEOUT).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> WorkerClient {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        WorkerClient::new(crate::config::python(), dir.join("fake_worker.py"), dir)
    }

    #[tokio::test]
    async fn parses_lines_and_plan() {
        let w = fake();
        let values = w.run(&["catalogue"], QUICK_TIMEOUT).await.unwrap();
        assert_eq!(values.len(), 2);
        assert_eq!(values[0]["pid"], "web1");
        let plan = w
            .plan("web1", NameMode::Web, Quality::HD1080)
            .await
            .unwrap();
        assert_eq!(plan.relative_path, "Þáttur s01e01.mp4");
        assert_eq!(
            plan.metadata[1],
            ("comment".into(), "ruvinfo:web1:100".into())
        );
    }

    #[tokio::test]
    async fn error_line_becomes_typed_error() {
        let err = fake()
            .plan("err1", NameMode::Web, Quality::HD1080)
            .await
            .unwrap_err();
        match err {
            WorkerError::Failed(message) => {
                assert_eq!(message, "pid err1 not found in the schedule")
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_zero_exit_without_error_line() {
        let err = fake().run(&["crash"], QUICK_TIMEOUT).await.unwrap_err();
        assert!(
            matches!(err, WorkerError::Failed(ref m) if m.contains("exited") && m.contains("boom")),
            "{err}"
        );
    }

    #[tokio::test]
    async fn times_out() {
        let err = fake()
            .run(&["sleep"], Duration::from_millis(300))
            .await
            .unwrap_err();
        assert!(matches!(err, WorkerError::Timeout(_)), "{err}");
    }

    #[tokio::test]
    async fn missing_python_is_a_spawn_error() {
        let w = WorkerClient::new("/nonexistent/python", "x.py", ".");
        assert!(matches!(
            w.run(&["catalogue"], QUICK_TIMEOUT).await,
            Err(WorkerError::Spawn { .. })
        ));
    }
}
