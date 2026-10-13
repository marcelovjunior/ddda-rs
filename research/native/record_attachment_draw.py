"""L184 oracle: 0x00F23260 object->draw_attachment(ctx) (thiscall, ret 4) run as the original's code
in Unicorn on random objects (prints instructions no case reached). Callees are stubs recorded in
the format of crates/mtcore/tests/semantic.rs `abi_cases_full`: 0x00EA1EE0 / 0x00EA1F40 / 0x00EA1FD0
(ECX = attached node, 1 arg), the node's virtual method 0x64, 0x00F1FD30 (ECX = object, 6 args, ret
0x18), 0x00FED890 (arena grow); the CRT memcpy 0x012EFEA0 is done by the stub (the test drops its
events). Output is private (E:/ddda-re/notes/native-L184)."""
import json, random, struct, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00F23260, 0x00F235D6 - 0x00F23260
SLOT, PREPARE, VMETHOD, QUERY, DRAW, GROW, MEMCPY = 0x00EA1EE0, 0x00EA1F40, 0x31000100, 0x00EA1FD0, 0x00F1FD30, 0x00FED890, 0x012EFEA0
STUBS = {SLOT: 1, PREPARE: 1, VMETHOD: 1, QUERY: 1, DRAW: 6, GROW: 1, MEMCPY: 3}
THIS, NODE, VTABLE, CTX, SHARED, HOLDER, BLOCK, SOURCE, ARENA, GROWN = 0x20000000, 0x21000000, 0x21001000, 0x22000000, 0x24000000, 0x25000000, 0x26000000, 0x27000000, 0x28000000, 0x29000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
POOL = [0, 1, 2, 3, 0x1234, 0xFFFFFFFF]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(STUBS) + [ENTRY, ENTRY + LENGTH, 0x01561738]}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((THIS, 0x1000), (NODE, 0x2000), (CTX, 0x32000), (SHARED, 0x3000), (HOLDER, 0x1000), (BLOCK, 0x1000), (SOURCE, 0x1000), (ARENA, 0x10000), (GROWN, 0x20000), (STACK, 0x10000), (h.RET, 0x1000)):
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

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.hit.add(address)
            return
        nargs = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        rng = self.rng
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        back = word(esp)
        pop, effects, eax = 4 + 4 * nargs, [], rng.randrange(1 << 32)
        if address == MEMCPY:
            dest, src, count = word(esp + 4), word(esp + 8), word(esp + 12)
            self.remember(esp + 4, 12)
            self.remember(src, count)
            self.stub_write(dest, bytes(uc.mem_read(src, count)))
            pop, eax = 4, dest
        elif address == GROW:
            count = word(esp + 4)
            self.remember(esp + 4, 4)
            effects += self.stub_write(CTX + 0x24, struct.pack('<I', GROWN))
            effects += self.stub_write(CTX + 0x28, struct.pack('<I', GROWN + max(count, 0x2000)))
        elif address == SLOT:
            eax = rng.choice((0xFFFFFFFF, 0, 1, 5, 0x80000000, rng.randrange(1 << 32)))
        elif address == DRAW and rng.random() < 0.5:
            effects += self.stub_write(CTX + 0x2E08, struct.pack('<I', rng.choice(POOL)))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x1000), (NODE, 0x2000), (CTX, 0x32000), (SHARED, 0x3000), (HOLDER, 0x1000), (BLOCK, 0x1000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x400, bytes(rng.randrange(256) for _ in range(0x400)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        w32(THIS + 4, rng.choice((1, 2, 2, 0, 3, 9, 0x11)))
        w32(THIS + 0x1E0, rng.choice((0, NODE, NODE, NODE)))
        w32(NODE, VTABLE)
        w32(VTABLE + 0x64, VMETHOD)
        w32(NODE + 0xF0, rng.choice((0, 1, SOURCE)))
        w32(NODE + 0xF8, rng.randrange(1 << 32))
        floor = rng.choice((0.0, 0.5, 1.0))
        uc.mem_write(0x01561738, struct.pack('<f', floor))
        if rng.random() < 0.9:
            uc.mem_write(NODE + 0x158, struct.pack('<f', rng.choice((floor, floor, 0.0, 0.5, 1.0, 2.0))))
        else:
            uc.mem_write(NODE + 0x158, struct.pack('<I', 0x7FC00000))
        w32(NODE + 0x108, rng.randrange(1 << 32))
        # the draw context
        w32(CTX + 0x20, SHARED)
        for off in range(0x2C10, 0x2CD0, 8):
            w32(SHARED + off, rng.choice(POOL))
        same1 = struct.unpack('<I', uc.mem_read(SHARED + 0x2C10, 4))[0]
        same2 = struct.unpack('<I', uc.mem_read(SHARED + 0x2CC8, 4))[0]
        w32(CTX + 0x2E08, rng.choice([same1, same1] + POOL))
        w32(CTX + 0x2E0C, rng.choice(POOL))
        w32(CTX + 0x2EB0, rng.choice([same2, same2] + POOL))
        w32(CTX + 0x2EB4, rng.choice(POOL))
        w32(CTX + 0x1D90, rng.choice((0, 0, 1, 7)))
        w32(CTX + 0x1D94, rng.choice((0, 0, 1, 5)))
        w32(CTX + 0x1330, rng.choice(POOL))
        w32(CTX + 0x1334, rng.choice(POOL))
        uc.mem_write(CTX + 0x5C, bytes([rng.choice((0, 0x80, 0x80, 0xFF, 0x7F, 0x82))]) + bytes(3))
        w32(CTX + 0x6C, rng.randrange(0, 30))
        w32(CTX + 0x154, rng.randrange(1 << 32))
        w32(CTX + 0x18, HOLDER)
        w32(CTX + 0x24, ARENA)
        w32(CTX + 0x28, ARENA + rng.choice((0, 0x30, 0x200, 0x4000)))
        w32(HOLDER + 0x89C, BLOCK)
        uc.mem_write(BLOCK + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 17, 40))))
        w32(BLOCK + 0x24, SOURCE)
        uc.mem_write(SOURCE, bytes(rng.randrange(256) for _ in range(0x100)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<II', h.RET, CTX))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00F23260', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{n} cases -> {out}', by)
    print(len(unhit), 'unhit', unhit[:30])


main()
