"""L184 s3 oracle: 0x0076B470 (the scene object's per-frame update of its two
object lists, then of the 0xda8 list) run as the original code in Unicorn on
randomized synthetic scenes. Every callee is a recorded stub that randomly
rewrites the flags read after it. Output format: crates/mtcore/tests/
semantic_s3.rs. Run with E:/ddda-re/tools/pyenv/Scripts/python.exe;
the output is private.
"""
import json
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x0076B470, 0x1F0  # through the tail jump at 0x0076B65B
# stubbed callees: address -> stack argument words
DIRECT = {0x0076F2D0: 1, 0x00F77BF0: 1, 0x00773830: 0, 0x01106E20: 2, 0x01107D70: 1, 0x00ED5370: 6,
          0x00ED5360: 2, 0x00779BF0: 1}
VCALL = 0x30000000 + 0x3C
STUBS = dict(DIRECT)
STUBS[VCALL] = 3
SCENE, SCENE_SIZE = 0x20000000, 0x2000
LIST_A, LIST_B, LIST_C, OBJS, VT = 0x21000000, 0x21001000, 0x21002000, 0x21010000, 0x21008000
GLOBAL_ECX, GLOBAL_F = 0x018D0DD0, 0x018CFE80
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

        for address, size in ((ENTRY, LENGTH), (SCENE, SCENE_SIZE), (0x21000000, 0x20000), (GLOBAL_ECX, 4),
                              (GLOBAL_F, 16), (0x30000000, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
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

    def side_effects(self, rng):
        fx = []
        if rng.random() < 0.2:
            fx += self.stub_write(SCENE + 0xCC4, struct.pack('<I', rng.choice((0, 0x1234, rng.randrange(1 << 32)))))
        if rng.random() < 0.2:
            fx += self.stub_write(SCENE + 0xD98, bytes([rng.choice((0, 1))]))
        if rng.random() < 0.3 and self.objects:
            obj = OBJS + 0x200 * rng.randrange(self.objects)
            fx += self.stub_write(obj + 0x1C0, bytes([rng.randrange(256)]))
        if rng.random() < 0.1:
            fx += self.stub_write(SCENE + 0xDA8, struct.pack('<I', rng.randrange(0, 4)))
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
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        effects = self.side_effects(rng)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, rng.randrange(1 << 32))
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((SCENE, SCENE_SIZE), (0x21000000, 0x20000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        self.objects = rng.randrange(0, 6)
        for k in range(self.objects):
            obj = OBJS + 0x200 * k
            w32(obj, VT)
            w32(obj + 0x150, rng.randrange(0, 8))
            w32(obj + 0x18C, rng.choice((0, 1, 2, 3, 4, 5, rng.randrange(1 << 32))))
            uc.mem_write(obj + 0x1C0, bytes([rng.choice((0x10, 0x10, 0, rng.randrange(256)))]))
        w32(VT + 0x3C, VCALL)

        def pick():
            return rng.choice((0, OBJS + 0x200 * rng.randrange(max(self.objects, 1))) if self.objects else (0,))

        counts = [rng.randrange(0, 5) for _ in range(3)]
        for lst, count, at, pointer in ((LIST_A, counts[0], 0xAB8, 0xAC4), (LIST_B, counts[1], 0xAE0, 0xAEC)):
            w32(SCENE + at, count)
            w32(SCENE + pointer, lst)
            for i in range(count):
                w32(lst + 4 * i, pick())
        w32(SCENE + 0xDA8, counts[2])
        w32(SCENE + 0xDB4, LIST_C)
        for i in range(counts[2]):
            w32(LIST_C + 4 * i, rng.randrange(1 << 32))
        for k in range(8):
            w32(SCENE + 0xCD4 + 4 * k, rng.randrange(1 << 32))
        w32(SCENE + 0xCC4, rng.choice((0, 0, 0x77, rng.randrange(1 << 32))))
        uc.mem_write(SCENE + 0xD04, struct.pack('<H', rng.randrange(1 << 16)))
        uc.mem_write(SCENE + 0xD98, bytes([rng.choice((0, 0, 1))]))
        w32(SCENE + 0xD44, rng.randrange(1 << 32))
        w32(SCENE + 0xD48, rng.randrange(1 << 32))
        for off in range(0x890, 0x8D4, 4):
            w32(SCENE + off, rng.randrange(1 << 32))
        w32(GLOBAL_ECX, rng.randrange(1 << 32))
        for i in range(3):
            w32(GLOBAL_F + 4 * i, rng.randrange(1 << 32))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(1, 5) - 4
        uc.mem_write(esp, struct.pack('<II', h.RET, SCENE))
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
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    from collections import Counter
    tally = Counter(e['target'] for c in cases for e in c['events'])
    json.dump(dict(function='0x0076B470', cases=cases), open(out, 'w'))
    print(f'{n} cases -> {out}')
    print({hex(k): v for k, v in sorted(tally.items())})


main()
