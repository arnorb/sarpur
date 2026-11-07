//! The poster proxy behind `/api/poster`, as ruvsarpur's: only `https` URLs
//! on an allowlist of hosts, cached on disk for 24 hours (at most 3,000
//! files), and a stale copy is served when RÚV can't be reached.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use sha2::{Digest, Sha256};

pub const POSTER_TTL: Duration = Duration::from_secs(24 * 3600);
const MAX_FILES: usize = 3000;
const CLEANUP_EVERY: Duration = Duration::from_secs(3600);
pub const ALLOWED_HOSTS: [&str; 3] = [
    "myndir.ruv.is",
    "d38kdhuogyllre.cloudfront.net",
    "m.media-amazon.com",
];

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum PosterError {
    #[error("Invalid or unsupported poster URL")]
    NotAllowed,
    #[error("Poster download failed: {0}")]
    Fetch(String),
}

/// The URL `/api/poster` serves for `url`, or `None` if it isn't allowed.
pub fn proxy_path(url: &str) -> Option<String> {
    let url = normalize(url, &ALLOWED_HOSTS, true)?;
    Some(format!(
        "/api/poster?url={}",
        crate::jobs::percent_encode(&url)
    ))
}

fn normalize(url: &str, hosts: &[&str], https_only: bool) -> Option<String> {
    let parsed = reqwest::Url::parse(url.trim()).ok()?;
    let scheme_ok = parsed.scheme() == "https" || (!https_only && parsed.scheme() == "http");
    let host = parsed.host_str()?.to_ascii_lowercase();
    (scheme_ok && hosts.contains(&host.as_str())).then(|| parsed.to_string())
}

/// The content type of an image, from its first bytes.
pub fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else {
        "application/octet-stream"
    }
}

pub struct PosterCache {
    dir: PathBuf,
    client: reqwest::Client,
    hosts: Vec<&'static str>,
    https_only: bool,
    last_cleanup: Mutex<Option<Instant>>,
}

fn age(path: &Path) -> Option<Duration> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(
        SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default(),
    )
}

