//! The catalogue the UI shows, kept in memory: one [`CatalogueItem`] per
//! episode of the schedule [`Ruv`] keeps. Loaded in the background at
//! startup and after a refresh.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use serde_json::Value;

use crate::ruv::Ruv;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogueItem {
    pub pid: String,
    pub sid: String,
    pub title: String,
    /// The title ruvsarpur shows (`createShowTitle`), e.g. `Séra Brown s01e01`.
    pub display_title: String,
    pub series_title: String,
    pub showtime: String,
    pub duration: Option<u64>,
    pub content_type: &'static str,
    pub english_subtitled: bool,
    /// `/api/poster?url=…` for the portrait, series or episode image.
    pub poster_url: Option<String>,
    /// The subtitle tracks' names, lowercased and sorted.
    pub subtitle_languages: Vec<String>,
    pub episode_description: Option<String>,
    pub series_description: Option<String>,
    /// RÚV's own duration text (`duration_friendly`).
    pub duration_label: Option<String>,
    /// When the episode first aired (`episode.firstrun`, else the showtime).
    pub first_appeared_at: Option<String>,
    /// When RÚV takes it down; `None` when it doesn't say (or says 9999).
    pub expires_at: Option<String>,
    pub web_url: String,
    pub categories: Vec<String>,
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

/// What the catalogue says about one series id.
#[derive(Debug, Clone, PartialEq)]
pub struct SeriesInfo {
    pub series_title: String,
    pub english_subtitled: bool,
}

#[derive(Default)]
pub struct Catalogue {
    items: RwLock<Arc<Vec<CatalogueItem>>>,
    status: Mutex<CatalogueStatus>,
}

fn opt_text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub use crate::ruv::content_type;

/// The saved schedule in the data folder.
pub fn schedule_file(data_dir: &Path) -> PathBuf {
    data_dir.join(crate::ruv::SCHEDULE_FILE)
}

impl CatalogueItem {
    pub fn from_item(value: &Value) -> Option<CatalogueItem> {
        let pid = text(value, "pid");
        if pid.is_empty() {
            return None;
        }
        let duration = match value.get("duration") {
            Some(Value::Number(n)) => n.as_f64().map(|d| d as u64),
            Some(Value::String(s)) => s.trim().parse::<f64>().ok().map(|d| d as u64),
            _ => None,
        };
        // gui_api's choice: the first of portrait, series and episode image
        // that is set, if the proxy allows it.
        let poster_url = ["portrait_image", "series_image", "episode_image"]
            .iter()
            .filter_map(|key| value.get(*key).and_then(Value::as_str))
            .find(|url| !url.trim().is_empty())
            .and_then(crate::posters::proxy_path);
        let mut subtitle_languages: Vec<String> = value
            .get("subtitles")
            .and_then(Value::as_array)
            .map(|tracks| {
                tracks
                    .iter()
                    .filter_map(|t| t.get("name").and_then(Value::as_str))
                    .map(|n| n.trim().to_lowercase())
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        subtitle_languages.sort();
        subtitle_languages.dedup();
        // Same fields as gui_api's /api/shows.
        let episode = value.get("episode").filter(|e| e.is_object());
        let expires_at = episode
            .and_then(|e| {
                e.pointer("/files/vodmp4/expires")
                    .and_then(Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| e.get("file_expires").and_then(Value::as_str))
            })
            .map(str::trim)
            .filter(|s| !s.is_empty() && !s.starts_with("9999-"))
            .map(str::to_string);
        let first_appeared_at = episode
            .and_then(|e| opt_text(e, "firstrun"))
            .or_else(|| opt_text(value, "showtime"));
        let slug = text(value, "slug");
        let web_url = if slug.trim().is_empty() {
            "https://www.ruv.is/sjonvarp".to_string()
        } else {
            format!("https://www.ruv.is/sjonvarp/spila/{}/{pid}", slug.trim())
        };
        let categories = value
            .get("categories")
            .and_then(Value::as_array)
            .map(|c| {
                c.iter()
                    .filter_map(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let title = text(value, "title");
        Some(CatalogueItem {
            display_title: crate::ruv::naming::show_title(value, false, false),
            episode_description: opt_text(value, "desc"),
            series_description: opt_text(value, "series_sdesc")
                .or_else(|| opt_text(value, "series_desc")),
            duration_label: opt_text(value, "duration_friendly"),
            first_appeared_at,
            expires_at,
            web_url,
            categories,
            pid,
            sid: text(value, "sid"),
            title,
            series_title: text(value, "series_title"),
            showtime: text(value, "showtime"),
            duration,
            content_type: content_type(value),
            english_subtitled: value
                .get("english_subtitled")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            poster_url,
            subtitle_languages,
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

    /// Each series id with its series title (or the episode title when the
    /// series title is empty) and English flag, from its newest episode.
    pub fn series_index(&self) -> HashMap<String, SeriesInfo> {
        let items = self.items.read().unwrap().clone();
        let mut index = HashMap::new();
        for item in items.iter().filter(|i| !i.sid.is_empty()) {
            index.entry(item.sid.clone()).or_insert_with(|| SeriesInfo {
                series_title: if item.series_title.trim().is_empty() {
                    item.title.clone()
                } else {
                    item.series_title.clone()
                },
                english_subtitled: item.english_subtitled,
            });
        }
        index
    }

    /// The episodes of the given series ids, newest first.
    pub fn items_for_sids(&self, sids: &HashSet<String>) -> Vec<CatalogueItem> {
        let items = self.items.read().unwrap().clone();
        items
            .iter()
            .filter(|i| sids.contains(&i.sid))
            .cloned()
            .collect()
    }

    /// Every item, newest first.
    pub fn all(&self) -> Arc<Vec<CatalogueItem>> {
        self.items.read().unwrap().clone()
    }

    /// The items for these pids, newest first (unknown pids are skipped).
    pub fn items_for_pids(&self, pids: &[String]) -> Vec<CatalogueItem> {
        let wanted: HashSet<&str> = pids.iter().map(String::as_str).collect();
        self.all()
            .iter()
            .filter(|i| wanted.contains(i.pid.as_str()))
            .cloned()
            .collect()
    }

    /// Waits until no load or refresh is running.
    pub async fn wait_idle(&self) {
        while matches!(self.status().state, "loading" | "refreshing") {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
    }

    pub fn set_items(&self, values: &[Value]) {
        let mut items: Vec<CatalogueItem> =
            values.iter().filter_map(CatalogueItem::from_item).collect();
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
    pub async fn load_at_startup(&self, ruv: &Ruv, data_dir: &Path) -> bool {
        let saved = tokio::fs::try_exists(schedule_file(data_dir))
            .await
            .unwrap_or(false);
        if !saved {
            tracing::info!("no saved catalogue; fetching it from RÚV (takes minutes)");
        }
        self.load(ruv, !saved).await
    }

    /// Loads (or with `refresh`, fetches from RÚV and then loads) the catalogue.
    /// Returns false without doing anything if a load is already running.
    pub async fn load(&self, ruv: &Ruv, refresh: bool) -> bool {
        {
            let mut status = self.status.lock().unwrap();
            if matches!(status.state, "loading" | "refreshing") {
                return false;
            }
            status.state = if refresh { "refreshing" } else { "loading" };
            status.error = None;
        }
        let result = if refresh {
            ruv.refresh(None).await
        } else {
            match ruv.load().await {
                Ok(true) => Ok(()),
                Ok(false) => Err("no saved catalogue yet; refresh it".to_string()),
                Err(e) => Err(e),
            }
        };
        match result {
            Ok(()) => {
                self.set_items(ruv.schedule().items());
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

    #[tokio::test]
    async fn loads_and_searches() {
        use crate::ruv::testing::{FakeRuv, episode, movie};
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let mut show = episode("web1", "100", "Þáttur", 1, 2, "2026-11-07T19:35:00");
        show["portrait_image"] = json!("https://myndir.ruv.is/p.jpg");
        show["series_image"] = json!("https://evil.example/s.jpg");
        show["subtitles"] = json!([{"name": "IS", "value": "https://x/is.vtt"}, {"name": "en", "value": "https://x/en.vtt"}]);
        let ruv = FakeRuv::new()
            .item(show)
            .item(movie("mov1", "200", "Bíómynd", "2026-11-06T21:00:00"))
            .build(dir.path());
        let catalogue = Catalogue::new();
        assert_eq!(catalogue.status().state, "empty");
        assert!(catalogue.load(&ruv, false).await);
        assert_eq!(catalogue.status().state, "ready");
        assert_eq!(catalogue.status().count, 2);

        let hits = catalogue.search("ÞÁTT", 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].duration, Some(300));
        assert_eq!(hits[0].content_type, "show");
        assert_eq!(hits[0].display_title, "Þáttur s01e01");
        // The first image there is, through the proxy.
        assert_eq!(
            hits[0].poster_url.as_deref(),
            Some("/api/poster?url=https%3A%2F%2Fmyndir.ruv.is%2Fp.jpg")
        );
        assert_eq!(hits[0].subtitle_languages, ["en", "is"]);
        let movie = catalogue.get("mov1").unwrap();
        assert_eq!(movie.content_type, "movie_or_docu");
        assert_eq!(movie.duration, Some(5400));
        assert_eq!(catalogue.search("", 1).len(), 1);
    }

    #[tokio::test]
    async fn startup_fetches_when_no_schedule_is_saved() {
        use crate::ruv::testing::FakeRuv;
        let data = tempfile::tempdir().unwrap();
        let ruv = FakeRuv::new()
            .featured(&[("300", 1)])
            .series_page("300", "Nýtt", &[("new1", "2026-11-08T20:00:00")])
            .build_empty(data.path());

        let catalogue = Catalogue::new();
        assert!(catalogue.load_at_startup(&ruv, data.path()).await);
        assert_eq!(catalogue.status().count, 1, "fetched from RÚV");
        assert!(schedule_file(data.path()).is_file(), "and saved");

        // Next time it reads the saved schedule; RÚV isn't asked.
        let offline = FakeRuv::new().build_empty(data.path());
        let catalogue = Catalogue::new();
        assert!(catalogue.load_at_startup(&offline, data.path()).await);
        assert_eq!(catalogue.status().count, 1, "read the saved schedule");
        assert_eq!(catalogue.get("new1").unwrap().series_title, "Nýtt");
    }
}
