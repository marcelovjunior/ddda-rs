"""L184 s3 oracle: 0x007A5FA0 (ESI = a proxy-object record that mirrors its
settings into a runtime object) run as the original code in Unicorn on
randomized synthetic records. Every callee is a recorded stub (the import at
[0x0139D0D0] included); stubs randomly rewrite the runtime object's state byte
read after them. Output format: crates/mtcore/tests/semantic_s3.rs.
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

ENTRY, LENGTH = 0x007A5FA0, 0x31B  # through RET at 0x007A62BA
IMPORT_SLOT, IMPORT_STUB = 0x0139D0D0, 0x30000100
ALLOC, CTOR = 0x00F0E170, 0x00F12400
# stub address -> (words the callee pops, words of the call window to record)
STUBS = {IMPORT_STUB: (1, 1), ALLOC: (0, 2), CTOR: (0, 0), 0x00DB58B0: (4, 4), 0x00F23830: (0, 0), 0x00F23F80: (1, 1),
         0x00F23710: (0, 0), 0x00F0EA30: (1, 1), 0x00F0EB60: (1, 1), 0x00DE0B90: (0, 0), 0x00F0EBC0: (0, 0),
         0x00F0EBA0: (0, 0), 0x007A65C0: (1, 1)}
S, INFO, T24, P20 = 0x20000000, 0x21000000, 0x21001000, 0x21010000
OBJ, NEWOBJ = 0x22000000, 0x22001000
G, G_SIZE = 0x23000000, 0xC0000
GLOBAL_A, GLOBAL_G, GLOBAL_B = 0x018FA50C, 0x018FA4BC, 0x018D09E0
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

        for address, size in ((ENTRY, LENGTH), (S, 0x1000), (INFO, 0x1000), (T24, 0x1000), (P20, 0x3000), (OBJ, 0x2000),
                              (G, G_SIZE), (GLOBAL_A, 4), (GLOBAL_G, 4), (GLOBAL_B, 4), (IMPORT_SLOT, 4),
                              (0x30000000, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for a in STUBS:
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
        pop, window = STUBS[address]
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + window)))])
        effects = []
        if rng.random() < 0.3:  # the runtime object's state byte, read after the calls
            current = struct.unpack('<I', uc.mem_read(S + 0x28, 4))[0]
            if current:
                effects += self.stub_write(current + 4, bytes([rng.choice((4, 0, 3, rng.randrange(256)))]))
        eax = rng.randrange(1 << 32)
        if address == IMPORT_STUB:
            eax = rng.choice((0, 1, 2, 3, 5, 0xFFFFFFFF, 0x80000000))
        elif address == ALLOC:
            eax = rng.choice((0, NEWOBJ, NEWOBJ))
        elif address == CTOR:
            eax = uc.reg_read(x.UC_X86_REG_ECX) if rng.random() < 0.9 else 0
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((S, 0x1000), (INFO, 0x1000), (T24, 0x1000), (P20, 0x3000), (OBJ, 0x2000), (G, G_SIZE)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        def w8(a, v):
            uc.mem_write(a, bytes([v]))

        w32(IMPORT_SLOT, IMPORT_STUB)
        w32(GLOBAL_A, rng.randrange(1 << 32))
        w32(GLOBAL_G, G)
        w32(GLOBAL_B, rng.randrange(1 << 32))
        w32(G + 0xB883C, rng.choice((0, 0x10000, rng.randrange(1 << 32))))
        w32(S + 0x18, rng.randrange(1 << 32))
        w32(S + 0x1C, INFO)
        w32(S + 0x20, rng.choice((0, P20)))
        w32(S + 0x24, rng.choice((0, T24, T24)))
        w32(S + 0x28, rng.choice((0, OBJ)))
        w32(S + 0x38, rng.randrange(1 << 32))
        w32(S + 0x3C, rng.randrange(1 << 32))
        for off in (0x40, 0x44, 0x48):
            uc.mem_write(S + off, struct.pack('<f', rng.choice((0.5, -2.0, 1e-40, float('nan'), rng.uniform(-9, 9)))))
        w32(S + 0x4C, rng.randrange(1 << 32))
        for off in (0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, 0x5B, 0x5D, 0x5F):
            w8(S + off, rng.choice((0, 1, 1, rng.randrange(256))))
        w8(S + 0x62, rng.choice((0, 1)))
        w8(S + 0x63, rng.choice((0, 1)))
        w32(INFO + 4, rng.randrange(1 << 32))
        w32(INFO + 0x18, rng.randrange(1 << 32))
        w32(INFO + 0x1C, rng.randrange(1 << 32))
        w32(INFO + 0xF0, rng.choice((0, 1)))
        w8(T24 + 0x50, rng.choice((0, 1, 0x10, 0x11, rng.randrange(256))))
        uc.mem_write(T24 + 0x94, struct.pack('<f', rng.choice((0.25, float('nan'), rng.uniform(-9, 9)))))
        w8(P20 + 0x21B0, rng.choice((0, 1)))
        w32(OBJ + 4, rng.choice((4, 4, 0, 3, rng.randrange(1 << 32))))
        w32(OBJ + 0xDC, rng.randrange(1 << 32))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(1, 5)
        uc.mem_write(esp, struct.pack('<I', h.RET))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4], start[6] = esp, S
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4)
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
    json.dump(dict(function='0x007A5FA0', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}')
    print({hex(k): v for k, v in sorted(tally.items())})


main()
