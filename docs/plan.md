# Plan: `sarpur`, a staged Rust port of ruvsarpur (revision 2)

**Status: approved 2026-11-07.** This is a hobby project, so treat the plan as a guide rather than a contract. Decisions left open, such as how long Python may stay or the speed and Windows checklists, are made when they come up.
**Date:** 2026-11-07. Revision 2 adds the Architect and Critic reviews and the user's choice of a **staged port**.
**Source repo:** `ruvsarpur` @ `648519d`. File references:
- `R:` = `src/ruvsarpur.py`
- `G:` = `src/gui_api.py`
- `A:` = `frontend/src/App.tsx`
- Known defects: `notes/ui-review-2026-11-07.md`

Line references (`R:1234`) point at ruvsarpur @ `648519d`. The vendored copy in `worker/ruvsarpur.py` has since been edited (the `plan` command, `buildFfmpegMetadata`), so its line numbers drift.

This file moved here from `ruvsarpur/.omc/plans/sarpur-rewrite.md` on 2026-11-07, so it is tracked. Update **Status** at the end of every slice.

## Status

Last updated: 2026-11-07.

| Step | State | Where / evidence |
| --- | --- | --- |
| **S1** steps 1–6: fix the Python app | Done | `ruvsarpur` tag `v-s1`. A follow-up fix (ffmpeg writes `.part`, renamed on success) is in `212bf1c`. |
| **S1** step 7: deploy to the NAS | Done | Both TrueNAS apps redeployed with the new images on 2026-11-07. No PyInstaller build (decision below). |
| **S2.0** spikes 1–4, 6 | Done | `spikes/README.md` and each spike's `RESULTS.md` |
| **S2.0** spike 5: ffmpeg on Windows | Deferred | Passes on a GitHub Windows runner. The Defender/SmartScreen check on the Windows user's PC waits for the Windows add-on (decision below). |
| **S2.0** spike 7: TrueNAS permissions | Partly | Library files are `568:568`, mode `770` from the dataset ACL. Still to check: `.part` then rename from a container running as 568. |
| **S2.0** spike 8: end to end | Mac half done | Through the slice 1 server on macOS (`spikes/README.md` row 8). NAS half at cutover; Windows half deferred. |
| **S2.1** slice 1: core download path | Done | Merged to `main` (`5c8462e`). Details below. |
| **S2.1** slice 2: follows, scheduler, tag scan, posters/subtitles, Plex, poster proxy, import | Done | Merged to `main`. Criteria 6, 7 and 14 are tests; 11, 12 and 13 have their local halves (below). See "Slice 2 progress". |
| **S2.1** slice 3: UI port | Next | |
| **S2.1** slice 4: packaging (Docker; the Windows zip is deferred) | Open | |
| **S2.2** cutover on the NAS | Open | Simpler now: the NAS can be replaced outright (decision below). |
| **S3** port the worker to Rust | Open | |

### Decisions since approval

