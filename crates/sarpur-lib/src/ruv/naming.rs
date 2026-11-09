//! File names and titles, ported from ruvsarpur's `sanitizeFileName`,
//! `createShowTitle` and `createLocalFileName` (R:1020-1138). Plex already
//! knows the names these make, so they must match the Python byte for byte;
//! `golden/names.jsonl` is the check (`tests::golden`).
//!
//! Items are schedule entries as JSON, because the Python branches on
//! whether a key is present as well as on its value.

use serde_json::Value;

use super::py::{has, has_some, int_of, is_true, slice, str_at, str_of, strip, truthy_at, zfill};

/// The path separator ruvsarpur joins with (`os.sep`).
#[cfg(windows)]
const SEP: &str = "\\";
#[cfg(not(windows))]
const SEP: &str = "/";

/// `sanitizeFileName`: characters NTFS rejects become `sep`, then strip.
pub fn sanitize_with(name: &str, sep: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if matches!(
            c,
            '.' | ','
                | '\''
                | ';'
                | '"'
                | '/'
                | ':'
                | '<'
                | '>'
                | '|'
                | '?'
                | '*'
                | '\n'
                | '\r'
                | '\t'
                | '\0'
        ) {
            out.push_str(sep);
        } else {
            out.push(c);
        }
    }
    strip(&out).to_string()
}

pub fn sanitize(name: &str) -> String {
    sanitize_with(name, " ")
}

/// `rchop` with a list: drops the first suffix in the list that matches.
fn rchop<'a>(s: &'a str, suffixes: &[&str]) -> &'a str {
    for suffix in suffixes {
        if let Some(rest) = s.strip_suffix(suffix) {
            return rest;
        }
    }
    s
}

const ROMAN_SUFFIXES: [&str; 9] = [
    " I", " II", " III", " IV", " V", " VI", " VII", " VIII", " IX",
];

/// `int(show.get(key) or 0)`.
fn int_or_zero(item: &Value, key: &str) -> i64 {
    match item.get(key) {
        Some(v) if super::py::truthy(v) => int_of(v).unwrap_or(0),
        _ => 0,
    }
}

/// `int(show[key])`, with anything Python couldn't convert read as 0.
fn int_at(item: &Value, key: &str) -> i64 {
    item.get(key).and_then(int_of).unwrap_or(0)
}

/// `createShowTitle`.
pub fn show_title(item: &Value, original_title: bool, plex: bool) -> String {
    let title = str_at(item, "title");
    if plex {
        let series_title = sanitize(&str_at(item, "series_title"));
        if has_some(item, "original-title") {
            let original = sanitize(&str_at(item, "original-title"));
            return format!("{series_title} - {}", rchop(&original, &ROMAN_SUFFIXES));
        }
        return series_title;
    }
    let episodic = has_some(item, "ep_num")
        && !truthy_at(item, "is_movie")
        && !truthy_at(item, "is_docu")
        && !truthy_at(item, "is_sport")
        && (truthy_at(item, "multiple_episodes") || int_or_zero(item, "ep_total") > 1);
    if episodic {
        let season = match item.get("season_num") {
            Some(v) if super::py::truthy(v) => str_of(v),
            _ => "1".into(),
        };
        let series_title = match item.get("series_title") {
            Some(v) if super::py::truthy(v) => str_of(v),
            _ => title.clone(),
        };
        let mut out = format!(
            "{} s{}e{}",
            sanitize(&series_title),
            zfill(&season, 2),
            zfill(&str_at(item, "ep_num"), 2)
        );
        if original_title && truthy_at(item, "original-title") {
            out = format!("{out} - {}", sanitize(&str_at(item, "original-title")));
        }
        return out;
    }
    if original_title && has_some(item, "original-title") {
        return format!(
            "{} - {}",
            sanitize(&title),
            sanitize(&str_at(item, "original-title"))
        );
    }
    sanitize(&title)
}

