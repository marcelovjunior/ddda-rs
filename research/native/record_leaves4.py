"""Oracles for two more leaf functions with register arguments (no guest calls):

  init_node.json     0x004241F0  EAX = object (EDX ignored): resets a node (vectors from three image float triples)
  flag_to_index.json 0x005F1F90  ECX = table index: maps a flag value from a table in the engine object to a small index

Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_leaves4.py OUT_DIR [N]"""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from s10_oracle import Oracle, finish  # noqa: E402

SP = 0x0F008000
STACK = (0x0F000000, 0x10000)
REGION = 0x20000000
NAMES = 'eax ecx edx ebx esp ebp esi edi'.split()


def run_flags(oracle, *args):
    """Oracle.run plus the EFLAGS the original leaves."""
    import unicorn.x86_const as ux
    case = oracle.run(*args)
    case['after']['eflags'] = oracle.uc.reg_read(ux.UC_X86_REG_EFLAGS)
    return case


def regs_for(r, **fixed):
    regs = [r.randrange(1 << 32) for _ in range(8)]
    for name, v in fixed.items():
        regs[NAMES.index(name)] = v
    return regs


def init_node(out, count):
    consts = (0x018CFE80, 0x01629510)
    oracle = Oracle(0x004241F0, 0xE5, {}, [(REGION, 0x1000), (0x018CF000, 0x1000), (0x01629000, 0x1000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        uc.mem_write(REGION, bytes(r.randrange(256) for _ in range(0x1000)))
        for base in consts:
            uc.mem_write(base, struct.pack('<3I', *[r.randrange(1 << 32) for _ in range(3)]))
        regs = regs_for(r, eax=REGION + 0x100 + 4 * r.randrange(16))
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), []))
    finish(out, '0x004241F0', cases)


def flag_to_index(out, count):
    values = [1, 8, 0x100, 0x200, 0x400, 0x800, 0x1000, 0x2000, 0x4000, 0x8000, 0x400000]
    oracle = Oracle(0x005F1F90, 0xA0, {}, [(REGION, 0x4000), (0x018D8000, 0x1000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        base = REGION + 0x100
        uc.mem_write(0x018D8BB0, struct.pack('<I', base))
        index = r.choice((0x38, 0x38, r.randrange(0x40), r.randrange(0x40)))
        uc.mem_write(REGION, bytes(r.randrange(256) for _ in range(0x4000)))
        for k in range(0x40):
            v = r.choice(values + values + [0, 2, 3, 4, 7, 9, 0x10, 0x80, 0x300, 0xFFFFFFFF, r.randrange(1 << 32)])
            uc.mem_write(base + 0x198C + 4 * k, struct.pack('<I', v))
        regs = regs_for(r, ecx=index)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), []))
    finish(out, '0x005F1F90', cases)


def main():
    out, count = Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    out.mkdir(parents=True, exist_ok=True)
    init_node(out / 'init_node.json', count)
    flag_to_index(out / 'flag_to_index.json', count)


main()
