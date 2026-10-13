"""L184 oracle: the three helpers of the swept capsule/triangle collision
(0x00D7CF70), each run alone as the original's code in Unicorn (ABI level;
no guest calls: 0x00D54AE0/0x00D53C70 run inline):

  support   0x00D63FF0  shape->support(direction)            thiscall ret 4
  add_point 0x00D1B540  simplex->add_point(p, a, b) -> added thiscall ret 0xc
  segment   0x00D12E10  simplex->closest_on_segment(out)     thiscall ret 4

Output: E:/ddda-re/notes/native-L184/{support,add_point,segment}.json (private).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

FUNCTIONS = {'support': (0xD63FF0, 1), 'add_point': (0xD1B540, 3), 'segment': (0xD12E10, 1)}
PAGES = [0xD63000, 0xD64000, 0xD1B000, 0xD12000, 0xD13000, 0xD53000, 0xD54000, 0x01406000, 0x01407000, 0x01408000, 0x0161C000]
SHAPE, CAPSULE, TRIANGLE, SIMPLEX, DIRECTION, POINT, WIT_A, WIT_B, OUT = (0x20000000 + 0x1000 * k for k in range(9))
SP = 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self, name):
        self.entry, self.nargs = FUNCTIONS[name]
        self.name = name
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for page in PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.mem_map(0x20000000, 0x10000)
        uc.mem_map(0x0F000000, 0x10000)
        uc.mem_map(h.RET, 0x1000)
        uc.hook_add(UC_HOOK_MEM_READ, lambda uc, a, address, size, v, d: self.remember(address, size))
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: uc.emu_stop(), begin=h.RET, end=h.RET)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def vec(self, rng, scale=2.0):
        r = rng.random()
        if r < 0.1:
            return [0.0, 0.0, 0.0]
        if r < 0.2:
            return [float(rng.randrange(-2, 3)) for _ in range(3)]
        return [rng.uniform(-scale, scale) for _ in range(3)]

    def put(self, address, values, pad=True):
        self.uc.mem_write(address, b''.join(f32(v) for v in values) + (f32(0.0) if pad else b''))

    def setup(self, rng):
        uc, name = self.uc, self.name
        uc.mem_write(0x20000000, bytes(0x10000))
        if name == 'support':
            a0 = self.vec(rng)
            a1 = self.vec(rng)
            tri = [self.vec(rng) for _ in range(3)]
            if rng.random() < 0.2:
                tri[1] = tri[0][:]
            self.put(CAPSULE, a0)
            self.put(CAPSULE + 0x10, a1)
            self.uc.mem_write(CAPSULE + 0x20, f32(rng.choice((0.0, rng.uniform(0, 1)))))
            for k in range(3):
                self.put(TRIANGLE + 0x10 * k, tri[k])
            uc.mem_write(SHAPE, struct.pack('<II', CAPSULE, TRIANGLE))
            uc.mem_write(SHAPE + 0x10, bytes(rng.randrange(256) for _ in range(0x20)))
            self.put(DIRECTION, self.vec(rng))
            return SHAPE, [DIRECTION]
        uc.mem_write(SIMPLEX, bytes(rng.randrange(256) for _ in range(0x180)))
        count = rng.choice((0, 1, 2, 2, 3, 3, 4, 5)) if name == 'add_point' else 2
        pts = [self.vec(rng, 1.0) for _ in range(5)]
        if rng.random() < 0.2:
            pts[1] = [v + rng.uniform(-0.002, 0.002) for v in pts[0]]
        if rng.random() < 0.2:
            pts[2] = [v * 2 for v in pts[0]]
        for k in range(5):
            self.put(SIMPLEX + 0x10 + 16 * k, pts[k])
            self.put(SIMPLEX + 0x70 + 16 * k, self.vec(rng))
            self.put(SIMPLEX + 0xD0 + 16 * k, self.vec(rng))
        uc.mem_write(SIMPLEX, struct.pack('<I', count))
        if name == 'segment':
            # origin projects before, onto or after the segment
            a = self.vec(rng, 2.0)
            d = self.vec(rng, 2.0)
            t = rng.choice((-1.0, 0.5, 2.0, rng.uniform(-1, 2), 0.0, 1.0))
            b = [a[k] + d[k] for k in range(3)]
            if rng.random() < 0.15:
                b = a[:]
            self.put(SIMPLEX + 0x10, a)
            self.put(SIMPLEX + 0x20, b)
            self.uc.mem_write(OUT, bytes(rng.randrange(256) for _ in range(16)))
            return SIMPLEX, [OUT]
        # add_point: the new point near an existing feature half of the time
        p = self.vec(rng, 1.0)
        r = rng.random()
        if r < 0.25 and count >= 1:
            p = [v + rng.uniform(-0.0015, 0.0015) for v in pts[0]]
        elif r < 0.4 and count == 2:
            p = [pts[0][k] + (pts[1][k] - pts[0][k]) * 0.4 + rng.uniform(-0.01, 0.01) for k in range(3)]
        elif r < 0.55 and count == 3:
            p = [pts[0][k] + (pts[1][k] - pts[0][k]) * 0.3 + (pts[2][k] - pts[0][k]) * 0.3 + rng.uniform(-0.001, 0.001) for k in range(3)]
        self.put(POINT, p)
        self.put(WIT_A, self.vec(rng))
        self.put(WIT_B, self.vec(rng))
        return SIMPLEX, [POINT, WIT_A, WIT_B]

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        self.initial, self.written = {}, set()
        this, args = self.setup(rng)
        esp = SP - 0x40 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', a) for a in args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4], start[1] = esp, this
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        self.remember(esp, 4 + 4 * self.nargs)
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=20000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 3000
    for name in FUNCTIONS:
        o = Oracle(name)
        cases = [o.case(s) for s in range(n)]
        out = f'E:/ddda-re/notes/native-L184/{name}.json'
        json.dump(dict(function=hex(o.entry), cases=cases), open(out, 'w'))
        ok = sum(1 for c in cases if c['after']['regs'][0] & 0xFF)
        print(name, n, 'cases', ok, 'with AL != 0 ->', out)


main()
