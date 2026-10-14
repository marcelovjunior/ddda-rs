#!/usr/bin/env python3
"""Offline ddplay report (stdlib only; never starts or attaches to a process).

  python scripts/analyze_run.py RUN PROFILE [GPU] [--locks LOCK_STATS]
  python scripts/analyze_run.py BEFORE PROFILE [GPU] --compare AFTER \
      --after-profile PROFILE [--after-gpu GPU] [--after-locks LOCK_STATS]

Stage timestamps must use the rate-log epoch. --stage-offset adds seconds to
stage times; --after-stage-offset does the same for the comparison. Without
events, report 20 s windows. Intervals are assigned to their endpoint stage or
window (boundary intervals can mix stages). FPS is frames / estimated elapsed,
using cumulative frame deltas and printed interval FPS, not rounded timestamps.
The first sample is omitted: a partial log cannot recover its frame delta.
Missing lines, zero-rounded rates and counter resets are excluded and reported.
Profiles and GPU files are whole-capture summaries: ensure they cover the same
phase yourself. Inclusive percentages overlap and cannot be summed into a
subsystem total. Thread/bound selection is a heuristic, not a dependency trace.
"""

import argparse
import csv
from dataclasses import dataclass, field
import math
from pathlib import Path
import re
import statistics

NUM = r"\d+(?:\.\d+)?"
RATE = re.compile(rf"ddplay rate t=({NUM})s frames=(\d+)\s+({NUM}) FPS")
STAGE = re.compile(rf"ddruntime stage:\s*(.+?)\s+t=({NUM})s?\b")
THREAD = re.compile(rf"== thread (\d+)\s+['\"](.*?)['\"]: cpu {NUM} s of {NUM} s "
                    rf"\(({NUM})%\), busy (\d+)/(\d+) samples")
WAIT = re.compile(rf"== WAITS thread (\d+)\s+['\"](.*?)['\"]: waiting ({NUM})% of (\d+) samples")
COST = re.compile(rf"\s*({NUM})%\s+(.+)")
LOCK = re.compile(rf"\s*(\d+)\s+({NUM})\s+({NUM})\s+({NUM})\s+"
                  rf"({NUM})\s+({NUM})\s+({NUM})\s+(\S.*)")
GROUPS = ('d3d9draw', 'ddgpu/wgpu', 'threads/locks', 'lifted game code', 'audio', 'allocations', 'other')


def read_text(path):
    data = Path(path).read_bytes()
    encoding = 'utf-16' if data.startswith((b'\xff\xfe', b'\xfe\xff')) else 'utf-8-sig'
    return data.decode(encoding, errors='replace')


@dataclass
class Bucket:
    frames: int = 0
    seconds: float = 0
    samples: int = 0

    @property
    def fps(self):
        return self.frames / self.seconds if self.seconds else None


@dataclass
class Run:
    buckets: dict = field(default_factory=dict)
    markers: list = field(default_factory=list)
    stages: list = field(default_factory=list)
    excluded: int = 0
    rates: int = 0


def parse_run(text, stage_offset=0):
    run = Run()
    lines = text.splitlines()
    for line in lines:
        if m := STAGE.search(line):
            run.stages.append((float(m[2]) + stage_offset, m[1].strip()))
    run.stages.sort(key=lambda event: event[0])
    previous = None
    pending_markers = []
    for line in lines:
        if 'ddplay marker:' in line:
            pending_markers.append((line.split('ddplay marker:', 1)[1].strip(),
                                    previous[0] if previous else None))
        m = RATE.search(line)
        if not m:
            continue
        t, frames, fps = float(m[1]), int(m[2]), float(m[3])
        run.rates += 1
        for name, start in pending_markers:
            run.markers.append((name, start, t))
        pending_markers.clear()
        label = f'{int(t // 20) * 20}-{int(t // 20) * 20 + 20}s'
        if run.stages:
            label = 'before first stage'
            for index, (when, name) in enumerate(run.stages):
                if when > t:
                    break
                label = f'{name} #{sum(n == name for _, n in run.stages[:index + 1])}'
        bucket = run.buckets.setdefault(label, Bucket())
        if previous:
            old_t, old_frames = previous
            delta = frames - old_frames
            elapsed = delta / fps if fps else 0
            # Rounded timestamps differ by up to a second. A larger discrepancy
            # indicates missing rate rows; the last FPS cannot describe that gap.
            if delta > 0 and t >= old_t and elapsed > 0 and abs(elapsed - (t - old_t)) <= 1.1 + elapsed * .02:
                bucket.frames += delta
                bucket.seconds += elapsed
                bucket.samples += 1
            else:
                run.excluded += 1
        else:
            run.excluded += 1
        previous = t, frames
    run.markers.extend((name, start, None) for name, start in pending_markers)
    return run


