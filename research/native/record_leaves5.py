"""Oracles for five small leaf functions (no guest calls), one JSON each in OUT_DIR (thiscall, ECX = object):

  init_record_c.json   0x00DE5CC0  initialiser (vtable 0x0142BB30), sibling of 0x00DE5BD0
  update_flag_bit4.json 0x00B76580 sets bit 4 of the flags at +0x1DDC and clears it again when bit 0 of +0x26BC is set; returns 0
  clear_entry_flag.json 0x00850410 clears bit 16 of the flags (+0x94) of each of the object's 0xA0-byte entries
  select_constant.json  0x00B85F70 one of seven float constants, chosen by a flag byte (+0x2F84) and the bits of a mode byte (+0x206D)
  init_vtables.json     0x004CA510 stores vtable pointers into a 24-slot array at 0x90 byte spacing (the object's base slots)

Run:  DDDA_EXE=reference/DDDA.exe python3 research/native/record_leaves5.py OUT_DIR [N]"""
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


def put(uc, addr, *values):
    uc.mem_write(addr, b''.join(struct.pack('<I', v & 0xFFFFFFFF) for v in values))


def run_cases(out, label, entry, length, count, setup, size=0x4000):
    oracle = Oracle(entry, length, {}, [(REGION, size), STACK])
    cases = []
    for seed in range(count):
        r = random.Random(seed)
        oracle.uc.mem_write(REGION, bytes(r.randrange(256) for _ in range(size)))
        this = setup(oracle.uc, r)
        regs = regs_for(r, ecx=this)
        cases.append(run_flags(oracle, seed, regs, SP - 0x20 - 4 * r.randrange(4), []))
    finish(out, label, cases)


def main():
    out, count = Path(sys.argv[1]), int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    out.mkdir(parents=True, exist_ok=True)
    this = REGION + 0x100

    run_cases(out / 'init_record_c.json', '0x00DE5CC0', 0x00DE5CC0, LEN_DE5CC0, count, lambda uc, r: this)

    def bit4(uc, r):
        put(uc, this + 0x1DDC, r.choice((0, 0x10, r.randrange(1 << 32))))
        put(uc, this + 0x26BC, r.choice((0, 1, 2, 3, r.randrange(1 << 32))))
        return this
    run_cases(out / 'update_flag_bit4.json', '0x00B76580', 0x00B76580, LEN_B76580, count, bit4)

    def entries(uc, r):
        n = r.choice((0, 1, 2, 3, 5))
        entries = REGION + 0x1000
        put(uc, this + 0x360, n)
        put(uc, this + 0x364, entries)
        for i in range(n):
            put(uc, entries + 0xA0 * i + 0x94, r.choice((0, 0x10000, 0xFFFFFFFF, r.randrange(1 << 32))))
        return this
    run_cases(out / 'clear_entry_flag.json', '0x00850410', 0x00850410, LEN_850410, count, entries)

    def constant(uc, r):
        uc.mem_write(this + 0x2F84, bytes([r.choice((0, 1, 2, r.randrange(256)))]))
        uc.mem_write(this + 0x206D, bytes([r.choice((0, 1, 2, 4, 8, 6, 10, 12, 14, 0xFF, r.randrange(256)))]))
        return this
    run_cases(out / 'select_constant.json', '0x00B85F70', 0x00B85F70, LEN_B85F70, count, constant, size=0x4000)

    run_cases(out / 'init_vtables.json', '0x004CA510', 0x004CA510, LEN_4CA510, count, lambda uc, r: REGION + 0x40, size=0x8000)


LEN_DE5CC0, LEN_B76580, LEN_850410, LEN_B85F70, LEN_4CA510 = 0x47, 0x2F, 0x2C, 0x50, 0x3E
main()
