//! The schedule: every episode RÚV offers, as ruvsarpur's items, kept in
//! `tvschedule.json` in the data folder. Ported from `getVodSeriesSchedule`
//! (R:1321), `getVodSchedule` (R:1187), `getVodCategoryPrograms` (R:545),
//! `saveCurrentTvSchedule` and the worker's `catalogue` command.
//!
//! Items stay JSON, in ruvsarpur's shape, because the naming code branches on
//! which keys are present. In memory the raw `episode` object is cut down to
//! the keys anything reads; its image renditions and clips are most of the
//! file and nothing uses them.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;
use serde_json::{Map, Value, json};

use super::http::Http;
use super::imdb::{self, Kind};
use super::py::{int_of, slice, str_of, strip, truthy};

pub const FEATURED_URL: &str = "https://api.ruv.is/api/programs/featured/tv";
pub const CATEGORY_URL: &str = "https://spilari.nyr.ruv.is/gql/";
const CATEGORY_QUERY: &str = "query { Category(station: tv) { categories { programs { id title web_available_episodes } } } }";
const MONTH_NAMES: [&str; 13] = [
    "", "jan", "feb", "mar", "apr", "maí", "jún", "júl", "ágú", "sep", "okt", "nóv", "des",
];
/// The `episode` keys kept in memory and in the saved file.
const EPISODE_KEYS: [&str; 8] = [
    "description",
    "duration",
    "file_expires",
    "files",
    "firstrun",
    "id",
    "number",
    "title",
];
/// Series fetched at once during a refresh. ruvsarpur fetched one at a time.
const PARALLEL: usize = 6;

static EPNUM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?P<ep_num>\d+) af (?P<ep_total>\d+)").unwrap());
static YEAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i) frá (?P<year>\d{4})").unwrap());

/// `getGroup`: the first non-blank match of `group`, stripped.
fn group(re: &Regex, name: &str, haystack: &str) -> Option<String> {
    re.captures_iter(haystack)
        .filter_map(|c| c.name(name).map(|m| strip(m.as_str()).to_string()))
        .find(|s| !s.is_empty())
}

/// Episodes in schedule order, by pid. Inserting a pid that's there already
/// replaces the item but keeps its place, like a Python dict.
#[derive(Debug, Clone, Default)]
pub struct Schedule {
    /// The day the schedule was fetched (`YYYY-MM-DD`).
    pub date: Option<String>,
    items: Vec<Value>,
    index: HashMap<String, usize>,
}

impl Schedule {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, pid: &str) -> Option<&Value> {
        self.index.get(pid).map(|&i| &self.items[i])
    }

    pub fn items(&self) -> &[Value] {
        &self.items
    }

    pub fn insert(&mut self, item: Value) {
        let pid = str_of(&item["pid"]);
        let item = slim(item);
        match self.index.get(&pid) {
            Some(&i) => self.items[i] = item,
            None => {
                self.index.insert(pid, self.items.len());
                self.items.push(item);
            }
        }
    }

    /// How many items belong to `sid`.
    fn count_for(&self, sid: &str) -> usize {
        self.items
            .iter()
            .filter(|i| i.get("sid").and_then(Value::as_str) == Some(sid))
            .count()
    }

    /// Reads `tvschedule.json`. `Ok(None)` when there is none.
    pub fn load(path: &Path) -> Result<Option<Schedule>, String> {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let Value::Object(map) = value else {
            return Err(format!("{}: not a JSON object", path.display()));
        };
        let mut schedule = Schedule::default();
        for (key, item) in map {
            if key == "date" {
                schedule.date = item.as_str().map(str::to_string);
            } else if item.is_object() {
                schedule.insert(item);
            }
        }
        Ok(Some(schedule))
    }

    /// `saveCurrentTvSchedule`: the items by pid, sorted, with `date` set to
    /// `today`. Written to a temporary file and renamed, so a crash can't
    /// leave half a schedule.
    pub fn save(&mut self, path: &Path, today: &str) -> Result<(), String> {
        self.date = Some(today.to_string());
        let mut map = Map::new();
        map.insert("date".into(), json!(today));
        for item in &self.items {
            map.insert(str_of(&item["pid"]), item.clone());
        }
        let text = serde_json::to_string_pretty(&Value::Object(map)).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Drops the parts of `episode` nothing reads.
pub fn slim(mut item: Value) -> Value {
    if let Some(Value::Object(episode)) = item.get_mut("episode") {
        episode.retain(|k, _| EPISODE_KEYS.contains(&k.as_str()));
    }
    item
}

/// Settings for IMDB matching during a fetch: the cache (`imdb-cache.json`,
/// by sid) and whether to ask IMDB about series that aren't in it.
#[derive(Clone)]
pub struct ImdbCache {
    pub entries: Arc<Mutex<Map<String, Value>>>,
    pub lookups: bool,
}

impl ImdbCache {
    pub fn new(entries: Map<String, Value>, lookups: bool) -> ImdbCache {
        ImdbCache {
            entries: Arc::new(Mutex::new(entries)),
            lookups,
        }
    }

    fn get(&self, sid: &str) -> Option<Value> {
        self.entries
            .lock()
            .unwrap()
            .get(sid)
            .map(|e| e.get("imdb").cloned().unwrap_or(Value::Null))
    }
}

fn req<'a>(obj: &'a Value, key: &str, what: &str) -> Result<&'a Value, String> {
    obj.get(key).ok_or_else(|| format!("{what} has no `{key}`"))
}

