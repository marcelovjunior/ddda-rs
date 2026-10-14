"""L184 s5 oracle: 0x0086B2E0, mode 16 of uCnsEdit::vf24 (usercall: ECX =
model, EDI = joint, ESI = output buffer, stack argument `this`, `ret 4`).
It writes an identity matrix scaled row-wise by the constraint's scale
vector (+0x80) to the buffer, re-expresses the joint in the frame of its
parent bone (joint byte +0x91: a joint of the model, or 0xFF for the
matrix of the model's virtual +0x58(-1)) and adds the constraint's
translation (+0x90) carried through the parent. Its only guest call, the
virtual accessor, is a stub recorded as an event (format of
crates/mtcore/tests/semantic.rs). Run with
E:/ddda-re/tools/pyenv/Scripts/python.exe; output is private.
"""
import json
import random
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import harness as h  # noqa: E402
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_READ, UC_HOOK_MEM_WRITE  # noqa: E402
import unicorn.x86_const as x  # noqa: E402

ENTRY, END = 0x0086B2E0, 0x0086D17B
VSTUB = 0x30000058
THIS, JOINT, MODEL, VTABLE, JOINTS, PTRS = 0x20000000, 0x21000000, 0x22000000, 0x22000800, 0x23000000, 0x24000000
STACK, SP = 0x0F000000, 0x0F008000
OUT = SP + 0x400
REGS = [getattr(x, 'UC_X86_REG_' + n) for n in ('EAX', 'ECX', 'EDX', 'EBX', 'ESP', 'EBP', 'ESI', 'EDI')]


def f32(v):
    return struct.pack('<f', v)


class Oracle:
    def __init__(self):
        uc = self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        uc.mem_map(0x00860000, 0x10000)
        uc.mem_write(ENTRY, h.code(ENTRY, END - ENTRY))
        for page in (0x01521000, 0x01531000, 0x0161C000, 0x01629000):
            uc.mem_map(page, 0x1000)
            uc.mem_write(page, h.code(page, 0x1000))
        uc.mem_map(0x30000000, 0x1000)
        uc.mem_write(VSTUB, b'\xc3')
        for address, size in ((THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x1000), (JOINTS, 0x10000), (PTRS, 0x10000),
                              (STACK, 0x10000), (h.RET, 0x1000)):
            uc.mem_map(address, size)
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

    def matrix(self, kind=None):
        rng = self.rng
        kind = kind or rng.choice(('rand', 'rand', 'rand', 'rot', 'zero', 'identity', 'sparse'))
        if kind == 'zero':
            v = [0.0] * 16
        elif kind == 'identity':
            v = [1.0 if i % 5 == 0 else 0.0 for i in range(16)]
        elif kind == 'sparse':
            v = [rng.choice((0.0, 0.0, 1.0, -1.0, rng.uniform(-2, 2))) for _ in range(16)]
        elif kind == 'rot':
            import math
            a = rng.uniform(-3.2, 3.2)
            c, s = math.cos(a), math.sin(a)
            v = [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, rng.uniform(-5, 5), rng.uniform(-5, 5), rng.uniform(-5, 5), 1.0]
        else:
            v = [rng.choice((rng.uniform(-3, 3), rng.uniform(-3, 3), rng.uniform(-3, 3), 0.0, 1.0)) for _ in range(16)]
        if rng.random() < 0.01:  # a signalling NaN in the last element (the x87 load/store quiets it)
            return b''.join(f32(e) for e in v[:15]) + struct.pack('<I', 0x7fa00001)
        return b''.join(f32(e) for e in v)

    def code(self, uc, address, size, data):
        if address == h.RET:
            uc.emu_stop()
        elif address == VSTUB:
            esp = uc.reg_read(x.UC_X86_REG_ESP)
            window = [(esp + i, b) for i, b in enumerate(uc.mem_read(esp, 8))]
            before = dict(regs=self.regs(), memory=window)
            dest = PTRS + 0x40 * self.rng.randrange(64)
            effects = self.stub_write(dest, self.matrix())
            back = struct.unpack('<I', uc.mem_read(esp, 4))[0]
            uc.reg_write(x.UC_X86_REG_EAX, dest)
            uc.reg_write(x.UC_X86_REG_ESP, esp + 8)
            uc.reg_write(x.UC_X86_REG_EIP, back)
            self.events.append(dict(target=address, before=before, reads=[], after=dict(regs=self.regs(), memory=effects)))
        else:
            assert ENTRY <= address < END, hex(address)

    def case(self, seed):
        rng = self.rng = random.Random(seed)
        uc = self.uc
        for a, n in ((THIS, 0x1000), (JOINT, 0x1000), (MODEL, 0x1000), (JOINTS, 0x10000), (PTRS, 0x10000)):
            uc.mem_write(a, b'\0' * n)
        w = uc.mem_write
        r = rng.random()
        if r < 0.2:
            scale = (0.0, 0.0, 0.0)
        elif r < 0.3:
            scale = (1.0, 1.0, 1.0)
        elif r < 0.4:
            scale = tuple(rng.uniform(-6e-4, 6e-4) for _ in range(3))
        else:
            scale = tuple(rng.uniform(-2, 2) for _ in range(3))
        w(THIS + 0x80, b''.join(f32(v) for v in scale))
        w(THIS + 0x90, b''.join(f32(rng.choice((0.0, rng.uniform(-5, 5)))) for _ in range(3)))
        w(JOINT + 0x10, self.matrix())
        index = rng.choice((0xFF, 0xFF, rng.randrange(0, 256), rng.randrange(0, 4)))
        w(JOINT + 0x91, bytes([index]))
        w(MODEL, struct.pack('<I', VTABLE))
        w(MODEL + 0x364, struct.pack('<I', JOINTS))
        w(VTABLE + 0x58, struct.pack('<I', VSTUB))
        if index != 0xFF:
            w(JOINTS + index * 0xA0 + 0x10, self.matrix())
        self.initial, self.written, self.events = {}, set(), []
        esp = SP - 8 - 4 * rng.randrange(0, 64)
        uc.mem_write(esp, struct.pack('<II', h.RET, THIS))
        start = [rng.randrange(1 << 32) for _ in range(8)]
        start[1], start[4], start[6], start[7] = MODEL, esp, OUT + 0x40 * rng.randrange(4), JOINT
        for reg, v in zip(REGS, start):
            uc.reg_write(reg, v)
        self.remember(esp, 8)
        before = dict(regs=self.regs())
        uc.emu_start(ENTRY, h.RET, count=100000)
        after = dict(regs=self.regs(), memory=[(a, uc.mem_read(a, 1)[0]) for a in sorted(self.written)])
        return dict(seed=seed, mode=16, before=before, initial=sorted(self.initial.items()), events=self.events, after=after)


def main():
    out, n = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3000
    oracle = Oracle()
    cases = [oracle.case(seed) for seed in range(n)]
    json.dump(dict(function='0x0086B2E0', cases=cases), open(out, 'w'))
    print(f'{n} cases, {sum(len(c["events"]) for c in cases)} calls -> {out}')


main()
