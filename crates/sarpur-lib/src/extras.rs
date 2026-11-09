//! What ruvsarpur downloads next to a finished video:
//!
//! - **Subtitles** (both modes): each track as `<video>.<name>.vtt`, plus an
//!   `.srt` converted from it (`downloadSubtitlesFiles`). A request can limit
//!   the languages; `none` means no subtitles (`--subtitlelanguages`).
//! - **Posters** (library mode only, `--plex`): for a movie, `poster.jpg` in
//!   its folder; for an episode, its image as `<video>.jpg` and the series
//!   poster as `poster.jpg` two folders up, never overwritten. Nothing is
//!   written into the category folder itself.
//!
//! Failures here never fail the job: they are logged and returned as warnings.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ruv::Plan;
use crate::subtitles;

/// One subtitle track from the plan: `{name, value}` (value is the URL).
fn subtitle_tracks(plan: &Plan) -> Vec<(String, String)> {
    plan.subtitles
        .as_array()
        .map(|tracks| {
            tracks
                .iter()
                .filter_map(|t| {
                    let name = t.get("name").and_then(Value::as_str)?.trim();
                    let url = t.get("value").and_then(Value::as_str)?.trim();
                    (!name.is_empty() && !url.is_empty())
                        .then(|| (name.to_string(), url.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The tracks to download: all of them without a language list, none when
/// it holds `none`, else those whose name is in it (case-insensitive).
fn selected(tracks: Vec<(String, String)>, languages: Option<&[String]>) -> Vec<(String, String)> {
    let Some(languages) = languages else {
        return tracks;
    };
    let wanted: Vec<String> = languages.iter().map(|l| l.trim().to_lowercase()).collect();
    if wanted.iter().any(|l| l == "none") {
        return Vec::new();
    }
    tracks
        .into_iter()
        .filter(|(name, _)| wanted.contains(&name.to_lowercase()))
        .collect()
}

/// `<video without .mp4><suffix>`, as ruvsarpur's `split(".mp4")[0]`.
fn beside(video: &Path, suffix: &str) -> PathBuf {
    let text = video.to_string_lossy();
    let stem = text.strip_suffix(".mp4").unwrap_or(&text);
    PathBuf::from(format!("{stem}{suffix}"))
}

fn same_dir(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Downloads `url` to `target` (via `<target>.part`). Only HTTP 200 counts,
/// as ruvsarpur's `download_file`.
async fn download(client: &reqwest::Client, url: &str, target: &Path) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let write = async {
        tokio::fs::write(&part, &bytes).await?;
        tokio::fs::rename(&part, target).await
    };
    if let Err(e) = write.await {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(e.to_string());
    }
    Ok(bytes.to_vec())
}

pub struct Request<'a> {
    pub client: &'a reqwest::Client,
    pub plan: &'a Plan,
    pub video: &'a Path,
    /// `Some(category folder)` for library downloads, which get posters.
    pub library_root: Option<&'a Path>,
    pub languages: Option<&'a [String]>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Fetched {
    /// Subtitle files written, `.srt` first, then any `.vtt` that has no
    /// `.srt` (the order ruvsarpur's GUI listed them for browser downloads).
    pub subtitle_files: Vec<PathBuf>,
    pub posters: Vec<PathBuf>,
    pub warnings: Vec<String>,
}

pub async fn fetch(req: Request<'_>) -> Fetched {
    let mut out = Fetched::default();
    let mut srts = Vec::new();
    let mut vtts_only = Vec::new();
    for (name, url) in selected(subtitle_tracks(req.plan), req.languages) {
        let vtt = beside(req.video, &format!(".{name}.vtt"));
        match download(req.client, &url, &vtt).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                match subtitles::vtt_to_srt(&text) {
                    Some(srt) => {
                        let path = vtt.with_extension("srt");
                        match tokio::fs::write(&path, srt).await {
                            Ok(()) => srts.push(path),
                            Err(e) => {
                                out.warnings
                                    .push(format!("Could not write {}: {e}", path.display()));
                                vtts_only.push(vtt);
                            }
                        }
                    }
                    None => {
                        out.warnings
                            .push(format!("No subtitle cues in {}", vtt.display()));
                        vtts_only.push(vtt);
                    }
                }
            }
            Err(e) => out
                .warnings
                .push(format!("Could not download {name} subtitles: {e}")),
        }
    }
    out.subtitle_files = srts.into_iter().chain(vtts_only).collect();

    if let Some(root) = req.library_root {
        let images = &req.plan.images;
        let poster = images.portrait.as_deref().or(images.series.as_deref());
        let mut wanted: Vec<(String, PathBuf, bool)> = Vec::new();
        if req.plan.content_type == "movie_or_docu" {
            if let (Some(url), Some(dir)) = (poster, req.video.parent()) {
                if !same_dir(dir, root) {
                    wanted.push((url.into(), dir.join("poster.jpg"), true));
                }
            }
        } else {
            if let Some(url) = images.episode.as_deref() {
                wanted.push((url.into(), beside(req.video, ".jpg"), true));
            }
            if let (Some(url), Some(dir)) = (poster, req.video.parent().and_then(Path::parent)) {
                if !same_dir(dir, root) {
                    wanted.push((url.into(), dir.join("poster.jpg"), false));
                }
            }
        }
        for (url, target, overwrite) in wanted {
            if !overwrite && tokio::fs::try_exists(&target).await.unwrap_or(false) {
                continue;
            }
            match download(req.client, &url, &target).await {
                Ok(_) => out.posters.push(target),
                Err(e) => out
                    .warnings
                    .push(format!("Could not download {}: {e}", target.display())),
            }
        }
    }
    for warning in &out.warnings {
        tracing::warn!("{}: {warning}", req.plan.pid);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ruv::Images;
    use crate::testutil::{MockServer, Reply};
    use serde_json::json;

    fn plan(content_type: &str, base: &str) -> Plan {
        serde_json::from_value(json!({
            "pid": "p1", "sid": "1", "title": "t", "relative_path": "x.mp4",
            "content_type": content_type, "playlist_url": "", "audio_url": null,
            "quality": null, "duration_seconds": null, "metadata": [],
            "subtitles": [
                {"name": "is", "value": format!("{base}/is.vtt")},
                {"name": "en", "value": format!("{base}/missing.vtt")},
                {"name": "pl", "value": format!("{base}/empty.vtt")},
            ],
            "images": {
                "portrait": format!("{base}/portrait.jpg"),
                "series": format!("{base}/series.jpg"),
                "episode": format!("{base}/episode.jpg"),
            },
        }))
        .unwrap()
    }

    async fn server() -> MockServer {
        MockServer::start(|path| match path {
            "/is.vtt" => Reply::ok(
                "text/vtt",
                "WEBVTT\n\n00:01.000 --> 00:02.000\nHalló\n".as_bytes(),
            ),
            "/empty.vtt" => Reply::ok("text/vtt", "WEBVTT\n".as_bytes()),
            "/portrait.jpg" => Reply::ok("image/jpeg", "portrait".as_bytes()),
            "/episode.jpg" => Reply::ok("image/jpeg", "episode".as_bytes()),
            _ => Reply::status(404),
        })
        .await
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn language_selection() {
        let tracks = || vec![("IS".into(), "a".into()), ("en".into(), "b".into())];
        assert_eq!(selected(tracks(), None).len(), 2);
        assert_eq!(selected(tracks(), Some(&["is".into()])).len(), 1);
        assert!(selected(tracks(), Some(&["none".into(), "is".into()])).is_empty());
        assert!(selected(tracks(), Some(&[])).is_empty());
    }

    #[tokio::test]
    async fn episode_in_the_library() {
        let mock = server().await;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("TV Shows");
        let video = root.join("Þáttur/Season 01/Þáttur - s01e01.mp4");
        std::fs::create_dir_all(video.parent().unwrap()).unwrap();
        std::fs::write(&video, b"video").unwrap();
        let plan = plan("show", &mock.url);
        let client = crate::plex::http_client();

        let got = fetch(Request {
            client: &client,
            plan: &plan,
            video: &video,
            library_root: Some(&root),
            languages: None,
        })
        .await;
        let season = video.parent().unwrap();
        assert_eq!(
            got.subtitle_files,
            [
                season.join("Þáttur - s01e01.is.srt"),
                season.join("Þáttur - s01e01.pl.vtt"),
            ]
        );
        assert_eq!(
            read(&season.join("Þáttur - s01e01.is.srt")),
            "1\n00:00:01,000 --> 00:00:02,000\nHalló\n\n"
        );
        assert!(season.join("Þáttur - s01e01.is.vtt").is_file());
        assert_eq!(got.warnings.len(), 2, "{:?}", got.warnings);
        assert_eq!(read(&season.join("Þáttur - s01e01.jpg")), "episode");
        assert_eq!(read(&root.join("Þáttur/poster.jpg")), "portrait");
        assert!(!root.join("poster.jpg").exists());

        // The series poster is never overwritten; only the chosen languages.
        std::fs::write(root.join("Þáttur/poster.jpg"), b"mine").unwrap();
        let got = fetch(Request {
            client: &client,
            plan: &plan,
            video: &video,
            library_root: Some(&root),
            languages: Some(&["none".into()]),
        })
        .await;
        assert!(got.subtitle_files.is_empty());
        assert_eq!(got.posters, [season.join("Þáttur - s01e01.jpg")]);
        assert_eq!(read(&root.join("Þáttur/poster.jpg")), "mine");
        // No .part files are left behind.
        let leftovers: Vec<_> = walk(dir.path())
            .into_iter()
            .filter(|p| p.to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    #[tokio::test]
    async fn movies_and_browser_downloads() {
        let mock = server().await;
        let dir = tempfile::tempdir().unwrap();
        let client = crate::plex::http_client();
        let mut movie = plan("movie_or_docu", &mock.url);
        movie.images = Images {
            portrait: None,
            series: Some(format!("{}/portrait.jpg", mock.url)),
            episode: None,
        };

        // A movie in its own folder gets poster.jpg there.
        let root = dir.path().join("Movies");
        let video = root.join("Mynd (2020)/Mynd (2020).mp4");
        std::fs::create_dir_all(video.parent().unwrap()).unwrap();
        let got = fetch(Request {
            client: &client,
            plan: &movie,
            video: &video,
            library_root: Some(&root),
            languages: Some(&["IS".into()]),
        })
        .await;
        assert_eq!(got.posters, [root.join("Mynd (2020)/poster.jpg")]);
        assert_eq!(got.subtitle_files.len(), 1);

        // A movie straight in the category folder gets none.
        let flat = root.join("Mynd.mp4");
        let got = fetch(Request {
            client: &client,
            plan: &movie,
            video: &flat,
            library_root: Some(&root),
            languages: Some(&["none".into()]),
        })
        .await;
        assert!(got.posters.is_empty());

        // Browser downloads get subtitles but no posters.
        let web = dir.path().join("web/job1/Mynd.mp4");
        std::fs::create_dir_all(web.parent().unwrap()).unwrap();
        let got = fetch(Request {
            client: &client,
            plan: &movie,
            video: &web,
            library_root: None,
            languages: None,
        })
        .await;
        assert!(got.posters.is_empty());
        assert_eq!(
            got.subtitle_files[0],
            dir.path().join("web/job1/Mynd.is.srt")
        );
    }

    fn walk(dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                out.extend(walk(&path));
            } else {
                out.push(path);
            }
        }
        out
    }
}
