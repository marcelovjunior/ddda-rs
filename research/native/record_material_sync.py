"""L184 oracle: 0x00C88380 nDraw::DDMaterialStd::vf12 (thiscall, ret 8), the
standard material's state sync into a draw context, run as the original's
code in Unicorn on randomized contexts covering every branch (the script
prints the instructions no case reached).

Callees are stubs recorded in the format of crates/mtcore/tests/semantic.rs
`abi_cases_full`: 0x01145BF0 (base sync, ECX, 2 args), 0x01066AF0 (flag query,
ECX, 1 arg), 0x010671F0 (parameter set, ECX, 2 args), 0x00FED890 (arena grow,
ECX, 1 arg: moves the context's bump allocator). The CRT memcpy 0x012EFEA0 is
done by the stub (a byte copy, platform model) and its events are dropped by
the test (`native_callees`).
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

ENTRY, LENGTH = 0x00C88380, 0x00C89D5C - 0x00C88380  # through RET 0x8 at 0x00C89D59
BASE_SYNC, QUERY, SET_PARAM, GROW, MEMCPY = 0x01145BF0, 0x01066AF0, 0x010671F0, 0x00FED890, 0x012EFEA0
STUBS = {BASE_SYNC: 2, QUERY: 1, SET_PARAM: 2, GROW: 1, MEMCPY: 3}
CTX, THIS, SHARED, TEXTURES, HOLDER, BLOCKS, SOURCES = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000
ARENA, GROWN = 0x28000000, 0x29000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
POOL = [0, 1, 2, 3, 0x1234, 0xFFFFFFFF]
BLOCK_HOLDERS = (0xC2C, 0xC08, 0x7B8, 0xC1C)
BLOCK_FIELDS = (0x1A50, 0x1A08, 0x1168, 0x1A30)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {a & ~0xFFF for a in list(STUBS) + [0x018D2CE8, 0x015319B8]}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        uc.mem_map(ENTRY & ~0xFFF, 0x2000)
        for a, size in ((CTX, 0x32000), (THIS, 0x1000), (SHARED, 0x3000), (TEXTURES, 0x2000), (HOLDER, 0x2000),
                        (BLOCKS, 0x2000), (SOURCES, 0x8000), (ARENA, 0x10000), (GROWN, 0x20000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
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
            pop = 4  # cdecl
            eax = dest
        elif address == QUERY:
            eax = rng.choice((0, 0, 1, 1, 0x100, rng.randrange(1 << 32)))
        elif address == GROW:
            count = word(esp + 4)
            self.remember(esp + 4, 4)
            effects += self.stub_write(CTX + 0x24, struct.pack('<I', GROWN + 0x100 * self.grows))
            effects += self.stub_write(CTX + 0x28, struct.pack('<I', GROWN + 0x100 * self.grows + max(count, 0x2000)))
            self.grows += 1
        elif address == BASE_SYNC:
            if rng.random() < 0.5:
                effects += self.stub_write(CTX + 0x154, struct.pack('<I', rng.randrange(8)))
            if rng.random() < 0.3:  # the base sync changes a mode-dependent binding
                effects += self.stub_write(CTX + 0x2068, struct.pack('<I', rng.choice((0, TEXTURES))))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((CTX, 0x32000), (THIS, 0x1000), (SHARED, 0x3000), (TEXTURES, 0x2000), (HOLDER, 0x2000), (BLOCKS, 0x2000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(STACK + 0x8000 - 0x400, bytes(rng.randrange(256) for _ in range(0x400)))
        self.grows = 0
        w32 = lambda a, v: uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))
        mode = rng.choice((0, 1, 2, 2, 3, 5))
        w32(THIS + 0xA4, mode)
        # the shared state the context follows, and the context's slots (equal / different)
        for off in range(0x27B8, 0x2CD0, 8):
            w32(SHARED + off, rng.choice(POOL))
        for off in range(0x29B0, 0x29E8, 8):
            w32(CTX + off, rng.choice(POOL) if rng.random() < 0.5 else struct.unpack('<I', uc.mem_read(SHARED + off - 0x29B0 + 0x27B8, 4))[0])
            w32(CTX + off + 4, rng.choice(POOL))
        for slot, source in ((0x2EB0, rng.choice((0x2CB8, 0x2CC8))), (0x2BB8, 0x29C0), (0x2DA8, 0x2BB0), (0x2D78, 0x2B80)):
            for s in (0x2CB8, 0x2CC8):
                pass
            same = struct.unpack('<I', uc.mem_read(SHARED + source, 4))[0]
            w32(CTX + slot, same if rng.random() < 0.4 else rng.choice(POOL))
            w32(CTX + slot + 4, rng.choice(POOL))
        w32(CTX + 0x2EB0, rng.choice((0, 1, struct.unpack('<I', uc.mem_read(SHARED + 0x2CB8, 4))[0], struct.unpack('<I', uc.mem_read(SHARED + 0x2CC8, 4))[0])))
        for off in list(range(0x1B90, 0x1C10, 8)) + [0x1C50, 0x1CA8]:
            w32(CTX + off, rng.choice((0, 0, 1, 7)))
            w32(CTX + off + 4, rng.choice((0, 0, 1, 5)))
        textures = [TEXTURES + 0x100 * k for k in range(4)]
        for t in textures:
            w32(t + 0xC, rng.randrange(1 << 32))
            w32(t + 4, rng.randrange(1 << 32))
        mine = [rng.choice([0] + textures) for _ in range(5)]
        for i in range(5):
            w32(THIS + 0xD0 + 4 * i, mine[i])
            w32(CTX + 0x2068 + 8 * i, mine[i] if rng.random() < 0.35 else rng.choice([0] + textures))
            w32(CTX + 0x206C + 8 * i, rng.choice((0, 5, rng.randrange(1 << 32))))
        uc.mem_write(0x018D2CE8, struct.pack('<I', rng.choice((0, 7, 0xFFFFFFFF, rng.randrange(1 << 32)))))
        # journal, flags, arena and constant blocks
        w32(CTX + 0x20, SHARED)
        uc.mem_write(CTX + 0x5C, bytes([rng.choice((0, 0x80, 0x80, 0xFF, 0x7F))]))
        w32(CTX + 0x6C, rng.randrange(0, 30))
        w32(CTX + 0x154, rng.randrange(1 << 32))
        w32(CTX + 0x18, HOLDER)
        w32(CTX + 0x24, ARENA)
        w32(CTX + 0x28, ARENA + rng.choice((0, 0x30, 0x200, 0x4000)))
        for k, (holder, field) in enumerate(zip(BLOCK_HOLDERS, BLOCK_FIELDS)):
            block = BLOCKS + 0x100 * k
            w32(HOLDER + holder, block)
            uc.mem_write(block + 0x18, struct.pack('<H', rng.choice((0, 1, 3, 4, 17, 40))))
            w32(CTX + field, SOURCES + 0x1000 * k)
        uc.mem_write(SOURCES, bytes(rng.randrange(256) for _ in range(0x4000)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12
        uc.mem_write(esp, struct.pack('<III', h.RET, CTX, rng.choice((0x77, 0x1234))))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x00C88380', cases=cases), open(out, 'w'))
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
