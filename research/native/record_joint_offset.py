"""L184 s7 oracle: 0x00F2B5E0 `uCnsJointOffset::vf24(joint, model)` (thiscall,
`ret 8`, void) run as the original's code in Unicorn on synthetic inputs.

The function applies a joint-offset constraint: it builds a rotation matrix
from the constraint's Euler angles (six orders), takes the joint's matrix
into the space of a reference joint (the cofactor inverse of the reference
world matrix; identity when singular), splits it into scale / rotation /
translation, blends the rotation with the offset (quaternion nlerp) and the
scale/translation with the constraint's weight, and writes the joint matrix
back through the reference matrix. The CRT sinf/cosf (0x012F0906 /
0x012F0A55) run as original code (not stubbed); the only guest call is the
model's virtual method at vtable +0x58 (`ret 4`), recorded as a stub event
in the format of crates/mtcore/tests/semantic.rs `abi_cases_full`.

Angles are kept below 8192 radians (beyond that the CRT falls back to an
x87 routine, which the port calls rather than reimplements).
Reads the user's own exe; output is private (E:/ddda-re/notes/native-L184).
Run with E:/ddda-re/tools/pyenv/Scripts/python.exe.
"""
import json
import math
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, LENGTH = 0x00F2B5E0, 0x30C3  # through RET 0x8 at 0x00F2E6A0
STUB = 0x30000020  # the model's virtual method at vtable +0x58 (thiscall, one argument, ret 4)
THIS, JOINT, MODEL, VTABLE, REF, RECORDS = 0x20000000, 0x20001000, 0x20002000, 0x20003000, 0x20004000, 0x20010000
STACK, SP = 0x0F000000, 0x0F008000
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]
RECORD_COUNT = 6


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        _, secs = h.image()
        for va, b in secs:
            start = va & ~0xFFF
            size = ((va + len(b) - start) + 0xFFF) & ~0xFFF
            uc.mem_map(start, size)
            uc.mem_write(va, b)
        for address, size in ((THIS, 0x20000), (STUB & ~0xFFF, 0x1000), (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
        uc.mem_write(STUB, b'\xc3')
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

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
            return
        if address != STUB:
            return
        esp = uc.reg_read(x.UC_X86_REG_ESP)
        window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 8))]
        before = dict(regs=self.regs(), memory=window)
        back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
        uc.reg_write(x.UC_X86_REG_EAX, REF)
        uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
        uc.reg_write(x.UC_X86_REG_EIP, back)
        self.events.append(dict(target=STUB, before=before, after=dict(regs=self.regs(), memory=[])))

    # ---- input generation ----
    def angle(self, rng):
        r = rng.random()
        if r < 0.55:
            return rng.uniform(-180, 180)
        if r < 0.65:
            return rng.uniform(-720, 720)
        if r < 0.80:
            return rng.choice((0.0, -0.0, 90.0, -90.0, 180.0, -180.0, 45.0, 360.0, 270.0, 30.0, 11.25, 5.625))
        if r < 0.85:
            return rng.choice((1e-5, -1e-5, 1e-30, -1e-30, 1e-40, 3e-45, 0.001))
        if r < 0.92:
            return rng.uniform(-460000, 460000)
        # no inf/NaN inputs: Unicorn (QEMU) picks the second NaN when two different NaNs meet,
        # real SSE the first (crates/mtmath scalar.rs); the port pins the hardware rule
        return rng.uniform(-1000, 1000)

    def matrix(self, rng, kind=None):
        kind = kind or rng.choices(('rigid', 'ints', 'random', 'zero', 'degenerate'), (60, 12, 10, 3, 15))[0]
        if kind == 'zero':
            return [0.0] * 16
        if kind == 'random':
            return [rng.uniform(-5, 5) for _ in range(16)]
        if kind == 'ints':
            return [float(rng.choice((0, 0, 1, -1, 2))) for _ in range(16)]
        qx = [rng.gauss(0, 1) for _ in range(4)]
        n = math.sqrt(sum(v * v for v in qx)) or 1.0
        a, b, c, d = (v / n for v in qx)
        rot = [[1 - 2 * (c * c + d * d), 2 * (b * c - a * d), 2 * (b * d + a * c)],
               [2 * (b * c + a * d), 1 - 2 * (b * b + d * d), 2 * (c * d - a * b)],
               [2 * (b * d - a * c), 2 * (c * d + a * b), 1 - 2 * (b * b + c * c)]]
        scale = [rng.choice((1.0, 1.0, rng.uniform(0.3, 3.0))) for _ in range(3)]
        m = []
        for r in range(3):
            m += [rot[r][0] * scale[r], rot[r][1] * scale[r], rot[r][2] * scale[r], 0.0]
        m += [rng.uniform(-100, 100), rng.uniform(-100, 100), rng.uniform(-100, 100), 1.0]
        if kind == 'degenerate':
            r = rng.randrange(3)
            if rng.random() < 0.5:
                m[4 * r:4 * r + 4] = [0.0] * 4  # a zero row: det exactly 0 or a zero-length axis
            else:
                m[4 * r:4 * r + 4] = m[4 * ((r + 1) % 3):4 * ((r + 1) % 3) + 4]  # equal rows
        return m

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        uc.mem_write(THIS, bytes(rng.randrange(256) for _ in range(0x300)))
        uc.mem_write(JOINT, bytes(rng.randrange(256) for _ in range(0x100)))
        uc.mem_write(MODEL, bytes(rng.randrange(256) for _ in range(0x400)))
        uc.mem_write(STACK, b''.join(f32(rng.uniform(-2, 2)) for _ in range(0x4000)))  # stale stack words: finite floats (see angle())
        # this
        enabled = 0 if rng.random() < 0.06 else rng.choice((1, 1, 1, 2, 0xff))
        uc.mem_write(THIS + 0x61, bytes([enabled]))
        for k in range(3):
            uc.mem_write(THIS + 0x70 + 4 * k, f32(self.angle(rng)))
        order = rng.randrange(6) if rng.random() < 0.9 else rng.choice((6, 7, 9, 100, 0x80000000, 0xFFFFFFFF))
        uc.mem_write(THIS + 0xA0, struct.pack('<I', order))
        t = rng.choices(('unit', 'edge', 'wide', 'special'), (70, 15, 14, 1))[0]
        weight = {'unit': rng.random(), 'edge': rng.choice((0.0, 1.0, 0.5)), 'wide': rng.uniform(-0.5, 1.5),
                  'special': rng.choice((1e30, -1e30, 1e-40))}[t]
        uc.mem_write(THIS + 0x44, f32(weight))
        for off in (0x80, 0x84, 0x88):
            uc.mem_write(THIS + off, f32(rng.choice((1.0, 1.0, rng.uniform(0.2, 3.0), 0.0))))
        for off in (0x90, 0x94, 0x98):
            uc.mem_write(THIS + off, f32(rng.choice((0.0, rng.uniform(-10, 10)))))
        # joint
        index = 0xFF if rng.random() < 0.4 else rng.randrange(RECORD_COUNT)
        uc.mem_write(JOINT + 0x91, bytes([index]))
        uc.mem_write(JOINT + 0x10, b''.join(f32(v) for v in self.matrix(rng)))
        # model: vtable slot 0x58, joint array pointer at +0x364, the default reference matrix
        uc.mem_write(MODEL, struct.pack('<I', VTABLE))
        uc.mem_write(MODEL + 0x364, struct.pack('<I', RECORDS))
        uc.mem_write(VTABLE + 0x58, struct.pack('<I', STUB))
        kind = rng.choices((None, 'zero', 'ints'), (80, 8, 12))[0]
        uc.mem_write(REF, b''.join(f32(v) for v in self.matrix(rng, kind)))
        for i in range(RECORD_COUNT):
            kind = rng.choices((None, 'zero', 'ints'), (80, 8, 12))[0]
            uc.mem_write(RECORDS + 0xA0 * i + 0x10, b''.join(f32(v) for v in self.matrix(rng, kind)))
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 12 - 4 * rng.randrange(4)  # all four alignments of the entry stack
        uc.mem_write(esp, struct.pack('<III', h.RET, JOINT, MODEL))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4] = THIS, esp
        for r, v in zip(REGS, start):
            uc.reg_write(r, v)
        self.remember(esp, 12)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=200000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    calls = sum(len(c['events']) for c in cases)
    json.dump(dict(function='0x00F2B5E0', length=LENGTH, cases=cases), open(out, 'w'))
    print(f'{n} cases, {calls} calls -> {out}')


main()
