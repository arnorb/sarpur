# Recorded RÚV responses

Recorded on **2026-11-07** from the real RÚV services, by ruvsarpur's own functions
(`worker/ruvsarpur.py`, vendored from tag v-s1). 38 responses, about 780 KB of bodies
(880 KB on disk). `index.json` maps `(method, url)` to a file, status and content type.

## What is here

| Endpoint | Count | Made by |
| --- | --- | --- |
| `GET api.ruv.is/api/programs/featured/tv` (trimmed, see below) | 1 | `getVodSchedule` |
| `GET api.ruv.is/api/programs/program/{sid}/all` | 14 | `getVodSeriesSchedule` (5 from the capped `getVodSchedule` walk, 9 chosen) |
| `GET www.ruv.is/gql/?operationName=getSearch` ("Veður", "Brown") | 2 | `getVodSearchResults` |
| `GET www.ruv.is/gql/?operationName=getProgramType` (3 pids) | 3 | `requestsVodDataRetrieveWithRetries`, same query string as `runMain` and the worker's `resolve` |
| master playlists (`*.m3u8`) and the one variant playlist `find_m3u8_playlist_url` fetches | 18 | `find_m3u8_playlist_url`, quality HD1080 |

Chosen sids: 30766 Veður (long episodic), 35732 Séra Brown (Father Brown), 34968
Ofurhetjuskólinn (season 2), 33071 Jólastund snjóbarnanna (movie), 31136 Grænir fingur
(documentary), 37857 Systraslagur 2025 (sport), 35785 Undraveröld villtu dýranna (kids),
32719 Verbúðin (with English subtitles), 38169 Kastljós (newest playlist format).
No media segments were recorded.

## Trimmed bodies

The code under test saw the trimmed body, so the fixtures are exactly what it processed
(index entries carry a `trimmed` note):

- `featured/tv`: 2 panels x 3 programs (the real body is about 5 MB).
- `program/30766/all` (Veður): first 40 of 1,377 episodes. `program/38169/all` (Kastljós): first 40 of 151.
  Because `ep_total` is `len(episodes)`, the golden for those two shows e.g. "299 af 40".

## Replay

`tools/record/replay.py` patches `requests.Session.request`, the single layer under both
`requests.get()` and `__create_retry_session()`. A request that was not recorded raises
`UnrecordedRequest` (a `BaseException`, so ruvsarpur's `except Exception` blocks cannot hide it).
IMDB lookups (`lookupItemInIMDB`) are stubbed to return nothing in record and replay; IMDB is
covered by `golden/imdb.jsonl`.

```bash
PY=/path/to/venv/bin/python        # needs the worker requirements
$PY -m unittest discover -s tools/record          # offline tests
$PY tools/record/golden_series.py                 # regenerates fixtures/ruv-golden/series.jsonl
$PY tools/record/record.py                        # re-record (network, about 45 requests, 0.4 s apart); overwrites this folder
```

`fixtures/ruv-golden/series.jsonl`: one line per episode (sid, pid, is_movie/is_docu/is_sport,
ep_num, ep_total, season_num, english_subtitled, series_title, title), generated offline under the
worker's frozen clock `2026-11-07T12:00:00`. The only date-dependent branch is `sport_season`
(`date.today()`); output is byte-identical across runs.

## Playlist formats

The master playlist is parsed by `_playlist_quality_variants` / `_playlist_default_audio_url`
whenever it advertises `RESOLUTION=`; otherwise `find_m3u8_playlist_url` guesses paths.

1. **Current: separate audio group, `1080p/stream.m3u8`.** Found (e.g. `5499461-MRDLS2`, `5458120-AQMTO5`).
   `#EXT-X-MEDIA:TYPE=AUDIO,...URI="ISL/stream.m3u8"` gives `audio_url`.
2. **Muxed audio, `3600/index.m3u8` (2022 "lokad"/"opid" style).** Found, the most common (`5458296T0`, `5396807A0`, ...).
   Some masters add a `TYPE=SUBTITLES` group. No audio group, so `audio_url` is `None`.
3. **Legacy fixed-bitrate, `asset-audio=50000-video=3550000.m3u8?tlm=hls&streams=...`.** Found once
   (`lokad/5226245U0`). Its master still has `RESOLUTION=`, so the code takes the advertised-variant
   path; the audio `EXT-X-MEDIA` line has no `URI`, so `audio_url` is `None`.

**Not found:** a master without `RESOLUTION=`. The code's guessing branches (`old_format`
`asset-audio=...` and `{vod_url}/{code}/index.m3u8`) were not reached by any sampled playlist
(about 50 masters probed across all file-URL patterns in `fixtures/tvschedule.json`); they may be dead code.
One sampled `opid/5457279M1` master returned 404 (expired).

## Differences from what the code expects

- `getProgramType` now answers `{"errors":[{"message":"PersistedQueryNotFound"...}]}` (HTTP 200) for the
  hash `9d18a07f...` in `runMain` and the worker's `resolve`. The code retries 3 times with a 3 s sleep, then gives up.
  The three recordings are those error bodies. The playlist recordings use the `file` URL from `program/{sid}/all` instead.
- Category slug for documentaries is now `heimildarefni`; the code tests `heimildarmyndir`, so `is_docu` is False for every
  recorded series (the pinned `tvschedule.json` also has 0 `is_docu` items).
- Pid `akoha1` (Father Brown in `tvschedule.json`) no longer exists; sid 35732 now lists 4 episodes (`akohaa`, `akoha9`, `akoha8`, `akoha7`).
- `getSearch` (hash `823f9e99...`) still works.
