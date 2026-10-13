"""Attribute sampled job stacks without treating sequential samples as a timeline.

python scripts/critical_jobs.py stacks.json --top 20
Only stacks containing the original worker dispatcher 00DBDC90 are included.
The outermost visible lifted function above it is a sampled path, not proof of
the dispatched callback (tail calls and missing symbols can hide that boundary).
Counts are samples, never CPU time or per-join critical-path measurements.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re

LIFTED = re.compile(r"f_([0-9a-fA-F]{8})(?:_|$)")


def join_metrics(batch, end, jobs, joins):
    """Accept only a complete, non-overlapping, loss-free batch with one join."""
    if batch["nested"] or end["panicked"] or end["lost"]:
        raise ValueError("nested, panicked or lossy batch")
    if len(jobs) != batch["queued"] or len(joins) != 1 or not jobs:
        raise ValueError("missing callbacks/join or serial/empty batch")
    join = joins[0]
    if join["result"] != 0 or not batch["ns"] <= join["start"] <= join["end"] <= end["ns"]:
        raise ValueError("unsuccessful or unordered join")
    if any(j["panicked"] or not batch["ns"] <= j["start"] <= j["end"] <= join["end"] for j in jobs):
        raise ValueError("callback outside batch/join")
    ordered = sorted(jobs, key=lambda j: j["end"])
    last = ordered[-1]
    second = ordered[-2]["end"] if len(ordered) > 1 else None
    longest = max(jobs, key=lambda j: j["end"] - j["start"])
    tail_start = max(join["start"], second) if second is not None else None
    return {"batch": batch["id"], "frame": batch.get("frame", 0), "present_label": batch["present"],
            "queued": batch["queued"], "join_ns": join["end"] - join["start"],
            "last_target": last["target"], "last_candidate_virtual": last["resolved"],
            "last_vtable": last["vtable"], "last_tid": last["tid"],
            "last_duration_ns": last["end"] - last["start"],
            "longest_target": longest["target"],
            "longest_duration_ns": longest["end"] - longest["start"],
            "last_end_ns": last["end"], "penultimate_end_ns": second,
            "wait_after_other_jobs_ns": join["end"] - tail_start if tail_start is not None else None,
            "last_job_wait_overlap_ns": max(0, last["end"] - tail_start) if tail_start is not None else None,
            "after_last_completion_ns": max(0, join["end"] - max(join["start"], last["end"]))}


def timeline(lines):
    active = {}
    rejected = Counter()
    rows = []
    wakes = []
    version = False
    for line in lines:
        if line.strip() == "critical version=1":
            version = True
            continue
        match = re.fullmatch(r"(Frame|Batch|End|Job|Join|Wake) \{ (.*) \}\s*", line)
        if not match:
            rejected["malformed line"] += 1
            continue
        kind, fields = match.groups()
        record = {}
        for field in fields.split(", "):
            key, value = field.split(": ", 1)
            record[key] = value == "true" if value in ("true", "false") else int(value)
        if kind == "Frame":
            continue
        if kind == "Wake":
            wakes.append(record)
            continue
        ident = record["id"]
        if kind == "Batch":
            if ident in active:
                raise ValueError("duplicate batch id")
            active[ident] = (record, [], [])
        elif ident not in active:
            rejected["event without batch"] += 1
        elif kind == "Job":
            active[ident][1].append(record)
        elif kind == "Join":
            active[ident][2].append(record)
        else:
            batch, jobs, joins = active.pop(ident)
            try:
                rows.append(join_metrics(batch, record, jobs, joins))
            except (ValueError, KeyError) as e:
                rejected[str(e)] += 1
    if not version:
        raise ValueError("missing critical trace version")
    rejected["unfinished batch"] += len(active)
    return {"joins": rows, "wakes": wakes, "rejected": dict(rejected)}


def address(name):
    match = LIFTED.search(name)
    return int(match[1], 16) if match else None


def category(name):
    if "fast_set_u" in name:
        return "fast_store"
    if "fast_u" in name:
        return "fast_load"
    if "::Flags::" in name:
        return "flags"
    if "::x87::" in name or "3x87" in name:
        return "x87"
    if "::sse::" in name or "::sse2::" in name:
        return "sse"
    if "<unresolved" in name or name.startswith("?"):
        return "unresolved"
    if address(name) is not None:
        return "lifted"
    return "other"


def analyze(data):
    symbols = data["symbols"]
    paths = Counter()
    nearest = defaultdict(Counter)
    roots = defaultdict(Counter)
    helper_roots = defaultdict(Counter)
    totals = Counter()
    threads = []
    for thread in data["threads"]:
        matched = 0
        for stack in thread["stacks"]:
            names = [symbols.get(str(pc), f"?{pc:#x}") for pc in stack]
            functions = [(i, address(name)) for i, name in enumerate(names)]
            dispatch = next((i for i, a in functions if a == 0xDBDC90), None)
            if dispatch is None:
                continue
            matched += 1
            totals["dispatcher_samples"] += 1
            # A dispatcher itself waiting/claiming/signalling work has no
            # visible job above it. Keep these out of the job denominator.
            chain = []
            for i, a in functions:
                if i >= dispatch:
                    break
                if a is not None and (not chain or chain[-1] != a):
                    chain.append(a)
            if not chain:
                totals["no_visible_job"] += 1
                continue
            totals["visible_job_samples"] += 1
            root, leaf = chain[-1], chain[0]
            nearest[leaf][names[0]] += 1
            roots[root][leaf] += 1
            helper_roots[root][category(names[0])] += 1
            paths[tuple(reversed(chain))] += 1
        if matched:
            threads.append({"name": thread["name"], "host_tid": thread["tid"],
                            "cpu_seconds": thread["cpu"], "dispatcher_samples": matched})
    def ranked(counter):
        return [{"value": k, "samples": v} for k, v in counter.most_common()]
    return {"totals": dict(totals), "threads": threads,
            "nearest_visible_lifted": [{"address": a, "samples": sum(c.values()),
                "self_symbols": ranked(c)} for a, c in sorted(nearest.items(),
                key=lambda item: -sum(item[1].values()))],
            "outermost_visible_lifted": [{"address": a, "samples": sum(c.values()),
                "nearest": ranked(c), "self_categories": dict(helper_roots[a])}
                for a, c in sorted(roots.items(), key=lambda item: -sum(item[1].values()))],
            "paths": [{"addresses": list(k), "samples": v} for k, v in paths.most_common()]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("capture", type=Path)
    parser.add_argument("--top", type=int, default=20)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--trace", action="store_true", help="parse correlated runtime file instead of sampled stacks")
    args = parser.parse_args()
    raw = args.capture.read_bytes()
    if args.trace:
        result = timeline(raw.decode().splitlines())
        result["sha256"] = hashlib.sha256(raw).hexdigest()
        if args.json:
            print(json.dumps(result, indent=2))
            return
        print(f"sha256={result['sha256']} joins={len(result['joins'])} rejected={result['rejected']}")
        ranked = defaultdict(lambda: [0, 0, 0])
        for row in result["joins"]:
            key = (row["last_target"], row["last_candidate_virtual"], row["last_vtable"])
            group = ranked[key]
            group[0] += 1
            group[1] += row["last_job_wait_overlap_ns"] or 0
            group[2] += row["after_last_completion_ns"]
        print("Last-completing target / candidate virtual / vtable; joins, last-job overlap ms, after-last ms")
        for key, group in sorted(ranked.items(), key=lambda kv: -kv[1][1])[:args.top]:
            print(" ".join(f"{v:08x}" for v in key), group[0], group[1] / 1e6, group[2] / 1e6)
        return
    data = json.loads(raw)
    result = analyze(data)
    result.update(sha256=hashlib.sha256(raw).hexdigest(), rounds=data["rounds"], wall=data["wall"])
    if args.json:
        print(json.dumps(result, indent=2))
        return
    print(f"sha256={result['sha256']} rounds={result['rounds']} wall={result['wall']:.6f}s")
    print(result["totals"])
    print("SAMPLE COUNTS ONLY; nearest visible function is not necessarily the leaf.")
    for key in ("outermost_visible_lifted", "nearest_visible_lifted"):
        print(key)
        for row in result[key][:args.top]:
            print(f"  {row['address']:08x} {row['samples']:5d}",
                  row.get("self_categories", row.get("self_symbols", [])[:4]))


if __name__ == "__main__":
    main()