@dataclass
class Thread:
    tid: str
    name: str
    cpu: float = 0
    busy: float | None = None
    wait: float | None = None
    inclusive: list = field(default_factory=list)
    leaf: list = field(default_factory=list)
    waits: list = field(default_factory=list)


def parse_profile(text):
    threads = {}
    current, section = None, None
    for line in text.splitlines():
        if m := THREAD.search(line):
            current = threads.setdefault(m[1], Thread(m[1], m[2]))
            current.cpu = float(m[3])
            total = int(m[5])
            current.busy = 100 * int(m[4]) / total if total else None
            if current.wait is None and total:
                current.wait = 100 - current.busy
            section = None
        elif m := WAIT.search(line):
            current = threads.setdefault(m[1], Thread(m[1], m[2]))
            current.wait = float(m[3])
            section = 'waits'
        elif line.lstrip().startswith('=='):
            current, section = None, None
        elif line.strip().startswith('--'):
            section = line.strip()[2:].strip()
        elif current and (m := COST.fullmatch(line)):
            cost = float(m[1]), m[2]
            if section == 'inclusive':
                current.inclusive.append(cost)
            elif section == 'self':
                current.leaf.append(cost)
            elif section == 'waits' and m[2].startswith('in '):
                current.waits.append((cost[0], cost[1][3:]))
    return list(threads.values())


def subsystem(symbol):
    """Recognize paths/identifiers even in raw v0 names or truncated generics.

    Explicit lock operations beat generic context, then graphics, audio, lifted
    code. Generic instantiation context is necessarily approximate without stacks.
    """
    s = symbol.lower()
    if any(x in s for x in ('rtlallocateheap', 'rtlfreeheap', 'heapalloc', 'heapfree', '__rust_alloc', '__rust_dealloc')):
        return 'allocations'
    if any(x in s for x in ('guestlock', 'acquire_locked', 'signal_preempt', 'waitonaddress', 'entercriticalsection')):
        return 'threads/locks'
    if 'd3d9draw' in s:
        return 'd3d9draw'
    if any(x in s for x in ('ddgpu', 'wgpu', 'vkget', 'drv', 'vulkan')):
        return 'ddgpu/wgpu'
    if any(x in s for x in ('ddaudio', 'xaudio', 'audioengine')):
        return 'audio'
    if any(x in s for x in ('ddlift', 'ddnative', '::f_', '::s_')):
        return 'lifted game code'
    if any(x in s for x in ('threads', 'mutex', 'condvar', 'ntwait', 'sleep_for')):
        return 'threads/locks'
    return 'other'


def short_symbol(symbol):
    # Keep the actual symbol: do not pretend to fully demangle v0 backreferences.
    return symbol if len(symbol) <= 135 else symbol[:100] + ' ... ' + symbol[-30:]


def cost_entries(thread, group):
    # These stack ancestors wrap the thread/service lifetime or simply release
    # the guest lock around backend work. Their inclusive counts hide the work
    # below them; this is a display filter, not subtraction or attribution.
    wrappers = ('run_thread', 'create_thread', 'thread_start', 'ThreadStart',
                'BaseThreadInitThunk', 'backtrace', 'spawn_unchecked', 'call_ext',
                'backend_unlocked', 'threads::unlocked', 'threads8unlocked',
                'Registry::call', 'Registry4call', '::install::closure')
    return sorted(((p, s) for p, s in thread.inclusive
                   if subsystem(s) == group and not any(w in s for w in wrappers)), reverse=True)