/// `' '.join(x) if type(x) is list else x`.
fn joined(value: &Value) -> Value {
    match value {
        Value::Array(parts) => {
            Value::String(parts.iter().map(str_of).collect::<Vec<_>>().join(" "))
        }
        other => other.clone(),
    }
}

/// `formatCoverArtResolutionMacro`.
fn cover_art(value: &Value) -> Value {
    match value {
        Value::String(s) if !s.is_empty() => Value::String(s.replace("$$IMAGESIZE$$", "2048")),
        _ => Value::Null,
    }
}

/// `trimSeasonNumberSuffix`: drops one season suffix (` II`, ` 3`, …).
pub fn trim_season_suffix(title: &str) -> String {
    const SUFFIXES: [&str; 24] = [
        " I", " II", " III", " IV", " V", " VI", " VII", " VIII", " IX", " X", " XI", " XII", " 1",
        " 2", " 3", " 4", " 5", " 6", " 7", " 8", " 9", " 10", " 11", " 12",
    ];
    for suffix in SUFFIXES {
        if let Some(rest) = title.strip_suffix(suffix) {
            return rest.to_string();
        }
    }
    title.to_string()
}

fn season(title_with_season: &str, foreign: &str, desc: &str) -> &'static str {
    let desc = desc.to_lowercase();
    let rules: [(&str, &str, &str, &str); 9] = [
        ("2", " 2", " II", "önnur þáttaröð"),
        ("3", " 3", " III", "þriðja þáttaröð"),
        ("4", " 4", " IV", "fjórða þáttaröð"),
        ("5", " 5", " V", "fimmta þáttaröð"),
        ("6", " 6", " VI", "sjötta þáttaröð"),
        ("7", " 7", " VII", "sjöunda þáttaröð"),
        ("8", " 8", " VIII", "áttunda þáttaröð"),
        ("9", " 9", " IX", "níunda þáttaröð"),
        // ruvsarpur checks `XX` for season 10.
        ("10", " 10", " XX", "tíunda þáttaröð"),
    ];
    for (n, digit, roman, words) in rules {
        if title_with_season.ends_with(digit)
            || title_with_season.ends_with(roman)
            || foreign.ends_with(roman)
            || desc.contains(words)
        {
            return n;
        }
    }
    "1"
}

