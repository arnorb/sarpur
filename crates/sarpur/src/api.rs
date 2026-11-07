//! The HTTP API under `/api` (see docs/api.md).

use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use sarpur_lib::catalogue::Catalogue;
use sarpur_lib::jobs::{Event, JobError, Mode, Priority, Queue, State as JobState};
use sarpur_lib::settings;
use sarpur_lib::worker::WorkerClient;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::{BroadcastStream, WatchStream};

#[derive(Clone)]
pub struct AppState {
    pub queue: Queue,
    pub catalogue: Arc<Catalogue>,
    pub worker: WorkerClient,
    pub dry_run: bool,
    /// Flips to true on shutdown, which ends the SSE streams.
    pub shutdown: watch::Receiver<bool>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/settings", get(get_settings).post(post_settings))
        .route("/api/catalogue", get(search_catalogue))
        .route("/api/catalogue/status", get(catalogue_status))
        .route("/api/catalogue/refresh", post(refresh_catalogue))
        .route("/api/downloads", post(create_download))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", delete(delete_job))
        .route("/api/jobs/{id}/retry", post(retry_job))
        .route("/api/jobs/{id}/files/{name}", get(job_file))
        .route("/api/events", get(events))
        .with_state(state)
}

pub struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

fn internal(e: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

fn not_found(what: impl std::fmt::Display) -> ApiError {
    ApiError(StatusCode::NOT_FOUND, format!("{what} not found"))
}

impl From<JobError> for ApiError {
    fn from(e: JobError) -> Self {
        match e {
            JobError::Invalid(m) => ApiError(StatusCode::BAD_REQUEST, m),
            JobError::Db(e) => internal(e),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;

async fn health(State(s): State<AppState>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "version": env!("CARGO_PKG_VERSION"),
        "dryRun": s.dry_run,
        "catalogue": s.catalogue.status(),
    }))
}

async fn get_settings(State(s): State<AppState>) -> ApiResult<Json<settings::Settings>> {
    settings::load(s.queue.db(), s.queue.data_dir())
        .await
        .map(Json)
        .map_err(internal)
}

async fn post_settings(
    State(s): State<AppState>,
    Json(patch): Json<Value>,
) -> ApiResult<Json<settings::Settings>> {
    settings::update(s.queue.db(), s.queue.data_dir(), &patch)
        .await
        .map(Json)
        .map_err(|e| match e {
            settings::SettingsError::Invalid(m) => ApiError(StatusCode::BAD_REQUEST, m),
            other => internal(other),
        })
}

#[derive(Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    limit: Option<usize>,
}

async fn search_catalogue(
    State(s): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    Json(json!({
        "items": s.catalogue.search(&query.q, limit),
        "status": s.catalogue.status(),
    }))
}

async fn catalogue_status(State(s): State<AppState>) -> Json<Value> {
    Json(json!(s.catalogue.status()))
}

async fn refresh_catalogue(State(s): State<AppState>) -> (StatusCode, Json<Value>) {
    let catalogue = s.catalogue.clone();
    let worker = s.worker.clone();
    tokio::spawn(async move {
        catalogue.load(&worker, true).await;
    });
    // Give the task a moment to flip the status so the reply shows it.
    tokio::task::yield_now().await;
    (StatusCode::ACCEPTED, Json(json!(s.catalogue.status())))
}

#[derive(Deserialize)]
struct DownloadRequest {
    pid: String,
    mode: Mode,
    #[serde(default, alias = "client_id", rename = "clientId")]
    client_id: Option<String>,
}

async fn create_download(
    State(s): State<AppState>,
    Json(req): Json<DownloadRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let title = s
        .catalogue
        .get(req.pid.trim())
        .map(|i| i.title)
        .unwrap_or_default();
    let client_id = req.client_id.filter(|c| !c.trim().is_empty());
    let job = s
        .queue
        .enqueue(&req.pid, &title, req.mode, Priority::Interactive, client_id)
        .await?;
    Ok((StatusCode::CREATED, Json(json!(job))))
}

async fn list_jobs(State(s): State<AppState>) -> ApiResult<Json<Value>> {
    let jobs = s.queue.list().await.map_err(internal)?;
    Ok(Json(json!({ "jobs": jobs })))
}

async fn delete_job(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if s.queue.get(&id).await.map_err(internal)?.is_none() {
        return Err(not_found(format!("job {id}")));
    }
    let job = s.queue.cancel_or_remove(&id).await?;
    Ok(Json(json!({ "removed": job.is_none(), "job": job })))
}

