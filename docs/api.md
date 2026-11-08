# sarpur HTTP API (S2.1 slices 1–3)

JSON over HTTP under `/api`. Field names are camelCase, as in ruvsarpur's `gui_api.py`. Errors come back as `{"error": "..."}` with a 4xx or 5xx status. Responses are gzipped when the client accepts it (except SSE, images and video).

Everything outside `/api` is the UI, embedded in the binary from `web/dist`; paths that aren't files get `index.html`.

**Password.** With `SARPUR_PASSWORD` set, every `/api` route except `health`, `session`, `login` and `logout` needs the `sarpur_session` cookie, else 401. The cookie is signed with the password and a secret in `<data>/session-secret`, so it survives restarts and changing the password ends every session.

| Method and path | What it does |
| --- | --- |
| `GET /api/health` | `{ok, version, dryRun, catalogue: <status>, update: {available, url, checkedAt, error}}`. `update.available` is the newer release's version from the daily GitHub Releases check, else `null`. |
| `GET /api/session` | `{required, authenticated}`. |
| `POST /api/login` | Body `{password}`. Sets the session cookie (90 days); 401 for a wrong password. |
| `POST /api/logout` | Clears the cookie. |
| `GET /api/settings` | The settings (below). |
| `POST /api/settings` | Applies the known keys of the JSON body and returns the settings. Unknown keys, including `watchlistSids`, are ignored. A known key with the wrong type is a 400. |
| `GET /api/shows?q=` | `{shows, status}`. Every episode in the catalogue (the UI groups, filters and sorts them), or with `q` those matching ruvsarpur's fuzzy search (`fuzz.partial_ratio > 85` on the titles, run by the worker). Each show is a catalogue item (below) plus `isFollowed` and `downloadState`: `library` (a file is known in the library), `recorded` (downloaded before but no file known; "Allow re-download" clears it) or `null`. |
| `GET /api/catalogue?q=&limit=` | `{items, status}`. Case-insensitive substring search over `title` and `seriesTitle`, newest first. `limit` defaults to 100 (max 1000). Each item: `pid, sid, title, displayTitle, seriesTitle, showtime, duration, durationLabel, contentType, englishSubtitled, posterUrl, subtitleLanguages, episodeDescription, seriesDescription, firstAppearedAt, expiresAt, webUrl, categories`. `displayTitle` is ruvsarpur's `createShowTitle`. `posterUrl` is `/api/poster?url=…` for the first of the portrait, series and episode image (`null` if none or not allowed). |
| `GET /api/catalogue/status` | `{state, count, loadedAt, error}`; `state` is `empty`, `loading`, `refreshing`, `ready` or `failed`. |
| `POST /api/catalogue/refresh` | 202. Fetches the catalogue from RÚV in the background (takes minutes), then reloads it. |
| `POST /api/downloads` | Body `{pid, mode: "web" \| "library", clientId?, subtitleLanguages?}` (`client_id` also accepted). `subtitleLanguages` limits the subtitle tracks (absent: all; `["none"]`: none). 201 with the new job. |
| `GET /api/jobs` | `{jobs}`, newest first. |
| `DELETE /api/jobs/{id}` | Queued: cancelled. Running: ffmpeg is stopped, the `.part` deleted, then the job becomes `cancelled`. Finished: removed from the list (a browser download's file goes too). Returns `{removed, job}`. 404 for an unknown id. |
| `POST /api/jobs/{id}/retry` | Puts a finished job back at the end of the queue. 400 if it is still queued or running. |
| `GET /api/jobs/{id}/files/{name}` | The file of a completed browser download, for 24 hours. `Content-Disposition` carries an ASCII `filename` and an RFC 5987 `filename*`. |
| `GET /api/follows` | `{follows, status}`. Each follow: `id, titleKey, label, englishSubtitled, createdAt, sids: [{sid, seriesTitle, addedBy: "user" \| "auto", addedAt}]`. |
| `POST /api/follows` | Body `{sid}`. Follows the series for its strict title key and English flag (looked up in the catalogue, else on RÚV); 201 with the follow. 404 for an unknown sid. |
| `DELETE /api/follows/{id}` | Stops following the show. `{removed: true}`, or 404. |
| `DELETE /api/follows/{id}/sids/{sid}` | Stops following one sid. It is remembered, so a run won't add it back. `{follow}` (`null` when that was the last sid and the follow is gone), or 404. |
| `GET /api/follows/status` | The run status (below). |
| `POST /api/follows/run` | Optional body `{dryRun?: false, refresh?: true}`. 202 and the run starts in the background; 409 while one is running. With the server's `--dry-run`, every run is a dry run. |
| `GET /api/library/scan` | The library scan status (below). |
| `POST /api/library/scan` | 202 and a scan starts in the background. Scans also run at startup and before each follow run. |
| `POST /api/recorded/{pid}/forget` | "Allow re-download": follow runs stop skipping the pid for being recorded (a file still on disk is still skipped). Returns `{pid, origin, recordedAt, forgottenAt}`; 404 if the pid isn't recorded. |
| `GET /api/poster?url=` | The poster proxy. Only `https` URLs on `myndir.ruv.is`, `d38kdhuogyllre.cloudfront.net` and `m.media-amazon.com`; anything else is a 400. Cached in `<data>/poster-cache` for 24 hours (at most 3,000 files); a stale copy is served when the fetch fails, else 502. The content type is sniffed from the bytes. |
| `GET /api/plex/status` | `{last}`: the last Plex refresh, `{state: "ok" \| "skipped" \| "failed", message}` or `null`. |
| `POST /api/plex/refresh` | Refreshes Plex now and returns the result. |
| `GET /api/import/candidates` | `{candidates}`: folders holding ruvsarpur's `gui-settings.json` or `prevrecorded.log` (the data folder, `~/.ruvsarpur`, the program's folder and its parent). |
| `POST /api/import/preview` | Body `{source}`. What importing that folder would do, changing nothing (below). 400 if it isn't a folder with ruvsarpur's files. |
| `POST /api/import/apply` | Body `{source}`. Imports it and returns `{preview, recordedAdded, followsAdded, warnings}`. Safe to repeat: recorded pids already known are left as they are. |
| `GET /api/import/report` | Scans the library, then `{recordedButMissing: [{pid, origin, title}], onDiskNotRecorded: [{pid, path}], scanError}`. |
| `GET /api/events?client_id=` | Server-sent events. `event: job` (every job change, to everyone), `event: follows` (run status changes, to everyone) and `event: file_ready` (only to the `client_id` that queued the browser download; routing, not security). |

