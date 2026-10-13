"""L184 s2 oracle: 0x0079E200 `sweep->step(pos, next, anchor, query,
reanchor)` (this in ESI, ret 0x14) run as the original's code in Unicorn
on synthetic sweep objects. The ten tiny callees (the collision parameter
block's methods at 0x00DE45C0, 0x00DE4710, 0x00DE4730, 0x00DE4780,
0x00DE48B0, 0x00DE4920, 0x00DE4B70, 0x012BB7A0 and 0x004038F0) run their
ORIGINAL code, so the port's inlined versions are checked against them;
the seven collision entry points (0x010AF350, 0x010B4710, 0x010B4560,
0x010B4960, 0x010FEA80, 0x0111B2B0, 0x0111B570) are stubs that write random
floats into the vectors they are handed and return random flags. At every
stub call the bytes the original has written to its own stack frame so far
are recorded ("objects"), so the port must have built the same parameter
blocks and vectors. Format: crates/mtcore/tests/semantic_s2.rs.
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import math
import random
import struct
import sys

sys.path.insert(0, str(__import__('pathlib').Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x0079E200, 0x8B4  # through RET 0x14 at 0x0079EAB1
STUBS = {0x010AF350: 4, 0x010B4710: 4, 0x010B4560: 4, 0x010B4960: 4, 0x010FEA80: 4, 0x0111B2B0: 4, 0x0111B570: 5}
REAL = {0x00DE45C0: 0x130, 0x00DE48B0: 0x70, 0x00DE4780: 0x120, 0x00DE4B70: 0x60, 0x00DE4710: 0x20,
        0x00DE4730: 0x30, 0x00DE4920: 8, 0x012BB7A0: 0x10, 0x004038F0: 8}
FLOATS = [0x0150A350, 0x0150A088, 0x014F0E9C, 0x014EAE5C, 0x01629540, 0x01629544, 0x01629548,
          0x0142BC50, 0x01408E58, 0x0142BAC8, 0x0140712C, 0x0142AF90, 0x018CFE80, 0x018CFE84, 0x018CFE88]
OTHER = [0x0161C9F0, 0x0161C9F4, 0x0161C280, 0x018FA4E4, 0x018D0DD0]
THIS, WORLD, BLOB = 0x20000000, 0x22000000, 0x21000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    try:
        return struct.unpack('<f', struct.pack('<f', v))[0]
    except OverflowError:
        return math.copysign(math.inf, v)


def bits(v):
    return struct.pack('<f', f32(v))


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        pages = {ENTRY & ~0xFFF} | {a & ~0xFFF for a in list(STUBS) + list(REAL)}
        pages |= {a & ~0xFFF for a in FLOATS + OTHER}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for a, s in ((THIS, 0x1000), (BLOB, 0x1000), (WORLD, 0xA0000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        for a, n in REAL.items():
            uc.mem_write(a, h.code(a, n))
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
        data = bytes(data)
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
            ok = ENTRY <= address < ENTRY + LENGTH or any(a <= address < a + n for a, n in REAL.items())
            assert ok, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        nargs = STUBS[address]
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        # the bytes of its own frame the original has written so far
        frame = self.frame
        before['objects'] = [(a, uc.mem_read(a, 1)[0]) for a in range(frame + 8, frame + 0x1e0) if a in self.written]
        effects = []
        args = [word(esp + 4 + 4 * k) for k in range(nargs)]
        targets = [args[0]] + ([args[1]] if address == 0x0111B570 else [])
        for t in targets:
            if rng.random() < 0.5:
                for k in range(3):
                    effects += self.stub_write(t + 4 * k, bits(rng.uniform(-60, 60)))
        eax = rng.choice((0, 8, 8, 9, 1, rng.randrange(1 << 32), rng.randrange(1 << 32) | 8))
        back = word(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for a, n in ((THIS, 0x1000), (BLOB, 0x1000), (WORLD, 0xA0000)):
            uc.mem_write(a, b'\0' * n)
        for p in {a & ~0xFFF for a in FLOATS + OTHER}:
            uc.mem_write(p, b'\0' * 0x1000)

        def fl(lo=-50.0, hi=50.0):
            r = rng.random()
            if r < 0.04:
                return rng.choice((0.0, -0.0, 1e-40, -1e-40, 1.0e-7))
            return rng.uniform(lo, hi)

        def put(address, value):
            uc.mem_write(address, bits(value))

        for a in FLOATS:
            put(a, fl(-3, 3))
        put(0x0150A350, rng.choice((1.1920929e-07, 1.1920929e-07, 1.1920929e-07, 0.5, 1e-9)))
        uc.mem_write(0x0161C9F0, struct.pack('<d', rng.choice((1.4142135623730951, 1.4142135623730951, rng.uniform(-2, 2)))))
        uc.mem_write(0x0161C280, struct.pack('<I', rng.choice((0x80000000, 0x80000000, 0x80000000, rng.randrange(1 << 32)))))
        uc.mem_write(0x018FA4E4, struct.pack('<I', WORLD))
        uc.mem_write(WORLD + 0x9C448, bytes([rng.choice((1, 1, 1, 0))]))
        uc.mem_write(0x018D0DD0, struct.pack('<I', rng.randrange(1 << 32)))
        # the sweep object
        d = [fl() for _ in range(3)]
        e = list(d) if rng.random() < 0.3 else [fl() for _ in range(3)]
        if rng.random() < 0.5:
            e[1] = d[1] + rng.choice((-1, 1)) * rng.uniform(0.01, 5)
        for off, v in zip((0xD0, 0xD4, 0xD8), d):
            put(THIS + off, v)
        for off, v in zip((0xE0, 0xE4, 0xE8), e):
            put(THIS + off, v)
        put(THIS + 0xF0, rng.uniform(0.1, 3))
        for off in (0x100, 0x104, 0x108, 0x190, 0x194, 0x1A4):
            put(THIS + off, fl(-2, 2))
        for off in (0x110, 0x114, 0x118, 0x120, 0x124, 0x128, 0x130, 0x1A8):
            uc.mem_write(THIS + off, struct.pack('<I', rng.randrange(1 << 32)))
        for off in (0x1AC, 0x1AD, 0x1AE, 0x1AF, 0x1B0, 0x1B1):
            uc.mem_write(THIS + off, bytes([rng.choice((0, 1)) if off != 0x1AC else rng.choice((0, 0, 0, 0, 0, 1))]))
        uc.mem_write(THIS + 0x1A0, struct.pack('<I', rng.choice((0, 1, 2, 3, 1, 2))))
        plain = rng.random() < 0.5
        uc.mem_write(THIS + 0x44, struct.pack('<I', 0 if plain else rng.choice((0, 5))))
        uc.mem_write(THIS + 0x28, struct.pack('<I', 0 if plain else rng.choice((0, 7))))
        uc.mem_write(THIS + 0x2C, struct.pack('<I', 0xFFFFFFFF if plain else rng.choice((0xFFFFFFFF, 3))))
        uc.mem_write(THIS + 0x42, bytes([0 if plain else rng.choice((0, 1))]))
        # arguments: pos, next, anchor (16-byte vectors), query with a float at +0x14
        pos, nxt, anchor, query = BLOB, BLOB + 0x100, BLOB + 0x200, BLOB + 0x300
        for base in (pos, nxt, anchor):
            for k in range(3):
                put(base + 4 * k, fl())
        put(query + 0x14, rng.choice((fl(0, 3), 0.0, 1e-9)))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 24 - 4 * rng.randrange(0, 8)
        reanchor = rng.choice((0, 1, 1)) | (rng.randrange(1 << 24) << 8 if rng.random() < 0.5 else 0)
        uc.mem_write(esp, struct.pack('<IIIIII', h.RET, pos, nxt, anchor, query, reanchor))
        self.frame = ((esp - 4) & ~15) - 0x1D8 - 8
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[4], start[6] = esp, THIS
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 24)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    kinds = {}
    for c in cases:
        for e in c['events']:
            kinds[hex(e['target'])] = kinds.get(hex(e['target']), 0) + 1
    print('mean original instructions per call (incl. stub entries):', sum(c['insns'] for c in cases) / n)
    json.dump(dict(function='0x0079E200', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls {kinds} -> {out}')


main()