def parse_locks(text):
    """Use each service's latest cumulative row; never add repeated snapshots."""
    rows, active, stamp = {}, False, None
    for line in text.splitlines():
        if m := re.search(rf'lock stats at ({NUM}) s:', line):
            stamp, active = float(m[1]), False
        elif 'guest lock by service:' in line:
            active = True
        elif active and (m := LOCK.fullmatch(line)):
            rows[m[8]] = (float(m[5]), int(m[1]), stamp)
        elif not line.strip():
            active = False
    return sorted(((wait, name, calls, stamp) for name, (wait, calls, stamp) in rows.items()), reverse=True)


def parse_gpu(text):
    values = []
    for row in csv.reader(text.splitlines()):
        if len(row) < 2 or not re.match(r'\d{4}[-/]\d{2}[-/]\d{2}', row[0].strip()):
            continue
        try:
            value = float(row[1].strip().rstrip('%').strip())
        except ValueError:
            continue
        if math.isfinite(value) and 0 <= value <= 100:
            values.append(value)
    return values


def candidate(threads):
    # CPU time is measured over wall time; busy samples are activity-filtered.
    return max((t for t in threads if t.busy is not None), key=lambda t: t.cpu, default=None)


def likely_bound(thread, gpu):
    if not thread:
        return 'undetermined (no complete thread profile)'
    utilization = statistics.mean(gpu) if gpu else None
    if utilization is not None and utilization >= 90:
        return 'GPU or mixed CPU/GPU (whole-capture GPU mean >=90%; heuristic)'
    if thread.cpu < 50:
        return 'wait/pacing or unsampled work (no thread >=50% CPU; heuristic)'
    graphics = max((p for p, s in thread.inclusive if subsystem(s) in ('d3d9draw', 'ddgpu/wgpu')), default=0)
    work = 'graphics translation' if graphics >= 20 else 'thread execution'
    confidence = 'GPU unmeasured' if utilization is None else f'GPU mean {utilization:.1f}%'
    return f'CPU {work} on {thread.name or thread.tid} ({thread.cpu:.0f}% CPU; {confidence}; heuristic)'


@dataclass
class Analysis:
    run: Run
    threads: list
    gpu: list
    locks: list


def analyze(run, profile, gpu=None, locks=None, stage_offset=0):
    log = read_text(run)
    return Analysis(parse_run(log, stage_offset), parse_profile(read_text(profile)),
                    parse_gpu(read_text(gpu)) if gpu else [],
                    parse_locks(read_text(locks) if locks else log))


