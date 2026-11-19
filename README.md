# sarpur

A downloader for [RÚV](https://www.ruv.is)'s Sarpur, with a web UI. It is one Rust program: you browse RÚV's catalogue in the browser, and sarpur downloads episodes into a library that Plex can read.

- **Browse and search** RÚV's catalogue by title, with posters, descriptions and expiry dates.
- **Download to the library**, with the folder and file names Plex expects, MP4 tags, artwork and subtitles (RÚV's VTT, also converted to SRT).
- **Download to the browser** instead, when you just want the file.
- **Follow a show**: new episodes go to the library on a schedule, and new seasons are picked up. Earlier episodes come too if you ask for them.
- **Refresh Plex** when downloads finish.
- **Import** settings and download history from ruvsarpur.
- Optional password, and a banner when a new version is out.

sarpur runs on Linux in Docker and from source on macOS and Linux. There is no Windows package yet; the work that remains is in [`docs/windows-task.md`](docs/windows-task.md).

## Run it with Docker

The image holds the server, the UI and ffmpeg. It is built for `linux/amd64`.

```yaml
services:
  sarpur:
    image: ghcr.io/arnorb/sarpur:latest
    user: "568:568"
    environment:
      TZ: Atlantic/Reykjavik
      # SARPUR_PASSWORD: change-me
    volumes:
      - ./data:/data              # database, catalogue cache, browser downloads
      - /path/to/your/media:/media  # the folder Plex reads
    ports:
      - "8090:8090"
    stop_grace_period: 30s
    restart: unless-stopped
```

1. Save that as `docker-compose.yml` (the one in this repository is the same, with the owner's NAS paths) and run `docker compose up -d`.
2. Open <http://localhost:8090>.
3. In Settings, set the library folder to `/media`.

New files are written as user `568`, so that user needs write access to the media folder; change `user:` if yours differs. Volumes, environment variables, stopping and releases are covered in [`docs/docker.md`](docs/docker.md).

## Run it from source

You need Rust (rustup), Node.js and ffmpeg. ffmpeg must be on `PATH` or named in `SARPUR_FFMPEG`.

```bash
(cd web && npm ci && npm run build)              # the UI; the binary embeds web/dist
cargo run -p sarpur -- --data-dir ./data         # http://127.0.0.1:8090
```

| Option | Environment | Default | |
| --- | --- | --- | --- |
| `--bind` | `SARPUR_BIND` | `127.0.0.1` | Address to listen on. |
| `--port` | `SARPUR_PORT` | `8090` | |
| `--data-dir` | `SARPUR_DATA_DIR` | the platform's local data folder | Database, catalogue cache and browser downloads. |
| `--dry-run` | | off | Jobs finish without ffmpeg; follow runs only report. |
| | `SARPUR_PASSWORD` | unset | Turns on the login. |

## Where it comes from

sarpur is a staged Rust rewrite of [ruvsarpur](https://github.com/sverrirs/ruvsarpur) by Sverrir Sigmundarson, a Python command-line tool for downloading from RÚV. It grew out of a fork, [arnorb/ruvsarpur](https://github.com/arnorb/ruvsarpur), which added a web GUI.

The RÚV code is a port of ruvsarpur's, and the file names are ruvsarpur's byte for byte, so an existing Plex library keeps working. ruvsarpur's Python stays in `tools/reference/` as the reference the port is checked against; it isn't used at run time.

| Stage | What | Status |
| --- | --- | --- |
| S1 | Fix the Python app in `ruvsarpur` | Done (tag `v-s1`) |
| S2 | Rust server, queue, database and UI, with RÚV access still in a Python worker | Done; running on the owner's NAS since 2026-11-08 |
| S3 | Port the worker to Rust, checked against the Python output | Done: no Python at run time |
| Later | Windows package | Not started ([`docs/windows-task.md`](docs/windows-task.md)) |

The plan, its status table and the decisions made along the way are in [`docs/plan.md`](docs/plan.md).

## Development

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
(cd web && npm run test:e2e)                      # Playwright, against a dry-run server
python tools/diff/diff.py                         # the Rust port against the Python
python tools/golden/generate.py                   # must leave golden/ unchanged
```

For UI work, `cd web && npm run dev` serves the UI on port 5173 and passes `/api` to the running server (`SARPUR_URL` points it elsewhere). The Playwright tests replay RÚV from recordings (`SARPUR_RUV_REPLAY`), so they don't touch the network.

The Python tools need ruvsarpur's dependencies: `pip install -r tools/reference/requirements.txt`. `python tools/diff/diff.py --live` runs the comparison against RÚV itself.

### Layout

- `crates/sarpur-lib/`: the core (database, queue, the RÚV client in `ruv/`, ffmpeg runner, library rules, catalogue, follows).
- `crates/sarpur/`: the server binary. Its routes are in [`docs/api.md`](docs/api.md).
- `web/`: the UI (React, Vite, Tailwind), embedded in the binary from `web/dist`.
- `tools/reference/`: ruvsarpur's Python, from `ruvsarpur` tag `v-s1`.
- `fixtures/`: the pinned catalogue (`tvschedule.json`: 8,508 items, schedule date 2026-03-21) and recorded RÚV responses.
- `golden/`: the expected file names in three modes and the IMDB data, generated from the Python with a frozen clock.
- `spikes/`: the checks made before the rewrite, with their results.

### The golden files

`golden/` is what protects existing libraries. `python tools/golden/generate.py` must reproduce `golden/manifest.json` byte for byte. If it doesn't, the naming code or the fixture has changed, and Plex would see renamed files as new ones.

## Licence

MIT, as ruvsarpur. [`LICENSE`](LICENSE) carries Sverrir Sigmundarson's copyright notice for ruvsarpur alongside sarpur's own.
