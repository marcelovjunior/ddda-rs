"""Run a checked original constructor/setup, then record a method on its state.

No game fields or addresses are prescribed here. A private recipe identifies
an accepted setup case and binds method registers to that case's input/output
registers. Setup calls keep their recorded mock boundary; this does not claim
to construct the complete runtime. Only the method's inputs become its fixture.
"""
import argparse
import gc
import hashlib
import json
import random
from pathlib import Path

import harness as h


def digest(value):
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(',', ':')).encode()).hexdigest()


def validate_state(state):
    if set(state)-{'writes'} != {'pages', 'heap', 'registers'}:
        raise ValueError('invalid constructed state fields')
    if len(state['pages']) > h.MAX_PAGES:
        raise ValueError('constructed state exceeds page cap')
    for p, data in state['pages'].items():
        if not isinstance(p, int) or not 0 <= p <= 0xfffff000 or p % 4096 or not isinstance(data, bytes) or len(data) != 4096:
            raise ValueError('invalid constructed page')
    if not h.HEAP_BASE <= state['heap'] <= 0xffffffff or state['heap'] % 16:
        raise ValueError('invalid constructed heap cursor')
    for name, value in state['registers'].items():
        if name not in dict(h.GPR) or name == 'esp' or not isinstance(value, int) or not 0 <= value <= 0xffffffff:
            raise ValueError('invalid constructed register binding')
    for a,v in state.get('writes',{}).items():
        if type(a) is not int or not 0 <= a <= 0xffffffff or type(v) is not int or not 0 <= v <= 255:
            raise ValueError('invalid setup-written byte')
        if state['pages'].get(a & ~4095, b'')[a & 4095:(a & 4095)+1] != bytes([v]):
            raise ValueError('setup-written byte differs from captured state')


def construct(setup, accepted, bindings, run, key, initial_state=None):
    """`run(f, seed, **inputs)` must use the selected recording boundary."""
    import structured_inputs as s
    if accepted['addr'] != setup['addr'] or accepted.get('body_key') != key:
        raise ValueError('setup case/body mismatch')
    if accepted.get('terminal') or accepted.get('construction'):
        raise ValueError('setup must be a returning, independently recorded case')
    seed = accepted.get('seed')
    if seed is None:
        # Early lift.py vectors did not serialize their seed. The first GPR
        # draw identifies the old random trial; the complete recorded result
        # below must still match. Do not infer seeds for other distributions.
        if accepted.get('input_strategy') or accepted.get('zero_inputs'):
            raise ValueError('setup case lacks seed for its input distribution')
        matches = [(setup['addr'] << 8) ^ i for i in range(1, 25)
                   if random.Random((setup['addr'] << 8) ^ i).getrandbits(32) == accepted['regs_in']['eax']]
        if len(matches) != 1:
            raise ValueError('cannot identify legacy setup seed')
        seed = matches[0]
    state = {}
    if initial_state is not None:
        validate_state(initial_state)
        if initial_state['registers']:
            raise ValueError('setup sequence must retain each accepted register distribution')
    actual = run(setup, seed, capture_state=state,
                 **({'initial_state': initial_state} if initial_state is not None else {}),
                 **s._inputs([accepted]))
    expected = {k: v for k, v in accepted.items() if k not in ('addr', 'seed', 'body_key')}
    # Older accepted cases predate added x87/MXCSR observations. Compare every
    # field they actually recorded, including complete calls, reads and writes.
    if digest({k: actual.get(k) for k in expected}) != digest(expected):
        raise h.Rejected('constructed input: setup replay differs from accepted case')
    if initial_state is not None:
        # Capture contains only pages this phase touched; retain all earlier
        # pages and writes. Later writes win in actual execution order.
        state['pages'] = {**initial_state['pages'], **state['pages']}
        state['writes'] = {**initial_state.get('writes', {}), **state.get('writes', {})}
        state['heap'] = max(initial_state['heap'], state['heap'])
    registers = {}
    for target, source in bindings.items():
        if len(source) != 2 or source[0] not in ('regs_in', 'regs_out') or source[1] not in dict(h.GPR):
            raise ValueError('binding must name a setup input/output register')
        registers[target] = actual[source[0]][source[1]]
    state['registers'] = registers
    validate_state(state)
    provenance = dict(rule='constructed-v1', setup=setup['addr'], seed=seed,
                      body_key=key, case_hash=digest(accepted), bindings=bindings)
    return state, provenance


