"""Trace one recorder rejection without compiling Rust or resuming faults.

PY trace_recorder.py 0xADDR [--strategy objects-v1] [--seed 1]
Reads private selected/candidate lists; emits bounded JSON diagnostics to stdout.
Use trace_one.py once a usable vector exists to compare Rust block traces.
"""
import argparse
from collections import deque
import json
from pathlib import Path
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from unicorn import UC_HOOK_CODE
from unicorn.x86_const import UC_X86_REG_ESP, UC_X86_REG_EIP
import harness as h
import lift


def trace(function, closed, strategy, seed, seconds=5, limit=64, frames=None):
    """Wrap the ext hook for diagnostics only; never change its decision."""
    original = h.Uc.hook_add
    recent, transfers = deque(maxlen=limit), deque(maxlen=limit)
    md = Cs(CS_ARCH_X86, CS_MODE_32)

    def hook_add(uc, kind, callback, *args, **kwargs):
        if kind != UC_HOOK_CODE or callback.__name__ != 'on_ext':
            return original(uc, kind, callback, *args, **kwargs)
        def wrapped(machine, pc, size, data):
            # Named recorder state is inspected only; hook ordering is intact.
            state = dict(zip(callback.__code__.co_freevars,
                             (c.cell_contents for c in callback.__closure__)))
            branch = state['branch'][0]
            row = dict(pc=pc, esp=machine.reg_read(UC_X86_REG_ESP),
                       root=state['root_frame'][0], source=branch.address if branch else None)
            try:
                ins = next(md.disasm(bytes(machine.mem_read(pc, min(max(size,1),15))), pc))
                row['instruction'] = ins.mnemonic+' '+ins.op_str
            except Exception:
                row['instruction'] = 'unavailable'
            result = callback(machine, pc, size, data)
            row.update(next_pc=machine.reg_read(UC_X86_REG_EIP), next_esp=machine.reg_read(UC_X86_REG_ESP))
            recent.append(row)
            if row['next_pc'] != pc:
                transfers.append(row)
            return result
        return original(uc, kind, wrapped, *args, **kwargs)

    h.Uc.hook_add = hook_add
    f = function
    kw = {'zero_inputs':True} if strategy == 'zero' else {}
    if strategy not in ('random','zero'):
        kw['input_strategy'] = strategy
    try:
        c = h.run_sparse(f['addr'], (f['addr']<<8)^seed, end=f['addr']+f['size'],
                         body=f['body'], lifted=closed, x87_push=f.get('x87_push',[]), x87_pop=f.get('x87_pop', ()),
                         noreturn_sites=f.get('noreturn_sites',[]),
                         original_entry=f.get('original_entry',False), x87_entry=f.get('x87_entry',0), max_insn=f.get('insn_budget') or lift.INSN_BUDGET,
                         frame_contracts=frames,
                         noreturn_targets=lift.noreturn_entries(), max_seconds=seconds, **kw)
        reason, calls = 'ok', c['calls']
    except h.Rejected as e:
        reason, calls = str(e), None
    finally:
        h.Uc.hook_add = original
    return dict(addr=f['addr'],strategy=strategy,seed=seed,reason=reason,calls=calls,
                recent=list(recent),transfers=list(transfers))


if __name__ == '__main__':
    ap=argparse.ArgumentParser(description=__doc__)
    ap.add_argument('address',type=lambda v:int(v,0))
    ap.add_argument('--strategy',choices=('random','zero',*h.INPUT_STRATEGIES),default='random')
    ap.add_argument('--seed',type=int,default=1)
    ap.add_argument('--seconds',type=float,default=5)
    args=ap.parse_args()
    root=Path(lift.N)
    with (root/'patterns/lift.json').open() as stream: selected=json.load(stream)
    functions={f['addr']:f for f in selected}
    path=root/'patterns/structured_candidates.json'
    if path.exists():
        with path.open() as stream: functions.update((f['addr'],f) for f in json.load(stream))
    if args.address not in functions: ap.error('address absent from selected/candidate lists; prepare first')
    print(json.dumps(trace(functions[args.address],{f['addr'] for f in selected if not f.get('ext')},
                           args.strategy,args.seed,args.seconds,
                           frames={f['addr']:f['frame_exit'] for f in selected
                                   if not f.get('ext') and f.get('frame_exit')})))
