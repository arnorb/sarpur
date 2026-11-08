//! The update check: once a day, ask GitHub Releases for the latest sarpur
//! release and tell the UI when it's newer than this binary.
//!
//! `SARPUR_UPDATE_REPO` (`owner/name`, default `arnorb/sarpur`; empty turns the
//! check off) picks the repository. A private repository needs
//! `SARPUR_GITHUB_TOKEN`. A failed check is only logged: the banner stays as
//! the last good check left it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_REPO: &str = "arnorb/sarpur";
const INTERVAL: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The newer release's version, when there is one.
    pub available: Option<String>,
    /// The release page.
    pub url: Option<String>,
    pub checked_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

pub struct UpdateChecker {
    client: reqwest::Client,
    /// `https://api.github.com/repos/<repo>/releases/latest`, or `None` when off.
    url: Option<String>,
    token: Option<String>,
    status: Mutex<UpdateStatus>,
}

impl UpdateChecker {
    /// From `SARPUR_UPDATE_REPO` and `SARPUR_GITHUB_TOKEN`.
    pub fn from_env(client: reqwest::Client) -> UpdateChecker {
        let repo = std::env::var("SARPUR_UPDATE_REPO").unwrap_or_else(|_| DEFAULT_REPO.into());
        let repo = repo.trim();
        let url = (!repo.is_empty())
            .then(|| format!("https://api.github.com/repos/{repo}/releases/latest"));
        let token = std::env::var("SARPUR_GITHUB_TOKEN")
            .ok()
            .filter(|t| !t.trim().is_empty());
        UpdateChecker::new(client, url, token)
    }

    pub fn new(client: reqwest::Client, url: Option<String>, token: Option<String>) -> Self {
        UpdateChecker {
            client,
            url,
            token,
            status: Mutex::new(UpdateStatus::default()),
        }
    }

    pub fn status(&self) -> UpdateStatus {
        self.status.lock().unwrap().clone()
    }

    /// Checks now and updates the status.
    pub async fn check(&self) {
        let Some(url) = &self.url else { return };
        let result = self.latest(url).await;
        let mut status = self.status.lock().unwrap();
        status.checked_at = Some(crate::now_iso());
        match result {
            Ok(release) => {
                let version = release.tag_name.trim_start_matches('v').to_string();
                if is_newer(&version, CURRENT) {
                    status.available = Some(version);
                    status.url = Some(release.html_url);
                } else {
                    status.available = None;
                    status.url = None;
                }
                status.error = None;
            }
            Err(e) => {
                tracing::info!("update check: {e}");
                status.error = Some(e);
            }
        }
    }

    async fn latest(&self, url: &str) -> Result<Release, String> {
        let mut request = self
            .client
            .get(url)
            .header("Accept", "application/vnd.github+json")
            .timeout(Duration::from_secs(20));
        if let Some(token) = &self.token {
            request = request.bearer_auth(token);
        }
        let response = request
            .send()
            .await
            .map_err(|e| e.without_url().to_string())?;
        match response.status().as_u16() {
            200 => {
                let body = response
                    .bytes()
                    .await
                    .map_err(|e| e.without_url().to_string())?;
                serde_json::from_slice::<Release>(&body)
                    .map_err(|e| format!("unexpected answer from GitHub: {e}"))
            }
            404 => Err("no published release (or the repository is private)".into()),
            code => Err(format!("GitHub answered HTTP {code}")),
        }
    }

    /// Checks shortly after startup, then daily.
    pub fn spawn(self: &Arc<Self>) {
        if self.url.is_none() {
            return;
        }
        let checker = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let mut tick = tokio::time::interval(INTERVAL);
            loop {
                tick.tick().await;
                checker.check().await;
            }
        });
    }
}

/// Whether `candidate` is a later version than `current`, comparing the
/// numeric `major.minor.patch` parts. Pre-releases (`1.2.0-rc1`) never count.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        if v.contains('-') {
            return None;
        }
        v.split('.').map(|p| p.parse().ok()).collect()
    }
    match (
        parse(candidate),
        parse(current.split('-').next().unwrap_or(current)),
    ) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plex::http_client;
    use crate::testutil::{MockServer, Reply};

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("0.2.0-rc1", "0.1.0"));
        assert!(!is_newer("nightly", "0.1.0"));
    }

    #[tokio::test]
    async fn reports_a_newer_release() {
        let github = MockServer::start(|path| match path {
            "/new" => Reply::ok(
                "application/json",
                r#"{"tag_name":"v99.0.0","html_url":"https://github.com/arnorb/sarpur/releases/tag/v99.0.0","name":"x"}"#,
            ),
            "/same" => Reply::ok(
                "application/json",
                format!(r#"{{"tag_name":"v{CURRENT}","html_url":"u"}}"#),
            ),
            _ => Reply::status(404),
        })
        .await;
        let checker = UpdateChecker::new(
            http_client(),
            Some(format!("{}/new", github.url)),
            Some("t0ken".into()),
        );
        checker.check().await;
        let status = checker.status();
        assert_eq!(status.available.as_deref(), Some("99.0.0"));
        assert_eq!(
            status.url.as_deref(),
            Some("https://github.com/arnorb/sarpur/releases/tag/v99.0.0")
        );
        assert!(status.checked_at.is_some() && status.error.is_none());
        assert!(
            github.requests()[0]
                .lines()
                .any(|l| l.eq_ignore_ascii_case("authorization: Bearer t0ken"))
        );

        // A failed check reports why.
        let failing = UpdateChecker::new(http_client(), Some(format!("{}/gone", github.url)), None);
        failing.check().await;
        assert_eq!(failing.status().available, None);
        assert!(
            failing
                .status()
                .error
                .unwrap()
                .contains("no published release")
        );

        let same = UpdateChecker::new(http_client(), Some(format!("{}/same", github.url)), None);
        same.check().await;
        assert_eq!(same.status().available, None);
        assert_eq!(same.status().error, None);
    }

    #[tokio::test]
    async fn off_when_the_repo_is_empty() {
        let checker = UpdateChecker::new(http_client(), None, None);
        checker.check().await;
        assert_eq!(checker.status(), UpdateStatus::default());
    }
}
