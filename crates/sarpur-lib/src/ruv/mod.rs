//! RÚV: the catalogue, file names, playlists, search and IMDB, ported from
//! the Python worker in S3. Each part is checked against the worker's output
//! by `tools/diff/diff.py`; file names also against `golden/names.jsonl`.
//!
//! [`Ruv`] is what the rest of sarpur uses. It keeps the schedule
//! (`tvschedule.json`) in memory and in the data folder.

pub mod fuzz;
pub mod http;
pub mod imdb;
pub mod metadata;
pub mod naming;
pub mod playlist;
pub mod py;
pub mod schedule;
pub mod search;
#[doc(hidden)]
pub mod testing;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::settings::Quality;
use http::Http;
use py::str_of;
use schedule::{ImdbCache, Schedule};

pub const SCHEDULE_FILE: &str = "tvschedule.json";
pub const IMDB_CACHE_FILE: &str = "imdb-cache.json";

/// Everything needed for one download.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plan {
    pub pid: String,
    pub sid: String,
    pub title: String,
    pub relative_path: String,
    pub content_type: String,
    pub playlist_url: String,
    pub audio_url: Option<String>,
    pub quality: Option<String>,
    pub duration_seconds: Option<u64>,
    pub metadata: Vec<(String, String)>,
    #[serde(default)]
    pub subtitles: Value,
    #[serde(default)]
    pub images: Images,
}

/// Artwork URLs for a download (`portrait_image`, `series_image`, `episode_image`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct Images {
    pub portrait: Option<String>,
    pub series: Option<String>,
    pub episode: Option<String>,
}

/// The file name a download of `pid` would get.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct Name {
    pub pid: String,
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub not_found: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMode {
    /// Browser downloads: `Title s01e01.mp4`.
    Web,
    /// The library: Plex's folders.
    Plex,
}

/// `movie_or_docu`, `sport` or `show` (`gui_api._content_type_from_item`).
pub fn content_type(item: &Value) -> &'static str {
    let flag = |key: &str| item.get(key).is_some_and(py::truthy);
    if flag("is_movie") || flag("is_docu") {
        "movie_or_docu"
    } else if flag("is_sport") {
        "sport"
    } else {
        "show"
    }
}

/// `int(float(item["duration"]))`, or `None`.
fn duration_seconds(item: &Value) -> Option<u64> {
    let seconds = match item.get("duration")? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => py::strip(s).parse::<f64>().ok()?,
        _ => return None,
    };
    (seconds.is_finite() && seconds >= 0.0).then_some(seconds as u64)
}

fn today() -> String {
    crate::now_iso()[..10].to_string()
}

fn this_year() -> i32 {
    crate::now_iso()[..4].parse().unwrap_or(2026)
}

#[derive(Clone)]
pub struct Ruv {
    inner: Arc<Inner>,
}

struct Inner {
    http: Http,
    data_dir: PathBuf,
    /// Ask IMDB about series that aren't in `imdb-cache.json`.
    imdb_lookups: bool,
    schedule: RwLock<Arc<Schedule>>,
    /// One fetch that writes the schedule at a time.
    writing: tokio::sync::Mutex<()>,
}