async fn retry_job(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    if s.queue.get(&id).await.map_err(internal)?.is_none() {
        return Err(not_found(format!("job {id}")));
    }
    Ok(Json(json!(s.queue.retry(&id).await?)))
}

/// `Content-Disposition` with an ASCII fallback and an RFC 5987 `filename*`.
pub fn content_disposition(name: &str) -> String {
    let fallback: String = name
        .chars()
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "attachment; filename=\"{fallback}\"; filename*=UTF-8''{}",
        sarpur_lib::jobs::percent_encode(name)
    )
}

async fn job_file(
    State(s): State<AppState>,
    Path((id, name)): Path<(String, String)>,
) -> ApiResult<Response> {
    let job = s
        .queue
        .get(&id)
        .await
        .map_err(internal)?
        .ok_or_else(|| not_found(format!("job {id}")))?;
    // Only names the job itself lists, so the path can't escape its folder.
    if job.mode != Mode::Web
        || job.state != JobState::Completed
        || !job.files.iter().any(|f| f.name == name)
    {
        return Err(not_found("file"));
    }
    let path: PathBuf = s.queue.web_dir().join(&job.id).join(&name);
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|_| not_found("file (it may have expired)"))?;
    let len = file.metadata().await.map_err(internal)?.len();
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file));
    Response::builder()
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CONTENT_LENGTH, len)
        .header(header::CONTENT_DISPOSITION, content_disposition(&name))
        .body(body)
        .map_err(internal)
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(alias = "clientId")]
    client_id: Option<String>,
}

