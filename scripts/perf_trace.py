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
import sys


@dataclass
class Span:
    name: str
    thread: tuple
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
        # Bevy's log formatter appends ": " even for an empty field list.
        name = event.get("name", "").removesuffix(": ")
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
            span = Span(name, thread, event["ts"], event["ts"] + event["dur"])
            complete.append(span)
            if stacks[thread]:
                stacks[thread][-1].children.append(span)
    if any(stacks.values()):
        raise ValueError("unfinished spans: capture must exit normally and flush")
    return complete


def overlap(span, start, end):
    return max(0.0, min(span.end, end) - max(span.start, start))


def percentile(values, fraction):
    values = sorted(values)
    return values[int((len(values) - 1) * fraction)] if values else 0.0


def trace_events(path):
    """Stream tracing-chrome's array (one serialized event per line)."""
    with path.open(encoding="utf-8") as source:
        if source.readline().strip() != "[":
            raise ValueError("expected tracing-chrome array with one event per line")
        closed = False
        for line in source:
            value = line.strip().removesuffix(",")
            if value == "]":
                closed = True
                break
            if value:
                yield json.loads(value)
        if not closed:
            raise ValueError("truncated JSON trace: array was not closed")


def stream_summary(events, start, end, frame_span="update", frame_thread=None,
                   allow_open_scopes=False):
    """Aggregate without retaining raw events or completed span objects.

    Direct-child overlap is accumulated as a number on the active stack,
    so even long-lived app/thread scopes use bounded memory. An explicitly
    allowed terminal open scope is reported, never treated as a complete span.
    """
    if end <= start:
        raise ValueError("end must follow start")
    stacks = defaultdict(list)
    grouped = defaultdict(lambda: {"inclusive_us": 0.0, "exclusive_us": 0.0, "durations_us": []})
    frame_durations = defaultdict(list)
    thread_names = {}
    for event in events:
        phase = event.get("ph")
        thread = (event.get("pid", 0), event.get("tid", 0))
        if phase == "M" and event.get("name") == "thread_name":
            thread_names[thread] = event.get("args", {}).get("name", "")
        if phase not in ("B", "E", "X"):
            continue
        name = sys.intern(event.get("name", "").removesuffix(": "))
        if phase == "B":
            stacks[thread].append([name, event["ts"], 0.0])
            continue
        if phase == "E":
            if not stacks[thread]:
                raise ValueError(f"unmatched end on thread {thread}: {name}")
            span_name, span_start, children = stacks[thread].pop()
            if name and span_name != name:
                raise ValueError(f"mismatched end: {span_name!r} / {name!r}")
            span_end = event["ts"]
        else:
            span_name, span_start, children = name, event["ts"], 0.0
            span_end = span_start + event["dur"]
        if span_end < span_start:
            raise ValueError("negative span duration")
        duration = max(0.0, min(span_end, end) - max(span_start, start))
        if stacks[thread]:
            stacks[thread][-1][2] += duration
        if duration <= 0:
            continue
        group = grouped[(thread, span_name)]
        group["inclusive_us"] += duration
        group["exclusive_us"] += max(0.0, duration - children)
        if span_start >= start and span_end <= end:
            group["durations_us"].append(span_end - span_start)
            if span_name == frame_span:
                frame_durations[thread].append(span_end - span_start)
    open_scopes = [{"thread": thread, "name": item[0], "start_seconds": item[1] / 1e6}
                   for thread, stack in stacks.items() for item in stack]
    if open_scopes and not allow_open_scopes:
        raise ValueError(f"terminal open scopes: {open_scopes}; inspect normal exit, then --allow-open-scopes")
    frame_threads = sorted(frame_durations)
    if frame_thread is None and len(frame_threads) > 1:
        raise ValueError(f"frame span {frame_span!r} occurs on multiple threads {frame_threads}; select --frame-thread")
    frames = frame_durations.get(frame_thread, []) if frame_thread is not None else (
        frame_durations[frame_threads[0]] if frame_threads else [])
    rows = []
    for (thread, name), group in grouped.items():
        durations = group["durations_us"]
        rows.append({"thread": thread, "name": name, "runs": len(durations),
                     "inclusive_ms": group["inclusive_us"] / 1000,
                     "exclusive_ms": group["exclusive_us"] / 1000,
                     "inclusive_ms_per_frame": group["inclusive_us"] / (1000 * len(frames)) if frames else None,
                     "exclusive_ms_per_frame": group["exclusive_us"] / (1000 * len(frames)) if frames else None,
                     "median_ms": percentile(durations, 0.5) / 1000,
                     "p95_ms": percentile(durations, 0.95) / 1000,
                     "worst_ms": max(durations, default=0) / 1000})
    rows.sort(key=lambda row: row["inclusive_ms"], reverse=True)
    return {"window_seconds": [start / 1e6, end / 1e6], "frame_span": frame_span,
            "frame_threads": frame_threads, "selected_frame_thread": frame_thread,
            "frames": len(frames), "frame_median_ms": percentile(frames, 0.5) / 1000,
            "frame_p95_ms": percentile(frames, 0.95) / 1000, "spans": rows,
            "thread_names": [{"thread": thread, "name": name} for thread, name in thread_names.items()],
            "ignored_terminal_open_scopes": open_scopes}


