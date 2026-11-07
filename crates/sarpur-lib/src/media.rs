//! Runs ffmpeg for one download: writes `<final>.part`, reports progress from
//! `-progress pipe:1`, and renames to the final name only on success.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

/// Why a running download is being stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The user cancelled: the `.part` file is deleted.
    Cancel,
    /// The server is shutting down: the `.part` file is left for startup
    /// recovery, which deletes it and requeues the job.
    Shutdown,
}

pub struct Download<'a> {
    pub ffmpeg: &'a str,
    pub playlist_url: &'a str,
    pub audio_url: Option<&'a str>,
    pub metadata: &'a [(String, String)],
    pub final_path: &'a Path,
    pub duration_secs: Option<f64>,
    /// How long ffmpeg gets to quit after `q` before it is killed.
    pub stop_grace: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Completed,
    Failed { exit: Option<i32>, message: String },
    Stopped(Stop),
}

pub fn part_path(final_path: &Path) -> PathBuf {
    let mut name = OsString::from(final_path.as_os_str());
    name.push(".part");
    PathBuf::from(name)
}

/// ffmpeg's arguments, mirroring ruvsarpur's `download_m3u8_playlist_using_ffmpeg`
/// (inputs, the audio `-map`, `-c copy -bsf:a aac_adtstoasc`, the metadata) with
/// machine-readable progress instead of `-stats`.
pub fn build_args(d: &Download<'_>, part: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-progress",
        "pipe:1",
        "-y",
        "-i",
        d.playlist_url,
    ]
    .iter()
    .map(OsString::from)
    .collect();
    if let Some(audio) = d.audio_url {
        for a in ["-i", audio, "-map", "0:v:0", "-map", "1:a:0"] {
            args.push(a.into());
        }
    }
    for a in ["-c", "copy", "-bsf:a", "aac_adtstoasc"] {
        args.push(a.into());
    }
    for (key, value) in d.metadata {
        args.push("-metadata".into());
        args.push(format!("{key}={value}").into());
    }
    args.push("-f".into());
    args.push("mp4".into());
    args.push(part.as_os_str().to_owned());
    args
}

/// Parses one `-progress` line into a whole percent below 100.
fn progress_percent(line: &str, duration_secs: f64) -> Option<u8> {
    let (key, value) = line.split_once('=')?;
    if key != "out_time_us" && key != "out_time_ms" {
        return None;
    }
    let micros: f64 = value.trim().parse().ok()?;
    if duration_secs <= 0.0 || micros < 0.0 {
        return None;
    }
    Some((micros / 1e6 / duration_secs * 100.0).floor().min(99.0) as u8)
}

/// Runs the download. Progress (whole percent, only on change) goes to
/// `progress`. A value sent on `stop` stops ffmpeg (`q`, then kill after
/// `stop_grace`).
pub async fn download(
    d: &Download<'_>,
    progress: mpsc::UnboundedSender<u8>,
    stop: &mut watch::Receiver<Option<Stop>>,
) -> io::Result<Outcome> {
    let part = part_path(d.final_path);
    let mut cmd = Command::new(d.ffmpeg);
    cmd.args(build_args(d, &part))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Its own process group, so a Ctrl-C in the terminal reaches only sarpur,
    // which then stops ffmpeg itself.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut tail: Vec<String> = Vec::new();
        while let Ok(Some(line)) = lines.next_line().await {
            tracing::debug!(target: "sarpur::ffmpeg", "{line}");
            if !line.trim().is_empty() {
                tail.push(line);
                if tail.len() > 5 {
                    tail.remove(0);
                }
            }
        }
        tail
    });

    let duration = d.duration_secs.unwrap_or(0.0);
    let mut last = None;
    let mut lines = BufReader::new(stdout).lines();
    let mut stop_open = true;
    let mut stopped = *stop.borrow();
    while stopped.is_none() {
        tokio::select! {
            line = lines.next_line() => match line {
                Ok(Some(line)) => {
                    if let Some(pct) = progress_percent(&line, duration) {
                        if last != Some(pct) {
                            last = Some(pct);
                            let _ = progress.send(pct);
                        }
                    }
                }
                _ => break,
            },
            changed = stop.changed(), if stop_open => {
                if changed.is_err() {
                    stop_open = false;
                } else {
                    stopped = *stop.borrow();
                }
            }
        }
    }

    if let Some(reason) = stopped {
        if let Some(stdin) = stdin.as_mut() {
            let _ = stdin.write_all(b"q").await;
            let _ = stdin.flush().await;
        }
        if tokio::time::timeout(d.stop_grace, child.wait())
            .await
            .is_err()
        {
            tracing::warn!("ffmpeg didn't stop within {:?}; killing it", d.stop_grace);
            let _ = child.kill().await;
        }
        if reason == Stop::Cancel {
            remove_if_exists(&part);
        }
        return Ok(Outcome::Stopped(reason));
    }

    drop(stdin);
    let status = child.wait().await?;
    let tail = stderr_task.await.unwrap_or_default();
    if !status.success() {
        remove_if_exists(&part);
        let message = if tail.is_empty() {
            format!("ffmpeg exited with {status}")
        } else {
            format!("ffmpeg exited with {status}: {}", tail.join(" | "))
        };
        return Ok(Outcome::Failed {
            exit: status.code(),
            message,
        });
    }
    if let Err(e) = std::fs::rename(&part, d.final_path) {
        remove_if_exists(&part);
        return Ok(Outcome::Failed {
            exit: status.code(),
            message: format!("could not rename the finished download: {e}"),
        });
    }
    if !d.final_path.is_file() {
        return Ok(Outcome::Failed {
            exit: status.code(),
            message: "ffmpeg finished but the file is missing".into(),
        });
    }
    Ok(Outcome::Completed)
}

