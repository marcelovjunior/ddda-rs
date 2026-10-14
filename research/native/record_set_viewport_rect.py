"""L184 s9 oracle: 0x00FF04C0 `ctx->set_viewport(rect)` run as the original's code
in Unicorn on synthetic draw contexts. Its guest calls are stubs: the page
allocator 0x00DA7D70 (thiscall, ret 4; returns 0 or a block) and the CRT
memcpy 0x012EFEA0 (cdecl, copies). Format of
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

ENTRY, LENGTH = 0x00FF04C0, 0x2B2  # through RET 0x4 at 0x00FF076F
PAGES, MEMCPY = 0x00DA7D70, 0x012EFEA0
ALLOCATOR_PTR = 0x018D08F4
RECT_UNUSED = 0
ARGS = {PAGES: 1, MEMCPY: 3}
CTX, CTX_SIZE = 0x20000000, 0x2000
DESC_TABLE, P, SOURCE = 0x21000000, 0x21001000, 0x21004000
ARENA, ARENA_SIZE = 0x22000000, 0x20000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x1000), (PAGES & ~0xFFF, 0x1000), (MEMCPY & ~0xFFF, 0x1000),
                              (ALLOCATOR_PTR & ~0xFFF, 0x1000), (0x01406000, 0x1000), (0x0161C000, 0x1000), (0, 0x1000), (CTX, CTX_SIZE), (DESC_TABLE, 0x6000),
                              (ARENA, ARENA_SIZE), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x04\x00', h.code(ENTRY + LENGTH - 3, 3)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(PAGES, b'\xc3')
        uc.mem_write(MEMCPY, b'\xc3')
        self.data = {p: h.code(p, 0x1000) for p in (0x01406000, 0x0161C000)}
        for p, d in self.data.items():
            uc.mem_write(p, d)
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

    def w32(self, address, value):
        self.uc.mem_write(address, struct.pack('<I', value & 0xFFFFFFFF))

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in ARGS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            self.visited.add(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = ARGS[address]
        self.remember(esp, 4 * (1 + nargs))
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        args = struct.unpack('<%dI' % nargs, uc.mem_read(esp + 4, 4 * nargs))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        if address == MEMCPY:
            dst, src, n = args
            self.remember(src, n)
            effects += self.stub_write(dst, bytes(uc.mem_read(src, n)))
            eax, pop = dst, 4
        else:
            eax = 0 if rng.random() < 0.3 else ARENA + 0x10000 + 16 * rng.randrange(0, 0x100)
            pop = 8
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        for r in (x.UC_X86_REG_ECX, x.UC_X86_REG_EDX):
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((CTX, CTX_SIZE), (DESC_TABLE, 0x6000), (ARENA, ARENA_SIZE), (STACK, 0x10000)):
            uc.mem_write(base, b'\0' * size)
        for page in (0x01406000, 0x0161C000):
            uc.mem_write(page, self.data[page])
        uc.mem_write(SOURCE, rng.randbytes(0x1000))
        uc.mem_write(ALLOCATOR_PTR & ~0xFFF, rng.randbytes(0x1000))
        interesting = (0, 1, -1, 100, 640, 1280, 1920, -50, 0x7FFFFFFF, -0x80000000, 3, 4095)
        rect = [rng.choice(interesting) if rng.random() < 0.5 else rng.randrange(-2000, 4000) for _ in range(4)]
        RECT = DESC_TABLE + 0x2000
        uc.mem_write(RECT, struct.pack('<4i', *rect))
        stored = list(rect) if rng.random() < 0.2 else [rng.choice(interesting) for _ in range(4)]
        if rng.random() < 0.15:
            stored = list(rect)
            stored[rng.randrange(4)] ^= 1
        uc.mem_write(CTX + 0xCC, struct.pack('<4i', *stored))
        self.w32(CTX + 0xC8, rng.getrandbits(32))
        self.w32(CTX + 0x10C, rng.choice((0, 0x00400040, 0x04380780, 0x03000400, rng.getrandbits(32), 0x0001FFFF, 0xFFFF0000)))
        self.w32(CTX + 0x18, DESC_TABLE)
        self.w32(DESC_TABLE + 0x54C, P)
        uc.mem_write(P + 0x18, struct.pack('<H', rng.choice((0, 1, 4, 12, 16, rng.randrange(0x100)))))
        cursor = ARENA + 16 * rng.randrange(0, 0x100)
        self.w32(CTX + 0x24, cursor)
        self.w32(CTX + 0x28, cursor + rng.choice((0, 0x20, 0x100, 0x10000)))
        self.w32(CTX + 0xC90, SOURCE)
        self.w32(CTX + 0x154, rng.getrandbits(32))
        self.w32(0x018D08F4, rng.getrandbits(32))
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 8 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<II', h.RET, RECT))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = CTX, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        uc.reg_write(x.UC_X86_REG_MXCSR, 0x1F80)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 1500
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00FF04C0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {len(visited)} instruction addresses visited -> {out}')


main()
