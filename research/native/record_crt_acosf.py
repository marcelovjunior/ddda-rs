"""L184 s7 oracle: the CRT acosf (0x012F161F, SSE2 entry: x in xmm0, result in
xmm0) run as the original's code in Unicorn on random floats of every
magnitude class. Optional third argument: the entry (hex), e.g. 012F0D4C for asinf. Output: {tables: [[addr, [bytes]]...], cases: [[x_bits, result_bits]...]}
(the tables are the guest memory the routine reads, for the harness).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
import unicorn.x86_const as x

ENTRY = 0x012F161F if len(sys.argv) < 4 else int(sys.argv[3], 16)
TABLES = [(0x0144DE60, 0x810), (0x0144E660, 0x400), (0x0144EA60, 0x30), (0x0144C640, 0x810), (0x0144CE40, 0x400), (0x0144D240, 0x60), (0x01455A90, 0x300), (0x01455D10, 0x20)]

def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 30000
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    _, secs = h.image()
    for va, b in secs:
        start = va & ~0xFFF
        uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    uc.mem_map(0x0F000000, 0x10000)
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    rng = random.Random(11)
    bits_of = lambda f: struct.unpack('<I', struct.pack('<f', f))[0]
    cases = []
    for i in range(n):
        r = rng.random()
        if ENTRY == 0x012F4D01 and r < 0.6:
            b = bits_of(rng.choice((rng.uniform(-110, 110), rng.uniform(-5, 5), rng.uniform(-1e-3, 1e-3), rng.uniform(80, 90), rng.uniform(-90, -80), rng.uniform(-1000, 1000))))
        elif r < 0.55:
            b = bits_of(rng.uniform(-1.0, 1.0))
        elif r < 0.65:
            b = bits_of(rng.uniform(-0.1, 0.1))
        elif r < 0.72:
            b = bits_of(rng.choice((-1, 1)) * (1 - rng.random() * 0.01))
        elif r < 0.8:
            b = rng.randrange(1 << 32)
        elif r < 0.9:
            b = bits_of(rng.choice((-1, 1)) * 10 ** rng.uniform(-8, 0.2))
        else:
            b = rng.choice((0, 0x80000000, 1, 0x3f800000, 0xbf800000, 0x3f800001, 0x7f800000, 0xff800000, 0x7fc00000, 0x37800000, 0x377fffff, 0x3d800000, 0x3d7fffff, 0x3f7e0000, 0x3f7fffff, 0x3effffff, 0x3f000000, 0x3f0b3d7e, 0x3f0b3d7f, 0x3f0c0000))
        uc.reg_write(x.UC_X86_REG_ESP, 0x0F008000)
        uc.mem_write(0x0F008000, struct.pack('<I', h.RET))
        uc.reg_write(x.UC_X86_REG_XMM0, b)
        uc.emu_start(ENTRY, h.RET)
        cases.append([b, uc.reg_read(x.UC_X86_REG_XMM0) & 0xFFFFFFFF])
    tables = [[a, list(h.code(a, size))] for a, size in TABLES]
    json.dump(dict(tables=tables, cases=cases), open(out, 'w'))
    print(len(cases), 'cases')

main()
