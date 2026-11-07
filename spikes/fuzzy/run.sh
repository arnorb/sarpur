#!/bin/sh
# Usage (from spikes/fuzzy): PATH="$HOME/.cargo/bin:$PATH" ./run.sh
PY=/Users/arnorbogason/Projects/ruvsarpur/venv/bin/python
set -e
cargo build --release
$PY make_pairs.py
./target/release/fuzzy-spike < out/pairs.jsonl > out/rust.jsonl
$PY py_scores.py out/pairs.jsonl > out/py.jsonl
$PY compare.py | tee out/compare.txt | head -20
