"""Offline L162d attribution. Inputs/outputs contain private capture evidence.

Stack counts are not time measurements and cannot be joined to another run.
Trace mode consumes the complete-batch selection produced by job_wake.py.
"""
import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import json
from pathlib import Path

from critical_jobs import address, category
from job_wake import iter_records


def stacks(data):
    result = {}
    for lane, dispatcher in (("main", 0xDBD8D0), ("worker", 0xDBDC90)):
        counts = Counter()
        paths, leaves, helpers, inclusive = Counter(), Counter(), Counter(), Counter()
        top_symbols, services = Counter(), Counter()
        for thread in data["threads"]:
            for stack in thread["stacks"]:
                names = [data["symbols"].get(str(pc), f"?{pc:x}") for pc in stack]
                addrs = [address(n) for n in names]
                if dispatcher not in addrs:
                    continue
                counts["dispatcher"] += 1
                chain = [a for a in addrs[:addrs.index(dispatcher)] if a is not None]
                if 0xDB7620 not in chain:
                    continue
                counts["db7620"] += 1
                # Require an actual visible shadow target, not just generic DB7620.
                if 0xEE2C90 not in chain:
                    continue
                counts["shadow"] += 1
                chain = chain[:chain.index(0xDB7620) + 1]
                paths[" -> ".join(f"{a:08X}" for a in reversed(chain))] += 1
                leaves[f"{chain[0]:08X}"] += 1
                inclusive.update(f"{a:08X}" for a in set(chain))
                helpers[category(names[0])] += 1
                top_symbols[names[0]] += 1
                if any("::run_service_as" in n for n in names[:addrs.index(0xDB7620)]):
                    services["visible_service_stack"] += 1
        result[lane] = dict(counts=counts, paths=paths.most_common(),
                            nearest=leaves.most_common(), inclusive=inclusive.most_common(),
                            helpers=helpers, top_symbols=top_symbols.most_common(), services=services)
    return result


def trace(path, selection):
    batches = {b["batch"]: b for b in selection["batches"]}
    digest = hashlib.sha256()
    counts, family, seen = Counter(), defaultdict(list), Counter()
    opener = gzip.open if path.suffix == ".gz" else open
    with opener(path, "rb") as source:
        for line in source:
            digest.update(line)
            if not line.startswith(b"Job ") or not line.endswith(b"\n"):
                continue
            kind, job = next(iter_records(["critical version=1", line.decode()]))
            if job["id"] not in batches:
                continue
            seen[job["id"]] += 1
            if job["target"] != 0xDB7620 or job["resolved"] != 0xEE2C90:
                continue
            lane = "main" if job["tid"] == 0x1000 else "worker"
            key = f"{job['vtable']:08X}/{lane}/{job['tid']:04X}"
            family[key].append(job["end"] - job["start"])
            counts[f"object/{job['object']:08X}"] += 1
    if digest.hexdigest() != selection["source_sha256"]:
        raise ValueError("trace does not match validated selection hash")
    if any(seen[i] != b["queued"] for i, b in batches.items()):
        raise ValueError("selected callback counts do not match validated batches")
    tails = defaultdict(lambda: dict(count=0, solo_ns=0, duration_ns=0))
    for b in batches.values():
        j = b["last_job"]
        if not j:
            continue
        key = f"{j['target']:08X}/{j['resolved']:08X}/{j['vtable']:08X}/" + (
            "main" if j["tid"] == 0x1000 else "worker")
        tails[key]["count"] += 1
        tails[key]["solo_ns"] += b["last_job_solo_tail_ns"]
        tails[key]["duration_ns"] += j["end"] - j["start"]
    return dict(source_sha256=digest.hexdigest(), batches=len(batches),
                frames=len({b["frame"] for b in batches.values()}), objects=counts,
                family={k: dict(count=len(v), total_ns=sum(v), min_ns=min(v), max_ns=max(v))
                        for k, v in sorted(family.items())},
                tails=sorted(tails.items(), key=lambda kv: kv[1]["solo_ns"], reverse=True))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("--selection", type=Path)
    args = parser.parse_args()
    if args.selection:
        result = trace(args.source, json.loads(args.selection.read_text()))
    else:
        result = stacks(json.loads(args.source.read_text()))
        result["source_sha256"] = hashlib.sha256(args.source.read_bytes()).hexdigest()
    print(json.dumps(result, indent=2))
