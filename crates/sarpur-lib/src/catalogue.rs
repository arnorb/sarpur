//! The catalogue, kept in memory. It is loaded from the worker's `catalogue`
//! output in the background at startup and after a refresh.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use serde_json::Value;

use crate::worker::WorkerClient;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueItem {
    pub pid: String,
    pub sid: String,
    pub title: String,
    pub series_title: String,
    pub showtime: String,
    pub duration: Option<u64>,
    pub content_type: &'static str,
    pub english_subtitled: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueStatus {
    /// `empty`, `loading`, `refreshing`, `ready` or `failed`.
    pub state: &'static str,
    pub count: usize,
    pub loaded_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Catalogue {
    items: RwLock<Arc<Vec<CatalogueItem>>>,
    status: Mutex<CatalogueStatus>,
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// Same rule as `gui_api._content_type_from_item`.
pub fn content_type(value: &Value) -> &'static str {
    let flag = |key: &str| value.get(key).and_then(Value::as_bool).unwrap_or(false);
    if flag("is_movie") || flag("is_docu") {
        "movie_or_docu"
    } else if flag("is_sport") {
        "sport"
    } else {
        "show"
    }
}

/// The worker's saved schedule in the data folder (`--portable`).
pub fn schedule_file(data_dir: &Path) -> PathBuf {
    data_dir.join("tvschedule.json")
}

impl CatalogueItem {
    pub fn from_worker(value: &Value) -> Option<CatalogueItem> {
        let pid = text(value, "pid");
        if pid.is_empty() {
            return None;
        }
        let duration = match value.get("duration") {
            Some(Value::Number(n)) => n.as_f64().map(|d| d as u64),
            Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|d| d as u64),
            _ => None,
        };
        Some(CatalogueItem {
            pid,
            sid: text(value, "sid"),
            title: text(value, "title"),
            series_title: text(value, "series_title"),
            showtime: text(value, "showtime"),
            duration,
            content_type: content_type(value),
            english_subtitled: value
                .get("english_subtitled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

impl Catalogue {
    pub fn new() -> Catalogue {
        let catalogue = Catalogue::default();
        catalogue.status.lock().unwrap().state = "empty";
        catalogue
    }

    pub fn status(&self) -> CatalogueStatus {
        self.status.lock().unwrap().clone()
    }

    pub fn get(&self, pid: &str) -> Option<CatalogueItem> {
        self.items
            .read()
            .unwrap()
            .iter()
            .find(|i| i.pid == pid)
            .cloned()
    }

    pub fn set_items(&self, values: &[Value]) {
        let mut items: Vec<CatalogueItem> = values
            .iter()
            .filter_map(CatalogueItem::from_worker)
            .collect();
        items.sort_by(|a, b| b.showtime.cmp(&a.showtime).then_with(|| a.pid.cmp(&b.pid)));
        let count = items.len();
        *self.items.write().unwrap() = Arc::new(items);
        let mut status = self.status.lock().unwrap();
        status.state = "ready";
        status.count = count;
        status.loaded_at = Some(crate::now_iso());
        status.error = None;
    }

    /// Case-insensitive substring search over title and series title, newest
    /// first. An empty query lists everything.
    pub fn search(&self, query: &str, limit: usize) -> Vec<CatalogueItem> {
        let query = query.trim().to_lowercase();
        let items = self.items.read().unwrap().clone();
        items
            .iter()
            .filter(|i| {
                query.is_empty()
                    || i.title.to_lowercase().contains(&query)
                    || i.series_title.to_lowercase().contains(&query)
            })
            .take(limit)
            .cloned()
            .collect()
    }

    /// The startup load: reads the saved schedule, or fetches it from RÚV when
    /// the data folder has none yet.
    pub async fn load_at_startup(&self, worker: &WorkerClient, data_dir: &Path) -> bool {
        let saved = tokio::fs::try_exists(schedule_file(data_dir))
            .await
            .unwrap_or(false);
        if !saved {
            tracing::info!("no saved catalogue; fetching it from RÚV (takes minutes)");
        }
        self.load(worker, !saved).await
    }

    /// Loads (or with `refresh`, fetches from RÚV and then loads) the catalogue.
    /// Returns false without doing anything if a load is already running.
    pub async fn load(&self, worker: &WorkerClient, refresh: bool) -> bool {
        {
            let mut status = self.status.lock().unwrap();
            if matches!(status.state, "loading" | "refreshing") {
                return false;
            }
            status.state = if refresh { "refreshing" } else { "loading" };
            status.error = None;
        }
        match worker.catalogue(refresh).await {
            Ok(values) => {
                self.set_items(&values);
                tracing::info!("catalogue loaded: {} items", self.status().count);
            }
            Err(e) => {
                tracing::warn!("catalogue load failed: {e}");
                let mut status = self.status.lock().unwrap();
                status.state = "failed";
                status.error = Some(e.to_string());
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[tokio::test]
    async fn loads_and_searches() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let worker = WorkerClient::new(crate::config::python(), dir.join("fake_worker.py"), &dir);
        let catalogue = Catalogue::new();
        assert_eq!(catalogue.status().state, "empty");
        assert!(catalogue.load(&worker, false).await);
        assert_eq!(catalogue.status().state, "ready");
        assert_eq!(catalogue.status().count, 2);

        let hits = catalogue.search("ÞÁTT", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].duration, Some(300));
        assert_eq!(hits[0].content_type, "show");
        let movie = catalogue.get("mov1").unwrap();
        assert_eq!(movie.content_type, "movie_or_docu");
        assert_eq!(movie.duration, Some(5400));
        assert_eq!(catalogue.search("", 1).len(), 1);
    }

    #[tokio::test]
    async fn startup_fetches_when_no_schedule_is_saved() {
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let worker = WorkerClient::new(
            crate::config::python(),
            testdata.join("fake_worker.py"),
            &testdata,
        );
        let data = tempfile::tempdir().unwrap();

        let catalogue = Catalogue::new();
        assert!(catalogue.load_at_startup(&worker, data.path()).await);
        assert_eq!(catalogue.status().count, 3, "fetched with --refresh");

        std::fs::write(schedule_file(data.path()), "{}").unwrap();
        let catalogue = Catalogue::new();
        assert!(catalogue.load_at_startup(&worker, data.path()).await);
        assert_eq!(catalogue.status().count, 2, "read the saved schedule");
    }
}
