"""L184 oracle: 0x00D7CF70 `sweep(owner = ECX, shape, velocity, simplex,
hit_a, hit_b) -> hit` (thiscall, `ret 0x14`), run as the original's code
in Unicorn, together with its real callees run inline (no events):
0x00D63FF0 (support points of the shape's capsule and triangle),
0x00D1B540 (simplex insertion; its segment branch calls 0x00D54AE0 and
0x00D53C70, x87), 0x00D12E10 (closest point of a segment simplex).
The tetrahedron step 0x00D8E400 (2070 instructions) is a stub recorded as
a guest call: it writes the closest point (+0x150), sometimes shrinks the
simplex, and returns a random boolean.

The shape: [0] -> a capsule (two points at +0 and +0x10, radius at +0x20),
[4] -> a triangle (points at +0, +0x10, +0x20); the support results go to
shape +0x10 and +0x20. Reads the user's own exe; output is private
(E:/ddda-re/notes/native-L184). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
Set COVER=<file of Ghidra disassembly lines> to print instruction coverage.
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

ENTRY = 0x00D7CF70
TETRA = 0x00D8E400  # stubbed
CODE_PAGES = [0xD7C000, 0xD7D000, 0xD7E000, 0xD63000, 0xD64000, 0xD1B000, 0xD12000, 0xD13000, 0xD53000, 0xD54000, TETRA & ~0xFFF]
DATA_PAGES = [0x018CF000, 0x01629000, 0x01406000, 0x01407000, 0x01408000, 0x0161C000]
OWNER, SHAPE, CAPSULE, TRIANGLE, VELOCITY, SIMPLEX, OUT_A, OUT_B = (0x20000000 + 0x1000 * k for k in range(8))
SP = 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for page in CODE_PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        for page in DATA_PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.mem_map(0x20000000, 0x10000)
        uc.mem_map(0x0F000000, 0x10000)
        uc.mem_map(h.RET, 0x1000)
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.tetra, begin=TETRA, end=TETRA)
        uc.hook_add(UC_HOOK_CODE, self.finish, begin=h.RET, end=h.RET)
        self.covered = set()
        if os.environ.get('COVER'):
            uc.hook_add(UC_HOOK_CODE, lambda uc, a, s, d: self.covered.add(a))

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            self.initial.setdefault(address + i, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.remember(address, size)
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def finish(self, uc, address, size, data):
        uc.emu_stop()

    def put(self, address, data):
        """A guest write made by the stub (recorded like a hooked write)."""
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))

    def tetra(self, uc, address, size, data):
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 24))]
        self.remember(esp, 24)
        before = dict(regs=self.regs(), memory=window)
        back, out = struct.unpack('<II', uc.mem_read(esp, 8))
        simplex = uc.reg_read(x.UC_X86_REG_ECX)
        rng = self.rng
        point = [rng.uniform(-1, 1) for _ in range(3)] + [0.0]
        mark = len(self.written)
        self.put(out, b''.join(f32(v) for v in point))
        if rng.random() < 0.4:
            self.put(simplex, struct.pack('<I', rng.randrange(1, 4)))
        eax = rng.choice((0, 1)) | (rng.randrange(1 << 24) << 8) * rng.choice((0, 1))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 24)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        written = sorted(self.written)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in written])
        self.events.append(dict(target=address, before=before, after=after))

    def vector(self, rng, scale):
        return [rng.uniform(-scale, scale) for _ in range(3)]

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(0x20000000, bytes(0x10000))
        # junk the original must overwrite
        uc.mem_write(SIMPLEX, bytes(rng.randrange(256) for _ in range(0x180)))
        scale = rng.choice((1, 2, 4))
        tri = [self.vector(rng, scale) for _ in range(3)]
        if rng.random() < 0.1:
            tri[2] = [v * 2 for v in tri[0]]  # degenerate triangles
        if rng.random() < 0.05:
            tri[1] = tri[0][:]
        a0 = self.vector(rng, scale)
        a1 = [v + rng.uniform(-1, 1) for v in a0]
        if rng.random() < 0.1:
            a1 = a0[:]
        radius = rng.choice((0.0, rng.uniform(0, 0.5), rng.uniform(0, 2)))
        # sometimes the capsule is placed on a triangle vertex or edge
        if rng.random() < 0.15:
            a0 = [v + rng.uniform(-0.01, 0.01) for v in tri[rng.randrange(3)]]
        mem = lambda a, vs: uc.mem_write(a, b''.join(f32(v) for v in vs))
        mem(CAPSULE, a0 + [0.0] + a1 + [0.0, radius])
        mem(TRIANGLE, tri[0] + [0.0] + tri[1] + [0.0] + tri[2])
        uc.mem_write(SHAPE, struct.pack('<II', CAPSULE, TRIANGLE))
        kind = rng.random()
        if kind < 0.1:
            velocity = [0.0, 0.0, 0.0]
        elif kind < 0.2:
            velocity = [rng.uniform(-1e-6, 1e-6) for _ in range(3)]
        else:
            velocity = self.vector(rng, rng.choice((0.5, 3, 20)))
        mem(VELOCITY, velocity)
        limit = rng.choice((0, 1, 2, 3, 5, 30, 30, 30, 100))
        tolerance = rng.choice((1e-10, 1e-6, 1e-6, 1e-3, 0.0, 0.5))
        uc.mem_write(OWNER + 0x190, struct.pack('<I', limit) + f32(tolerance))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 0x40 - 4 * rng.randrange(4)
        outs = [rng.choice((0, OUT_A)), rng.choice((0, OUT_B))]
        uc.mem_write(esp, struct.pack('<IIIIII', h.RET, SHAPE, VELOCITY, SIMPLEX, *outs))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        start[1] = OWNER
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        self.remember(esp, 24)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=400000)
        except Exception as e:
            raise RuntimeError(f'seed {seed} eip {uc.reg_read(x.UC_X86_REG_EIP):x}') from e
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    hits = sum(1 for c in cases if c['after']['regs'][0] & 0xFF)
    calls = sum(len(c['events']) for c in cases)
    if oracle.covered:
        every = [int(line.split(':')[0], 16) for line in open(os.environ['COVER']) if line[:8].strip() and ':' in line[:10]]
        missing = [a for a in every if a not in oracle.covered]
        print(f'coverage {len(every) - len(missing)}/{len(every)} instructions; missing:', ' '.join(f'{a:x}' for a in missing[:120]))
    json.dump(dict(function='0x00D7CF70', cases=cases), open(out, 'w'))
    print(f'{n} cases ({hits} hits), {calls} tetra calls -> {out}')


if __name__ == "__main__":
    main()