def summarize(records, start, end, frame_span="update", frame_thread=None):
    """Window is in microseconds. Group by both thread and span name."""
    if end <= start:
        raise ValueError("end must follow start")
    grouped = defaultdict(lambda: {"inclusive_us": 0.0, "exclusive_us": 0.0, "durations_us": []})
    frame_records = [s for s in records
                     if s.name == frame_span and s.start >= start and s.end <= end]
    frame_threads = sorted({s.thread for s in frame_records})
    if frame_thread is None and len(frame_threads) > 1:
        raise ValueError(f"frame span {frame_span!r} occurs on multiple threads {frame_threads}; select --frame-thread")
    frames = [s.end - s.start for s in frame_records
              if frame_thread is None or s.thread == frame_thread]
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
            "frame_threads": frame_threads, "selected_frame_thread": frame_thread,
            "frames": len(frames), "frame_median_ms": percentile(frames, 0.5) / 1000,
            "frame_p95_ms": percentile(frames, 0.95) / 1000, "spans": rows}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--start", type=float, default=0, help="seconds from trace start")
    parser.add_argument("--end", type=float, required=True, help="seconds from trace start")
    parser.add_argument("--frame-span", default="update")
    parser.add_argument("--frame-thread", type=int, help="explicit main-thread Chrome tid")
    parser.add_argument("--frame-pid", type=int, default=1, help="Chrome pid for explicit thread selection")
    parser.add_argument("--allow-open-scopes", action="store_true",
                        help="report and omit terminal open scopes after verifying a normal process exit")
    parser.add_argument("--limit", type=int, default=40)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    result = stream_summary(trace_events(args.trace), args.start * 1e6, args.end * 1e6,
                            args.frame_span,
                            (args.frame_pid, args.frame_thread) if args.frame_thread is not None else None,
                            args.allow_open_scopes)
    if args.json:
        print(json.dumps(result, indent=2))
        return
    print(f"{result['frames']} complete {args.frame_span!r} frames in {args.start:g}..{args.end:g}s; "
          f"median {result['frame_median_ms']:.3f}ms, p95 {result['frame_p95_ms']:.3f}ms")
    print("CPU wall time; overlapping threads/parent spans MUST NOT be summed. Trace adds overhead.")
    if result["ignored_terminal_open_scopes"]:
        print("Omitted terminal open scopes:", result["ignored_terminal_open_scopes"])
    print("thread     runs    avg/frame inclusive/exclusive ms    span")
    for row in result["spans"][:args.limit]:
        inclusive = row["inclusive_ms_per_frame"]
        exclusive = row["exclusive_ms_per_frame"]
        times = f"{inclusive:8.3f} / {exclusive:8.3f}" if inclusive is not None else "no complete frame denominator"
        print(f"{str(row['thread']):10} {row['runs']:6} {times:30} {row['name']}")


if __name__ == "__main__":
    main()
