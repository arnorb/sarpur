use super::*;
use crate::jobs::{ALREADY_IN_LIBRARY, QueueConfig, State};
use crate::ruv::testing;
use crate::scan::tests::mp4_with_comment;
use serde_json::{Value, json};
use tempfile::TempDir;

#[test]
fn title_key_is_strict() {
    assert_eq!(title_key("  Fréttir "), "fréttir");
    // The first matching suffix in ruvsarpur's order is dropped, once.
    assert_eq!(title_key("Fyrir alla muni 3"), "fyrir alla muni");
    assert_eq!(title_key("Ófærð II"), "ófærð");
    assert_eq!(title_key("Ófærð III"), "ófærð");
    assert_eq!(title_key("Ófærð 2 2"), "ófærð 2");
    assert_eq!(title_key("Þáttur 12"), "þáttur");
    assert_eq!(
        title_key("Sögur Snjómannsins"),
        title_key("Sögur snjómannsins")
    );
    // Parentheses stay: accessibility variants are different shows.
    assert_eq!(
        title_key("Fréttir (með táknmálstúlkun)"),
        "fréttir (með táknmálstúlkun)"
    );
    // NFC: a decomposed "á" gives the same key as a composed one.
    assert_eq!(title_key("Þa\u{301}ttur"), title_key("Þáttur"));
}

#[test]
fn labels_drop_trailing_parentheses() {
    assert_eq!(series_label("Fréttir (með táknmálstúlkun)"), "Fréttir");
    assert_eq!(series_label("Show (a) (b) "), "Show");
    assert_eq!(series_label("(Bara sviginn)"), "(Bara sviginn)");
    assert_eq!(series_label("Engir svigar"), "Engir svigar");
}

/// An episode of `series_title`. [`Setup::new`] numbers the episodes of
/// each series in the order given.
fn item(pid: &str, sid: &str, series_title: &str, english: bool, showtime: &str) -> Value {
    let mut item = testing::episode(pid, sid, series_title, 1, 1, showtime);
    item["english_subtitled"] = json!(english);
    item
}

struct Setup {
    dir: TempDir,
    ruv: Ruv,
    queue: Queue,
    follows: Follows,
}

impl Setup {
    /// `catalogue`: `{"items": [...], "series": {"<sid>": [(pid, firstrun)...]},
    /// "refresh_fails": bool}`. Series in `series` are on RÚV but not in the
    /// schedule.
    async fn new(catalogue: Value) -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let items = catalogue["items"].as_array().cloned().unwrap_or_default();
        let mut fake = testing::FakeRuv::new();
        let mut numbers: HashMap<String, u32> = HashMap::new();
        let totals = items
            .iter()
            .fold(HashMap::<String, u32>::new(), |mut m, i| {
                *m.entry(i["sid"].as_str().unwrap().to_string()).or_default() += 1;
                m
            });
        for mut item in items {
            let sid = item["sid"].as_str().unwrap().to_string();
            let n = numbers.entry(sid.clone()).or_default();
            *n += 1;
            let total = totals[&sid].max(2);
            let series_title = item["series_title"].as_str().unwrap().to_string();
            item["ep_num"] = json!(n.to_string());
            item["ep_total"] = json!(total.to_string());
            item["multiple_episodes"] = json!(true);
            item["title"] = json!(format!("{series_title} ({n} af {total})"));
            fake = fake.playable(item, "ok");
        }
        if let Some(series) = catalogue["series"].as_object() {
            for (sid, episodes) in series {
                let title = episodes["title"].as_str().unwrap();
                let list: Vec<(&str, &str)> = episodes["episodes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| (e[0].as_str().unwrap(), e[1].as_str().unwrap()))
                    .collect();
                fake = fake.series_page(sid, title, &list);
            }
        }
        if !catalogue["refresh_fails"].as_bool().unwrap_or(false) {
            fake = fake.featured(&[]);
        }
        let ruv = fake.build(dir.path());
        let testdata = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata");
        let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
        let queue = Queue::new(
            db.clone(),
            ruv.clone(),
            QueueConfig {
                data_dir: dir.path().to_path_buf(),
                ffmpeg: testdata
                    .join("fake_ffmpeg.py")
                    .to_string_lossy()
                    .into_owned(),
                dry_run: false,
                stop_grace: Duration::from_secs(1),
                free_space: Arc::new(|_| Ok(u64::MAX)),
            },
        );
        let cat = Arc::new(Catalogue::new());
        assert!(cat.load(&ruv, false).await);
        let scanner = Arc::new(Scanner::new(db.clone(), dir.path().to_path_buf()));
        let follows = Follows::new(
            db,
            ruv.clone(),
            cat,
            queue.clone(),
            scanner,
            dir.path().to_path_buf(),
        );
        Setup {
            dir,
            ruv,
            queue,
            follows,
        }
    }