- **2026-11-07, follow key (spike 6):** `title_key` drops `normalizeSeriesLabel`. Stripping parentheses would auto-add the sign-language and audio-described versions. §3.5 is updated.
- **2026-11-07, Windows spikes:** the Windows user runs them on their own PC. A GitHub Actions workflow (`Windows spike 5`) smoke-tests the script first.
- **2026-11-07, no PyInstaller in `ruvsarpur`:** the Windows user gets a packaged app from S2. The S2 Windows zip still plans a PyInstaller worker; revisit at slice 4.
- **2026-11-07, `getProgramType`:** RÚV rejects the persisted query, but only 4 radio items need it. Not fixed in ruvsarpur. The worker's `resolve`/`plan` dropped the GraphQL fallback and fail with a clear message instead.
- **2026-11-07, documentaries (`is_docu` always false):** RÚV renamed the category slug (`heimildarefni`). Left as is so existing library names stay stable; decide in S3 together with naming.
- **2026-11-07, movie Follow:** kept. Movies are sometimes released in parts over several weeks. Known edge case for S3 naming: when RÚV's titles carry no "(N af M)", part 1 is named as a plain movie until part 2 appears, after which it would be expected as `… - part01`. The slice 2 tag check stops the re-download, but the names stay mixed.
- **2026-11-07, slice 2 follows:** no `episodes` table in S2. The worker's `tvschedule.json` stays the catalogue cache until S3 replaces the worker. A follow is unique by `(title_key, english_flag)`; the flag is stored on `follows` and on each `follow_sids` row. A removed sid is kept as removed so runs don't add it back. A failed RÚV refresh keeps the catalogue loaded before it.
- **2026-11-07, slice 2 scan:** the scan covers the three category folders, not all of `libraryRootDir`. A missing category folder keeps its `library` rows, so an unmounted share doesn't wipe them. Any `library` row whose file exists counts as (a), whichever source wrote it.
- **2026-11-07, slice 2 extras and Plex:** Plex is refreshed once the queue goes idle after library downloads, not after each one (the old app refreshed after each automatic run). The poster proxy answers 502 instead of 400 when RÚV fails and nothing is cached; 400 stays for URLs outside the allowlist. Artwork keeps ruvsarpur's `.jpg` names even though RÚV now serves WebP (Plex reads both). Subtitle or artwork failures never fail a job.
- **2026-11-07, HTTP client:** `reqwest` 0.13 with `rustls`, which uses `aws-lc-rs`. That needs a C compiler at build time; check it in the slice 4 Docker and Windows builds (fallback: `rustls-no-provider` with `ring`).
- **2026-11-07, NAS replaced outright (owner):** the NAS hasn't really been running ruvsarpur, so it can be taken down and replaced by sarpur as soon as sarpur can run there (the Docker image, slice 4) — no import or side-by-side period. Criterion 11 becomes a dry run on the fresh NAS install; the import is mainly for the Windows user.
- **2026-11-07, Windows is a later add-on (owner):** Windows work doesn't block anything. Deferred: spike 5's PC check, the Windows zip and console/Job Object work (§3.8), the Windows-only sanitising and long paths, criterion 15, and criterion 13 with the Windows user's real data. The code stays portable and the CI smoke test stays, but the path to "done" is Mac for development and the NAS for production.
- **2026-11-07, slice 2 import and log:** pids from automatic runs import as `unknown`, because the old log only recorded sids for them (`unknown` still blocks follow re-downloads, like `library`). The import never overwrites a `recorded` row. sarpur writes the same `download-events.log` as ruvsarpur, in the same shape, so a reused data folder keeps one history.
- **2026-11-07, permissions (spike 7):** run as 568 and let the dataset ACL decide modes; criterion 16's "mode 664" follows the dataset instead.

### S2.1 slice 1: what's built

A running `sarpur` server takes one RÚV episode through queue → worker → ffmpeg → `.part` → final name, in library and browser modes, with its state in SQLite.

