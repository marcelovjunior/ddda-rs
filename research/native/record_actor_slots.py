"""L184 oracle: 0x0076F2D0 `refresh_slots(actor)` (stdcall, ret 4) run as the
original's code in Unicorn on randomized actors. Its callees are stubs
(recorded in the format of crates/mtcore/tests/semantic.rs `abi_cases_full`):
the controller's virtual method 4 (returns a DTI node whose parent chain may
or may not hold the class [0x018D2D10]), 0x00419210 (ECX, 1 arg), 0x0076F430
(1 arg), 0x004C9B80 (EAX, 1 arg) and 0x0076F660 (EAX, 2 args).
Output is private (E:/ddda-re/notes/native-L184). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe.
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

ENTRY, LENGTH = 0x0076F2D0, 0x153  # through RET 0x4 at 0x0076F420
VSTUB = 0x30000100
STUBS = {VSTUB: (0, 'node'), 0x00419210: (1, 'flag'), 0x0076F430: (1, 'void'), 0x004C9B80: (1, 'flag'), 0x0076F660: (2, 'void')}
ACTOR, CONTROLLER, VTABLE, NODES, EQUIP = 0x20000000, 0x21000000, 0x21001000, 0x22000000, 0x23000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
CLASS = 0x1234


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {ENTRY & ~0xFFF, (ENTRY + LENGTH) & ~0xFFF, 0x018D2000, 0x018D2000}
        pages |= {a & ~0xFFF for a in STUBS}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((ACTOR, 0x2000), (CONTROLLER, 0x2000), (NODES, 0x1000), (EQUIP, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
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
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        nargs, kind = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        eax = rng.randrange(1 << 32)
        if kind == 'node':
            eax = NODES + 0x20 * self.head
        elif kind == 'flag':
            eax = rng.choice((0, 0, 0, 1, rng.randrange(1 << 32)))
            if address == 0x00419210:
                eax = rng.choice((0, 0, 0, 0, 0, 0, 1, 0x1000))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=[])))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((ACTOR, 0x2000), (CONTROLLER, 0x2000), (NODES, 0x1000), (EQUIP, 0x1000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x400, bytes(rng.randrange(256) for _ in range(0x400)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v))
        uc.mem_write(0x018D2D10, struct.pack('<I', CLASS))
        w32(CONTROLLER, VTABLE)
        w32(VTABLE + 0x10, VSTUB)
        w32(ACTOR + 0xCD0, rng.choice((0, CONTROLLER, CONTROLLER, CONTROLLER, CONTROLLER)))
        # a DTI chain of 1..4 nodes; the class is on one of them or absent
        length = rng.randrange(1, 5)
        hit = rng.choice((None, 0, 1, 2, 3, 0, 0))
        self.head = rng.randrange(0, 2) if length > 1 else 0
        for i in range(5):
            w32(NODES + 0x20 * i + 4, CLASS if i == hit else rng.choice((1, 2, 3)))
            w32(NODES + 0x20 * i + 0x10, NODES + 0x20 * (i + 1) if i + 1 < length else 0)
        w32(ACTOR + 0xD3C, rng.randrange(256) if rng.random() < 0.8 else 0xFF)
        uc.mem_write(ACTOR + 0xD99, bytes([rng.choice((0, 1, 1, 1))]))
        for s in range(8):
            eq = rng.choice((0, EQUIP + 0x100 * s, EQUIP + 0x100 * s)) if rng.random() < 0.85 else 0
            w32(ACTOR + 0xC3C + 4 * s, eq)
            if eq:
                w32(eq + 0x70, rng.randrange(0, 5))
                uc.mem_write(eq + 0x50, bytes([rng.randrange(4)]))
        for g in range(8):
            for k in range(16):
                a = ACTOR + 0x70 + 0x40 * g + 4 * k
                uc.mem_write(a, struct.pack('<H', (rng.randrange(0, 5) * 1000 + rng.randrange(1000)) & 0xFFFF))
                uc.mem_write(a + 2, bytes([rng.choice((0, 1, 1, 7)), rng.randrange(256)]))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<II', h.RET, ACTOR))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x0076F2D0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{n} cases -> {out}', by)
    print('unhit instructions:', unhit)


main()
