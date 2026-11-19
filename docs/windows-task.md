# Task: sarpur for the Windows user

**Status:** not started. Deferred on 2026-11-07 so it wouldn't block the NAS (`docs/plan.md`, decisions).

## Goal

The second user runs sarpur on their own Windows 10 or 11 PC. They are not technical: they unzip a folder, double-click `sarpur.exe`, and the browser opens on the app. Their settings and download history from the old ruvsarpur come along.

## Where things stand

Already there:

- The code is portable. The server listens on `127.0.0.1` by default and keeps its data in `%LOCALAPPDATA%\sarpur` (`config.rs`).
- Names use `\` as the separator on Windows (`ruv/naming.rs`), as ruvsarpur does.
- The import reads the `--portable` folder next to the old script (`import.rs`), tested on a folder built from the old formats (`crates/sarpur-lib/testdata/import/`).
- Ctrl+C stops the server on Windows (`main.rs`).
- Since S3 there is no Python worker, so the zip needs no PyInstaller part. The plan's `ruvworker.exe` is gone.
- Spike 5 passes on a GitHub Windows runner (workflow `Windows spike 5`): the pinned ffmpeg verifies and runs, writes a 326-character path with Icelandic letters, reports progress, stops on `q`, and dies with its Job Object.

Missing:

- No Windows build. CI and the release workflow only build for Linux.
- sarpur has never been compiled or tested on Windows. The tests use a Python stand-in for ffmpeg (`crates/sarpur-lib/testdata/fake_ffmpeg.py`).
- ffmpeg must be on `PATH` or in `SARPUR_FFMPEG`. Nothing downloads it.
- Nothing opens the browser, and there is no Quit button.
- ffmpeg isn't tied to a Job Object, so it could outlive sarpur.
- The Windows-only name rules (reserved names, trailing dots and spaces) aren't written.
- Spike 5 hasn't run on the user's own PC, so Defender and SmartScreen are untested.

## Work, in order

1. **Build on Windows in CI.** Add a `windows-latest` job to `ci.yml` that builds the UI and runs `cargo test --workspace`.
   - `reqwest` uses `aws-lc-rs`, which needs a C compiler; if that fails, fall back to `rustls-no-provider` with `ring`.
   - Done when the job is green.
2. **Windows-only name rules** (`cfg(windows)`, plan §3.8): add a suffix to reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`) and drop trailing dots and spaces from each folder name.
   - Done when unit tests cover them and `golden/` is unchanged on Linux.
3. **ffmpeg on first run.** When ffmpeg is missing, download the pinned BtbN build into the data folder, check its SHA-256 and show progress in the UI.
   - The pin is in `spikes/windows/spike5.ps1`. BtbN deletes dated builds after a while, so mirror the zip in a GitHub release of this repository, or update the pin.
   - Done when a fresh PC gets a working ffmpeg with no manual step and a wrong hash is refused.
4. **Start and stop like a desktop app.**
   - A console window that says "Close this window to stop sarpur".
   - The browser opens on start.
   - A Quit button in the UI.
   - ffmpeg runs in a Job Object with `CREATE_NO_WINDOW`.
   - Done when Quit, or closing the console, leaves no `ffmpeg.exe` in Task Manager, even mid-download.
5. **Long paths.** Spike 5 says ffmpeg writes paths over 260 characters, so nothing should be needed.
   - Check it with a real download on the user's PC. Only if it fails, write the `.part` file to a short path on the same drive and rename it.
6. **The zip.** Add a Windows job to `release.yml` that attaches `sarpur-<version>-windows-x64.zip` (`sarpur.exe` and the licences) to the GitHub release.
   - Done when a `v*` tag publishes the zip next to the Docker image.
7. **`docs/windows.md` for the user:** how to unzip and start, what to click if SmartScreen or Defender objects (from the spike 5 results), where the data lives, how to import from ruvsarpur and how to update.
8. **Checks on the user's PC** (next section), then fix what they find.

Steps 2 to 5 don't depend on each other. Step 6 needs step 1.

## What the Windows user does

Each of these is a few minutes and needs no technical knowledge. Send the instructions with each build.

1. **Now: spike 5.** Run `spike5.ps1` as described in [`spikes/windows/README.md`](../spikes/windows/README.md) and send back `sarpur-spike5-results.json`, with a screenshot of any Windows warning.
   - The script's RÚV episode expires; check it still downloads (run the `Windows spike 5` workflow) before asking.
2. **With the first zip:** unzip it, double-click `sarpur.exe`, and report what Windows asks and whether the browser opens.
3. **Import:** point the import at their old ruvsarpur folder and check that the preview shows their settings and paths before confirming.
4. **One download:** a short episode into their real library folder, then Quit.

## Done when

- **Criterion 15:** on Windows 11, unzip and double-click; the browser opens within 30 seconds; ffmpeg downloads with progress and passes its checksum; a 5-minute episode lands in a folder with Icelandic letters in its name; Quit or closing the console leaves no `ffmpeg.exe` running.
- **Criterion 13, Windows half:** the user's own ruvsarpur data imports, including the `--portable` location, and the path mapping is shown before confirming.
- The user has run it on their own PC and it works without help.
- `docs/plan.md` has the result in its Status section.

## Decided already

- Windows listens on `127.0.0.1` only, with no password by default.
- Windows paths are kept as they are on import.
- Names that ruvsarpur already produces don't change. A path is shortened only on Windows, only when no existing file matches its prefix, and every shortened name is logged.
- Code signing is a follow-up, not part of this task. The user gets steps to unblock the exe instead.

## Open

- **Mirror ffmpeg or track BtbN?** Mirroring costs about 195 MB in a release, but the pin never breaks.
- **Console window or tray icon?** The plan says a console window. A tray icon is nicer and more work.
- **Updates.** The banner already says when a version is out. Whether sarpur should replace its own exe, or the user downloads the new zip, is undecided; the zip is enough to start.
