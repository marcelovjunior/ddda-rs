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
    # L49: a caller state (caller_state.py) also binds ESP into its frame
    if set(state) - {'esp'} != {'pages', 'heap', 'registers'}:
        raise ValueError('invalid constructed state fields')
    if 'esp' in state:
        esp = state['esp']
        if (not isinstance(esp, int) or esp % 4 or not h.STACK <= esp < h.STACK + h.STACK_SIZE - 4
                or esp & ~0xFFF not in state['pages']):
            raise ValueError('invalid constructed stack pointer')
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


def construct(setup, accepted, bindings, run, key):
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
    actual = run(setup, seed, capture_state=state, **s._inputs([accepted]))
    expected = {k: v for k, v in accepted.items() if k not in ('addr', 'seed', 'body_key')}
    # Older accepted cases predate added x87/MXCSR observations. Compare every
    # field they actually recorded, including complete calls, reads and writes.
    if digest({k: actual.get(k) for k in expected}) != digest(expected):
        raise h.Rejected('constructed input: setup replay differs from accepted case')
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


def main():
    import lift
    import structured_inputs as s
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('method', type=lambda v: int(v, 0))
    p.add_argument('setup', type=lambda v: int(v, 0))
    p.add_argument('--bind', action='append', default=[], metavar='ECX=regs_in.ecx')
    p.add_argument('--seconds', type=float, default=20)
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
    setup = fs[args.setup]
    method = next(f for f in s.read(root/'patterns/structured_candidates.json') if f['addr'] == args.method)
    accepted = [c for c in s.read(root/'vectors/lift.json') if c['addr'] == args.setup]
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
    for index, case in enumerate(accepted[:3], 1):
        seed = (args.method << 8) ^ index
        try:
            state, recipe = construct(setup, case, bindings, run, lift.vector_key(setup))
            result = run(method, seed, initial_state=state, input_strategy='zero-call-v1')
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
