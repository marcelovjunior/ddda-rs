"""Validate and summarize the bounded, alternating L161 CPU measurements."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path
import re
from statistics import median

ROW = re.compile(r'l161-(cost|hash) (?:addr=(\w+) case=(\d+)|length=(\d+) offset=(\d+)) '
                 r'round=(\d+) cached=([01]) ns=([\d.]+)')


def summarize(path):
    text = path.read_text()
    if not all(f'test test_{variant}::every_lifted_function_matches_the_original ... ok' in text
               for variant in ('baseline', 'candidate')):
        raise ValueError('missing successful original-vector baseline/candidate replay')
    if 'test result: ok. 2 passed; 0 failed; 0 ignored' not in text:
        raise ValueError('missing completed two-benchmark result')
    pairs = defaultdict(dict)
    for m in ROW.finditer(text):
        key = (m[1], m[2] or m[4], m[3] or m[5], int(m[6]))
        variant = int(m[7])
        if variant in pairs[key]:
            raise ValueError(f'duplicate measurement: {key}/{variant}')
        pairs[key][variant] = float(m[8])
    groups = defaultdict(list)
    rounds = defaultdict(set)
    if not pairs:
        raise ValueError('no measurement rows')
    for (kind, first, second, round), values in pairs.items():
        if set(values) != {0, 1} or min(values.values()) <= 0:
            raise ValueError('incomplete or nonpositive pair')
        group = (kind, first, second if kind == 'hash' else '')
        groups[group].append(values)
        rounds[(kind, first, second)].add(round)
    if any(value != set(range(7)) for value in rounds.values()):
        raise ValueError('each fixture must have all seven paired rounds')
    result = []
    for key, values in sorted(groups.items()):
        result.append(dict(kind=key[0], address_or_length=key[1], offset=key[2], pairs=len(values),
            improved=sum(v[0] > v[1] for v in values),
            baseline_ns=median(v[0] for v in values), candidate_ns=median(v[1] for v in values),
            median_paired_speedup=median(v[0] / v[1] for v in values)))
    return dict(path=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest(), groups=result)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('logs', nargs='+', type=Path)
    args = parser.parse_args()
    print(json.dumps([summarize(path) for path in args.logs], indent=2))
