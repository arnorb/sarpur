# Running sarpur in Docker

The image holds the server (with the UI) and ffmpeg. It runs as `568:568` under `tini` and listens on port 8090.

```bash
docker build -t sarpur .                       # or pull ghcr.io/arnorb/sarpur
docker compose up -d                           # docker-compose.yml: the NAS layout
```

## Volumes

- `/data`: the database, the catalogue cache (`tvschedule.json`), the event log, the poster cache and browser downloads. Keep it.
- The library: mount the folder Plex reads (on the NAS, `/mnt/storage/Media` at `/media`) and set the library folder in Settings to `/media`. New files are written as 568 with `umask 002`; on the NAS the dataset ACL decides the final mode.

## Environment

| Variable | Default in the image | |
| --- | --- | --- |
| `SARPUR_PASSWORD` | unset | Turns on the login. |
| `TZ` | `Atlantic/Reykjavik` | |
| `SARPUR_BIND`, `SARPUR_PORT` | `0.0.0.0`, `8090` | |
| `SARPUR_UPDATE_REPO` | `arnorb/sarpur` | Repository for the update banner; empty turns the check off. |
| `SARPUR_GITHUB_TOKEN` | unset | Needed for the update check while the repository is private. |
| `RUST_LOG` | `info` | |

`SARPUR_DATA_DIR` is set by the image; leave it.

## Stopping

`docker stop` sends SIGTERM: sarpur stops ffmpeg, leaves the job for recovery and exits (under a second when tested). On the next start the `.part` file is deleted and an interrupted interactive job is queued again; a scheduled one is marked failed and the next follow run picks it up. Give it `stop_grace_period: 30s`.

The healthcheck (`sarpur --healthcheck`) calls `/api/health` (open even with a password) and is green about 2 seconds after start.

## Releases

`.github/workflows/release.yml` builds the image for `linux/amd64` and pushes it to GHCR: `:main` on every push to `main`, and `:<version>` plus `:latest` for a `v*` tag, which also publishes a GitHub release for the update banner. To release, bump `version` in `Cargo.toml` (and `web/package.json`), commit, then tag `v<version>` and push the tag. The workflow fails when the tag and `Cargo.toml` disagree.
