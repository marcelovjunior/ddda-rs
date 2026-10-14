"""L184 s2 oracle: 0x00FAC6F0 `world->probe_segment(segment, query)` run as
the original's code in Unicorn on synthetic segments, thresholds and
constants. Its five guest callees are stubs that write random bytes into
the objects they are handed (the result buffer, the AABB, the segment, the
query) and return random values; each call is recorded (registers and
argument window before; effects and registers after) in the format of
crates/mtcore/tests/semantic_s2.rs. Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
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

ENTRY, LENGTH = 0x00FAC6F0, 0x3BC  # through RET 0x8 at 0x00FACAA9
CALLEES = {0x011C2810: 4, 0x01166C00: 2, 0x00DE46F0: 1, 0x011C3A30: 0}
VF11 = 0x00B16AF0  # sNetwork::vf11: a bare `ret 0xc`, run as the original (not a stub)
DATA_PAGES = (0x01621000, 0x01629000, 0x0162A000)
THIS, THIS_SIZE = 0x20000000, 0x20000
SEG = 0x21000000
QUERY = 0x22000000
HEAP = 0x23000000
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
        pages = {ENTRY & ~0xFFF, VF11 & ~0xFFF} | {a & ~0xFFF for a in CALLEES}
        for p in sorted(pages):
            uc.mem_map(p, 0x1000)
        for p in DATA_PAGES:
            uc.mem_map(p, 0x1000)
        for a, s in ((THIS, THIS_SIZE), (SEG, 0x1000), (QUERY, 0x1000), (HEAP, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(a, s)
        uc.mem_write(ENTRY, h.code(ENTRY, LENGTH))
        uc.mem_write(VF11, h.code(VF11, 3))
        for a in CALLEES:
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

    def rand_float(self):
        r = self.rng
        return bits(r.choice((r.uniform(-50, 50), r.uniform(-1e6, 1e6), 0.0, -0.0, math.nan, math.inf, 1e-30)))

    def code(self, uc, address, size, data):
        self.insns += 1
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in CALLEES:
            assert ENTRY <= address < ENTRY + LENGTH or VF11 <= address < VF11 + 3, hex(address)
            return
        rng = self.rng
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        ecx = uc.reg_read(x.UC_X86_REG_ECX)
        nargs = CALLEES[address]
        word = lambda a: struct.unpack('<I', uc.mem_read(a, 4))[0]
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 * (1 + nargs)))]
        before = dict(regs=self.regs(), memory=window)
        # the objects the callee is handed, as the original built them: only
        # the bytes the original wrote (the rest is stack garbage)
        pointers = {0x011C2810: [(word(esp + 12), 0x30), (word(esp + 16), 0x30), (ecx + 0xe4, 4)],
                    0x01166C00: [(word(esp + 4), 0x30), (word(esp + 8) + 0x94, 4)]}.get(address, [])
        before['objects'] = [(a + i, self.uc.mem_read(a + i, 1)[0]) for a, n in pointers for i in range(n) if a + i in self.written]
        effects = []
        if address == 0x011C2810:    # construct the result buffer; sometimes touches the segment
            for _ in range(rng.randrange(0, 4)):
                effects += self.stub_write(ecx + rng.choice((0, 4, 0x84, 0xe4)) + rng.randrange(4), [rng.randrange(256)])
            if rng.random() < 0.3:
                effects += self.stub_write(SEG + rng.choice((0, 4, 8, 0x10, 0x14, 0x18)), self.rand_float())
            if rng.random() < 0.15:
                effects += self.stub_write(0x0162ADB0, self.rand_float())
        elif address == 0x01166C00:  # collision test(shape, query)
            query = word(esp + 8)
            if rng.random() < 0.3:
                effects += self.stub_write(query + 0x98, struct.pack('<I', rng.choice((0, HEAP + 4 * rng.randrange(8)))))
            if rng.random() < 0.5:
                effects += self.stub_write(QUERY + 0x9c + rng.randrange(4), [rng.randrange(256)])
        elif address == 0x00DE46F0:
            effects += self.stub_write(HEAP + 0x100 + rng.randrange(0x20), [rng.randrange(256)])
        eax = rng.choice((0, 1, rng.randrange(1 << 32), rng.randrange(1 << 32)))
        if address == 0x01166C00 and rng.random() < 0.4:
            eax = 0
        back = word(esp)
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + 4 * nargs)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before, after=dict(regs=self.regs(), memory=effects)))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, b'\0' * THIS_SIZE)
        uc.mem_write(SEG, b'\0' * 0x1000)
        uc.mem_write(QUERY, b'\0' * 0x1000)
        uc.mem_write(HEAP, b'\0' * 0x1000)
        for p in DATA_PAGES:
            uc.mem_write(p, b'\0' * 0x1000)
        coords = [rng.uniform(-100, 100) for _ in range(6)]
        if rng.random() < 0.2:
            coords[3:] = coords[:3]          # a point
        if rng.random() < 0.15:
            coords[rng.randrange(6)] = rng.choice((math.nan, math.inf, 0.0))
        for pair in ((4, 1), (3, 0), (5, 2)):
            if rng.random() < 0.2:
                coords[pair[0]] = coords[pair[1]]
        segment = [coords[0], coords[1], coords[2], 0.0, coords[3], coords[4], coords[5], 0.0]
        for i, v in enumerate(segment):
            uc.mem_write(SEG + 4 * i, bits(v))
        # squared length, as the original adds it
        dx, dy, dz = f32(coords[3] - coords[0]), f32(coords[4] - coords[1]), f32(coords[5] - coords[2])
        sq = f32(f32(f32(dx * dx) + f32(dy * dy)) + f32(dz * dz))
        thr = rng.choice((sq, sq, 0.0, 1e9, math.nan, f32(sq * 0.5), f32(sq * 2 + 1), -1.0, rng.uniform(0, 1e4)))
        uc.mem_write(THIS + 0x115F8, bits(thr))
        for off in (0x0162ADB0, 0x01621124, 0x0162112C, 0x01629510, 0x01629514, 0x01629518):
            uc.mem_write(off, bits(rng.uniform(-3, 3)) if rng.random() < 0.7 else struct.pack('<I', rng.randrange(1 << 32)))
        uc.mem_write(QUERY + 0x94, struct.pack('<I', rng.randrange(1 << 32)))
        uc.mem_write(QUERY + 0x98, struct.pack('<I', rng.choice((0, HEAP, rng.randrange(1 << 32)))))
        self.initial, self.written, self.events, self.insns = {}, set(), [], 0
        esp = SP - 12 - 4 * rng.randrange(0, 8)
        uc.mem_write(esp, struct.pack('<III', h.RET, SEG, QUERY))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, self.uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after, insns=self.insns)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    nhit = sum(1 for c in cases if any(e['target'] == 0x00DE46F0 for e in c['events']))
    print('mean original instructions per call (incl. stub entries):', sum(c['insns'] for c in cases) / n)
    json.dump(dict(function='0x00FAC6F0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls, {nhit} hit -> {out}')


main()