/// `getVodSeriesSchedule`: every episode of series `sid` from RÚV's API, in
/// RÚV's order. Empty when RÚV has none; an error where the Python raises.
pub async fn fetch_series(
    http: &Http,
    sid: &Value,
    imdb_cache: &ImdbCache,
    today_year: i32,
) -> Result<Vec<Value>, String> {
    let sid_text = str_of(sid);
    let response = http
        .get(
            &format!("https://api.ruv.is/api/programs/program/{sid_text}/all"),
            None,
        )
        .await?;
    let mut prog = response.json()?;
    let episodes_len = prog
        .get("episodes")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if response.status != 200 || prog.is_null() || episodes_len < 1 {
        return Ok(Vec::new());
    }
    let what = format!("series {sid_text}");

    let image = |v: Option<&Value>| match v {
        Some(Value::String(s)) => Value::String(s.replace("/480x/", "/$$IMAGESIZE$$x/")),
        _ => Value::Null,
    };
    let series_image = cover_art(&image(prog.get("image")));
    let portrait_image = cover_art(&image(prog.get("portrait_image")));
    let description = joined(req(&prog, "description", &what)?);
    prog["description"] = description.clone();

    let mut slugs = Vec::new();
    let mut cat_names = Vec::new();
    for category in req(&prog, "categories", &what)?
        .as_array()
        .ok_or("categories is not a list")?
    {
        slugs.push(str_of(req(category, "slug", "category")?));
        cat_names.push(req(category, "title", "category")?.clone());
    }
    let has = |slug: &str| slugs.iter().any(|s| s == slug);
    let prog_title = str_of(req(&prog, "title", &what)?);
    let is_movie = has("kvikmyndir") && !has("leiknir-thaettir");
    let is_docu = has("heimildarmyndir");
    let is_sport = has("ithrottir");
    let english = prog_title.contains("with English subtitles");
    let kind = if is_docu {
        Some(Kind::Documentary)
    } else if is_movie {
        Some(Kind::Movie)
    } else if !is_sport || has("leiknir-thaettir") {
        Some(Kind::TvShow)
    } else {
        None
    };

    let series_title = if is_movie {
        prog_title.clone()
    } else {
        trim_season_suffix(&prog_title)
    };
    let series_description = req(&prog, "short_description", &what)?.clone();
    let series_short = if description.is_null() {
        series_description.clone()
    } else {
        description
    };
    let foreign_title = req(&prog, "foreign_title", &what)?.clone();
    let episodes = prog["episodes"].as_array().cloned().unwrap_or_default();
    let mut remaining = episodes.len();

    let mut imdb_result = imdb_cache.get(&sid_text).unwrap_or(Value::Null);
    if imdb_result.is_null() && kind.is_some() && !has("born") {
        let year = match &series_short {
            Value::String(text) => group(&YEAR, "year", text),
            _ => return Err(format!("{what}: no description to read a year from")),
        };
        let first = &episodes[0];
        let duration = first.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
        let first_title = match first.get("title") {
            Some(Value::Null) | None => series_title.clone(),
            Some(t) => str_of(t),
        };
        let detected = group(&EPNUM, "ep_total", &first_title);
        let episode_count = if truthy(prog.get("multiple_episodes").unwrap_or(&Value::Null)) {
            let available = int_of(req(&prog, "web_available_episodes", &what)?)
                .ok_or("web_available_episodes")?;
            available.max(
                detected
                    .as_deref()
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(1),
            )
        } else {
            1
        };
        if imdb_cache.lookups {
            let kind = kind.unwrap_or(Kind::TvShow);
            let mut found = None;
            if let Value::String(foreign) = &foreign_title {
                found = imdb::lookup(
                    http,
                    foreign,
                    year.as_deref(),
                    kind,
                    duration,
                    episode_count,
                )
                .await?;
            }
            if found.is_none() && is_movie {
                found = imdb::lookup(
                    http,
                    &series_title,
                    year.as_deref(),
                    kind,
                    duration,
                    episode_count,
                )
                .await?;
            }
            if let Some(found) = found {
                imdb_cache.entries.lock().unwrap().insert(
                    sid_text.clone(),
                    json!({"series_id": sid, "original-title": foreign_title, "series_title": series_title, "imdb": found}),
                );
                imdb_result = found;
            }
        }
    }

    let short_description = prog["short_description"].clone();
    let mut out: Vec<Value> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    for mut episode in episodes {
        let ep_what = format!("{what} episode");
        let description = joined(req(&episode, "description", &ep_what)?);
        episode["description"] = description.clone();
        if episode.get("title").is_none_or(Value::is_null) {
            episode["title"] = json!("");
        }
        let episode_title = str_of(&episode["title"]);
        let firstrun = req(&episode, "firstrun", &ep_what)?.clone();
        let showtime = str_of(&firstrun);
        let desc = match &description {
            Value::String(d) if d.chars().count() > 10 => description.clone(),
            Value::String(_) => short_description.clone(),
            Value::Null => return Err(format!("{ep_what} has a null description")),
            _ => short_description.clone(),
        };
        let mut subtitles = Vec::new();
        if let Some(map) = req(&episode, "subtitles", &ep_what)?.as_object() {
            for (name, value) in map {
                if !value.is_null() {
                    subtitles.push(json!({"name": name, "value": value}));
                }
            }
        }
        let subtitles_url = req(&episode, "subtitles_url", &ep_what)?.clone();
        let ep_num = match episode.get("number") {
            Some(n) => str_of(n),
            None => {
                group(&EPNUM, "ep_num", &episode_title).unwrap_or_else(|| remaining.to_string())
            }
        };
        let ep_total =
            group(&EPNUM, "ep_total", &episode_title).unwrap_or_else(|| out_len_hint(&prog));
        let season_num = season(&prog_title, &str_of(&foreign_title), &str_of(&desc));

        let mut entry_series_title = series_title.clone();
        let mut title = series_title.clone();
        if int_of(&json!(ep_total)).is_some_and(|t| t > 1) {
            title = format!("{title} ({ep_num} af {ep_total})");
        }
        let mut sport_season = None;
        if is_sport {
            let year = slice(&showtime, 0, 4);
            if !entry_series_title.contains(&year) {
                entry_series_title = format!("{series_title} {year}");
            }
            let time = slice(&showtime, 11, 16).replace(':', ".");
            let date = format!(
                "{}.{}.{}",
                slice(&showtime, 8, 10),
                slice(&showtime, 5, 7),
                year
            );
            title = if episode_title == date {
                format!("{entry_series_title} ({episode_title}) kl.{time}")
            } else {
                let month = int_of(&json!(slice(&showtime, 5, 7)))
                    .and_then(|m| MONTH_NAMES.get(usize::try_from(m).ok()?))
                    .ok_or_else(|| format!("{ep_what}: odd date {showtime}"))?;
                format!(
                    "{entry_series_title} ({episode_title}) {}.{month} kl.{time}",
                    slice(&showtime, 8, 10)
                )
            };
            sport_season = Some(if !firstrun.is_null() && showtime.chars().count() > 4 {
                year
            } else {
                today_year.to_string()
            });
        }

        let pid = str_of(req(&episode, "id", &ep_what)?);
        let mut entry = json!({
            "imdb": imdb_result,
            "series_title": entry_series_title,
            "series_desc": series_description,
            "series_sdesc": series_short,
            "series_image": series_image,
            "portrait_image": portrait_image,
            "episode_title": episode_title,
            "episode_image": cover_art(req(&episode, "image", &ep_what)?),
            "title": title,
            "pid": pid,
            "showtime": firstrun,
            "duration": episode.get("duration").map(str_of).unwrap_or_default(),
            "duration_friendly": req(&episode, "duration_friendly", &ep_what)?,
            "sid": sid_text,
            "desc": desc,
            "original-title": foreign_title,
            "file": req(&episode, "file", &ep_what)?,
            "subtitles_url": subtitles_url,
            "subtitles": subtitles,
            "has_subtitles": subtitles_url.as_str().is_some_and(|u| u.chars().count() > 1),
            "eventid": req(&episode, "event", &ep_what)?,
            "rating": req(&episode, "rating", &ep_what)?,
            "slug": req(&episode, "slug", &ep_what)?,
            "is_movie": is_movie,
            "is_sport": is_sport,
            "is_docu": is_docu,
            "english_subtitled": english,
            "categories": cat_names,
            "multiple_episodes": req(&prog, "multiple_episodes", &what)?,
            "web_available_episodes": req(&prog, "web_available_episodes", &what)?,
            "ep_num": ep_num,
            "ep_total": ep_total,
            "season_num": season_num,
        });
        if let Some(season) = sport_season {
            entry["sport_season"] = json!(season);
        }
        entry["episode"] = episode;
        match positions.get(&pid) {
            Some(&i) => out[i] = entry,
            None => {
                positions.insert(pid, out.len());
                out.push(entry);
            }
        }
        remaining = remaining.saturating_sub(1);
    }
    Ok(out)
}

