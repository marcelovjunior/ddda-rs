"""L09 private candidate scan and bounded, reason-preserving fixture trials.

Use LIFT_DATA and INVENTORY_DB; generated reports/vectors stay private.
prepare preserves the selected closed-call boundary. record never overwrites
the accepted pair; adopt merges successful trials, then requires Rust replay.
"""
import argparse
from collections import Counter
from contextlib import closing
import gc
import json
from multiprocessing import Pool
from pathlib import Path
import os
import sqlite3
import time

import harness as h
import lift


def read(path):
    with Path(path).open() as stream:
        return json.load(stream)


def write(path, value):
    path=Path(path)
    temporary=path.with_suffix(path.suffix+'.tmp')
    temporary.write_text(json.dumps(value),encoding='utf-8')
    temporary.replace(path)


def quarantined(root):
    path = root/'structured_quarantine.json'
    return set(read(path)) if path.exists() else set()


def boundary(selected):
    return {str(f['addr']):lift.vector_key(f) for f in selected if not f.get('ext')}


def check_boundary(root, selected):
    if read(root/'patterns/structured_boundary.json') != boundary(selected):
        raise ValueError('closed recorder boundary changed; use a fresh trial directory and prepare again')


def prepare(root):
    selected = read(root/'patterns/lift.json')
    have = {f['addr'] for f in selected}
    closed = lift.seed_closed_contracts(selected)
    context = root/'patterns/structured_boundary.json'
    if context.exists() and any(root.glob('structured-*.jsonl')):
        check_boundary(root, selected)
    write(context, boundary(selected))
    quarantine = quarantined(root)
    with closing(sqlite3.connect(f'file:{lift.DB}?mode=ro', uri=True)) as db:
        # The rebuilt inventory already merges extra_functions.tsv. No
        # address ceiling: high-address game initializers are candidates.
        # Historical platform classes and thunks are executable entry points too. Verify their actual tail
        # transfer instead of silently excluding the inventory's thunk flag.
        rows = db.execute("SELECT address,namespace,name,size FROM functions WHERE status != 'verified' AND size>=1 ORDER BY address").fetchall()
    candidates, rejected = [], {}
    for a, ns, name, size in rows:
        if a in have or a in h.PLATFORM or a in quarantine:
            continue
        try:
            arms = lift.lift(a, size, closed, ext=True)
            candidates.append(dict(addr=a,name=f'{ns}::{name}',size=size,arms=arms,
                                   calls=lift.callees(arms),x87_push=lift.X87_PUSH_SITES.get(a,[]),
                                   ext=True,**lift.BODY_INFO[a]))
        except lift.Unsupported as e:
            rejected[a] = str(e)
        finally:
            lift.release_body(a, size)
    write(root/'patterns/structured_candidates.json', candidates)
    write(root/'patterns/structured_rejections.json', rejected)
    print(f'{len(have)} retained; {len(candidates)} missing candidates; {len(rejected)} lift rejections; {len(quarantine)} quarantined', flush=True)
    print(Counter(s.split()[0] for s in rejected.values()), flush=True)


_CLOSED = frozenset()
_NORETURN = frozenset()


def init_worker(closed, noreturn):
    global _CLOSED, _NORETURN
    _CLOSED, _NORETURN = frozenset(closed), frozenset(noreturn)


def trial(args):
    f, strategy, seeds, seconds = args
    start = time.monotonic()
    cases, attempts = [], []
    key = lift.vector_key(f)
    for index in range(1, seeds+1):
        remaining = seconds-(time.monotonic()-start)
        if remaining <= 0:
            attempts.append(dict(reason='function time budget', seed=None))
            break
        seed = (f['addr'] << 8) ^ index
        kw = dict(zero_inputs=True) if strategy == 'zero' else {}
        if strategy not in ('random', 'zero'):
            kw['input_strategy'] = strategy
        try:
            result = h.run_sparse(f['addr'], seed, end=f['addr']+f['size'], lifted=_CLOSED,
                                  body=f['body'], x87_push=f.get('x87_push',[]),
                                  noreturn_sites=f.get('noreturn_sites',[]), noreturn_targets=_NORETURN,
                                  max_seconds=remaining, **kw)
            cases.append(dict(addr=f['addr'],seed=seed,body_key=key,**result))
            attempts.append(dict(reason='ok',seed=seed))
        except h.Rejected as e:
            attempts.append(dict(reason=str(e),seed=seed))
        except Exception as e:
            attempts.append(dict(reason=f'error:{type(e).__name__}: {e}',seed=seed))
        finally:
            gc.collect()
    return dict(addr=f['addr'],body_key=key,strategy=strategy,requested_seeds=seeds,
                seconds=seconds,attempts=attempts,cases=cases)


