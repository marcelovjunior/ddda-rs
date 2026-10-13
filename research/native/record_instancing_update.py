"""L184 oracle: 0x00C25EA0 uOmSwingBaseInstancing::vf11 (thiscall, ret 4), the
instanced object's per-draw update, run as the original's code in Unicorn on
randomized objects covering every branch (prints the instructions no case
reached). Callees are stubs recorded in the format of crates/mtcore/tests/
semantic.rs `abi_cases_full`: 0x00FED1E0 (ECX=ctx), 0x00FED890 (arena grow),
0x00FA5860 (1 arg), the five virtual methods of the embedded part at
this+0x470 (slots 0x2C ret 0xC, 0x30/0x34/0x38/0x3C ret 4) and the CRT
memcpy 0x012EFEA0 (done by the stub; the test drops its events).
Output is private (E:/ddda-re/notes/native-L184).
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

ENTRY, LENGTH = 0x00C25EA0, 0x00C26304 - 0x00C25EA0  # through RET 0x4 at 0x00C26301
CLEAR, GROW, SWING, MEMCPY = 0x00FED1E0, 0x00FED890, 0x00FA5860, 0x012EFEA0
SLOT_STUB = {0x2C: 0x31000020, 0x30: 0x31000030, 0x34: 0x31000040, 0x38: 0x31000050, 0x3C: 0x31000060}
STUBS = {CLEAR: 0, GROW: 1, SWING: 1, MEMCPY: 3, 0x31000020: 3, 0x31000030: 1, 0x31000040: 1, 0x31000050: 1, 0x31000060: 1}
CTX, THIS, CULL, HOLDER, BLOCK, SOURCE, SHARED, ARENA, GROWN = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000, 0x28000000, 0x29000000
VTABLE = 0x27000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
POOL = [0.0, 0.5, 1.0, 2.0, 3.0, -1.0, 0.25]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(STUBS) + [0x01629530, 0x0161C280, ENTRY]}
        pages.add((ENTRY + LENGTH) & ~0xFFF)
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, size in ((CTX, 0x32000), (THIS, 0x2000), (CULL, 0x1000), (HOLDER, 0x1000), (BLOCK, 0x1000), (SOURCE, 0x1000),
                        (SHARED, 0x2000), (ARENA, 0x10000), (GROWN, 0x20000), (VTABLE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
        uc.mem_write(0x0161C280, struct.pack('<I', 0x80000000))
        for slot, stub in SLOT_STUB.items():
            uc.mem_write(VTABLE + slot, struct.pack('<I', stub))
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
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        effects, eax = [], rng.randrange(1 << 32)
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        back = word(esp)
        pop = 4 * (1 + nargs)
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
        elif address == 0x31000020:  # method 0x2C: accepts?
            eax = rng.choice((0, 1, 1, 0x100, rng.randrange(1 << 32)))
        elif address == 0x31000050 and rng.random() < 0.5:  # method 0x38 changes the clip binding
            effects += self.stub_write(CTX + 0xCA0, struct.pack('<I', rng.choice((0, ARENA + 0x40, SHARED + 0x10))))
            effects += self.stub_write(CTX + 0xCA4, struct.pack('<I', rng.choice((0, 3))))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((CTX, 0x32000), (THIS, 0x2000), (CULL, 0x1000), (HOLDER, 0x1000), (BLOCK, 0x1000), (SHARED, 0x2000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x400, bytes(rng.randrange(256) for _ in range(0x400)))
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        wf = lambda a, v: uc.mem_write(a, struct.pack('<f', v))
        # normals and constants
        for a in (0x01629530, 0x01629540, 0x01629550):
            uc.mem_write(a, struct.pack('<fff', rng.choice(POOL), rng.choice(POOL), rng.choice(POOL)))
        # the culling record (aligned to 16, low nibble set in the pointer)
        w32(CTX + 0x1370, CULL | rng.randrange(16))
        for k in range(4):
            wf(CULL + 4 * k, rng.choice(POOL))
        w32(CULL + 0x10, rng.choice((0, 1, 5)))
        w32(CULL + 0x14, rng.choice((0, 1, 0x80000000)))
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0, 0x208, 0x200, 0x8, 0x1, 0xFFFF))))
        uc.mem_write(THIS + 0x15F0, bytes([rng.choice((0, 1, 1, 1, 7))]))
        w32(THIS + 0x108, rng.randrange(1 << 32))
        w32(THIS + 0x100, rng.randrange(1 << 32))
        uc.mem_write(THIS + 0x449, bytes([rng.choice((0, 1, 1, 2))]))
        w32(THIS + 0x470, VTABLE)
        # the arena, the block copied into it, the shared state and the binding
        w32(CTX + 0x18, HOLDER)
        block_n = rng.choice((0, 1, 5, 17))
        w32(HOLDER + 0x554, BLOCK)
        uc.mem_write(BLOCK + 0x18, struct.pack('<H', block_n))
        w32(BLOCK + 0x24, SOURCE)
        uc.mem_write(SOURCE, bytes(rng.randrange(256) for _ in range(0x100)))
        w32(CTX + 0x24, ARENA)
        w32(CTX + 0x28, ARENA + rng.choice((0, 0x40, 0x4000)))
        w32(CTX + 0x20, SHARED)
        w32(SHARED + 0xAA8, rng.choice((0, ARENA + 0x100)))
        w32(CTX + 0xCA0, rng.choice((0, ARENA + 0x100, ARENA + 0x200)) | rng.choice((0, 0, 5)))
        w32(CTX + 0xCA4, rng.choice((0, 0, 7)))
        uc.mem_write(CTX + 0x5C, bytes([rng.choice((0, 0x80, 0xFF))]))
        w32(CTX + 0x6C, rng.randrange(0, 20))
        w32(CTX + 0x154, rng.randrange(1 << 32))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<II', h.RET, CTX))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
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
    json.dump(dict(function='0x00C25EA0', cases=cases), open(out, 'w'))
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    unhit = [hex(i.address) for i in md.disasm(h.code(ENTRY, LENGTH), ENTRY) if i.address not in oracle.hit]
    by = {}
    for c in cases:
        for e in c['events']:
            by[hex(e['target'])] = by.get(hex(e['target']), 0) + 1
    print(f'{n} cases -> {out}', by)
    print(len(unhit), 'unhit instructions:', unhit[:40])


main()
