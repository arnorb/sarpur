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
python -m unittest discover -s worker -p "test_*.py"  # worker tests
python -m unittest discover -s tools/record           # record/replay tests
python tools/golden/generate.py                       # must leave golden/ unchanged
```

The worker needs ruvsarpur's Python dependencies (`pip install -r worker/requirements.txt`). On this Mac a ready venv is `/Users/arnorbogason/Projects/ruvsarpur/venv/bin/python`; point `SARPUR_PYTHON` at it when running the server. ffmpeg is on PATH (Homebrew).

## Layout

- `crates/sarpur-lib/`: the core (`config`, `settings`, `db`, `worker`, `catalogue`, `library`, `media`, `jobs`, `follows`, `scan`).
- `crates/sarpur/`: the axum server and CLI. Routes are listed in `docs/api.md`.
- `worker/`: ruvsarpur's Python, vendored at tag `v-s1` and ours to edit. It's run as `ruvsarpur.py --json <command>` and writes JSON lines only. In S3 each command is replaced by Rust and checked against this output.
- `fixtures/`: the pinned catalogue (`tvschedule.json`) and recorded RÚV responses (`fixtures/ruv/`, replayed by `tools/record/`).
- `golden/`: expected file names and IMDB data from `tools/golden/generate.py`. A change to `golden/` means naming changed. Plex already knows the existing names, so that's a bug unless it's deliberate and agreed.
- `spikes/`: the S2.0 spikes. Raw outputs in `spikes/*/out/` are git-ignored.

## Rules that protect the library

- Never change a file name that ruvsarpur already produces on Linux; `golden/` is the check.
- A job counts as successful only when its file exists at the final path. ffmpeg writes `<name>.mp4.part`, which is renamed only on success.
- Browser downloads never mark an episode as recorded for the library.
- Follow keys use the strict title key (`spikes/follow/RESULTS.md`), not `normalizeSeriesLabel`.

## Environment

- The owner's TrueNAS SCALE NAS runs the old app (two Custom Apps, `ruvsarpur-api` and `ruvsarpur-web`). Its notes are in `~/homelab/CLAUDE.md`. `ssh truenas` logs in as `admin` with key auth. `/home`, `/tmp` and `/run` are `noexec`, and root needs the TrueNAS API. Ask before changing anything on the NAS.
- A second user runs the app on Windows and runs the Windows checks on their own PC (`spikes/windows/README.md`).

## Commits

Commit on a branch per slice and merge to `main` with a fast-forward when it's verified. End commit messages with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
