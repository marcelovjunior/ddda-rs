"""L184 oracle: 0x00872420 `uCnsMotFilter::vf24(joint, model)` (thiscall,
`ret 8`) run as the original's code in Unicorn on synthetic filters.

The function samples a pose matrix by calling one of eleven builders
(0x00828BD0 ... 0x00829C60, custom registers), optionally blends it toward
the rest pose with a slerp (CRT acosf 0x012F161F and sinf 0x012F0906,
SSE2 convention: argument and result in XMM0), then folds it into the
joint's local matrix (joint + 0x10), either directly or conjugated by the
inverse of the parent's world matrix (a joint record of the model, or the
model's root matrix from its vtable slot 0x58, 0x00 stubbed here).
All guest callees are stubs, recorded as events (the format of
crates/mtcore/tests/semantic.rs `abi_cases_full`, plus the XMM0 bits
before and after, for the CRT calls).

Each case randomises the filter fields (gate bytes, weight, kind, space
flag), the joint matrix, the parent chain, the pose matrices the builders
write (rotations of all four trace branches, near-identity, general,
singular, special values) and the stack alignment.
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

ENTRY, LENGTH = 0x00872420, 0x2913  # through RET 0x8 at 0x00874D30
CODE_PAGES = (0x00872000, 0x4000)  # the function and its jump/index tables
CONST_PAGES = (0x014F0000, 0x014F9000, 0x0150A000, 0x01521000, 0x01531000, 0x0161C000, 0x01629000)
# guest callees: address -> (name, bytes popped by its own ret)
HELPERS = {0x00828BD0: 0, 0x00828EC0: 0, 0x008291B0: 0, 0x008293F0: 0, 0x008296F0: 0, 0x00829EC0: 4,
           0x00829A00: 0, 0x00829C60: 0}
ACOS, SIN = 0x012F161F, 0x012F0906
VF58 = 0x30000058
THIS, OUT, MODEL, VTABLE = 0x20000000, 0x21000000, 0x22000000, 0x22001000
JOINTS, ROOT, SRC = 0x23000000, 0x24000000, 0x25000000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
EPS = struct.unpack('<f', struct.pack('<I', 0x34000000))[0]  # 1.1920929e-7

# Kinds that reach a builder, by the original's jump table (kind -> builder)
KINDS = [1, 2, 3, 5, 6, 9, 10, 14, 16, 18, 20]


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
    """Row-major 4x4 of the unit quaternion q = (x, y, z, w), the layout the
    filter's quaternion step reads back (rows are the basis vectors)."""
    x_, y_, z_, w_ = q
    return [1 - 2 * (y_ * y_ + z_ * z_), 2 * (x_ * y_ + z_ * w_), 2 * (x_ * z_ - y_ * w_), 0,
            2 * (x_ * y_ - z_ * w_), 1 - 2 * (x_ * x_ + z_ * z_), 2 * (y_ * z_ + x_ * w_), 0,
            2 * (x_ * z_ + y_ * w_), 2 * (y_ * z_ - x_ * w_), 1 - 2 * (x_ * x_ + y_ * y_), 0,
            0, 0, 0, 1]


def random_quat(rng):
    mode = rng.random()
    if mode < 0.12:  # near identity (slerp's linear branch)
        v = [rng.uniform(-1, 1) * 10 ** rng.uniform(-6, -3) for _ in range(3)] + [1.0]
    elif mode < 0.45:  # large rotations about a single axis (other trace branches)
        axis = rng.randrange(3)
        angle = rng.choice((math.pi, math.pi * 0.999, math.pi * 0.6, -2.5, 3.0, 2.0, 1.7))
        v = [0.0, 0.0, 0.0, math.cos(angle / 2)]
        v[axis] = math.sin(angle / 2) * rng.choice((1, -1))
    else:
        v = [rng.gauss(0, 1) for _ in range(4)]
    n = math.sqrt(sum(a * a for a in v)) or 1.0
    return [a / n for a in v]


def pose_matrix(rng):
    """The 16 floats a builder writes."""
    kind = rng.random()
    if kind < 0.55:
        m = quat_matrix(random_quat(rng))
        if rng.random() < 0.3:
            m[12:15] = [rng.uniform(-100, 100) for _ in range(3)]
        return m
    if kind < 0.65:
        m = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
        return m
    if kind < 0.75:
        return [0.0] * 16
    if kind < 0.9:
        return [rng.uniform(-2, 2) for _ in range(16)]
    m = quat_matrix(random_quat(rng))
    m[rng.randrange(16)] = rng.choice((math.nan, math.inf, -math.inf, -0.0, 1e-40, 3e38))
    return m