pub(crate) fn remove_if_exists(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("could not delete {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_ffmpeg() -> String {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata/fake_ffmpeg.py")
            .to_string_lossy()
            .into_owned()
    }

    fn spec<'a>(
        ffmpeg: &'a str,
        url: &'a str,
        final_path: &'a Path,
        metadata: &'a [(String, String)],
    ) -> Download<'a> {
        Download {
            ffmpeg,
            playlist_url: url,
            audio_url: Some("fake://audio"),
            metadata,
            final_path,
            duration_secs: Some(300.0),
            stop_grace: Duration::from_secs(5),
        }
    }

    #[test]
    fn args_mirror_ruvsarpur() {
        let meta = vec![("comment".to_string(), "ruvinfo:p:1".to_string())];
        let final_path = Path::new("/x/Þáttur.mp4");
        let d = spec("ffmpeg", "https://v/1080p.m3u8", final_path, &meta);
        let args: Vec<String> = build_args(&d, &part_path(final_path))
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let joined = args.join(" ");
        assert!(joined.contains("-progress pipe:1"));
        assert!(joined.contains(
            "-i https://v/1080p.m3u8 -i fake://audio -map 0:v:0 -map 1:a:0 -c copy -bsf:a aac_adtstoasc -metadata comment=ruvinfo:p:1 -f mp4 /x/Þáttur.mp4.part"
        ));
        assert_eq!(args.last().unwrap(), "/x/Þáttur.mp4.part");
    }

    #[test]
    fn parses_progress() {
        assert_eq!(progress_percent("out_time_us=150000000", 300.0), Some(50));
        assert_eq!(progress_percent("out_time_ms=299999999", 300.0), Some(99));
        assert_eq!(progress_percent("out_time_us=900000000", 300.0), Some(99));
        assert_eq!(progress_percent("out_time_us=N/A", 300.0), None);
        assert_eq!(progress_percent("total_size=10", 300.0), None);
        assert_eq!(progress_percent("out_time_us=1", 0.0), None);
    }

    async fn run(url: &str, final_path: &Path, stop_after: Option<Duration>) -> (Outcome, Vec<u8>) {
        let ffmpeg = fake_ffmpeg();
        let d = spec(&ffmpeg, url, final_path, &[]);
        let (ptx, mut prx) = mpsc::unbounded_channel();
        let (stop_tx, mut stop_rx) = watch::channel(None);
        if let Some(after) = stop_after {
            tokio::spawn(async move {
                tokio::time::sleep(after).await;
                let _ = stop_tx.send(Some(Stop::Cancel));
                tokio::time::sleep(Duration::from_secs(30)).await;
            });
        }
        let outcome = download(&d, ptx, &mut stop_rx).await.unwrap();
        let mut seen = Vec::new();
        while let Ok(p) = prx.try_recv() {
            seen.push(p);
        }
        (outcome, seen)
    }

    #[tokio::test]
    async fn success_renames_part() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("Þáttur s01e01.mp4");
        let (outcome, progress) = run("fake://ok", &final_path, None).await;
        assert_eq!(outcome, Outcome::Completed);
        assert!(final_path.is_file());
        assert!(!part_path(&final_path).exists());
        assert_eq!(progress, vec![0, 25, 50, 99]);
    }

    #[tokio::test]
    async fn failure_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("Bilun.mp4");
        let (outcome, _) = run("fake://fail", &final_path, None).await;
        match outcome {
            Outcome::Failed { exit, message } => {
                assert_eq!(exit, Some(1));
                assert!(message.contains("404"), "{message}");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(!final_path.exists());
        assert!(!part_path(&final_path).exists());
    }

    #[tokio::test]
    async fn cancel_removes_part() {
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("Hægt.mp4");
        let (outcome, progress) =
            run("fake://slow", &final_path, Some(Duration::from_millis(500))).await;
        assert_eq!(outcome, Outcome::Stopped(Stop::Cancel));
        assert!(!progress.is_empty());
        assert!(!final_path.exists());
        assert!(!part_path(&final_path).exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
