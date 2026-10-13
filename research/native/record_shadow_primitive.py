"""L172 P2b (s9) oracle: the sPrimitive singleton methods vf11 0x00E61010
and vf12 0x00E68390 run as the original's code in Unicorn on synthetic
primitive pools and schedulers. Guest callees are stubs with recorded
effects: 0x0111D610 / 0x0111D650 (list begin / end), 0x0111E180 (sort),
0x00DBE560 (scheduler run), the allocator's virtuals (+0x20 alloc, +0x24
free of the object at 0x0187663C), the unit consumer (this vtable +0x3C),
and the CRT memcpy (done natively by the port). Format of
crates/mtcore/tests/shadow_primitive.rs (the abi_cases_full format).
Usage: record_shadow_primitive.py <out.json> <n> <vf11|vf12>
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

ENTRIES = {'vf11': (0x00E61010, 0x69), 'vf12': (0x00E68390, 0x69E), 'begin': (0x0107FF00, 0x22), 'end': (0x0107FF30, 0x1D)}
STUB_BEGIN, STUB_END = 0x30000050, 0x30000060
BEGIN, END, SORT, RUN = 0x0111D610, 0x0111D650, 0x0111E180, 0x00DBE560
MEMCPY = 0x012EFEA0
ALLOC, FREE, UNIT = 0x30000020, 0x30000024, 0x30000040
ARGS = {STUB_BEGIN: 4, STUB_END: 3, BEGIN: 1, END: 0, SORT: 1, RUN: 1, MEMCPY: 3, ALLOC: 2, FREE: 1, UNIT: 2}
# the stubs whose whole recorded window the test asserts
THIS, POOLS, DESCS, DATA, SCHED, LANES, CTX = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000
ALLOCATOR, ALLOC_VT, UNIT_VT, HEAP = 0x27000000, 0x27001000, 0x27002000, 0x28000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self, which):
        self.which = which
        self.entry, _ = ENTRIES[which]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, sections = h.image()
        for va, bytes_ in sections:
            start = va & ~0xFFF
            uc.mem_map(start, ((va + len(bytes_) - start) + 0xFFF) & ~0xFFF)
            uc.mem_write(va, bytes_)
        mapped = set()
        for address, size in ((0, 0x1000), (THIS, 0x2000), (POOLS, 0x9000), (DESCS, 0x9000), (DATA, 0x20000), (SCHED, 0x1D000),
                              (LANES, 0x1000), (CTX, 0x1000), (ALLOCATOR, 0x4000), (HEAP, 0x40000), (STACK, 0x10000),
                              (h.RET, 0x1000), (0x30000000, 0x1000), (0x18D0000, 0x4000), (0x1876000, 0x1000)):
            for page in range(address, address + size, 0x1000):
                if page not in mapped:
                    try:
                        uc.mem_map(page, 0x1000)
                    except Exception:
                        pass  # already part of the image
                    mapped.add(page)
        for a in (BEGIN, END, SORT, RUN, MEMCPY, ALLOC, FREE, UNIT, STUB_BEGIN, STUB_END):
            uc.mem_write(a, b'\xc3')
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)
        # find the function end: the last RET with an immediate
        self.length = None

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

    def stub_write32(self, effects, address, value):
        data = struct.pack('<I', value & 0xFFFFFFFF)
        self.remember(address, 4)
        self.uc.mem_write(address, data)
        self.written.update(range(address, address + 4))
        effects += [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in ARGS:
            assert any(a <= address < a + n for a, n in ((self.entry, 0x800), (0x00E61010, 0x70), (0x00E68390, 0x6A0))), hex(address)
            self.visited.add(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = ARGS[address]
        self.remember(esp, 4 * (1 + nargs))
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        args = struct.unpack('<%dI' % nargs, uc.mem_read(esp + 4, 4 * nargs)) if nargs else ()
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        extra = []
        if address == UNIT:
            extra.append((args[1], 20))
        if address == RUN:
            count = self.u32(SCHED + 0x8C)
            extra.append((SCHED + 0x8C, 4))
            extra.append((SCHED + 0xC0, 12 * count))
        for a, n in extra:
            self.remember(a, n)
            window += [(a + i, b) for i, b in enumerate(uc.mem_read(a, n))]
        before = dict(regs=self.regs(), memory=window)
        effects = []
        eax, pop = rng.getrandbits(32), 4 + 4 * nargs
        if address == MEMCPY:
            dst, src, n = args
            self.remember(src, n)
            self.remember(dst, n)
            data = bytes(uc.mem_read(src, n))
            uc.mem_write(dst, data)
            self.written.update(range(dst, dst + n))
            effects += [(dst + i, b) for i, b in enumerate(data)]
            eax, pop = dst, 4
        elif address == ALLOC:
            eax = HEAP + 0x1000 * (len(self.events) % 0x30)
        elif address in (BEGIN, END):
            eax = rng.choice((0, 1, rng.getrandbits(32)))
        elif address == RUN and rng.random() < 0.5:
            # the run consumes the queue: the job counter may change
            self.stub_write32(effects, SCHED + 0x8C, 0 if rng.random() < 0.7 else rng.randrange(0, 5))
        if address == SORT and rng.random() < 0.3:
            ecx = uc.reg_read(x.UC_X86_REG_ECX)
            self.stub_write32(effects, ecx + 0x100, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        for r in (x.UC_X86_REG_ECX, x.UC_X86_REG_EDX):
            uc.reg_write(r, rng.getrandbits(32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def build(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x2000), (POOLS, 0x9000), (DESCS, 0x9000), (DATA, 0x20000), (SCHED, 0x1D000), (CTX, 0x1000),
                           (ALLOCATOR, 0x4000), (HEAP, 0x40000), (STACK, 0x10000)):
            uc.mem_write(base, b'\0' * size)
        lanes = rng.choice((0, 0, 1, 2, 4, 8))   # 0: serial
        # the singleton: vtable, eight pool lanes, lane count
        self.w32(THIS, UNIT_VT)
        self.w32(UNIT_VT + 0x3C, UNIT)
        self.w32(0x18D1848, THIS)
        self.w32(UNIT_VT + 0x2C, rng.choice((0x00E61010, 0x00E61010, STUB_BEGIN)))
        self.w32(UNIT_VT + 0x30, rng.choice((0x00E68390, 0x00E68390, STUB_END)))
        self.w32(THIS + 0xD0, rng.choice((0, 1, 2, 3, 8, 8)))
        selected = rng.randrange(0, 8)
        # pools: 4 per lane; each pool object has a descriptor with a count and an array
        for i in range(8):
            pool = POOLS + 0x1000 * i
            desc = DESCS + 0x1000 * i
            present = rng.random() < 0.85 or (i == selected and lanes == 0)
            self.w32(THIS + 0x30 + 4 * i, pool if present else 0)
            self.w32(pool + 4, desc)
            self.w32(pool + 0x54, rng.randrange(0, 3))
            for k in range(0, 0x40, 4):
                self.w32(pool + k + 0x100, rng.getrandbits(32))
            uc.mem_write(pool + 0x54 * 0 + 0, rng.randbytes(1) + b'\0\0\0')
            for d in range(3):
                uc.mem_write(pool + 12 * d, rng.randbytes(1))
            depth = rng.randrange(1, 4)
            self.w32(desc + 0x64, depth)
            count = rng.choice((0, 0, 1, 2, 5, 17, 40))
            self.w32(desc + 16 * depth - 4, count)
            array = DATA + 0x2000 * i
            self.w32(desc + 16 * depth - 8, array)
            for e in range(64):
                self.w32(array + 8 * e, rng.getrandbits(32))
                self.w32(array + 8 * e + 4, rng.choice((rng.randrange(0, 50), rng.getrandbits(32))))
        # the scheduler, allocator and consumer
        self.w32(0x18D0B28, SCHED)
        self.w32(SCHED + 0x8C, rng.choice((0, 0, 1, 3)))
        self.w32(0x187663C, ALLOCATOR)
        self.w32(ALLOCATOR, ALLOC_VT)
        self.w32(ALLOC_VT + 0x20, ALLOC)
        self.w32(ALLOC_VT + 0x24, FREE)
        uc.mem_write(THIS + 0x1B0, bytes([rng.choice((0, 1))]))
        self.w32(CTX + 0x58, selected)
        return lanes

    def case(self, seed):
        lanes = self.build(seed)
        rng = self.rng
        uc = self.uc
        self.initial, self.written, self.events, self.visited = {}, set(), [], set()
        esp = SP - 0x20 - 4 * rng.randrange(4)
        if self.which in ('vf11', 'begin'):
            args = [CTX, LANES, lanes, rng.choice((0, 1, 7))]
        else:
            args = [CTX, LANES, lanes]
        uc.mem_write(esp, struct.pack('<%dI' % (1 + len(args)), h.RET, *args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_EFLAGS, 0x202)
        self.remember(esp, 4 * (1 + len(args)))
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=400000)
        assert uc.reg_read(x.UC_X86_REG_EIP) == h.RET, 'instruction limit'
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n, which = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    oracle = Oracle(which)
    cases, visited = [], set()
    for seed in range(n):
        try:
            cases.append(oracle.case(seed))
        except Exception:
            print('failed seed', seed, 'EIP', hex(oracle.uc.reg_read(x.UC_X86_REG_EIP)), 'ESP', hex(oracle.uc.reg_read(x.UC_X86_REG_ESP)), flush=True)
            raise
        visited |= oracle.visited
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function=hex(oracle.entry), cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {len(visited)} instruction addresses visited -> {out}')
    open(out + '.visited', 'w').write('\n'.join('%08x' % a for a in sorted(visited)))


main()