    /// Where a library download of `pid` goes (`title` is its series).
    fn episode_path(&self, title: &str, pid: &str) -> PathBuf {
        let name = self.ruv.names(&[pid.to_string()], NameMode::Plex).remove(0);
        let path = self
            .dir
            .path()
            .join("downloads/shows")
            .join(name.relative_path.expect("pid in the schedule"));
        assert!(path.starts_with(self.dir.path().join("downloads/shows").join(title)));
        path
    }

    async fn queued_pids(&self) -> Vec<String> {
        let mut pids: Vec<String> = self
            .queue
            .list()
            .await
            .unwrap()
            .into_iter()
            .filter(|j| j.state == State::Queued)
            .map(|j| {
                assert_eq!((j.mode, j.priority), (Mode::Library, Priority::Scheduled));
                j.pid
            })
            .collect();
        pids.sort();
        pids
    }

    async fn sql(&self, sql: &'static str) {
        self.follows
            .db()
            .call(move |c| c.execute_batch(sql))
            .await
            .unwrap();
    }
}

const RUN: RunOptions = RunOptions {
    refresh: true,
    dry_run: false,
};

fn sids(follow: &Follow) -> Vec<(&str, &str)> {
    follow
        .sids
        .iter()
        .map(|s| (s.sid.as_str(), s.added_by.as_str()))
        .collect()
}

/// Criterion 7: sid B joins the follow of A only when its title key matches
/// exactly and its English flag is equal.
#[tokio::test]
async fn adds_only_sids_with_the_same_key_and_flag() {
    let s = Setup::new(json!({"items": [
        item("a1", "A", "Þáttur", false, "2026-01-01T20:00:00"),
        item("b1", "B", "Þáttur 2", false, "2026-02-01T20:00:00"),
        item("c1", "C", "Þáttur", true, "2026-02-02T20:00:00"),
        item("d1", "D", "Þáttur (með táknmálstúlkun)", false, "2026-02-03T20:00:00"),
        item("e1", "E", "Þáttur í dag", false, "2026-02-04T20:00:00"),
        item("f1", "F", "Annar þáttur", false, "2026-02-05T20:00:00"),
    ]}))
    .await;
    let follow = s.follows.follow_sid("A").await.unwrap();
    assert_eq!(follow.label, "Þáttur");
    assert_eq!(sids(&follow), [("A", "user")]);

    // A dry run reports B but adds and queues nothing.
    let report = s
        .follows
        .run(RunOptions {
            refresh: true,
            dry_run: true,
        })
        .await
        .unwrap();
    assert_eq!(report.added_sids.len(), 1);
    assert_eq!(report.added_sids[0].sid, "B");
    let scheduled: Vec<&str> = report.scheduled.iter().map(|e| e.pid.as_str()).collect();
    assert_eq!(scheduled, ["a1", "b1"]);
    assert_eq!(
        sids(&list(s.follows.db()).await.unwrap()[0]),
        [("A", "user")]
    );
    assert!(s.queued_pids().await.is_empty());

    let report = s.follows.run(RUN).await.unwrap();
    assert_eq!(report.added_sids.len(), 1);
    assert_eq!(s.queued_pids().await, ["a1", "b1"]);
    let follows = list(s.follows.db()).await.unwrap();
    assert_eq!(follows.len(), 1);
    assert_eq!(sids(&follows[0]), [("A", "user"), ("B", "auto")]);
    assert_eq!(s.follows.status().last_run_state, Some("ok"));

    // A removed sid isn't added back.
    let id = follows[0].id;
    remove_sid(s.follows.db(), id, "B").await.unwrap();
    let report = s.follows.run(RUN).await.unwrap();
    assert!(report.added_sids.is_empty());
    assert_eq!(
        sids(&list(s.follows.db()).await.unwrap()[0]),
        [("A", "user")]
    );
    // Following it by hand brings it back as the user's.
    s.follows.follow_sid("B").await.unwrap();
    assert_eq!(
        sids(&list(s.follows.db()).await.unwrap()[0]),
        [("A", "user"), ("B", "user")]
    );

    // The English-subtitled version is its own follow; removing its only
    // sid deletes it.
    let english = s.follows.follow_sid("C").await.unwrap();
    assert_ne!(english.id, id);
    assert!(english.english_subtitled);
    assert_eq!(
        remove_sid(s.follows.db(), english.id, "C").await.unwrap(),
        None
    );
    assert_eq!(list(s.follows.db()).await.unwrap().len(), 1);
    assert!(remove(s.follows.db(), id).await.unwrap());
    assert!(list(s.follows.db()).await.unwrap().is_empty());
}

