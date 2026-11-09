use super::*;
use crate::jobs::{Queue, QueueConfig};
use crate::ruv::{Ruv, testing};
use crate::scan::tests::mp4_with_comment;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("testdata/import")
        .join(name)
}

struct Setup {
    dir: TempDir,
    db: Db,
    ruv: Ruv,
    catalogue: Arc<Catalogue>,
    scanner: Arc<Scanner>,
    follows: Follows,
    log: EventLog,
}

impl Setup {
    async fn new() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        // No saved schedule: the import copies the old one in. Series S is
        // on RÚV; the watch list's "gone" isn't.
        let ruv = testing::FakeRuv::new()
            .series_page("S", "Sería", &[("s1", "2026-01-01T20:00:00")])
            .build_empty(dir.path());
        let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
        let queue = Queue::new(
            db.clone(),
            ruv.clone(),
            QueueConfig {
                data_dir: dir.path().to_path_buf(),
                ffmpeg: "ffmpeg".into(),
                dry_run: false,
                stop_grace: Duration::from_secs(1),
                free_space: Arc::new(|_| Ok(u64::MAX)),
            },
        );
        let catalogue = Arc::new(Catalogue::new());
        catalogue.load(&ruv, false).await;
        let scanner = Arc::new(Scanner::new(db.clone(), dir.path().to_path_buf()));
        let follows = Follows::new(
            db.clone(),
            ruv.clone(),
            catalogue.clone(),
            queue,
            scanner.clone(),
            dir.path().to_path_buf(),
        );
        let log = EventLog::new(dir.path());
        Setup {
            dir,
            db,
            ruv,
            catalogue,
            scanner,
            follows,
            log,
        }
    }

    async fn apply(&self, source: &Path) -> Applied {
        apply(
            source,
            Services {
                db: &self.db,
                data_dir: self.dir.path(),
                follows: &self.follows,
                catalogue: &self.catalogue,
                ruv: &self.ruv,
                log: &self.log,
            },
        )
        .await
        .unwrap()
    }

    async fn recorded(&self) -> Vec<(String, String)> {
        self.db
            .call(|c| {
                c.prepare("SELECT pid, origin FROM recorded ORDER BY pid")?
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect()
            })
            .await
            .unwrap()
    }
}

fn mapping(preview: &Preview) -> Vec<(&str, &str, &str)> {
    preview
        .path_mapping
        .iter()
        .map(|m| (m.key.as_str(), m.from.as_str(), m.to.as_str()))
        .collect()
}

#[test]
fn maps_only_the_old_data_folder() {
    let data = Path::new("/srv/sarpur");
    assert_eq!(map_path("/data/downloads", data), "/srv/sarpur/downloads");
    assert_eq!(map_path(" /data ", data), "/srv/sarpur");
    assert_eq!(map_path("/database", data), "/database");
    assert_eq!(map_path("/media/TV Shows", data), "/media/TV Shows");
    assert_eq!(
        map_path("C:\\Users\\x\\downloads", data),
        "C:\\Users\\x\\downloads"
    );
    assert_eq!(map_path("TV Shows", data), "TV Shows");
}

/// Criterion 13, NAS half: the mapping is shown first and nothing changes
/// until the import is applied.
#[tokio::test]
async fn nas_settings() {
    let s = Setup::new().await;
    let preview = super::preview(&fixture("nas"), s.dir.path(), &s.db)
        .await
        .unwrap();
    assert_eq!(
        mapping(&preview),
        [
            ("libraryRootDir", "/media", "/media"),
            ("showsSubdir", "TV Shows", "TV Shows"),
            ("moviesSubdir", "Movies", "Movies"),
            ("sportsSubdir", "Sports", "Sports"),
        ]
    );
    assert_eq!(
        preview.recorded,
        RecordedCounts {
            total: 1,
            web: 1,
            ..RecordedCounts::default()
        }
    );
    assert_eq!(preview.event_logs, 1);
    assert!(preview.follow_sids.is_empty());
    assert!(s.recorded().await.is_empty(), "a preview changes nothing");

    let applied = s.apply(&fixture("nas")).await;
    assert!(applied.warnings.is_empty(), "{:?}", applied.warnings);
    assert_eq!(applied.recorded_added, 1);
    assert_eq!(s.recorded().await, [("ablmlh".into(), "web".into())]);
    let settings = settings::load(&s.db, s.dir.path()).await.unwrap();
    assert_eq!(settings.library_root_dir, "/media");
    assert_eq!(settings.shows_subdir, "TV Shows");

    // The old single-folder setup: outputDir under the old /data.
    let old = tempfile::tempdir().unwrap();
    std::fs::write(
        old.path().join("gui-settings.json"),
        r#"{"outputDir": "/data/downloads", "libraryRootDir": ""}"#,
    )
    .unwrap();
    let preview = super::preview(old.path(), s.dir.path(), &s.db)
        .await
        .unwrap();
    let expected = s
        .dir
        .path()
        .join("downloads")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        mapping(&preview),
        [("libraryRootDir", "/data/downloads", expected.as_str())]
    );
    assert!(preview.path_mapping[0].changed);
}

