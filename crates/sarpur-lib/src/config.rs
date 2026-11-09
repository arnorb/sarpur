//! Where things live: the data folder and ffmpeg.

use std::path::PathBuf;

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

/// ffmpeg: `SARPUR_FFMPEG`, else `ffmpeg` on PATH.
pub fn ffmpeg() -> String {
    env_or("SARPUR_FFMPEG", "ffmpeg")
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}
