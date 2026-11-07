use super::*;
use serde_json::json;
use tempfile::TempDir;

fn testdata() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")
}

fn queue_with(dir: &TempDir, dry_run: bool, free: u64) -> Queue {
    let db = Db::open(&dir.path().join("sarpur.db")).unwrap();
    let worker = WorkerClient::new(
        crate::config::python(),
        testdata().join("fake_worker.py"),
        dir.path(),
    );
    Queue::new(
        db,
        worker,
        QueueConfig {
            data_dir: dir.path().to_path_buf(),
            ffmpeg: testdata()
                .join("fake_ffmpeg.py")
                .to_string_lossy()
                .into_owned(),
            dry_run,
            stop_grace: Duration::from_secs(5),
            free_space: Arc::new(move |_| Ok(free)),
        },
    )
}

fn queue(dir: &TempDir) -> Queue {
    queue_with(dir, false, u64::MAX)
}

async fn wait_for(queue: &Queue, id: &str, pred: impl Fn(&Job) -> bool) -> Job {
    for _ in 0..400 {
        let job = queue.get(id).await.unwrap().expect("job exists");
        if pred(&job) {
            return job;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for job {id}: {:?}", queue.get(id).await);
}

async fn wait_finished(queue: &Queue, id: &str) -> Job {
    wait_for(queue, id, |j| j.state.is_finished()).await
}

async fn origin(queue: &Queue, pid: &str) -> Option<String> {
    let pid = pid.to_string();
    queue
        .db()
        .call(move |c| {
            let mut stmt = c.prepare("SELECT origin FROM recorded WHERE pid = ?1")?;
            let mut rows = stmt.query_map([pid], |r| r.get(0))?;
            rows.next().transpose()
        })
        .await
        .unwrap()
}

async fn set_running(queue: &Queue, id: &str, part: Option<&Path>) {
    let id = id.to_string();
    let part = part.map(|p| p.to_string_lossy().into_owned());
    queue
        .db()
        .call(move |c| {
            c.execute(
                "UPDATE jobs SET state = 'running', part_path = ?2 WHERE id = ?1",
                (id, part),
            )
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn interactive_job_queued_during_scheduled_run_goes_next() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let mut scheduled = Vec::new();
    for _ in 0..10 {
        scheduled.push(
            q.enqueue("slow1", "", Mode::Web, Priority::Scheduled, None)
                .await
                .unwrap(),
        );
    }
    // The first scheduled job is running.
    set_running(&q, &scheduled[0].id, None).await;
    let interactive = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let next = q.db().call(|c| next_queued(c)).await.unwrap().unwrap();
    assert_eq!(next.id, interactive.id);

    // After it, FIFO among the scheduled ones.
    set_running(&q, &interactive.id, None).await;
    let next = q.db().call(|c| next_queued(c)).await.unwrap().unwrap();
    assert_eq!(next.id, scheduled[1].id);
}

#[tokio::test]
async fn interactive_job_runs_before_queued_scheduled_jobs_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let mut events = q.subscribe();
    let runner = q.spawn_runner();
    let first = q
        .enqueue("slow1", "", Mode::Web, Priority::Scheduled, None)
        .await
        .unwrap();
    let second = q
        .enqueue("slow2", "", Mode::Web, Priority::Scheduled, None)
        .await
        .unwrap();
    wait_for(&q, &first.id, |j| {
        j.state == State::Running && j.progress > 0
    })
    .await;
    let interactive = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    q.cancel_or_remove(&first.id).await.unwrap();
    wait_finished(&q, &interactive.id).await;

    let mut started = Vec::new();
    while let Ok(event) = events.try_recv() {
        if let Event::Job { job } = event {
            if job.state == State::Running && job.message.as_deref() == Some("Starting") {
                started.push(job.id);
            }
        }
    }
    assert_eq!(started[..2], [first.id.clone(), interactive.id.clone()]);
    q.cancel_or_remove(&second.id).await.unwrap();
    wait_finished(&q, &second.id).await;
    q.shutdown(runner).await;
}

#[tokio::test]
async fn restart_recovery_deletes_parts_and_requeues_interactive_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let interactive = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let scheduled = q
        .enqueue("lib1", "", Mode::Library, Priority::Scheduled, None)
        .await
        .unwrap();
    let untouched = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let part_a = dir.path().join("a.mp4.part");
    let part_b = dir.path().join("b.mp4.part");
    std::fs::write(&part_a, b"x").unwrap();
    std::fs::write(&part_b, b"x").unwrap();
    set_running(&q, &interactive.id, Some(&part_a)).await;
    set_running(&q, &scheduled.id, Some(&part_b)).await;
    drop(q);

    // A new server process.
    let q = queue(&dir);
    assert_eq!(q.recover().await.unwrap(), 2);
    assert!(!part_a.exists());
    assert!(!part_b.exists());
    let a = q.get(&interactive.id).await.unwrap().unwrap();
    assert_eq!(a.state, State::Queued);
    assert_eq!(a.part_path, None);
    let b = q.get(&scheduled.id).await.unwrap().unwrap();
    assert_eq!(b.state, State::Failed);
    assert_eq!(b.error.as_deref(), Some("interrupted"));
    assert_eq!(
        q.get(&untouched.id).await.unwrap().unwrap().state,
        State::Queued
    );
    // The requeued job keeps its place ahead of the later one.
    assert_eq!(
        q.db().call(|c| next_queued(c)).await.unwrap().unwrap().id,
        interactive.id
    );
}

#[tokio::test]
async fn refuses_when_disk_space_is_short() {
    let dir = tempfile::tempdir().unwrap();
    // 300 s at HD1080 is about 244 MB, so 200 MB free is less than twice that.
    let q = queue_with(&dir, false, 200_000_000);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Failed);
    let error = job.error.unwrap();
    assert!(error.starts_with("Not enough disk space"), "{error}");
    assert!(error.contains("200 MB free"), "{error}");
    assert!(!q.web_dir().join(&job.id).exists());
    q.shutdown(runner).await;

    // With 500 MB it goes ahead.
    let dir = tempfile::tempdir().unwrap();
    let q = queue_with(&dir, false, 500_000_000);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    assert_eq!(wait_finished(&q, &job.id).await.state, State::Completed);
    q.shutdown(runner).await;
}

async fn library_settings(q: &Queue, root: &Path) {
    settings::update(
        q.db(),
        q.data_dir(),
        &json!({"libraryRootDir": root.to_string_lossy(), "showsSubdir": "TV Shows"}),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn library_download_is_skipped_when_a_prefix_match_exists() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let root = dir.path().join("media");
    library_settings(&q, &root).await;
    let season = root.join("TV Shows/Þáttur [x]/Season 01");
    std::fs::create_dir_all(&season).unwrap();
    let existing = season.join("Þáttur [x] - s01e01 (mitt eintak).mp4");
    std::fs::write(&existing, b"x").unwrap();

    let runner = q.spawn_runner();
    let job = q
        .enqueue("lib1", "", Mode::Library, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Completed);
    assert_eq!(job.message.as_deref(), Some(ALREADY_IN_LIBRARY));
    assert_eq!(job.final_path.as_deref(), Some(existing.to_str().unwrap()));
    assert_eq!(std::fs::read_dir(&season).unwrap().count(), 1);
    assert_eq!(origin(&q, "lib1").await.as_deref(), Some("library"));
    q.shutdown(runner).await;
}

#[tokio::test]
async fn library_download_writes_the_plex_path_and_records_it() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let root = dir.path().join("media");
    library_settings(&q, &root).await;
    // A browser download of the same pid first: it must not block the library one.
    let runner = q.spawn_runner();
    let web = q
        .enqueue("lib1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    assert_eq!(wait_finished(&q, &web.id).await.state, State::Completed);
    assert_eq!(origin(&q, "lib1").await.as_deref(), Some("web"));

    let job = q
        .enqueue("lib1", "", Mode::Library, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Completed, "{:?}", job.error);
    let expected = root.join("TV Shows/Þáttur [x]/Season 01/Þáttur [x] - s01e01.mp4");
    assert!(expected.is_file());
    assert!(!media::part_path(&expected).exists());
    assert_eq!(job.final_path.as_deref(), Some(expected.to_str().unwrap()));
    assert!(job.files.is_empty());
    assert_eq!(origin(&q, "lib1").await.as_deref(), Some("library"));
    let source: String = q
        .db()
        .call(|c| {
            c.query_row("SELECT source FROM library WHERE pid = 'lib1'", [], |r| {
                r.get(0)
            })
        })
        .await
        .unwrap();
    assert_eq!(source, "tag");
    q.shutdown(runner).await;
}

#[tokio::test]
async fn web_download_completes_with_a_file_and_file_ready_event() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let mut events = q.subscribe();
    let runner = q.spawn_runner();
    let job = q
        .enqueue(
            "web1",
            "",
            Mode::Web,
            Priority::Interactive,
            Some("browser-a".into()),
        )
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Completed);
    assert_eq!(job.progress, 100);
    assert_eq!(job.title, "Þáttur s01e01");
    let file = q.web_dir().join(&job.id).join("Þáttur s01e01.mp4");
    assert!(file.is_file());
    assert_eq!(job.files[0].name, "Þáttur s01e01.mp4");
    assert_eq!(
        job.files[0].url,
        format!("/api/jobs/{}/files/%C3%9E%C3%A1ttur%20s01e01.mp4", job.id)
    );
    assert_eq!(origin(&q, "web1").await.as_deref(), Some("web"));

    let mut progress = Vec::new();
    let mut ready = None;
    while let Ok(event) = events.try_recv() {
        match event {
            Event::Job { job: j } if j.state == State::Running => progress.push(j.progress),
            Event::FileReady { .. } => ready = Some(event),
            _ => {}
        }
    }
    assert!(progress.contains(&50), "{progress:?}");
    let ready = ready.expect("file_ready sent");
    assert!(ready.visible_to(Some("browser-a")));
    assert!(!ready.visible_to(Some("browser-b")));
    assert!(!ready.visible_to(None));
    q.shutdown(runner).await;
}

