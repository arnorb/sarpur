# Spike 3: fuzzy matching (fuzzywuzzy `fuzz.ratio` vs Rust `rapidfuzz`)

## Setup
- Python: fuzzywuzzy 0.18.0 with **python-Levenshtein 0.27.5 installed**, so `fuzz.SequenceMatcher` is `fuzzywuzzy.StringMatcher` (Levenshtein-based), not difflib. This is what the venv (and so S1) actually uses. Note `requirements.txt` decides this in production: without python-Levenshtein fuzzywuzzy falls back to difflib and scores change.
- Rust: `rapidfuzz = "=0.5.0"`, `serde_json = "=1.0.154"`. rapidfuzz 0.5.0 exposes only `fuzz::ratio` (no `partial_ratio`, no token_* scorers). The `--find` search (R:1587-1609) uses `fuzz.partial_ratio`, which is **not covered** by this spike and not available in this crate version.
- Pairs (4,924, in `out/pairs.jsonl`, regenerate with `run.sh`): 436 real matched (query lowered, IMDB title lowered) pairs from the IMDB cache (R:1338/1344: query is the foreign title, or the series title for movies) and tvschedule.json; 3,488 real-cross pairs (every real query against 8 random real IMDB titles, standing in for the non-matching candidates in an IMDB result list, which are not cached); 300 near-threshold perturbations of real matches; 700 synthetic hard pairs (þ ð æ ö á, case, punctuation, empty strings, NFC vs NFD, emoji, İ, reversed, padded). 149 pairs score 80..90.
- The code only ever compares `fuzz.ratio(a.lower(), b.lower()) > 85`.

## Result
- Rust raw score is a float (0..100). fuzzywuzzy returns `int(round(100 * ratio))` (Python 3 `round` = ties-to-even).
- **With ties-to-even rounding: 4,924 / 4,924 identical (100%), max diff 0, 0 threshold flips.**
- With round-half-up instead: 4,875 / 4,924 identical (99.0%); all 49 diffs are exact .5 ties (12.5 -> 12 vs 13, 62.5 -> 62 vs 63), max diff 1, still 0 threshold flips (a tie at 85.5 gives 86 both ways, and 84.5 gives 84 or 85, both `> 85` false).
- For information, difflib (what fuzzywuzzy uses without python-Levenshtein) differs from the Levenshtein scorer on 1,627 of the 4,924 pairs, though none flip the threshold in this set.
- NFC vs NFD pairs: both implementations work on code points, so they agree (NFC vs NFD of the same title scores e.g. 79 in both, so a normalization step would be a behavior change, not a port detail); there is no normalization in either.

## Verdict
rapidfuzz 0.5.0 `fuzz::ratio` is a drop-in for `fuzz.ratio` as used at R:268-284, **provided** we compare on `chars()` (code points, as Python does) and round with `f64::round_ties_even(score * 100.0)` before `> 85` (equivalently `raw >= 85.5`). No port of fuzzywuzzy's algorithm is needed. Keep this comparison set (`spikes/fuzzy/out/pairs.jsonl` generator) as a regression test in S2. Open item: `partial_ratio` for `--find` needs a different crate version or its own port.
