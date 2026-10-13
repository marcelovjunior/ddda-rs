"""L184 s9 oracle: 0x00EA5B50 `model->upload_state(ctx)` run as the original's
code in Unicorn on synthetic draw contexts and models covering every branch.
Guest callees are stubs with recorded effects: 0x00FED890 (arena grow, ret 4),
0x00FE66A0 (ret 0x10), the listener's virtual method (ret 0x10), 0x00FF1310
(ret 4), and the CRT memcpy 0x012EFEA0 (cdecl, copies). The pointer arguments
of the two reporting calls are recorded as call memory. Format of
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

ENTRY, LENGTH = 0x00EA5B50, 0xCEF  # through RET 0x4 at 0x00EA683C
GROW, REPORT, FADE, MEMCPY = 0x00FED890, 0x00FE66A0, 0x00FF1310, 0x012EFEA0
CALLBACK = 0x25000000
TICK, LISTENER_PTR, CONSTANT = 0x018D2CE8, 0x018D2C80, 0x01406CE0
ARGS = {GROW: 1, REPORT: 4, FADE: 1, CALLBACK: 4, MEMCPY: 3}
CTX, CTX_SIZE = 0x20000000, 0x32000
DEFAULTS, DEFAULTS_SIZE = 0x21000000, 0x4000
RES, T = 0x22000000, 0x23000000
LISTENER, VTABLE = 0x24000000, 0x24001000
MODEL, PARENT = 0x26000000, 0x26001000
DESC_TABLE, P, Q, SOURCE = 0x27000000, 0x27001000, 0x27002000, 0x27003000
ARENA, ARENA_SIZE = 0x28000000, 0x20000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
SLOTS = (0x2868, 0x30A0, 0x1C78, 0x1AA0, 0x2EB0, 0x1CF0, 0x30D8, 0x2248)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        for address, size in ((ENTRY & ~0xFFF, 0x2000), (GROW & ~0xFFF, 0x1000), (REPORT & ~0xFFF, 0x1000),
                              (FADE & ~0xFFF, 0x1000), (MEMCPY & ~0xFFF, 0x1000), (CALLBACK, 0x1000),
                              (TICK & ~0xFFF, 0x1000), (CONSTANT & ~0xFFF, 0x1000), (CTX, CTX_SIZE),
                              (DEFAULTS, DEFAULTS_SIZE), (RES, 0x2000), (T, 0x1000), (LISTENER, 0x2000),
                              (MODEL, 0x2000), (DESC_TABLE, 0x4000), (ARENA, ARENA_SIZE), (STACK, 0x10000),
                              (h.RET, 0x1000)):
            try:
                uc.mem_map(address, size)
            except Exception:
                pass  # page shared with an earlier region (TICK and LISTENER_PTR)
        assert h.code(ENTRY + LENGTH - 3, 3) == b'\xc2\x04\x00', h.code(ENTRY + LENGTH - 3, 3)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for address in (GROW, REPORT, FADE, MEMCPY, CALLBACK):
            uc.mem_write(address, b'\xc3')
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

    def u32(self, address):
        return struct.unpack('<I', self.uc.mem_read(address, 4))[0]

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
        if address in (REPORT, CALLBACK):
            self.remember(args[3], 16)
            window += [(args[3] + i, b) for i, b in enumerate(uc.mem_read(args[3], 16))]
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        if address == MEMCPY:
            dst, src, n = args
            self.remember(src, n)
            data = bytes(uc.mem_read(src, n))
            effects += self.stub_write(dst, data)
            eax, pop = dst, 4
        else:
            eax, pop = rng.getrandbits(32), 4 + 4 * nargs
            if address == GROW:
                size = args[0]
                if back == 0x00EA5D32 and rng.random() < 0.3:
                    cursor, limit = 0, 0
                else:
                    cursor = ARENA + 0x10000 + 16 * rng.randrange(0, 0x100)
                    limit = cursor + rng.choice((size, size + 16, 0x400, 0xE000))
                effects += self.stub_write(CTX + 0x24, struct.pack('<II', cursor, limit))
                if rng.random() < 0.5:
                    # growing the arena may also move the slots the second pass re-checks
                    field = rng.choice((0x2EB0, 0x1CF0, 0x5C, 0x2EB4))
                    value = rng.getrandbits(32)
                    if field == 0x5C:
                        value = (value & ~0x80) | rng.choice((0, 0x80))
                    effects += self.stub_write(CTX + field, struct.pack('<I', value))
                if rng.random() < 0.3:
                    effects += self.stub_write(LISTENER + 0x2CC, struct.pack('<I', rng.choice((0, RES, RES + 0x100))))
            elif rng.random() < 0.4:
                # the reporting calls and the fade may touch the context
                field = rng.choice((0x154, 0x1D4, 0x1C78, 0x1C7C, 0x1AA0, 0x2EB0, 0x16A, 0x5C))
                value = rng.getrandbits(32)
                if field == 0x1D4:
                    value = rng.choice((0, T))
                if field == 0x5C:
                    value &= ~2
                if field == 0x16A:
                    value = rng.choice((0, 0x813, 0x208, 0x1, 0x800))
                    effects += self.stub_write(CTX + field, struct.pack('<H', value))
                else:
                    effects += self.stub_write(CTX + field, struct.pack('<I', value))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        for r in (x.UC_X86_REG_ECX, x.UC_X86_REG_EDX):
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((CTX, CTX_SIZE), (DEFAULTS, DEFAULTS_SIZE), (RES, 0x2000), (T, 0x1000), (LISTENER, 0x2000),
                           (MODEL, 0x2000), (DESC_TABLE, 0x4000), (ARENA, ARENA_SIZE), (STACK, 0x10000)):
            uc.mem_write(base, b'\0' * size)
        uc.mem_write(DEFAULTS, rng.randbytes(DEFAULTS_SIZE))
        uc.mem_write(SOURCE, rng.randbytes(0x1000))
        uc.mem_write(TICK & ~0xFFF, b'\0' * 0x1000)
        uc.mem_write(CONSTANT & ~0xFFF, rng.randbytes(0x1000))
        uc.mem_write(RES, rng.randbytes(0x2000))
        self.w32(TICK, rng.getrandbits(32))
        resources = [RES + 0x100 * k for k in range(6)]
        # model
        uc.mem_write(MODEL, rng.randbytes(0x400))
        flags = rng.getrandbits(32)
        flags = (flags & ~0x80000000) | (rng.randrange(2) << 31)
        flags = (flags & ~0x40000000) | (rng.choice((0, 1, 1)) << 30)
        self.w32(MODEL + 0x108, flags)
        self.w32(MODEL + 0x104, rng.getrandbits(32))
        self.w32(MODEL + 0x30, rng.choice((0, PARENT)))
        uc.mem_write(PARENT, rng.randbytes(0x400))
        # listener
        self.w32(LISTENER_PTR, LISTENER)
        self.w32(LISTENER, VTABLE)
        self.w32(VTABLE + 0x34, CALLBACK)
        # resource object and descriptors
        self.w32(T + 0x210, rng.choice((0, 0, *resources)))
        self.w32(T + 0x264, rng.choice((0, 0, *resources)))
        listener_resource = rng.choice((0, *resources))
        self.w32(LISTENER + 0x2CC, listener_resource)
        self.w32(DESC_TABLE + 0x558, P)
        self.w32(DESC_TABLE + 0x89C, Q)
        for descriptor in (P, Q):
            uc.mem_write(descriptor, rng.randbytes(0x40))
            uc.mem_write(descriptor + 0x18, struct.pack('<H', rng.choice((0, 3, 12, 16, rng.randrange(0x100)))))
        self.w32(Q + 0x24, SOURCE)
        # context
        self.w32(CTX + 0x18, DESC_TABLE)
        self.w32(CTX + 0x20, DEFAULTS)
        uc.mem_write(CTX + 0x16A, struct.pack('<H', rng.choice((0, 0x813, 0x208, 0x1, 0x800, 0x2, 0x10, rng.getrandbits(16)))))
        self.w32(CTX + 0x5C, (rng.getrandbits(32) & ~0x82) | rng.choice((0, 0x80, 0x80)) | rng.choice((0, 2)))
        self.w32(CTX + 0x6C, rng.randrange(0, 5))
        self.w32(CTX + 0x154, rng.getrandbits(32))
        self.w32(CTX + 0x1D4, rng.choice((0, T, T)))
        cursor = ARENA + 16 * rng.randrange(0, 0x100)
        self.w32(CTX + 0x24, cursor)
        self.w32(CTX + 0x28, cursor + rng.choice((0, 0x20, 0x100, 0x10000)))
        wanted = {
            0x2868: self.u32(DEFAULTS + (0x2670 if flags & 0x80000000 else 0x2678)),
            0x30A0: self.u32(DEFAULTS + 0x2EA8),
            0x1C78: self.u32(T + 0x210) if self.u32(CTX + 0x1D4) else 0,
            0x1AA0: self.u32(T + 0x264) if self.u32(CTX + 0x1D4) else 0,
            0x2EB0: self.u32(DEFAULTS + 0x2CC8),
            0x1CF0: listener_resource,
            0x30D8: self.u32(DEFAULTS + 0x2EE0),
            0x2248: self.u32(DEFAULTS + 0x2050),
        }
        for slot in SLOTS:
            value = wanted[slot] if rng.random() < 0.4 else rng.choice((0, rng.getrandbits(32), *resources))
            self.w32(CTX + slot, value)
            self.w32(CTX + slot + 4, rng.choice((0, rng.getrandbits(32))))
        # a context tag already equal to the wanted one exercises "tag unchanged"
        for slot in (0x1C78, 0x1AA0, 0x1CF0):
            if rng.random() < 0.3:
                self.w32(CTX + slot, rng.choice((wanted[slot], 0, rng.choice(resources))))
                tag_bits = {0x1C78: 0xC496E000, 0x1AA0: 0x377B6000, 0x1CF0: 0x45FBB000}[slot]
                self.w32(CTX + slot + 4, (self.u32(wanted[slot] + 0xC) | tag_bits) if wanted[slot] else 0)
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 8 - 4 * rng.randrange(4)
        uc.mem_write(esp, struct.pack('<II', h.RET, CTX))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = MODEL, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases, visited = [], set()
    for seed in range(n):
        cases.append(oracle.case(seed))
        visited |= oracle.visited
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00EA5B50', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {len(visited)} instruction addresses visited -> {out}')
    open(out + '.visited', 'w').write('\n'.join('%08x' % a for a in sorted(visited)))


main()