/// The §3.3 rule for follow runs: skip what's on disk (tag/library row or a
/// prefix match), recorded for the library, or already queued.
#[tokio::test]
async fn schedules_only_what_is_missing() {
    let s = Setup::new(json!({"items": [
        item("tag1", "S", "Sería", false, "2026-01-01T20:00:00"),
        item("ren1", "S", "Sería", false, "2026-01-02T20:00:00"),
        item("del1", "S", "Sería", false, "2026-01-03T20:00:00"),
        item("unk1", "S", "Sería", false, "2026-01-04T20:00:00"),
        item("web1", "S", "Sería", false, "2026-01-05T20:00:00"),
        item("que1", "S", "Sería", false, "2026-01-06T20:00:00"),
        item("gone1", "S", "Sería", false, "2026-01-07T20:00:00"),
        item("new1", "S", "Sería", false, "2026-01-08T20:00:00"),
        item("oth1", "O", "Önnur", false, "2026-01-09T20:00:00"),
    ]}))
    .await;
    s.follows.follow_sid("S").await.unwrap();

    // tag1: in the library table, at a moved path that exists.
    let moved = s.dir.path().join("elsewhere/x.mp4");
    std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
    std::fs::write(&moved, b"x").unwrap();
    // gone1: in the library table, but the file is gone and it's not recorded.
    let sql = format!(
        "INSERT INTO library (path, pid, size, mtime, source, verified_at)
         VALUES ('{}', 'tag1', 1, 1, 'tag', 'now'), ('/nope/gone.mp4', 'gone1', 1, 1, 'tag', 'now');
         INSERT INTO recorded (pid, origin, recorded_at) VALUES
           ('del1', 'library', 'now'), ('unk1', 'unknown', 'now'), ('web1', 'web', 'now');",
        moved.display()
    );
    s.follows
        .db()
        .call(move |c| c.execute_batch(&sql))
        .await
        .unwrap();
    // ren1: renamed by the user, matched as a prefix.
    let renamed = s.episode_path("Sería", "ren1").with_extension("");
    let renamed = PathBuf::from(format!("{} (copy).mp4", renamed.display()));
    std::fs::create_dir_all(renamed.parent().unwrap()).unwrap();
    std::fs::write(&renamed, b"x").unwrap();
    // que1: already queued.
    s.queue
        .enqueue("que1", "", Mode::Library, Priority::Scheduled, None)
        .await
        .unwrap();

    let report = s.follows.run(RUN).await.unwrap();
    assert_eq!(
        report.skipped,
        Skipped {
            on_disk: 1,
            name_match: 1,
            recorded: 2,
            in_queue: 1,
        }
    );
    let scheduled: Vec<&str> = report.scheduled.iter().map(|e| e.pid.as_str()).collect();
    assert_eq!(scheduled, ["web1", "gone1", "new1"], "oldest first");
    assert_eq!(
        report.scheduled[2].path,
        s.episode_path("Sería", "new1").to_string_lossy()
    );
    assert_eq!(s.queued_pids().await, ["gone1", "new1", "que1", "web1"]);

    // "Allow re-download" (forgotten) makes a recorded pid eligible again,
    // and a second run doesn't queue the same pids twice.
    s.sql("UPDATE recorded SET forgotten_at = 'now' WHERE pid = 'del1'")
        .await;
    let report = s.follows.run(RUN).await.unwrap();
    let scheduled: Vec<&str> = report.scheduled.iter().map(|e| e.pid.as_str()).collect();
    assert_eq!(scheduled, ["del1"]);
    assert_eq!(report.skipped.in_queue, 4);
}

