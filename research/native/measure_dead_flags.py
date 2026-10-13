"""Static flag-site counts from the player's private lift cache (no builds).

Usage: python measure_dead_flags.py [--profile FILE ...] [--all]
Reports counts only; never copies cached game bodies into the repository.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re

import lean_emission as le
import lift


def records(path):
    """Stream a JSON array without retaining the 390 MB cache in memory."""
    decoder = json.JSONDecoder()
    with open(path, encoding='utf-8') as source:
        buffer = ''
        eof = False
        while True:
            buffer = buffer.lstrip(' \r\n\t[,')
            if buffer.startswith(']'):
                return
            try:
                value, end = decoder.raw_decode(buffer)
            except json.JSONDecodeError:
                if eof:
                    raise
                chunk = source.read(1024 * 1024)
                eof = not chunk
                buffer += chunk
                continue
            yield value
            buffer = buffer[end:]


def counts(arms):
    out = Counter()
    for arm in arms:
        events = le._scan(arm)
        out['writes'] += sum(e[0] in ('kill', 'write') for e in events)
        out['conditions'] += len(le._condition_sites(arm))
        out['bytes'] += len(arm.encode())
        out['fused'] += arm.count('let l129_condition =')
    return out


def measure(function):
    previous = lift.LEAN
    lift.LEAN = False
    try:
        raw = [lift.cpu_arm(a) for a in function['arms']]
    finally:
        lift.LEAN = previous
    local = [le.lean(a) for a in raw]
    flow = le.function_flags(local)
    before, current, after = map(counts, (raw, local, flow))
    return dict(addr=f"0x{function['addr']:08X}", blocks=len(raw),
                writes=before['writes'], local_dead=before['writes']-current['writes'],
                additional_removed=current['writes']-after['writes'],
                fused=after['fused'], conditions=before['conditions'],
                current_bytes=current['bytes'], new_bytes=after['bytes'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', action='append', default=[])
    parser.add_argument('--all', action='store_true')
    args = parser.parse_args()
    selected = {0x00DAB480, 0x00DAA5F0, 0x00FE9580, 0x012EB690, 0x00CF34D0}
    for profile in args.profile:
        selected.update(int(a, 16) for a in re.findall(r'f_([0-9a-fA-F]{8})', Path(profile).read_text()))
    cache = Path(lift.N, 'patterns/lift.json')
    for function in records(cache):
        if function['addr'] == 0x00DAB480:
            selected.update(lift.callees(function['arms']))
    totals = Counter()
    for function in records(cache):
        if args.all or function['addr'] in selected:
            result = measure(function)
            print(json.dumps(result), flush=True)
            totals.update({k: v for k, v in result.items() if k != 'addr'})
            totals['functions'] += 1
    print(json.dumps({'total': dict(totals)}), flush=True)


if __name__ == '__main__':
    main()
