"""L184 s7 oracle: the CRT sinf / cosf (0x012F0906 / 0x012F0A55, SSE2 entries,
x in xmm0, result in xmm0) run as the original's code in Unicorn on random
floats of every magnitude class (normal, tiny, denormal, huge, inf, NaN).
Output: E:/ddda-re/notes/native-L184/crt_trig.json {cases: [[kind, x_bits, result_bits], ...]}.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32
import unicorn.x86_const as x

SIN, COS = 0x012F0906, 0x012F0A55

def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 20000
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    _, secs = h.image()
    for va, b in secs:
        start = va & ~0xFFF
        uc.mem_map(start, ((va + len(b) - start) + 0xFFF) & ~0xFFF)
        uc.mem_write(va, b)
    uc.mem_map(0x0F000000, 0x10000)
    uc.mem_map(h.RET & ~0xFFF, 0x1000)
    uc.mem_write(h.RET, b'\xf4')
    rng = random.Random(7)
    cases = []
    for i in range(n):
        r = rng.random()
        if r < 0.5:
            bits = struct.unpack('<I', struct.pack('<f', rng.uniform(-400, 400)))[0]
        elif r < 0.6:
            bits = struct.unpack('<I', struct.pack('<f', rng.uniform(-8192, 8192)))[0]
        elif r < 0.7:
            bits = rng.randrange(1 << 32)
        elif r < 0.78:
            bits = rng.choice((0, 0x80000000, 1, 0x80000001, 0x007fffff, 0x00800000, 0x00800001, 0x80800000, 0x7f800000, 0xff800000, 0x7fc00000, 0xffc00000, 0x7f800001, 0x45ffffff, 0x46000000, 0xc5ffffff, 0xc6000000))
        else:
            bits = struct.unpack('<I', struct.pack('<f', rng.uniform(-8, 8) * 10 ** rng.randrange(-8, 4)))[0]
        x_f = struct.unpack('<f', struct.pack('<I', bits))[0]
        if abs(x_f) >= 8192 and abs(x_f) != float('inf') and x_f == x_f:
            continue  # the x87 range: a guest call in the port
        kind = i % 2
        uc.reg_write(x.UC_X86_REG_ESP, 0x0F008000)
        uc.mem_write(0x0F008000, struct.pack('<I', h.RET))
        uc.reg_write(x.UC_X86_REG_XMM0, bits)
        uc.emu_start(SIN if kind == 0 else COS, h.RET)
        res = uc.reg_read(x.UC_X86_REG_XMM0) & 0xFFFFFFFF
        cases.append([kind, bits, res])
    json.dump(dict(cases=cases), open(out, 'w'))
    print(len(cases), 'cases')

main()
