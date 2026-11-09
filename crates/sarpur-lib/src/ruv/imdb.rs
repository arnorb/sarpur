//! Matching a RÚV series to IMDB through IMDB's suggestion API, ported from
//! ruvsarpur's `lookupItemInIMDB` (R:209-330). The match decides the
//! `{imdb-tt…}` and `(year)` parts of Plex names, so the rules are kept as
//! they are, including the order the checks run in.
//!
//! ruvsarpur could also read IMDB's `title.basics.tsv` for original titles
//! (`--imdbfolder`); sarpur never passed it, so those checks always came up
//! empty and are left out.

use serde_json::{Map, Value, json};

use super::fuzz;
use super::http::Http;

/// `urllib.parse.quote(s)` (safe `/`).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"_.-~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Movie,
    Documentary,
    TvShow,
}

impl Kind {
    fn types(self, duration_sec: f64, episodes: i64) -> &'static [&'static str] {
        // A short is 40 minutes or less; 45 leaves room for filler.
        let short = duration_sec > 0.0 && duration_sec < 2700.0;
        match self {
            Kind::Movie if short => &["short"],
            Kind::Movie => &["feature", "tv movie"],
            Kind::Documentary if short => &["short"],
            Kind::Documentary => &["feature", "tv special"],
            Kind::TvShow if episodes > 1 && episodes <= 6 => &["mini-series", "tv mini-series"],
            Kind::TvShow => &["tv series"],
        }
    }
}

fn s(obj: &Map<String, Value>, key: &str) -> Option<String> {
    obj.get(key).map(super::py::str_of)
}

/// The lowercased `l` (primary title) of a suggestion, when it has one.
fn label(obj: &Map<String, Value>) -> Option<String> {
    obj.get("l").and_then(Value::as_str).map(str::to_lowercase)
}

/// Picks the suggestion ruvsarpur would pick, or `None`. `Err` where the
/// Python raises (a suggestion without an `id`).
pub fn choose(
    suggestions: &[Value],
    title: &str,
    year: Option<&str>,
    kind: Kind,
    duration_sec: f64,
    episodes: i64,
) -> Result<Option<Value>, String> {
    let types = kind.types(duration_sec, episodes);
    let mut matches: Vec<&Map<String, Value>> = Vec::new();
    for obj in suggestions.iter().filter_map(Value::as_object) {
        if !(obj.contains_key("i") && obj.contains_key("q") && obj.contains_key("s")) {
            continue;
        }
        let cast_len = match &obj["s"] {
            Value::String(text) => text.chars().count(),
            Value::Array(a) => a.len(),
            Value::Object(o) => o.len(),
            _ => return Err("IMDB suggestion with an odd `s`".into()),
        };
        if cast_len <= 2 {
            continue;
        }
        let id = obj.get("id").ok_or("IMDB suggestion without an id")?;
        if super::py::str_of(id).starts_with("tt") {
            matches.push(obj);
        }
    }
    if matches.is_empty() {
        return Ok(None);
    }
    let lower = title.to_lowercase();
    let of_type = |o: &Map<String, Value>| {
        s(o, "q").is_some_and(|q| types.contains(&q.to_lowercase().as_str()))
    };
    let similar = |o: &Map<String, Value>| label(o).is_some_and(|l| fuzz::ratio(&lower, &l) > 85);

    let mut found: Option<(&Map<String, Value>, &str)> = None;
    let exact: Vec<_> = matches
        .iter()
        .filter(|o| label(o).as_deref() == Some(lower.as_str()))
        .collect();
    if exact.len() == 1 {
        found = Some((exact[0], "Exact primary title"));
    }
    if found.is_none() {
        let close: Vec<_> = matches.iter().filter(|o| similar(o)).collect();
        if close.len() == 1 {
            found = Some((close[0], "Similar primary title, single match"));
        }
    }
    if found.is_none() {
        found = matches
            .iter()
            .find(|o| similar(o) && of_type(o))
            .map(|o| (*o, "Similar primary title and type, first match"));
    }
    if found.is_none()
        && let Some(year) = year
    {
        found = matches
            .iter()
            .find(|o| {
                of_type(o) && o.contains_key("y") && s(o, "y").is_some_and(|y| y.contains(year))
            })
            .map(|o| (*o, "Same type and year, first match"));
    }
    if found.is_none() && matches.len() == 1 {
        found = matches
            .iter()
            .find(|o| of_type(o))
            .map(|o| (*o, "Only result"));
    }
    let Some((obj, via)) = found else {
        return Ok(None);
    };
    if obj.get("id").is_none_or(Value::is_null) {
        return Ok(None);
    }
    let field = |key: &str| obj.get(key).cloned().unwrap_or(Value::Null);
    let mut result = json!({
        "id": field("id"),
        "title": field("l"),
        "actors": field("s"),
        "year": field("y"),
        "image": obj.get("i").and_then(|i| i.get("imageUrl")).cloned().unwrap_or(Value::Null),
        "foundvia": via,
    });
    if let Some(first) = obj
        .get("v")
        .and_then(Value::as_array)
        .and_then(|v| v.first())
        && let Some(id) = first.get("id")
    {
        result["video"] = json!({
            "id": id,
            "title": first.get("l").cloned().unwrap_or(Value::Null),
            "length": first.get("s").cloned().unwrap_or(Value::Null),
        });
    }
    Ok(Some(result))
}

/// `lookupItemInIMDB`: asks IMDB and picks a match. A failed request is no
/// match; an answer that isn't JSON is an error, as in the Python.
pub async fn lookup(
    http: &Http,
    title: &str,
    year: Option<&str>,
    kind: Kind,
    duration_sec: f64,
    episodes: i64,
) -> Result<Option<Value>, String> {
    if title.is_empty() {
        return Ok(None);
    }
    let url = format!(
        "https://v2.sg.media-imdb.com/suggestion/x/{}.json?includeVideos=1",
        quote(title)
    );
    let response = match http.get(&url, None).await {
        Ok(r) if r.status == 200 => r,
        _ => return Ok(None),
    };
    let data = response.json()?;
    let Some(suggestions) = data
        .get("d")
        .and_then(Value::as_array)
        .filter(|d| !d.is_empty())
    else {
        return Ok(None);
    };
    choose(suggestions, title, year, kind, duration_sec, episodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suggestion(id: &str, l: &str, q: &str, y: i64) -> Value {
        json!({"id": id, "l": l, "q": q, "y": y, "s": "Actor One, Actor Two", "i": {"imageUrl": "https://img"}})
    }

    #[test]
    fn picks_like_ruvsarpur() {
        let d = vec![
            suggestion("tt1", "Father Brown", "TV series", 2013),
            suggestion("tt2", "Father Brown", "feature", 1954),
            suggestion("tt3", "The Father", "feature", 2020),
        ];
        // Two exact titles: falls through to similar title and type.
        let r = choose(&d, "Father Brown", None, Kind::TvShow, 2700.0, 10)
            .unwrap()
            .unwrap();
        assert_eq!(r["id"], "tt1");
        assert_eq!(r["foundvia"], "Similar primary title and type, first match");
        let r = choose(&d, "The Father", None, Kind::Movie, 6000.0, 1)
            .unwrap()
            .unwrap();
        assert_eq!(
            (r["id"].as_str(), r["foundvia"].as_str()),
            (Some("tt3"), Some("Exact primary title"))
        );
        assert!(
            choose(&d, "Nothing like it", Some("1999"), Kind::Movie, 6000.0, 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(quote("Séra Brown/x?"), "S%C3%A9ra%20Brown/x%3F");
    }
}