impl PosterCache {
    pub fn new(dir: PathBuf, client: reqwest::Client) -> PosterCache {
        PosterCache {
            dir,
            client,
            hosts: ALLOWED_HOSTS.to_vec(),
            https_only: true,
            last_cleanup: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn for_tests(dir: PathBuf, host: &'static str) -> PosterCache {
        PosterCache {
            hosts: vec![host],
            https_only: false,
            ..PosterCache::new(dir, crate::plex::http_client())
        }
    }

    fn cache_file(&self, url: &str) -> PathBuf {
        let path = reqwest::Url::parse(url)
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        let ext = Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .filter(|e| ["jpg", "jpeg", "png", "webp"].contains(&e.as_str()))
            .unwrap_or_else(|| "jpg".into());
        let digest: String = Sha256::digest(url.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        self.dir.join(format!("{digest}.{ext}"))
    }

    /// The poster's bytes and content type, from the cache when it is fresh.
    pub async fn get(&self, url: &str) -> Result<(Vec<u8>, &'static str), PosterError> {
        let url = normalize(url, &self.hosts, self.https_only).ok_or(PosterError::NotAllowed)?;
        self.maybe_cleanup().await;
        let file = self.cache_file(&url);
        let check = file.clone();
        let fresh =
            tokio::task::spawn_blocking(move || age(&check).is_some_and(|a| a <= POSTER_TTL))
                .await
                .unwrap_or(false);
        if fresh {
            if let Ok(bytes) = tokio::fs::read(&file).await {
                let mime = sniff_mime(&bytes);
                return Ok((bytes, mime));
            }
        }
        match self.fetch(&url).await {
            Ok(bytes) => {
                let tmp = file.with_extension("tmp");
                let written = async {
                    tokio::fs::create_dir_all(&self.dir).await?;
                    tokio::fs::write(&tmp, &bytes).await?;
                    tokio::fs::rename(&tmp, &file).await
                }
                .await;
                if let Err(e) = written {
                    tracing::warn!("could not cache poster {}: {e}", file.display());
                    let _ = tokio::fs::remove_file(&tmp).await;
                }
                let mime = sniff_mime(&bytes);
                Ok((bytes, mime))
            }
            Err(e) => match tokio::fs::read(&file).await {
                // Stale artwork is better than none.
                Ok(bytes) => {
                    let mime = sniff_mime(&bytes);
                    Ok((bytes, mime))
                }
                Err(_) => Err(e),
            },
        }
    }

    async fn fetch(&self, url: &str) -> Result<Vec<u8>, PosterError> {
        let response = self
            .client
            .get(url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| PosterError::Fetch(e.without_url().to_string()))?;
        if !response.status().is_success() {
            return Err(PosterError::Fetch(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| PosterError::Fetch(e.without_url().to_string()))?;
        if bytes.is_empty() {
            return Err(PosterError::Fetch("empty file".into()));
        }
        Ok(bytes.to_vec())
    }

    /// Hourly: deletes expired files, then the oldest beyond 3,000.
    async fn maybe_cleanup(&self) {
        {
            let mut last = self.last_cleanup.lock().unwrap();
            if last.is_some_and(|t| t.elapsed() < CLEANUP_EVERY) {
                return;
            }
            *last = Some(Instant::now());
        }
        let dir = self.dir.clone();
        let _ = tokio::task::spawn_blocking(move || cleanup(&dir, MAX_FILES)).await;
    }
}

fn cleanup(dir: &Path, max_files: usize) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<(PathBuf, Duration)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter_map(|p| age(&p).map(|a| (p, a)))
        .collect();
    let mut removed = 0;
    files.retain(|(path, age)| {
        if *age > POSTER_TTL && std::fs::remove_file(path).is_ok() {
            removed += 1;
            return false;
        }
        true
    });
    if files.len() > max_files {
        // Oldest first.
        files.sort_by_key(|f| std::cmp::Reverse(f.1));
        let overflow = files.len() - max_files;
        for (path, _) in files.into_iter().take(overflow) {
            if std::fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{MockServer, Reply};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];

    #[test]
    fn allowlist() {
        let ok = "https://myndir.ruv.is/eyJidWNrZXQiOiJydXYi==";
        assert_eq!(normalize(ok, &ALLOWED_HOSTS, true).as_deref(), Some(ok));
        assert!(normalize("https://MYNDIR.ruv.is/x.jpg", &ALLOWED_HOSTS, true).is_some());
        for bad in [
            "http://myndir.ruv.is/x.jpg",
            "https://evil.example/x.jpg",
            "https://myndir.ruv.is.evil.example/x.jpg",
            "https://evil.example/?u=https://myndir.ruv.is/x.jpg",
            "file:///etc/passwd",
            "not a url",
        ] {
            assert_eq!(normalize(bad, &ALLOWED_HOSTS, true), None, "{bad}");
        }
        assert_eq!(
            proxy_path("https://myndir.ruv.is/a b.jpg").unwrap(),
            "/api/poster?url=https%3A%2F%2Fmyndir.ruv.is%2Fa%2520b.jpg"
        );
        assert_eq!(proxy_path("https://evil.example/x.jpg"), None);
    }

    #[test]
    fn sniffs_types() {
        assert_eq!(sniff_mime(JPEG), "image/jpeg");
        assert_eq!(sniff_mime(b"RIFF\0\0\0\0WEBPVP8 "), "image/webp");
        assert_eq!(sniff_mime(b"\x89PNG\r\n\x1a\n..."), "image/png");
        assert_eq!(sniff_mime(b"<html>"), "application/octet-stream");
    }

    #[tokio::test]
    async fn caches_and_serves_stale_when_down() {
        let up = Arc::new(AtomicBool::new(true));
        let flag = up.clone();
        let server = MockServer::start(move |_| {
            if flag.load(Ordering::SeqCst) {
                Reply::ok("image/jpeg", JPEG)
            } else {
                Reply::status(503)
            }
        })
        .await;
        let dir = tempfile::tempdir().unwrap();
        let cache = PosterCache::for_tests(dir.path().join("poster-cache"), "127.0.0.1");
        let url = format!("{}/poster.jpg", server.url);

        assert_eq!(
            cache.get("https://evil.example/x.jpg").await,
            Err(PosterError::NotAllowed)
        );
        assert_eq!(
            cache.get(&url).await.unwrap(),
            (JPEG.to_vec(), "image/jpeg")
        );
        assert_eq!(cache.get(&url).await.unwrap().0, JPEG);
        assert_eq!(server.requests().len(), 1, "the second came from the cache");

        // Expired and RÚV is down: the stale copy is served.
        up.store(false, Ordering::SeqCst);
        let file = cache.cache_file(&normalize(&url, &["127.0.0.1"], false).unwrap());
        let old = SystemTime::now() - POSTER_TTL - Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(cache.get(&url).await.unwrap().0, JPEG);
        assert_eq!(server.requests().len(), 2);
        // Nothing cached and down: an error.
        let missing = format!("{}/other.jpg", server.url);
        assert!(matches!(
            cache.get(&missing).await,
            Err(PosterError::Fetch(m)) if m == "HTTP 503"
        ));
    }

    #[test]
    fn cleanup_drops_expired_then_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let now = SystemTime::now();
        for (name, age_secs) in [("a", 10), ("b", 20), ("c", 30), ("old", 90_000)] {
            let path = dir.path().join(name);
            std::fs::write(&path, b"x").unwrap();
            std::fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(now - Duration::from_secs(age_secs))
                .unwrap();
        }
        assert_eq!(cleanup(dir.path(), 2), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["a", "b"]);
    }
}