/// `str(len(prog['episodes']))`.
fn out_len_hint(prog: &Value) -> String {
    prog["episodes"].as_array().map_or(0, Vec::len).to_string()
}

/// `getVodCategoryPrograms`: programmes from the category feed with episodes
/// to watch. Empty when the feed fails; it only adds to the featured feed.
pub async fn category_programs(http: &Http) -> Vec<Value> {
    let Ok(response) = http
        .post_json(CATEGORY_URL, &json!({"query": CATEGORY_QUERY}))
        .await
    else {
        return Vec::new();
    };
    if response.status != 200 {
        return Vec::new();
    }
    let Ok(data) = response.json() else {
        return Vec::new();
    };
    if data.get("errors").is_some_and(truthy) {
        return Vec::new();
    }
    let mut programs: Vec<Value> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let categories = data
        .pointer("/data/Category/categories")
        .and_then(Value::as_array);
    for program in categories
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("programs")?.as_array())
        .flatten()
    {
        let Some(id) = program.get("id").filter(|v| !v.is_null()) else {
            continue;
        };
        let available = match program.get("web_available_episodes") {
            None | Some(Value::Null) => 0,
            Some(v) if !truthy(v) => 0,
            Some(v) => match int_of(v) {
                Some(n) => n,
                None => return Vec::new(),
            },
        };
        if available <= 0 {
            continue;
        }
        let key = str_of(id);
        match seen.get(&key) {
            Some(&i) => programs[i] = program.clone(),
            None => {
                seen.insert(key, programs.len());
                programs.push(program.clone());
            }
        }
    }
    programs
}

