//! SQLite storage: WAL mode, foreign keys on, migrations by `PRAGMA user_version`.
//! All access goes through [`Db::call`], which runs on tokio's blocking pool.

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

pub use rusqlite;

/// Each entry moves the schema up one version. Never edit a shipped entry;
/// add a new one.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL            -- JSON
);
CREATE TABLE jobs (
    id          TEXT PRIMARY KEY,
    queue_order INTEGER NOT NULL,  -- FIFO order within a priority
    client_id   TEXT,
    pid         TEXT NOT NULL,
    title       TEXT NOT NULL,
    mode        TEXT NOT NULL CHECK (mode IN ('web', 'library')),
    priority    TEXT NOT NULL CHECK (priority IN ('interactive', 'scheduled')),
    state       TEXT NOT NULL CHECK (state IN ('queued', 'running', 'completed', 'failed', 'cancelled')),
    progress    INTEGER NOT NULL DEFAULT 0,
    message     TEXT,
    part_path   TEXT,
    final_path  TEXT,
    files       TEXT NOT NULL DEFAULT '[]',  -- JSON array of file names (web jobs)
    ffmpeg_exit INTEGER,
    error       TEXT,
    created_at  TEXT NOT NULL,
    started_at  TEXT,
    finished_at TEXT
);
CREATE INDEX jobs_state ON jobs (state, priority, queue_order);
CREATE TABLE recorded (
    pid          TEXT PRIMARY KEY,
    origin       TEXT NOT NULL CHECK (origin IN ('library', 'web', 'unknown')),
    recorded_at  TEXT NOT NULL,
    forgotten_at TEXT
);
CREATE TABLE library (
    path        TEXT PRIMARY KEY,
    pid         TEXT NOT NULL,
    size        INTEGER NOT NULL,
    mtime       INTEGER NOT NULL,
    source      TEXT NOT NULL CHECK (source IN ('tag', 'name', 'import')),
    verified_at TEXT NOT NULL
);
CREATE INDEX library_pid ON library (pid);
"#,
    r#"
-- A follow is one show: the sids the user chose plus sids added because their
-- strict title key and English flag match (plan §3.5).
CREATE TABLE follows (
    id           INTEGER PRIMARY KEY,
    title_key    TEXT NOT NULL,
    english_flag INTEGER NOT NULL,
    label        TEXT NOT NULL,
    created_at   TEXT NOT NULL,
    UNIQUE (title_key, english_flag)
);
CREATE TABLE follow_sids (
    follow_id    INTEGER NOT NULL REFERENCES follows (id) ON DELETE CASCADE,
    sid          TEXT NOT NULL,
    series_title TEXT NOT NULL,
    english_flag INTEGER NOT NULL,
    added_by     TEXT NOT NULL CHECK (added_by IN ('user', 'auto')),
    added_at     TEXT NOT NULL,
    removed_at   TEXT,              -- set when removed, so it isn't added again
    PRIMARY KEY (follow_id, sid)
);
CREATE INDEX follow_sids_sid ON follow_sids (sid);
"#,
    r#"
-- What the disk scan last read from each .mp4, so a warm scan only reads the
-- tags of new or changed files. `pid`/`sid` are NULL for untagged files.
CREATE TABLE scan_cache (
    path  TEXT PRIMARY KEY,
    size  INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    pid   TEXT,
    sid   TEXT,
    error TEXT
);
"#,
];

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Opens (creating if needed) and migrates the database. Blocking; call it
    /// at startup.
    pub fn open(path: &Path) -> Result<Db, DbError> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        migrate(&mut conn)?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Runs `f` with the connection on the blocking pool.
    pub async fn call<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&mut conn)
        })
        .await?;
        Ok(result?)
    }
}

pub fn schema_version() -> i64 {
    MIGRATIONS.len() as i64
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current.max(0) as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", index as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn migrates_once_and_uses_wal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sarpur.db");
        let db = Db::open(&path).unwrap();
        let (mode, version, fk): (String, i64, i64) = db
            .call(|c| {
                Ok((
                    c.pragma_query_value(None, "journal_mode", |r| r.get(0))?,
                    c.pragma_query_value(None, "user_version", |r| r.get(0))?,
                    c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?,
                ))
            })
            .await
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(version, schema_version());
        assert_eq!(fk, 1);
        db.call(|c| {
            c.execute(
                "INSERT INTO recorded (pid, origin, recorded_at) VALUES ('p1', 'web', 'now')",
                [],
            )
        })
        .await
        .unwrap();
        drop(db);

        // Reopening keeps data and doesn't re-run migrations.
        let db = Db::open(&path).unwrap();
        let count: i64 = db
            .call(|c| c.query_row("SELECT count(*) FROM recorded", [], |r| r.get(0)))
            .await
            .unwrap();
        assert_eq!(count, 1);
        let bad = db
            .call(|c| {
                c.execute(
                    "INSERT INTO recorded (pid, origin, recorded_at) VALUES ('p2', 'nope', 'now')",
                    [],
                )
            })
            .await;
        assert!(bad.is_err(), "origin is checked");
    }
}
