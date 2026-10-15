"""Oracles for eight small leaf functions (no guest calls), one JSON each in OUT_DIR:

  xorshift_step.json      0x00D00690  thiscall: one step of the engine's xorshift128 generator (state at +0..+0xC)
  table_entry_or_default  0x00FF9A20  thiscall `ret 4`: entry for a byte index, or the default entry at +0xB0
  table_entry_or_null     0x00421ED0  thiscall `ret 4`: entry for a byte index, or null
  fix_state_flags         0x005D8EB0  thiscall: promotes state 1/2 of a flag word to 3 unless bit 13 is set
  find_entry_by_key       0x00E0A4E0  thiscall `ret 4`: entry of 16 bytes with the given first dword, or null
  init_record_a           0x00DE5BD0  thiscall: constructor-style initialiser (vtable 0x1558134)
  init_record_b           0x00DE0AA0  thiscall: constructor-style initialiser (vtable 0x142B73C)
  set_float_if_changed    0x01105EF0  thiscall `ret 8`: stores a float and its predecessor when it changed or is forced

Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_leaves3.py OUT_DIR [N]"""
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
NAMES = 'eax ecx edx ebx esp ebp esi edi'.split()


def run_flags(oracle, *args):
    """Oracle.run plus the EFLAGS the original leaves (arithmetic flags are compared by the tests)."""
    import unicorn.x86_const as ux
    case = oracle.run(*args)
    case['after']['eflags'] = oracle.uc.reg_read(ux.UC_X86_REG_EFLAGS)
    return case


def regs_for(r, **fixed):
    regs = [r.randrange(1 << 32) for _ in range(8)]
    for name, v in fixed.items():
        regs[NAMES.index(name)] = v
    return regs


def word(r):
    return r.choice((0, 1, 2, 3, 4, 7, 0x2000, 0x1FFF, 0xFFFFFFFF, 0x80000000, r.randrange(1 << 32), r.randrange(1 << 32)))


def run_cases(out, label, entry, length, count, setup, extra_maps=()):
    oracle = Oracle(entry, length, {}, [(REGION, 0x4000), STACK, *extra_maps])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        oracle.uc.mem_write(REGION, bytes(r.randrange(256) for _ in range(0x4000)))
        this, args, fixed = setup(oracle.uc, r)
        regs = regs_for(r, ecx=this, **fixed)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), args))
    finish(out, label, cases)


def put(uc, addr, *values):
    uc.mem_write(addr, b''.join(struct.pack('<I', v & 0xFFFFFFFF) for v in values))


def main():
    out, count = Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    out.mkdir(parents=True, exist_ok=True)
    this = REGION + 0x100

    def xorshift(uc, r):
        put(uc, this, *[word(r) for _ in range(4)])
        return this, [], {}
    run_cases(out / 'xorshift_step.json', '0x00D00690', 0x00D00690, 0x2E, count, xorshift)

    def table(uc, r):
        entries, index_table = REGION + 0x1000, REGION + 0x800
        put(uc, this + 0x364, entries)
        put(uc, this + 0x368, index_table)
        for k in range(256):
            uc.mem_write(index_table + k, bytes([r.choice((0xFF, 0xFF, 0, 1, 2, r.randrange(256)))]))
        # an index above 0xFF, negative, or in range
        index = r.choice((-1, -5, 0, 1, 2, 0x100, 0x1FF, 0x7FFFFFFF, r.randrange(256), r.randrange(256), r.randrange(-300, 300)))
        return this, [index & 0xFFFFFFFF], {}
    run_cases(out / 'table_entry_or_default.json', '0x00FF9A20', 0x00FF9A20, 0x3A, count, table)
    run_cases(out / 'table_entry_or_null.json', '0x00421ED0', 0x00421ED0, 0x2E, count, table)

    def flags(uc, r):
        put(uc, this + 4, r.choice((word(r), (word(r) & ~0x2007) | r.choice((0, 1, 2, 3, 4, 5, 6, 7)), word(r) | 0x2000)))
        return this, [], {}
    run_cases(out / 'fix_state_flags.json', '0x005D8EB0', 0x005D8EB0, 0x26, count, flags)

    def entries(uc, r):
        holder, table_obj, array = REGION + 0x400, REGION + 0x600, REGION + 0x1000
        n = r.choice((0, 1, 2, 3, 6))
        keys = [word(r) for _ in range(max(n, 1))]
        put(uc, this + 0xFC, r.choice((holder, holder, holder, 0)))
        put(uc, holder + 0x60, table_obj)
        put(uc, table_obj + 0xB8, array)
        put(uc, table_obj + 0x48, n)
        for i, k in enumerate(keys):
            put(uc, array + 16 * i, k)
        key = r.choice((keys[r.randrange(len(keys))], word(r)))
        return this, [key], {}
    run_cases(out / 'find_entry_by_key.json', '0x00E0A4E0', 0x00E0A4E0, 0x33, count, entries)

    def blank(uc, r):
        return this, [], {}
    run_cases(out / 'init_record_a.json', '0x00DE5BD0', 0x00DE5BD0, 0x35, count, blank)
    run_cases(out / 'init_record_b.json', '0x00DE0AA0', 0x00DE0AA0, 0x37, count, blank)

    def setfloat(uc, r):
        def f():
            return r.choice((0.0, -0.0, 1.0, -1.0, 0.5, float('nan'), float('inf'), r.uniform(-4, 4)))
        old = f()
        new = r.choice((old, f()))
        bits = lambda v: struct.pack('<f', v) if v == v else struct.pack('<I', 0x7FC00000)
        uc.mem_write(this + 0xC, bits(old))
        put(uc, this + 8, word(r))
        flag = r.choice((0, 1, 1, 2, r.randrange(256)))
        return this, [struct.unpack('<I', bits(new))[0], flag], {}
    run_cases(out / 'set_float_if_changed.json', '0x01105EF0', 0x01105EF0, 0x2F, count, setfloat)


main()