/// The programmes to fetch: the featured feed (deduplicated by id) plus the
/// category feed's programmes it leaves out, with episodes available.
async fn programs(http: &Http) -> Result<Vec<Value>, String> {
    let response = http.get(FEATURED_URL, None).await?;
    let api = response.json()?;
    let panels = api
        .get("panels")
        .and_then(Value::as_array)
        .ok_or("the featured feed has no panels")?;
    let mut data: Vec<Value> = Vec::new();
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for program in panels
        .iter()
        .filter_map(|p| p.get("programs")?.as_array())
        .flatten()
    {
        let id = program.get("id").ok_or("a featured programme has no id")?;
        // Python dict keys: 1 and "1" are different ids here.
        let key = serde_json::to_string(id).unwrap_or_default();
        match by_id.get(&key) {
            Some(&i) => data[i] = program.clone(),
            None => {
                by_id.insert(key, data.len());
                data.push(program.clone());
            }
        }
    }
    let featured: std::collections::HashSet<String> = data
        .iter()
        .filter_map(|p| p.get("id"))
        .map(str_of)
        .collect();
    let extra = category_programs(http).await;
    data.extend(
        extra
            .into_iter()
            .filter(|p| !featured.contains(&str_of(p.get("id").unwrap_or(&Value::Null)))),
    );
    if response.status != 200 || data.is_empty() {
        return Ok(Vec::new());
    }
    let mut wanted = Vec::new();
    for p in data {
        let (Some(available), Some(_)) = (p.get("web_available_episodes"), p.get("id")) else {
            continue;
        };
        let available = available
            .as_f64()
            .ok_or("web_available_episodes is not a number")?;
        if available > 0.0 {
            wanted.push(p);
        }
    }
    Ok(wanted)
}

/// Progress of a refresh: programmes done and in all.
pub type Progress = Arc<dyn Fn(usize, usize) + Send + Sync>;

