"""Per-rule platform lifting report; private data only, no status edits.

Run after families.py platform, private inventory.py build and Rust replay.
A selection is called selected, not verified: verification comes from the
rebuilt inventory, which must follow successful required-vector replay.
"""
from collections import Counter, defaultdict
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import harness as h
import lift
import structured_inputs as s


def report(root):
    selected = {f['addr'] for f in s.read(root/'patterns/lift.json')}
    baseline = {f['addr'] for f in s.read(root/'patterns/lift-baseline.json')}
    added = selected - baseline
    candidates = {f['addr']: f for f in s.read(root/'patterns/structured_candidates.json')}
    rejected = {int(a): why for a, why in s.read(root/'patterns/structured_rejections.json').items()}
    quarantine_path = root/'quarantine_evidence.json'
    quarantine = {int(a): evidence for a,evidence in s.read(quarantine_path).items()} if quarantine_path.exists() else {}
    trials = defaultdict(list)
    for path in sorted(root.glob('structured-*.jsonl')):
        with path.open() as stream:
            for line in stream:
                r = json.loads(line)
                f = candidates.get(r['addr'])
                if r['addr'] not in selected and f and r['body_key'] == lift.vector_key(f):
                    trials[r['addr']].append(dict(strategy=r['strategy'], attempts=r['attempts'], has_cases=bool(r.get('cases'))))
    with closing(sqlite3.connect(f'file:{lift.DB}?mode=ro', uri=True)) as db:
        rows = db.execute('SELECT address,name,size,thunk,status,platform_rule FROM functions WHERE platform_rule IS NOT NULL ORDER BY address').fetchall()
    with closing(sqlite3.connect(f'file:{root / "inventory-baseline.sqlite"}?mode=ro', uri=True)) as db:
        before = dict(db.execute('SELECT address,status FROM functions'))
    groups = defaultdict(Counter); entries = []
    for a, name, size, thunk, status, rule in rows:
        g = groups[rule];g['total'] += 1;g['thunks'] += thunk
        g['non_thunks'] += not thunk
        g['verified_before'] += before[a] == 'verified'
        g['verified_non_thunks_before'] += before[a] == 'verified' and not thunk
        g['verified_non_thunks'] += status == 'verified' and not thunk
        g['verified'] += status == 'verified';g['selected_before'] += a in baseline
        g['selected_after'] += a in selected;g['added'] += a in added
        g['liftable'] += a in selected or a in candidates or a in quarantine
        g['liftable_non_thunks'] += (a in selected or a in candidates or a in quarantine) and not thunk
        if a in selected: category, evidence = 'selected', 'required Rust replay and inventory credit reported separately'
        elif a in h.PLATFORM: category, evidence = 'test-model', h.PLATFORM[a]
        elif status == 'verified': category, evidence = 'verified-hand-port', 'inventory declaration/test pair'
        elif a in rejected: category, evidence = 'lift-rejection', rejected[a]
        elif a in quarantine: category, evidence = 'replay-mismatch', quarantine[a]
        elif any(t['has_cases'] for t in trials[a]): category, evidence = 'unadopted', trials[a]
        elif a in trials and trials[a]: category, evidence = 'recorder-rejection', trials[a]
        else: category, evidence = 'unaccounted', None
        g[category] += 1
        entries.append(dict(address=a,name=name,size=size,thunk=bool(thunk),status=status,status_before=before[a],rule=rule,category=category,evidence=evidence))
    result = dict(rules=dict(groups),entries=entries)
    s.write(root/'platform-report.json',result)
    for rule,g in sorted(groups.items()):print(rule,dict(g))
    if any(g['unaccounted'] or g['unadopted'] for g in groups.values()):
        raise SystemExit('platform rows lack evidence or successful trials are unadopted')
    return result


if __name__ == '__main__':
    root=Path(lift.N)
    if root.resolve() == Path('E:/ddda-re/notes/native').resolve():raise SystemExit('private LIFT_DATA required')
    report(root)
