#!/usr/bin/env python3
"""Summarize DDRUNTIME_LOAD_LOG=1 stderr without guessing UI states.

Head-observed phases may be supplied as --mark main_menu=42.5 (seconds
since process start). Draw counts alone cannot identify a menu or loading
screen. Movie times measure reader lifetime, not a fixed visible duration.
"""
import argparse
from dataclasses import dataclass
import math
from pathlib import Path
import re

LINE = re.compile(r"load-log t=([0-9.]+) event=(\w+)(.*)")
TOTALS = {"io_totals", "gpu_totals", "archive_totals", "frame_totals", "boot_cpu_totals", "allocator_totals", "zlib_entry_totals"}
FIELDS = re.compile(r"(\w+)=([^\s]+)")


@dataclass
class Event:
    time: float
    name: str
    fields: dict


def parse(text):
    events = []
    for line in text.splitlines():
        match = LINE.search(line)
        if not match:
            continue
        try:
            time = float(match[1])
        except ValueError:
            continue
        if math.isfinite(time) and time >= 0:
            events.append(Event(time, match[2], dict(FIELDS.findall(match[3]))))
    # Concurrent stderr writers can print later timestamps before earlier ones.
    return sorted(events, key=lambda e: e.time)


def add_mark(events, mark):
    name, value = mark.split("=", 1)
    time = float(value)
    if not re.fullmatch(r"\w+", name) or not math.isfinite(time) or time < 0:
        raise ValueError("marks require name=nonnegative_seconds")
    events.append(Event(time, name, {"source": "head_observation"}))
    events.sort(key=lambda e: e.time)


def phases(events):
    """Adjacent observed milestones; no inferred time zero for missing logs."""
    last = None
    for event in events:
        if event.name in TOTALS or event.name in {"stage_probe", "timing_unavailable"}:
            continue
        yield event, None if last is None else event.time - last
        last = event.time


def movie_spans(events):
    opened = {}
    result = []
    for event in events:
        key = (event.fields.get("reader"), event.fields.get("stream"))
        if event.name == "movie_start":
            opened[key] = event.time
        elif event.name == "movie_end":
            start = opened.pop(key, None)
            result.append((key, start, event.time))
    result.extend((key, start, None) for key, start in opened.items())
    return result


def render(events):
    if not events:
        return "No load-log records. Legacy stage-probe clocks have no process-start anchor.\n"
    out = ["| Phase | Wall time (s) |", "|---|---:|"]
    for label, begin, end in [
        ("Boot to title", "process_start", "title_screen"),
        ("Boot to main menu", "process_start", "main_menu"),
        ("Load Game to loading end", "load_game_pressed", "loading_end"),
        ("Loading screen", "loading_start", "loading_end"),
        ("Runtime initialization", "runtime_init_begin", "runtime_init_end"),
        ("GPU initialization", "gpu_init_begin", "gpu_init_end"),
    ]:
        start = next((e.time for e in events if e.name == begin), None)
        finish = next((e.time for e in events if e.name == end and start is not None and e.time >= start), None)
        duration = f"{finish - start:.3f}" if start is not None and finish is not None else "unmeasured"
        out.append(f"| {label} | {duration} |")
    out += ["", "First matching phase boundaries above; repeated movie lifetimes below.", "",
           "| Milestone | Since process start (s) | Since preceding milestone (s) | Evidence |",
           "|---|---:|---:|---|"]
    for event, delta in phases(events):
        elapsed = f"{delta:.3f}" if delta is not None else "--"
        out.append(f"| {event.name} | {event.time:.3f} | {elapsed} | {event.fields.get('source', 'runtime')} |")
    out += ["", "Movie reader lifetimes (includes decoding/waits; close may follow skip):"]
    for key, start, end in movie_spans(events):
        duration = f"{end - start:.3f} s" if start is not None and end is not None else "incomplete"
        out.append(f"- reader {key[0]}, stream {key[1]}: {duration}")
    out += ["", "Last cumulative samples (inclusive service/thread time; do not add to wall time):"]
    latest = {e.name: e for e in events if e.name in TOTALS}
    for name, event in latest.items():
        fields = []
        for key, value in event.fields.items():
            if key.endswith("_ns"):
                try:
                    fields.append(f"{key[:-3]}={int(value) / 1e9:.6f} s")
                except ValueError:
                    fields.append(f"{key}={value}")
            else:
                fields.append(f"{key}={value}")
        out.append(f"- {name} at {event.time:.3f} s: " + ", ".join(fields))
    out += ["", "Frame samples (draw-count changes are evidence, not semantic stage labels):",
            "| Sample end (s) | Interval (s) | Frames | Draws/frame |",
            "|---|---:|---:|---:|"]
    previous = None
    for event in events:
        if event.name != "frame_totals":
            continue
        try:
            current = (event.time, int(event.fields["draws"]), int(event.fields["presents"]))
        except (KeyError, ValueError):
            continue
        if previous:
            dt, draws, frames = (a - b for a, b in zip(current, previous))
            if dt > 0 and draws >= 0 and frames >= 0:
                per = f"{draws / frames:.1f}" if frames else "—"
                out.append(f"| {event.time:.3f} | {dt:.3f} | {frames} | {per} |")
        previous = current
    observed = {e.name for e in events}
    missing = [name for name in ("title_screen", "main_menu", "load_game_pressed", "loading_start", "loading_end") if name not in observed]
    if missing:
        out += ["", "Unmeasured UI milestones: " + ", ".join(missing) + ". Use head-observed --mark name=seconds."]
    return "\n".join(out) + "\n"


def render_legacy(text):
    """Keep old stage-probe logs useful without mixing their different clocks."""
    probes = re.findall(r"ddplay stage-probe t=([0-9.]+)s draws/frame=(\d+) fps=([0-9.]+)", text)
    out = ["Legacy stage probes: seconds since autoplay started, NOT process start.",
           "No reliable title/menu/Load Game/movie or service-time boundaries are present.",
           "", "| Probe time (s) | Draws/frame | FPS |", "|---:|---:|---:|"]
    for time, draws, fps in probes:
        out.append(f"| {time} | {draws} | {fps} |")
    return "\n".join(out) + "\n" if probes else render([])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log", type=Path)
    parser.add_argument("--mark", action="append", default=[])
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    text = args.log.read_text(encoding="utf-8-sig", errors="replace")
    events = parse(text)
    if args.mark and not events:
        parser.error("manual process-relative marks require load-log records; legacy clocks differ")
    if sum(e.name == "process_start" for e in events) > 1:
        parser.error("multiple process starts: split concatenated runs before comparison")
    for mark in args.mark:
        try:
            add_mark(events, mark)
        except ValueError as error:
            parser.error(str(error))
    report = render(events) if events else render_legacy(text)
    if args.output:
        args.output.write_text(report, encoding="utf-8")
    else:
        print(report, end="")


if __name__ == "__main__":
    main()
