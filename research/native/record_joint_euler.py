"""L184 s5 oracle: 0x00DEB960 `joint->euler_angles(out)` (thiscall, ECX = joint,
stack argument `out`, `ret 4`) run as the original's code in Unicorn, together
with the six rotation-order routines it dispatches to (0x00CFC9E0, 0x00CFCB00,
0x00CFCC20, 0x00CFCD40, 0x00CFCF80, 0x00CFCE60) and the CRT's atan2/asinf.
Quaternions include exact axis-aligned and half-turn ones so the gimbal-lock
poles (+-1 pivots) are reached; orders above 5 take the default.
Run from research/native with E:/ddda-re/tools/pyenv/Scripts/python.exe;
output is private.
"""
import struct
import sys

import cns_oracle as o

ENTRY, LENGTH = 0x00DEB960, 0x234
ROUTINES = [(0x00CFC9E0, 0x113), (0x00CFCB00, 0x117), (0x00CFCC20, 0x117), (0x00CFCD40, 0x114), (0x00CFCF80, 0x113), (0x00CFCE60, 0x118)]
THIS, OUT, GLOBAL = 0x20000000, 0x22000000, 0x018CF000
EXACT = (0.0, 0.0, 1.0, -1.0, 0.5, -0.5, 0.70710678, -0.70710678)


def setup(self, rng):
    w = self.uc.mem_write
    f = o.f32
    if rng.random() < 0.6:
        q = [rng.choice(EXACT) for _ in range(4)]
    else:
        q = [rng.uniform(-1, 1) for _ in range(4)]
    w(THIS + 0x50, b''.join(f(v) for v in q))
    w(THIS + 0x38, struct.pack('<I', (rng.randrange(1 << 16) << 16) | rng.choice((0, 1, 2, 3, 4, 5, 5, 6, 7, 0xFFFF, 0x100, 0x102, 0x205))))
    w(GLOBAL + 0xE80, b''.join(f(rng.uniform(-3, 3)) for _ in range(4)))
    esp = o.SP - 8 - 4 * rng.randrange(0, 64)
    regs = [rng.randrange(1 << 32) for _ in range(8)]
    regs[1], regs[4] = THIS, esp
    return regs, [OUT + 0x10 * rng.randrange(0, 4)]


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    arenas = [(THIS, 0x1000), (OUT, 0x1000), (GLOBAL, 0x1000)]
    code = [(ENTRY, LENGTH)] + ROUTINES + [(0x012F0D4C, 0x3BD), (0x012F1109, 0x516)]
    oracle = o.Oracle(code, arenas, data_pages=o.DATA_PAGES + (0x0144C000, 0x0144D000, 0x01606000))
    oracle.record('0x00DEB960', ENTRY, setup, out, n)


main()