#[tokio::test]
async fn failed_ffmpeg_leaves_no_files() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("fail1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Failed);
    assert_eq!(job.ffmpeg_exit, Some(1));
    assert!(job.error.unwrap().contains("404"));
    assert!(!q.web_dir().join(&job.id).exists());
    assert_eq!(origin(&q, "fail1").await, None);

    // Worker errors fail the job with the worker's message.
    let job = q
        .enqueue("nosuch", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(
        job.error.as_deref(),
        Some("pid nosuch not found in the schedule")
    );
    q.shutdown(runner).await;
}

#[tokio::test]
async fn cancel_and_remove() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("slow1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    let queued = q
        .enqueue("web1", "", Mode::Web, Priority::Scheduled, None)
        .await
        .unwrap();
    wait_for(&q, &job.id, |j| j.progress > 0).await;
    let part = PathBuf::from(q.get(&job.id).await.unwrap().unwrap().part_path.unwrap());
    assert!(part.exists());

    let queued = q.cancel_or_remove(&queued.id).await.unwrap().unwrap();
    assert_eq!(queued.state, State::Cancelled);
    q.cancel_or_remove(&job.id).await.unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Cancelled);
    assert!(!part.exists());
    assert!(!q.web_dir().join(&job.id).exists());

    // Finished jobs are removed from the list; retry requeues.
    let retried = q.retry(&queued.id).await.unwrap();
    assert_eq!(retried.state, State::Queued);
    assert_eq!(wait_finished(&q, &queued.id).await.state, State::Completed);
    assert!(q.cancel_or_remove(&job.id).await.unwrap().is_none());
    assert!(q.get(&job.id).await.unwrap().is_none());
    assert_eq!(q.list().await.unwrap().len(), 1);
    q.shutdown(runner).await;
}

