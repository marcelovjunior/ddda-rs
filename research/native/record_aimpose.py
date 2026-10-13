"""L184 oracle: 0x00829EC0, the motion filter's "aim and twist" pose builder
(kinds 6, 10, 14 and 18 of 0x00872420), run as the original's code in
Unicorn. Custom registers: ECX = the joint record, ESI = the 16-float
output matrix, EDI = the model, one stack argument (the pose source, whose
float at +0x538 is the angle in degrees), `ret 4`; EAX = ESI on return.

It writes the identity matrix, then (by the joint's byte at +0x93: 6, 10,
14, 18 select an axis table) rotates it about the direction from the joint
to a partner joint of the model, expressed in the joint's own frame, and
about the Z axis. The CRT sinf (0x012F0906) and cosf (0x012F0A55) are
guest calls, stubbed and recorded with their XMM0 values (events in the
format of crates/mtcore/tests/semantic.rs plus "xmm0").
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

ENTRY = 0x00829EC0
CODE_PAGES = (0x00829000, 0x3000)  # the function and its jump/index tables
CONST_PAGES = (0x014F0000, 0x014F9000, 0x0150A000, 0x01521000, 0x01531000, 0x0161C000, 0x01629000, 0x01832000)
SIN, COS = 0x012F0906, 0x012F0A55
JOINT, MODEL, JOINTS, TABLE, SOURCE, OUT = 0x21000000, 0x22000000, 0x23000000, 0x24000000, 0x25000000, 0x26000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    try:
        return struct.unpack('<f', struct.pack('<f', v))[0]
    except OverflowError:
        return math.copysign(math.inf, v)


def bits(v):
    return struct.unpack('<I', struct.pack('<f', v))[0]


def from_bits(b):
    return struct.unpack('<f', struct.pack('<I', b))[0]


def quat_matrix(q):
    x_, y_, z_, w_ = q
    return [1 - 2 * (y_ * y_ + z_ * z_), 2 * (x_ * y_ + z_ * w_), 2 * (x_ * z_ - y_ * w_), 0,
            2 * (x_ * y_ - z_ * w_), 1 - 2 * (x_ * x_ + z_ * z_), 2 * (y_ * z_ + x_ * w_), 0,
            2 * (x_ * z_ + y_ * w_), 2 * (y_ * z_ - x_ * w_), 1 - 2 * (x_ * x_ + y_ * y_), 0,
            0, 0, 0, 1]


def random_quat(rng):
    v = [rng.gauss(0, 1) for _ in range(4)]
    n = math.sqrt(sum(a * a for a in v)) or 1.0
    return [a / n for a in v]


def transform_matrix(rng):
    """A joint world matrix: rotation, scale and translation, sometimes
    singular, sometimes general or non-finite."""
    kind = rng.random()
    if kind < 0.6:
        m = quat_matrix(random_quat(rng))
        s = [rng.choice((1.0, 1.0, rng.uniform(0.2, 3))) for _ in range(3)]
        for r in range(3):
            for c in range(3):
                m[4 * r + c] *= s[r]
        m[12:15] = [rng.uniform(-50, 50) for _ in range(3)]
        return m
    if kind < 0.7:
        return [0.0] * 16
    if kind < 0.8:  # integer rows, one duplicated: determinant exactly zero
        m = [float(rng.randrange(-3, 4)) for _ in range(16)]
        a, b = rng.sample(range(4), 2)
        m[4 * a:4 * a + 4] = m[4 * b:4 * b + 4]
        return m
    if kind < 0.9:
        return [rng.uniform(-3, 3) for _ in range(16)]
    m = quat_matrix(random_quat(rng))
    m[rng.randrange(16)] = rng.choice((math.nan, math.inf, -0.0, 1e-30))
    return m


def crt(address, value):
    """The CRT results the stubs return (any deterministic function)."""
    if math.isinf(value) or math.isnan(value):
        return math.nan
    return f32(math.sin(value) if address == SIN else math.cos(value))


class Oracle:
    def __init__(self):
        self.covered = set()
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        # the whole exe image: sinf/cosf run as the original's code (the port
        # computes them natively) except for arguments it hands back
        for va, data in h.image()[1]:
            lo, hi = va & ~0xFFF, (va + len(data) + 0xFFF) & ~0xFFF
            uc.mem_map(lo, hi - lo)
            uc.mem_write(va, data)
        for base, size in ((JOINT, 0x1000), (MODEL, 0x1000), (JOINTS, 0x1000), (TABLE, 0x1000), (SOURCE, 0x1000),
                           (OUT, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(base, size)
        uc.hook_add(UC_HOOK_MEM_READ, self.read)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.write)
        uc.hook_add(UC_HOOK_CODE, self.code)

    def remember(self, address, size):
        for i, b in enumerate(self.uc.mem_read(address, size)):
            a = address + i
            if a not in self.written:
                self.initial.setdefault(a, b)

    def read(self, uc, access, address, size, value, data):
        self.remember(address, size)

    def write(self, uc, access, address, size, value, data):
        self.written.update(range(address, address + size))

    def regs(self):
        return [self.uc.reg_read(r) for r in REGS]

    def xmm0(self):
        return self.uc.reg_read(x.UC_X86_REG_XMM0) & 0xFFFFFFFF

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in (SIN, COS):
            self.covered.add(address)
            return
        v = from_bits(self.xmm0())
        if not (math.isfinite(v) and abs(v) >= 8192.0):
            return  # the real sinf/cosf runs (the port handles all but large finite arguments)
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4))]
        before = dict(regs=self.regs(), xmm0=self.xmm0(), memory=window)
        result = crt(address, from_bits(self.xmm0()))
        uc.reg_write(x.UC_X86_REG_XMM0, bits(result))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before,
                                after=dict(regs=self.regs(), xmm0=self.xmm0(), memory=[])))

    def put(self, address, values):
        self.uc.mem_write(address, b''.join(struct.pack('<f', v) for v in values))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((JOINT, 0x1000), (MODEL, 0x1000), (JOINTS, 0x1000), (TABLE, 0x1000), (SOURCE, 0x1000),
                           (OUT, 0x1000)):
            uc.mem_write(base, b'\0' * size)
        kind = rng.choice((6, 10, 14, 18) * 8 + (0, 5, 7, 11, 17, 19, 100, 255))
        uc.mem_write(JOINT + 0x93, bytes([kind]))
        matrix = transform_matrix(rng)
        tiny = rng.random() < 0.15  # partner at a distance around the epsilon, from the origin
        if tiny:
            matrix[12:15] = [0.0, 0.0, 0.0]
        self.put(JOINT + 0x10, matrix)
        angle = rng.choice((0.0, -0.0, 1e-9, 1e-5, 0.01, -0.01, 0.5, -0.5, 3.0, -3.0, 20.0, -20.0, 45.0, -45.0, 90.0,
                            179.0, 1000.0, -1000.0, 9000.0, -90000.0, math.nan, math.inf, rng.uniform(-60, 60), rng.uniform(-60, 60),
                            rng.uniform(-60, 60), rng.uniform(-5, 5)))
        uc.mem_write(SOURCE + 0x538, struct.pack('<f', angle))
        no_joints = rng.random() < 0.05  # a null joint array: joint 0 is "no joint"
        uc.mem_write(MODEL + 0x364, struct.pack('<I', 0 if no_joints else JOINTS))
        uc.mem_write(MODEL + 0x368, struct.pack('<I', TABLE))
        for bone in range(256):
            uc.mem_write(TABLE + bone, bytes([rng.choice((0xFF, 0xFF, 0, 1, 2, 3))]))
        # the bone this kind aims at is usually present
        if no_joints:
            uc.mem_write(TABLE + ((kind + 2) & 0xFF), bytes([rng.choice((0, 0xFF))]))
        elif rng.random() < 0.85:
            uc.mem_write(TABLE + ((kind + 2) & 0xFF), bytes([rng.choice((0, 0, 1, 2, 3))]))
        for k in range(4):
            m = transform_matrix(rng)
            if rng.random() < 0.3:  # a partner at (nearly) the joint's own position
                m[12:15] = matrix[12:15] if rng.random() < 0.5 else [matrix[12], matrix[13], matrix[14] + rng.choice((0.0, 1e-9, 1e-3))]
            if tiny:
                m[12:15] = [0.0, 0.0, rng.choice((1e-7, 1.1e-7, 1.19e-7, 1.2e-7, 1.3e-7, 8e-8, 2e-7))]
            self.put(JOINTS + 0xA0 * k + 0x10, m)
        self.initial, self.written, self.events = {}, set(), []
        entry = SP - 8 - 4 * rng.randrange(4)
        uc.mem_write(entry, struct.pack('<II', h.RET, SOURCE))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4], start[6], start[7] = JOINT, entry, OUT, MODEL
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_XMM0, rng.randrange(1 << 32))
        self.remember(entry, 8)
        before = dict(regs=self.regs())
        try:
            uc.emu_start(ENTRY, h.RET, count=400000)
        except Exception:
            print('seed', seed, 'eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()])
            raise
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        assert esp == entry + 8, (hex(esp), hex(entry))
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if a >= esp])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    if len(sys.argv) > 3:
        starts = [int(l.split(':')[0], 16) for l in open(sys.argv[3]) if l[:8].strip() and l[8:9] == ':']
        missed = [a for a in starts if a not in oracle.covered]
        print(f'{len(starts) - len(missed)}/{len(starts)} instructions run; never run: '
              f'{hex(min(missed))}..{hex(max(missed))} ({len(missed)})' if missed else 'all instructions run')
    json.dump(dict(function='0x00829EC0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
