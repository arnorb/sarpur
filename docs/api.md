# sarpur HTTP API (S2.1 slice 1)

JSON over HTTP under `/api`. Field names are camelCase, as in ruvsarpur's `gui_api.py`. Errors come back as `{"error": "..."}` with a 4xx or 5xx status.

| Method and path | What it does |
| --- | --- |
| `GET /api/health` | `{ok, version, dryRun, catalogue: <status>}` |
| `GET /api/settings` | The settings (below). |
| `POST /api/settings` | Applies the known keys of the JSON body and returns the settings. Unknown keys, including `watchlistSids`, are ignored. A known key with the wrong type is a 400. |
| `GET /api/catalogue?q=&limit=` | `{items, status}`. Case-insensitive substring search over `title` and `seriesTitle`, newest first. `limit` defaults to 100 (max 1000). Each item: `pid, sid, title, seriesTitle, showtime, duration, contentType, englishSubtitled`. |
| `GET /api/catalogue/status` | `{state, count, loadedAt, error}`; `state` is `empty`, `loading`, `refreshing`, `ready` or `failed`. |
| `POST /api/catalogue/refresh` | 202. Fetches the catalogue from RÚV in the background (takes minutes), then reloads it. |
| `POST /api/downloads` | Body `{pid, mode: "web" \| "library", clientId?}` (`client_id` also accepted). 201 with the new job. |
| `GET /api/jobs` | `{jobs}`, newest first. |
| `DELETE /api/jobs/{id}` | Queued: cancelled. Running: ffmpeg is stopped, the `.part` deleted, then the job becomes `cancelled`. Finished: removed from the list (a browser download's file goes too). Returns `{removed, job}`. 404 for an unknown id. |
| `POST /api/jobs/{id}/retry` | Puts a finished job back at the end of the queue. 400 if it is still queued or running. |
| `GET /api/jobs/{id}/files/{name}` | The file of a completed browser download, for 24 hours. `Content-Disposition` carries an ASCII `filename` and an RFC 5987 `filename*`. |
| `GET /api/events?client_id=` | Server-sent events. `event: job` (every job change, to everyone) and `event: file_ready` (only to the `client_id` that queued the browser download; routing, not security). |

## Settings

```json
{
  "autoEnabled": false, "autoIntervalMinutes": 60,
  "libraryRootDir": "<data>/downloads", "showsSubdir": "shows", "moviesSubdir": "movies", "sportsSubdir": "sports",
  "plexBaseUrl": "", "plexToken": "", "plexLibrarySectionId": "", "plexLibraryPath": "",
  "quality": "HD1080"
}
```

`quality` is `Normal`, `HD720` or `HD1080`. Library downloads go to `libraryRootDir/<subdir>/<Plex path>`; an absolute subfolder stands on its own. `auto*` and `plex*` are stored but not used yet.

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

`files` is only filled for browser (`web`) downloads. A library download that finds a matching file already on disk completes with the message `Already in the library` and runs no ffmpeg.

## SSE payloads

```
event: job
data: {"type":"job","job":{…job…}}

event: file_ready
data: {"type":"file_ready","clientId":"…","jobId":"…","name":"Veður s01e299.mp4","url":"/api/jobs/…/files/…"}
```
