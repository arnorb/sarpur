//! Where things live: the data folder, the Python worker and ffmpeg.

use std::path::{Path, PathBuf};

/// The data folder: `SARPUR_DATA_DIR`, else the platform's local data folder
/// (`%LOCALAPPDATA%\sarpur`, `~/Library/Application Support/sarpur`,
/// `$XDG_DATA_HOME/sarpur`).
pub fn default_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SARPUR_DATA_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("sarpur")
}

/// The Python interpreter for the worker: `SARPUR_PYTHON`, else `python3`.
pub fn python() -> String {
    env_or("SARPUR_PYTHON", "python3")
}

/// ffmpeg: `SARPUR_FFMPEG`, else `ffmpeg` on PATH.
pub fn ffmpeg() -> String {
    env_or("SARPUR_FFMPEG", "ffmpeg")
}

/// The worker script: `SARPUR_WORKER`, else the first `worker/ruvsarpur.py`
/// found in the executable's folder or one of its parents (so
/// `target/debug/sarpur` finds the repo's copy), else the repo path this
/// binary was built from.
pub fn worker_script() -> PathBuf {
    if let Some(path) = std::env::var_os("SARPUR_WORKER").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(found) = exe
            .ancestors()
            .skip(1)
            .map(|dir| dir.join("worker").join("ruvsarpur.py"))
            .find(|candidate| candidate.is_file())
        {
            return found;
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../worker/ruvsarpur.py")
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}