/// `createLocalFileName`. Plex names have no `.mp4`; the others do (the
/// download code appends it to Plex names, see [`relative_path`]).
pub fn local_file_name(item: &Value, original_title: bool, plex: bool, suffix: &str) -> String {
    let show_title = show_title(item, original_title, plex);
    if !plex {
        let local = if has(item, "ep_num") {
            format!("{show_title}{suffix}")
        } else {
            let showtime = str_at(item, "showtime");
            format!(
                "{show_title} - {} ({}){suffix}",
                slice(&showtime, 0, 10),
                str_at(item, "pid")
            )
        };
        return format!("{}.mp4", sanitize(&local));
    }

    let original = if has_some(item, "original-title") {
        format!(" ({})", sanitize(&str_at(item, "original-title")))
    } else {
        String::new()
    };
    let series_title = str_at(item, "series_title");
    let (mut imdb_id, mut imdb_year) = (String::new(), String::new());
    if let Some(imdb) = item.get("imdb").filter(|v| !v.is_null())
        && imdb
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
    {
        imdb_id = format!(" {{imdb-{}}}", str_at(imdb, "id"));
        if has_some(imdb, "year") {
            imdb_year = format!(" ({})", str_at(imdb, "year"));
        }
    }
    let multi = has(item, "ep_num") && has(item, "ep_total") && int_at(item, "ep_total") > 1;
    let show_dir = sanitize(&show_title);

    if is_true(item, "is_movie") || is_true(item, "is_docu") {
        let base = format!(
            "{show_dir}{suffix}{SEP}{}{original}{suffix}{imdb_year}{imdb_id}",
            sanitize(&series_title)
        );
        if multi {
            return format!("{base} - part{}", zfill(&str_at(item, "ep_num"), 2));
        }
        return base;
    }
    if truthy_at(item, "is_sport") {
        let showtime = str_at(item, "showtime");
        let mut sport_title = sanitize(&str_at(item, "title"));
        let formatted = format!(
            "{}.{}.{}",
            slice(&showtime, 8, 10),
            slice(&showtime, 5, 7),
            slice(&showtime, 0, 4)
        );
        let day = slice(&showtime, 0, 10);
        if !sport_title.contains(&day)
            && !sport_title.contains(&day.replace('-', "."))
            && !sport_title.contains(&formatted)
        {
            sport_title = format!("{sport_title} ({formatted})");
        }
        return format!(
            "{show_dir}{SEP}Season 01{SEP}{}{suffix}",
            sport_title.replace(':', ".")
        );
    }
    if multi {
        let season = if has(item, "season_num") {
            str_at(item, "season_num")
        } else {
            "1".into()
        };
        let season = zfill(&season, 2);
        return format!(
            "{show_dir}{SEP}Season {season}{SEP}{}{original} - s{season}e{}{suffix}{imdb_id}",
            sanitize(&series_title),
            zfill(&str_at(item, "ep_num"), 2)
        );
    }
    format!(
        "{show_dir}{SEP}{}{original} - {} - [{}]{suffix}{imdb_id}",
        sanitize(&series_title),
        sanitize(&slice(&str_at(item, "showtime"), 0, 10)),
        sanitize(&str_at(item, "pid"))
    )
}

/// The path a download is written to, relative to the download folder:
/// [`local_file_name`] with `.mp4` added when it's missing, as
/// `download_m3u8_playlist_using_ffmpeg` does.
pub fn relative_path(item: &Value, plex: bool) -> String {
    let name = local_file_name(item, false, plex, "");
    if name.ends_with(".mp4") {
        name
    } else {
        format!("{name}.mp4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sanitizing() {
        assert_eq!(sanitize(" a.b:c/d? "), "a b c d");
        assert_eq!(sanitize_with("a.b", "_"), "a_b");
    }

    #[test]
    fn rchop_takes_the_first_matching_suffix() {
        assert_eq!(rchop("Foo III", &ROMAN_SUFFIXES), "Foo");
        assert_eq!(rchop("Foo I", &ROMAN_SUFFIXES), "Foo");
        assert_eq!(rchop("Foo", &ROMAN_SUFFIXES), "Foo");
    }

    #[test]
    fn father_brown_stays_in_season_one() {
        // Criterion 4: `akoha1` keeps `Season 01`.
        let golden = include_str!("../../../../golden/names.jsonl");
        let line = golden
            .lines()
            .find(|l| l.contains("\"pid\": \"akoha1\""))
            .expect("akoha1 is in golden/names.jsonl");
        let value: Value = serde_json::from_str(line).unwrap();
        let path = local_file_name(&value["input"], false, true, "");
        assert!(path.contains("/Season 01/"), "{path}");
    }

    #[test]
    fn a_sport_item_gets_its_date() {
        let item = json!({
            "pid": "x", "title": "Fótbolti: KR - Valur", "series_title": "Fótbolti 2024",
            "showtime": "2024-05-01T19:15:00", "ep_num": "1", "ep_total": "3",
            "is_sport": true, "is_movie": false, "is_docu": false,
        });
        assert_eq!(
            local_file_name(&item, false, true, ""),
            "Fótbolti 2024/Season 01/Fótbolti  KR - Valur (01.05.2024)"
        );
    }

    /// Every item of the pinned schedule, in the three modes, against the
    /// names the Python made (`tools/golden/generate.py`).
    #[cfg(not(windows))]
    #[test]
    fn golden() {
        let golden = include_str!("../../../../golden/names.jsonl");
        let modes = [
            ("web", false, false),
            ("plex", false, true),
            ("web_original", true, false),
        ];
        let mut checked = 0;
        let mut failures = Vec::new();
        for line in golden.lines() {
            let value: Value = serde_json::from_str(line).unwrap();
            let input = &value["input"];
            for (mode, original, plex) in modes {
                let expected = &value["output"][mode];
                let path = local_file_name(input, original, plex, "");
                let title = show_title(input, original, plex);
                if path != expected["path"].as_str().unwrap()
                    || title != expected["title"].as_str().unwrap()
                {
                    failures.push(format!(
                        "{} {mode}: got ({path:?}, {title:?}), want ({}, {})",
                        value["pid"], expected["path"], expected["title"]
                    ));
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 8508 * 3);
        assert!(
            failures.is_empty(),
            "{} differences, first: {:#?}",
            failures.len(),
            &failures[..failures.len().min(5)]
        );
    }
}
