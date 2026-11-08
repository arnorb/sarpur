# sarpur

A downloader for RÚV's Sarpur, being rebuilt as one Rust program with a built-in web UI. It will replace [ruvsarpur](https://github.com/arnorb/ruvsarpur) (a Python CLI plus a separate web GUI) in stages:

| Stage | What | Status |
| --- | --- | --- |
| S1 | Fix the Python app in `ruvsarpur` | Done (tag `v-s1`), running on the NAS |
| S2.0 | Spikes: check the risky parts before building | Mostly done; Windows PC, NAS container and end-to-end halves open |
| S2 | Rust server, queue, database and UI; RÚV access and naming still in the Python worker | Slices 1–4 done (Docker image: `docs/docker.md`); cutover on the NAS next |
| S3 | Port the worker to Rust, checked against the Python output | Not started |

The plan, with a status table and the decisions made since it was approved, is in [`docs/plan.md`](docs/plan.md).

## Layout

- `crates/sarpur-lib/`: the core (database, queue, worker client, ffmpeg runner, library rules, catalogue).
- `crates/sarpur/`: the server binary. `docs/api.md` lists its routes.
- `web/`: the UI (React, Vite, Tailwind), embedded in the binary from `web/dist`.
- `worker/`: the Python worker, vendored from `ruvsarpur` at tag `v-s1`. Run it as `python worker/ruvsarpur.py --json <command>`. It writes JSON lines only. Commands: `catalogue`, `series` (`--save` merges into the saved schedule), `resolve`, `plan`, `name`, `names` (the `plan` names of many pids), `imdb`, plus `--now` to freeze the clock.
- `fixtures/tvschedule.json`: the pinned catalogue (8,508 items, schedule date 2026-03-21, sha256 `78eb107c…df1b6e`).
- `golden/`: names in three modes and IMDB data, generated from the worker by `tools/golden/generate.py` with a frozen clock. Each line carries the input fields that produce it. `manifest.json` records the hashes.
- `spikes/`: the S2.0 spikes and their results (see `spikes/README.md`).

## Running the server

```bash
pip install -r worker/requirements.txt          # the worker's Python dependencies
(cd web && npm ci && npm run build)              # the UI, embedded from web/dist
cargo run -p sarpur -- --data-dir ./data         # http://127.0.0.1:8090
```

For UI work, `cd web && npm run dev` serves the UI on :5173 and passes `/api` to the running server (`SARPUR_URL` to point elsewhere). `npm run test:e2e` runs the Playwright tests against a dry-run server with a fake worker. `SARPUR_PASSWORD` turns on the login.

`SARPUR_PYTHON` picks the Python that has those dependencies (default `python3`); ffmpeg must be on PATH or in `SARPUR_FFMPEG`. Tests: `cargo test --workspace` and `python -m unittest discover -s worker -p "test_*.py"`.

## Regenerating the golden files

```bash
pip install -r worker/requirements.txt
python tools/golden/generate.py
```

The output must match `golden/manifest.json` byte for byte. If it doesn't, the naming code or the fixture has changed.