## Settings

```json
{
  "autoEnabled": false, "autoIntervalMinutes": 60,
  "libraryRootDir": "<data>/downloads", "showsSubdir": "shows", "moviesSubdir": "movies", "sportsSubdir": "sports",
  "plexBaseUrl": "", "plexToken": "", "plexLibrarySectionId": "", "plexLibraryPath": "",
  "quality": "HD1080"
}
```

`quality` is `Normal`, `HD720` or `HD1080`. `plex*` configure the Plex refresh: `GET {plexBaseUrl}/library/sections/{plexLibrarySectionId}/refresh[?path={plexLibraryPath}]` with the token in the `X-Plex-Token` header. It runs once the queue is idle after one or more library downloads. Library downloads go to `libraryRootDir/<subdir>/<Plex path>`; an absolute subfolder stands on its own. With `autoEnabled`, a follow run starts every `autoIntervalMinutes` (at least 5), the first one shortly after startup.

## Follow runs

A run refreshes the catalogue (incremental), adds sids whose strict title key and English flag match a follow, fetches followed sids that aren't in the catalogue, and queues a `scheduled` library job for each episode except those that are:

- on disk: a `library` row for the pid whose file exists, from the tag scan or a finished job (`onDisk`);
- matched by name: a file with the expected name as a prefix (`nameMatch`);
- recorded with origin `library` or `unknown` and not forgotten (`recorded`);
- already queued or running (`inQueue`).

An explicit Library download checks only the first two (a tagged file anywhere in the library, or a name match) and then completes as `Already in the library`. Being recorded never blocks it.

If the refresh fails (RÚV down), the run ends as a `warning` and the next one tries again.

```json
{
  "running": false, "lastRunAt": "…", "lastRunState": "ok | warning | error",
  "lastRunMessage": "Queued 2 episodes from 1 followed show",
  "lastReport": {
    "dryRun": false, "follows": 1, "episodes": 25,
    "addedSids": [{"followId": 1, "sid": "27447", "seriesTitle": "Fyrir alla muni"}],
    "scheduled": [{"pid": "85o53h", "title": "…", "path": "…/Fyrir alla muni - s01e01.mp4"}],
    "skipped": {"onDisk": 0, "nameMatch": 0, "recorded": 23, "inQueue": 0},
    "warnings": []
  }
}
```

