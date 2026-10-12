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
        if a in have or a in quarantine:
            continue
        try:
            arms = lift.lift(a, size, closed, ext=True)
            candidates.append(dict(addr=a,name=f'{ns}::{name}',size=size,arms=arms,
                                   calls=lift.callees(arms),x87_push=lift.X87_PUSH_SITES.get(a,[]),
                                   ext=True,**lift.BODY_INFO[a]))
            if a in h.PLATFORM:
                candidates[-1]['original_entry'] = True
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
_FRAMES = {}


def frame_contracts(selected):
    """Caller-frame exits of closed entries (set by promote), for nested frames."""
    return {f['addr']: f['frame_exit'] for f in selected if not f.get('ext') and f.get('frame_exit')}


def init_worker(closed, noreturn, frames=None):
    global _CLOSED, _NORETURN, _FRAMES
    _CLOSED, _NORETURN, _FRAMES = frozenset(closed), frozenset(noreturn), dict(frames or {})


def trial(args):
    f, strategy, seeds, seconds = args[:4]
    terminal = len(args) > 4 and args[4]
    layout = args[5] if len(args) > 5 else None
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
        if layout is not None:
            kw['input_layout'] = layout
        try:
            result = h.run_sparse(f['addr'], seed, end=f['addr']+f['size'], lifted=_CLOSED,
                                  body=f['body'], x87_push=f.get('x87_push',[]),
                                  noreturn_sites=f.get('noreturn_sites',[]), noreturn_targets=_NORETURN,
                                  original_entry=f.get('original_entry',False), x87_entry=f.get('x87_entry',0), max_insn=f.get('insn_budget') or lift.INSN_BUDGET,
                                  frame_contracts=_FRAMES, terminal=terminal, max_seconds=remaining, **kw)
            cases.append(dict(addr=f['addr'],seed=seed,body_key=key,**result))
            attempts.append(dict(reason='ok',seed=seed))
        except h.Rejected as e:
            attempts.append(dict(reason=str(e),seed=seed))
        except Exception as e:
            attempts.append(dict(reason=f'error:{type(e).__name__}: {e}',seed=seed))
        finally:
            gc.collect()
    return dict(addr=f['addr'],body_key=key,strategy=strategy,requested_seeds=seeds,
                seconds=seconds,attempts=attempts,cases=cases,
                **({'input_layout':layout} if layout is not None else {}))


def done_keys(prior, seeds, seconds, retry=None, layout=None):
    """Logged (addr, body_key) trials to skip on resume. `retry` (a regex):
    after a recorder change, failed trials with a matching reason run again;
    their earlier records stay in the log."""
    import re
    stale = re.compile(retry) if retry else None
    return {(r['addr'],r['body_key']) for r in prior
            if r.get('requested_seeds')==seeds and r.get('seconds')==seconds
            and r.get('input_layout') == layout
            and not (stale and not r['cases'] and any(stale.search(a['reason']) for a in r['attempts']))}


def trial_log(root, strategy, terminal=False):
    """Terminal trials (terminal-v1 cases) keep their own logs and history."""
    return root/f"structured-{strategy}{'-terminal' if terminal else ''}.jsonl"


def record(root, strategy, seeds, seconds, limit, retry=None, terminal=False, layout=None, addresses=None):
    if (strategy == 'record-range-v1') != (layout is not None):
        raise ValueError('record-range-v1 requires a layout; other strategies do not use one')
    if layout is not None:
        from record_inputs import RecordRangeInputs
        RecordRangeInputs(layout, 1)  # reject malformed layouts before opening a trial log
    selected = read(root/'patterns/lift.json')
    check_boundary(root, selected)
    have = {f['addr'] for f in selected}
    quarantine = quarantined(root)
    candidates = read(root/'patterns/structured_candidates.json')
    path = trial_log(root, strategy, terminal)
    prior = records(path) if path.exists() else []
    done = done_keys(prior, seeds, seconds, retry, layout)
    # Omit assembly strings from worker arguments; workers need only the
    # existing body and call boundary, not another copy of generated Rust.
    todo = [{k:v for k,v in f.items() if k!='arms'} for f in candidates
            if f['addr'] not in have and f['addr'] not in quarantine and (f['addr'],lift.vector_key(f)) not in done
            and (addresses is None or f['addr'] in addresses)]
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
            (closed,lift.noreturn_entries(),frame_contracts(selected)),maxtasksperchild=8) as pool:
        for n,result in enumerate(pool.imap_unordered(trial,((f,strategy,seeds,seconds,terminal,layout) for f in todo),chunksize=1),1):
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


