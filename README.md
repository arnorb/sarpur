# sarpur

A downloader for RÚV's Sarpur, being rebuilt as one Rust program with a built-in web UI. It will replace [ruvsarpur](https://github.com/arnorb/ruvsarpur) (a Python CLI plus a separate web GUI) in stages:

| Stage | What | Status |
| --- | --- | --- |
| S1 | Fix the Python app in `ruvsarpur` | Done (tag `v-s1`), running on the NAS |
| S2.0 | Spikes: check the risky parts before building | In progress |
| S2 | Rust server, queue, database and UI; RÚV access and naming still in the Python worker | Not started |
| S3 | Port the worker to Rust, checked against the Python output | Not started |

The plan lives in the `ruvsarpur` repo at `.omc/plans/sarpur-rewrite.md` (not committed).

## Layout

- `worker/`: the Python worker, vendored from `ruvsarpur` at tag `v-s1`. Run it as `python worker/ruvsarpur.py --json <command>`. It writes JSON lines only. Commands: `catalogue`, `series`, `resolve`, `name`, `imdb`, plus `--now` to freeze the clock.
- `fixtures/tvschedule.json`: the pinned catalogue (8,508 items, schedule date 2026-03-21, sha256 `78eb107c…df1b6e`).
- `golden/`: names in three modes and IMDB data, generated from the worker by `tools/golden/generate.py` with a frozen clock. Each line carries the input fields that produce it. `manifest.json` records the hashes.
- `spikes/`: the S2.0 spikes and their results (see `spikes/README.md`).

## Regenerating the golden files

```bash
pip install -r worker/requirements.txt
python tools/golden/generate.py
```

The output must match `golden/manifest.json` byte for byte. If it doesn't, the naming code or the fixture has changed.
