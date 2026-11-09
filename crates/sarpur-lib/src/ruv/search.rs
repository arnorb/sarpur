//! ruvsarpur's title search (`searchForItemsInTvSchedule` with `--find`, as
//! its GUI used it): an item matches when `fuzz.partial_ratio` of the query
//! against its shown title, title, series title or original title is over 85.

use serde_json::Value;

use super::fuzz::partial_ratio;
use super::naming::show_title;
use super::py::str_of;
use super::schedule::Schedule;

fn matches(query: &str, item: &Value) -> bool {
    let over = |text: &str| partial_ratio(query, &text.to_lowercase()) > 85;
    if item.get("title").is_some()
        && (over(&show_title(item, false, false)) || over(&str_of(&item["title"])))
    {
        return true;
    }
    if let Some(series) = item.get("series_title")
        && over(&str_of(series))
    {
        return true;
    }
    matches!(item.get("original-title"), Some(o) if !o.is_null() && over(&str_of(o)))
}

/// The pids of the matching items, in schedule order.
pub fn search(schedule: &Schedule, query: &str) -> Vec<String> {
    let query = query.to_lowercase();
    schedule
        .items()
        .iter()
        .filter(|item| item.get("pid").is_some() && matches(&query, item))
        .map(|item| str_of(&item["pid"]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn finds_by_any_title() {
        let mut s = Schedule::default();
        s.insert(json!({"pid": "1", "title": "Séra Brown (1 af 10)", "series_title": "Séra Brown",
                        "original-title": "Father Brown", "ep_num": "1", "ep_total": "10", "multiple_episodes": true}));
        s.insert(
            json!({"pid": "2", "title": "Veður", "series_title": "Veður", "original-title": null}),
        );
        assert_eq!(search(&s, "father brown"), ["1"]);
        assert_eq!(search(&s, "SÉRA"), ["1"]);
        assert_eq!(search(&s, "veð"), ["2"]);
        assert!(search(&s, "xyzzy").is_empty());
    }
}