def adopt(root, strategy, terminal=False):
    selected=read(root/'patterns/lift.json');cases=read(root/'vectors/lift.json')
    check_boundary(root, selected)
    candidates={f['addr']:f for f in read(root/'patterns/structured_candidates.json')}
    have={f['addr'] for f in selected};added=[]
    quarantine=quarantined(root)
    for r in records(trial_log(root, strategy, terminal)):
        a=r['addr'];f=candidates.get(a)
        if a not in have and a not in quarantine and f and r['body_key']==lift.vector_key(f) and r['cases']:
            selected.append(f);cases.extend(r['cases']);have.add(a);added.append(a)
    write(root/'patterns/lift.json',selected);write(root/'vectors/lift.json',cases)
    prune_no_vectors(root,have)
    history=trial_log(root, strategy, terminal).with_name(trial_log(root, strategy, terminal).stem+'-adopted.json')
    previous=read(history) if history.exists() else []
    write(history,sorted(set(previous)|set(added)))
    print(f'{strategy}: added {len(added)}; {len(selected)} functions / {len(cases)} cases; Rust replay still required',flush=True)


def direct_targets(arms):
    """Constant call/tail targets an ext body sends through call_ext."""
    import re
    text = ' '.join(arms)
    return ({int(t, 16) for t in re.findall(r'let t = 0x([0-9A-F]+)u32;', text)}
            | {int(t, 16) for t in re.findall(r'call_ext\(m, cpu, 0x([0-9A-F]+)\)', text)})


def _inputs(cases):
    """run_sparse keywords reproducing the input distribution of `cases`."""
    strategies = {c.get('input_strategy') or ('zero' if c.get('zero_inputs') else 'random') for c in cases}
    strategy = strategies.pop() if len(strategies) == 1 else 'random'
    if strategy == 'zero':
        return dict(zero_inputs=True)
    if strategy == 'record-range-v1':
        layouts = {json.dumps(c.get('input_layout'),sort_keys=True) for c in cases}
        if len(layouts) != 1 or None in [c.get('input_layout') for c in cases]:
            raise ValueError('inconsistent recorded range layouts')
        return dict(input_strategy=strategy,input_layout=json.loads(layouts.pop()))
    return {} if strategy == 'random' else dict(input_strategy=strategy)


FALLBACK_INPUTS = ({}, dict(zero_inputs=True), dict(input_strategy='zero-call-v1'), dict(input_strategy='small-v1'),
                   dict(input_strategy='mockzero-v1'), dict(input_strategy='objects-v1'))


def _rerecord_any(f, closed, frames, noreturn, kw, seeds, seconds):
    """The previous distribution first, then the standard ones in order: a
    boundary change can make the old inputs unusable (a caller of a promoted
    `__alloca_probe` with random EAX now really allocates), and any
    distribution gives valid fixtures."""
    for k in [kw] + [k for k in FALLBACK_INPUTS if k != kw]:
        new = _rerecord(f, closed, frames, noreturn, k, seeds, seconds)
        if new:
            return new
    return []


def _rerecord(f, closed, frames, noreturn, kw, seeds, seconds):
    """Fresh cases for `f` under the given closed boundary (its own mode)."""
    a, key, new, start = f['addr'], lift.vector_key(f), [], time.monotonic()
    for index in range(1, seeds * 8 + 1):
        if len(new) == seeds or time.monotonic() - start > seconds:
            break
        seed = (a << 8) ^ index
        mode = (dict(end=a+f['size'], lifted=frozenset(closed), x87_push=f.get('x87_push', []),
                     original_entry=f.get('original_entry', False)) if f.get('ext') else dict(size=f['size']))
        try:
            r = h.run_sparse(a, seed, body=f['body'], noreturn_sites=f.get('noreturn_sites', []),
                             noreturn_targets=noreturn, frame_contracts=frames, max_seconds=seconds,
                             x87_entry=f.get('x87_entry',0), max_insn=f.get('insn_budget') or lift.INSN_BUDGET, **mode, **kw)
        except h.Rejected:
            continue
        finally:
            gc.collect()
        new.append(dict(addr=a, seed=seed, body_key=key, **r))
    return new


