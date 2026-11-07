//! The UI, built from `web/` (`npm run build` writes `web/dist`) and embedded
//! in the binary. Paths that aren't files get `index.html`, so client routes
//! such as `/title/123` work on reload; unknown `/api` paths stay 404s.

use axum::body::Body;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};

#[derive(rust_embed::Embed)]
#[folder = "$CARGO_MANIFEST_DIR/../../web/dist"]
#[allow_missing = true]
struct Assets;

const NOT_BUILT: &str = "<!doctype html><meta charset=utf-8><title>sarpur</title>\
<p>The sarpur UI isn't built into this binary. Run <code>npm ci &amp;&amp; npm run build</code> \
in <code>web/</code>, then build sarpur again. The API is at <code>/api</code>.</p>";

fn file(path: &str) -> Option<Response> {
    let asset = Assets::get(path)?;
    // Vite's hashed files never change; index.html must always be fresh.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Some(
        (
            [
                (header::CONTENT_TYPE, asset.metadata.mimetype().to_string()),
                (header::CACHE_CONTROL, cache.to_string()),
            ],
            Body::from(asset.data.into_owned()),
        )
            .into_response(),
    )
}

pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path == "api" || path.starts_with("api/") {
        return (
            StatusCode::NOT_FOUND,
            axum::Json(serde_json::json!({ "error": "no such API route" })),
        )
            .into_response();
    }
    if !path.is_empty() {
        if let Some(response) = file(path) {
            return response;
        }
    }
    file("index.html").unwrap_or_else(|| {
        (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            NOT_BUILT,
        )
            .into_response()
    })
}
