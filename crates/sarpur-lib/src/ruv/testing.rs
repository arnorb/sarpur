//! A fake RÚV for tests, here and in the server crate: schedule items in
//! ruvsarpur's shape, with their playlists and series pages served from
//! memory. Playlists end in `fake-<behaviour>.m3u8`, which tells
//! `testdata/fake_ffmpeg.py` what to do (`ok`, `fail`, `slow`).

use std::path::Path;

use serde_json::{Value, json};

use super::Ruv;
use super::http::Http;
use super::schedule::Schedule;

/// An episode of `series_title`: number `ep` of `total`, or a single item
/// when `total` is 1.
pub fn episode(
    pid: &str,
    sid: &str,
    series_title: &str,
    ep: u32,
    total: u32,
    showtime: &str,
) -> Value {
    let title = if total > 1 {
        format!("{series_title} ({ep} af {total})")
    } else {
        series_title.to_string()
    };
    json!({
        "pid": pid, "sid": sid, "title": title, "series_title": series_title,
        "episode_title": format!("Þáttur {ep}"), "original-title": null,
        "showtime": showtime, "duration": "300", "duration_friendly": "5 mín",
        "ep_num": ep.to_string(), "ep_total": total.to_string(), "season_num": "1",
        "multiple_episodes": total > 1, "is_movie": false, "is_docu": false, "is_sport": false,
        "english_subtitled": false, "imdb": null, "desc": "Lýsing á þættinum.",
        "series_desc": null, "series_sdesc": "Lýsing á þáttaröðinni.",
        "episode": {"description": "Lýsing á þættinum.", "firstrun": showtime},
        "file": null, "subtitles": [], "categories": [],
    })
}

/// A movie (`is_movie`), one part.
pub fn movie(pid: &str, sid: &str, title: &str, showtime: &str) -> Value {
    let mut item = episode(pid, sid, title, 1, 1, showtime);
    item["is_movie"] = json!(true);
    item["duration"] = json!("5400");
    item
}

/// The playlist URL ffmpeg gets for an item made playable with `behaviour`.
pub fn playlist_url(pid: &str, behaviour: &str) -> String {
    format!("https://ruv-vod.akamaized.net/test/{pid}/fake-{behaviour}.m3u8")
}

/// The fake RÚV: items, and the HTTP answers behind them.
#[derive(Default)]
pub struct FakeRuv {
    items: Vec<Value>,
    responses: Vec<(String, String, u16, Vec<u8>)>,
}

impl FakeRuv {
    pub fn new() -> FakeRuv {
        FakeRuv::default()
    }

    /// Adds an item whose playlist resolves and makes ffmpeg do `behaviour`.
    pub fn playable(mut self, mut item: Value, behaviour: &str) -> FakeRuv {
        let pid = item["pid"].as_str().unwrap_or_default().to_string();
        let master = format!("https://ruv-vod.akamaized.net/test/{pid}/{pid}.m3u8");
        item["file"] = json!(master);
        let body = format!(
            "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1,RESOLUTION=1920x1080\nfake-{behaviour}.m3u8\n"
        );
        self.responses
            .push(("GET".into(), master, 200, body.into_bytes()));
        self.responses.push((
            "GET".into(),
            playlist_url(&pid, behaviour),
            200,
            b"#EXTM3U\nseg.ts\n".to_vec(),
        ));
        self.items.push(item);
        self
    }

    /// Adds an item as it is, with no playlist.
    pub fn item(mut self, item: Value) -> FakeRuv {
        self.items.push(item);
        self
    }

    /// Serves `items` as series `sid` on RÚV's API (`program/<sid>/all`),
    /// for follows of series that aren't in the schedule.
    pub fn series_page(mut self, sid: &str, title: &str, episodes: &[(&str, &str)]) -> FakeRuv {
        let page = series_page(sid, title, episodes);
        self.responses.push((
            "GET".into(),
            format!("https://api.ruv.is/api/programs/program/{sid}/all"),
            200,
            page.to_string().into_bytes(),
        ));
        self
    }

    /// Serves a featured feed listing these series (for a refresh).
    pub fn featured(mut self, sids: &[(&str, u32)]) -> FakeRuv {
        let programs: Vec<Value> = sids
            .iter()
            .map(|(sid, n)| json!({"id": sid.parse::<i64>().unwrap_or(0), "title": sid, "web_available_episodes": n}))
            .collect();
        let feed = json!({"panels": [{"programs": programs}]});
        self.responses.push((
            "GET".into(),
            super::schedule::FEATURED_URL.into(),
            200,
            feed.to_string().into_bytes(),
        ));
        self
    }

    /// Writes the items to `tvschedule.json` in `data_dir` (dated today, so a
    /// refresh is incremental) and returns a [`Ruv`] with them loaded.
    pub fn build(self, data_dir: &Path) -> Ruv {
        let mut schedule = Schedule::default();
        for item in self.items {
            schedule.insert(item);
        }
        schedule
            .save(&data_dir.join(super::SCHEDULE_FILE), &super::today())
            .expect("schedule written");
        let ruv = Ruv::new(data_dir, Http::canned(self.responses));
        ruv.set_schedule(schedule);
        ruv
    }

    /// A [`Ruv`] with no saved schedule.
    pub fn build_empty(self, data_dir: &Path) -> Ruv {
        Ruv::new(data_dir, Http::canned(self.responses))
    }
}

/// RÚV's `program/<sid>/all` answer for a series with `episodes`
/// (`(pid, firstrun)`), newest first as RÚV lists them.
pub fn series_page(sid: &str, title: &str, episodes: &[(&str, &str)]) -> Value {
    let total = episodes.len();
    let episodes: Vec<Value> = episodes
        .iter()
        .enumerate()
        .map(|(i, (pid, firstrun))| {
            json!({
                "id": pid, "number": total - i, "title": format!("Þáttur {}", total - i),
                "firstrun": firstrun, "duration": 600, "duration_friendly": "10 mín",
                "description": ["Lýsing á þessum þætti."], "image": null, "file": null,
                "subtitles_url": null, "subtitles": {}, "event": 1, "rating": 0, "slug": "thattur",
            })
        })
        .collect();
    json!({
        "id": sid.parse::<i64>().unwrap_or(0), "title": title, "foreign_title": null,
        "description": ["Lýsing á þáttaröðinni."], "short_description": null,
        "image": null, "portrait_image": null, "categories": [{"slug": "born", "title": "Barnaefni"}],
        "multiple_episodes": total > 1, "web_available_episodes": total, "episodes": episodes,
    })
}