## Jobs

```json
{
  "id": "…", "clientId": "…", "pid": "bfcj7b", "title": "Veður s01e299",
  "mode": "library", "priority": "interactive",
  "state": "queued | running | completed | failed | cancelled",
  "progress": 0, "message": "Downloading", "finalPath": "/media/TV Shows/Veður/Season 01/Veður - s01e299.mp4",
  "files": [{"name": "Veður s01e299.mp4", "url": "/api/jobs/…/files/Ve%C3%B0ur%20s01e299.mp4"}],
  "ffmpegExit": null, "error": null,
  "createdAt": "2026-11-07T22:17:02Z", "startedAt": null, "finishedAt": null
}
```

`files` is only filled for browser (`web`) downloads: the video, then its `.srt` subtitles, then any `.vtt` that has no `.srt`. `subtitleLanguages` is what the request asked for (`null`: all).

After the video, a download fetches its subtitles as `<video>.<lang>.vtt` plus a converted `.srt` (both modes). Library downloads also fetch artwork: a movie's `poster.jpg` in its folder, an episode's image as `<video>.jpg` and the series `poster.jpg` two folders up (never overwritten); nothing goes in the category folder itself. Failures there are logged and don't fail the job. A library download that finds a matching file already on disk completes with the message `Already in the library` and runs no ffmpeg.

## Library scan

Walks the three category folders for `*.mp4` and reads each file's `©cmt` tag (`ruvinfo:{pid}:{sid}`). Tags are only read for new or changed files (cached by path, size and mtime). Tagged files get a `library` row with source `tag`; rows under a scanned folder whose file is gone are removed. A category folder that doesn't exist (an unmounted share) is skipped and keeps its rows.

```json
{
  "running": false, "lastScanAt": "…", "roots": ["/media/TV Shows", "…"],
  "files": 79, "tagged": 15, "read": 0, "removed": 0, "errors": 0, "elapsedMs": 70, "error": null
}
```

## Import

From ruvsarpur's folder (its Docker data folder, `~/.ruvsarpur`, or the `--portable` folder next to the old script):

- `gui-settings.json`: the folder settings, mapped (`/data/...` becomes `<data>/...`; anything else, such as `/media/...` or a Windows path, is kept), after the old `outputDir` → `libraryRootDir` migration. `auto*` and `plex*` are copied; `watchlistSids` become follows.
- `prevrecorded.log`: `recorded` rows. The origin comes from `download_request` lines in `download-events.log` and its backups: `library` if the pid was ever requested for the library, `web` if only for the browser, else `unknown`.
- `tvschedule.json` and `imdb-cache.json` are copied when the data folder has none.

```json
{
  "source": "C:\\Users\\…\\ruvsarpur\\src", "settingsFile": true, "recordedFile": true, "eventLogs": 2,
  "settings": {"libraryRootDir": "…", "plexToken": "••••", "…": "…"},
  "pathMapping": [{"key": "libraryRootDir", "from": "/data/downloads", "to": "<data>/downloads", "changed": true}],
  "followSids": ["36706"],
  "recorded": {"total": 4, "library": 2, "web": 1, "unknown": 1, "alreadyKnown": 0},
  "copies": ["tvschedule.json"], "warnings": []
}
```

## Event log

`<data>/download-events.log`, one JSON object per line, as ruvsarpur wrote it: `{"details": {…}, "event": "…", "status": "…", "timestampUtc": "2026-11-07T22:17:02.123456Z"}`, rotated at 5 MB with 5 backups. Events: `download_request` (`queued`), `download` (`completed`, `failed` or `cancelled`), `follow_run`, `plex_refresh` and `import`.

## SSE payloads

```
event: job
data: {"type":"job","job":{…job…}}

event: follows
data: {"type":"follows","status":{…run status…}}

event: plex
data: {"type":"plex","result":{"state":"ok","message":"…"}}

event: file_ready
data: {"type":"file_ready","clientId":"…","jobId":"…","name":"Veður s01e299.mp4","url":"/api/jobs/…/files/…"}
```
