"""L184 oracle: 0x00872420 `uCnsMotFilter::vf24(joint, model)` (thiscall,
`ret 8`) run as the original's code in Unicorn on synthetic filters.

The function builds a pose matrix with one of eleven pose routines
(0x00828BD0 ... 0x00829EC0, with 0x007E4B60 and 0x00CFC9E0 below them),
optionally blends it toward the rest pose with a slerp, then folds it into
the joint's local matrix (joint + 0x10), either directly or conjugated by
the inverse of the parent's world matrix (a joint record of the model, or
the model's root matrix from its vtable slot 0x58, stubbed here).
The pose routines run as the original's code. Stubbed and recorded as
events (the format of crates/mtcore/tests/semantic.rs, plus the XMM0 bits
before and after): the CRT's sinf, cosf, acosf, asinf and double atan2
(XMM0 = y, XMM1 = x as doubles) and the model's root-matrix method. The
CRT's double sin(0.0) and cos(0.0) (0x012F1E8A, 0x012F2042) are answered
silently with exactly 0.0 and 1.0 (as the real code does: the port
replaces them by constants).

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
CODE_RANGES = ((0x00828000, 0x5000), (0x00872000, 0x4000), (0x007E4000, 0x1000), (0x00CFC000, 0x1000))
CONST_PAGES = (0x014F0000, 0x014F9000, 0x0150A000, 0x01521000, 0x01531000, 0x0161C000, 0x01629000, 0x01406000, 0x01517000,
               0x01518000, 0x015E6000, 0x01520000, 0x01606000, 0x01832000)
# stubbed guest callees (CRT; arguments and results in XMM0/XMM1)
ACOS, SIN, COS, ASIN, ATAN2 = 0x012F161F, 0x012F0906, 0x012F0A55, 0x012F0D4C, 0x012F1109
SIN0, COS0 = 0x012F1E8A, 0x012F2042  # double sin(0.0), cos(0.0): answered silently
SILENT = (SIN0, COS0)
RECORDED = (SIN, COS)  # acosf, asinf and atan2 run as the original's code (the port computes them natively)
TABLE = 0x26000000
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
    if address == ASIN:
        return math.nan if not -1.0 <= value <= 1.0 else f32(math.asin(value))
    if math.isinf(value) or math.isnan(value):
        return math.nan
    return f32(math.sin(value) if address == SIN else math.cos(value))


class Oracle:
    def __init__(self):
        self.covered = set()
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        # the whole exe image: the pose routines and the CRT's sinf/cosf run
        # as the original's code (the port computes them natively), except
        # for arguments the port hands back to the guest (see code())
        for va, data in h.image()[1]:
            lo, hi = va & ~0xFFF, (va + len(data) + 0xFFF) & ~0xFFF
            uc.mem_map(lo, hi - lo)
            uc.mem_write(va, data)
        for address in list(RECORDED) + [SILENT[0], SILENT[1], VF58]:
            if address in (SIN, COS):
                continue
            if address != VF58:
                uc.mem_write(address, b'\xc3')
        uc.mem_map(VF58 & ~0xFFF, 0x1000)
        uc.mem_write(VF58, b'\xc3')
        for base, size in ((THIS, 0x1000), (OUT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x1000), (ROOT, 0x1000),
                           (SRC, 0x1000), (TABLE, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
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

    def xmm(self, n):
        return self.uc.reg_read(x.UC_X86_REG_XMM0 + n)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        stubs = RECORDED + SILENT + (VF58,)
        if address in (SIN, COS):
            v = from_bits(self.xmm(0) & 0xFFFFFFFF)
            if not (math.isfinite(v) and abs(v) >= 8192.0):
                return  # the real sinf/cosf runs (the port handles all but large finite arguments)
        elif address not in stubs:
            self.covered.add(address)
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        if address in SILENT:
            value = 0.0 if address == SIN0 else 1.0
            uc.reg_write(x.UC_X86_REG_XMM0, struct.unpack('<Q', struct.pack('<d', value))[0])
            uc.reg_write(x.UC_X86_REG_ESP, esp + 4)
            uc.reg_write(x.UC_X86_REG_EIP, back)
            return
        pop = 4 if address == VF58 else 0
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 4 + pop))]
        x0, x1 = self.xmm(0), self.xmm(1)
        q = 2 ** 64 - 1
        before = dict(regs=self.regs(), xmm0=x0 & 0xFFFFFFFF, xmm0q=x0 & q, xmm1q=x1 & q, memory=window)
        eax = uc.reg_read(x.UC_X86_REG_EAX)
        if address == VF58:
            eax = ROOT
        elif address == ATAN2:
            y = struct.unpack('<d', struct.pack('<Q', x0 & q))[0]
            xx = struct.unpack('<d', struct.pack('<Q', x1 & q))[0]
            uc.reg_write(x.UC_X86_REG_XMM0, struct.unpack('<Q', struct.pack('<d', math.atan2(y, xx)))[0])
        else:
            result = crt(address, from_bits(x0 & 0xFFFFFFFF))
            uc.reg_write(x.UC_X86_REG_XMM0, bits(result))
        uc.reg_write(x.UC_X86_REG_EAX, eax)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 4 + pop)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        x0 = self.xmm(0)
        self.events.append(dict(target=address, before=before,
                                after=dict(regs=self.regs(), xmm0=x0 & 0xFFFFFFFF, xmm0q=x0 & q, memory=[])))

    def put(self, address, values):
        self.uc.mem_write(address, b''.join(struct.pack('<f', v) for v in values))

    def quaternion(self, rng):
        mode = rng.random()
        if mode < 0.15:  # pitch of +-90 degrees about y: the gimbal cases of the Euler extraction
            s = math.sqrt(0.5) * rng.choice((1, -1))
            q = [0.0, s, 0.0, math.sqrt(0.5)]
            if rng.random() < 0.5:
                q[0] = rng.uniform(-1e-3, 1e-3)
            n = math.sqrt(sum(a * a for a in q))
            return [a / n for a in q]
        if mode < 0.3:
            return rng.choice(([0.0] * 4, [0, 0, 0, 1.0], [math.nan, 0, 0, 1.0], [0.5, 0.5, 0.5, 0.5], [1.0, 1.0, 1.0, 1.0],
                                [0, 1.0, 0, 1.0], [0, 2.0, 0, 2.0], [0, 0.7071068, 0, 0.7071068], [0, 0.70710683, 0, 0.70710683], [0, -1.0, 0, 1.0], [0.3, -1.0, 0.2, 1.0], [0.3, 1.0, 0.2, 1.0], [-0.4, -1.0, 0.1, 1.0], [-0.4, 1.0, 0.1, 1.0]))
        return random_quat(rng)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for base, size in ((THIS, 0x1000), (OUT, 0x1000), (MODEL, 0x2000), (JOINTS, 0x1000), (ROOT, 0x1000),
                           (SRC, 0x1000), (TABLE, 0x1000)):
            uc.mem_write(base, b'\0' * size)
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
        # the pose source's two angles (degrees)
        angles = (0.0, -0.0, 1e-9, 0.01, -0.01, 0.5, -0.5, 3.0, -3.0, 20.0, -20.0, 45.0, -45.0, 90.0, 179.0, 1000.0,
                  math.nan, math.inf, 9000.0, -90000.0)
        for off in (0x538, 0x53C):
            a = rng.choice(angles + (rng.uniform(-60, 60), rng.uniform(-60, 60), rng.uniform(-5, 5), 0.0, 0.0))
            uc.mem_write(SRC + off, struct.pack('<f', a))
        # the joint (OUT): local/world matrix at +0x10, rotation quaternion at +0x60,
        # parent index at +0x91, aim kind at +0x93
        matrix = transform_matrix(rng)
        tiny = rng.random() < 0.1  # an aim partner at a distance around the epsilon, from the origin
        if tiny:
            matrix[12:15] = [0.0, 0.0, 0.0]
        self.put(OUT + 0x10, matrix)
        self.put(OUT + 0x60, self.quaternion(rng))
        uc.mem_write(OUT + 0x91, bytes([rng.choice((0xFF, 0xFF, 0, 1, 2, 3))]))
        aim_kind = rng.choice((6, 10, 14, 18) * 4 + (0, 7, 100, 255))
        uc.mem_write(OUT + 0x93, bytes([aim_kind]))
        # the model: vtable, joint array, bone table, root matrix
        uc.mem_write(MODEL, struct.pack('<I', VTABLE))
        uc.mem_write(VTABLE + 0x58, struct.pack('<I', VF58))
        no_joints = rng.random() < 0.03
        if no_joints:  # without a joint array only the model's root can be a parent
            uc.mem_write(OUT + 0x91, bytes([0xFF]))
        uc.mem_write(MODEL + 0x364, struct.pack('<I', 0 if no_joints else JOINTS))
        uc.mem_write(MODEL + 0x368, struct.pack('<I', TABLE))
        for bone in range(256):
            uc.mem_write(TABLE + bone, bytes([rng.choice((0xFF, 0xFF, 0, 1, 2, 3))]))
        if no_joints:
            uc.mem_write(TABLE + ((aim_kind + 2) & 0xFF), bytes([rng.choice((0, 0xFF))]))
        elif rng.random() < 0.85:
            uc.mem_write(TABLE + ((aim_kind + 2) & 0xFF), bytes([rng.choice((0, 0, 1, 2, 3))]))
        for k in range(4):
            m = transform_matrix(rng)
            if rng.random() < 0.2:  # a partner at (nearly) the joint's own position
                m[12:15] = matrix[12:15] if rng.random() < 0.5 else [matrix[12], matrix[13], matrix[14] + rng.choice((0.0, 1e-9, 1e-3))]
            if tiny:
                m[12:15] = [0.0, 0.0, rng.choice((1e-7, 1.1e-7, 1.19e-7, 1.2e-7, 1.3e-7, 8e-8, 2e-7))]
            self.put(JOINTS + 0xA0 * k + 0x10, m)
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
        try:
            uc.emu_start(ENTRY, h.RET, count=800000)
        except Exception:
            print('seed', seed, 'eip', hex(uc.reg_read(x.UC_X86_REG_EIP)), [hex(r) for r in self.regs()])
            raise
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
        print(f"{len(starts) - len(missed)}/{len(starts)} run; never run outside 0x82ab89..0x82afaa:", [a for a in missed if not (0x82ab89 <= int(a,16) <= 0x82afaa)])
    # the CRT tables atan2/asinf read (private: never committed): the port's reciprocal
    # estimate is the host's, so it can read a different table entry than this run did
    table = h.code(0x0144C640, 0x0144DE60 - 0x0144C640)
    json.dump(dict(function='0x00872420', cases=cases, table_base=0x0144C640, table=list(table)), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')
    print(by_target)


main()
