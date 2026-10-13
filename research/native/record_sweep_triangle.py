"""L184 oracle: the triangle-simplex reduction inside 0x00D7CF70 (case 3 of
its simplex switch, 0x00D7D5AB..0x00D7D586): given a simplex of three
points (with witnesses) it keeps the feature (vertex, edge or interior)
closest to the origin and writes the closest point. Run as the original's
code in Unicorn from the case label to the common continuation, on random
and exactly-degenerate triangles, so that every Voronoi region (including
the reciprocal-or-zero corner cases) is exercised. Private output
(E:/ddda-re/notes/native-L184/sweep_triangle.json).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import os
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

START, END = 0x00D7D5AB, 0x00D7D586
SIMPLEX = 0x20005000
SP = 0x0F008000


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for page in (0xD7C000, 0xD7D000, 0xD7E000, 0x018CF000, 0x01406000, 0x01407000):
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.mem_map(0x20000000, 0x10000)
        uc.mem_map(0x0F000000, 0x10000)
        uc.hook_add(UC_HOOK_MEM_READ, lambda uc, a, address, size, v, d: self.remember(address, size))
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: uc.emu_stop(), begin=END, end=END)
        self.covered = set()
        if os.environ.get('COVER'):
            uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: self.covered.add(a))

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def coordinate(self, rng, mode):
        if mode == 0:
            return rng.uniform(-3, 3)
        if mode == 2:
            return float(rng.randrange(-1, 2))
        return float(rng.randrange(-3, 4))

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(SIMPLEX, bytes(rng.randrange(256) for _ in range(0x180)))
        mode = rng.choice((0, 0, 0, 1, 2))
        pts = [[self.coordinate(rng, mode) for _ in range(3)] for _ in range(3)]
        if rng.random() < 0.1:
            pts[2] = [(pts[0][k] + pts[1][k]) / 2 for k in range(3)]
        if rng.random() < 0.05:
            pts[1] = pts[0][:]
        if rng.random() < 0.05:
            pts[rng.randrange(3)] = [rng.choice((-1e20, 1e20, 0.0)) for _ in range(3)]
        for k, p in enumerate(pts):
            uc.mem_write(SIMPLEX + 0x10 + 16 * k, b''.join(f32(v) for v in p) + f32(0))
            for base in (0x70, 0xD0):
                uc.mem_write(SIMPLEX + base + 16 * k, b''.join(f32(rng.uniform(-2, 2)) for _ in range(3)) + f32(0))
        # (no signalling NaNs: Unicorn does not quiet them on FLD as hardware does)
        uc.mem_write(SIMPLEX, struct.pack('<I', 3))
        self.initial, self.written = {}, set()
        esp = SP - 0x400
        for r, v in ((x.UC_X86_REG_ESI, SIMPLEX), (x.UC_X86_REG_EBX, SIMPLEX + 0x10), (x.UC_X86_REG_EDI, SIMPLEX + 0x150), (x.UC_X86_REG_ESP, esp),
                     (x.UC_X86_REG_EAX, rng.randrange(1 << 32))):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        uc.emu_start(START, 0, count=20000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == END, hex(uc.reg_read(x.UC_X86_REG_EIP))
        # only the simplex record is compared (the original's locals are on its stack)
        initial = sorted((a, b) for a, b in self.initial.items() if SIMPLEX <= a < SIMPLEX + 0x200 or a >= 0x018CF000)
        after = [(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if SIMPLEX <= a < SIMPLEX + 0x200]
        return dict(seed=seed, initial=initial, after=after, eax_low=uc.reg_read(x.UC_X86_REG_EAX) & 0xFF)


def main():
    out, n = sys.argv[1], int(sys.argv[2])
    o = Oracle()
    cases = [o.case(s) for s in range(n)]
    if o.covered:
        every = [int(l.split(':')[0], 16) for l in open(os.environ['COVER']) if l[:8].strip() and ':' in l[:10]]
        every = [a for a in every if START <= a < 0x00D7DF18 or a == END]
        missing = [a for a in every if a not in o.covered]
        print(f'coverage {len(every) - len(missing)}/{len(every)}; missing', ' '.join(f'{a:x}' for a in missing))
    json.dump(dict(cases=cases), open(out, 'w'))
    print(len(cases), 'cases ->', out)


if __name__ == '__main__':
    main()