def record(root, strategy, seeds, seconds, limit):
    selected = read(root/'patterns/lift.json')
    check_boundary(root, selected)
    have = {f['addr'] for f in selected}
    quarantine = quarantined(root)
    candidates = read(root/'patterns/structured_candidates.json')
    path = root/f'structured-{strategy}.jsonl'
    prior = records(path) if path.exists() else []
    done = {(r['addr'],r['body_key']) for r in prior
            if r.get('requested_seeds')==seeds and r.get('seconds')==seconds}
    # Omit assembly strings from worker arguments; workers need only the
    # existing body and call boundary, not another copy of generated Rust.
    todo = [{k:v for k,v in f.items() if k!='arms'} for f in candidates
            if f['addr'] not in have and f['addr'] not in quarantine and (f['addr'],lift.vector_key(f)) not in done]
    if limit is not None:
        todo = todo[:limit]
    closed = [f['addr'] for f in selected if not f.get('ext')]
    print(f'{strategy}: {len(todo)} functions, {seeds} seeds, {seconds}s/function', flush=True)
    if path.exists() and path.stat().st_size:
        with path.open('rb+') as stream:
            stream.seek(-1,2)
            if stream.read(1)!=b'\n':
                stream.seek(0,2);stream.write(b'\n')
    with path.open('a') as stream, Pool(int(os.environ.get('LIFT_WORKERS','6')),init_worker,
            (closed,lift.noreturn_entries()),maxtasksperchild=8) as pool:
        for n,result in enumerate(pool.imap_unordered(trial,((f,strategy,seeds,seconds) for f in todo),chunksize=1),1):
            stream.write(json.dumps(result)+'\n');stream.flush()
            if n%250==0:
                print(n,'completed',flush=True)
    results = records(path)
    counts = Counter(a['reason'].split(' at ')[0].split(';')[0] for r in results for a in r['attempts'])
    print(f'{sum(bool(r["cases"]) for r in results)} functions with cases; reasons: {counts.most_common(20)}',flush=True)


def records(path):
    result=[]
    with Path(path).open() as stream:
        for line in stream:
            try:
                result.append(json.loads(line))
            except ValueError:
                continue  # interrupted final line only
    return result


def adopt(root, strategy):
    selected=read(root/'patterns/lift.json');cases=read(root/'vectors/lift.json')
    check_boundary(root, selected)
    candidates={f['addr']:f for f in read(root/'patterns/structured_candidates.json')}
    have={f['addr'] for f in selected};added=[]
    quarantine=quarantined(root)
    for r in records(root/f'structured-{strategy}.jsonl'):
        a=r['addr'];f=candidates.get(a)
        if a not in have and a not in quarantine and f and r['body_key']==lift.vector_key(f) and r['cases']:
            selected.append(f);cases.extend(r['cases']);have.add(a);added.append(a)
    write(root/'patterns/lift.json',selected);write(root/'vectors/lift.json',cases)
    prune_no_vectors(root,have)
    history=root/f'structured-{strategy}-adopted.json'
    previous=read(history) if history.exists() else []
    write(history,sorted(set(previous)|set(added)))
    print(f'{strategy}: added {len(added)}; {len(selected)} functions / {len(cases)} cases; Rust replay still required',flush=True)


def prune_no_vectors(root, have):
    """Successful adoptions must not remain in the historical failure list."""
    path=root/'patterns/lift_novector.json'
    if path.exists():
        write(path,[a for a in read(path) if a not in have])
    path=root/'patterns/lift_novector_keys.json'
    if path.exists():
        write(path,{a:k for a,k in read(path).items() if int(a) not in have})


if __name__ == '__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('command',choices=['prepare','record','adopt'])
    ap.add_argument('--strategy',default='random',choices=('random','zero',*h.INPUT_STRATEGIES))
    ap.add_argument('--seeds',type=int,default=3)
    ap.add_argument('--seconds',type=float,default=20)
    ap.add_argument('--limit',type=int)
    args=ap.parse_args();root=Path(lift.N)
    if not os.environ.get('LIFT_DATA') or root.resolve() == Path('E:/ddda-re/notes/native').resolve():
        ap.error('set LIFT_DATA to a private trial directory, not the shared native directory')
    if args.seeds < 1 or args.seconds <= 0 or (args.limit is not None and args.limit < 0):
        ap.error('positive seeds/seconds and nonnegative limit required')
    if args.command=='prepare':prepare(root)
    elif args.command=='record':record(root,args.strategy,args.seeds,args.seconds,args.limit)
    else:adopt(root,args.strategy)
