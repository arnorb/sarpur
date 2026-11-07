//! Library folder rules.

use std::io;
use std::path::{Path, PathBuf};

use crate::db::{Db, DbError};
use crate::settings::Settings;

/// The category folder for a library download, as ruvsarpur's
/// `gui_api._resolve_library_output_dir`: `libraryRootDir` joined with the
/// category's subfolder; an absolute subfolder stands on its own and an empty
/// one means the root itself. An empty root falls back to `<data>/downloads`.
pub fn output_dir(settings: &Settings, content_type: &str, data_dir: &Path) -> PathBuf {
    let root = settings.library_root_dir.trim();
    let root = if root.is_empty() {
        data_dir.join("downloads")
    } else {
        PathBuf::from(root)
    };
    let subdir = match content_type {
        "movie_or_docu" => &settings.movies_subdir,
        "sport" => &settings.sports_subdir,
        _ => &settings.shows_subdir,
    }
    .trim();
    if subdir.is_empty() {
        root
    } else if Path::new(subdir).is_absolute() {
        PathBuf::from(subdir)
    } else {
        root.join(subdir)
    }
}

/// Finds a file that matches `final_path` as a prefix glob,
/// `<final without .mp4>*.mp4`, in the same folder. This is ruvsarpur's
/// `--checklocal` rule, which lets users add text to the end of a file name.
/// It compares names directly instead of building a glob pattern, so `[`, `*`
/// and `?` in a name are plain characters (the S1 escaping bug can't happen).
pub fn find_existing(final_path: &Path) -> io::Result<Option<PathBuf>> {
    let (Some(dir), Some(name)) = (final_path.parent(), final_path.file_name()) else {
        return Ok(None);
    };
    let name = name.to_string_lossy();
    let stem = name.strip_suffix(".mp4").unwrap_or(&name);
    let entries = match std::fs::read_dir(if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    }) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let candidate = entry.file_name();
        let candidate = candidate.to_string_lossy();
        if candidate.starts_with(stem)
            && candidate.ends_with(".mp4")
            && candidate.len() >= stem.len() + 4
            && entry.file_type().map(|t| !t.is_dir()).unwrap_or(false)
        {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

/// §3.3 (a): a file the `library` table knows for `pid` (from its tag or a
/// finished job) that still exists, wherever it was moved within the library.
pub async fn known_file(db: &Db, pid: &str) -> Result<Option<PathBuf>, DbError> {
    let pid = pid.to_string();
    let paths: Vec<String> = db
        .call(move |c| {
            c.prepare("SELECT path FROM library WHERE pid = ?1 ORDER BY source = 'tag' DESC, path")?
                .query_map([pid], |r| r.get(0))?
                .collect()
        })
        .await?;
    Ok(tokio::task::spawn_blocking(move || {
        paths.into_iter().map(PathBuf::from).find(|p| p.is_file())
    })
    .await?)
}

/// What the UI shows per episode: `library` when a file for the pid is known
/// in the library (from its tag or a finished job), `recorded` when it was
/// downloaded for the library before (or by the old app) but no file is
/// known, so follow runs skip it until "Allow re-download".
pub async fn download_states(
    db: &Db,
) -> Result<std::collections::HashMap<String, &'static str>, DbError> {
    db.call(|c| {
        let mut states = std::collections::HashMap::new();
        let mut stmt = c.prepare(
            "SELECT pid FROM recorded WHERE origin IN ('library', 'unknown') AND forgotten_at IS NULL",
        )?;
        for pid in stmt.query_map([], |r| r.get::<_, String>(0))? {
            states.insert(pid?, "recorded");
        }
        let mut stmt = c.prepare("SELECT DISTINCT pid FROM library")?;
        for pid in stmt.query_map([], |r| r.get::<_, String>(0))? {
            states.insert(pid?, "library");
        }
        Ok(states)
    })
    .await
}

/// What `recorded` says about a pid.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recorded {
    pub pid: String,
    pub origin: String,
    pub recorded_at: String,
    pub forgotten_at: Option<String>,
}

/// "Allow re-download": sets `forgotten_at`, so follow runs no longer skip the
/// pid because it was recorded (a file still on disk is still skipped).
/// `None` when the pid isn't recorded.
pub async fn forget(db: &Db, pid: &str) -> Result<Option<Recorded>, DbError> {
    let pid = pid.to_string();
    db.call(move |c| {
        c.execute(
            "UPDATE recorded SET forgotten_at = COALESCE(forgotten_at, ?2) WHERE pid = ?1",
            (&pid, crate::now_iso()),
        )?;
        let mut stmt = c.prepare(
            "SELECT pid, origin, recorded_at, forgotten_at FROM recorded WHERE pid = ?1",
        )?;
        let mut rows = stmt.query_map([&pid], |r| {
            Ok(Recorded {
                pid: r.get(0)?,
                origin: r.get(1)?,
                recorded_at: r.get(2)?,
                forgotten_at: r.get(3)?,
            })
        })?;
        rows.next().transpose()
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_folders() {
        let data = Path::new("/data");
        let mut s = Settings::defaults(data);
        s.library_root_dir = "/media".into();
        s.shows_subdir = "TV Shows".into();
        s.movies_subdir = String::new();
        s.sports_subdir = "/elsewhere/Sports".into();
        assert_eq!(
            output_dir(&s, "show", data),
            PathBuf::from("/media/TV Shows")
        );
        assert_eq!(
            output_dir(&s, "movie_or_docu", data),
            PathBuf::from("/media")
        );
        assert_eq!(
            output_dir(&s, "sport", data),
            PathBuf::from("/elsewhere/Sports")
        );
        s.library_root_dir = "  ".into();
        assert_eq!(
            output_dir(&s, "show", data),
            PathBuf::from("/data/downloads/TV Shows")
        );
    }

    #[test]
    fn prefix_glob_treats_glob_characters_literally() {
        let dir = tempfile::tempdir().unwrap();
        let season = dir.path().join("Þáttur [*?]").join("Season 01");
        std::fs::create_dir_all(&season).unwrap();
        let expected = season.join("Þáttur [*?] - s01e02.mp4");

        assert_eq!(find_existing(&expected).unwrap(), None, "empty folder");

        // A .part file is not a finished episode.
        std::fs::write(season.join("Þáttur [*?] - s01e02.mp4.part"), b"x").unwrap();
        // A name that a glob would match if `[*?]` were a pattern, but isn't a prefix.
        std::fs::write(season.join("Þáttur * - s01e02.mp4"), b"x").unwrap();
        std::fs::write(season.join("Þáttur [*?] - s01e03.mp4"), b"x").unwrap();
        assert_eq!(find_existing(&expected).unwrap(), None);

        let renamed = season.join("Þáttur [*?] - s01e02 (my copy).mp4");
        std::fs::write(&renamed, b"x").unwrap();
        assert_eq!(find_existing(&expected).unwrap(), Some(renamed));

        assert_eq!(
            find_existing(&dir.path().join("missing").join("x.mp4")).unwrap(),
            None
        );
    }
}
