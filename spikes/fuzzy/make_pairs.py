"""Build pairs.jsonl: (a=query lowered, b=candidate lowered) as lookupItemInIMDB compares them."""
import json, random, unicodedata, sys
random.seed(7)
ROOT = "/Users/arnorbogason/Projects/sarpur"
cache = json.load(open("/Users/arnorbogason/Projects/ruvsarpur/.local-data/imdb-cache.json"))
sched = json.load(open(ROOT + "/fixtures/tvschedule.json"))
recs = {}  # (query_title, imdb_title) real matched entries
def add(q, t, src):
    if q and t:
        recs.setdefault((q.lower(), t.lower()), src)
for sid, c in cache.items():
    im = c.get("imdb") or {}
    for q in (c.get("original-title"), c.get("series_title")):
        add(q, im.get("title"), "cache")
for v in sched.values():
    if isinstance(v, dict) and v.get("imdb"):
        im = v["imdb"]
        for q in (v.get("original-title"), v.get("series_title")):
            add(q, im.get("title"), "schedule")
real = [(q, t, s) for (q, t), s in recs.items()]
queries = sorted({q for q, _, _ in real})
cands = sorted({t for _, t, _ in real})
pairs = [(q, t, "real-" + s) for q, t, s in real]
# Realistic non-matching candidates: other results IMDB would return for the same query (approximated by random others + nearest by shared prefix)
for q in queries:
    for t in random.sample(cands, min(8, len(cands))):
        pairs.append((q, t, "real-cross"))
# Near-threshold perturbations of real matches (edit 1-4 chars) to land around 85
for q, t, _ in random.sample(real, min(300, len(real))):
    t2 = list(t)
    for _ in range(random.randint(1, 4)):
        if not t2: break
        op = random.choice("ids")
        i = random.randrange(len(t2))
        if op == "i": t2.insert(i, random.choice("abcdeðþæö "))
        elif op == "d": del t2[i]
        else: t2[i] = random.choice("abcdeðþæö")
    pairs.append((q, "".join(t2), "perturbed"))
# Synthetic hard pairs
base = ["þjóðleikhúsið", "ævintýri á gönguför", "ösku buska", "áramótaskaup", "Ísland í dag", "Hvolpasveitin", "Víkingar", "the office", "The Office (US)", "game of thrones", "Mr. Bean", "Dr. Who", "a", "ab", "abc", "ö", "Þ", "ÐÞÆÖÁ", "ß", "İstanbul", "ǅ", "é", "日本語", "😀 smiley", "  spaced  ", "x" * 60]
def forms(s):
    return {s, s.lower(), s.upper(), unicodedata.normalize("NFC", s), unicodedata.normalize("NFD", s), s.replace(" ", ""), s.replace(".", "").replace("(", "").replace(")", ""), s + "!", s + " ", " " + s, s[::-1]}
syn = []
for s in base:
    f = sorted(forms(s))
    for x in f:
        for y in f:
            syn.append((x, y, "synthetic"))
syn += [("", "", "synthetic"), ("", "abc", "synthetic"), ("abc", "", "synthetic"), (" ", "", "synthetic"), ("a", "a", "synthetic")]
for _ in range(300):
    s = random.choice(base); t = random.choice(base)
    syn.append((s.lower(), t.lower(), "synthetic"))
random.shuffle(syn)
pairs += syn[:700]
with open("out/pairs.jsonl", "w") as f:
    for a, b, src in pairs:
        f.write(json.dumps({"a": a, "b": b, "src": src}, ensure_ascii=False) + "\n")
from collections import Counter
print(len(pairs), Counter(s for _, _, s in pairs))
