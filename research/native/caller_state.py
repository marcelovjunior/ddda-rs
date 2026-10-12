"""caller-state-v1 (L49): record an inner helper from its caller's state.

Some helpers run in their caller's frame: the CRT's x87 error handlers
(0x01304753, 0x0130474C, 0x01300617) load the control word their caller
saved with `fnstcw [ebp - 0xa4]`, so random frame bytes stand in for a
value the caller wrote, and the recorder rejects the unmasked exceptions
they would unmask. A verified caller's accepted case reaches the helper as
a mocked call. That case is replayed exactly (constructed_inputs.construct's
check: every recorded field must match) up to the call; the pages, heap
cursor, registers and ESP there (harness.run_sparse capture_call) start the
helper in its caller's frame. Other inputs keep the random distribution,
and the helper records its own reads and writes as any case does.
Legacy random cases use constructed_inputs' seed identification, followed
by the same complete replay comparison. Other unseeded policies are refused.

    python caller_state.py 0x01304753 [--adopt]

Use LIFT_DATA; trials go to structured-caller-state-v1.jsonl.
"""
import argparse
import gc
import json
from pathlib import Path

import harness as h
from constructed_inputs import digest, validate_state, case_seed


def capture(caller, accepted, target, run, key):
    """(state, provenance) at `accepted`'s first mocked call to `target`;
    `run(f, seed, **inputs)` must use the caller's recording boundary."""
    import structured_inputs as s
    if accepted['addr'] != caller['addr'] or accepted.get('body_key') != key:
        raise ValueError('caller case/body mismatch')
    if accepted.get('terminal') or accepted.get('construction'):
        raise ValueError('caller case must be a returning, independently recorded case')
    if not any(e.get('target') == target for e in accepted.get('calls') or ()):
        raise ValueError('caller case does not mock the target')
    seed = case_seed(caller, accepted)
    box = {'target': target}
    try:
        actual = run(caller, seed, capture_call=box, **s._inputs([accepted]))
    except h.Rejected as error:
        raise h.Rejected(f'caller replay: {error}') from error
    expected = {k: v for k, v in accepted.items() if k not in ('addr', 'seed', 'body_key')}
    if digest({k: actual.get(k) for k in expected}) != digest(expected):
        raise h.Rejected('caller state: caller replay differs from accepted case')
    state = box.get('state')
    if state is None:
        raise h.Rejected('caller state: the replay does not reach the call')
    validate_state(state)
    provenance = dict(rule='caller-state-v1', caller=caller['addr'], seed=seed, target=target,
                      body_key=key, case_hash=digest(accepted))
    return state, provenance


def main():
    import lift
    import structured_inputs as s
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('target', type=lambda v: int(v, 0), nargs='+')
    p.add_argument('--seeds', type=int, default=3)
    p.add_argument('--seconds', type=float, default=20)
    p.add_argument('--adopt', action='store_true', help='adopt the successful trials (structured_inputs.adopt)')
    args = p.parse_args()
    root = Path(lift.N)
    selected = s.read(root/'patterns/lift.json')
    s.check_boundary(root, selected)
    closed = lift.seed_closed_contracts(selected)
    frames = s.frame_contracts(selected)
    fs = {f['addr']: f for f in selected}
    candidates = {f['addr']: f for f in s.read(root/'patterns/structured_candidates.json')}
    cases = s.read(root/'vectors/lift.json')
    quarantine = s.quarantined(root)

    def run(f, seed, **inputs):
        mode = dict(end=f['addr']+f['size'], lifted=closed, x87_push=f.get('x87_push', [])) if f.get('ext') else dict(size=f['size'])
        return h.run_sparse(f['addr'], seed, body=f['body'], original_entry=f.get('original_entry', False),
                            noreturn_sites=f.get('noreturn_sites', []), noreturn_targets=lift.noreturn_entries(),
                            x87_entry=f.get('x87_entry', 0), frame_contracts=frames,
                            max_insn=f.get('insn_budget') or lift.INSN_BUDGET,
                            max_seconds=args.seconds, **mode, **inputs)

    for target in args.target:
        if target in fs or target in quarantine or target not in candidates:
            print(f'{target:#010x}: already selected, quarantined or not a prepared candidate', flush=True)
            continue
        helper = candidates[target]
        # one case per caller in turn, so that several callers' states are used
        by_caller = {}
        for c in sorted(cases, key=lambda c: (c['addr'], c.get('seed') or 0)):
            if (c['addr'] in fs
                    and any(e.get('target') == target for e in c.get('calls') or ())):
                try:
                    case_seed(fs[c['addr']], c)
                except ValueError:
                    continue
                by_caller.setdefault(c['addr'], []).append(c)
        order = [c for k in range(max(map(len, by_caller.values()), default=0))
                 for a in sorted(by_caller) for c in by_caller[a][k:k + 1]]
        found, attempts = [], []
        for index, case in enumerate(order[:args.seeds], 1):
            seed = (target << 8) ^ index
            try:
                state, recipe = capture(fs[case['addr']], case, target, run, lift.vector_key(fs[case['addr']]))
                result = run(helper, seed, initial_state=state)
                found.append(dict(addr=target, seed=seed, body_key=lift.vector_key(helper), construction=recipe, **result))
                reason = 'ok'
            except (h.Rejected, ValueError) as e:
                reason = str(e)
            finally:
                gc.collect()
            attempts.append(dict(seed=seed, reason=reason, caller=case['addr']))
            print(f'{target:#010x} from {case["addr"]:#010x} seed {case.get("seed", "legacy")}: {reason}', flush=True)
        if not order:
            print(f'{target:#010x}: no verified case mocks it', flush=True)
        row = dict(addr=target, body_key=lift.vector_key(helper), strategy='caller-state-v1',
                   cases=found, attempts=attempts)
        with s.trial_log(root, 'caller-state-v1').open('a') as out:
            out.write(json.dumps(row) + '\n')
    if args.adopt:
        s.adopt(root, 'caller-state-v1')


if __name__ == '__main__':
    main()