def transform_matrix(rng):
    """A joint/world matrix: rotation, scale and translation, sometimes
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
    if address == ACOS:
        return math.nan if not -1.0 <= value <= 1.0 else f32(math.acos(value))
    return math.nan if math.isinf(value) or math.isnan(value) else f32(math.sin(value))


class Oracle:
    def __init__(self):
        self.covered = set()
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(CODE_PAGES[0], CODE_PAGES[1])
        uc.mem_write(CODE_PAGES[0], h.code(CODE_PAGES[0], CODE_PAGES[1]))
        for page in CONST_PAGES:
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        for page in sorted({a & ~0xFFF for a in list(HELPERS) + [ACOS, SIN, VF58]}):
            if not (CODE_PAGES[0] <= page < CODE_PAGES[0] + CODE_PAGES[1]) and page not in CONST_PAGES:
                uc.mem_map(page, 0x1000)
        for address in list(HELPERS) + [ACOS, SIN, VF58]:
            uc.mem_write(address, b'\xc3')
        for base, size in ((THIS, 0x1000), (OUT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x1000), (ROOT, 0x1000),
                           (SRC, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
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

    def stub_write(self, address, data):
        self.written.update(range(address, address + len(data)))
        self.uc.mem_write(address, data)
        return [(address + i, b) for i, b in enumerate(data)]

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address not in HELPERS and address not in (ACOS, SIN, VF58):
            self.covered.add(address)
            assert CODE_PAGES[0] <= address < CODE_PAGES[0] + CODE_PAGES[1], hex(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        pop = {VF58: 4, ACOS: 0, SIN: 0}.get(address, HELPERS.get(address))
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 + pop + 4))]
        before = dict(regs=self.regs(), xmm0=self.xmm0(), memory=window)
        effects = []
        eax = uc.reg_read(x.UC_X86_REG_EAX)
        if address in HELPERS:
            buffer = uc.reg_read(x.UC_X86_REG_ESI)
            matrix = self.rng.choice(self.pose_choices)
            effects += self.stub_write(buffer, b''.join(struct.pack('<f', v) if not isinstance(v, int) else struct.pack('<I', v)
                                                        for v in matrix))
            eax = buffer
        elif address == VF58:
            eax = ROOT
        else:
            value = from_bits(self.xmm0())
            result = crt(address, value)
            uc.reg_write(x.UC_X86_REG_XMM0, bits(result))
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=address, before=before,
                                after=dict(regs=self.regs(), xmm0=self.xmm0(), memory=effects)))

    def put(self, address, values):
        self.uc.mem_write(address, b''.join(struct.pack('<f', v) for v in values))

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x1000), (OUT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x1000), (ROOT, 0x1000)):
            uc.mem_write(base, b'\0' * size)
        self.pose_choices = [pose_matrix(rng) for _ in range(3)]
        weight = rng.choice((0.0, EPS / 2, EPS, from_bits(0x34000001), 1e-5, 0.1, 0.3, 0.5, 0.75, 0.999, 0.9999999,
                             from_bits(0x3F7FFFFF), 1.0, 1.0000001, 1.5, 100.0, math.nan, -0.5, math.inf,
                             rng.uniform(0.001, 1.0), rng.uniform(0.001, 1.0), rng.uniform(0.001, 1.0)))
        kind = rng.choice(KINDS * 6 + [0, 4, 7, 8, 11, 12, 13, 15, 17, 19, 21, 22, 0x100, 0xFFFFFFFF])
        space = rng.choice((0, 0, 1, 1, 1, 7, 0x80000000))
        enabled = rng.choice((1, 1, 1, 1, 1, 1, 1, 1, 0, 2))
        suppressed = rng.choice((0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0xFF))
        source = rng.choice((SRC, SRC, SRC, SRC, SRC, 0))
        uc.mem_write(THIS + 0x44, struct.pack('<f', weight))
        uc.mem_write(THIS + 0x61, bytes([enabled]))
        uc.mem_write(THIS + 0x68, struct.pack('<I', kind))
        uc.mem_write(THIS + 0x6C, struct.pack('<I', space))
        uc.mem_write(THIS + 0x70, struct.pack('<I', source))
        uc.mem_write(THIS + 0x74, bytes([suppressed]))
        # the joint (OUT): local matrix at +0x10, parent index at +0x91
        self.put(OUT + 0x10, transform_matrix(rng))
        parent = rng.choice((0xFF, 0xFF, 0, 1, 2, 3))
        uc.mem_write(OUT + 0x91, bytes([parent]))
        # the model: vtable, joint array, root matrix
        uc.mem_write(MODEL, struct.pack('<I', VTABLE))
        uc.mem_write(VTABLE + 0x58, struct.pack('<I', VF58))
        uc.mem_write(MODEL + 0x364, struct.pack('<I', JOINTS))
        for k in range(4):
            self.put(JOINTS + 0xA0 * k + 0x10, transform_matrix(rng))
        self.put(ROOT, transform_matrix(rng))
        self.initial, self.written, self.events = {}, set(), []
        entry = SP - 12 - 4 * rng.randrange(4)
        uc.mem_write(entry, struct.pack('<III', h.RET, OUT, MODEL))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, entry
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        uc.reg_write(x.UC_X86_REG_XMM0, rng.randrange(1 << 32))
        # the stack bytes the call wrote, as the caller left them
        self.remember(entry, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=400000)
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        assert esp == entry + 12, (hex(esp), hex(entry))
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written) if a >= esp])
        initial = sorted(self.initial.items())
        return dict(seed=seed, before=before, initial=initial, events=self.events, after=after)


def main():
    out = sys.argv[1]
    n = int(sys.argv[2]) if len(sys.argv) > 2 else 4000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    by_target = {}
    for c in cases:
        for e in c['events']:
            by_target[hex(e['target'])] = by_target.get(hex(e['target']), 0) + 1
    # coverage: the instruction starts of the function the cases never ran
    # (the disassembly list is passed as a third argument when wanted)
    if len(sys.argv) > 3:
        starts = [int(l.split(':')[0], 16) for l in open(sys.argv[3]) if l[:8].strip() and l[8:9] == ':']
        missed = [hex(a) for a in starts if a not in oracle.covered]
        print(f'{len(starts) - len(missed)}/{len(starts)} instructions run; never run: {missed}')
    json.dump(dict(function='0x00872420', cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')
    print(by_target)


main()
