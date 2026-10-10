"""Summarize an opt-in ddview Bevy Chrome trace (private capture stays private).

Build: cargo build --release -p ddview --features profile-trace
Capture: set TRACE_CHROME to a private path and DDVIEW_PERF_SECONDS to a duration.
Analyze: python scripts/perf_trace.py capture.json --start 30 --end 45

Times are CPU wall time inside spans, not GPU timings. Threads overlap, and
inclusive parent/child times overlap: neither column may be summed as frame time.
The default frame span `update` is Bevy's main-thread SubApps::update, including
render-world handoff/extraction. Use an untraced build for acceptance frame times.
"""

import argparse
from collections import defaultdict
from dataclasses import dataclass, field
import json
from pathlib import Path


@dataclass
class Span:
    name: str
    thread: int
    start: float
    end: float
    children: list = field(default_factory=list)


def spans(events):
    """Pair Chrome threaded B/E records, rejecting broken nesting/captures."""
    stacks = defaultdict(list)
    complete = []
    for event in events:
        phase = event.get("ph")
        if phase not in ("B", "E", "X"):
            continue
        thread = (event.get("pid", 0), event.get("tid", 0))
        name = event.get("name", "")
        if phase == "B":
            stacks[thread].append(Span(name, thread, event["ts"], 0.0))
        elif phase == "E":
            if not stacks[thread]:
                raise ValueError(f"unmatched end on thread {thread}: {name}")
            span = stacks[thread].pop()
            if name and span.name != name:
                raise ValueError(f"mismatched end: {span.name!r} / {name!r}")
            span.end = event["ts"]
            if span.end < span.start:
                raise ValueError("negative span duration")
            complete.append(span)
            if stacks[thread]:
                stacks[thread][-1].children.append(span)
        else:
            complete.append(Span(name, thread, event["ts"], event["ts"] + event["dur"]))
    if any(stacks.values()):
        raise ValueError("unfinished spans: capture must exit normally and flush")
    return complete


def overlap(span, start, end):
    return max(0.0, min(span.end, end) - max(span.start, start))


def percentile(values, fraction):
    values = sorted(values)
    return values[int((len(values) - 1) * fraction)] if values else 0.0


def summarize(records, start, end, frame_span="update"):
    """Window is in microseconds. Group by both thread and span name."""
    if end <= start:
        raise ValueError("end must follow start")
    grouped = defaultdict(lambda: {"inclusive_us": 0.0, "exclusive_us": 0.0, "durations_us": []})
    frames = [s.end - s.start for s in records
              if s.name == frame_span and s.start >= start and s.end <= end]
    for span in records:
        duration = overlap(span, start, end)
        if duration == 0:
            continue
        group = grouped[(span.thread, span.name)]
        group["inclusive_us"] += duration
        # Direct children are disjoint for the threaded B/E spans Bevy emits.
        group["exclusive_us"] += max(0.0, duration - sum(overlap(c, start, end) for c in span.children))
        if span.start >= start and span.end <= end:
            group["durations_us"].append(span.end - span.start)
    rows = []
    for (thread, name), group in grouped.items():
        durations = group["durations_us"]
        rows.append({
            "thread": thread, "name": name, "runs": len(durations),
            "inclusive_ms": group["inclusive_us"] / 1000,
            "exclusive_ms": group["exclusive_us"] / 1000,
            "inclusive_ms_per_frame": group["inclusive_us"] / (1000 * len(frames)) if frames else None,
            "exclusive_ms_per_frame": group["exclusive_us"] / (1000 * len(frames)) if frames else None,
            "median_ms": percentile(durations, 0.5) / 1000,
            "p95_ms": percentile(durations, 0.95) / 1000,
            "worst_ms": max(durations, default=0) / 1000,
        })
    rows.sort(key=lambda row: row["inclusive_ms"], reverse=True)
    return {"window_seconds": [start / 1e6, end / 1e6], "frame_span": frame_span,
            "frames": len(frames), "frame_median_ms": percentile(frames, 0.5) / 1000,
            "frame_p95_ms": percentile(frames, 0.95) / 1000, "spans": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--start", type=float, default=0, help="seconds from trace start")
    parser.add_argument("--end", type=float, required=True, help="seconds from trace start")
    parser.add_argument("--frame-span", default="update")
    parser.add_argument("--limit", type=int, default=40)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    data = json.loads(args.trace.read_text(encoding="utf-8"))
    result = summarize(spans(data if isinstance(data, list) else data["traceEvents"]),
                       args.start * 1e6, args.end * 1e6, args.frame_span)
    if args.json:
        print(json.dumps(result, indent=2))
        return
    print(f"{result['frames']} complete {args.frame_span!r} frames in {args.start:g}..{args.end:g}s; "
          f"median {result['frame_median_ms']:.3f}ms, p95 {result['frame_p95_ms']:.3f}ms")
    print("CPU wall time; overlapping threads/parent spans MUST NOT be summed. Trace adds overhead.")
    print("thread     runs    avg/frame inclusive/exclusive ms    span")
    for row in result["spans"][:args.limit]:
        inclusive = row["inclusive_ms_per_frame"]
        exclusive = row["exclusive_ms_per_frame"]
        times = f"{inclusive:8.3f} / {exclusive:8.3f}" if inclusive is not None else "no complete frame denominator"
        print(f"{str(row['thread']):10} {row['runs']:6} {times:30} {row['name']}")


if __name__ == "__main__":
    main()
