# AGENTS.md

Guidance for coding agents working in this repository.

## What this is

`sarpur` is a staged Rust rewrite of [ruvsarpur](https://github.com/arnorb/ruvsarpur), a downloader for RÚV's Sarpur. The plan is in `docs/plan.md`. Read its **Status** section first: it says what's done, what's next and which decisions changed the plan. Update that section at the end of every slice.

It's a hobby project with one developer. Keep the process light: no extra review rounds or strict gates, and decide open details when they come up. The plan is a guide, not a contract.

## Commands

```bash
export PATH="$HOME/.cargo/bin:$PATH"                 # rustup's toolchain
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo run -p sarpur -- --data-dir ./data              # http://127.0.0.1:8090
python -m unittest discover -s tools/reference -p "test_*.py"  # reference tests
python tools/diff/diff.py                             # Rust port vs the Python (criterion 18)
python -m unittest discover -s tools/record           # record/replay tests
python tools/golden/generate.py                       # must leave golden/ unchanged
(cd web && npm ci && npm run build)                   # the UI; the binary embeds web/dist
(cd web && npm run test:e2e)                          # Playwright (criterion 17)
```

The server needs no Python. The Python tools need ruvsarpur's dependencies (`pip install -r tools/reference/requirements.txt`); on this Mac a ready venv is `/Users/arnorbogason/Projects/ruvsarpur/venv/bin/python`. ffmpeg is on PATH (Homebrew).

## Layout

- `crates/sarpur-lib/`: the core (`config`, `settings`, `db`, `ruv`, `catalogue`, `library`, `media`, `jobs`, `follows`, `scan`, `subtitles`, `extras`, `plex`, `posters`, `import`, `eventlog`).
- `crates/sarpur/`: the axum server and CLI. Routes are listed in `docs/api.md`.
- `web/`: the UI. `src/api` is the typed client, `src/store.tsx` the shared state over SSE, `e2e/` the Playwright tests.
- `crates/sarpur-lib/src/ruv/`: RÚV, ported from ruvsarpur in S3: `naming`, `metadata`, `playlist`, `schedule` (catalogue and series), `search` and `fuzz` (fuzzywuzzy over RapidFuzz, exactly), `imdb`, `http` (live or replayed). `Ruv` is the entry point; `testing` is a fake RÚV for tests. Keep each function's behaviour equal to the Python: `tools/diff/diff.py` checks it.
- `tools/reference/`: ruvsarpur's Python (tag `v-s1` plus sarpur's JSON worker). Not shipped; the reference for `golden/` and `tools/diff/`.
- `fixtures/`: the pinned catalogue (`tvschedule.json`), recorded RÚV responses (`fixtures/ruv/`, replayed by `tools/record/`) and VTT files with the SRT ruvsarpur makes from them (`fixtures/subtitles/`; regenerate with `python tools/reference/test_subtitles.py --regenerate`).
- `golden/`: expected file names and IMDB data from `tools/golden/generate.py`. A change to `golden/` means naming changed. Plex already knows the existing names, so that's a bug unless it's deliberate and agreed.
- `spikes/`: the S2.0 spikes. Raw outputs in `spikes/*/out/` are git-ignored.

## Rules that protect the library

- Never change a file name that ruvsarpur already produces on Linux; `golden/` is the check.
- A job counts as successful only when its file exists at the final path. ffmpeg writes `<name>.mp4.part`, which is renamed only on success.
- Browser downloads never mark an episode as recorded for the library.
- Follow keys use the strict title key (`spikes/follow/RESULTS.md`), not `normalizeSeriesLabel`.

## Environment

- The owner's TrueNAS SCALE NAS runs sarpur as the Custom App `sarpur` (port 8090, `ghcr.io/arnorb/sarpur:latest`, data in `/mnt/storage/apps/sarpur`). The old ruvsarpur apps are stopped, not deleted. Its notes are in `~/homelab/CLAUDE.md`. `ssh truenas` logs in as `admin` with key auth. `/home`, `/tmp` and `/run` are `noexec`, and root needs the TrueNAS API. Ask before changing anything on the NAS.
- A second user runs the app on Windows and runs the Windows checks on their own PC (`spikes/windows/README.md`). Windows support is a later add-on: don't let it block a slice (`docs/plan.md`, decisions).

## Commits

Commit on a branch per slice and merge to `main` with a fast-forward when it's verified. End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