impl Ruv {
    /// IMDB is asked only over a live connection; replayed tests stay offline.
    pub fn new(data_dir: impl Into<PathBuf>, http: Http) -> Ruv {
        let imdb_lookups = matches!(http, Http::Live(_));
        Ruv {
            inner: Arc::new(Inner {
                http,
                data_dir: data_dir.into(),
                imdb_lookups,
                schedule: RwLock::new(Arc::new(Schedule::default())),
                writing: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    pub fn schedule_path(&self) -> PathBuf {
        self.inner.data_dir.join(SCHEDULE_FILE)
    }

    /// The schedule in memory.
    pub fn schedule(&self) -> Arc<Schedule> {
        self.inner.schedule.read().unwrap().clone()
    }

    fn set_schedule(&self, schedule: Schedule) {
        *self.inner.schedule.write().unwrap() = Arc::new(schedule);
    }

    /// Reads the saved schedule. `Ok(false)` when the data folder has none.
    pub async fn load(&self) -> Result<bool, String> {
        let path = self.schedule_path();
        let loaded = tokio::task::spawn_blocking(move || Schedule::load(&path))
            .await
            .map_err(|e| e.to_string())??;
        match loaded {
            Some(schedule) => {
                self.set_schedule(schedule);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    async fn imdb_cache(&self) -> ImdbCache {
        let path = self.inner.data_dir.join(IMDB_CACHE_FILE);
        let entries = tokio::fs::read(&path)
            .await
            .ok()
            .and_then(|b| serde_json::from_slice::<Map<String, Value>>(&b).ok())
            .unwrap_or_default();
        ImdbCache::new(entries, self.inner.imdb_lookups)
    }

    /// Writes the schedule (dated `date`, else its own date, else today) and
    /// the IMDB cache, then makes it the schedule in memory.
    async fn save(
        &self,
        mut schedule: Schedule,
        cache: &ImdbCache,
        date: Option<String>,
    ) -> Result<(), String> {
        let path = self.schedule_path();
        let cache_path = self.inner.data_dir.join(IMDB_CACHE_FILE);
        let entries = cache.entries.lock().unwrap().clone();
        let schedule = tokio::task::spawn_blocking(move || -> Result<Schedule, String> {
            let date = date.or_else(|| schedule.date.clone()).unwrap_or_else(today);
            schedule.save(&path, &date)?;
            if !entries.is_empty() {
                let text = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
                std::fs::write(&cache_path, text)
                    .map_err(|e| format!("{}: {e}", cache_path.display()))?;
            }
            Ok(schedule)
        })
        .await
        .map_err(|e| e.to_string())??;
        self.set_schedule(schedule);
        Ok(())
    }

    /// Fetches the catalogue from RÚV and saves it (the worker's `catalogue
    /// --refresh --incremental`): on the day of the last whole fetch only
    /// series with new episodes are fetched again, otherwise everything.
    /// Takes minutes. A fetch that finds nothing keeps the schedule.
    pub async fn refresh(&self, progress: Option<schedule::Progress>) -> Result<(), String> {
        let _writing = self.inner.writing.lock().await;
        let today = today();
        let current = self.schedule();
        let existing = (!current.is_empty() && current.date.as_deref() == Some(today.as_str()))
            .then(|| (*current).clone());
        let cache = self.imdb_cache().await;
        let fetched =
            schedule::fetch_catalogue(&self.inner.http, existing, &cache, this_year(), progress)
                .await?;
        if fetched.is_empty() {
            return Err("RÚV listed no programmes".into());
        }
        self.save(fetched, &cache, Some(today)).await
    }

    /// Fetches one series from RÚV, including one that isn't in the catalogue,
    /// and adds its episodes to the schedule so they can be downloaded.
    pub async fn series(&self, sid: &str) -> Result<Vec<Value>, String> {
        let _writing = self.inner.writing.lock().await;
        let cache = self.imdb_cache().await;
        let sid_value = Value::String(sid.to_string());
        let items =
            schedule::fetch_series(&self.inner.http, &sid_value, &cache, this_year()).await?;
        if !items.is_empty() {
            let mut merged = (*self.schedule()).clone();
            for item in &items {
                merged.insert(item.clone());
            }
            // The date stays: it says when the catalogue was last fetched whole.
            self.save(merged, &cache, None).await?;
        }
        Ok(items)
    }

    /// Everything needed to download `pid`, its playlist resolved on RÚV.
    pub async fn plan(&self, pid: &str, mode: NameMode, quality: Quality) -> Result<Plan, String> {
        let schedule = self.schedule();
        let item = schedule
            .get(pid)
            .ok_or_else(|| format!("pid {pid} not found in the schedule"))?;
        let relative_path = naming::relative_path(item, mode == NameMode::Plex);
        let metadata = metadata::ffmpeg_metadata(item)?;
        let file = item.get("file").filter(|f| !f.is_null()).map(str_of);
        let playlist = playlist::resolve(&self.inner.http, pid, file.as_deref(), quality).await?;
        let image = |key: &str| item.get(key).and_then(Value::as_str).map(str::to_string);
        Ok(Plan {
            pid: pid.to_string(),
            sid: str_of(item.get("sid").unwrap_or(&Value::Null)),
            title: naming::show_title(item, false, false),
            relative_path,
            content_type: content_type(item).to_string(),
            playlist_url: playlist.url,
            audio_url: playlist.audio_url,
            quality: Some(playlist.quality.as_str().to_string()),
            duration_seconds: duration_seconds(item),
            metadata,
            subtitles: item.get("subtitles").cloned().unwrap_or(Value::Null),
            images: Images {
                portrait: image("portrait_image"),
                series: image("series_image"),
                episode: image("episode_image"),
            },
        })
    }

    /// The file names `plan` would use, without asking RÚV.
    pub fn names(&self, pids: &[String], mode: NameMode) -> Vec<Name> {
        let schedule = self.schedule();
        pids.iter()
            .map(|pid| match schedule.get(pid) {
                Some(item) => Name {
                    pid: pid.clone(),
                    relative_path: Some(naming::relative_path(item, mode == NameMode::Plex)),
                    content_type: Some(content_type(item).to_string()),
                    not_found: false,
                },
                None => Name {
                    pid: pid.clone(),
                    relative_path: None,
                    content_type: None,
                    not_found: true,
                },
            })
            .collect()
    }

    /// ruvsarpur's fuzzy title search: the matching pids, in schedule order.
    pub async fn search(&self, query: &str) -> Vec<String> {
        let schedule = self.schedule();
        let query = query.to_string();
        tokio::task::spawn_blocking(move || search::search(&schedule, &query))
            .await
            .unwrap_or_default()
    }
}
