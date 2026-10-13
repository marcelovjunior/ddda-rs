"""Summarize DDGPU_PIPELINE events; phase boundaries are observed Present numbers.

Usage: python scripts/pipeline_summary.py run.log --phases 0:boot,100:title,...
Without --phases use labels recorded by DDGPU_PIPELINE_PHASES.
Times are summed CPU-call wall times, not elapsed loading time. Nested IR events
are separate rows and must not be added to enclosing shader-module totals.
"""
import argparse
import collections
import re


def summarize(lines, phases=()):
    rows = collections.defaultdict(lambda: [0, 0.0, 0.0])
    for line in lines:
        if "DDGPU_PIPELINE " not in line:
            continue
        fields = dict(re.findall(r"(\w+)=([^\s]+)", line))
        try:
            frame, ms = int(fields["frame"]), float(fields["ms"])
            phase = fields["phase"]
            for start, label in sorted(phases):
                if start <= frame:
                    phase = label
            key = phase, fields["kind"], fields["cache"]
        except (KeyError, ValueError):
            continue
        row = rows[key]
        row[0] += 1
        row[1] += ms
        row[2] = max(row[2], ms)
    return rows


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("log")
    parser.add_argument("--phases", default="")
    args = parser.parse_args()
    phases = [(int(f), p) for f, p in (s.split(":") for s in args.phases.split(",") if s)]
    with open(args.log, encoding="utf-8", errors="replace") as source:
        rows = summarize(source, phases)
    print("phase,kind,cache,count,total_ms,max_ms")
    for (phase, kind, cache), (count, total, maximum) in sorted(rows.items()):
        print(f"{phase},{kind},{cache},{count},{total:.3f},{maximum:.3f}")
