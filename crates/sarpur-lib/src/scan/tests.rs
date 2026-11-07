use super::*;
use serde_json::json;

/// Copies the tiny test MP4 to `path` with `comment` as its `©cmt` tag (none
/// when `None`).
pub(crate) fn mp4_with_comment(path: &Path, comment: Option<&str>) {
    let tiny = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/tiny.mp4");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::copy(tiny, path).unwrap();
    let mut tag = mp4ameta::Tag::read_from_path(path).unwrap();
    tag.remove_comments();
    if let Some(comment) = comment {
        tag.set_comment(comment);
    }
    tag.write_to_path(path).unwrap();
}

async fn library_rows(db: &Db) -> Vec<(String, String, String)> {
    db.call(|c| {
        c.prepare("SELECT path, pid, source FROM library ORDER BY path")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect()
    })
    .await
    .unwrap()
}

#[test]
fn parses_ruvinfo() {
    assert_eq!(
        parse_ruvinfo("ruvinfo:abc1:123"),
        Some(("abc1".into(), "123".into()))
    );
    assert_eq!(parse_ruvinfo("ruvinfo::123"), None);
    assert_eq!(parse_ruvinfo("something else"), None);
    assert_eq!(parse_ruvinfo("ruvinfo:abc1"), None);
}

#[test]
fn the_fixture_carries_a_tag() {
    let tiny = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/tiny.mp4");
    assert_eq!(read_tag(&tiny).pid.as_deref(), Some("tpl"));
}

#[test]
fn roots_skip_duplicates_and_nested_folders() {
    let data = Path::new("/data");
    let mut s = settings::Settings::defaults(data);
    s.library_root_dir = "/media".into();
    s.shows_subdir = "TV".into();
    s.movies_subdir = String::new(); // the root itself, which holds TV
    s.sports_subdir = "/sport".into();
    assert_eq!(
        roots(&s, data),
        [PathBuf::from("/media"), PathBuf::from("/sport")]
    );
}

#[tokio::test]
async fn keeps_the_library_table_in_step_with_tags() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
    let media = dir.path().join("media");
    settings::update(
        &db,
        dir.path(),
        &json!({"libraryRootDir": media.to_string_lossy(), "showsSubdir": "TV", "moviesSubdir": "Movies"}),
    )
    .await
    .unwrap();
    let show = media.join("TV/Þáttur/Season 01/Þáttur - s01e01.mp4");
    let other = media.join("TV/Sonarr/Show.S01E01.WEBDL-1080p.mp4");
    mp4_with_comment(&show, Some("ruvinfo:p1:100"));
    mp4_with_comment(&other, Some("Encoded by someone"));
    std::fs::write(media.join("TV/Þáttur/broken.mp4"), b"not an mp4").unwrap();
    std::fs::write(media.join("TV/Þáttur/x.mp4.part"), b"partial").unwrap();
    let scanner = Scanner::new(db.clone(), dir.path().to_path_buf());

    // Cold: every file is read. The Movies and sports folders don't exist yet.
    let status = scanner.scan().await.unwrap();
    assert_eq!(
        (status.files, status.tagged, status.read, status.errors),
        (3, 1, 3, 1)
    );
    assert_eq!(status.roots.len(), 3);
    let show_text = show.to_string_lossy().into_owned();
    assert_eq!(
        library_rows(&db).await,
        [(show_text.clone(), "p1".into(), "tag".into())]
    );

    // Warm: nothing changed, nothing read.
    let status = scanner.scan().await.unwrap();
    assert_eq!((status.files, status.tagged, status.read), (3, 1, 0));

    // A folder moved by hand: the old row goes, the new path is found.
    let moved = media.join("TV/Annað nafn/Þáttur - s01e01.mp4");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::rename(&show, &moved).unwrap();
    // A sarpur job's row for a file that was deleted.
    let deleted = media.join("TV/Eytt/Eytt - s01e01.mp4");
    let deleted_text = deleted.to_string_lossy().into_owned();
    db.call(move |c| {
        c.execute(
            "INSERT INTO library (path, pid, size, mtime, source, verified_at)
             VALUES (?1, 'gone1', 1, 1, 'name', 'now')",
            [&deleted_text],
        )
    })
    .await
    .unwrap();
    let status = scanner.scan().await.unwrap();
    assert_eq!((status.read, status.removed), (1, 2));
    assert_eq!(
        library_rows(&db).await,
        [(
            moved.to_string_lossy().into_owned(),
            "p1".into(),
            "tag".into()
        )]
    );

    // A file whose tag was removed (a new size or mtime) is read again.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    mp4_with_comment(&moved, None);
    let status = scanner.scan().await.unwrap();
    assert_eq!((status.read, status.tagged, status.removed), (1, 0, 1));
    assert!(library_rows(&db).await.is_empty());

    // A library folder that disappears (an unmounted share) doesn't wipe the
    // rows under it.
    mp4_with_comment(&moved, Some("ruvinfo:p1:100"));
    scanner.scan().await.unwrap();
    std::fs::rename(media.join("TV"), dir.path().join("TV-away")).unwrap();
    let status = scanner.scan().await.unwrap();
    assert_eq!((status.files, status.removed), (0, 0));
    assert_eq!(library_rows(&db).await.len(), 1);
}