def refresh_closed(root, seeds=3, seconds=20):
    """Repair imported cases that still mock an already closed selected body.

    No boundary or unrelated case changes. If any affected caller cannot
    re-record, fail before writing either selected file.
    """
    selected = read(root/'patterns/lift.json')
    cases = read(root/'vectors/lift.json')
    closed = lift.seed_closed_contracts(selected)
    old = {}
    for c in cases:
        old.setdefault(c['addr'], []).append(c)
    affected = {c['addr'] for c in cases if any(e['target'] in closed for e in c.get('calls', []))}
    frames, noreturn = frame_contracts(selected), lift.noreturn_entries()
    replacements = {}
    for f in selected:
        a = f['addr']
        if a not in affected:
            continue
        try:
            arms = lift.lift(a, f['size'], closed, ext=bool(f.get('ext')))
            prior = {k:v for k,v in f.items() if k not in ('x87_entry','insn_budget')}
            g = dict(prior, arms=arms, calls=lift.callees(arms), x87_push=lift.X87_PUSH_SITES.get(a, []),
                     **lift.BODY_INFO[a])
        finally:
            lift.release_body(a, f['size'])
        fresh = _rerecord_any(g, closed, frames, noreturn, _inputs(old[a]), seeds, seconds)
        if not fresh or any(e['target'] in closed for c in fresh for e in c.get('calls', [])):
            raise ValueError(f'closed-boundary refresh failed for {a:#010x}; selected pair untouched')
        replacements[a] = (g, fresh)
    if replacements:
        write(root/'patterns/lift.json', [replacements[f['addr']][0] if f['addr'] in replacements else f for f in selected])
        cases = [c for c in cases if c['addr'] not in replacements] + [c for _, fresh in replacements.values() for c in fresh]
        write(root/'vectors/lift.json', cases)
    print(f'refreshed {len(replacements)} stale closed callers: {[hex(a) for a in sorted(replacements)]}; '
          f'{len(selected)} functions / {len(cases)} cases; Rust replay still required', flush=True)


