"""L184 oracle: 0x00DE50B0 load_triangle_vertices (thiscall, ret 0xc) run as the
original's code in Unicorn (a leaf; x87 copies, quiet NaNs only because Unicorn
does not quiet signalling ones). Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00DE50B0, 0x00DE5124 - 0x00DE50B0
THIS, HEADER, VERTS, OUT = 0x20000000, 0x21000000, 0x22000000, 0x23000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for a, size in ((ENTRY & ~0xFFF, 0x1000), (THIS, 0x1000), (HEADER, 0x1000), (VERTS, 0x100000), (OUT, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        self.hit = set()

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
        else:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, b'\0' * 0x1000)
        uc.mem_write(OUT, bytes(rng.randrange(256) for _ in range(0x100)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v))
        w32(THIS + 0x38, HEADER)
        w32(THIS + 0x3C, VERTS)
        uc.mem_write(HEADER + 0xC, struct.pack('<HHH', *[rng.choice((0, 1, 5, rng.randrange(0x40), 0xFFFF)) for _ in range(3)]))
        def quiet(v):
            return v if (v & 0x7F800000) != 0x7F800000 or not (v & 0x7FFFFF) or v & 0x400000 else v | 0x400000
        uc.mem_write(VERTS, b''.join(struct.pack('<I', quiet(rng.randrange(1 << 32))) for _ in range(0x400)))
        # outputs may overlap each other or the vertices sometimes
        outs = [OUT + 0x20 * k for k in range(3)]
        if rng.random() < 0.2:
            outs[1] = outs[0] + rng.choice((0, 4, 8))
        if rng.random() < 0.15:
            outs[rng.randrange(3)] = VERTS + 16 * rng.randrange(0, 8)
        self.initial, self.written = {}, set()
        esp = SP - 16
        uc.mem_write(esp, struct.pack('<IIII', h.RET, *outs))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 16)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00DE50B0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    print(f'{n} cases -> {out}; {len(unhit)} unhit {unhit[:20]}')


main()
