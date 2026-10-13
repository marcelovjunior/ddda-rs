"""L184 s6 oracle: the CRT's asinf (0x012F0D4C: x in xmm0 as a float, result
in xmm0) and double-precision atan2 (0x012F1109: y in xmm0, x in xmm1 as
doubles, result in xmm0) run as the original's code in Unicorn on random
arguments of every magnitude class (normal, tiny, denormal, huge, zero,
infinite, NaN, exact ratios).
Output: E:/ddda-re/notes/native-L184/crt_atan2.json
  {asinf: [[x_bits, result_bits], ...], atan2: [[y_bits, x_bits, result_bits], ...]}
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ASIN, ATAN2 = 0x012F0D4C, 0x012F1109
Q = 2 ** 64 - 1


def f32_bits(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def f64_bits(v):
    return struct.unpack('<Q', struct.pack('<d', v))[0]


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 30000
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    for va, b in h.image()[1]:
        lo, hi = va & ~0xFFF, (va + len(b) + 0xFFF) & ~0xFFF
        uc.mem_map(lo, hi - lo)
        uc.mem_write(va, b)
    uc.mem_map(0x0F000000, 0x10000)
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    uc.mem_write(h.RET, b'\xf4')
    rng = random.Random(11)
    covered = set()
    uc.hook_add(UC_HOOK_CODE, lambda uc, address, size, data: covered.add(address))

    def run(address, xmm0, xmm1=0):
        uc.reg_write(x.UC_X86_REG_ESP, 0x0F008000)
        uc.mem_write(0x0F008000, struct.pack('<I', h.RET))
        uc.reg_write(x.UC_X86_REG_XMM0, xmm0)
        uc.reg_write(x.UC_X86_REG_XMM1, xmm1)
        uc.emu_start(address, h.RET)
        return uc.reg_read(x.UC_X86_REG_XMM0) & Q

    specials32 = (0, 0x80000000, 1, 0x80000001, 0x007FFFFF, 0x00800000, 0x00800001, 0x7F800000, 0xFF800000, 0x7FC00000,
                  0xFFC00000, 0x7F800001, 0x3F800000, 0xBF800000, 0x3F7FFFFF, 0xBF7FFFFF, 0x3F800001, 0x3D800000,
                  0x3D7FFFFF, 0x3F5DB3D7, 0x3F5DB3D8, 0x3F5DB3D6, 0x3F7EFFFF, 0x3F7F0000, 0x3F7F0001, 0xBD800000,
                  0xBF5DB3D7, 0x32000000, 0x31FFFFFF, 0x33000000)
    asin_cases = []
    for i in range(n):
        r = rng.random()
        if r < 0.55:
            bits = f32_bits(rng.uniform(-1, 1))
        elif r < 0.7:
            bits = f32_bits(rng.uniform(-1, 1) * 10 ** rng.uniform(-9, 0))
        elif r < 0.8:
            bits = rng.randrange(1 << 32)
        elif r < 0.9:
            bits = rng.choice(specials32)
        else:
            # the neighbourhoods of the range boundaries
            base = rng.choice((0x3D800000, 0x3F5DB3D7, 0x3F7F0000, 0x3F800000, 0x32000000, 0x31800000))
            bits = (base + rng.randrange(-40, 40)) & 0xFFFFFFFF | (rng.randrange(2) << 31)
        asin_cases.append([bits, run(ASIN, bits) & 0xFFFFFFFF])

    specials64 = (0.0, -0.0, 1.0, -1.0, float('inf'), float('-inf'), float('nan'), 5e-324, -5e-324, 2.2250738585072014e-308,
                  1e-300, 1e300, 1.7976931348623157e308, 3.141592653589793, 0.5, 2.0, 1e-160, 1e160)
    atan_cases = []
    for i in range(n):
        r = rng.random()

        def one():
            m = rng.random()
            if m < 0.5:
                return rng.uniform(-100, 100)
            if m < 0.75:
                return rng.uniform(-1, 1) * 10 ** rng.uniform(-30, 30)
            if m < 0.85:
                return rng.choice(specials64)
            if m < 0.9:
                return struct.unpack('<d', struct.pack('<Q', rng.randrange(1 << 64)))[0]
            return rng.choice((-1, 1)) * 2.0 ** rng.randrange(-1074, 1024) if m < 0.95 else rng.uniform(0.5, 2) * 2.0 ** rng.randrange(-60, 60)
        y, xv = one(), one()
        if r < 0.15:  # ratios near 1 and powers of two
            xv = rng.choice((-1, 1)) * 2.0 ** rng.randrange(-40, 40)
            y = xv * rng.choice((1.0, 1.0 + 2 ** -52, 1.0 - 2 ** -53, 0.5, 2.0, 1e-3, 1e3, rng.uniform(0.9, 1.1)))
        if 0.15 <= r < 0.2:  # infinities against everything
            y, xv = rng.choice((float('inf'), float('-inf'))), rng.choice((float('inf'), float('-inf'), float('nan'), 1.0, 0.0))
        yb, xb = f64_bits(y), f64_bits(xv)
        atan_cases.append([yb, xb, run(ATAN2, yb, xb)])
    if len(sys.argv) > 3:  # disassembly listings of the two routines: instruction starts never run
        starts = [int(l.split(':')[0], 16) for f in sys.argv[3:] for l in open(f) if l[:8].strip() and l[8:9] == ':']
        print('never run:', [hex(a) for a in starts if a not in covered])
    table = h.code(0x0144C640, 0x0144DE60 - 0x0144C640)  # the CRT tables the routines read (private: never committed)
    json.dump(dict(asinf=asin_cases, atan2=atan_cases, table_base=0x0144C640, table=list(table)), open(out, 'w'))
    print(len(asin_cases), 'asinf cases,', len(atan_cases), 'atan2 cases')


main()
