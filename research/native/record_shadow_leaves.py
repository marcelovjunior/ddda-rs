"""L184 s3 oracle: four small functions of the shadow job run as the original
code in Unicorn on randomized synthetic objects, their guest callees stubbed
and recorded.

  copy16  0x00EA1EB0  obj->copy_position(out) -> bool    (thiscall, ret 4)
  origin  0x00EA1F40  obj->world_position(out) -> out    (thiscall, ret 4; one virtual call)
  flags   0x00EA1EE0  obj->classify(other) -> mask       (thiscall, ret 4; 0x00FF0780, 0x00FF0AD0)
  any     0x00E61010  obj->test_all(a, b, c, d) -> bool  (thiscall, ret 0x10; 0x0111D610)

Output format: crates/mtcore/tests/semantic_s3.rs.
Usage: record_shadow_leaves.py MODE OUT.json [cases]
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

# mode -> (entry, length, stack argument words of the function, {stub: argument words})
MODES = {
    'copy16': (0x00EA1EB0, 0x30, 1, {}),
    'origin': (0x00EA1F40, 0x8B, 1, {0x30000030: 1}),
    'flags': (0x00EA1EE0, 0x5C, 1, {0x00FF0780: 1, 0x00FF0AD0: 2}),
    'any': (0x00E61010, 0x6E, 4, {0x0111D610: 1}),
}
OBJ, PARENTS, OUT, OTHER, TABLE = 0x20000000, 0x21000000, 0x22000000, 0x23000000, 0x24000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
NAN = struct.unpack('<f', struct.pack('<I', 0x7FC00000))[0]


class Oracle:
    def __init__(self, mode):
        self.mode = mode
        self.entry, length, self.nargs, self.stubs = MODES[mode]
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        mapped = set()

        def mapping(address, size):
            for page in range(address & ~0xFFF, address + size, 0x1000):
                if page not in mapped:
                    mapped.add(page)
                    uc.mem_map(page, 0x1000)

        for address, size in ((self.entry, length), (OBJ, 0x1000), (PARENTS, 0x4000), (OUT, 0x1000), (OTHER, 0x1000),
                              (TABLE, 0x1000), (0x30000000, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            mapping(address, size)
        for a in self.stubs:
            mapping(a, 1)
        uc.mem_write(self.entry, h.code(self.entry, length))
        for a in self.stubs:
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
        if address not in self.stubs:
            assert self.entry <= address < self.entry + 0x100, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = self.stubs[address]
        before = dict(regs=self.regs(), memory=[(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))])
        effects = []
        eax = rng.randrange(1 << 32)
        if self.mode == 'origin':  # the root's own position, through the out pointer
            out = struct.unpack('<I', uc.mem_read(esp + 4, 4))[0]
            effects += self.stub_write(out, b''.join(struct.pack('<f', self.number()) for _ in range(3)))
            eax = rng.choice((0, 1, 0x100, 0x1FF, rng.randrange(1 << 32)))
        elif self.mode == 'flags':
            eax = rng.choice((0, 1, 2, 0x3F, 0x3E, 0xFFFFFFFF, 0x80000000, 0x7FFFFFFF, rng.randrange(1 << 32)))
        elif self.mode == 'any':
            eax = rng.choice((0, 0, 1, 0x100, rng.randrange(1 << 32)))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def number(self):
        # quiet NaNs only: Unicorn does not quiet a signalling NaN on x87 fld
        return self.rng.choice((0.0, -0.0, 1.5, -2.25, NAN, float('inf'), 1e-40, self.rng.uniform(-9, 9)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((OBJ, 0x1000), (PARENTS, 0x4000), (OUT, 0x1000), (OTHER, 0x1000), (TABLE, 0x1000)):
            uc.mem_write(base, b'\0' * size)

        def w32(a, v):
            uc.mem_write(a, struct.pack('<I', v & 0xFFFFFFFF))

        args = [OUT]
        if self.mode == 'copy16':
            w32(OBJ + 0xF0, rng.choice((0, 1, 0x55)))
            uc.mem_write(OBJ + 0x160, bytes(rng.randrange(256) for _ in range(16)))
            uc.mem_write(OUT, bytes(rng.randrange(256) for _ in range(16)))
        elif self.mode == 'origin':
            for off in (0x160, 0x164, 0x168):
                uc.mem_write(OBJ + off, struct.pack('<f', self.number()))
            uc.mem_write(OUT, bytes(rng.randrange(256) for _ in range(16)))
            depth = rng.choice((0, 1, 2, 3))
            previous = OBJ
            vtable = PARENTS + 0x3000
            w32(vtable + 0x30, 0x30000030)
            for k in range(depth):
                parent = PARENTS + 0x400 * k
                w32(previous + 0x30, parent)
                w32(parent, vtable)
                previous = parent
            w32(OBJ, vtable)
        elif self.mode == 'flags':
            uc.mem_write(OBJ + 0x108, bytes([rng.choice((0, 0x40, 1, 2, 0x3F, 0x81, rng.randrange(256)))]))
            args = [OTHER]
        elif self.mode == 'any':
            w32(OTHER + 0x58, rng.randrange(0, 6))
            w32(OBJ + 0xD0, rng.randrange(0, 6))
            for k in range(6):
                w32(OBJ + 0x30 + 4 * k, rng.randrange(1 << 32))
            args = [OTHER, rng.randrange(1 << 32), rng.choice((0, 0, 1, 7)), rng.randrange(1 << 32)]
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 4 * rng.randrange(0, 4) - 4 * (len(args) + 1)
        uc.mem_write(esp, struct.pack('<I', h.RET) + b''.join(struct.pack('<I', a) for a in args))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = OBJ, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 4 + 4 * len(args))
        before = dict(regs=self.regs())
        uc.emu_start(self.entry, h.RET, count=10000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        record = dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after, insns=self.insns)
        # the x87 stack is balanced; reset it for the next case anyway
        uc.reg_write(x.UC_X86_REG_FPSW, 0)
        uc.reg_write(x.UC_X86_REG_FPTAG, 0xFFFF)
        return record


def main():
    mode, out = sys.argv[1], sys.argv[2]
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 3000
    oracle = Oracle(mode)
    cases = [oracle.case(seed) for seed in range(n)]
    from collections import Counter
    tally = Counter(e['target'] for c in cases for e in c['events'])
    json.dump(dict(function=hex(MODES[mode][0]), cases=cases), open(out, 'w'))
    print(f'{mode}: {n} cases -> {out}', {hex(k): v for k, v in sorted(tally.items())})


main()
