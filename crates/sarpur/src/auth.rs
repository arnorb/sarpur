//! Optional password protection (`SARPUR_PASSWORD`, plan §3.7).
//!
//! When a password is set, every `/api` route except `health`, `session`,
//! `login` and `logout` needs the session cookie; without it the answer is
//! 401. The UI's own files are always served, so it can show the login form.
//!
//! The cookie is stateless: `<id>.<sha256(secret, id, password)>`, where the
//! secret is kept in `<data>/session-secret`. Sessions survive restarts, and
//! changing the password ends all of them.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Json;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub const COOKIE: &str = "sarpur_session";
const MAX_AGE_SECS: u64 = 90 * 24 * 3600;

pub struct Auth {
    password: Option<String>,
    secret: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Compares in time that doesn't depend on where the inputs differ.
fn same(a: &str, b: &str) -> bool {
    let (a, b) = (Sha256::digest(a.as_bytes()), Sha256::digest(b.as_bytes()));
    a.iter()
        .zip(b.iter())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

impl Auth {
    /// No password: everything is open.
    pub fn open() -> Arc<Auth> {
        Arc::new(Auth {
            password: None,
            secret: String::new(),
        })
    }

    /// Reads `SARPUR_PASSWORD`; the secret is created on first use.
    pub fn from_env(data_dir: &Path) -> std::io::Result<Arc<Auth>> {
        let password = std::env::var("SARPUR_PASSWORD")
            .ok()
            .filter(|p| !p.is_empty());
        Self::with_password(password, data_dir)
    }

    pub fn with_password(password: Option<String>, data_dir: &Path) -> std::io::Result<Arc<Auth>> {
        if password.is_none() {
            return Ok(Self::open());
        }
        let path = data_dir.join("session-secret");
        let secret = match std::fs::read_to_string(&path) {
            Ok(s) if s.trim().len() >= 32 => s.trim().to_string(),
            _ => {
                let s = format!(
                    "{}{}",
                    uuid::Uuid::new_v4().simple(),
                    uuid::Uuid::new_v4().simple()
                );
                std::fs::write(&path, &s)?;
                s
            }
        };
        Ok(Arc::new(Auth { password, secret }))
    }

    pub fn required(&self) -> bool {
        self.password.is_some()
    }

    fn signature(&self, id: &str) -> String {
        let password = self.password.as_deref().unwrap_or("");
        hex(&Sha256::digest(
            format!("{}\0{id}\0{password}", self.secret).as_bytes(),
        ))
    }

    fn new_token(&self) -> String {
        let id = uuid::Uuid::new_v4().simple().to_string();
        format!("{id}.{}", self.signature(&id))
    }

    fn valid(&self, token: &str) -> bool {
        token
            .split_once('.')
            .is_some_and(|(id, sig)| !id.is_empty() && same(sig, &self.signature(id)))
    }

    /// Whether this request may use the API.
    pub fn authenticated(&self, headers: &HeaderMap) -> bool {
        !self.required() || session_cookie(headers).is_some_and(|t| self.valid(&t))
    }
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_string())
}

fn cookie(value: &str, max_age: u64) -> String {
    format!("{COOKIE}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}")
}

/// Routes that work without a session.
fn is_public(path: &str) -> bool {
    !path.starts_with("/api/")
        || matches!(
            path,
            "/api/health" | "/api/session" | "/api/login" | "/api/logout"
        )
}

pub async fn middleware(State(auth): State<Arc<Auth>>, request: Request, next: Next) -> Response {
    if is_public(request.uri().path()) || auth.authenticated(request.headers()) {
        return next.run(request).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Log in first" })),
    )
        .into_response()
}

pub async fn session(State(auth): State<Arc<Auth>>, headers: HeaderMap) -> Json<serde_json::Value> {
    Json(json!({
        "required": auth.required(),
        "authenticated": auth.authenticated(&headers),
    }))
}

#[derive(Deserialize)]
pub struct Login {
    password: String,
}

pub async fn login(State(auth): State<Arc<Auth>>, Json(body): Json<Login>) -> Response {
    let Some(password) = auth.password.as_deref() else {
        return Json(json!({ "required": false, "authenticated": true })).into_response();
    };
    if !same(&body.password, password) {
        // Slows down guessing.
        tokio::time::sleep(Duration::from_millis(500)).await;
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Wrong password" })),
        )
            .into_response();
    }
    (
        [(header::SET_COOKIE, cookie(&auth.new_token(), MAX_AGE_SECS))],
        Json(json!({ "required": true, "authenticated": true })),
    )
        .into_response()
}

pub async fn logout() -> Response {
    (
        [(header::SET_COOKIE, cookie("", 0))],
        Json(json!({ "authenticated": false })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens() {
        let dir = tempfile::tempdir().unwrap();
        let auth = Auth::with_password(Some("lykilorð".into()), dir.path()).unwrap();
        let token = auth.new_token();
        assert!(auth.valid(&token));
        assert!(!auth.valid("x.y"));
        assert!(!auth.valid(&token.replace('.', "")));
        let (id, _) = token.split_once('.').unwrap();
        assert!(!auth.valid(&format!("{id}.{}", "0".repeat(64))));

        // Same secret after a restart: still valid. New password: not.
        let again = Auth::with_password(Some("lykilorð".into()), dir.path()).unwrap();
        assert!(again.valid(&token));
        let changed = Auth::with_password(Some("annað".into()), dir.path()).unwrap();
        assert!(!changed.valid(&token));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("a=b; {COOKIE}={token}").parse().unwrap(),
        );
        assert!(auth.authenticated(&headers));
        assert!(!changed.authenticated(&headers));
        assert!(Auth::open().authenticated(&HeaderMap::new()));
    }

    #[test]
    fn public_routes() {
        assert!(is_public("/"));
        assert!(is_public("/title/123"));
        assert!(is_public("/assets/index.js"));
        assert!(is_public("/api/health"));
        assert!(!is_public("/api/jobs"));
        assert!(!is_public("/api/events"));
        assert!(!is_public("/api/healthz"));
    }
}
