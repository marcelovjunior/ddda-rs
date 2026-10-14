"""Rank the lifted functions that run under a root function (default the shadow
job callback 0x00DB7620) in saved `sample_profile.py --save-stacks` captures.

    python scripts/profile_units.py CAPTURE.json [CAPTURE2.json ...] [--root 0x00db7620] [--top 40]

Prints, over the guest-thread stacks that contain the root, the share of those
stacks each lifted function (or mtcore port) appears in (inclusive), and the
direct lifted callee of the root (the unit's virtual method). Captures come
from autorun3.sh (C:/ddda-targets/L172/runs/stacks_<tag>.json)."""
import argparse
import collections
import json
import re

p = argparse.ArgumentParser()
p.add_argument("captures", nargs="+")
p.add_argument("--root", default="0x00db7620")
p.add_argument("--top", type=int, default=40)
a = p.parse_args()
root = "0x%08x" % int(a.root, 16)
inclusive, child, stacks = collections.Counter(), collections.Counter(), 0
for path in a.captures:
    d = json.load(open(path))
    sym = d["symbols"]

    def name(pc):
        n = sym.get(str(pc), "")
        m = re.search(r"lifted_\d+10f_([0-9a-f]{8})", n)
        if m:
            return "0x" + m.group(1)
        if "mtcore" in n:
            return "mt:" + re.sub(r".*mtcore\d*", "", n)[:50]
        return None

    for th in d["threads"]:
        if "guest" not in th["name"]:
            continue
        for st in th["stacks"]:
            fr = [f for f in (name(pc) for pc in st) if f]  # leaf first
            if root not in fr:
                continue
            stacks += 1
            i = fr.index(root)
            for f in set(fr[:i]):
                inclusive[f] += 1
            if i:
                child[fr[i - 1]] += 1
print(f"{stacks} stacks contain {root}")
print("inclusive share below the root:")
for k, v in inclusive.most_common(a.top):
    print(f"{100 * v / stacks:5.1f}% {k}")
print("direct lifted callee of the root:")
for k, v in child.most_common(15):
    print(f"{100 * v / stacks:5.1f}% {k}")