def promote(root, seeds=3, seconds=20):
    """Promote verified call-free ext entries that unverified candidates call.

    An entry qualifies when its arms make no call at all (no closed callee,
    no call_ext) and some candidate calls it directly. It is re-lifted closed
    and its own cases are re-recorded in closed mode with its previous input
    distribution. Callers then execute the original body, e.g. the frame
    moves of the SEH helpers. Recorded cases that mocked a promoted entry
    would change meaning (replay and recorder run closed targets for real).
    An ordinary call-free entry that some case mocks therefore stays ext: a
    mock with arbitrary returns is a faithful stand-in for it. A frame-moving
    helper (harness.moves_frame: a caller-frame exit, or ESP moved by a
    value as in `__alloca_probe`) is promoted even when mocked, because no
    constant-cleanup mock can move the caller's frame: as in any boundary refresh, the callers whose
    cases mock it are re-lifted against the new closed set and re-recorded
    with their previous distribution. It is promoted only if every such
    caller records again; otherwise it stays ext (repeat until stable), so
    no verified function is lost.
    The closed boundary changes: prepare again in fresh trial logs.
    """
    selected = read(root/'patterns/lift.json'); cases = read(root/'vectors/lift.json')
    candidates = read(root/'patterns/structured_candidates.json')
    if any(root.glob('structured-*.jsonl')):
        raise ValueError('archive the trial logs first: promotion changes the closed boundary')
    closed = lift.seed_closed_contracts(selected)
    wanted = set().union(*(direct_targets(f['arms']) for f in candidates)) if candidates else set()
    old, index = {}, {f['addr']: k for k, f in enumerate(selected)}
    for c in cases:
        old.setdefault(c['addr'], []).append(c)
    quarantine = quarantined(root)
    noreturn = lift.noreturn_entries()
    base_frames = frame_contracts(selected)
    mocked = {e['target'] for c in cases for e in (c.get('calls') or [])}
    eligible = {}
    for f in selected:
        a = f['addr']
        if (not f.get('ext') or f.get('original_entry') or a in h.PLATFORM or a in quarantine
                or a not in wanted or f.get('calls') or any('call_ext' in arm for arm in f['arms'])):
            continue
        try:
            arms = lift.lift(a, f['size'], closed, ext=False)
        except lift.Unsupported:
            continue
        finally:
            lift.release_body(a, f['size'])
        g = {key: value for key, value in f.items() if key != 'ext'}
        g.update(arms=arms, calls=lift.callees(arms), x87_push=lift.X87_PUSH_SITES.get(a, []), **lift.BODY_INFO[a])
        if g['calls']:
            continue
        contract = h.frame_exit(a, g['body'])
        if h.caller_frame(contract):
            g['frame_exit'] = contract
        elif h.moves_frame(a, g['body']):
            g['frame_exit'] = 'return-pc'
        elif a in mocked:
            continue
        new = _rerecord_any(g, closed, base_frames, noreturn, _inputs(old.get(a, [])), seeds, seconds)
        if new:
            eligible[a] = (g, new)
    affected = {}
    for c in cases:
        hit = {e['target'] for e in (c.get('calls') or [])} & eligible.keys()
        if hit:
            affected.setdefault(c['addr'], set()).update(hit)
    chosen = set(eligible)
    while True:
        now = closed | chosen
        frames = {**base_frames, **{a: eligible[a][0]['frame_exit'] for a in chosen if 'frame_exit' in eligible[a][0]}}
        redone, failed = {}, set()
        for a, targets in affected.items():
            if not targets & chosen:
                continue
            f = selected[index[a]]
            try:
                arms = lift.lift(a, f['size'], now, ext=True)
                g = dict(f, arms=arms, calls=lift.callees(arms), x87_push=lift.X87_PUSH_SITES.get(a, []),
                         **lift.BODY_INFO[a])
            except lift.Unsupported:
                failed.add(a)
                continue
            finally:
                lift.release_body(a, f['size'])
            new = _rerecord_any(g, now, frames, noreturn, _inputs(old[a]), seeds, seconds)
            if new:
                redone[a] = (g, new)
            else:
                failed.add(a)
        if not failed:
            break
        chosen -= set().union(*(affected[a] for a in failed))
    replaced = {**{a: eligible[a] for a in chosen}, **redone}
    for a, (g, _) in replaced.items():
        selected[index[a]] = g
    cases = [c for c in cases if c['addr'] not in replaced] + [c for a in replaced for c in replaced[a][1]]
    write(root/'patterns/lift.json', selected); write(root/'vectors/lift.json', cases)
    history = root/'structured-promoted.json'
    previous = read(history) if history.exists() else dict(promoted=[], rerecorded=[])
    if isinstance(previous, list):
        previous = dict(promoted=previous, rerecorded=[])
    write(history, dict(promoted=sorted(set(previous['promoted']) | chosen),
                        rerecorded=sorted(set(previous['rerecorded']) | set(redone)),
                        kept_ext=sorted(set(previous.get('kept_ext', [])) | (set(eligible) - chosen))))
    print(f'promoted {len(chosen)} closed: {[hex(a) for a in sorted(chosen)]}; re-recorded callers '
          f'{[hex(a) for a in sorted(redone)]}; kept ext {[hex(a) for a in sorted(set(eligible) - chosen)]}; '
          f'{len(selected)} functions / {len(cases)} cases; Rust replay still required', flush=True)


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
    ap.add_argument('command',choices=['prepare','record','adopt','promote','refresh-closed'])
    ap.add_argument('--strategy',default='random',choices=('random','zero',*h.INPUT_STRATEGIES))
    ap.add_argument('--seeds',type=int,default=3)
    ap.add_argument('--seconds',type=float,default=20)
    ap.add_argument('--limit',type=int)
    ap.add_argument('--layout',type=Path,help='JSON record-range layout with evidence-derived bindings/stride/fields')
    ap.add_argument('--address',type=lambda s:int(s,0),action='append',help='record only these candidate addresses (repeatable)')
    ap.add_argument('--retry',help='regex: re-run functions whose logged attempts failed with a matching reason')
    ap.add_argument('--terminal',action='store_true',help='accept terminal-v1 cases (end at a root noreturn call)')
    args=ap.parse_args();root=Path(lift.N)
    if not os.environ.get('LIFT_DATA') or root.resolve() == Path('E:/ddda-re/notes/native').resolve():
        ap.error('set LIFT_DATA to a private trial directory, not the shared native directory')
    if args.seeds < 1 or args.seconds <= 0 or (args.limit is not None and args.limit < 0):
        ap.error('positive seeds/seconds and nonnegative limit required')
    if args.command=='prepare':prepare(root)
    elif args.command=='record':record(root,args.strategy,args.seeds,args.seconds,args.limit,args.retry,args.terminal,
                                      read(args.layout) if args.layout else None,args.address)
    elif args.command=='refresh-closed':refresh_closed(root,args.seeds,args.seconds)
    elif args.command=='promote':promote(root,args.seeds,args.seconds)
    else:adopt(root,args.strategy,args.terminal)
