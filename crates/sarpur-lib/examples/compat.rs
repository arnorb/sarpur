//! The Rust half of the differential check (`tools/diff/diff.py`): prints
//! one JSON line per input, in the same shape as the Python half, so the two
//! can be compared line by line.
//!
//!     cargo run -q -p sarpur-lib --example compat -- metadata fixtures/tvschedule.json
//!     cargo run -q -p sarpur-lib --example compat -- resolve --replay fixtures/ruv HD1080 <url>...

use std::io::Write;
use std::path::Path;

use sarpur_lib::ruv::http::Http;
use sarpur_lib::ruv::{metadata, playlist};
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
        other => panic!("unknown command {other}"),
    }
}
