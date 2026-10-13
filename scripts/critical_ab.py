"""Stream private scheduler traces into complete-join per-frame/job rankings.

Callback durations overlap across workers: their sum is work, not frame time.
The candidate virtual target is diagnostic identity, not proven dispatch.
"""
import argparse
from collections import Counter, defaultdict
import hashlib
import gzip
import json
from pathlib import Path
import re

from critical_jobs import join_metrics

LINE = re.compile(rb"(Frame|Batch|End|Job|Join|Wake) \{ (.*) \}\r?\n")


def family(job):
    return '/'.join(f'{job[k]:08x}' for k in ('target', 'resolved', 'vtable'))


def summarize(stream, first, last):
    digest = hashlib.sha256()
    active = {}
    frames = defaultdict(Counter)
    ranks = defaultdict(Counter)
    frame_ranks = defaultdict(lambda: defaultdict(Counter))
    rejected = Counter()
    header = False
    truncated = 0
    for line in stream:
        digest.update(line)
        if not line.endswith(b'\n'):
            truncated += len(line)
            continue
        if line.strip() == b'critical version=1':
            if header:
                raise ValueError('duplicate header')
            header = True
            continue
        match = LINE.fullmatch(line)
        if not header or not match:
            raise ValueError('malformed interior trace')
        kind, fields = match.groups()
        # Wake records are not used to assign callback or join costs.
        if kind in (b'Frame', b'Wake'):
            continue
        record = {}
        for field in fields.split(b', '):
            key, value = field.split(b': ', 1)
            record[key.decode()] = value == b'true' if value in (b'true', b'false') else int(value)
        ident = record['id']
        if kind == b'Batch':
            if first <= record['frame'] <= last:
                if ident in active:
                    raise ValueError('duplicate batch')
                active[ident] = (record, [], [])
        elif ident in active:
            batch, jobs, joins = active[ident]
            if kind == b'Job':
                jobs.append(record)
            elif kind == b'Join':
                joins.append(record)
            elif kind == b'End':
                del active[ident]
                # This report deliberately excludes serial/empty batches.
                if not joins:
                    rejected['serial/empty batch'] += 1
                    continue
                try:
                    metric = join_metrics(batch, record, jobs, joins)
                except (ValueError, KeyError) as error:
                    rejected[str(error)] += 1
                    continue
                frame = frames[batch['frame']]
                frame['joins'] += 1
                frame['join_ns'] += metric['join_ns']
                frame['after_last_ns'] += metric['after_last_completion_ns']
                ordered = sorted(jobs, key=lambda j: j['end'])
                final = ordered[-1]
                # Exclude the gap before this callback even if every other
                # callback had already completed before it started.
                tail = max(0, final['end'] - max(final['start'], joins[0]['start'],
                           ordered[-2]['end'] if len(ordered) > 1 else final['start']))
                frame['solo_tail_ns'] += tail
                for job in jobs:
                    for rank in (ranks[family(job)], frame_ranks[batch['frame']][family(job)]):
                        rank['calls'] += 1
                        rank['work_ns'] += job['end'] - job['start']
                for rank in (ranks[family(final)], frame_ranks[batch['frame']][family(final)]):
                    rank['last_count'] += 1
                    rank['last_join_ns'] += metric['join_ns']
                    rank['solo_tail_ns'] += tail
                    rank['after_last_ns'] += metric['after_last_completion_ns']
    if not header:
        raise ValueError('missing header')
    rejected['unfinished batch'] += len(active)
    return dict(sha256=digest.hexdigest(), first=first, last=last,
                frames=dict(sorted(frames.items())), families=dict(sorted(ranks.items())),
                frame_families={f:dict(r) for f,r in sorted(frame_ranks.items())},
                rejected=dict(rejected), unterminated_eof_bytes=truncated)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('trace', type=Path)
    parser.add_argument('--first', type=int, required=True)
    parser.add_argument('--last', type=int, required=True)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    opener = gzip.open if args.trace.suffix == '.gz' else open
    with opener(args.trace, 'rb') as stream:
        result = summarize(stream, args.first, args.last)
    args.out.write_text(json.dumps(result, indent=2))
    print(f"{args.out}: {len(result['frames'])} frames sha256={result['sha256']} rejected={result['rejected']}")


if __name__ == '__main__':
    main()