def construct_chain(setups, bindings, run):
    """Replay each accepted producer on preceding state; reject dependencies
    that change any accepted observation. Two-part register sources name the
    last producer; a leading zero-based phase index selects an earlier one.
    Nothing is published if any phase fails.
    """
    if not setups:
        raise ValueError('setup sequence must not be empty')
    phase_bindings = [{} for _ in setups]
    for target, source in bindings.items():
        index = len(setups)-1
        if len(source) == 3:
            try:
                index = int(source[0])
            except (TypeError, ValueError):
                raise ValueError('setup binding phase must be a zero-based index') from None
            source = source[1:]
        if not 0 <= index < len(setups):
            raise ValueError('setup binding phase is outside the sequence')
        phase_bindings[index][target] = source
    state, recipes, registers = None, [], {}
    for index, (setup, accepted, key) in enumerate(setups):
        state, recipe = construct(setup, accepted, phase_bindings[index],
                                  run, key, initial_state=state)
        registers.update(state['registers'])
        # A method binding never changes the next producer's entry registers.
        state = {**state, 'registers': {}}
        recipes.append(recipe)
    state['registers'] = registers
    validate_state(state)
    return state, dict(rule='constructed-chain-v1', setups=recipes)


def main():
    import lift
    import structured_inputs as s
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('method', type=lambda v: int(v, 0))
    p.add_argument('setup', type=lambda v: int(v, 0))
    p.add_argument('--also-setup', action='append', default=[], type=lambda v: int(v, 0),
                   help='replay another checked setup on the accumulated state')
    p.add_argument('--bind', action='append', default=[], metavar='ECX=[PHASE.]regs_in.ecx',
                   help='bind from the last setup, or a zero-based setup phase')
    p.add_argument('--seconds', type=float, default=20)
    p.add_argument('--strategy', choices=('zero-call-v1','guarded-stores-v1'), default='zero-call-v1')
    args = p.parse_args()
    bindings = {k.lower(): v.split('.') for k, v in (b.split('=') for b in args.bind)}
    root = Path(lift.N)
    selected = s.read(root/'patterns/lift.json')
    s.check_boundary(root, selected)
    closed = lift.seed_closed_contracts(selected)
    frames = s.frame_contracts(selected)
    fs = {f['addr']: f for f in selected}
    if args.method in fs or args.method in s.quarantined(root):
        raise ValueError('method is already selected or quarantined')
    setups = [fs[a] for a in [args.setup, *args.also_setup]]
    method = next(f for f in s.read(root/'patterns/structured_candidates.json') if f['addr'] == args.method)
    all_cases = s.read(root/'vectors/lift.json')
    accepted = [[c for c in all_cases if c['addr'] == f['addr']] for f in setups]
    if any(not cases for cases in accepted):
        raise ValueError('every setup needs an accepted case')
    del all_cases
    del selected, fs
    gc.collect()

    def run(f, seed, **inputs):
        mode = dict(end=f['addr']+f['size'], lifted=closed, x87_push=f.get('x87_push', [])) if f.get('ext') else dict(size=f['size'])
        return h.run_sparse(f['addr'], seed, body=f['body'], original_entry=f.get('original_entry', False),
                            noreturn_sites=f.get('noreturn_sites', []), noreturn_targets=lift.noreturn_entries(),
                            x87_entry=f.get('x87_entry', 0), frame_contracts=frames,
                            max_insn=f.get('insn_budget') or lift.INSN_BUDGET,
                            max_seconds=args.seconds, **mode, **inputs)

    cases, attempts = [], []
    for index in range(1, min(3, *map(len, accepted))+1):
        seed = (args.method << 8) ^ index
        try:
            if args.also_setup:
                phases = [(f, cs[index-1], lift.vector_key(f)) for f, cs in zip(setups, accepted)]
                state, recipe = construct_chain(phases, bindings, run)
            else:
                setup = setups[0]
                state, recipe = construct(setup, accepted[0][index-1], bindings, run, lift.vector_key(setup))
            result = run(method, seed, initial_state=state, input_strategy=args.strategy)
            cases.append(dict(addr=args.method, seed=seed, body_key=lift.vector_key(method),
                              construction=recipe, **result))
            reason = 'ok'
        except (h.Rejected, ValueError) as e:
            reason = str(e)
        finally:
            gc.collect()
        attempts.append(dict(seed=seed, reason=reason))
        print(f'{args.method:#010x} setup {args.setup:#010x} seed {index}: {reason}', flush=True)
    row = dict(addr=args.method, body_key=lift.vector_key(method), strategy='constructed-v1',
               cases=cases, attempts=attempts)
    with s.trial_log(root, 'constructed-v1').open('a') as out:
        out.write(json.dumps(row)+'\n')


if __name__ == '__main__':
    main()