/// `getVodSchedule`: fetches the catalogue from RÚV. With `existing`, only
/// series whose available episode count grew are fetched again (an
/// incremental refresh); the rest is kept. A series that fails is skipped.
pub async fn fetch_catalogue(
    http: &Http,
    existing: Option<Schedule>,
    imdb_cache: &ImdbCache,
    today_year: i32,
    progress: Option<Progress>,
) -> Result<Schedule, String> {
    let incremental = existing.is_some();
    let mut schedule = existing.unwrap_or_default();
    let programs = programs(http).await?;
    let total = programs.len();

    let mut to_fetch = Vec::new();
    for program in programs {
        let sid = &program["id"];
        if incremental {
            let have = schedule.count_for(&str_of(sid));
            let available = program["web_available_episodes"].as_f64().unwrap_or(0.0);
            if have > 0 && available <= have as f64 {
                continue;
            }
        }
        to_fetch.push(program);
    }
    let skipped = total - to_fetch.len();
    if let Some(p) = &progress {
        p(skipped, total);
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(PARALLEL));
    let done = Arc::new(std::sync::atomic::AtomicUsize::new(skipped));
    let mut tasks = tokio::task::JoinSet::new();
    for (i, program) in to_fetch.into_iter().enumerate() {
        let (http, imdb_cache, semaphore) = (http.clone(), imdb_cache.clone(), semaphore.clone());
        let (done, progress) = (done.clone(), progress.clone());
        tasks.spawn(async move {
            let _permit = semaphore.acquire().await;
            let result = fetch_series(&http, &program["id"], &imdb_cache, today_year).await;
            if let Err(e) = &result {
                let (sid, title) = (str_of(&program["id"]), str_of(&program["title"]));
                tracing::warn!("series {sid} ({title}): {e}; its episodes are left out");
            }
            let n = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if let Some(p) = progress {
                p(n, total);
            }
            (i, result)
        });
    }
    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        results.push(joined.map_err(|e| e.to_string())?);
    }
    results.sort_by_key(|(i, _)| *i);
    for (_, result) in results {
        for item in result.into_iter().flatten() {
            schedule.insert(item);
        }
    }
    Ok(schedule)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> ImdbCache {
        ImdbCache::new(Map::new(), false)
    }

    #[test]
    fn season_suffixes() {
        assert_eq!(trim_season_suffix("Hvolpasveitin IV"), "Hvolpasveitin");
        assert_eq!(trim_season_suffix("Monsurnar 1"), "Monsurnar");
        // Only the first matching suffix goes: " I" doesn't match "II".
        assert_eq!(trim_season_suffix("Séra Brown II"), "Séra Brown");
        assert_eq!(season("Taka tvö", "None", "Önnur þáttaröð um..."), "2");
        assert_eq!(season("X 10", "None", ""), "10");
    }

    #[tokio::test]
    async fn father_brown_from_the_recording() {
        let items = fetch_series(&Http::fixtures(), &json!("35732"), &cache(), 2026)
            .await
            .unwrap();
        assert_eq!(items.len(), 4);
        let first = items.iter().find(|i| i["pid"] == "akohaa").unwrap();
        assert_eq!(first["series_title"], "Séra Brown");
        assert_eq!(first["season_num"], "1");
        assert_eq!(first["is_movie"], false);
        assert!(
            first["episode"].get("clips").is_some(),
            "slimming happens in the schedule"
        );
    }

    #[test]
    fn insert_keeps_the_first_place_and_slims() {
        let mut s = Schedule::default();
        s.insert(json!({"pid": "a", "n": 1, "episode": {"description": "d", "clips": [1]}}));
        s.insert(json!({"pid": "b", "n": 2}));
        s.insert(json!({"pid": "a", "n": 3}));
        assert_eq!(
            s.items()
                .iter()
                .map(|i| i["n"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
        let mut s2 = Schedule::default();
        s2.insert(json!({"pid": "a", "episode": {"description": "d", "clips": [1]}}));
        assert_eq!(s2.get("a").unwrap()["episode"], json!({"description": "d"}));
    }

    #[test]
    fn saves_and_loads() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tvschedule.json");
        let mut s = Schedule::default();
        s.insert(json!({"pid": "b", "title": "Þáttur"}));
        s.insert(json!({"pid": "a", "title": "Annar"}));
        s.save(&path, "2026-11-09").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("{\n  \"a\": {"),
            "sorted keys, two-space indent: {text}"
        );
        let loaded = Schedule::load(&path).unwrap().unwrap();
        assert_eq!(loaded.date.as_deref(), Some("2026-11-09"));
        assert_eq!(loaded.get("b").unwrap()["title"], "Þáttur");
        assert!(
            Schedule::load(&dir.path().join("none.json"))
                .unwrap()
                .is_none()
        );
    }
}
