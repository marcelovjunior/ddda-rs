"""Summarize complete L153 census snapshots; no game data is required.

python scripts/sync_profile.py run.log --first 2 --last -1 --top 30
Indices select complete two-second snapshots (default: zero to final).
Times are inclusive wall time, summed across threads; not CPU percentages.
"""
import argparse
import json
import re
from pathlib import Path

FIELDS = ("calls", "ns", "contended", "service_contended", "blocks", "timeouts")


def snapshots(lines):
    current = None
    for line in lines:
        if line.startswith("sync-census "):
            current = {"meta": dict(re.findall(r"(\w+)=(\d+)", line)), "rows": {}}
        elif current is not None and line.startswith("sync-row "):
            before, _, name = line.strip().partition(" name=")
            fields = dict(re.findall(r"(\w+)=(0x[\da-f]+|\d+)", before))
            if not name or not all(k in fields for k in (*FIELDS, "tid", "target", "site", "arg0")):
                raise ValueError("malformed sync-row")
            row = {k: int(v, 0) for k, v in fields.items()}
            row["name"] = name
            hist = re.search(r"hist_us=(\[[\d, ]+\])", before)
            row["hist_us"] = json.loads(hist[1]) if hist else [0] * 16
            key = (row["tid"], row["target"], row["site"], row["arg0"], name)
            current["rows"][key] = row
        elif current is not None and line.startswith("sync-census-end "):
            yield current
            current = None


def difference(first, last):
    rows = []
    for key, row in last["rows"].items():
        old = first["rows"].get(key, {})
        diff = dict(row)
        for field in FIELDS:
            diff[field] -= old.get(field, 0)
            if diff[field] < 0:
                raise ValueError("counters decreased: snapshots are not from one ordered run")
        diff["hist_us"] = [v - p for v, p in zip(row["hist_us"], old.get("hist_us", [0] * 16))]
        if diff["calls"]:
            rows.append(diff)
    frames = int(last["meta"]["frames"]) - int(first["meta"].get("frames", 0))
    return frames, rows


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("log", type=Path)
    p.add_argument("--first", type=int, help="baseline snapshot; omitted means process start")
    p.add_argument("--last", type=int, default=-1)
    p.add_argument("--top", type=int, default=30)
    p.add_argument("--json", action="store_true")
    a = p.parse_args()
    data = list(snapshots(a.log.read_text(encoding="utf-8", errors="replace").splitlines()))
    if not data:
        p.error("no complete sync census snapshots")
    baseline = data[a.first] if a.first is not None else {"meta": {}, "rows": {}}
    end = data[a.last]
    frames, rows = difference(baseline, end)
    rows.sort(key=lambda r: r["ns"], reverse=True)
    if a.json:
        print(json.dumps({"frames": frames, "meta": end["meta"], "rows": rows}, indent=2))
        return
    print(f"complete snapshots={len(data)} delta frames={frames} overflow={end['meta'].get('overflow')} "
          f"participants={end['meta'].get('participants')} max_jobs={end['meta'].get('max_jobs')}")
    print("Inclusive wall times; do not sum job and nested service time. Interlocked/TLS contention is not measurable here.")
    print("tid       site       arg0       calls   calls/frame   ns/call   miss%  lock%  total_ms  name/target")
    for r in rows[:a.top]:
        n = r["calls"]
        per_frame = f"{n / frames:.2f}" if frames > 0 else "n/a"
        print(f"{r['tid']:#08x} {r['site']:#010x} {r['arg0']:#010x} {n:7d} {per_frame:>11} "
              f"{r['ns']/n:9.1f} {100*r['contended']/n:6.2f} {100*r['service_contended']/n:6.2f} "
              f"{r['ns']/1e6:9.3f} {r['name']} {r['target']:#x}")
        if r["name"] == "scheduler-job":
            print(f"    duration log2-us buckets={r['hist_us']} (last >=16384us)")
    print("Join: rows at site 0x00dbe66d; job distributions: scheduler-job rows by target and tid.")


if __name__ == "__main__":
    main()
