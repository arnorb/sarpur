//! The `-metadata` pairs ffmpeg writes into every download, ported from
//! ruvsarpur's `buildFfmpegMetadata` (R:676). The `comment` pair
//! (`ruvinfo:<pid>:<sid>`) is what the library scan reads back.

use serde_json::Value;

use super::naming::sanitize;
use super::py::{has, has_some, int_of, slice, str_at, truthy_at};

fn text(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .filter(|v| !v.is_null())
        .map(super::py::str_of)
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// `buildFfmpegMetadata`. An error where the Python would raise (an episode
/// or season number that isn't a number).
pub fn ffmpeg_metadata(item: &Value) -> Result<Vec<(String, String)>, String> {
    let is_movie = truthy_at(item, "is_movie");
    let is_docu = truthy_at(item, "is_docu");
    let is_sport = truthy_at(item, "is_sport");

    // The series description, favouring the longer of the two.
    let mut series_description = match text(item, "desc") {
        Some(desc) => Some(desc),
        None if has(item, "series_desc") => text(item, "series_desc"),
        None => None,
    };
    if let Some(short) = text(item, "series_sdesc")
        && series_description
            .as_ref()
            .is_none_or(|d| chars(&short) > chars(d))
    {
        series_description = Some(short);
    }

    let mut description = String::new();
    if is_movie || is_docu {
        description = series_description.clone().unwrap_or_default();
    }
    if description.is_empty()
        && let Some(episode_description) = item
            .get("episode")
            .and_then(|e| e.get("description"))
            .and_then(Value::as_str)
        && chars(episode_description) > 4
    {
        description = episode_description.to_string();
    }
    if chars(&description) < 4 {
        description = series_description.unwrap_or_default();
    }
    if is_sport {
        let showtime = str_at(item, "showtime");
        description = format!(
            "{}. Sýnt {}.{}.{} kl.{}",
            description.trim_end_matches('.'),
            slice(&showtime, 8, 10),
            slice(&showtime, 5, 7),
            slice(&showtime, 0, 4),
            slice(&showtime, 11, 16).replace(':', ".")
        );
    }

    let mut pairs = Vec::new();
    let mut push = |key: &str, value: String| pairs.push((key.to_string(), value));
    let title_key = if is_movie || is_docu || is_sport {
        "title"
    } else {
        "episode_title"
    };
    push("title", sanitize(&str_at(item, title_key)));
    push(
        "comment",
        format!("ruvinfo:{}:{}", str_at(item, "pid"), str_at(item, "sid")),
    );
    push("synopsis", sanitize(&description));
    if (!is_movie && !is_docu) || is_sport {
        push("show", sanitize(&str_at(item, "series_title")));
    }
    if (is_movie || is_docu)
        && let Some(imdb) = item.get("imdb").filter(|v| !v.is_null())
        && has_some(imdb, "year")
    {
        push("date", str_at(imdb, "year"));
    }
    if !is_movie && !is_docu {
        let number = |key: &str| {
            item.get(key)
                .and_then(int_of)
                .ok_or_else(|| format!("{key} {:?} isn't a number", str_at(item, key)))
        };
        push("episode_id", str_at(item, "ep_num"));
        push("episode_sort", number("ep_num")?.to_string());
        push("season_number", number("season_num")?.to_string());
    }
    let media_type = if is_movie || is_docu {
        "Movie"
    } else if is_sport {
        "Sports"
    } else {
        "TV Show"
    };
    push("media_type", media_type.to_string());
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_episode() {
        let item = json!({
            "pid": "p1", "sid": "s1", "title": "Taka tvö (1 af 10)", "episode_title": "Fyrsti þáttur",
            "series_title": "Taka tvö", "desc": "Stutt.", "series_sdesc": "Lengri lýsing á þáttunum.",
            "episode": {"description": "Lýsing þáttarins."}, "ep_num": "1", "season_num": "2",
            "is_movie": false, "is_docu": false, "is_sport": false,
        });
        let pairs = ffmpeg_metadata(&item).unwrap();
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("title"), Some("Fyrsti þáttur"));
        assert_eq!(get("comment"), Some("ruvinfo:p1:s1"));
        assert_eq!(get("synopsis"), Some("Lýsing þáttarins"));
        assert_eq!(get("season_number"), Some("2"));
        assert_eq!(get("media_type"), Some("TV Show"));
    }
}
