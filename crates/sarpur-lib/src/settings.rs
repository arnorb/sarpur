//! GUI settings, stored one key per row in the `settings` table. The defaults
//! mirror ruvsarpur's `gui_api._default_settings`, minus the watchlist (follows
//! get their own tables) and the old `outputDir`, plus `quality`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::db::{Db, DbError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Normal,
    HD720,
    HD1080,
}

impl Quality {
    pub fn as_str(self) -> &'static str {
        match self {
            Quality::Normal => "Normal",
            Quality::HD720 => "HD720",
            Quality::HD1080 => "HD1080",
        }
    }

    /// Video bits per second, from ruvsarpur's `QUALITY_BITRATE`.
    pub fn bits_per_second(self) -> u64 {
        match self {
            Quality::Normal => 1_150_000,
            Quality::HD720 => 2_350_000,
            Quality::HD1080 => 3_550_000,
        }
    }

    pub fn parse(value: &str) -> Option<Quality> {
        match value {
            "Normal" => Some(Quality::Normal),
            "HD720" => Some(Quality::HD720),
            "HD1080" => Some(Quality::HD1080),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub auto_enabled: bool,
    pub auto_interval_minutes: u32,
    pub library_root_dir: String,
    pub shows_subdir: String,
    pub movies_subdir: String,
    pub sports_subdir: String,
    pub plex_base_url: String,
    pub plex_token: String,
    pub plex_library_section_id: String,
    pub plex_library_path: String,
    pub quality: Quality,
}

impl Settings {
    pub fn defaults(data_dir: &Path) -> Settings {
        Settings {
            auto_enabled: false,
            auto_interval_minutes: 60,
            library_root_dir: data_dir.join("downloads").to_string_lossy().into_owned(),
            shows_subdir: "shows".into(),
            movies_subdir: "movies".into(),
            sports_subdir: "sports".into(),
            plex_base_url: String::new(),
            plex_token: String::new(),
            plex_library_section_id: String::new(),
            plex_library_path: String::new(),
            quality: Quality::HD1080,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("invalid settings: {0}")]
    Invalid(String),
}

fn to_map(settings: &Settings) -> Map<String, Value> {
    match serde_json::to_value(settings) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

/// Defaults overlaid with the stored values. Stored values that no longer
/// parse are ignored.
pub async fn load(db: &Db, data_dir: &Path) -> Result<Settings, DbError> {
    let defaults = Settings::defaults(data_dir);
    let rows: Vec<(String, String)> = db
        .call(|c| {
            let mut stmt = c.prepare("SELECT key, value FROM settings")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let mut merged = to_map(&defaults);
    for (key, raw) in rows {
        if !merged.contains_key(&key) {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let mut candidate = merged.clone();
        candidate.insert(key, value);
        if serde_json::from_value::<Settings>(Value::Object(candidate.clone())).is_ok() {
            merged = candidate;
        }
    }
    Ok(serde_json::from_value(Value::Object(merged)).unwrap_or(defaults))
}

/// Applies the known keys of `patch` and stores them. Unknown keys (including
/// any `watchlistSids`) are ignored; a known key with the wrong type is an error.
pub async fn update(db: &Db, data_dir: &Path, patch: &Value) -> Result<Settings, SettingsError> {
    let Value::Object(patch) = patch else {
        return Err(SettingsError::Invalid("expected a JSON object".into()));
    };
    let current = load(db, data_dir).await?;
    let mut merged = to_map(&current);
    let mut changed = Vec::new();
    for (key, value) in patch {
        if merged.contains_key(key) {
            merged.insert(key.clone(), value.clone());
            changed.push((key.clone(), value.to_string()));
        }
    }
    let mut settings: Settings = serde_json::from_value(Value::Object(merged))
        .map_err(|e| SettingsError::Invalid(e.to_string()))?;
    // Same floor as gui_api.
    if settings.auto_interval_minutes < 5 {
        settings.auto_interval_minutes = 5;
        changed.retain(|(k, _)| k != "autoIntervalMinutes");
        changed.push(("autoIntervalMinutes".into(), "5".into()));
    }
    db.call(move |c| {
        let tx = c.transaction()?;
        for (key, value) in &changed {
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                (key, value),
            )?;
        }
        tx.commit()
    })
    .await?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn update_ignores_unknown_keys_and_validates_types() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("t.db")).unwrap();
        let s = load(&db, dir.path()).await.unwrap();
        assert_eq!(s, Settings::defaults(dir.path()));

        let s = update(
            &db,
            dir.path(),
            &json!({"libraryRootDir": "/media", "showsSubdir": "TV Shows", "watchlistSids": ["1"], "bogus": 1, "quality": "HD720"}),
        )
        .await
        .unwrap();
        assert_eq!(s.library_root_dir, "/media");
        assert_eq!(s.shows_subdir, "TV Shows");
        assert_eq!(s.quality, Quality::HD720);
        assert_eq!(load(&db, dir.path()).await.unwrap(), s);

        assert!(
            update(&db, dir.path(), &json!({"quality": "4K"}))
                .await
                .is_err()
        );
        assert!(
            update(&db, dir.path(), &json!({"autoEnabled": "yes"}))
                .await
                .is_err()
        );
        let s = update(&db, dir.path(), &json!({"autoIntervalMinutes": 1}))
            .await
            .unwrap();
        assert_eq!(s.auto_interval_minutes, 5);
        let stored: i64 = db
            .call(|c| {
                c.query_row(
                    "SELECT count(*) FROM settings WHERE key = 'watchlistSids'",
                    [],
                    |r| r.get(0),
                )
            })
            .await
            .unwrap();
        assert_eq!(stored, 0);
    }
}