/// Criterion 13, Windows half: a portable folder with backslash paths, CRLF
/// lines, a watchlist and rotated event logs.
#[tokio::test]
async fn windows_portable_folder() {
    let s = Setup::new().await;
    // Already recorded here: kept as it is.
    s.db.call(|c| {
        c.execute(
            "INSERT INTO recorded (pid, origin, recorded_at) VALUES ('unk1', 'library', 'then')",
            [],
        )
    })
    .await
    .unwrap();
    let source = fixture("windows");
    let preview = super::preview(&source, s.dir.path(), &s.db).await.unwrap();
    assert_eq!(
        mapping(&preview),
        [
            (
                "libraryRootDir",
                "C:\\Users\\Notandi\\ruvsarpur\\downloads",
                "C:\\Users\\Notandi\\ruvsarpur\\downloads"
            ),
            ("showsSubdir", "Þættir", "Þættir"),
            ("moviesSubdir", "Kvikmyndir", "Kvikmyndir"),
            ("sportsSubdir", "D:\\Íþróttir", "D:\\Íþróttir"),
        ]
    );
    assert_eq!(preview.settings["plexToken"], "••••", "tokens are masked");
    assert_eq!(preview.settings["autoIntervalMinutes"], 5);
    assert_eq!(preview.follow_sids, ["S", "gone"]);
    assert_eq!(preview.event_logs, 2);
    assert_eq!(preview.warnings.len(), 1, "{:?}", preview.warnings);
    assert_eq!(
        preview.recorded,
        RecordedCounts {
            total: 4,
            library: 2,
            web: 1,
            unknown: 1,
            already_known: 1,
        }
    );
    assert_eq!(preview.copies, ["tvschedule.json"]);

    let applied = s.apply(&source).await;
    assert_eq!(applied.recorded_added, 3);
    assert_eq!(
        s.recorded().await,
        [
            ("both1".into(), "library".into()),
            ("lib1".into(), "library".into()),
            ("unk1".into(), "library".into()),
            ("web1".into(), "web".into()),
        ]
    );
    assert_eq!(applied.follows_added, ["S"]);
    assert_eq!(applied.warnings.len(), 1, "{:?}", applied.warnings);
    assert!(applied.warnings[0].contains("gone"));
    let settings = settings::load(&s.db, s.dir.path()).await.unwrap();
    assert_eq!(settings.plex_token, "leyndó");
    assert!(settings.auto_enabled);
    assert_eq!(settings.auto_interval_minutes, 5);
    assert!(s.dir.path().join("tvschedule.json").is_file());

    // The import is in the event log.
    let log = std::fs::read_to_string(s.log.path()).unwrap();
    let line: Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!(
        (line["event"].as_str(), line["status"].as_str()),
        (Some("import"), Some("warning"))
    );
    assert_eq!(line["details"]["recordedAdded"], 3);
    assert!(!log.contains("leyndó"), "no secrets in the log");

    // Importing again adds nothing.
    let again = s.apply(&source).await;
    assert_eq!(again.recorded_added, 0);
}

#[tokio::test]
async fn rejects_folders_without_old_data() {
    let s = Setup::new().await;
    let empty = tempfile::tempdir().unwrap();
    assert!(matches!(
        preview(empty.path(), s.dir.path(), &s.db).await,
        Err(ImportError::Invalid(_))
    ));
    assert!(matches!(
        preview(&empty.path().join("nope"), s.dir.path(), &s.db).await,
        Err(ImportError::Invalid(_))
    ));
}

#[tokio::test]
async fn report_compares_recorded_with_the_disk() {
    let s = Setup::new().await;
    s.apply(&fixture("windows")).await;
    let root = s.dir.path().join("media");
    settings::update(
        &s.db,
        s.dir.path(),
        &json!({"libraryRootDir": root.to_string_lossy()}),
    )
    .await
    .unwrap();
    // lib1 is on disk (the import set showsSubdir to Þættir); an untracked
    // tagged file too.
    mp4_with_comment(&root.join("Þættir/A/lib1.mp4"), Some("ruvinfo:lib1:S"));
    mp4_with_comment(&root.join("Þættir/B/x.mp4"), Some("ruvinfo:extra1:S"));

    let report = report(&s.db, &s.scanner, &s.catalogue).await.unwrap();
    assert_eq!(report.scan_error, None);
    let missing: Vec<(&str, &str)> = report
        .recorded_but_missing
        .iter()
        .map(|m| (m.pid.as_str(), m.origin.as_str()))
        .collect();
    // web1 is browser-only, so it isn't expected on disk.
    assert_eq!(missing, [("both1", "library"), ("unk1", "unknown")]);
    assert_eq!(report.on_disk_not_recorded.len(), 1);
    assert_eq!(report.on_disk_not_recorded[0].pid, "extra1");
}
