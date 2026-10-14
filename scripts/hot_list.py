"""Choose the lifted functions to build at opt-level 1 (the `lift_hot_00` package) from saved
`sample_profile.py --save-stacks` captures (autorun3.sh writes them).

    python scripts/hot_list.py CAPTURE.json [CAPTURE2.json ...] [--top 1000] [--out hot.txt]

A busy (not waiting) guest-thread sample counts for the nearest lifted function on its stack, the
function whose code (and whose memory/flag helper calls) was running. Functions with a registered
port (`crates/ddports/src/lib.rs`, and the semantic modules' addresses) are skipped: the port
replaces them. The functions already in the hot package are kept (they stay listed first).
The output is one hexadecimal address per line, for
`research/native/lift.py gen --hot-functions hot.txt` (see docs/HOT_PATHS.md section 10).
Measured: opt-level 1 runs a lifted function about 2.5x faster than opt-level 0 (HOT_PATHS.md)."""
import argparse
import collections
import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
WAIT = re.compile(r"Wait|wait_until|ZwAlert|NtDelay|Sleep|park|Alert|futex|SwitchToThread|NtYield", re.I)

p = argparse.ArgumentParser()
p.add_argument("captures", nargs="+")
p.add_argument("--top", type=int, default=1000)
p.add_argument("--out", default="hot.txt")
a = p.parse_args()

ports = {int(x, 16) for x in re.findall(r"0x([0-9a-fA-F_]{6,10})", (ROOT / "crates/ddports/src/lib.rs").read_text().replace("_", ""))}
current = [int(x, 16) for x in re.findall(r"\(0x([0-9A-F]+), f_", (ROOT / "crates/ddlift/lift_hot_00/src/generated/mod.rs").read_text())]
count = collections.Counter()
busy = 0
for path in a.captures:
    d = json.load(open(path))
    sym = d["symbols"]
    cache = {}

    def lifted(pc):
        if pc not in cache:
            m = re.search(r"lifted_\d+10f_([0-9a-f]{8})", sym.get(str(pc), ""))
            cache[pc] = int(m.group(1), 16) if m else None
        return cache[pc]

    for th in d["threads"]:
        if "guest" not in th["name"]:
            continue
        for st in th["stacks"]:
            if any(WAIT.search(sym.get(str(pc), "")) for pc in st[:6]):
                continue
            busy += 1
            f = next((x for x in map(lifted, st) if x is not None), None)
            if f is not None and f not in ports:
                count[f] += 1
chosen = list(dict.fromkeys(current + [f for f, _ in count.most_common(a.top)]))
covered = sum(v for f, v in count.items() if f in set(chosen))
total = sum(count.values())
Path(a.out).write_text("\n".join(f"0x{f:08X}" for f in chosen) + "\n")
print(f"{busy} busy guest samples, {total} attributed to {len(count)} lifted functions without a port;")
print(f"{len(chosen)} functions chosen ({len(current)} already hot) cover {100 * covered / max(total, 1):.1f}% of them -> {a.out}")