def report(a, name):
    lines = [f'Run: {name}', 'FPS by stage' if a.run.stages else 'FPS by 20 s window']
    for label, b in a.run.buckets.items():
        value = f'{b.fps:.2f} FPS, {b.frames} frames / ~{b.seconds:.1f}s ({b.samples} intervals)' if b.fps is not None else 'no usable intervals'
        lines.append(f'  {label}: {value}')
    lines.append(f'  {a.run.rates} rate rows; {a.run.excluded} excluded (first/gap/reset/zero); endpoint assignment, rounded FPS.')
    if a.run.stages:
        lines.append('  Stage times assumed aligned to rate epoch (use --stage-offset if needed).')
    for name, start, end in a.run.markers:
        lines.append(f'  marker {name}: between rate t={start if start is not None else "?"} and {end if end is not None else "?"}s')
    thread = candidate(a.threads)
    if thread:
        lines.append(f'Frame-bound candidate: {thread.name!r} (host {thread.tid}), CPU {thread.cpu:.0f}%, busy {thread.busy:.1f}%, wait {thread.wait:.1f}%')
        lines.append('  Highest measured CPU thread; sampled waits are not proof of a frame dependency.')
        lines.append('Top inclusive costs (% of busy samples; overlaps, NOT additive; thread/dispatch/unlock wrappers omitted):')
        for group in GROUPS:
            costs = cost_entries(thread, group)[:4]
            lines.append(f'  {group}: ' + ('; '.join(f'{p:.1f}% {short_symbol(s)}' for p, s in costs) or 'not in captured top list'))
        for t in sorted(a.threads, key=lambda t: t.cpu, reverse=True)[:3]:
            if t.waits:
                lines.append(f'  Wait sites {t.name!r}: ' + '; '.join(f'{p:.1f}% {short_symbol(s)}' for p, s in sorted(t.waits, reverse=True)[:2]) + ' (% of waits)')
    else:
        lines.append('Frame-bound candidate: unavailable')
    lines.append('Top lock-waited services (latest cumulative row/service; all threads):')
    for wait, name, calls, stamp in a.locks[:5]:
        lines.append(f'  {wait:.1f} ms, {calls} calls: {name} (t={stamp if stamp is not None else "?"}s)')
    if not a.locks:
        lines.append('  unavailable (use --locks for a separate stats capture)')
    if a.gpu:
        lines.append(f'GPU busy: mean {statistics.mean(a.gpu):.1f}%, range {min(a.gpu):.0f}-{max(a.gpu):.0f}%, {len(a.gpu)} samples (whole file, unaligned; includes other processes)')
    else:
        lines.append('GPU busy: unavailable')
    lines.append('The likely bound: ' + likely_bound(thread, a.gpu))
    return '\n'.join(lines)


def comparison(before, after):
    lines = ['Comparison (after - before; match stages/occurrences or elapsed windows):']
    for label in dict.fromkeys([*before.run.buckets, *after.run.buckets]):
        b, a = before.run.buckets.get(label), after.run.buckets.get(label)
        if b and a and b.fps is not None and a.fps is not None:
            lines.append(f'  {label}: {b.fps:.2f} -> {a.fps:.2f} FPS ({a.fps - b.fps:+.2f}, {(a.fps / b.fps - 1) * 100:+.1f}%)')
        else:
            lines.append(f'  {label}: unmatched/no usable intervals')
    b, a = candidate(before.threads), candidate(after.threads)
    if b and a:
        lines.append(f'  Candidate: {b.name!r} -> {a.name!r}; CPU {a.cpu-b.cpu:+.1f} pp; busy {a.busy-b.busy:+.1f} pp; wait {a.wait-b.wait:+.1f} pp')
    if before.gpu and after.gpu:
        lines.append(f'  GPU mean: {statistics.mean(after.gpu)-statistics.mean(before.gpu):+.1f} pp')
    lines.append('  Compare equivalent capture phases; window labels alone do not establish equivalence.')
    return '\n'.join(lines)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('run')
    parser.add_argument('profile')
    parser.add_argument('gpu', nargs='?')
    parser.add_argument('--locks')
    parser.add_argument('--stage-offset', type=float, default=0)
    parser.add_argument('--compare', metavar='AFTER_RUN')
    parser.add_argument('--after-profile')
    parser.add_argument('--after-gpu')
    parser.add_argument('--after-locks')
    parser.add_argument('--after-stage-offset', type=float, default=0)
    args = parser.parse_args(argv)
    if bool(args.compare) != bool(args.after_profile):
        parser.error('--compare and --after-profile must be supplied together')
    if not args.compare and (args.after_gpu or args.after_locks or args.after_stage_offset):
        parser.error('--after-* requires --compare')
    try:
        before = analyze(args.run, args.profile, args.gpu, args.locks, args.stage_offset)
        print(report(before, args.run))
        if args.compare:
            after = analyze(args.compare, args.after_profile, args.after_gpu, args.after_locks, args.after_stage_offset)
            print('\n' + report(after, args.compare))
            print('\n' + comparison(before, after))
    except OSError as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