#[tokio::test]
async fn ruv_down_is_a_warning_and_schedules_nothing() {
    let s = Setup::new(json!({"refresh_fails": true, "items": [
        item("a1", "A", "Þáttur", false, "2026-01-01T20:00:00"),
    ]}))
    .await;
    s.follows.follow_sid("A").await.unwrap();
    let report = s.follows.run(RUN).await.unwrap();
    assert!(report.scheduled.is_empty());
    let status = s.follows.status();
    assert_eq!(status.last_run_state, Some("warning"));
    assert!(
        status
            .last_run_message
            .unwrap()
            .contains("Could not refresh the catalogue"),
        "{:?}",
        s.follows.status()
    );
    assert!(!status.running);
    assert!(s.queued_pids().await.is_empty());

    // Without a refresh it goes ahead on the loaded catalogue.
    let report = s
        .follows
        .run(RunOptions {
            refresh: false,
            dry_run: false,
        })
        .await
        .unwrap();
    assert_eq!(report.scheduled.len(), 1);
}

#[tokio::test]
async fn a_followed_sid_outside_the_catalogue_is_fetched() {
    let s = Setup::new(json!({
        "items": [item("a1", "A", "Þáttur", false, "2026-01-01T20:00:00")],
        "series": {"Z": {"title": "Gamalt", "episodes": [["z1", "2020-01-01T20:00:00"]]}},
    }))
    .await;
    let follow = s.follows.follow_sid("Z").await.unwrap();
    assert_eq!(follow.label, "Gamalt");
    let report = s.follows.run(RUN).await.unwrap();
    let scheduled: Vec<&str> = report.scheduled.iter().map(|e| e.pid.as_str()).collect();
    assert_eq!(scheduled, ["z1"]);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    assert!(matches!(
        s.follows.follow_sid("nosuch").await,
        Err(FollowError::NotFound(_))
    ));
}

