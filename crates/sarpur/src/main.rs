//! sarpur: the server. Downloads RÚV Sarpur episodes through a queue, with the
//! RÚV client (`sarpur_lib::ruv`) for the catalogue and naming, and ffmpeg for
//! the video.

mod api;
mod auth;
mod web;

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::Context;
use clap::Parser;
use sarpur_lib::catalogue::Catalogue;
use sarpur_lib::config;
use sarpur_lib::db::Db;
use sarpur_lib::eventlog::{self, EventLog};
use sarpur_lib::follows::Follows;
use sarpur_lib::jobs::{Queue, QueueConfig, real_free_space};
use sarpur_lib::posters::PosterCache;
use sarpur_lib::ruv::{Ruv, http::Http};
use sarpur_lib::scan::Scanner;
use sarpur_lib::updates::UpdateChecker;
use tokio::sync::watch;

#[derive(Parser, Debug)]
#[command(version, about = "Downloads RÚV Sarpur episodes through a web API")]
struct Args {
    /// Address to listen on.
    #[arg(long, env = "SARPUR_BIND", default_value = "127.0.0.1")]
    bind: IpAddr,
    /// Port to listen on.
    #[arg(long, env = "SARPUR_PORT", default_value_t = 8090)]
    port: u16,
    /// Data folder (database, schedule, browser downloads). Default:
    /// SARPUR_DATA_DIR, else the platform's local data folder.
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Run jobs without ffmpeg: log what would be downloaded and finish them
    /// as "dry run". Follow runs only report what they would queue.
    #[arg(long)]
    dry_run: bool,
    /// Ask the server on 127.0.0.1:<port> for /api/health and exit 0 when it
    /// answers 200 (the Docker health check).
    #[arg(long)]
    healthcheck: bool,
}

/// `--healthcheck`: a plain HTTP/1.0 request, so the image needs no curl.
fn healthcheck(port: u16) -> std::process::ExitCode {
    use std::io::{Read, Write};
    let ok = (|| -> std::io::Result<bool> {
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut stream = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(4))?;
        stream.set_read_timeout(Some(Duration::from_secs(4)))?;
        stream.write_all(b"GET /api/health HTTP/1.0\r\nHost: 127.0.0.1\r\n\r\n")?;
        let mut head = [0u8; 12];
        stream.read_exact(&mut head)?;
        Ok(head.starts_with(b"HTTP/1.") && &head[9..12] == b"200")
    })()
    .unwrap_or(false);
    if ok {
        std::process::ExitCode::SUCCESS
    } else {
        std::process::ExitCode::FAILURE
    }
}

fn main() -> anyhow::Result<std::process::ExitCode> {
    let args = Args::parse();
    if args.healthcheck {
        return Ok(healthcheck(args.port));
    }
    serve(args)?;
    Ok(std::process::ExitCode::SUCCESS)
}

#[tokio::main]
async fn serve(args: Args) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        // Plain text in Docker logs and redirected output.
        .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stdout()))
        .init();
    // New files are group-writable unless the filesystem's ACL says otherwise.
    #[cfg(unix)]
    // SAFETY: umask only sets the process's file mode mask.
    unsafe {
        libc::umask(0o002);
    }

    let data_dir = args.data_dir.unwrap_or_else(config::default_data_dir);
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("creating the data folder {}", data_dir.display()))?;
    tracing::info!(
        "data folder {}, ffmpeg {}{}",
        data_dir.display(),
        config::ffmpeg(),
        if args.dry_run { " (dry run)" } else { "" }
    );

    let db = Db::open(&data_dir.join("sarpur.db")).context("opening the database")?;
    // SARPUR_RUV_REPLAY answers RÚV requests from recordings (the e2e tests).
    let http = match std::env::var_os("SARPUR_RUV_REPLAY").filter(|v| !v.is_empty()) {
        Some(dir) => Http::replay(std::path::Path::new(&dir))
            .map_err(|e| anyhow::anyhow!("SARPUR_RUV_REPLAY: {e}"))?,
        None => Http::live(),
    };
    let ruv = Ruv::new(&data_dir, http);
    let queue = Queue::new(
        db.clone(),
        ruv.clone(),
        QueueConfig {
            data_dir: data_dir.clone(),
            ffmpeg: config::ffmpeg(),
            dry_run: args.dry_run,
            stop_grace: Duration::from_secs(10),
            free_space: Arc::new(real_free_space),
        },
    );
    queue
        .recover()
        .await
        .context("recovering interrupted jobs")?;
    let runner = queue.spawn_runner();
    // The NDJSON event log, in ruvsarpur's format and file.
    let log = EventLog::new(&data_dir);
    eventlog::spawn_logger(&queue, log.clone());

    // Browser downloads: sweep now and hourly.
    {
        let queue = queue.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(3600));
            loop {
                tick.tick().await;
                if let Err(e) = queue.sweep_web(SystemTime::now()).await {
                    tracing::warn!("sweep: {e}");
                }
            }
        });
    }

    let catalogue = Arc::new(Catalogue::new());
    {
        let catalogue = catalogue.clone();
        let ruv = ruv.clone();
        let data_dir = data_dir.clone();
        tokio::spawn(async move {
            catalogue.load_at_startup(&ruv, &data_dir).await;
        });
    }

    // The library scan runs in the background, never blocking startup.
    let scanner = Arc::new(Scanner::new(db.clone(), data_dir.clone()));
    {
        let scanner = scanner.clone();
        tokio::spawn(async move {
            let _ = scanner.scan().await;
        });
    }

    // Follow runs on the settings' interval while autoEnabled is on.
    let follows = Arc::new(Follows::new(
        db,
        ruv.clone(),
        catalogue.clone(),
        queue.clone(),
        scanner.clone(),
        data_dir.clone(),
    ));
    follows.spawn_scheduler(args.dry_run);

    // The update banner: GitHub Releases, daily.
    let updates = Arc::new(UpdateChecker::from_env(queue.http().clone()));
    updates.spawn();

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let app = api::router(api::AppState {
        queue: queue.clone(),
        catalogue,
        ruv,
        follows,
        scanner,
        posters: Arc::new(PosterCache::new(
            data_dir.join("poster-cache"),
            queue.http().clone(),
        )),
        log,
        auth: auth::Auth::from_env(&data_dir).context("setting up SARPUR_PASSWORD")?,
        updates,
        dry_run: args.dry_run,
        shutdown: shutdown_rx,
    });
    let addr = SocketAddr::new(args.bind, args.port);
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("listening on {addr}"))?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            tracing::info!("shutting down");
            // End the SSE streams, then stop ffmpeg before draining requests.
            let _ = shutdown_tx.send(true);
            queue.shutdown(runner).await;
        })
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}
