"""L184 s9 oracle: 0x00D8E400 `simplex->tetrahedron_step(closest_out, v0, v1,
v2, v3)` (the closest point of a tetrahedron to the origin, the GJK step
s8's sweep calls) run as the original's code in Unicorn on synthetic
simplexes. The exe image supplies the constants (the zero vector at
0x018CFE80 and the three at 0x01629520). No guest calls. Format of
crates/mtcore/tests/semantic_s9.rs `abi_cases_full`.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00D8E400, 0x24FC  # through RET 0x14 at 0x00D908F9
THIS, THIS_SIZE = 0x20000000, 0x1000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
COORD = (-2.0, -1.0, -0.5, 0.0, 0.0, 0.5, 1.0, 2.0)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, sections = h.image()
        for va, bytes_ in sections:
            start = va & ~0xFFF
            uc.mem_map(start, ((va + len(bytes_) - start) + 0xFFF) & ~0xFFF)
            uc.mem_write(va, bytes_)
        for address, size in ((THIS, THIS_SIZE), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x14\x00', h.code(ENTRY + LENGTH - 3, 3)
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

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

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        assert ENTRY <= address < ENTRY + LENGTH, hex(address)
        self.visited.add(address)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, rng.randbytes(THIS_SIZE))
        mode = rng.random()

        def coordinate():
            if mode < 0.5:
                return rng.choice(COORD)
            if mode < 0.85:
                return rng.uniform(-3, 3)
            return rng.choice(COORD) if rng.random() < 0.5 else rng.uniform(-3, 3)

        # the four vertices (xyz, w) in the simplex record, and their two
        # witness records, which the step copies around
        for i in range(4):
            point = [coordinate(), coordinate(), coordinate(), 0.0]
            if rng.random() < 0.08:
                point = list(rng.choice(([0.0] * 3 + [0.0]), [1e-30, 0.0, 0.0, 0.0], [1e30, 1e30, 1e30, 0.0]))
            uc.mem_write(THIS + 0x10 + 16 * i, struct.pack('<4f', *point))
        for i in range(4):
            for base in (0x70, 0xD0):
                uc.mem_write(THIS + base + 16 * i, struct.pack('<4f', *[rng.uniform(-3, 3) for _ in range(3)], 0.0))
        self.initial, self.written, self.visited = {}, set(), set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        args = [THIS + 0x150, THIS + 0x10, THIS + 0x20, THIS + 0x30, THIS + 0x40]
        uc.mem_write(esp, struct.pack("<6I", h.RET, *args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        uc.reg_write(x.UC_X86_REG_FPCW, 0x37F)
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        self.remember(esp, 24)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET, 'instruction limit'
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 6000
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    json.dump(dict(function='0x00D8E400', cases=cases), open(out, 'w'))
    print(f'{n} cases, {len(visited)} instruction addresses visited -> {out}')
    open(out + '.visited', 'w').write('\n'.join('%08x' % a for a in sorted(visited)))


main()
