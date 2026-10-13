"""L184 s3 oracle: 0x00C52A30 (a vf93 virtual method of the unit-draw job tree)
run as the original code in Unicorn on randomized synthetic units.
All of its callees are stubs (`ret n`) recorded as events with their
registers and argument window before and effects/registers after; the stubs
randomly rewrite fields of the unit so the branches after them get covered.
Output format: crates/mtcore/tests/semantic_s3.rs.
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

ENTRY, LENGTH = 0x00C52A30, 0x215  # through RET at 0x00C52C44
# guest callees: address -> stack argument words
DIRECT = {0x0076F740: 5, 0x00884270: 1, 0x0076B470: 1, 0x007A5FA0: 0, 0x00794550: 1,
          0x005D1AF0: 2, 0x00C52C50: 0, 0x00C540D0: 0, 0x0049E300: 1}
STUB_BASE = 0x30000000
V50, V54, V20, V14 = STUB_BASE + 0x50, STUB_BASE + 0x54, STUB_BASE + 0x20, STUB_BASE + 0x14
STUBS = dict(DIRECT)
STUBS.update({V50: 0, V54: 0, V20: 0, V14: 0})
THIS, THIS_SIZE = 0x20000000, 0x4000
TABLE, ARR, OBJ_A, OBJ_B, VT0, VT_OBJ, VT_2070 = (0x21000000 + 0x1000 * i for i in range(7))
G, G_SIZE = 0x22000000, 0xC0000
GLOBAL_G, GLOBAL_500, GLOBAL_FLOAT = 0x018FA4BC, 0x018FA500, 0x01531AE8
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((ENTRY, 0x2000), (THIS, THIS_SIZE), (0x21000000, 0x8000), (G, G_SIZE),
                              (GLOBAL_G, 4), (GLOBAL_500, 4), (GLOBAL_FLOAT, 4),
                              (STUB_BASE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for a in DIRECT:
            mapping(a, 1)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
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

    def random_float(self, rng):
        # quiet NaNs only: Unicorn does not quiet a signalling NaN on fld
        return rng.choice((0.0, -0.0, 0.5, 1.0, 2.5, -3.0, 1e-40, 1e30, float('inf'), float('-inf'), float('nan'),
                           rng.uniform(-4, 4), rng.uniform(0, 1)))

    def side_effects(self, rng):
        """what a callee may rewrite in the unit (the fields read after it)"""
        fx = []
        if rng.random() < 0.3:
            fx += self.stub_write(THIS + 0x25AC, struct.pack('<I', rng.choice((0, 1, 0x800, 0x801, 0x100001, 0x100800, rng.randrange(1 << 32)))))
        if rng.random() < 0.3:
            fx += self.stub_write(THIS + 0x25F0, bytes([rng.randrange(256)]))
        if rng.random() < 0.2:
            fx += self.stub_write(THIS + 0x16C, f32(self.random_float(rng)))
        if rng.random() < 0.2:
            fx += self.stub_write(THIS + 0x2574, struct.pack('<I', rng.choice((0x29F9, rng.randrange(1 << 32)))))
        if rng.random() < 0.1:
            fx += self.stub_write(THIS + 0x2684, struct.pack('<I', rng.randrange(0, self.elements + 1)))
        if rng.random() < 0.1:
            fx += self.stub_write(THIS + 0x1D1C, struct.pack('<I', rng.choice((0, TABLE))))
        return fx

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = STUBS[address]
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        effects = self.side_effects(rng)
        eax = rng.randrange(1 << 32)
        if address == 0x00C540D0:
            eax = rng.choice((0, 1, rng.randrange(1 << 32) & ~0xFF, rng.randrange(1 << 32)))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, THIS_SIZE), (0x21000000, 0x8000), (G, G_SIZE)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def w8(a, v):
            uc.mem_write(a, bytes([v]))

        self.elements = rng.randrange(0, 5)
        gate = rng.random() < 0.7
        w8(THIS + 0x2627, (0x0E | rng.randrange(256)) if gate else rng.randrange(256))
        w8(THIS + 0x25F2, rng.choice((0, 1, 1, 5)))
        w8(THIS + 0x25F3, rng.choice((0, 1, 1, 7)))
        w32(THIS + 0x2684, self.elements)
        w32(THIS + 0x1D1C, rng.choice((0, TABLE, TABLE)))
        w32(TABLE + 0x78, rng.randrange(0, 5))
        w32(TABLE + 0x84, ARR)
        for i in range(8):
            w32(ARR + 4 * i, rng.randrange(1 << 32))
        for i in range(self.elements + 4):
            w32(THIS + 0x2688 + 4 * i, rng.choice((rng.randrange(0, 6), rng.randrange(1 << 32))))
        w32(THIS, VT0)
        w32(VT0 + 0x50, V50)
        w32(VT0 + 0x54, V54)
        w32(VT_OBJ + 0x20, V20)
        w32(OBJ_A, VT_OBJ)
        w32(OBJ_B, VT_OBJ)
        w32(THIS + 0x1ED8 + 0x20, rng.choice((0, OBJ_A)))
        w32(THIS + 0x1ED8 + 0x24, rng.choice((0, OBJ_B, OBJ_A)))
        w32(THIS + 0x2070, VT_2070)
        w32(VT_2070 + 0x14, V14)
        for i in range(16):
            w32(THIS + 0xFB0 + 4 * i, rng.choice((0, 0, rng.randrange(1, 1 << 32))))
        w32(THIS + 0x2574, rng.choice((0x29F9, 0x29F9, rng.randrange(1 << 32))))
        w8(THIS + 0x29A7, rng.randrange(256))
        w8(THIS + 0x25F0, rng.choice((0x40, 0x40, 0xC0, 0x80, 0, rng.randrange(256))))
        w32(THIS + 0x25AC, rng.choice((0, 1, 0x800, 0x801, 0x100001, 0x100800, rng.randrange(1 << 32))))
        uc.mem_write(THIS + 0x16C, f32(self.random_float(rng)))
        uc.mem_write(THIS + 0x2928, f32(self.random_float(rng)))
        w32(GLOBAL_G, G)
        w32(GLOBAL_500, rng.randrange(1 << 32))
        uc.mem_write(GLOBAL_FLOAT, f32(rng.choice((0.0, 0.5, 1.0, 2.0, rng.uniform(0, 3), float('nan')))))
        for off in (0xBE454, 0xBE458, 0xBE45C, 0xBE460):
            uc.mem_write(G + off, f32(rng.choice((self.random_float(rng), rng.uniform(0, 3), 0.5, 1.0, 2.0))))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(1, 5)
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00C52A30', cases=cases), open(out, 'w'))
    from collections import Counter
    tally = Counter(e['target'] for c in cases for e in c['events'])
    print(f'{n} cases, {calls} calls -> {out}; original instructions per case: mean {sum(c["insns"] for c in cases) / n:.0f}')
    print({hex(k): v for k, v in sorted(tally.items())})


main()