/// Criterion 6: a deleted-but-recorded pid, a renamed file (prefix match), a
/// tagged file in a moved folder and a browser-only pid. A follow run
/// schedules only the browser-only pid; Library downloads also download the
/// deleted one.
#[tokio::test]
async fn criterion_6_library_rule() {
    let s = Setup::new(json!({"items": [
        item("del1", "S", "Sería", false, "2026-01-01T20:00:00"),
        item("ren1", "S", "Sería", false, "2026-01-02T20:00:00"),
        item("tag1", "S", "Sería", false, "2026-01-03T20:00:00"),
        item("web1", "S", "Sería", false, "2026-01-04T20:00:00"),
    ]}))
    .await;
    s.follows.follow_sid("S").await.unwrap();
    // del1 was downloaded to the library, then deleted on purpose.
    // web1 was only ever a browser download.
    s.sql(
        "INSERT INTO recorded (pid, origin, recorded_at) VALUES
           ('del1', 'library', 'now'), ('web1', 'web', 'now');",
    )
    .await;
    // ren1: the user added text to the end of the name.
    let renamed = s.episode_path("Sería", "ren1").with_extension("");
    let renamed = PathBuf::from(format!("{} - gamalt.mp4", renamed.display()));
    mp4_with_comment(&renamed, None);
    // tag1: tagged, in a folder the user moved; nothing in the database.
    let moved = s
        .dir
        .path()
        .join("downloads/shows/Sería (gamla möppan)/tag1.mp4");
    mp4_with_comment(&moved, Some("ruvinfo:tag1:S"));

    let report = s
        .follows
        .run(RunOptions {
            refresh: false,
            dry_run: true,
        })
        .await
        .unwrap();
    let scheduled: Vec<&str> = report.scheduled.iter().map(|e| e.pid.as_str()).collect();
    assert_eq!(scheduled, ["web1"]);
    assert_eq!(
        report.skipped,
        Skipped {
            on_disk: 1,
            name_match: 1,
            recorded: 1,
            in_queue: 0,
        }
    );

    let runner = s.queue.spawn_runner();
    let mut messages = Vec::new();
    for pid in ["del1", "ren1", "tag1", "web1"] {
        let job = s
            .queue
            .enqueue(pid, "", Mode::Library, Priority::Interactive, None)
            .await
            .unwrap();
        let job = loop {
            let job = s.queue.get(&job.id).await.unwrap().unwrap();
            if job.state.is_finished() {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(job.state, State::Completed, "{pid}: {:?}", job.error);
        messages.push((pid, job.message.unwrap(), job.final_path.unwrap()));
    }
    s.queue.shutdown(runner).await;
    let downloaded = |pid| {
        (
            pid,
            "Downloaded".to_string(),
            s.episode_path("Sería", pid).to_string_lossy().into_owned(),
        )
    };
    assert_eq!(
        messages,
        [
            downloaded("del1"),
            (
                "ren1",
                ALREADY_IN_LIBRARY.into(),
                renamed.to_string_lossy().into_owned()
            ),
            (
                "tag1",
                ALREADY_IN_LIBRARY.into(),
                moved.to_string_lossy().into_owned()
            ),
            downloaded("web1"),
        ]
    );
}

/// After the video: subtitles in both modes (browser downloads list them),
/// posters for the library, and one Plex refresh once the queue is idle.
#[tokio::test]
async fn downloads_fetch_extras_and_refresh_plex_once() {
    use crate::testutil::{MockServer, Reply};
    let ruv = MockServer::start(|path| match path {
        "/is.vtt" => Reply::ok(
            "text/vtt",
            "WEBVTT\n\n00:01.000 --> 00:02.000\nHalló\n".as_bytes(),
        ),
        "/library/sections/2/refresh" => Reply::ok("text/plain", ""),
        _ => Reply::ok("image/jpeg", path.as_bytes()),
    })
    .await;
    let with_extras = |pid: &str| {
        let mut v = item(pid, "S", "Sería", false, "2026-01-01T20:00:00");
        v["subtitles"] = json!([{"name": "is", "value": format!("{}/is.vtt", ruv.url)}]);
        v["portrait_image"] = json!(format!("{}/portrait", ruv.url));
        v["episode_image"] = json!(format!("{}/episode-{pid}", ruv.url));
        v
    };
    let s = Setup::new(json!({"items": [with_extras("e1"), with_extras("e2"), with_extras("w1")]}))
        .await;
    settings::update(
        s.follows.db(),
        s.dir.path(),
        &json!({"plexBaseUrl": ruv.url, "plexToken": "tok", "plexLibrarySectionId": "2"}),
    )
    .await
    .unwrap();

    let runner = s.queue.spawn_runner();
    let mut ids = Vec::new();
    for pid in ["e1", "e2"] {
        let job = s
            .queue
            .enqueue(pid, "", Mode::Library, Priority::Scheduled, None)
            .await
            .unwrap();
        ids.push(job.id);
    }
    let web = s
        .queue
        .enqueue_with_languages(
            "w1",
            "",
            Mode::Web,
            Priority::Scheduled,
            None,
            Some(vec!["IS".into()]),
        )
        .await
        .unwrap();
    ids.push(web.id.clone());
    for id in &ids {
        loop {
            let job = s.queue.get(id).await.unwrap().unwrap();
            if job.state.is_finished() {
                assert_eq!(job.state, State::Completed, "{:?}", job.error);
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }
    for _ in 0..100 {
        if s.queue.last_plex().is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    s.queue.shutdown(runner).await;

    let e1 = s.episode_path("Sería", "e1");
    let season = e1.parent().unwrap();
    let stem = e1.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(stem, "Sería - s01e01");
    assert!(season.join(format!("{stem}.is.srt")).is_file());
    assert!(season.join(format!("{stem}.is.vtt")).is_file());
    assert_eq!(
        std::fs::read_to_string(season.join(format!("{stem}.jpg"))).unwrap(),
        "/episode-e1"
    );
    assert_eq!(
        std::fs::read_to_string(season.parent().unwrap().join("poster.jpg")).unwrap(),
        "/portrait"
    );

    let web = s.queue.get(&web.id).await.unwrap().unwrap();
    let names: Vec<&str> = web.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Sería s01e03.mp4", "Sería s01e03.is.srt"]);
    assert!(
        !s.dir
            .path()
            .join("web")
            .join(&web.id)
            .join("poster.jpg")
            .exists()
    );

    assert_eq!(s.queue.last_plex().unwrap().state, "ok");
    let refreshes = ruv
        .requests()
        .iter()
        .filter(|r| r.starts_with("GET /library/sections/2/refresh"))
        .count();
    assert_eq!(refreshes, 1, "one refresh for the batch");
}
