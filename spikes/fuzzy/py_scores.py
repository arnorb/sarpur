import json, sys
from fuzzywuzzy import fuzz
import fuzzywuzzy.StringMatcher as sm
print("SequenceMatcher impl:", fuzz.SequenceMatcher.__module__, file=sys.stderr)
for line in open(sys.argv[1]):
    p = json.loads(line)
    print(json.dumps({"ratio": fuzz.ratio(p["a"], p["b"])}))