async fn events(
    State(s): State<AppState>,
    Query(query): Query<EventsQuery>,
) -> Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>> {
    let client_id = query.client_id.filter(|c| !c.is_empty());
    let events = BroadcastStream::new(s.queue.subscribe())
        .filter_map(move |event| {
            let event: Event = event.ok()?;
            event
                .visible_to(client_id.as_deref())
                .then(|| {
                    SseEvent::default()
                        .event(event.name())
                        .json_data(&event)
                        .ok()
                })
                .flatten()
        })
        .map(Some);
    let stop = WatchStream::new(s.shutdown.clone())
        .filter(|down| *down)
        .map(|_| None);
    let stream = events
        .merge(stop)
        .take_while(Option::is_some)
        .map(|event| Ok(event.expect("checked by take_while")));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use sarpur_lib::db::Db;
    use sarpur_lib::jobs::QueueConfig;
    use std::time::Duration;
    use tower::ServiceExt;

    fn testdata() -> PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sarpur-lib/testdata")
    }

    async fn call(app: &Router, req: Request<Body>) -> (StatusCode, Response) {
        let res = app.clone().oneshot(req).await.unwrap();
        (res.status(), res)
    }

    async fn json_body(res: Response) -> Value {
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn web_download_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
        let worker = WorkerClient::new(
            sarpur_lib::config::python(),
            testdata().join("fake_worker.py"),
            dir.path(),
        );
        let queue = Queue::new(
            db,
            worker.clone(),
            QueueConfig {
                data_dir: dir.path().to_path_buf(),
                ffmpeg: testdata()
                    .join("fake_ffmpeg.py")
                    .to_string_lossy()
                    .into_owned(),
                dry_run: false,
                stop_grace: Duration::from_secs(5),
                free_space: Arc::new(|_| Ok(u64::MAX)),
            },
        );
        let runner = queue.spawn_runner();
        let catalogue = Arc::new(Catalogue::new());
        catalogue.load(&worker, false).await;
        let (_shutdown_tx, shutdown) = watch::channel(false);
        let app = router(AppState {
            queue: queue.clone(),
            catalogue,
            worker,
            dry_run: false,
            shutdown,
        });

        let (status, res) = call(
            &app,
            Request::get("/api/catalogue?q=%C3%BE%C3%A1ttur")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let found = json_body(res).await;
        assert_eq!(found["items"][0]["pid"], "web1");
        assert_eq!(found["items"][0]["seriesTitle"], "Þáttur");

        let (status, res) = call(
            &app,
            Request::post("/api/settings")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"watchlistSids":["1"],"quality":"HD720"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let saved = json_body(res).await;
        assert_eq!(saved["quality"], "HD720");
        assert!(saved.get("watchlistSids").is_none());

        let (status, res) = call(
            &app,
            Request::post("/api/downloads")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"pid":"web1","mode":"web","clientId":"browser-a"}"#,
                ))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
        let job = json_body(res).await;
        assert_eq!(job["state"], "queued");
        assert_eq!(job["title"], "Þáttur (1 af 2)");
        let id = job["id"].as_str().unwrap().to_string();

        let mut done = Value::Null;
        for _ in 0..200 {
            let (_, res) = call(&app, Request::get("/api/jobs").body(Body::empty()).unwrap()).await;
            let jobs = json_body(res).await;
            let job = jobs["jobs"][0].clone();
            if job["state"] == "completed" || job["state"] == "failed" {
                done = job;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(done["state"], "completed", "{done}");
        let url = done["files"][0]["url"].as_str().unwrap().to_string();

        let (status, res) = call(&app, Request::get(&url).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            res.headers()[header::CONTENT_DISPOSITION],
            "attachment; filename=\"__ttur s01e01.mp4\"; filename*=UTF-8''%C3%9E%C3%A1ttur%20s01e01.mp4"
        );
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(bytes.len(), 1024);

        // Names the job doesn't list are refused.
        let (status, _) = call(
            &app,
            Request::get(format!("/api/jobs/{id}/files/..%2Fsarpur.db"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // Removing a finished job.
        let (status, res) = call(
            &app,
            Request::delete(format!("/api/jobs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json_body(res).await["removed"], true);
        let (status, _) = call(
            &app,
            Request::delete(format!("/api/jobs/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, _) = call(
            &app,
            Request::post("/api/downloads")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"pid":"web1","mode":"elsewhere"}"#))
                .unwrap(),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        queue.shutdown(runner).await;
    }

    /// Reads SSE text from `body` until `until` appears in it (or 20 s pass).
    async fn read_sse_until(body: &mut Body, until: &str) -> String {
        let mut text = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        while !text.contains(until) {
            let frame = tokio::time::timeout_at(deadline, body.frame())
                .await
                .expect("SSE timed out")
                .expect("stream ended")
                .unwrap();
            if let Ok(data) = frame.into_data() {
                text.push_str(&String::from_utf8_lossy(&data));
            }
        }
        text
    }

    #[tokio::test]
    async fn sse_file_ready_goes_only_to_the_matching_client() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
        let worker = WorkerClient::new(
            sarpur_lib::config::python(),
            testdata().join("fake_worker.py"),
            dir.path(),
        );
        let queue = Queue::new(
            db,
            worker.clone(),
            QueueConfig {
                data_dir: dir.path().to_path_buf(),
                ffmpeg: testdata()
                    .join("fake_ffmpeg.py")
                    .to_string_lossy()
                    .into_owned(),
                dry_run: false,
                stop_grace: Duration::from_secs(5),
                free_space: Arc::new(|_| Ok(u64::MAX)),
            },
        );
        let runner = queue.spawn_runner();
        let (shutdown_tx, shutdown) = watch::channel(false);
        let app = router(AppState {
            queue: queue.clone(),
            catalogue: Arc::new(Catalogue::new()),
            worker,
            dry_run: false,
            shutdown,
        });
        let (_, a) = call(
            &app,
            Request::get("/api/events?client_id=a")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let (_, b) = call(
            &app,
            Request::get("/api/events?client_id=b")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
        let mut a = a.into_body();
        let mut b = b.into_body();

        let job = queue
            .enqueue(
                "web1",
                "",
                Mode::Web,
                Priority::Interactive,
                Some("a".into()),
            )
            .await
            .unwrap();
        let a_text = read_sse_until(&mut a, "event: file_ready").await;
        assert!(a_text.contains(&format!("\"jobId\":\"{}\"", job.id)));
        assert!(a_text.contains("event: job"));
        // b sees the job complete, but no file_ready.
        let b_text = read_sse_until(&mut b, "\"state\":\"completed\"").await;
        assert!(!b_text.contains("file_ready"), "{b_text}");

        // Shutdown ends the streams.
        shutdown_tx.send(true).unwrap();
        let end = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(frame) = b.frame().await {
                let data = frame.unwrap().into_data().unwrap_or_default();
                assert!(!String::from_utf8_lossy(&data).contains("file_ready"));
            }
        })
        .await;
        assert!(end.is_ok(), "SSE stream did not end on shutdown");
        queue.shutdown(runner).await;
    }

    #[test]
    fn file_ready_reaches_only_its_client() {
        let ready = Event::FileReady {
            client_id: "a".into(),
            job_id: "j".into(),
            name: "n.mp4".into(),
            url: "/api/jobs/j/files/n.mp4".into(),
        };
        assert!(ready.visible_to(Some("a")));
        assert!(!ready.visible_to(Some("b")));
        assert!(!ready.visible_to(None));
        assert_eq!(ready.name(), "file_ready");
    }
}
