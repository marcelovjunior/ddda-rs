"""L184 oracle: 0x007781A0 gauge object tick (register convention EAX=object,
plain ret) run as the original's code in Unicorn on randomized objects (a leaf:
no callees). Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x007781A0, 0x00778AB4 - 0x007781A0
OBJ, OWNER = 0x20000000, 0x21000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
POOL = [0.0, 0.5, 1.0, 2.0, 3.0, -1.0, -0.0, 0.25, 10.0]
NAN = 0x7FC00000


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for a, size in ((ENTRY & ~0xFFF, 0x1000), (0x015319B8 & ~0xFFF, 0x1000), (OBJ, 0x1000), (OWNER, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(0x015319B8, struct.pack('<f', 1.0))
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
        uc.mem_write(OBJ, b'\0' * 0x1000)
        uc.mem_write(OWNER, b'\0' * 0x1000)
        f = lambda v: struct.pack('<f', v)
        pick = lambda nan=False: (struct.pack('<I', NAN) if nan and rng.random() < 0.08 else f(rng.choice(POOL)))
        for off in (0x10, 0x14, 0x18, 0x1C, 0x20, 0x24, 0x38, 0x3C, 0x44, 0x48, 0x1C4, 0x1C8, 0x1CC, 0x1D0, 0x1D4, 0x1D8):
            uc.mem_write(OBJ + off, pick(nan=off in (0x14, 0x20, 0x3C, 0x48, 0x1CC, 0x1D0, 0x1D4, 0x1D8)))
        for i in range(20):
            uc.mem_write(OBJ + 0x70 + 4 * i, pick())
            uc.mem_write(OBJ + 0x114 + 4 * i, pick())
        uc.mem_write(OBJ + 0x34, bytes([rng.choice((0, 1, 1, 2))]))
        uc.mem_write(OBJ + 0x40, bytes([rng.choice((0, 1, 1, 2))]))
        owner = rng.choice((0, OWNER, OWNER))
        uc.mem_write(OBJ + 0x1B4, struct.pack('<I', owner))
        uc.mem_write(OWNER + 0x14, f(rng.choice([0.5, 1.0, 2.0, 0.0, 3.0])))
        source = rng.choice((0, OWNER, OWNER))
        uc.mem_write(OBJ + 0x1B8, struct.pack('<I', source))
        uc.mem_write(OWNER + 0xD14, struct.pack('<I', rng.choice((0, 1, 1, 2, 7))))
        uc.mem_write(OBJ + 0x1BC, struct.pack('<I', rng.choice((0, 1, 2, 3, 4))))
        uc.mem_write(OBJ + 0x1C0, struct.pack('<I', rng.randrange(8)))
        uc.mem_write(OBJ + 0x1E0, struct.pack('<i', rng.choice((0, 1, 2, -1, -3, 4, 5, -100))))
        self.initial, self.written = {}, set()
        esp = SP - 4
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[0], start[4] = OBJ, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=[], after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x007781A0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    print(f'{n} cases -> {out}; {len(unhit)} unhit {unhit[:20]}')


main()
