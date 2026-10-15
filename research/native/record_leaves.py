"""Oracles for four small leaf functions (no guest calls), one JSON per function in OUT_DIR:

  clear_grid.json       0x005A65E0  EAX = index, ECX = base (register arguments), no stack arguments
  lanes_in_range.json   0x00767B40  thiscall `ret 4`: two SSE lane comparisons blended by two image masks
  find_record.json      0x004EA200  thiscall `ret 4`: first record of 5 dwords equal to a key, or 0
  advance_bits.json     0x01328FD0  cdecl(bit reader, bits): moves a bit position

Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_leaves.py OUT_DIR [N]"""
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from s10_oracle import Oracle, finish  # noqa: E402

SP = 0x0F008000
STACK = (0x0F000000, 0x10000)
REGION = 0x20000000


def f32(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def run_flags(oracle, *args):
    """Oracle.run plus the EFLAGS the original leaves (arithmetic flags are compared by the tests)."""
    import unicorn.x86_const as ux
    case = oracle.run(*args)
    case['after']['eflags'] = oracle.uc.reg_read(ux.UC_X86_REG_EFLAGS)
    return case


def regs_for(r, **fixed):
    regs = [r.randrange(1 << 32) for _ in range(8)]
    for name, v in fixed.items():
        regs['eax ecx edx ebx esp ebp esi edi'.split().index(name)] = v
    return regs


def clear_grid(out, count):
    oracle = Oracle(0x005A65E0, 0x33, {}, [(REGION, 0x40000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        uc.mem_write(REGION, bytes(r.randrange(1, 256) for _ in range(0x40000)))
        if r.random() < 0.08:
            # the address computes to zero: nothing is cleared
            index = r.randrange(0, 3)
            base = (-(0x10 + index * 0x41D0)) & 0xFFFFFFFF
        else:
            index = r.randrange(0, 3)
            base = REGION + r.randrange(0, 0x40) * 4
        regs = regs_for(r, eax=index, ecx=base)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), []))
    finish(out, '0x005A65E0', cases)


def lanes_in_range(out, count):
    real_k1, real_k2 = h.code(0x0162AC60, 16), h.code(0x0162AC30, 16)
    oracle = Oracle(0x00767B40, 0x4E, {}, [(REGION, 0x1000), (0x0162A000, 0x1000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        this, other = REGION + 0x100, REGION + 0x400

        def lane():
            k = r.random()
            if k < 0.05:
                return float('nan')
            return r.choice((0.0, 1.0, -1.0, 2.5, r.uniform(-3, 3)))

        def vec():
            return b''.join(struct.pack('<I', f32(lane())) for _ in range(4))
        # equal lanes between the two boxes exercise the <= (not <)
        a, b = [lane() for _ in range(12)], [lane() for _ in range(12)]
        uc.mem_write(this, b''.join(struct.pack('<f', v) if v == v else struct.pack('<I', 0x7FC00000) for v in a))
        uc.mem_write(other, b''.join(struct.pack('<f', v) if v == v else struct.pack('<I', 0x7FC00000) for v in b))
        for k in range(4):
            if r.random() < 0.35:
                uc.mem_write(other + 0x10 + 4 * k, bytes(uc.mem_read(this + 0x10 + 4 * k, 4)))
            if r.random() < 0.35:
                uc.mem_write(this + 0x20 + 4 * k, bytes(uc.mem_read(other + 4 * k, 4)))
        if r.random() < 0.5:
            k1, k2 = real_k1, real_k2
        else:
            k1 = bytes(r.choice((0, 255)) for _ in range(16))
            k2 = bytes(r.choice((0, 255)) for _ in range(16))
        uc.mem_write(0x0162AC60, k1)
        uc.mem_write(0x0162AC30, k2)
        regs = regs_for(r, ecx=this)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), [other]))
    finish(out, '0x00767B40', cases)


def find_record(out, count):
    oracle = Oracle(0x004EA200, 0x57, {}, [(REGION, 0x4000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        this, records, key = REGION + 0x100, REGION + 0x1000, REGION + 0x3000
        n = r.choice((0, 1, 2, 3, 5, 8))
        uc.mem_write(REGION, bytes(r.randrange(256) for _ in range(0x4000)))
        small = lambda: r.choice((0, 1, 2, 0xFFFFFFFF, r.randrange(1 << 32)))
        words = [[small() for _ in range(5)] for _ in range(max(n, 1))]
        for i, rec in enumerate(words):
            uc.mem_write(records + 20 * i, struct.pack('<5I', *rec))
        pick = r.randrange(max(n, 1))
        mode = r.random()
        if mode < 0.5:
            k = list(words[pick])                       # a record: found at its first occurrence
        elif mode < 0.85:
            k = list(words[pick]); k[r.randrange(5)] ^= 1 << r.randrange(32)   # all but one word equal
        else:
            k = [small() for _ in range(5)]
        uc.mem_write(key, struct.pack('<5I', *k))
        for off, v in ((4, records), (8, n), (0x10, r.choice((0, 100, 0x1000, 0xFFFFFFFF)))):
            uc.mem_write(this + off, struct.pack('<I', v))
        regs = regs_for(r, ecx=this)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), [key]))
    finish(out, '0x004EA200', cases)


def advance_bits(out, count):
    oracle = Oracle(0x01328FD0, 0x24, {}, [(REGION, 0x1000), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        uc = oracle.uc
        reader = REGION + 0x100
        uc.mem_write(reader, bytes(r.randrange(256) for _ in range(0x20)))
        uc.mem_write(reader + 4, struct.pack('<I', r.choice((0, 1, 7, 8, r.randrange(8), r.randrange(1 << 32)))))
        bits = r.choice((0, 1, 7, 8, 9, -1, -7, -8, -9, r.randrange(-1 << 20, 1 << 20), r.randrange(-1 << 31, 1 << 31)))
        regs = regs_for(r)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), [reader, bits & 0xFFFFFFFF]))
    finish(out, '0x01328FD0', cases)


def main():
    out, count = Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    out.mkdir(parents=True, exist_ok=True)
    clear_grid(out / 'clear_grid.json', count)
    lanes_in_range(out / 'lanes_in_range.json', count)
    find_record(out / 'find_record.json', count)
    advance_bits(out / 'advance_bits.json', count)


main()