- **Workspace:** `crates/sarpur-lib` (modules `config`, `settings`, `db`, `worker`, `catalogue`, `library`, `media`, `jobs`) and `crates/sarpur` (the axum server and CLI).
- **CLI:** `sarpur [--bind 127.0.0.1] [--port 8090] [--data-dir DIR] [--dry-run]`. Environment: `SARPUR_DATA_DIR`, `SARPUR_PYTHON` (default `python3`), `SARPUR_WORKER` (default: the first `worker/ruvsarpur.py` in the executable's folder or a parent, else the repo it was built from), `SARPUR_FFMPEG` (default `ffmpeg`), `RUST_LOG`. On unix the server sets `umask 002`.
- **Worker:** one `python ruvsarpur.py --json --portable <command>` per call, run in the data folder, with a timeout per command. The `plan` command returns the name, content type, playlist, duration and the exact ffmpeg metadata, built by `buildFfmpegMetadata`, which the old download path shares.
- **Database:** `settings`, `jobs`, `recorded`, `library`; WAL, foreign keys, migrations by `user_version`; all access on the blocking pool.
- **Queue:** one ffmpeg at a time, interactive before scheduled, FIFO within a priority. Startup recovery deletes leftover `.part` files, requeues interactive jobs and fails scheduled ones with "interrupted". Disk-space check (2× the estimate). Cancel sends `q`, kills after 10 s, then deletes the `.part`. SIGTERM or Ctrl-C stops ffmpeg and leaves the job for recovery. Retry requeues the same job.
- **Library rule:** an explicit Library download completes as "Already in the library" when a file matches the expected name as a prefix (`<name>*.mp4`, compared literally so `[`/`*`/`?` are safe). Library jobs record origin `library` plus a `library` row; browser jobs only ever write origin `web`.
- **Browser downloads:** `<data>/web/<job>/<name>`, served at `/api/jobs/{id}/files/{name}`, swept after 24 h (at startup and hourly).
- **Catalogue:** kept in memory, loaded from the worker at startup (in the background) and after `POST /api/catalogue/refresh`; substring search.
- **API:** see `docs/api.md`. SSE job updates go to everyone; `file_ready` goes only to the matching `client_id`.

### Slice 2 progress

- **Part 1, small fixes (done):** HD1080 estimate 6.5 Mbit/s; the catalogue is fetched from RÚV at startup when the data folder has no `tvschedule.json`; the remaining blocking file operations run off the async runtime.
- **Part 2, follows and the scheduler (done):** `sarpur-lib::follows`, the `follows`/`follow_sids` tables (migration 2), the `/api/follows` routes and `event: follows` (`docs/api.md`). The worker gained `names` (many pids per call) and `series --save`. Criterion 7 is a test (`follows::tests::adds_only_sids_with_the_same_key_and_flag`). The run already applies all three §3.3 checks; (a) reads the `library` table, which part 3's tag scan fills. Checked live on the pinned catalogue: following *Fyrir alla muni* adds its 3 other season sids.
- **Part 3, tag scan and the full §3.3 rule (done):** `sarpur-lib::scan` reads `©cmt` with `mp4ameta` 0.13 from the three category folders, caches results in `scan_cache` (migration 3) and keeps `library` rows with source `tag` in step. It runs in the background at startup, before each follow run and from `POST /api/library/scan`. Explicit Library downloads now also check (a). "Allow re-download" is `POST /api/recorded/{pid}/forget`. Criterion 6 is a test (`follows::tests::criterion_6_library_rule`) with a real tagged MP4 in a moved folder. Criterion 12, locally: 1,500 tagged files scanned in 224 ms cold and 33 ms warm (debug build); still to be timed on the NAS.
- **Part 4, posters, subtitles, Plex, poster proxy (done):** `subtitles` (VTT → SRT, ported from R:382), `extras` (subtitles in both modes, artwork for the library, as ruvsarpur names them), `plex` (refresh with `X-Plex-Token` as a header), `posters` (the proxy: allowlist, 24 h cache, stale on failure). The worker's `plan` gained `images`; jobs gained `subtitleLanguages` (migration 4). Criterion 14: `fixtures/subtitles/` holds VTT edge cases with the SRT ruvsarpur writes, checked by `worker/test_subtitles.py` and `subtitles::tests` (byte for byte); `plex::tests::sends_the_token_as_a_header` (mock server); the API test gets 400 for hosts outside the allowlist. Checked live: a RÚV poster through the proxy.
- **Part 5, import and the event log (done):** `import` (preview with the path mapping, apply, report) and `eventlog` (ruvsarpur's `download-events.log` format and rotation; queue, follow-run, Plex and import events). Criterion 13 is tested on the NAS's real `gui-settings.json` (redacted) and a Windows portable folder built from the old formats (`testdata/import/`). Checked live with the real worker.
- **Still open from slice 2's criteria, needing the NAS:** 12 (cold under 60 s, warm under 5 s; spike 4 suggests about 10 s and 0.1 s) and 11 as a dry run on the fresh install. 13's Windows half waits for the Windows add-on.

### Slice 2 scope

1. Small fixes first:
   - The disk-space estimate for HD1080 becomes 6.5 Mbit/s. `QUALITY_BITRATE`'s 3.55 Mbit/s undershoots real files, which run at about 6.2 Mbit/s.
   - Refresh the catalogue automatically when the data folder has none.
   - Move the remaining blocking file operations (`create_dir_all`, the prefix-check `read_dir`, the sweep's `remove_dir_all`) off the async runtime.
2. Follows and the scheduler: §3.5 with the strict key, the `episodes`/`follows`/`follow_sids` tables, criterion 7, and `--dry-run` for follow runs (criterion 11). Movies can be followed.
3. Tag scan into `library` (§3.2 disk scan, criterion 12), and the full §3.3 rule (criterion 6).
4. Posters and subtitles with VTT → SRT (criterion 14), the Plex refresh with the `X-Plex-Token` header, and the poster proxy (allowlist plus 24 h cache).
5. Import of the old settings and history, with the report (§3.4, criterion 13), and the NDJSON event log.

## 1. Requirements summary

| | Decision |
| --- | --- |
| End state | A new repo, `sarpur`: one Rust program (axum, tokio, rusqlite) with a React, Vite and TypeScript UI built in. It ships as a Windows x64 zip and as a Docker image (`ghcr.io/arnorb/sarpur`). |
| Users | **Owner:** TrueNAS SCALE 25.04, writing to `/Media/{TV Shows,Movies,Sports}`, which Plex reads. **Second user:** their own copy on Windows. |
| Strategy | **A staged port.** Every stage leaves both users with a working app. |
| ffmpeg | Docker: included in the image. Windows: downloaded on first run if missing (a fixed BtbN build, SHA-256 checked). |
| Kept | Plex folder and file naming, artwork, MP4 tags, IMDB matching, Plex refresh, subtitles converted from VTT to SRT |
| Dropped | The full command-line mode; the program only starts the server, with `--port`, `--data-dir`, `--bind` and `--dry-run`. The IMDB `title.basics.tsv` input (the GUI passes `None` at G:771). |
| Import | Old settings and download history, with the source of each recorded episode reconstructed (§3.4) |

### Staging

| Stage | Where | Result |
| --- | --- | --- |
| **S1. Fix the Python app** | the `ruvsarpur` repo | The HIGH defects are fixed in the app that runs today. Both users benefit within days. |
| **S2. Rust server with a Python worker** | the new `sarpur` repo | The new server, queue, database and UI, plus the Windows and Docker packaging. RÚV access and naming still run in the old Python code, as a **worker** program speaking JSON. |
| **S3. Port the worker to Rust** | `sarpur` | Catalogue parsing, naming and IMDB move into Rust, one function at a time. Each is checked against the Python worker on the same input. Then the worker is removed. |

## 2. RALPLAN-DR summary

**Principles**
1. **Never touch an existing library file.** Names Plex already knows stay byte-identical on Linux and NAS output. Any intended difference is listed and checked against `/Media` before release.
2. **Report only what happened.** A job counts as successful only when its file exists at the final path.
3. **Keep a usable app at every stage.** Neither user waits for a big-bang cutover, and the old apps stay as a fallback until real files prove the new one.
4. **Keep RÚV access behind one interface.** That interface is the worker's JSON output, and later the `ruv` module. It is tested with recorded RÚV responses.
5. **One program, one data folder** at the end state, on both operating systems.

**Decision drivers**
1. **Owner's preference:** Rust for the backend, for long-term ownership and enjoyment. This is a stated user preference, not a technical necessity.
2. **Library safety on TrueNAS:** no silent skips, no re-downloading episodes the owner deleted, no duplicate names in Plex.
3. **Easy for a non-technical Windows user** at every stage.

**Options**

| Option | Pros | Cons |
| --- | --- | --- |
| **E. Staged port (chosen)** | Fixes reach users now (S1). The Rust server can be built and judged while the proven Python naming code runs. Naming is ported last and compared live with the original. The old apps stay as a fallback. | Some throwaway Python work in S1. A worker contract (JSON) to maintain during S2. The S2 Windows zip ships a PyInstaller worker of about 40 MB. |
| A. Full Rust rewrite at once | No interim contract; reaches the single-exe end state sooner | Nothing usable until cutover. Naming risk lands at the same time as everything else. Reviewers found 3 places where it would change or re-download library files. |
| D. Keep Python, fix defects, ship a PyInstaller exe | Cheapest. Meets drivers 2 and 3 as well as E. The parser is already proven. | Fails driver 1. A PyInstaller exe gives the same double-click experience, so "one exe" does **not** rule D out; only the Rust preference does. |
| C. Tauri desktop app plus a separate server | Native window, auto-update | Two build pipelines; the NAS is headless; adds a WebView2 dependency |

D is honestly cheaper and meets drivers 2 and 3. E is chosen because of driver 1, and S1 captures most of D's value first. "Fix it by design" is a preference, not a reason to rule D out.

## 3. Architecture (end state, with the interim worker noted)

```
sarpur/
├─ crates/sarpur-lib/   # modules: ruv, naming, imdb, media, library, db, jobs, follows
├─ crates/sarpur/       # bin: axum API + SSE, rust-embed UI, scheduler, import, Plex refresh
├─ worker/              # S2 only: ruvsarpur.py --json (vendored from ruvsarpur@S1 tag) + PyInstaller spec
├─ web/                 # React + Vite + TS
├─ fixtures/            # pinned tvschedule.json (sha256 + count), recorded RÚV responses, golden names, IMDB cache
├─ tools/golden/        # pinned-version Python that produces golden outputs with a frozen clock
└─ docker/Dockerfile    # node build → rust (glibc) → debian-slim + ffmpeg + tini
```

### 3.1 Worker contract (S2)
- The S2 worker is `ruvsarpur.py --json <command>`. It writes JSON lines and nothing else.
- Commands:
  - `catalogue [--incremental]`: the schedule items;
  - `series <sid>`: one series, including series not in the main catalogue (R:1667-1696);
  - `resolve <pid> --quality Q`: the playlist and audio URLs (R:534, R:521);
  - `name <pid> --mode web|plex`: the target path and title;
  - `imdb <sid>`.
- The worker never downloads files and never writes `prevrecorded.log`; Rust runs ffmpeg itself.
- In S3, each command is replaced by a Rust function. A CI job runs both on the pinned fixtures and fails on any difference that isn't listed in `allowed-diffs.jsonl`.

### 3.2 Data (SQLite with WAL mode, rusqlite behind `spawn_blocking`)
- **Tables:**
  - `episodes`: the catalogue cache.
  - `library`: `pid`, `path`, `size`, `mtime`, `source` (one of `tag`, `name`, `import`), `verified_at`.
  - `recorded`: `pid`, `origin` (one of `library`, `web`, `unknown`), `forgotten_at`.
  - `follows`: `id` and `title_key`, plus `follow_sids` linking each follow to its series ids, each with `added_by` (`user` or `auto`) and `english_flag`.
  - `jobs`: `id`, `client_id`, `mode`, `priority` (`interactive` or `scheduled`), `state`, `part_path`, `final_path`, `files`, `ffmpeg_exit` and `error`.
  - `settings`: has **no** watchlist. Follows have their own endpoints.
- **Disk scan:**
  - Reads the `©cmt` tag (`ruvinfo:pid:sid`, written at R:703) with a Rust MP4 parser (`mp4ameta`), so there's no ffprobe and no extra process per file.
  - Cached by `(path, size, mtime)`.
  - Runs in the background after startup and before each follow run, and never blocks startup.

### 3.3 When an episode is downloaded again (replaces default 1 from revision 1)
- **Follow runs skip a pid when any of these holds:**
  - (a) Its tag is on disk.
  - (b) A file matches the expected name as a **prefix glob**, with glob characters such as `[pid]` escaped. This matches `--checklocal` (R:1066-1073, R:1810), which lets users add text to the end of file names.
  - (c) It is in `recorded` with origin `library` or `unknown`, and hasn't been forgotten. This keeps episodes the owner deleted on purpose from coming back (R:1789).
- **Explicit Library downloads** check only (a) and (b), so a browser download never blocks them (UI review #1).
- **Browser downloads** never write to `recorded` with origin `library`.
- **UI:** an "Allow re-download" action on an episode sets `forgotten_at`.

### 3.4 Import
- **`gui-settings.json`:** map paths with a table that is shown before import is confirmed:
  - `/data/downloads` becomes `<data>/downloads`;
  - `/media/...` stays as is;
  - Windows: paths are kept.
  - The `outputDir` to `libraryRootDir` migration from G:541 applies.
- **`prevrecorded.log`:** it holds bare pids (R:889-893), so there are no paths to change. Origin comes from the NDJSON event log (G:415):
  - pids from `web` jobs are marked `web`;
  - pids from library or automatic runs are marked `library`;
  - everything else is `unknown`.
- **Report:** after import, a report lists the pids in `recorded` that are missing on disk (likely deleted on purpose) and the files on disk that aren't in `recorded`.

### 3.5 Follows (replaces default 2 from revision 1)
- A follow stores the series ids the user chose, plus an exact `title_key`. The key is `trimSeasonNumberSuffix` (R:1443) of the trimmed `series_title`, lowercased, with Unicode normalised to NFC. It does **not** use `normalizeSeriesLabel`: that strips "(með táknmálstúlkun)" and "(með sjónlýsingu)", which would auto-add sign-language and audio-described variants (spike 6, `sarpur/spikes/follow/RESULTS.md`, 2026-11-07).
- A new sid is added automatically only when both its `title_key` matches **exactly** and its English-subtitle flag (R:1254) equals the followed sid's flag.
- Auto-added sids show in the Following list with "added automatically" and a remove button.

### 3.6 Queue
- One ffmpeg at a time. Interactive jobs (a user clicked Download or Library) run before scheduled ones.
- On restart:
  - The `*.part` file recorded in `part_path` is deleted.
  - Interrupted interactive jobs go back into the queue.
  - Interrupted scheduled jobs end as `failed`, with the message "interrupted"; the next follow run picks them up.
- Before each job, the server checks disk space and refuses the job if less than 2× the estimated size is free.
- If RÚV is down during a follow run, the run is retried in the next cycle and shown as a warning, not an error.

### 3.7 Browser downloads
- The finished file is kept for 24 hours and can be downloaded again during that time; after that it's swept.
- `client_id` (a random id the browser keeps in `localStorage`) decides which browser's SSE stream receives the event. **It is routing, not security.** With `SARPUR_PASSWORD` set, all `/api` calls also need the session cookie.

### 3.8 Platform

**Windows:**
- Listens on `127.0.0.1` only.
- A console window that says "Close this window to stop sarpur", plus a "Quit" button in the UI.
- Opens the browser on start.
- Stops ffmpeg through a Job Object, with `CREATE_NO_WINDOW` and UTF-8 arguments.
- Data in `%LOCALAPPDATA%\sarpur`. Import also checks the `--portable` location next to the old script.

**Sanitising:**
- Rules that apply **only on Windows** (`cfg(windows)`):
  - replace `\`;
  - add a suffix to reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`);
  - remove trailing dots or spaces from each folder name.
- Linux and NAS output stays identical to the golden files.

**Long paths:**
- Rust's standard library handles long paths; spike 5 tests whether ffmpeg does.
- If ffmpeg can't, it writes the `.part` file to a short temporary path on the same drive, and Rust renames it.
- A path is shortened only on Windows, only when no existing file matches its prefix, and every shortened name is logged.

**Docker:**
- A glibc build; it runs as user `568:568` under `tini`.
- The program sets `umask(0o002)` itself.
- SIGTERM stops ffmpeg; `stop_grace_period: 30s`; `HEALTHCHECK` on `/api/health`; `TZ=Atlantic/Reykjavik`.

**Updates:** a daily check against GitHub Releases shows a banner when a new version is out. There is also a UI banner when a RÚV call fails its schema check.

## 4. Implementation steps

### S1: Fix the Python app (in `ruvsarpur`, tag `v-s1`)
1. Add `--norecord` to `ruvsarpur.py`, used in web mode (G:893). Library jobs fail unless a file exists (UI review #1).
2. Remove `watchlistSids` from `POST /api/settings` on both the client (A:589) and the server (G:559).
3. Browser downloads: send a `client_id` with each job, keep handled job ids in `localStorage`, and keep files 24 hours before sweeping `_web_tmp`.
4. Follow: show a warning while automation is off, and count followed series correctly (A:458).
5. Accessibility fixes from the review: poster links, `group-focus-within`, native `<dialog>`, `motion-safe:`, the 320px layout, contrast.
6. Add `--json` worker commands (§3.1) to `ruvsarpur.py`. They're also used by the golden tools.
7. Deploy to the NAS. (Decided 2026-11-07: no PyInstaller build in the `ruvsarpur` repo; the Windows user gets a packaged app from S2.)

### S2.0: Spikes (all must pass before S2.1)
1. **Golden outputs:**
   - Pin `fixtures/tvschedule.json` (record its sha256 and item count; the current file has 8,508 items).
   - Generate the names in 3 modes, IMDB results and catalogue output from the S1 worker, with a frozen clock (R:1430, R:1802).
   - Store each input item next to its output.
2. **RÚV recordings:** `featured/tv`, `program/{sid}/all`, GQL `getSearch` and `getProgramType` (R:1580, R:1755), and master playlists in all 3 formats.
3. **Fuzzy matching:** compare fuzzywuzzy's `fuzz.ratio` (R:268-284) with `rapidfuzz` for Rust over the IMDB cache pairs.
4. **Disk scan on the NAS:**
   - Time a `mp4ameta` tag scan of `/Media`.
   - Count how many files have tags and how many are only matched by name.
   - Diff `prevrecorded.log` against the disk, to see how many episodes were deleted on purpose.
5. **ffmpeg on Windows:** BtbN ffmpeg with an Icelandic title on a path over 260 characters, `-progress pipe:1`, stopping with `q`, killing through a Job Object, and running the downloaded unsigned exe under Microsoft Defender.
6. **Follow identity:** list sids that share a `title_key`, and check the English-flag rule.
7. **TrueNAS:** user 568 with umask 002 writes `*.part` files and renames them inside `/Media`, and Plex can read the result.
8. **End to end:** one real RÚV episode through the queue, the worker, ffmpeg with progress, and the final rename, on Linux and on Windows.

### S2.1: Rust server with the worker (`sarpur`)
- **Foundations:**
  - The `db`, `jobs`, `library`, `follows` and `media` modules.
  - The worker is a child process, started per command, with a timeout.
- **API:**
  - axum, with routes taken from the S1 API.
  - SSE `/api/events`, filtered by `client_id`.
  - Follow endpoints.
  - The disk-space check (§3.6).
  - A `--dry-run` scheduler that logs what it would download and runs nothing.
- **Import and services:**
  - Import (§3.4), with the report.
  - Poster proxy: host allowlist and a 24-hour cache.
  - Plex refresh with the `X-Plex-Token` header.
  - The NDJSON event log.
- **Packaging:**
  - Windows zip: `sarpur.exe` plus `ruvworker.exe` (PyInstaller) and licences.
  - Docker image: the worker runs with the Python included in the image.
- **UI:** port the S1 UI to `web/` with typed clients, plus a Following list, "Allow re-download", a per-browser setting for the main action (Download or Library), and the update and RÚV-error banners.

### S2.2: Cutover on the NAS
1. Install sarpur next to the old apps, on port 8090.
2. Import the old data and read the import report.
3. Run one follow cycle with `--dry-run` and check it against the report.
4. Turn automation on and let it run at least one real cycle. Check the files with `ls -ln` and in Plex.
5. Stop the old apps but **don't delete them for 2 weeks**. Rollback means stopping sarpur and starting the old apps again. sarpur never writes `prevrecorded.log`, so the old apps keep their state.
6. Update `~/homelab/CLAUDE.md`.

### S3: Port the worker
- Order:
  - `resolve`: quality and audio choice.
  - `catalogue` and `series`: the R:1101 and R:1227 rules listed in the analyst notes in the changelog.
  - `name`: R:946-1066, with the prefix glob escaped.
  - `imdb`.
- Each one ships when the differential CI passes on the fixtures **and** on a live catalogue snapshot.
- When all four are done, delete `worker/` and remove Python from the image and the zip.

## 5. Acceptance criteria

| # | Stage | Criterion (each one testable) |
| --- | --- | --- |
| 1 | S1 | Integration test: browser download of pid X, then Library download of X; the library file exists. A library job that produces no file ends as `failed`. |
| 2 | S1 | `POST /api/settings` with any body leaves the watchlist unchanged. |
| 3 | S1 | Two browsers each queue a browser download; each receives only its own file. Reloading starts no download a second time. |
| 4 | S2 | Golden: the Rust (S3) or worker (S2) names for every item in pinned `fixtures/tvschedule.json`, in 3 modes, match `golden/names.jsonl` byte for byte on Linux. `allowed-diffs.jsonl` is **empty** for Linux. A test keeps `Father Brown X` (`akoha1`) at `Season 01`. |
| 5 | S2 | Catalogue fixture: sid, content type, episode and season numbers, and English flag all match the golden output; 0 differences. |
| 6 | S2 | Library rule (§3.3): a fixture with a deleted-but-recorded pid, a renamed file (prefix match), a tagged file in a moved folder and a browser-only pid. A follow run schedules only the browser-only pid; a library download schedules it as well. |
| 7 | S2 | Following series T (sid A) adds new sid B only if its title key matches exactly and its English flag is equal. Fixtures where the flag differs or the title is merely similar add nothing. |
| 8 | S2 | Queue: an interactive job queued during a 10-job scheduled run starts next. After a restart, no `*.part` file is left, interactive jobs are back in the queue, and scheduled jobs are `failed` ("interrupted"). |
| 9 | S2 | Two `client_id`s: each SSE stream shows only its own browser download. With `SARPUR_PASSWORD` set, a request without the cookie gets 401. Temp folders are gone 24 h after completion (fake clock). |
| 10 | S2 | Disk-space check: a job is refused, with a message, when free space is under 2× the estimated size (mocked). |
| 11 | S2 | Dry run: on the imported NAS data, `--dry-run` schedules nothing that is on disk, and nothing in the import report's "recorded but missing" list. |
| 12 | S2 | Disk scan: `/Media` (about 1,500 files today) scanned cold in under 60 s and warm in under 5 s; startup isn't blocked. |
| 13 | S2 | Import: the NAS `gui-settings.json` and the Windows user's own data (including the `--portable` location) import, and the path mapping is shown before confirming. |
| 14 | S2 | Services: the Plex refresh sends `X-Plex-Token` as a header (mock server). VTT to SRT conversion matches R:381 on fixtures. The poster proxy returns 400 for hosts outside the allowlist. |
| 15 | S2 | Windows 11 VM: unzip and double-click; within 30 s the browser opens; ffmpeg shows download progress and verifies its checksum. A 5-minute episode downloads into a folder with Icelandic characters. The UI's Quit button or closing the console leaves no `ffmpeg.exe` or worker process running. |
| 16 | S2 | TrueNAS: the container runs as user 568; new files are owned by 568:568 with mode 664; `docker stop` takes under 30 s and leaves no ffmpeg running; the healthcheck is green within 20 s. |
| 17 | S2 | UI: Playwright covers posters, the download dialog and Follow by keyboard, and Escape closing the dialogs. No horizontal scroll at 320px. Text tokens have contrast of at least 4.5:1. |
| 18 | S3 | The differential CI (Rust against the worker) passes for `resolve`, `catalogue`, `name` and `imdb` on the fixtures and one live snapshot. After the worker is removed, criteria 4 to 17 still pass. |

## 6. Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| Naming drift creates Plex duplicates | Golden files with a frozen clock and a pinned schedule; `allowed-diffs.jsonl` is empty on Linux; Windows-only sanitising is `cfg(windows)`; differential CI in S3 |
| Deleted episodes come back | The three-part rule in §3.3; origin reconstructed from the NDJSON log; dry run checked against the import report (criterion 11) |
| RÚV changes its API | RÚV access sits behind the worker contract or the `ruv` module; recorded responses; a schema check on every live call shows a UI banner; `sarpur record` (dev tool) re-records the fixtures; the update banner tells the Windows user a fix is out |
| Defender or SmartScreen blocks the exe, ffmpeg or the PyInstaller worker | Spike 5; a fixed SHA-256 for ffmpeg; `docs/windows.md` with steps to unblock; the worker goes away in S3; code signing as a follow-up |
| Windows paths or ffmpeg fail on long paths | Spike 5; a short temporary path for the `.part` file, then a Rust rename; shortening only on Windows, logged |
| TrueNAS permissions or renames across datasets | Spike 7; the `.part` file next to its final path; umask set by the program; criterion 16 |
| S2 drags on | S1 already gave both users the fixes; the old apps stay until S2.2 step 4 passes; S3 is optional for function (it removes Python, not features) |

## 7. Verification steps
1. **S1:** `python -m unittest discover -s tests` with new tests for criteria 1 to 3; manual NAS deploy.
2. **S2:**
   - `cargo test --workspace` (golden, fixtures, queue, library rule, import, follows).
   - `pnpm --dir web test` and `build`.
   - Playwright against the finished binary on Linux and on Windows CI runners.
   - The manual Windows VM checklist (criterion 15) recorded in `docs/windows.md`.
   - The S2.2 cutover log.
3. **S3:** the differential CI job's history; criteria 4 to 17 rerun after the worker is removed.

## 8. ADR
- **Decision:** build `sarpur` as a staged port.
  - S1 fixes the Python app.
  - S2 is a Rust server (axum, rusqlite, React UI built in) that uses the old Python code as a JSON worker for RÚV access and naming.
  - S3 ports that code to Rust, checked against the worker, and then removes Python.
- **Drivers:**
  - the owner's preference for Rust;
  - library safety (no silent skips, no re-downloads, no duplicates);
  - an easy Windows setup at every stage.
- **Alternatives considered:**
  - **A, a full rewrite at once:** puts the naming risk and the cutover at the same moment. Reviewers found three library-damaging changes in it.
  - **D, Python plus PyInstaller:** cheaper and meets drivers 2 and 3. It's rejected only on driver 1, and S1 delivers most of its value first.
  - **C, Tauri:** two build pipelines with no benefit to the headless NAS.
  - **Rust without ffmpeg:** RÚV's separate audio stream needs a muxer.
- **Why chosen:** it's the only option that satisfies the Rust preference without putting the existing library at risk. Each stage ships something usable, and the riskiest code (naming) is ported last, checked against the original on real input.
- **Consequences:**
  - S1 involves Python work that S3 later removes.
  - The worker contract has to be maintained through S2.
  - The S2 Windows zip is about 40 MB larger.
  - Follows become per series (title key and English flag).
  - Library state is decided by tags, prefix name matches, and the download history (`recorded`) with its origin.
- **Follow-ups:**
  - a tray icon;
  - "Start with Windows";
  - code signing;
  - self-update;
  - an arm64 image;
  - a weekly live RÚV check in CI;
  - an optional `sarpur download --pid` command;
  - a muxer without ffmpeg.

## Changelog (revision 1 → 2)

**Strategy:** a full rewrite became a staged port (user's decision).

**ADR:**
- Rust is stated as a driver.
- Option D is weighed honestly.
- "Fix it by design" is labelled a preference.

**From the Architect** (Critic agreed with these):
- Follow by sids plus an exact title key plus the English flag.
- The browser-download lifetime is no longer contradictory; `client_id` is routing, not security.
- Interactive jobs go ahead of scheduled ones.
- A frozen clock and the input items in the golden files.
- Citation fixes:
  - R:1755;
  - the tags are at R:703;
  - `fuzz.ratio` is at R:268-284.
- Docker: glibc build and the program sets the umask.
- Explicit path mapping on import.
- Two crates and rusqlite.
- An end-to-end spike.
- A rollback plan, the disk-space check and SQLite WAL mode.

**Changed at the Critic's request:**
- Library truth is no longer "disk only". It's the three-part rule (§3.3), so deleted episodes don't come back. Tags are read with `mp4ameta`, not ffprobe.
- `*.part` cleanup is tracked per job, and interactive jobs are requeued after a restart.
- Long paths are shortened only on Windows, after a test of whether ffmpeg handles them, and never on Linux.
- Removing the tray required replacing it with the console window and a Quit button. The update check is kept.

**New from the Critic:**
- The season-10 Roman numeral "fix" is removed; it would rename `Father Brown X`.
- The prefix-glob duplicate check is kept, with glob characters escaped.
- Windows sanitising applies only on Windows.
- The golden input is pinned (8,508 items, sha256).
- The origin of each recorded pid is reconstructed on import.
- A dry-run mode, with its own criterion.
- The cutover order includes a rollback.
- The Windows criterion is split so the ffmpeg download isn't timed.
- Import of the Windows user's data.
- New criteria for Plex, VTT, the poster proxy, the disk scan and disk space.
- The risk mitigations are made concrete.

**Dropped:**
- the 81-second performance criterion;
- axe in CI (a manual pass instead);
- the poster cache cap of 3,000 files (time-to-live only);
- the six-crate layout;
- sqlx.
