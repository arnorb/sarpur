import json, sys, math
from collections import Counter
THRESH = 85  # code: fuzz.ratio(...) > 85
pairs = [json.loads(l) for l in open("out/pairs.jsonl")]
py = [json.loads(l)["ratio"] for l in open("out/py.jsonl")]
rs = [json.loads(l)["ratio"] for l in open("out/rust.jsonl")]
assert len(pairs) == len(py) == len(rs)
def half_even(x): return int(round(x))
def half_up(x): return int(math.floor(x + 0.5))
for name, rnd in (("round-half-even (python round)", half_even), ("round-half-up", half_up)):
    diffs = [(i, py[i], rnd(rs[i])) for i in range(len(py)) if py[i] != rnd(rs[i])]
    flips = [(i, a, b) for i, a, b in diffs if (a > THRESH) != (b > THRESH)]
    print(f"== rapidfuzz {name}")
    print(f"pairs={len(py)} exact_int_match={len(py)-len(diffs)} ({100*(len(py)-len(diffs))/len(py):.3f}%) max_int_diff={max([abs(a-b) for _,a,b in diffs], default=0)} threshold_flips={len(flips)}")
    by = Counter(pairs[i]["src"] for i, _, _ in diffs)
    print("diffs by source:", dict(by))
    for i, a, b in diffs:
        print(f"  DIFF src={pairs[i]['src']} py={a} rust_raw={rs[i]!r} rust_rounded={b} a={pairs[i]['a']!r} b={pairs[i]['b']!r}{'  FLIP' if (a>THRESH)!=(b>THRESH) else ''}")
# raw float vs python float ratio
near = sum(1 for i in range(len(py)) if abs(rs[i] - py[i]) >= 0.5)
print("pairs with |rust_raw - py_int| >= 0.5:", near)
print("pairs near threshold (80..90 by py):", sum(1 for x in py if 80 <= x <= 90))
