//! Library folder rules.

use std::io;
use std::path::{Path, PathBuf};

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
