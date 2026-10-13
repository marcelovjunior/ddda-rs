"""L160: pair L158 scheduler signals with successful worker wait returns.

Reads private critical version=1 traces; never runs the game. Times include
runtime/dispatcher work and host scheduling, not just kernel wake latency.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import gzip
import json
from pathlib import Path
import re

SIGNAL = 0xDBE603
WAIT = 0xDBDCBC


def parse(lines):
    return list(iter_records(lines))


def iter_records(lines):
    """Validate incrementally, retaining no trace-sized string or row list."""
    header = False
    for line in lines:
        if line.strip() == "critical version=1":
            if header:
                raise ValueError("multiple process traces are not supported")
            header = True
            continue
        match = re.fullmatch(r"(Frame|Batch|End|Job|Join|Wake) \{ (.*) \}\s*", line)
        if not match:
            raise ValueError("malformed/truncated trace line")
        kind, fields = match.groups()
        row = {}
        for field in fields.split(", "):
            key, value = field.split(": ", 1)
            row[key] = value == "true" if value in ("true", "false") else int(value)
        yield kind, row
    if not header:
        raise ValueError("missing critical version=1 header")


def analyze_file(path, min_frame=0, max_frame=None, allow_truncated_tail=False):
    """Two passes preserve arbitrary cross-thread emission ordering.

    Only selected batch IDs and their records are retained. The source hash
    is checked on both passes so a growing/replaced capture is never reported.
    A bounded frame window is recommended for multi-GB captures.
    """
    if max_frame is not None and max_frame < min_frame:
        raise ValueError("max-frame must be >= min-frame")
    selected = set()
    hashes = []
    discarded = []
    opener = gzip.open if path.suffix.lower() == ".gz" else open
    def rows(index_only=False):
        digest = hashlib.sha256()
        with opener(path, "rb") as source:
            def lines():
                for line in source:
                    digest.update(line)
                    if allow_truncated_tail and not line.endswith(b"\n"):
                        # Binary iteration can yield an unterminated line only
                        # at EOF. Never accept its partial event or batch.
                        discarded.append(len(line))
                        continue
                    # The second pass validates every record. The index pass
                    # needs only batch boundaries and the process header.
                    if index_only and not line.startswith((b"Batch ", b"critical ")):
                        continue
                    yield line.decode("utf-8")
            yield from iter_records(lines())
        hashes.append(digest.hexdigest())
    for kind, row in rows(index_only=True):
        if kind == "Batch" and row["frame"] >= min_frame and (
                max_frame is None or row["frame"] <= max_frame):
            selected.add(row["id"])
    def window():
        for kind, row in rows():
            if kind == "Frame":
                continue
            ident = row["end_id"] if kind == "Wake" and row["site"] == WAIT else row["id"]
            if ident in selected:
                yield kind, row
    result = analyze(window(), min_frame)
    if hashes[0] != hashes[1]:
        raise ValueError("capture changed during analysis; wait for capture completion")
    result.update(source=str(path), source_sha256=hashes[0],
                  source_hash_kind="decompressed-trace",
                  min_frame=min_frame, max_frame=max_frame,
                  discarded_tail_bytes=discarded[0] if discarded else 0)
    return result


def distribution(values):
    values = sorted(values)
    if not values:
        return {"count": 0}
    def percentile(p):
        return values[max(0, (len(values) * p + 99) // 100 - 1)]
    return {"count": len(values), "mean_ns": sum(values) / len(values),
            "p50_ns": percentile(50), "p95_ns": percentile(95),
            "p99_ns": percentile(99), "max_ns": values[-1],
            "over_20us": sum(v > 20_000 for v in values)}


def batch_timeline(ident, batch, end, jobs, pairs, joins):
    """Callback boundaries bound visible work, not queue-claim or kernel times."""
    last_start = max((j["start"] for j in jobs), default=batch["ns"])
    ordered = sorted(jobs, key=lambda j: (j["end"], j["start"], j["tid"]))
    last = ordered[-1] if ordered else None
    # Portion of the last callback after every other callback completed.
    solo_start = max(last["start"], ordered[-2]["end"] if len(ordered) > 1
                     else last["start"]) if last else None
    gaps = []
    for pair in pairs:
        worker = sorted((j for j in jobs if j["tid"] == pair["tid"]),
                        key=lambda j: j["start"])
        cursor = pair["signal_start_ns"]
        for job in worker:
            stop = min(job["start"], last_start)
            if cursor < stop:
                gaps.append({"tid": pair["tid"], "start": cursor, "end": stop,
                             "ns": stop - cursor})
            cursor = max(cursor, job["end"])
        if cursor < last_start:
            gaps.append({"tid": pair["tid"], "start": cursor, "end": last_start,
                         "ns": last_start - cursor})
    participants = batch.get("participants")
    wake_end = max((p["signal_start_ns"] + p["signal_to_wait_ns"] for p in pairs),
                   default=batch["ns"])
    return {"batch": ident, "frame": batch["frame"], "queued": batch["queued"],
            "signaled_workers": len(pairs), "signaled_lanes_with_main": len(pairs) + 1,
            "participants": participants,
            "configured_participants": batch.get("configured"), "mode": batch.get("mode"),
            "workers_with_callbacks": sum(p["worker_jobs"] > 0 for p in pairs),
            "jobs_per_participant": batch["queued"] / participants if participants else None,
            "batch_ns": end["ns"] - batch["ns"],
            "last_job": last, "last_job_solo_tail_ns":
                last["end"] - solo_start if last else 0,
            "last_job_to_batch_end_ns": end["ns"] - last["end"] if last else None,
            "join_waits": [{**j, "duration_ns": j["end"] - j["start"],
                            "max_worker_signal_to_wait_ns": max(
                                (p["signal_to_wait_ns"] for p in pairs), default=0),
                            "until_last_worker_wait_return_ns": max(
                                0, min(j["end"], wake_end) - j["start"]),
                            "after_last_callback_ns": max(
                                0, j["end"] - max(j["start"], last["end"])) if last else j["end"] - j["start"],
                            "last_job_overlap_ns": max(0, min(j["end"], last["end"]) -
                                max(j["start"], solo_start)) if last else 0} for j in joins],
            "no_callback_gaps_while_callbacks_unstarted": gaps}


def join_summary(batches):
    """Overlapping timing observations, never an additive causal breakdown."""
    joins = [j for b in batches for j in b["join_waits"]]
    frames = len({b["frame"] for b in batches})
    keys = ("duration_ns", "max_worker_signal_to_wait_ns",
            "until_last_worker_wait_return_ns", "after_last_callback_ns",
            "last_job_overlap_ns")
    return {"accepted_frames": frames, "accepted_batches": len(batches),
            "joins": len(joins),
            "joins_per_frame": len(joins) / frames if frames else None,
            "metrics": {key: distribution([j[key] for j in joins]) for key in keys},
            "mean_ns_per_frame": {key: sum(j[key] for j in joins) / frames if frames else None
                                  for key in keys if key != "max_worker_signal_to_wait_ns"},
            "worker_callback_gaps": distribution([g["ns"] for b in batches
                for g in b["no_callback_gaps_while_callbacks_unstarted"]]),
            "interpretation": "Join intervals and callback/wake overlaps are observations, not additive costs. A late wake may overlap useful callbacks. After-last-callback includes dispatcher completion and join return, not just OS scheduling. Callback gaps are not proof of idle workers or unclaimed jobs."}


def analyze(records, min_frame=0):
    groups = defaultdict(lambda: defaultdict(list))
    rejected = Counter()
    for kind, row in records:
        if kind == "Frame":
            continue
        ident = row["end_id"] if kind == "Wake" and row["site"] == WAIT else row["id"]
        groups[ident][kind].append(row)
    pairs, batches = [], []
    for ident, group in sorted(groups.items()):
        if len(group["Batch"]) != 1 or len(group["End"]) != 1:
            rejected["missing/duplicate batch boundary"] += 1
            continue
        batch, end = group["Batch"][0], group["End"][0]
        if batch["frame"] < min_frame:
            continue
        if "participants" in batch and not 1 <= batch["participants"] <= 8:
            rejected["invalid active participant count"] += 1
            continue
        # Cumulative loss is deliberately conservative: no inference after
        # the first dropped record, even if a subsequent batch looks whole.
        if batch["nested"] or end["panicked"] or end["lost"]:
            rejected["nested/panicked/lossy batch"] += 1
            continue
        jobs = group["Job"]
        if len(jobs) != batch["queued"] or any(j["panicked"] or not
                batch["ns"] <= j["start"] <= j["end"] <= end["ns"] for j in jobs):
            rejected["incomplete/unordered callbacks"] += 1
            continue
        signals, waits = defaultdict(list), defaultdict(list)
        batch_pairs = []
        for wake in group["Wake"]:
            if wake["site"] == SIGNAL:
                signals[wake["handle"]].append(wake)
            elif wake["site"] == WAIT:
                waits[wake["handle"]].append(wake)
        for handle in signals.keys() | waits.keys():
            if len(signals[handle]) != 1 or len(waits[handle]) != 1:
                rejected["unpaired/duplicate handle"] += 1
                continue
            signal, wait = signals[handle][0], waits[handle][0]
            if signal["result"] == 0 or wait["result"] != 0:
                rejected["unsuccessful signal/wait"] += 1
                continue
            if signal["end_id"] != ident or not (
                    batch["ns"] <= signal["start"] <= signal["end"] <= end["ns"] and
                    wait["start"] <= wait["end"] and
                    signal["start"] <= wait["end"] <= end["ns"]):
                rejected["unordered/cross-batch pair"] += 1
                continue
            worker_jobs = [j for j in jobs if j["tid"] == wait["tid"]]
            first = min((j["start"] for j in worker_jobs), default=None)
            if first is not None and first < wait["end"]:
                rejected["job precedes successful wait return"] += 1
                continue
            batch_pairs.append({"batch": ident, "frame": batch["frame"], "handle": handle,
                "signal_start_ns": signal["start"],
                "tid": wait["tid"], "signal_to_wait_ns": wait["end"] - signal["start"],
                "signal_service_ns": signal["end"] - signal["start"],
                # Signed: the worker can wake before SetEvent returns.
                "signal_return_to_wait_ns": wait["end"] - signal["end"],
                "wait_to_first_job_ns": None if first is None else first - wait["end"],
                "signal_to_first_job_ns": None if first is None else first - signal["start"],
                "worker_jobs": len(worker_jobs)})
        batch_pairs.sort(key=lambda p: (p["tid"], p["handle"]))
        pairs.extend(batch_pairs)
        # A partial wake set cannot establish participant count or idle gaps.
        if len(batch_pairs) != len(signals) or len(signals) != len(waits):
            continue
        if len({p["tid"] for p in batch_pairs}) != len(batch_pairs):
            rejected["duplicate worker identity"] += 1
            continue
        joins = sorted(group["Join"], key=lambda j: j["start"])
        if any(j["result"] != 0 or not batch["ns"] <= j["start"] <= j["end"] <= end["ns"]
               for j in joins):
            rejected["unsuccessful/unordered join"] += 1
            continue
        batches.append(batch_timeline(ident, batch, end, jobs, batch_pairs, joins))
    metrics = ("signal_to_wait_ns", "signal_service_ns", "signal_return_to_wait_ns",
               "wait_to_first_job_ns", "signal_to_first_job_ns")
    return {"pairs": pairs, "batches": batches, "rejected": dict(rejected),
            "join_summary": join_summary(batches),
            "batch_summary": {
                "count": len(batches),
                "queued_histogram": dict(sorted(Counter(b["queued"] for b in batches).items())),
                "participants_histogram": dict(sorted(Counter(
                    str(b["participants"]) for b in batches).items())),
                "batches_with_jobs_at_least_participants": sum(
                    b["participants"] is not None and b["queued"] >= b["participants"]
                    for b in batches),
                "batches_with_at_least_eight_jobs": sum(b["queued"] >= 8 for b in batches),
                "last_job_solo_tail": distribution([b["last_job_solo_tail_ns"] for b in batches]),
                "last_job_to_batch_end": distribution([b["last_job_to_batch_end_ns"] for b in batches
                                                       if b["last_job_to_batch_end_ns"] is not None]),
                "callback_gaps": distribution([g["ns"] for b in batches
                    for g in b["no_callback_gaps_while_callbacks_unstarted"]]),
            },
            "gap_definition": "No callback executing on worker after its signal while some callback has not started. Includes wake/dispatch/claim overhead; not proof worker was descheduled or queue remained unclaimed. Gaps overlap across workers; never sum as frame time.",
            "summary": {key: distribution([r[key] for r in pairs if r[key] is not None])
                        for key in metrics},
            "per_thread": {str(tid): distribution([r["signal_to_wait_ns"] for r in pairs
                                                   if r["tid"] == tid])
                           for tid in sorted({r["tid"] for r in pairs})}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--min-frame", type=int, default=0)
    parser.add_argument("--max-frame", type=int)
    parser.add_argument("--allow-truncated-tail", action="store_true",
                        help="discard only an unterminated EOF fragment; incomplete batches still fail")
    parser.add_argument("--summary-only", action="store_true",
                        help="omit individual pairs/batches from JSON; preserve aggregates")
    args = parser.parse_args()
    result = analyze_file(args.trace, args.min_frame, args.max_frame, args.allow_truncated_tail)
    if args.summary_only:
        del result["pairs"], result["batches"]
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
