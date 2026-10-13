"""L184 oracle: 0x00E70710 `actor->update()` (vf09) run as the original's
code in Unicorn on synthetic actors. Every guest callee is a stub that
returns random values and sometimes rewrites actor fields (tick counts,
flags, the master pointer), recorded as events in the format of
crates/mtcore/tests/semantic.rs `abi_cases_full` (before: regs + argument
window; after: effects + regs). The tail calls to 0x00FDF600 return through
the stub straight to the harness return address.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
Usage: record_actor_update.py out.json [cases]
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00E70710, 0x338  # through RET at 0x00E70A47
THIS = 0x20000000
MASTER = 0x21000000
LINK = 0x22000000
CHILD = 0x23000000
VT_THIS, VT_MASTER, VT_CHILD = 0x24000000, 0x25000000, 0x26000000
STUB_BASE = 0x30000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]

# guest functions called directly, and their stack argument counts
DIRECT = {0x00FDF600: 0, 0x00FE3600: 0, 0x00E6BCA0: 0, 0x00E6D0C0: 0, 0x00E6EFD0: 0, 0x00FDF670: 0,
          0x00E6EA50: 0, 0x00E6D070: 0, 0x00E6D1C0: 0, 0x00E6D2E0: 0, 0x00E6D330: 0, 0x00E6E230: 1}
THIS_SLOTS = {0x9C: 0, 0x54: 0, 0x74: 0, 0x68: 0, 0x78: 0, 0x60: 0, 0x88: 0, 0xA4: 0, 0xA8: 0, 0x64: 1}
STUBS = dict(DIRECT)
for slot, n in THIS_SLOTS.items():
    STUBS[STUB_BASE + 0x100 + slot] = n
STUBS[STUB_BASE + 0x1000 + 0x60] = 0  # master vtable +0x60
STUBS[STUB_BASE + 0x2000 + 0x44] = 0  # child vtable +0x44


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {ENTRY & ~0xFFF, (ENTRY + 0x1000) & ~0xFFF}
        for a in STUBS:
            pages.add(a & ~0xFFF)
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for base, size in ((THIS, 0x1000), (MASTER, 0x1000), (LINK, 0x1000), (CHILD, 0x1000), (VT_THIS, 0x1000),
                           (VT_MASTER, 0x1000), (VT_CHILD, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
        for slot in THIS_SLOTS:
            uc.mem_write(VT_THIS + slot, struct.pack('<I', STUB_BASE + 0x100 + slot))
        uc.mem_write(VT_MASTER + 0x60, struct.pack('<I', STUB_BASE + 0x1000 + 0x60))
        uc.mem_write(VT_CHILD + 0x44, struct.pack('<I', STUB_BASE + 0x2000 + 0x44))
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

    def stub_write(self, address, data):
        self.remember(address, len(data))
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + len(data)))
        return [(address + i, b) for i, b in enumerate(data)]

    def mutate(self):
        """A stub rewriting an actor field, so later re-reads differ."""
        rng = self.rng
        choices = [
            (0xF0, '<I', lambda: self.flags()), (0x110, '<H', lambda: rng.randrange(4)),
            (0x20C, '<H', lambda: rng.randrange(4)), (0x1B4, '<H', lambda: rng.randrange(5)),
            (0x1B6, '<H', lambda: rng.randrange(2)), (0x1E2, '<H', lambda: rng.randrange(2)),
            (0x1F8, '<I', lambda: rng.randrange(2)), (0x204, '<I', lambda: rng.choice((3, rng.randrange(1 << 32)))),
            (0x206, '<H', lambda: rng.randrange(6)), (0x10C, '<I', lambda: rng.randrange(1 << 32)),
            (0x11C, '<I', lambda: rng.choice((0, LINK))),
            (0x11B, '<B', lambda: rng.randrange(256)),
        ]
        off, fmt, gen = rng.choice(choices)
        return self.stub_write(THIS + off, struct.pack(fmt, gen()))

    def flags(self):
        rng = self.rng
        v = rng.randrange(1 << 32)
        v = (v & ~0x20) | (0x20 if rng.random() < 0.15 else 0)
        v = (v & ~0x80000) | (0x80000 if rng.random() < 0.5 else 0)
        v = (v & ~0x40) | (0x40 if rng.random() < 0.5 else 0)
        return v

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        nargs = STUBS[address]
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        rng = self.rng
        effects = []
        if rng.random() < 0.3:
            effects += self.mutate()
        if address == 0x00FE3600:
            eax = rng.choice((0, 1, 2, 3, 2, 3, rng.randrange(1 << 32)))
        elif rng.random() < 0.5:
            eax = rng.choice((0, 1, 0x100, rng.randrange(1 << 32)))
        else:
            eax = rng.randrange(1 << 32)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))
        if back == h.RET:
            uc.emu_stop()

    def quiet_float(self):
        """A random f32 bit pattern; signalling NaNs are made quiet (Unicorn's
        x87 does not quiet them on FLD, the hardware does)."""
        rng = self.rng
        v = rng.choice((0, 0x80000000, 0x3F800000, 0xBF800000, 0x7F800000, 0xFF800000, 0x7FC00000, 0x00000001,
                        rng.randrange(1 << 32), rng.randrange(1 << 32)))
        if v & 0x7F800000 == 0x7F800000 and v & 0x7FFFFF:
            v |= 0x400000
        return v

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base in (THIS, MASTER, LINK, CHILD):
            uc.mem_write(base, bytes(rng.randrange(256) for _ in range(0x400)) if rng.random() < 0.3 else b'\0' * 0x400)

        def put(base, off, fmt, v):
            uc.mem_write(base + off, struct.pack(fmt, v))
        put(THIS, 0, '<I', VT_THIS)
        put(MASTER, 0, '<I', VT_MASTER)
        put(CHILD, 0, '<I', VT_CHILD)
        put(THIS, 0x11B, '<B', rng.choice((0, 1, 1, 1, 0xFF, 0xFE)))
        put(THIS, 0x11C, '<I', rng.choice((0, LINK, LINK)))
        put(LINK, 4, '<I', rng.choice((1, 2, 0, 3, 4, 5, 6, 7, 1, 2)) | (rng.randrange(1 << 8) << 3))
        put(THIS, 0x1D0, '<I', rng.choice((0, MASTER, MASTER, MASTER)))
        put(THIS, 0x1D4, '<I', rng.randrange(4))
        put(MASTER, 0x1D4, '<I', rng.choice((0, 1, 2, 3)))
        put(MASTER, 4, '<I', rng.choice((1, 2, 2, 2, 0, 3, 5, 7)) | rng.choice((0x4800, 0x4000, 0x800, 0, 0x4800)) | (rng.randrange(1 << 8) << 20))
        put(MASTER, 0x1E0, '<H', rng.choice((0, 1, 7, 0xFFFF)))
        put(THIS, 4, '<I', rng.randrange(1 << 32))
        for off in (0x100, 0x60, 0x64, 0x68, 0x1B0):
            put(MASTER, off, '<I', self.quiet_float())
        for off in (0x6C, 0x68, 0x60, 0x64, 0x100, 0x1B0):
            put(THIS, off, '<I', rng.randrange(1 << 32))
        put(THIS, 0xF0, '<I', self.flags())
        put(THIS, 0x110, '<I', rng.choice((0, 1, 2, 3, rng.randrange(4) | (rng.randrange(1 << 16) << 16))))
        put(THIS, 0x1B4, '<H', rng.randrange(5))
        put(THIS, 0x1B6, '<H', rng.choice((0, 1, 1)))
        put(THIS, 0x20C, '<H', rng.randrange(1 << 16))
        put(THIS, 0x1E2, '<H', rng.choice((0, 1)))
        put(THIS, 0x1F0, '<I', CHILD)
        put(THIS, 0x1F8, '<I', rng.choice((0, 1, 0x1234)))
        put(THIS, 0x204, '<I', rng.choice((3, 3, rng.randrange(1 << 32))))
        put(THIS, 0x206, '<H', rng.randrange(6))
        put(THIS, 0x10C, '<I', rng.randrange(1 << 32))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=400000)
        except Exception:
            print("seed", seed, hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()]); raise
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    from collections import Counter
    print(Counter(e['target'] for c in cases for e in c['events']).most_common())
    json.dump(dict(function='0x00E70710', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
