# Spike 6: follow identity

Run: `python spikes/follow/identity.py` on the pinned `fixtures/tvschedule.json` (580 series ids). Raw groups go to `out/groups.jsonl`.

## Finding: the plan's title key merges accessibility variants

Plan §3.5 defines `title_key` as `normalizeSeriesLabel` → `trimSeasonNumberSuffix` → lowercase → NFC. `normalizeSeriesLabel` (`App.tsx:269`, and `_normalize_series_title` in `gui_api.py`) strips **every** trailing parenthesis group. In `series_title` the only such groups are variant markers:

- `(with English subtitles)`: has its own `english_subtitled` flag, so the flag rule keeps it apart.
- `(með táknmálstúlkun)` (sign-language interpretation) and `(með sjónlýsingu)` (audio description) have **no** flag, so the plan's rule would auto-add them.

| Key | Groups with >1 sid | Mixed English flag | Raw titles differ |
| --- | --- | --- | --- |
| Plan (strip all parentheses) | 41 | 9 | 16 |
| **Strict (no stripping)** | **30** | **0** | **1** |

With the plan's key, following *Fréttir* (sid 30762) would auto-add *Fréttir (með táknmálstúlkun)* (sid 31575), which is 1,169 sign-language episodes. *Krakkafréttir* would pull in three sign-language sids.

The `(N af M)` counter appears in `title`, never in `series_title` (0 of 580), so stripping parentheses gains nothing.

## Recommendation for §3.5

- `title_key = NFC(lower(trimSeasonNumberSuffix(series_title.strip())))`, with no parenthesis stripping.
- Keep the English-flag equality check as a safeguard, though it never decides anything on this data.
- With the strict key, all 30 shared groups are the same show split across sids. For example, *Fyrir alla muni* has 4 sids, one per season. The one other merge is a case difference: *Sögur Snjómannsins* / *Sögur snjómannsins*. That's the behaviour Follow wants: new seasons are added automatically, and variants are not.
- The label the UI shows for a series can keep using `normalizeSeriesLabel`. Only the key changes.
