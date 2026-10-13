"""L184 s3 oracle: 0x00794550 (`ret 4`, one stack argument: a record that
enables or disables two linked objects, its +0x24 and +0x20) run as the
original code in Unicorn on randomized synthetic records. Its guest calls are
recorded stubs, except three trivial accessors the port does natively, which
run as original code here too: 0x010805D0 (element count), 0x011181F0
(store a float) and 0x00F65CB0 (store a float).
Output format: crates/mtcore/tests/semantic_s3.rs.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; the output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00794550, 0x21B  # through RET 4 at 0x00794768
# trivial callees run as the original code: address, length
NATIVE = ((0x010805D0, 0x11), (0x011181F0, 0x18), (0x00F65CB0, 0x0E))
# stub address -> stack argument words
STUBS = {0x01083990: 0, 0x01084210: 0, 0x010819E0: 0, 0x0088D660: 0, 0x010806C0: 1, 0x00794A90: 1}
JOB, E1, D1, A1, B1, E2, D2, A2 = 0x20000000, 0x21000000, 0x21001000, 0x21002000, 0x21003000, 0x22000000, 0x22001000, 0x22002000
G, GLOBAL = 0x23000000, 0x018D0B28
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((ENTRY, LENGTH), (JOB, 0x1000), (E1, 0x4000), (E2, 0x4000), (0x21004000, 0x1000), (G, 0x1000),
                              (GLOBAL, 4), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for a in STUBS:
            mapping(a, 1)
        for a, n in NATIVE:
            mapping(a, n)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a, n in NATIVE:
            uc.mem_write(a, h.code(a, n))
        for a in STUBS:
            uc.mem_write(a, b'\xc3')
        self.native_ranges = [(a, a + n) for a, n in NATIVE]
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

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in STUBS:
            assert ENTRY <= address < ENTRY + LENGTH or any(a <= address < b for a, b in self.native_ranges), hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = STUBS[address]
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        effects = []
        if rng.random() < 0.15:
            effects += self.stub_write(rng.choice((D1, D2)) + 0x68, struct.pack('<I', (rng.randrange(0, 4) << 8) | rng.randrange(256)))
        if rng.random() < 0.1:
            effects += self.stub_write(rng.choice((E1 + 0x64, E2 + 0x170)), struct.pack('<I', rng.randrange(1 << 32)))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((JOB, 0x1000), (E1, 0x4000), (E2, 0x4000), (0x21004000, 0x1000), (G, 0x1000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def w8(a, v):
            uc.mem_write(a, bytes([v]))

        def wf(a, v):
            uc.mem_write(a, struct.pack('<f', v))

        def number():
            return rng.choice((0.0, -0.0, 0.5, -1.0, 3.0, 1e-3, rng.uniform(-5, 5), float('nan'), float('inf')))

        w8(JOB + 0xC, rng.choice((0, 1, 1)))
        for off in (0xD, 0xE, 0xF, 0x10, 0x34, 0x35):
            w8(JOB + off, rng.choice((0, 1)))
        wf(JOB + 0x14, number())
        wf(JOB + 0x38, rng.choice((number(), 0.01, 0.0)))
        w32(JOB + 0x24, rng.choice((0, E1, E1)))
        w32(JOB + 0x20, rng.choice((0, E2, E2)))
        w32(GLOBAL, G)
        wf(G + 0x70, number())
        for e, d, a in ((E1, D1, A1), (E2, D2, A2)):
            w32(e + 0x64, rng.randrange(1 << 32))
            w32(e + 0x170, rng.randrange(1 << 32))
            w32(e + 0x38, d)
            w32(e + 0x4C, a)
            w32(e + 0x174, B1)
            w32(d + 0x68, (rng.randrange(0, 4) << 8) | rng.randrange(256))
            for i in range(4):
                w32(a + 4 * i, rng.randrange(1 << 32))
        for i in range(0x120 * 4 // 4):
            w32(B1 + 4 * i, rng.randrange(1 << 32))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(1, 5) - 4
        uc.mem_write(esp, struct.pack('<II', h.RET, JOB))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4] = esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    from collections import Counter
    tally = Counter(e['target'] for c in cases for e in c['events'])
    json.dump(dict(function='0x00794550', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}')
    print({hex(k): v for k, v in sorted(tally.items())})


main()
