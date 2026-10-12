"""Hash-only preservation audit for private recorder batches (not a snapshot).

baseline stores no arms, memory bytes, registers or vectors. compare fails
on removals, missing cases or edits outside newly closed helpers and the
existing cases that mocked those helpers. The head owns data snapshots.
"""
import argparse
from collections import defaultdict
import gc
import hashlib
import json
from pathlib import Path


def read(path):
    with Path(path).open(encoding='utf-8') as stream:
        return json.load(stream)


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def fingerprint(root):
    root = Path(root)
    paths = [root / 'patterns/lift.json', root / 'vectors/lift.json']
    stamps = [(p.stat().st_size, p.stat().st_mtime_ns) for p in paths]
    functions = {}
    for f in read(paths[0]):
        key = str(f['addr'])
        if key in functions:
            raise ValueError(f'duplicate selected address {key}')
        functions[key] = dict(hash=digest(f), ext=bool(f.get('ext')))
    gc.collect()
    cases, mocked = defaultdict(list), defaultdict(set)
    for c in read(paths[1]):
        key = str(c['addr'])
        if key not in functions:
            raise ValueError(f'orphan case {key}')
        cases[key].append(digest(c))
        mocked[key].update(e['target'] for e in c.get('calls', []))
    gc.collect()
    if set(cases) != set(functions):
        raise ValueError('selected functions without cases')
    if stamps != [(p.stat().st_size, p.stat().st_mtime_ns) for p in paths]:
        raise ValueError('pair changed during audit; stop the writer and retry')
    return dict(version=1, functions=functions,
                cases={a: sorted(v) for a, v in cases.items()},
                mocked={a: sorted(v) for a, v in mocked.items() if v})


def compare(before, after):
    old, new = before['functions'], after['functions']
    missing = old.keys() - new.keys()
    added = new.keys() - old.keys()
    promoted = {a for a in old.keys() & new.keys() if old[a]['ext'] and not new[a]['ext']}
    promoted_ints = {int(a) for a in promoted}
    affected = {a for a, targets in before['mocked'].items() if promoted_ints.intersection(targets)}
    changed = {a for a in old.keys() & new.keys()
               if old[a]['hash'] != new[a]['hash'] or before['cases'][a] != after['cases'][a]}
    unexpected = changed - promoted - affected
    # Even when a selected body's IR is unchanged, cases that mocked a newly
    # closed helper cannot survive unchanged across the dispatch boundary.
    stale = {a for a in affected if a in new and before['cases'][a] == after['cases'][a]}
    still_mocked = {a for a, targets in after['mocked'].items() if promoted_ints.intersection(targets)}
    addresses = lambda values: [f'0x{int(a):08X}' for a in sorted(values, key=int)]
    return dict(ok=not (missing or unexpected or stale or still_mocked),
                before_functions=len(old), after_functions=len(new),
                before_cases=sum(map(len, before['cases'].values())),
                after_cases=sum(map(len, after['cases'].values())),
                unchanged=len(old)-len(missing)-len(changed),
                added=addresses(added), promoted=addresses(promoted),
                affected_callers=addresses(affected), changed=addresses(changed),
                missing=addresses(missing), unexpected_changes=addresses(unexpected),
                stale_mock_cases=addresses(stale), still_mocking_promoted=addresses(still_mocked))


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('command', choices=['baseline', 'compare'])
    ap.add_argument('--root', required=True, type=Path)
    ap.add_argument('--manifest', required=True, type=Path)
    ap.add_argument('--report', type=Path)
    args = ap.parse_args()
    current = fingerprint(args.root)
    if args.command == 'baseline':
        # Never silently replace the evidence for a batch already in flight.
        with args.manifest.open('x', encoding='utf-8') as stream:
            json.dump(current, stream)
        print(f"hashes only: {len(current['functions'])} functions / {sum(map(len,current['cases'].values()))} cases")
    else:
        report = compare(read(args.manifest), current)
        text = json.dumps(report, indent=2)
        if args.report:
            args.report.write_text(text+'\n', encoding='utf-8')
        print(text)
        if not report['ok']:
            raise SystemExit(1)


if __name__ == '__main__':
    main()
