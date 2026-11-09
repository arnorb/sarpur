//! The Rust half of the differential check (`tools/diff/diff.py`): prints
//! one JSON line per input, in the same shape as the Python half, so the two
//! can be compared line by line.
//!
//!     cargo run -q -p sarpur-lib --example compat -- metadata fixtures/tvschedule.json
//!     cargo run -q -p sarpur-lib --example compat -- resolve --replay fixtures/ruv HD1080 <url>...

use std::io::Write;
use std::path::Path;

use sarpur_lib::ruv::http::Http;
use sarpur_lib::ruv::schedule::{self, ImdbCache};
use sarpur_lib::ruv::{fuzz, imdb, metadata, playlist};
use sarpur_lib::settings::Quality;
use serde_json::{Value, json};

fn schedule_items(path: &str) -> Vec<Value> {
    let schedule: Value =
        serde_json::from_slice(&std::fs::read(path).expect("schedule")).expect("JSON");
    let mut items: Vec<Value> = schedule
        .as_object()
        .expect("object")
        .iter()
        .filter(|(k, v)| *k != "date" && v.is_object())
        .map(|(_, v)| v.clone())
        .collect();
    items.sort_by_key(|i| i["pid"].as_str().unwrap_or_default().to_string());
    items
}

/// `--replay DIR` or `--live`, then the rest of the arguments.
fn http(args: &mut Vec<String>) -> Http {
    match args.first().map(String::as_str) {
        Some("--replay") => {
            let dir = args.remove(1);
            args.remove(0);
            Http::replay(Path::new(&dir)).expect("recordings")
        }
        Some("--live") => {
            args.remove(0);
            Http::live()
        }
        _ => panic!("--replay DIR or --live"),
    }
}

fn quality(name: &str) -> Quality {
    match name {
        "Normal" => Quality::Normal,
        "HD720" => Quality::HD720,
        _ => Quality::HD1080,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.remove(0);
    let mut out = std::io::stdout().lock();
    let mut emit = |v: Value| writeln!(out, "{v}").unwrap();
    match command.as_str() {
        "metadata" => {
            for item in schedule_items(&args[0]) {
                emit(match metadata::ffmpeg_metadata(&item) {
                    Ok(pairs) => json!({"pid": item["pid"], "metadata": pairs}),
                    Err(_) => json!({"pid": item["pid"], "error": true}),
                });
            }
        }
        "resolve" => {
            let http = http(&mut args);
            let q = quality(&args.remove(0));
            for url in args {
                emit(match playlist::resolve(&http, "p", Some(&url), q).await {
                    Ok(p) => json!({"file": url, "url": p.url, "audio_url": p.audio_url,
                                    "fragments": p.fragments, "quality": p.quality.as_str()}),
                    Err(_) => json!({"file": url, "error": true}),
                });
            }
        }
        "fuzz" => {
            // stdin: one [a, b] pair per line.
            for line in std::io::stdin().lines() {
                let pair: (String, String) = serde_json::from_str(&line.unwrap()).expect("pair");
                emit(json!({"ratio": fuzz::ratio(&pair.0, &pair.1),
                            "partial": fuzz::partial_ratio(&pair.0, &pair.1)}));
            }
        }
        "series" => {
            let http = http(&mut args);
            let cache = ImdbCache::new(Default::default(), false);
            for sid in args {
                match schedule::fetch_series(&http, &json!(sid), &cache, 2026).await {
                    Ok(items) => items.into_iter().for_each(|i| emit(schedule::slim(i))),
                    Err(_) => emit(json!({"sid": sid, "error": true})),
                }
            }
        }
        "catalogue" => {
            let http = http(&mut args);
            let cache = ImdbCache::new(Default::default(), false);
            match schedule::fetch_catalogue(&http, None, &cache, 2026, None).await {
                Ok(s) => s.items().iter().cloned().for_each(&mut emit),
                Err(_) => emit(json!({"error": true})),
            }
        }
        "search" => {
            let path = args.remove(0);
            let schedule = schedule::Schedule::load(Path::new(&path)).unwrap().unwrap();
            for query in args {
                emit(
                    json!({"query": query, "pids": sarpur_lib::ruv::search::search(&schedule, &query)}),
                );
            }
        }
        "imdb" => {
            // stdin: one [title, year, kind, duration, episodes] per line.
            let http = http(&mut args);
            for line in std::io::stdin().lines() {
                let (title, year, kind, duration, episodes): (
                    String,
                    Option<String>,
                    String,
                    f64,
                    i64,
                ) = serde_json::from_str(&line.unwrap()).expect("args");
                let kind = match kind.as_str() {
                    "movie" => imdb::Kind::Movie,
                    "documentary" => imdb::Kind::Documentary,
                    _ => imdb::Kind::TvShow,
                };
                emit(
                    match imdb::lookup(&http, &title, year.as_deref(), kind, duration, episodes)
                        .await
                    {
                        Ok(found) => json!({"title": title, "imdb": found}),
                        Err(_) => json!({"title": title, "error": true}),
                    },
                );
            }
        }
        other => panic!("unknown command {other}"),
    }
}
