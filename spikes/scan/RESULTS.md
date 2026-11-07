# Spike 4: disk scan with mp4ameta

## Built
- `scan-spike` (this dir): `mp4ameta = "=0.13.0"`, `serde_json = "=1.0.154"`. Walks the given dirs for `*.mp4`, reads `©cmt`, parses `ruvinfo:{pid}:{sid}`, prints JSONL `{path,size,mtime,pid,sid,tag_error}` and a summary JSON on stderr. Read-only.
- Local test (`out/test/`, ffmpeg-made files): tagged file -> pid=abc sid=123; untagged -> nulls; garbage file -> `tag_error` set and counted in `errors`. Works.
- Cross-compile for the NAS **worked**: `rustup target add x86_64-unknown-linux-musl` plus `.cargo/config.toml` with `linker = "rust-lld"`; `cargo build --release --target x86_64-unknown-linux-musl` gives a 835 KB static-pie ELF x86-64. No other tools needed.

## NAS run (2026-11-07)
- `/home`, `/tmp` and `/run` on TrueNAS are mounted `noexec`. With the owner's OK the binary ran from `/dev/shm` (tmpfs) as `admin`, read-only, and was deleted afterwards.
- Scanned `TV Shows` and `Movies` (`Sports` doesn't exist):

| Run | Elapsed | `.mp4` files | Tagged | Untagged | Errors |
| --- | --- | --- | --- | --- | --- |
| 1st (not truly cold: caches can't be dropped without root) | 9.5 s | 79 | 15 | 64 | 0 |
| 2nd (warm) | 0.07 s | 79 | 15 | 64 | 0 |

- **All ruvsarpur-style files carry the `ruvinfo` tag.** The 64 untagged files are Sonarr/Radarr downloads (`… WEBDL-1080p.mp4` and similar), and none look like ruvsarpur names. So tags alone identify every ruvsarpur file on the NAS today, and the name match (§3.3 b) is only a fallback for renamed or untagged files.
- The plan's "about 1,500 files" counts every video file; sarpur only reads `.mp4`, and there are 79 (210 GB). Both runs are far inside criterion 12 (60 s cold, 5 s warm).

## prevrecorded.log diff
- `admin` can read `/mnt/.ix-apps/app_mounts/ruvsarpur-api/storage_entry/prevrecorded.log` (world-readable). It holds **1 pid** (`ablmlh`, 7 bytes, written Nov 7 21:42, likely a test download). The production recorded list is effectively empty, so a diff against disk is trivial and says nothing about deleted-on-purpose episodes. Other files in that dir: `download-events.log`, `downloads/`, `gui-settings.json`, `imdb-cache.json`, `poster-cache/`, `tvschedule.json`.
- With one recorded pid there is nothing meaningful to diff. The app's data volume was apparently recreated recently, so the NAS has no download history for S2's import to bring over; the import mainly matters for the Windows user's data.
