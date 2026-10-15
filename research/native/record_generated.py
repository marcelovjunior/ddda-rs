"""Record oracle vectors for the functions that research/native/leafgen.py generated ports for.

  DDDA_EXE=reference/DDDA.exe python3 research/native/record_generated.py SPECS.json OUT_DIR [CASES]

One `gen_<name>.json` per function (the format of record_leaves.py: before/after registers, every byte read, every byte
written). Register and stack inputs the symbolic execution found used as addresses point into a random region whose
dwords are, one time in three, pointers back into it (so a pointer loaded from memory is usable too); other inputs
are random words. Absolute addresses the function reads or writes get their own random pages. A case where the
original faults (an access outside what is mapped) is dropped; a function with fewer than 100 good cases is reported.
Prints the names of the functions that were recorded."""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle  # noqa: E402

SP = 0x0F008000
STACK = (0x0F000000, 0x10000)
REGION, REGION_SIZE = 0x20000000, 0x20000
NAMES = 'eax ecx edx ebx esp ebp esi edi'.split()


def pages_of(addresses):
    pages = set()
    for a in addresses:
        for p in (a & ~0xFFF, (a + 3) & ~0xFFF):
            pages.add(p)
    return sorted(p for p in pages if not (REGION <= p < REGION + REGION_SIZE) and not (STACK[0] <= p < STACK[0] + STACK[1]))


def record(spec, count, out):
    abs_addrs = spec['abs_reads'] + spec['abs_writes']
    maps = [(REGION, REGION_SIZE), STACK] + [(p, 0x1000) for p in pages_of(abs_addrs)]
    oracle = Oracle(spec['addr'], spec['length'], {}, maps)
    run_flags_import = __import__('unicorn.x86_const', fromlist=['x'])
    cases, faults = [], 0
    for seed in range(count * 3):
        if len(cases) >= count:
            break
        r = random.Random(seed)
        uc = oracle.uc
        buf = bytearray(r.randbytes(REGION_SIZE))
        words = memoryview(buf).cast('I')
        for _ in range(0x1800):
            words[r.randrange(0x800, 0x2000)] = REGION + 4 * r.randrange(0x800, 0x2000)
        # a quarter of the words are small values (counts, flags, indices)
        for _ in range(0x600):
            words[r.randrange(0, 0x2000)] = r.choice((0, 1, 2, 3, 4, 7, 8, 0xFF, 0x100, 0xFFFFFFFF, r.randrange(64)))
        page_bytes = {p: bytearray(r.randbytes(0x1000)) for p in pages_of(abs_addrs)}
        if spec.get('float_arith'):
            # Unicorn's SSE NaN propagation differs from the hardware's for a signalling NaN with a quiet NaN (it returns the
            # second operand; Intel SDM table 4-7 says the first, quieted). The ports use the exact helpers, so the vectors
            # carry no NaN or infinity bit patterns for these functions: a word with exponent 0xFF loses its exponent's low bit.
            import numpy as np
            for data in [buf] + list(page_bytes.values()):
                words = np.frombuffer(data, dtype=np.uint32)   # writable view of the bytearray
                words[words & 0x7F800000 == 0x7F800000] &= np.uint32(0xFF7FFFFF)
        uc.mem_write(REGION, bytes(buf))
        for p, data in page_bytes.items():
            uc.mem_write(p, bytes(data))
        pointer = lambda: REGION + 4 * r.randrange(0x1000, 0x2000)
        regs = [r.randrange(1 << 32) for _ in range(8)]
        for name in spec['pointer_inputs']:
            if name in NAMES:
                regs[NAMES.index(name)] = pointer()
        args = [pointer() if f'arg{k}' in spec['pointer_inputs'] else r.randrange(1 << 32) for k in range(spec['stack_args'])]
        try:
            case = oracle.run(seed, regs, SP - 0x20 - 4 * r.randrange(4), args)
        except Exception:
            faults += 1
            continue
        case['after']['eflags'] = oracle.uc.reg_read(run_flags_import.UC_X86_REG_EFLAGS)
        cases.append(case)
    if len(cases) < 100:
        return None
    json.dump(dict(function=hex(spec['addr']), cases=cases), open(out, 'w'))
    return len(cases)


def main():
    specs = json.load(open(sys.argv[1]))
    out = Path(sys.argv[2])
    count = int(sys.argv[3]) if len(sys.argv) > 3 else 400
    out.mkdir(parents=True, exist_ok=True)
    good, bad = [], []
    for spec in specs:
        n = record(spec, count, out / f"gen_{spec['name']}.json")
        (good if n else bad).append(spec['name'])
    print(f'{len(good)} recorded, {len(bad)} unrecorded', file=sys.stderr)
    print('\n'.join(good))
    for b in bad:
        print('unrecorded:', b, file=sys.stderr)


main()
