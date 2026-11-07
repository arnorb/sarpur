//! Plex library refresh, as ruvsarpur's `_run_plex_refresh`, except that the
//! token goes in the `X-Plex-Token` header instead of the URL (so it doesn't
//! end up in proxy or server logs).

use std::time::Duration;

use serde::Serialize;

use crate::settings::Settings;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlexResult {
    /// `ok`, `skipped` or `failed`.
    pub state: &'static str,
    pub message: String,
}

/// The HTTP client sarpur uses for RÚV artwork, subtitles, posters and Plex.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("sarpur/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .expect("HTTP client")
}

/// Asks Plex to rescan the library section (or just `plexLibraryPath` in it).
/// Skipped when the base URL, token or section id is missing.
pub async fn refresh(client: &reqwest::Client, settings: &Settings) -> PlexResult {
    let base = settings.plex_base_url.trim().trim_end_matches('/');
    let token = settings.plex_token.trim();
    let section = settings.plex_library_section_id.trim();
    if base.is_empty() || token.is_empty() || section.is_empty() {
        return PlexResult {
            state: "skipped",
            message: "Plex refresh skipped (missing Plex settings)".into(),
        };
    }
    let mut request = client
        .get(format!("{base}/library/sections/{section}/refresh"))
        .header("X-Plex-Token", token)
        .timeout(Duration::from_secs(10));
    let path = settings.plex_library_path.trim();
    if !path.is_empty() {
        request = request.query(&[("path", path)]);
    }
    let result = match request.send().await {
        Ok(response) if response.status().is_success() => PlexResult {
            state: "ok",
            message: "Plex library refresh triggered".into(),
        },
        Ok(response) => PlexResult {
            state: "failed",
            message: format!("Plex refresh failed (HTTP {})", response.status().as_u16()),
        },
        // Without the URL: reqwest's error text includes it.
        Err(e) => PlexResult {
            state: "failed",
            message: format!("Plex refresh failed: {}", e.without_url()),
        },
    };
    match result.state {
        "ok" => tracing::info!("{}", result.message),
        _ => tracing::warn!("{}", result.message),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{MockServer, Reply};
    use std::path::Path;

    /// Criterion 14: the token is a header, never in the URL.
    #[tokio::test]
    async fn sends_the_token_as_a_header() {
        let plex = MockServer::start(|path| {
            if path.starts_with("/library/sections/3/refresh") {
                Reply::ok("text/plain", "")
            } else {
                Reply::status(404)
            }
        })
        .await;
        let mut settings = Settings::defaults(Path::new("/data"));
        let client = http_client();
        assert_eq!(refresh(&client, &settings).await.state, "skipped");

        settings.plex_base_url = format!("{}/", plex.url);
        settings.plex_token = "s3cret token".into();
        settings.plex_library_section_id = "3".into();
        settings.plex_library_path = "/media/TV Shows/Þáttur".into();
        let result = refresh(&client, &settings).await;
        assert_eq!(result.state, "ok", "{}", result.message);

        let requests = plex.requests();
        assert_eq!(requests.len(), 1);
        let head = &requests[0];
        let request_line = head.lines().next().unwrap();
        assert_eq!(
            request_line,
            "GET /library/sections/3/refresh?path=%2Fmedia%2FTV+Shows%2F%C3%9E%C3%A1ttur HTTP/1.1"
        );
        assert!(!request_line.contains("s3cret"), "{request_line}");
        assert!(
            head.lines()
                .any(|l| l.eq_ignore_ascii_case("x-plex-token: s3cret token")),
            "{head}"
        );

        settings.plex_library_section_id = "9".into();
        let result = refresh(&client, &settings).await;
        assert_eq!(
            (result.state, result.message.as_str()),
            ("failed", "Plex refresh failed (HTTP 404)")
        );
    }
}
