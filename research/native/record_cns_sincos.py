"""L184 s5 oracle: the CRT sinf (0x012F0906) and cosf (0x012F0A55) that
0x00860AC0 calls, run as the original's code in Unicorn on float bit
patterns that cover every branch of their SSE2 path: normal arguments (the
reduction by multiples of pi/32), denormals and zeros, infinities and NaNs.
Arguments of 8192 radians or more leave the SSE2 path for an x87 range
reduction (0x012F68B0 / 0x012F69E0, not mapped here): they are recorded as
`slow` and the port is only checked to hand them to the guest.
The vector file carries the two 1 KiB tables the functions read (as hex).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UcError  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

SIN, COS = 0x012F0906, 0x012F0A55
SIN_TABLE, COS_TABLE = 0x0144B410, 0x0144B870
SP = 0x0F008000


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 20000
    uc = Uc(UC_ARCH_X86, UC_MODE_32)
    for page in (0x012F0000, 0x0144B000, 0x0F000000, h.RET & ~0xFFF):
        uc.mem_map(page, 0x10000 if page == 0x0F000000 else 0x1000)
    uc.mem_write(0x012F0000, h.code(0x012F0000, 0x1000))
    uc.mem_write(0x0144B000, h.code(0x0144B000, 0x1000))
    rng = random.Random(184)
    cases = []
    for i in range(n):
        r = rng.random()
        if r < 0.5:
            v = rng.uniform(-8190, 8190) * rng.choice((1, 1, 1e-3, 1e-6))
        elif r < 0.6:
            k = rng.randrange(-2000, 2000)
            v = k * 3.14159265358979 / 32 + rng.choice((0, 1e-7, -1e-7, 1e-4))
        elif r < 0.7:
            v = rng.choice((0.0, -0.0, 1e-40, -1e-40, 1e-45, 1e-38, -1e-38, 1.17549435e-38, 2e-38, 1e-30))
        elif r < 0.75:
            v = rng.choice((float('inf'), -float('inf'), float('nan')))
        elif r < 0.85:
            v = rng.uniform(8192, 1e6) * rng.choice((1, -1))  # x87 range
        elif r < 0.9:
            v = rng.choice((8191.999, 8192.0, -8192.0, 8200.0, 1e30, -1e30, 3.4e38))
        else:
            v = rng.uniform(-10, 10)
        bits = struct.unpack('<I', struct.pack('<f', v))[0]
        if r > 0.97:
            bits = rng.randrange(1 << 32)  # raw patterns (NaN payloads, denormals, anything)
        for name, entry in (('sin', SIN), ('cos', COS)):
            esp = SP - 4
            uc.mem_write(esp, struct.pack('<I', h.RET))
            uc.reg_write(x.UC_X86_REG_ESP, esp)
            uc.reg_write(x.UC_X86_REG_XMM0, bits)
            slow = False
            try:
                uc.emu_start(entry, h.RET, count=1000)
                res = struct.unpack('<I', bytes(uc.reg_read(x.UC_X86_REG_XMM0).to_bytes(16, 'little')[:4]))[0]
            except UcError:
                slow, res = True, None
            cases.append(dict(fn=name, x=bits, out=res, slow=slow))
    tables = dict(sin=bytes(uc.mem_read(SIN_TABLE, 0x400)).hex(), cos=bytes(uc.mem_read(COS_TABLE, 0x400)).hex())
    json.dump(dict(function='0x012F0906/0x012F0A55', tables=tables, cases=cases), open(out, 'w'))
    print(f'{len(cases)} cases ({sum(c["slow"] for c in cases)} slow) -> {out}')


main()
