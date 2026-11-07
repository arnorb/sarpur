# S2.0 spikes

Each spike checks a risky part of the plan before S2 is built. Status as of 2026-11-07.

| # | Spike | Status | Result |
| --- | --- | --- | --- |
| 1 | Golden outputs | Done | `golden/` from `tools/golden/generate.py`: 8,508 items × 3 naming modes, plus IMDB data for 143 series. Deterministic, and self-checked: the stored input fields reproduce every output. `Father Brown X` stays in `Season 01`. |
| 2 | RÚV recordings | Done | 38 responses in `fixtures/ruv/` (see its README), replayed by `tools/record/`. Series golden: `fixtures/ruv-golden/series.jsonl`. All 3 playlist formats still exist. **Found two live bugs** (below). |
| 3 | Fuzzy matching | Done | Rust `rapidfuzz` 0.5.0 matches fuzzywuzzy + python-Levenshtein on 4,924 of 4,924 pairs, if scores are rounded ties-to-even. No port needed. `partial_ratio` (the old `--find` search) isn't in that crate. `fuzzy/RESULTS.md` |
| 4 | Disk scan on the NAS | Done | 79 `.mp4` files: 9.5 s on the first run, 0.07 s warm. Every ruvsarpur file is tagged. `scan/RESULTS.md` |
| 5 | ffmpeg on Windows | Mostly done | On a `windows-latest` runner (Actions workflow `Windows spike 5`, Windows PowerShell 5.1): the pinned ffmpeg 9.0.2 verifies and runs. It writes a 326-character path with Icelandic letters (the full 300 s episode), and `.part` → rename works. `-progress pipe:1` gave 152 progress updates. `q` stops a running download cleanly (a 10.7 s playable file). Closing a Job Object kills ffmpeg. **Still needed:** a run on the Windows user's own PC for Defender/SmartScreen (`windows/README.md`). BtbN deletes dated builds after a while, so S2 must mirror the pinned zip or update the pin. |
| 6 | Follow identity | Done | The plan's title key would auto-add sign-language and audio-described variants. A stricter key fixes it; the plan is updated. `follow/RESULTS.md` |
| 7 | TrueNAS permissions | Partly | ruvsarpur's library files are `568:568` (`apps`) with mode `770`, set by the dataset's ACL, and Plex reads them. sarpur should run as 568 and leave modes to the dataset rather than forcing 664. Still to check: `.part` then rename inside `/Media` from a container running as 568. |
| 8 | End to end | Not started | Needs the first S2 code. The Windows half goes to the Windows user. |

## Live bugs found by spike 2 (in the running ruvsarpur app)

- **`getProgramType` no longer works.** RÚV answers the persisted-query hash in `runMain` (and the worker's `resolve`) with `PersistedQueryNotFound`. Only episodes whose top-level `file` isn't a full `ruv-vod.akamaized.net` URL reach that lookup. In the 2026-11-07 catalogue that's 4 of 8,423 items, all radio (`ruv-radio…/*.mp3`), which the video pipeline doesn't handle anyway; every TV episode has a full URL. Not fixed (2026-11-07): no TV download is affected. S2's `resolve` should drop the GraphQL fallback, or fail with a clear message, rather than port a dead lookup.
- **`is_docu` is always false.** RÚV's documentary category slug is now `heimildarefni`, but the code checks for `heimildarmyndir`. Documentaries are named and filed as TV shows, not movies.
