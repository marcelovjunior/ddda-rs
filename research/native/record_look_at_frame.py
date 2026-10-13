"""L184 oracle: 0x01130A00 `build_look_at_frame(out, axis, up, position)`
(cdecl, plain `ret`), run as the original's code in Unicorn.

Row 0 of `out` is the normalised `axis`, row 2 the normalised `axis x up`,
row 1 `row2 x row0`, row 3 the position; when `axis x up` is shorter than
the epsilon only the position row is written. Inputs cover parallel, zero,
tiny and huge vectors. Reads the user's own exe; output is private
(E:/ddda-re/notes/native-L184). Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x01130A00, 0x1B1  # through RET at 0x01130BB0
OUT, AXIS, UP, POS = 0x20000000, 0x20001000, 0x20002000, 0x20003000
SP = 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
CONST_PAGES = (0x01406000, 0x01407000)


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x1000), (0x20000000, 0x4000), (0x0F000000, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for page in CONST_PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)

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

    def vector(self, rng):
        kind = rng.random()
        if kind < 0.1:
            return [0.0, 0.0, 0.0]
        if kind < 0.2:
            return [rng.uniform(-1e-8, 1e-8) for _ in range(3)]
        if kind < 0.3:
            return [rng.uniform(-1e5, 1e5) for _ in range(3)]
        return [rng.uniform(-2, 2) for _ in range(3)]

    def case(self, seed):
        rng = random.Random(seed)
        uc = self.uc
        axis = self.vector(rng)
        r = rng.random()
        if r < 0.15:
            k = rng.uniform(-3, 3)
            up = [v * k for v in axis]  # parallel
        elif r < 0.2:
            up = [rng.choice((0.0, 1.0, -1.0)) for _ in range(3)]
        else:
            up = self.vector(rng)
        position = [rng.uniform(-100, 100) for _ in range(3)] + [rng.choice((1.0, 0.0, 2.5))]
        # the out matrix starts with junk (a degenerate call leaves rows 0..2 alone)
        uc.mem_write(OUT, bytes(rng.randrange(256) for _ in range(0x40)))
        for base, values in ((AXIS, axis + [0.0]), (UP, up + [0.0]), (POS, position)):
            uc.mem_write(base, b''.join(f32(v) for v in values))
        self.initial, self.written = {}, set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<IIIII', h.RET, OUT, AXIS, UP, POS))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for reg, v in zip(REGS, start):
            uc.reg_write(reg, v)
        self.remember(esp, 20)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=10000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    full = sum(1 for c in cases if len(c['after']['memory']) > 16)
    json.dump(dict(function='0x01130A00', cases=cases), open(out, 'w'))
    print(f'{n} cases ({full} with a full frame) -> {out}')


main()
