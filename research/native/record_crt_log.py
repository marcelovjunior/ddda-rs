"""L184 s9 oracle: the CRT logf (0x012F45C4, x in xmm0 as f32) and log
(0x012F439E, x in xmm0 as f64) run as the original's code in Unicorn on
random inputs of every class. Output: {tables: [[addr, [bytes]]...],
cases: [[x_bits, result_bits]...]} (the tables are the guest memory the
routines read). Usage: record_crt_log.py <out.json> <n> <012F45C4|012F439E>
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86
import unicorn.x86_const as x

TABLES = [(0x01453F50, 0xD60)]

def main():
    out, n, entry = sys.argv[1], int(sys.argv[2]), int(sys.argv[3], 16)
    double = entry == 0x012F439E
    uc = Uc(UC_ARCH_X86, 4)
    _, secs = h.image()
    for va, b in secs:
        start = va & ~0xFFF
        uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    uc.mem_map(0x0F000000, 0x10000)
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    rng = random.Random(entry)
    cases = []
    def fbits(f):
        return struct.unpack('<I', struct.pack('<f', f))[0]
    def dbits(f):
        return struct.unpack('<Q', struct.pack('<d', f))[0]
    specials32 = (0, 0x80000000, 1, 0x7fffff, 0x800000, 0x3f800000, 0x3f800001, 0x3f7fffff, 0x7f800000, 0xff800000, 0x7fc00000, 0xffc00000, 0x7f7fffff, 0x7f800001, 0x00400000, 0x80000001, 0xbf800000, 0x3f3f0000, 0x3f3e0000, 0x3f410000, 0x3effffff, 0x40000000)
    specials64 = (0, 1 << 63, 1, 0xfffffffffffff, 0x10000000000000, dbits(1.0), dbits(1.0) + 1, dbits(1.0) - 1, 0x7ff0000000000000, 0xfff0000000000000, 0x7ff8000000000000, 0xfff8000000000000, 0x7ff0000000000001, 0x7fefffffffffffff, 0xbff0000000000000, dbits(2.0), dbits(0.5), 0x8000000000000001)
    for i in range(n):
        r = rng.random()
        if double:
            if r < 0.3:
                b = dbits(rng.uniform(0.5, 2.0))
            elif r < 0.5:
                b = dbits(10 ** rng.uniform(-300, 300))
            elif r < 0.6:
                b = rng.randrange(1 << 64)
            elif r < 0.7:
                b = dbits(10 ** rng.uniform(-320, -300))  # denormals
            elif r < 0.78:
                b = dbits(-(10 ** rng.uniform(-10, 10)))
            elif r < 0.85:
                b = dbits(rng.uniform(0.9, 1.1))
            elif r < 0.9:
                b = dbits(1.0 + rng.choice((-1, 1)) * 2.0 ** -rng.randrange(8, 53)) if rng.random() < 0.8 else dbits(1.0)
            else:
                b = rng.choice(specials64)
        else:
            if r < 0.3:
                b = fbits(rng.uniform(0.5, 2.0))
            elif r < 0.5:
                b = fbits(10 ** rng.uniform(-37, 37))
            elif r < 0.6:
                b = rng.randrange(1 << 32)
            elif r < 0.7:
                b = rng.randrange(1, 1 << 23)  # denormals
            elif r < 0.78:
                b = fbits(-(10 ** rng.uniform(-10, 10)))
            elif r < 0.85:
                b = fbits(rng.uniform(0.9, 1.1))
            elif r < 0.9:
                b = fbits(1.0 + rng.choice((-1, 1)) * 2.0 ** -rng.randrange(5, 24)) if rng.random() < 0.8 else fbits(1.0)
            else:
                b = rng.choice(specials32)
        uc.reg_write(x.UC_X86_REG_ESP, 0x0F008000)
        uc.mem_write(0x0F008000, struct.pack('<I', h.RET))
        uc.reg_write(x.UC_X86_REG_XMM0, b)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        uc.emu_start(entry, h.RET)
        v = uc.reg_read(x.UC_X86_REG_XMM0)
        cases.append([b, v & (0xFFFFFFFFFFFFFFFF if double else 0xFFFFFFFF)])
    tables = [[a, list(h.code(a, size))] for a, size in TABLES]
    json.dump(dict(tables=tables, cases=cases), open(out, 'w'))
    print(len(cases), 'cases')

main()