#[tokio::test]
async fn shutdown_leaves_the_job_for_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("slow1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    wait_for(&q, &job.id, |j| j.progress > 0).await;
    q.shutdown(runner).await;
    let stopped = q.get(&job.id).await.unwrap().unwrap();
    assert_eq!(stopped.state, State::Running);
    let part = PathBuf::from(stopped.part_path.unwrap());
    assert!(part.exists());
    drop(q);

    let q = queue(&dir);
    q.recover().await.unwrap();
    assert!(!part.exists());
    assert_eq!(q.get(&job.id).await.unwrap().unwrap().state, State::Queued);
}

#[tokio::test]
async fn dry_run_runs_no_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue_with(&dir, true, u64::MAX);
    let runner = q.spawn_runner();
    let job = q
        .enqueue("lib1", "", Mode::Library, Priority::Interactive, None)
        .await
        .unwrap();
    let job = wait_finished(&q, &job.id).await;
    assert_eq!(job.state, State::Completed);
    assert_eq!(job.message.as_deref(), Some("dry run"));
    assert!(!Path::new(job.final_path.as_deref().unwrap()).exists());
    assert_eq!(origin(&q, "lib1").await, None);
    q.shutdown(runner).await;
}

#[tokio::test]
async fn sweeps_old_browser_downloads_but_not_active_jobs() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    let active = q
        .enqueue("web1", "", Mode::Web, Priority::Interactive, None)
        .await
        .unwrap();
    std::fs::create_dir_all(q.web_dir().join("finished")).unwrap();
    std::fs::write(q.web_dir().join("finished/a.mp4"), b"x").unwrap();
    std::fs::create_dir_all(q.web_dir().join(&active.id)).unwrap();

    assert_eq!(q.sweep_web(SystemTime::now()).await.unwrap(), 0);
    let later = SystemTime::now() + Duration::from_secs(25 * 3600);
    assert_eq!(q.sweep_web(later).await.unwrap(), 1);
    assert!(!q.web_dir().join("finished").exists());
    assert!(q.web_dir().join(&active.id).exists());
}

#[tokio::test]
async fn rejects_bad_pids() {
    let dir = tempfile::tempdir().unwrap();
    let q = queue(&dir);
    assert!(
        q.enqueue("../x", "", Mode::Web, Priority::Interactive, None)
            .await
            .is_err()
    );
    assert!(
        q.enqueue("", "", Mode::Web, Priority::Interactive, None)
            .await
            .is_err()
    );
}
