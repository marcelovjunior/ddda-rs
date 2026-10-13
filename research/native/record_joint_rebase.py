"""L184 oracle: 0x0081FD10 (EDI = object, two byte flags on the stack,
`ret 8`) run as the original's code in Unicorn on synthetic joint tables.
The only guest callee, 0x00421ED0 (thiscall, one stack argument, `ret 4`),
is a stub returning a 16-byte-aligned matrix object or 0. Constants are
the exe's own bytes. Events use the format of crates/mtcore/tests/
semantic.rs `abi_cases_full`. Floats stay in a range where no operation
overflows (the emulator's default NaN for invalid operations is positive,
the hardware's negative).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
Usage: record_joint_rebase.py out.json [cases]
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x0081FD10, 0x1740  # through RET 0x8 (3 bytes) at 0x0082144D
LOOKUP = 0x00421ED0
OBJ, MODEL, TABLE, RECS, MATS = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
EXE_PAGES = [0x0161C000, 0x01521000, 0x01629000, 0x01531000]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for p in sorted({ENTRY & ~0xFFF, (ENTRY + 0x1000) & ~0xFFF, (ENTRY + LENGTH) & ~0xFFF, LOOKUP & ~0xFFF}):
            uc.mem_map(p, 0x1000)
        for p in EXE_PAGES:
            uc.mem_map(p, 0x1000)
            uc.mem_write(p, h.code(p, 0x1000))
        for base, size in ((OBJ, 0x1000), (MODEL, 0x1000), (TABLE, 0x1000), (RECS, 0x2000), (MATS, 0x1000),
                           (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(LOOKUP, bytes([0xC3]))
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
            return
        self.hit.add(address)
        if address != LOOKUP:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 8))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        eax = 0 if rng.random() < 0.15 else MATS + 0x100 * rng.randrange(4)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=LOOKUP, before=before, after=dict(regs=self.regs(), memory=[])))

    def f(self):
        rng = self.rng
        r = rng.random()
        if r < 0.15:
            return 0.0
        if r < 0.3:
            return float(rng.randrange(-3, 4))
        if r < 0.35:
            return rng.choice((1e6, -1e6, 1e-3, -1e-3))
        return rng.uniform(-4, 4)

    def matrix(self, singular):
        rows = [[self.f() for _ in range(4)] for _ in range(4)]
        if singular:
            kind = self.rng.randrange(3)
            if kind == 0:
                rows = [[0.0] * 4 for _ in range(4)]
            elif kind == 1:
                rows[3] = list(rows[2])
                rows[1] = [0.0] * 4
            else:
                rows = [[float(i == j) for j in range(4)] for i in range(4)]
        return rows

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((OBJ, 0x100), (MODEL, 0x400), (TABLE, 0x100), (RECS, 0x800), (MATS, 0x400)):
            uc.mem_write(base, bytes(size))

        def put(base, off, fmt, v):
            uc.mem_write(base + off, struct.pack(fmt, v))
        put(OBJ, 4, '<I', MODEL)
        put(OBJ, 0x24, '<I', rng.choice((0xFFFF, 0xFFFF, 0, 1, 2, 3, 4, 0x10001, rng.randrange(5))))
        put(OBJ, 0x28, '<I', rng.choice((0xFFFF, 0, 1, 2, 3, 4, rng.randrange(5))))
        base = rng.choice((RECS, RECS, RECS, 0))
        put(MODEL, 0x364, '<I', base)
        put(MODEL, 0x368, '<I', TABLE)
        for i in range(8):
            put(TABLE, i, '<B', rng.choice((0xFF, 0, 1, 2, 3, 4, rng.randrange(5))) if base else rng.choice((0xFF, 0)))
        for k in range(5):
            rec = RECS + 160 * k
            m = self.matrix(rng.random() < 0.3)
            for r in range(4):
                for cidx in range(4):
                    put(rec, 0x10 + 16 * r + 4 * cidx, '<f', m[r][cidx])
            put(rec, 0x91, '<B', rng.choice((0, 1, 2, 3, 4, rng.randrange(5))))
        for k in range(4):
            m = self.matrix(False)
            for r in range(4):
                for cidx in range(4):
                    put(MATS + 0x100 * k, 0x10 + 16 * r + 4 * cidx, '<f', m[r][cidx])
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<IBBBBBBBB', h.RET, rng.choice((0, 1, 1)), 0, 0, 0, rng.choice((0, 1, 1)), 0, 0, 0))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4], start[7] = esp, OBJ
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    insns = [i.address for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY)]
    missed = [a for a in insns if a not in oracle.hit]
    print(f'{len(insns) - len(missed)}/{len(insns)} instructions covered; missed:', ' '.join(hex(a) for a in missed[:40]))
    json.dump(dict(function='0x0081FD10', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
