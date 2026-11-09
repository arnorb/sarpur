# sarpur

A downloader for RÚV's Sarpur, being rebuilt as one Rust program with a built-in web UI.

sarpur originates in [ruvsarpur](https://github.com/sverrirs/ruvsarpur) by Sverrir Sigmundarson, a Python command-line tool for downloading from RÚV. It grew out of a fork of that project, [arnorb/ruvsarpur](https://github.com/arnorb/ruvsarpur), which added a web GUI. Its RÚV code is a Rust port of ruvsarpur's, and the file names sarpur gives downloads are ruvsarpur's, kept byte for byte so existing Plex libraries keep working. ruvsarpur's Python stays in `tools/reference/` as the reference the port is checked against.

sarpur replaces the fork in stages:

| Stage | What | Status |
| --- | --- | --- |
| S1 | Fix the Python app in `ruvsarpur` | Done (tag `v-s1`), running on the NAS |
| S2.0 | Spikes: check the risky parts before building | Mostly done; Windows PC, NAS container and end-to-end halves open |
| S2 | Rust server, queue, database and UI; RÚV access and naming still in the Python worker | Done (Docker image: `docs/docker.md`); running on the NAS since 2026-11-08 |
| S3 | Port the worker to Rust, checked against the Python output | Done: no Python at run time |

The plan, with a status table and the decisions made since it was approved, is in [`docs/plan.md`](docs/plan.md).

## Layout

- `crates/sarpur-lib/`: the core (database, queue, the RÚV client in `ruv/`, ffmpeg runner, library rules, catalogue).
- `crates/sarpur/`: the server binary. `docs/api.md` lists its routes.
- `web/`: the UI (React, Vite, Tailwind), embedded in the binary from `web/dist`.
- `tools/reference/`: ruvsarpur's Python (from `ruvsarpur` tag `v-s1`, with sarpur's JSON worker). Not used at run time; `tools/golden/` and `tools/diff/` run it to check the Rust port.
- `fixtures/tvschedule.json`: the pinned catalogue (8,508 items, schedule date 2026-03-21, sha256 `78eb107c…df1b6e`).
- `golden/`: names in three modes and IMDB data, generated from the Python by `tools/golden/generate.py` with a frozen clock. Each line carries the input fields that produce it. `manifest.json` records the hashes.
- `spikes/`: the S2.0 spikes and their results (see `spikes/README.md`).

## Running the server

```bash
(cd web && npm ci && npm run build)              # the UI, embedded from web/dist
cargo run -p sarpur -- --data-dir ./data         # http://127.0.0.1:8090
```

For UI work, `cd web && npm run dev` serves the UI on :5173 and passes `/api` to the running server (`SARPUR_URL` to point elsewhere). `npm run test:e2e` runs the Playwright tests against a dry-run server with RÚV replayed from recordings (`SARPUR_RUV_REPLAY`). `SARPUR_PASSWORD` turns on the login.

ffmpeg must be on PATH or in `SARPUR_FFMPEG`. Tests: `cargo test --workspace`; `python tools/diff/diff.py` compares the Rust port with the Python (`--live` against RÚV).

## Regenerating the golden files

```bash
pip install -r tools/reference/requirements.txt
python tools/golden/generate.py
```

The output must match `golden/manifest.json` byte for byte. If it doesn't, the naming code or the fixture has changed.

## Licence

MIT, as ruvsarpur. See [`LICENSE`](LICENSE): it carries Sverrir Sigmundarson's copyright notice for ruvsarpur alongside sarpur's own.
