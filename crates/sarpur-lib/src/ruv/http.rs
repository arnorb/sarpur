//! HTTP for the RÚV port: live through `reqwest`, with ruvsarpur's retry
//! policy, or replayed from `fixtures/ruv/` (recorded by `tools/record`)
//! so tests and the differential check run offline.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

/// The browser User-Agent ruvsarpur sends for playlists.
pub const BROWSER_AGENT: &str = "Mozilla/5.0 (Windows NT 6.1; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/63.0.3239.132 Safari/537.36";

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> Result<Value, String> {
        serde_json::from_slice(&self.body).map_err(|e| format!("invalid JSON: {e}"))
    }
}

#[derive(Debug, Clone)]
pub struct Recording {
    status: u16,
    body: Body,
}

#[derive(Debug, Clone)]
enum Body {
    File(PathBuf),
    Bytes(Arc<Vec<u8>>),
}

#[derive(Clone)]
pub enum Http {
    Live(reqwest::Client),
    Replay(Arc<HashMap<(String, String), Recording>>),
}

impl Http {
    pub fn live() -> Http {
        Http::Live(
            reqwest::Client::builder()
                .user_agent(concat!("sarpur/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(10))
                .timeout(Duration::from_secs(60))
                .build()
                .expect("HTTP client"),
        )
    }

    /// Replays the recordings in `dir` (`index.json` and the bodies). A
    /// request that wasn't recorded fails.
    pub fn replay(dir: &Path) -> Result<Http, String> {
        let index_path = dir.join("index.json");
        let index: Value = serde_json::from_slice(
            &std::fs::read(&index_path).map_err(|e| format!("{}: {e}", index_path.display()))?,
        )
        .map_err(|e| format!("{}: {e}", index_path.display()))?;
        let mut map = HashMap::new();
        for entry in index["entries"].as_array().into_iter().flatten() {
            let (Some(method), Some(url), Some(file)) = (
                entry["method"].as_str(),
                entry["url"].as_str(),
                entry["file"].as_str(),
            ) else {
                continue;
            };
            map.insert(
                (method.to_uppercase(), requote(url)),
                Recording {
                    status: entry["status"].as_u64().unwrap_or(200) as u16,
                    body: Body::File(dir.join(file)),
                },
            );
        }
        Ok(Http::Replay(Arc::new(map)))
    }

    /// Canned answers held in memory: `(method, url, status, body)`.
    pub fn canned(entries: impl IntoIterator<Item = (String, String, u16, Vec<u8>)>) -> Http {
        let map = entries
            .into_iter()
            .map(|(method, url, status, body)| {
                (
                    (method.to_uppercase(), requote(&url)),
                    Recording {
                        status,
                        body: Body::Bytes(Arc::new(body)),
                    },
                )
            })
            .collect();
        Http::Replay(Arc::new(map))
    }

    /// The recordings in the repository's `fixtures/ruv/`.
    #[cfg(test)]
    pub fn fixtures() -> Http {
        Http::replay(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ruv")).unwrap()
    }

    pub async fn get(&self, url: &str, user_agent: Option<&str>) -> Result<Response, String> {
        self.send("GET", url, None, user_agent).await
    }

    pub async fn post_json(&self, url: &str, body: &Value) -> Result<Response, String> {
        self.send("POST", url, Some(body), None).await
    }

    async fn send(
        &self,
        method: &str,
        url: &str,
        body: Option<&Value>,
        user_agent: Option<&str>,
    ) -> Result<Response, String> {
        match self {
            Http::Replay(map) => {
                let key = (method.to_string(), requote(url));
                let recording = map
                    .get(&key)
                    .ok_or_else(|| format!("no recording for {method} {url}"))?;
                let body = match &recording.body {
                    Body::File(path) => {
                        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?
                    }
                    Body::Bytes(bytes) => bytes.as_ref().clone(),
                };
                Ok(Response {
                    status: recording.status,
                    body,
                })
            }
            Http::Live(client) => {
                // ruvsarpur's retry session: 5 retries on connection errors
                // and on 500, 502 and 504, backing off 0.3 s × 2ⁿ.
                let mut attempt = 0u32;
                loop {
                    let mut request = if method == "POST" {
                        client.post(url)
                    } else {
                        client.get(url)
                    };
                    if let Some(agent) = user_agent {
                        request = request.header(reqwest::header::USER_AGENT, agent);
                    }
                    if let Some(body) = body {
                        request = request
                            .header(reqwest::header::CONTENT_TYPE, "application/json")
                            .body(body.to_string());
                    }
                    let result = request.send().await;
                    let retry = match &result {
                        Ok(r) => matches!(r.status().as_u16(), 500 | 502 | 504),
                        Err(e) => e.is_connect() || e.is_timeout(),
                    };
                    if retry && attempt < 5 {
                        if attempt > 0 {
                            tokio::time::sleep(Duration::from_millis(300 << (attempt - 1))).await;
                        }
                        attempt += 1;
                        continue;
                    }
                    let response = result.map_err(|e| format!("{method} {url}: {e}"))?;
                    let status = response.status().as_u16();
                    let body = response
                        .bytes()
                        .await
                        .map_err(|e| format!("{method} {url}: {e}"))?
                        .to_vec();
                    return Ok(Response { status, body });
                }
            }
        }
    }
}

/// `requests.utils.requote_uri`: what `requests` sends for a URL, so a
/// recording made through Python matches the URL Rust asks for.
pub fn requote(url: &str) -> String {
    const SAFE: &[u8] = b"!#$%&'()*+,/:;=?@[]~-._";
    // First decode escapes of unreserved characters, as requests does.
    let bytes = url.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
            && let Ok(v) = u8::from_str_radix(&url[i + 1..i + 3], 16)
            && (v.is_ascii_alphanumeric() || b"-._~".contains(&v))
        {
            decoded.push(v);
            i += 3;
            continue;
        }
        decoded.push(bytes[i]);
        i += 1;
    }
    let mut out = String::with_capacity(decoded.len());
    for b in decoded {
        if b.is_ascii_alphanumeric() || SAFE.contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requoting() {
        assert_eq!(
            requote("https://a/b?x={\"y\":1}"),
            "https://a/b?x=%7B%22y%22:1%7D"
        );
        assert_eq!(requote("https://a/%7Euser/%2F"), "https://a/~user/%2F");
        assert_eq!(requote("https://a/Veður"), "https://a/Ve%C3%B0ur");
    }

    #[tokio::test]
    async fn replays_and_refuses_unrecorded() {
        let http = Http::fixtures();
        let r = http
            .get("https://api.ruv.is/api/programs/featured/tv", None)
            .await
            .unwrap();
        assert_eq!(r.status, 200);
        assert!(r.json().unwrap()["panels"].is_array());
        assert!(http.get("https://api.ruv.is/nothing", None).await.is_err());
    }
}
